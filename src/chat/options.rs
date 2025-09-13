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

use once_cell::sync::Lazy;

use termios::Termios;

use crate::core::options::{ self, Options };

//CONSTS
pub const SERVER_PORT: u16          = 1204;                                                                          //PORT FOR SERVER COMMUNICATION
pub const SERVER_CONFIG: &str       = "/server.toml";                                                                //SERVER CONFIG FILE
pub const CLIENT_CONFIG: &str       = "/client.toml";                                                                //CLIENT CONFIG FILE
pub const SERVER_USERS_CONFIG: &str = "/server_users.toml";                                                          //SERVER USERS CONFIG FILE
pub const CONFIG_URL: &str          = "https://raw.githubusercontent.com/ENGO150/WHY2/development/src/chat/configs"; //CONFIG FILE DOWNLOAD URL

pub const AUTHORITY_DIR: &str       = "/certs";                                                                      //AUTHORITY DIRECTORY

pub const KEY_LOCATION: &str        = "/keys";                                                                       //KEY DIRECTORY
pub const KEY_FILENAME: &str        = "/secp521r1.pem";                                                              //NAME OF ECC KEYFILE

pub const MIN_PASSWORD_LEN: usize   = 8;                                                                             //MINIMAL PASSWORD LENGTH

//SETTINGS
static SHARED_KEY: Lazy<RwLock<Option<String>>> = Lazy::new(|| //SHARED SYMMETRIC KEY
{
    RwLock::new(None)
});

static ASKING_PASSWORD: Lazy<RwLock<bool>> = Lazy::new(|| //SHARED SYMMETRIC KEY
{
    RwLock::new(false)
});

//FUNCTIONS
//SHARED SYM KEY
pub fn set_shared_key(key: String) //SET KEY
{
    let mut shared_key = SHARED_KEY.write().unwrap();
    *shared_key = Some(key);
}

pub fn get_shared_key() -> Option<String> //RETURN KEY
{
    let shared_key = SHARED_KEY.read().unwrap();
    shared_key.clone()
}

//ASKING PASSWORD
pub fn set_asking_password(value: bool) //SET ASKING_PASSWORD
{
    //GET STDIN ATTRS
    let mut termios = Termios::from_fd(0).expect("Failed getting stdin attrs");

    if value //DISABLE ECHO
    {
        termios.c_lflag &= !termios::ECHO;
    } else //ENABLE ECHO
    {
        termios.c_lflag |= termios::ECHO;
    }

    //SAVE ATTRS
    termios::tcsetattr(0, termios::TCSANOW, &termios).expect("Failed setting stdin attrs");

    let mut asking_password = ASKING_PASSWORD.write().unwrap();
    *asking_password = value;
}

pub fn get_asking_password() -> bool //GET ASKING_PASSWORD
{
    let asking_password = ASKING_PASSWORD.read().unwrap();
    asking_password.clone()
}

//CORE ENCRYPTION OPTIONS
pub fn set_core_options()
{
    options::set_core_options
    (
        Options
        {
            no_check: true,
            ..Options::default()
        }
    );
}
