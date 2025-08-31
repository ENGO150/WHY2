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

//CONSTS
pub const SERVER_CONFIG: &str = "/server.toml";                                                                //SERVER CONFIG FILE
pub const CLIENT_CONFIG: &str = "/client.toml";                                                                //CLIENT CONFIG FILE
pub const SERVER_USERS_CONFIG: &str = "/server_users.toml";                                                    //SERVER USERS CONFIG FILE
pub const CONFIG_URL: &str    = "https://raw.githubusercontent.com/ENGO150/WHY2/development/src/chat/configs"; //CONFIG FILE DOWNLOAD URL

pub const AUTHORITY_DIR: &str = "/certs";                                                                      //AUTHORITY DIRECTORY

pub const KEY_LOCATION: &str = "/keys";                                                                        //KEY DIRECTORY
pub const KEY_FILENAME: &str = "/secp521r1.pem";                                                               //NAME OF ECC KEYFILE
