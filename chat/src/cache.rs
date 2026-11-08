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

//! # Client Attachment Cache
//!
//! Every attachment (and voice clip) the client has seen, stored by content hash so a replayed or
//! reposted one needs no transfer at all.
//!
//! # Scoping
//! The cache is split per server, by the fingerprint of its identity
//! ([`options::get_fingerprint`]). Over one shared cache a server could name any hash and watch
//! whether the client fetches it — an oracle over every attachment seen anywhere. Keying on the
//! identity rather than the address also keeps a server's attachments when it moves.
//!
//! # At-Rest Protection
//! Each file is a [`RexStream`] ciphertext followed by an HMAC-SHA256 tag, under keys derived
//! from `fingerprint || hash` ([`crypto::cache_keys`]). The tag proves three things at once:
//! that this client wrote the file, that it is whole, and that it is the attachment its name
//! promises. A file that fails to verify is deleted, so a half-written or tampered file is
//! refilled on the next fetch instead of poisoning that hash forever.
//!
//! # Bound
//! The cache is capped at [`MAX_ATTACHMENT_CACHE`](consts::MAX_ATTACHMENT_CACHE). A write that exceeds
//! it evicts the least recently used files, by modification time; every hit touches its file.

use std::
{
    fs::FileTimes,
    time::SystemTime,
    path::{ Path, PathBuf },
    io::{ self, ErrorKind },
};

use tokio::
{
    fs::{ self, File },
    io::{ AsyncReadExt, AsyncWriteExt },
};

use sha2::{ Sha256, Digest };

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

    let path = misc::get_attachment_cache_dir(&fingerprint).join(misc::hex(hash));

    Some((fingerprint, path))
}

//DROP THE OLDEST FILES UNTIL THE CACHE FITS
/// Deletes the oldest files in a cache directory until it fits
/// [`MAX_ATTACHMENT_CACHE`](consts::MAX_ATTACHMENT_CACHE).
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

    if total <= consts::MAX_ATTACHMENT_CACHE { return; }

    files.sort_by_key(|(modified, ..)| *modified);

    for (_, size, path) in files
    {
        if total <= consts::MAX_ATTACHMENT_CACHE { break; }

        if fs::remove_file(&path).await.is_ok() { total = total.saturating_sub(size); }
    }
}

//SEAL ONE ATTACHMENT FOR DISK: ENCRYPT-THEN-MAC
/// Encrypts and authenticates one attachment for disk: `ciphertext || tag`.
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
//READ ONE CACHED ATTACHMENT BACK, VERIFYING IT
/// Reads one cached attachment back, verifying it.
///
/// A file that does not verify is deleted. A hit refreshes the file's modification time for the
/// eviction order.
///
/// # Parameters
/// - `hash`: The SHA-256 of the attachment.
///
/// # Returns
/// - `Some(bytes)` with the original attachment.
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

    let mut data = crypto::i64_to_bytes(&decrypted);
    data.truncate(ciphertext.len());

    //TOUCH THE FILE FOR THE EVICTION ORDER
    if let Ok(file) = std::fs::File::options().write(true).open(&path)
    {
        let _ = file.set_times(FileTimes::new().set_modified(SystemTime::now()));
    }

    Some(data)
}

//COPY A VERIFIED CACHED FILE WITHOUT KEEPING ITS PLAINTEXT IN MEMORY
/// Copies a cached attachment to disk using bounded buffers.
/// A missing or unauthenticated cache entry returns `None`; output errors are returned separately.
pub async fn copy(hash: &[u8; 32], target: &Path) -> Option<io::Result<u64>>
{
    //OPEN AND AUTHENTICATE THE SAME FILE THAT WILL BE DECRYPTED
    let (fingerprint, path) = scope(hash)?;
    let mut source = File::open(&path).await.ok()?;
    let (key, nonce, mac_key) = crypto::cache_keys(&fingerprint, hash);
    let size = match crypto::disk_verify(&mut source, &mac_key).await
    {
        Some(size) => size,
        None =>
        {
            let _ = fs::remove_file(&path).await;
            return None;
        },
    };

    let mut stream: RexStream = RexStream::new(&Grid::from_key(&key).ok()?, Grid::from_flat(&nonce).ok()?).ok()?;

    //KEEP A FAILED COPY FROM REPLACING AN EXISTING DOWNLOAD
    let temporary = target.with_extension(format!("{}.part", rand::random::<u64>()));

    let result = async
    {
        let mut output = File::create(&temporary).await?;
        let mut buffer = vec![0; consts::UPLOAD_CHUNK_SIZE];
        let mut remaining = size;
        let mut written = 0;
        let mut hasher = Sha256::new();

        //THE CACHE FINALIZES ITS CIPHER ONLY AT THE END OF THE WHOLE FILE
        while remaining > 0
        {
            let count = remaining.min(buffer.len() as u64) as usize;
            source.read_exact(&mut buffer[..count]).await?;
            remaining -= count as u64;

            let mut decrypted = stream.update(&crypto::bytes_to_i64(&buffer[..count]))
                .map_err(|_| io::Error::new(ErrorKind::InvalidData, "Cache decryption failed"))?;

            if remaining == 0
            {
                decrypted.extend(stream.finalize()
                    .map_err(|_| io::Error::new(ErrorKind::InvalidData, "Cache decryption failed"))?);
            }

            let mut bytes = crypto::i64_to_bytes(&decrypted);
            bytes.truncate((size - written) as usize);

            output.write_all(&bytes).await?;
            hasher.update(&bytes);
            written += bytes.len() as u64;
        }

        //VERIFY THE PLAINTEXT BEFORE NAMING THE DOWNLOAD
        let digest: [u8; 32] = hasher.finalize().into();

        if digest != *hash
        {
            return Err(io::Error::new(ErrorKind::InvalidData, "Cached file hash mismatch"));
        }

        output.flush().await?;
        drop(output);
        fs::rename(&temporary, target).await?;

        Ok(size)
    }.await;

    //REMOVE AN UNFINISHED COPY
    if result.is_err() { let _ = fs::remove_file(&temporary).await; }

    Some(result)
}

//SEAL AND STORE THE BYTES OFF THE WIRE
/// Seals and stores a attachment under its hash, then evicts if the cache is over its bound.
///
/// An existing file is never overwritten, since its name is its content. Failures are silent:
/// a attachment that did not make it into the cache is simply fetched again.
///
/// # Parameters
/// - `hash`: The SHA-256 of `data`; the caller is responsible for having verified it.
/// - `data`: The attachment bytes.
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
