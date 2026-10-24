/*
This is part of WHY2
Copyright (C) 2022-2026 Václav Šmejkal

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
*/

//! # Client Image Cache
//!
//! Every picture (and voice clip) the client has seen, stored by content hash so a replayed or
//! reposted one needs no transfer at all.
//!
//! # Scoping
//! The cache is split per server, by the fingerprint of its identity
//! ([`options::get_fingerprint`]). Over one shared cache a server could name any hash and watch
//! whether the client fetches it — an oracle over every picture seen anywhere. Keying on the
//! identity rather than the address also keeps a server's pictures when it moves.
//!
//! # At-Rest Protection
//! Each file is a [`RexStream`] ciphertext followed by an HMAC-SHA256 tag, under keys derived
//! from `fingerprint || hash` ([`crypto::cache_keys`]). The tag proves three things at once:
//! that this client wrote the file, that it is whole, and that it is the picture its name
//! promises. A file that fails to verify is deleted, so a half-written or tampered file is
//! refilled on the next fetch instead of poisoning that hash forever.
//!
//! # Bound
//! The cache is capped at [`MAX_IMAGE_CACHE`](consts::MAX_IMAGE_CACHE). A write that exceeds
//! it evicts the least recently used files, by modification time; every hit touches its file.

use std::
{
    fs::FileTimes,
    time::SystemTime,
    path::PathBuf,
};

use tokio::fs;

use why2::
{
    grid::Grid,
    stream::RexStream,
};

use crate::
{
    misc,
    crypto,
    consts,
    options,
};

//PRIVATE
//RESOLVE WHICH SERVER'S CACHE A HASH BELONGS TO
/// Returns the server fingerprint and cache path for a hash, or `None` before a handshake.
fn scope(hash: &[u8; 32]) -> Option<(String, PathBuf)>
{
    let fingerprint = options::get_fingerprint();

    if fingerprint.is_empty() { return None; }

    let path = misc::get_image_cache_dir(&fingerprint).join(misc::hex(hash));

    Some((fingerprint, path))
}

//DROP THE OLDEST FILES UNTIL THE CACHE FITS
/// Deletes the oldest files in a cache directory until it fits
/// [`MAX_IMAGE_CACHE`](consts::MAX_IMAGE_CACHE).
async fn evict(directory: &PathBuf)
{
    let Ok(mut entries) = fs::read_dir(directory).await else { return };

    let mut files: Vec<(SystemTime, u64, PathBuf)> = Vec::new();
    let mut total: u64 = 0;

    while let Ok(Some(entry)) = entries.next_entry().await
    {
        let Ok(metadata) = entry.metadata().await else { continue };
        if !metadata.is_file() { continue; }

        let modified = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);

        total += metadata.len();
        files.push((modified, metadata.len(), entry.path()));
    }

    if total <= consts::MAX_IMAGE_CACHE { return; }

    files.sort_by_key(|(modified, ..)| *modified);

    for (_, size, path) in files
    {
        if total <= consts::MAX_IMAGE_CACHE { break; }

        if fs::remove_file(&path).await.is_ok() { total = total.saturating_sub(size); }
    }
}

//SEAL ONE PICTURE FOR DISK: ENCRYPT-THEN-MAC
/// Encrypts and authenticates one picture for disk: `ciphertext || tag`.
fn seal(fingerprint: &str, hash: &[u8; 32], data: &[u8]) -> Option<Vec<u8>>
{
    let (key, nonce, mac_key) = crypto::cache_keys(fingerprint, hash);

    let mut disk_stream: RexStream = RexStream::new(&Grid::from_key(&key).ok()?, Grid::from_flat(&nonce).ok()?).ok()?;

    let mut sealed = disk_stream.update(&crypto::bytes_to_i64(data)).ok()?;
    sealed.extend(disk_stream.finalize().ok()?);

    let mut bytes = crypto::i64_to_bytes(&sealed);
    bytes.truncate(data.len());

    //AUTHENTICATE
    let tag = crypto::disk_seal(&mac_key, &bytes);
    bytes.extend_from_slice(&tag);

    Some(bytes)
}

//PUBLIC
//READ ONE CACHED PICTURE BACK, VERIFYING IT
/// Reads one cached picture back, verifying it.
///
/// A file that does not verify is deleted. A hit refreshes the file's modification time for the
/// eviction order.
///
/// # Parameters
/// - `hash`: The SHA-256 of the picture.
///
/// # Returns
/// - `Some(bytes)` with the original picture.
/// - `None` on a miss, before a handshake, or if the file failed verification.
pub async fn load(hash: &[u8; 32]) -> Option<Vec<u8>>
{
    let (fingerprint, path) = scope(hash)?;

    let sealed = fs::read(&path).await.ok()?;

    let (key, nonce, mac_key) = crypto::cache_keys(&fingerprint, hash);

    //DELETE A FILE THAT DOES NOT VERIFY
    let Some(ciphertext) = crypto::disk_open(&mac_key, &sealed) else
    {
        let _ = fs::remove_file(&path).await;

        return None;
    };

    let mut disk_stream: RexStream = RexStream::new(&Grid::from_key(&key).ok()?, Grid::from_flat(&nonce).ok()?).ok()?;

    let mut decrypted = disk_stream.update(&crypto::bytes_to_i64(ciphertext)).ok()?;
    decrypted.extend(disk_stream.finalize().ok()?);

    let mut image = crypto::i64_to_bytes(&decrypted);
    image.truncate(ciphertext.len());

    //TOUCH THE FILE FOR THE EVICTION ORDER
    if let Ok(file) = std::fs::File::options().write(true).open(&path)
    {
        let _ = file.set_times(FileTimes::new().set_modified(SystemTime::now()));
    }

    Some(image)
}

//SEAL AND STORE THE BYTES OFF THE WIRE
/// Seals and stores a picture under its hash, then evicts if the cache is over its bound.
///
/// An existing file is never overwritten, since its name is its content. Failures are silent:
/// a picture that did not make it into the cache is simply fetched again.
///
/// # Parameters
/// - `hash`: The SHA-256 of `data`; the caller is responsible for having verified it.
/// - `data`: The picture bytes.
pub async fn store(hash: &[u8; 32], data: &[u8])
{
    let Some((fingerprint, path)) = scope(hash) else { return };

    //NOTHING IS OVERWRITTEN - THE NAME IS THE CONTENT
    if fs::try_exists(&path).await.unwrap_or(false) { return; }

    let Some(directory) = path.parent().map(PathBuf::from) else { return };

    if fs::create_dir_all(&directory).await.is_err() { return; }

    let Some(bytes) = seal(&fingerprint, hash, data) else { return };

    if fs::write(&path, &bytes).await.is_err() { return; }

    evict(&directory).await;
}
