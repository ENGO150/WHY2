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

use std::sync::Arc;

use tokio::
{
    sync::Mutex,
    net::tcp::{ OwnedReadHalf, OwnedWriteHalf },
};

use zeroize::Zeroizing;

//CONSTS
pub const METADATA_URL: &str           = "https://crates.io/api/v1/crates/why2-chat"; //URL FOR PROJECT METADATA

pub const CONFIG_DIR: &str             = env!("WHY2_CONFIG_DIR");                     //DIRECTORY FOR CONFIG FILES
pub const UPLOADS_DIR: &str            = "WHY2-Uploads";                              //DIRECTORY FOR FILE UPLOADS
pub const SERVER_CONFIG: &str          = "/server.toml";                              //SERVER CONFIG FILE
pub const CLIENT_CONFIG: &str          = "/client.toml";                              //CLIENT CONFIG FILE
pub const SERVER_USERS_CONFIG: &str    = "/server_users.toml";                        //SERVER USERS CONFIG FILE

pub const SERVER_KEYS_CONFIG: &str     = "/server_keys.toml";                         //SERVER PUBKEY CONFIG FILE
pub const SERVER_KEYS_DIR: &str        = "/server_keys";                              //SERVER KEYS DIRECTORY
pub const SERVER_SKEY: &str            = "/private";                                  //SERVER PRIVATE KEY FILE
pub const SERVER_PKEY: &str            = "/public";                                   //SERVER PUBLIC KEY FILE
pub const SERVER_PQ_SKEY: &str         = "/private_pq";                               //SERVER POST-QUANTUM PRIVATE KEY FILE
pub const SERVER_PQ_PKEY: &str         = "/public_pq";                                //SERVER POST-QUANTUM PUBLIC KEY FILE

pub const FETCH_TIMEOUT: u64           = 5000;                                        //TIMOUT FOR FETCHING DATA (MS)
pub const CONNECT_TIMEOUT: u64         = 3000;                                        //TIMEOUT FOR DIALING A SERVER (MS)

pub const MAX_HANDSHAKES_PER_IP: usize = 8;                                           //MAX CONCURRENT HANDSKAKES PER IP
pub const REKEY_INTERVAL: u64          = 600;                                         //INTERVAL FOR RE-REKEYING (SECS)

pub const EVENT_CHANNEL_BOUND: usize   = 1024;                                        //CLIENT UI EVENT BUFFER

pub const SERVER_OWNER_ROLE: usize     = 2;                                           //OWNER
pub const SERVER_MODERATOR_ROLE: usize = 1;                                           //MODERATOR
pub const SERVER_USER_ROLE: usize      = 0;                                           //USER
pub const SERVER_RESTART_SETTINGS: &[&str] =                                          //SERVER SETTINGS THAT REQUIRE
    &["server_ip", "server_port", "enable_voice_chat", "server_username"];            //  RESTART TO BE APPLIED

//DO NOT CHANGE CONSTS BELOW UNLESS YOU ARE ABSOLUTELY SURE WHAT ARE YOU DOING
pub const MEGABYTE: usize              = 1_000_000;                                   //MEGABYTE DEFINITION
pub const UPLOAD_CHUNK_SIZE: usize     = MEGABYTE;                                    //FILE UPLOAD CHUNK (1MB)

pub const MAX_AUXILIARY_PACKET_SIZE: usize = UPLOAD_CHUNK_SIZE * 2;                   //FILE/SCREEN SIDE CHANNELS (2MB)
pub const MAX_PACKET_CEILING: usize        = 16 * MEGABYTE;                           //ABSOLUTE CEILING WHEN SPAM PROTECTION IS OFF (16MB)

//TYPES
pub type SharedKeys  = (Zeroizing<Vec<i64>>, Zeroizing<Vec<u8>>);           //WHY2 KEY, HMAC
pub type Streams<'a> = (&'a mut OwnedReadHalf, Arc<Mutex<OwnedWriteHalf>>); //READ STREAM, WRITE STREAM
