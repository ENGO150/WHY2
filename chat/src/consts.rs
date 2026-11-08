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

//! # Shared Constants
//!
//! Protocol limits, file names, timeouts and type aliases shared by the client and the server.
//!
//! Paths are relative fragments appended to [`misc::get_why2_dir`](crate::misc::get_why2_dir),
//! which is why most of them start with a `/`.
//!
//! # Stability
//! The constants below the `DO NOT CHANGE` marker are part of the wire protocol or of the
//! on-disk format: changing one silently breaks compatibility with every peer and every stored
//! file built against the old value.

use std::
{
    sync::Arc,
    time::Duration,
};

use tokio::
{
    sync::Mutex,
    net::tcp::{ OwnedReadHalf, OwnedWriteHalf },
};

use zeroize::Zeroizing;

use wincode::config::Configuration;

//CONSTS
/// crates.io API endpoint the version check reads the newest release from.
pub const METADATA_URL: &str           = "https://crates.io/api/v1/crates/why2-chat"; //URL FOR PROJECT METADATA

/// Configuration directory, baked in at build time; `{HOME}` is expanded at run time.
pub const CONFIG_DIR: &str             = env!("WHY2_CONFIG_DIR");                     //DIRECTORY FOR CONFIG FILES
/// Per-user temp directory name the server stages uploads in.
pub const UPLOADS_DIR: &str            = "WHY2-Uploads";                              //DIRECTORY FOR FILE UPLOADS
/// Server configuration file.
pub const SERVER_CONFIG: &str          = "/server.toml";                              //SERVER CONFIG FILE
/// Client configuration file.
pub const CLIENT_CONFIG: &str          = "/client.toml";                              //CLIENT CONFIG FILE
/// Server account store (credentials, roles, colors, profiles).
pub const SERVER_USERS_CONFIG: &str    = "/server_users.toml";                        //SERVER USERS CONFIG FILE
/// Server ban list.
pub const SERVER_BANS_CONFIG: &str     = "/server_bans.toml";                         //SERVER BANS CONFIG FILE
/// Encrypted lobby message history (`config::messages`).
pub const SERVER_MESSAGES_FILE: &str   = "/server_messages.bin";                      //SERVER MESSAGE HISTORY FILE
/// Directory holding sealed persistent attachments and voice messages.
/// Retains its legacy path so existing files remain readable.
pub const SERVER_ATTACHMENTS_DIR: &str = "/server_images";                            //PERSISTENT ATTACHMENT DIRECTORY
/// File holding the hash of the server icon.
pub const SERVER_ICON_FILE: &str       = "/server_icon";                              //SERVER ICON HASH FILE
/// Client attachment cache directory, one subdirectory per server fingerprint.
pub const CLIENT_ATTACHMENTS_DIR: &str = "/client_images";                            //CACHED ATTACHMENT DIRECTORY
/// Directory searched for user-supplied translations.
pub const LOCALES_DIR: &str            = "/locales";                                  //USER TRANSLATION DIRECTORY

/// Client store of pinned (TOFU) server public keys.
pub const SERVER_KEYS_CONFIG: &str     = "/server_keys.toml";                         //SERVER PUBKEY CONFIG FILE
/// Directory holding the server's identity keypair.
pub const SERVER_KEYS_DIR: &str        = "/server_keys";                              //SERVER KEYS DIRECTORY

/// At-rest key of the message history.
pub const SERVER_HISTORY_KEY: &str     = "/server_history_key";                       //MESSAGE HISTORY AT-REST KEY FILE
/// At-rest key of the persistent attachment store.
pub const SERVER_ATTACHMENT_KEY: &str  = "/server_image_key";                         //PERSISTENT ATTACHMENT KEY FILE
/// At-rest key of the client attachment cache.
pub const CLIENT_CACHE_KEY: &str       = "/client_cache_key";                         //CACHED ATTACHMENT AT-REST KEY FILE
/// Server private key file, inside [`SERVER_KEYS_DIR`].
pub const SERVER_SKEY: &str            = "/private";                                  //SERVER PRIVATE KEY FILE
/// Server public key file, inside [`SERVER_KEYS_DIR`].
pub const SERVER_PKEY: &str            = "/public";                                   //SERVER PUBLIC KEY FILE

