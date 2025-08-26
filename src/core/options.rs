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

use std::sync::RwLock;

use lazy_static::lazy_static;

//ENUMS
//THESE ARE LEGACY VERSIONS FOR GENERATING tkch, SO YOU CAN DECRYPT OLD TEXT
#[derive(Clone)]
pub enum Version
{
    V1, //FIRST VERSION. Replaced on May 28th 17:45:26 2022 UTC in commit 0d64f4fa7c37f0b57914db902258e279a71c7f9a.
    V2, //SECOND VERSION. Replaced on July 11th 17:12:41 2022 UTC in commit 0f01cde0f1e1a9210f4eef7b949e6d247072d3a6.
    V3, //THIRD VERSION. Replaced on Nov 17 19:55:13 2024 UTC in commit f917140ae54e4f5e601a089fbbea33817233e534.
    V4, //LATEST VERSION, MOST SECURE (how unexpected)
}

#[derive(Clone)]
pub enum OutputFormat
{
    Text, //HUMAN-READABLE FORMAT
    Byte, //NON HUMAN-READABLE LIGHTWEIGHT FORMAT
}

pub enum ExitCode //exit codes you fucking idiot
{
    Success = 0, //EXIT CODE FOR WHY2_SUCCESSFUL RUN
    InvalidKey = 1, //EXIT CODE FOR INVALID KEY
    InvalidText = 4, //EXIT CODE FOR INVALID TEXT
    DownloadFailed = 2, //EXIT CODE FOR versions.json DOWNLOAD FAIL
}

//STRUCTS
#[derive(Clone)]
pub struct Options
{
    pub no_check: bool, //SKIP CHECKING VERSION
    pub no_output: bool, //DO NOT PRINT OUTPUT
    pub version: Version, //VERSION OF tkch
    pub format: OutputFormat, //FORMAT OF output
    pub padding: u32, //HOW MANY PADDING CHARS TO ADD
}

pub struct Data
{
    pub output: String, //ENCRYPTED/DECRYPTED TEST
    pub key: String, //KEY USED FOR ENCRYPTION
    pub exit_code: ExitCode, //EXIT CODE
}

//IMPLEMENTATIONS
impl Options
{
    fn default() -> Options
    {
        Options
        {
            no_check: false,
            no_output: false,
            version: Version::V4,
            format: OutputFormat::Text,
            padding: 64,
        }
    }
}

lazy_static!
{
    static ref CORE_SETTINGS: RwLock<Options> = RwLock::new(Options::default());
}

pub fn set_core_options(options: Options)
{
    let mut settings = CORE_SETTINGS.write().unwrap();
    *settings = options;
}

pub fn get_core_options() -> Options
{
    let options = CORE_SETTINGS.read().unwrap();
    options.clone()
}
