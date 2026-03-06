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

use std::
{
    net::TcpStream,
    sync::{ Arc, Mutex },
};

use zeroize::Zeroizing;

//CONSTS
pub const METADATA_URL: &str         = "https://crates.io/api/v1/crates/why2-chat"; //URL FOR PROJECT METADATA

pub const CONFIG_DIR: &str           = env!("WHY2_CONFIG_DIR");                     //DIRECTORY FOR CONFIG FILES
pub const UPLOADS_DIR: &str          = "WHY2-Uploads";                              //DIRECTORY FOR FILE UPLOADS
pub const SERVER_CONFIG: &str        = "/server.toml";                              //SERVER CONFIG FILE
pub const CLIENT_CONFIG: &str        = "/client.toml";                              //CLIENT CONFIG FILE
pub const SERVER_USERS_CONFIG: &str  = "/server_users.toml";                        //SERVER USERS CONFIG FILE

pub const SERVER_KEYS_CONFIG: &str   = "/server_keys.toml";                         //SERVER PUBKEY CONFIG FILE
pub const SERVER_KEYS_DIR: &str      = "/server_keys";                              //SERVER KEYS DIRECTORY
pub const SERVER_SKEY: &str          = "/private";                                  //SERVER PRIVATE KEY FILE
pub const SERVER_PKEY: &str          = "/public";                                   //SERVER PUBLIC KEY FILE
pub const SERVER_PQ_SKEY: &str       = "/private_pq";                               //SERVER POST-QUANTUM PRIVATE KEY FILE
pub const SERVER_PQ_PKEY: &str       = "/public_pq";                                //SERVER POST-QUANTUM PUBLIC KEY FILE

pub const FETCH_TIMEOUT: u64         = 5000;                                        //TIMOUT FOR FETCHING DATA (MS)

pub const REKEY_INTERVAL: u64        = 600;                                         //INTERVAL FOR RE-REKEYING (SECS)

//DO NOT CHANGE CONSTS BELOW UNLESS YOU ARE ABSOLUTELY SURE WHAT ARE YOU DOING
pub const UPLOAD_CHUNK_SIZE: usize   = 1_048_576;                                   //FILE UPLOAD CHUNK (1MB)

pub const OBFUSCATION_KEY: &[u8; 32] = //KEY FOR OBFUSCATING NON-ENCRYPTED PACKETS (NOT A SECURITY FEATURE)
&[
    0x9A, 0xF7, 0x1C, 0xD4, 0x62, 0x3E, 0x8B, 0x5A,
    0x0F, 0x2D, 0xE1, 0x79, 0x4C, 0xB8, 0x63, 0x90,
    0x21, 0x55, 0xAE, 0xD6, 0x04, 0x7F, 0x33, 0x82,
    0xBC, 0x19, 0x40, 0xE7, 0x95, 0x2A, 0x6B, 0xF8,
];

//TYPES
pub type SharedKeys  = (Zeroizing<Vec<i64>>, Zeroizing<Vec<u8>>);  //WHY2 KEY, HMAC
pub type Streams<'a> = (&'a mut TcpStream, Arc<Mutex<TcpStream>>); //READ STREAM, WRITE STREAM