/// Size of a SEC1 uncompressed NIST P-521 public point.
pub const ECC_PUBKEY_SIZE: usize       = 133;                                         //SEC1 UNCOMPRESSED NIST P-521 POINT
/// Size of a fixed-width ECDSA/P-521 signature (`r || s`).
pub const ECC_SIGNATURE_SIZE: usize    = 132;                                         //FIXED-SIZE ECDSA/P-521 SIGNATURE (r || s)
/// Size of an ML-KEM-768 encapsulation key.
pub const PQ_PUBKEY_SIZE: usize        = 1184;                                        //ML-KEM-768 ENCAPSULATION KEY
/// Size of an ML-KEM-768 ciphertext.
pub const PQ_CIPHERTEXT_SIZE: usize    = 1088;                                        //ML-KEM-768 CIPHERTEXT
/// Domain separation label for the signed handshake transcript.
pub const KEX_CONTEXT: &[u8]           = b"WHY2-KEX-V1";                              //DOMAIN SEPARATION FOR THE SIGNED HANDSHAKE TRANSCRIPT

/// Timeout for HTTP fetches, in milliseconds.
pub const FETCH_TIMEOUT: u64           = 5000;                                        //TIMOUT FOR FETCHING DATA (MS)
/// Timeout for dialing a server, in milliseconds.
pub const CONNECT_TIMEOUT: u64         = 3000;                                        //TIMEOUT FOR DIALING A SERVER (MS)

/// Maximum concurrent unauthenticated handshakes from one IP address.
pub const MAX_HANDSHAKES_PER_IP: usize = 8;                                           //MAX CONCURRENT HANDSKAKES PER IP
/// Interval between session rekeys, in seconds.
pub const REKEY_INTERVAL: u64          = 600;                                         //INTERVAL FOR RE-REKEYING (SECS)

/// Attempts at binding the listener before the server gives up.
pub const BIND_ATTEMPTS: usize         = 15;                                          //BINDS BEFORE GIVING UP (~3 SECONDS OF THEM)
/// Delay between bind attempts, in milliseconds.
pub const BIND_RETRY_DELAY: u64        = 200;                                         //MS BETWEEN THEM
/// Delay after a failing `accept()`, in milliseconds.
pub const ACCEPT_RETRY_DELAY: u64      = 100;                                         //MS TO WAIT OUT A FAILING accept()

/// Capacity of the channel carrying client events to the UI.
pub const EVENT_CHANNEL_BOUND: usize   = 1024;                                        //CLIENT UI EVENT BUFFER

/// `server.toml` keys only read at startup; a change to one applies after a restart.
pub const SERVER_RESTART_SETTINGS: &[&str] =                                          //SERVER SETTINGS THAT REQUIRE
    &["server_ip", "server_port", "enable_voice_chat", "server_username",             //RESTART TO BE APPLIED
      "log_level"];

//DO NOT CHANGE CONSTS BELOW UNLESS YOU ARE ABSOLUTELY SURE WHAT ARE YOU DOING
/// Decimal megabyte (10^6 bytes), not a mebibyte.
pub const MEGABYTE: usize              = 1_000_000;                                   //MEGABYTE DEFINITION
/// Chunk size of file uploads and of at-rest file encryption.
///
/// Readers of a sealed file must break it at exactly these boundaries: a chunk is not a whole
/// number of REX grids, so a single pass over the file falls off the keystream:
///
/// $$ \frac{10^6\ \text{bytes}}{8 \times 8 \times 8\ \text{bytes per grid}} = 1953.125\ \text{grids} $$
pub const UPLOAD_CHUNK_SIZE: usize     = MEGABYTE;                                    //FILE UPLOAD CHUNK (1MB)
/// Bytes of a file needed to recognise its image format.
pub const IMAGE_HEADER_SIZE: usize     = 16;                                          //BYTES AN IMAGE IS RECOGNISED BY
/// Largest attachment the server accepts and passes on.
pub const MAX_ATTACHMENT_SIZE: usize   = 8 * MEGABYTE;                                //BIGGEST ATTACHMENT THE SERVER PASSES ON (8MB)
/// Largest avatar or server icon the server keeps.
pub const MAX_AVATAR_SIZE: usize       = 2 * MEGABYTE;                                //BIGGEST AVATAR THE SERVER KEEPS (2MB)
/// Side of the square a still avatar is cut to, in pixels.
pub const AVATAR_DIMENSION: u32        = 512;                                         //SIDE OF THE SQUARE AN AVATAR IS CUT TO
/// Side of the square an animated avatar is cut to, in pixels.
pub const ANIMATED_AVATAR_DIMENSION: u32 = 256;                                       //AND OF AN ANIMATED ONE
/// Temp file name prefix of a cut avatar awaiting upload.
pub const AVATAR_TEMP_PREFIX: &str     = "why2_avatar_";                              //TEMP FILE A CUT AVATAR IS UPLOADED FROM
/// GIF quantiser speed for animated avatars (1 best, 30 fastest).
pub const AVATAR_GIF_SPEED: i32        = 10;                                          //GIF QUANTISER SPEED (1 BEST, 30 FASTEST)
/// Largest voice message the server keeps.
pub const MAX_VOICE_MESSAGE_SIZE: usize = 6 * MEGABYTE;                               //BIGGEST VOICE MESSAGE THE SERVER KEEPS (6MB)
/// Temp file name prefix of a recorded voice message awaiting upload.
pub const VOICE_TEMP_PREFIX: &str      = "why2_voice_";                               //TEMP FILE A VOICE MESSAGE IS UPLOADED FROM
/// Largest history page the server sends in one packet.
pub const MAX_HISTORY_SIZE: usize      = 12 * MEGABYTE;                               //BIGGEST HISTORY PAGE THE SERVER SENDS (12MB)
/// Widest or tallest picture a client agrees to decode, in pixels.
pub const MAX_IMAGE_DIMENSION: u32     = 16_384;                                      //WIDEST/TALLEST PICTURE A CLIENT DECODES

