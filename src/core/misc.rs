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

use std::
{
    str,
    fs,
    path::Path,
};

use crate::core::options;

//PRIVATE
fn __get_dir(dir: &str) -> String
{
    dir.replace("{HOME}", dirs::home_dir().expect("Could not determine home directory").to_str().expect("Invalid home directory"))
}

fn get_config_dir() -> String
{
    __get_dir(options::USER_CONFIG_DIR)
}

pub fn check_directory() //CREATE WHY2 CONFIG DIRECTORY
{
    let config = get_config_dir() + options::CONFIG_DIR;

    //CREATE WHY2 CONFIG DIRECTORY
    if !Path::new(&config).is_dir()
    {
        fs::create_dir_all(config).expect("Failed to create WHY2 config directory");
    }
}

pub fn get_why2_dir() -> String //RETURN PATH TO WHY2 CONFIG DIRECTORY
{
    get_config_dir() + options::CONFIG_DIR
}
