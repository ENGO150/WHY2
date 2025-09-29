/*
This is part of WHY2
Copyright (C) 2022-2025 Václav Šmejkal

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

use std::sync::{ LazyLock, RwLock };

//CONSTS
pub const USER_CONFIG_DIR: &str = "{HOME}/.config";                       //USER CONFIG DIRECTORY
pub const CONFIG_DIR: &str      = "/WHY2";                                //DIRECTORY FOR CONFIG FILES

//ENUMS
//THESE ARE LEGACY VERSIONS FOR GENERATING tkch, SO YOU CAN DECRYPT OLD TEXT
#[derive(Clone, PartialEq)]
pub enum Version
{
    V1, //FIRST VERSION. Replaced on May 28th 17:45:26 2022 UTC in commit 0d64f4fa7c37f0b57914db902258e279a71c7f9a.
    V2, //SECOND VERSION. Replaced on July 11th 17:12:41 2022 UTC in commit 0f01cde0f1e1a9210f4eef7b949e6d247072d3a6.
    V3, //THIRD VERSION. Replaced on Nov 17 19:55:13 2024 UTC in commit f917140ae54e4f5e601a089fbbea33817233e534.
    V4, //LATEST VERSION, MOST SECURE (how unexpected)
}

//STRUCTS
#[derive(Clone)]
pub struct Options
{
    pub key_length: usize,                         //LENGTH OF SYMMETRIC KEY
    pub version: Version,                          //VERSION OF tkch
    pub padding: usize,                            //HOW MANY PADDING CHARS TO ADD
    pub encryption_operation: fn(i64, i64) -> i64, //ENCRYPTION OPERATION CLOSURE
}

pub struct EncryptedData
{
    pub output: Vec<i64>, //ENCRYPTED TEXT
    pub key: String,      //KEY USED FOR ENCRYPTION
}

pub struct DecryptedData
{
    pub output: String, //DECRYPTED DATA
    pub key: String,    //KEY USED FOR ENCRYPTION
}

//IMPLEMENTATIONS
impl Default for Options
{
    fn default() -> Self
    {
        Self
        {
            key_length: 50,
            version: Version::V4,
            padding: 64,
            encryption_operation: |a, b| a - b,
        }
    }
}

//SETTINGS
static CORE_SETTINGS: LazyLock<RwLock<Options>> = LazyLock::new(||
{
    RwLock::new(Options::default())
});

//FUNCTIONS
//CORE SETTINGS
pub fn set_core_options(options: Options) //OVERWRITE DEFAULT SETTINGS
{
    let mut settings = CORE_SETTINGS.write().unwrap();
    *settings = options;
}

pub fn get_core_options() -> Options //RETURN SETTINGS
{
    let options = CORE_SETTINGS.read().unwrap();
    options.clone()
}