/// Size of the HMAC-SHA256 tag trailing an encrypted file.
pub const DISK_TAG_SIZE: usize         = 32;                                          //HMAC-SHA256 TAG ON AN ENCRYPTED FILE

/// Most disk space the client attachment cache may take.
pub const MAX_ATTACHMENT_CACHE: u64    = 512 * MEGABYTE as u64;                       //MOST DISK THE ATTACHMENT CACHE MAY TAKE
/// Most memory a single image decode may allocate.
pub const MAX_IMAGE_ALLOC: u64         = 256 * MEGABYTE as u64;                       //MOST MEMORY ONE DECODE MAY TAKE (256MB)

/// Most frames of one animation that are kept.
pub const MAX_ANIMATION_FRAMES: usize  = 512;                                         //FRAMES OF ONE ANIMATION THAT ARE KEPT
/// Most memory all frames of one animation may take.
pub const MAX_ANIMATION_ALLOC: u64     = 64 * MEGABYTE as u64;                        //MOST MEMORY ONE ANIMATION MAY TAKE (64MB)

/// Shortest frame delay honoured; anything shorter means "as fast as possible".
pub const MIN_FRAME_DELAY: Duration        = Duration::from_millis(20);               //FASTEST A FRAME MAY ASK TO BE HELD
/// Delay given to a frame that asks for less than [`MIN_FRAME_DELAY`].
pub const DEFAULT_FRAME_DELAY: Duration    = Duration::from_millis(100);              //WHAT A FRAME ASKING FOR LESS GETS

/// Minimum spacing of one client's attachment fetches on the server.
pub const ATTACHMENT_REQUEST_DELAY: Duration = Duration::from_millis(500);             //SPACING OF ONE CLIENT'S ATTACHMENT FETCHES

/// How often a typing client restates that it is typing.
pub const TYPING_INTERVAL: Duration        = Duration::from_secs(3);                  //HOW OFTEN A WRITING CLIENT RESTATES IT
/// How long a receiver believes a typing notice; twice [`TYPING_INTERVAL`].
pub const TYPING_TIMEOUT: Duration         = Duration::from_secs(6);                  //HOW LONG A RECEIVER BELIEVES IT

/// Largest packet accepted on the file and screen side channels.
pub const MAX_AUXILIARY_PACKET_SIZE: usize = UPLOAD_CHUNK_SIZE * 2;                   //FILE/SCREEN SIDE CHANNELS (2MB)
/// Absolute packet size ceiling, applied even with spam protection off.
pub const MAX_PACKET_CEILING: usize        = 16 * MEGABYTE;                           //ABSOLUTE CEILING WHEN SPAM PROTECTION IS OFF (16MB)

/// `wincode` configuration type used for every packet.
pub type PacketConfig = Configuration<true, MAX_PACKET_CEILING>;

/// `wincode` configuration with preallocation bounded by [`MAX_PACKET_CEILING`].
pub const PACKET_CONFIG: PacketConfig =
    Configuration::default().with_preallocation_size_limit::<MAX_PACKET_CEILING>();

//TYPES
/// Session keys: the `why2` grid key and the HMAC-SHA256 key.
pub type SharedKeys  = (Zeroizing<Vec<i64>>, Zeroizing<Vec<u8>>);           //WHY2 KEY, HMAC
/// At-rest keys of one sealed file: the `why2` grid key, the nonce and the HMAC key.
pub type DiskKeys    = (Zeroizing<Vec<i64>>, Vec<i64>, Zeroizing<[u8; 32]>); //WHY2 KEY, NONCE, HMAC
/// The read half of a connection and its shared write half.
pub type Streams<'a> = (&'a mut OwnedReadHalf, Arc<Mutex<OwnedWriteHalf>>); //READ STREAM, WRITE STREAM
