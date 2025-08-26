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
    env,
    fs,

    io::Write,
    path::Path,
    fs::File,
};

use curl::easy::Easy;

use crate::core::options;
use crate::core::options::ExitCode;

pub fn check_version() -> ExitCode
{
    if options::get_core_options().no_check { return ExitCode::Success; }

    check_directory(); //MAKE SURE WHY2 DIR EXISTS

    let mut easy = Easy::new();
    easy.url(options::VERSIONS_URL).expect("Invalid URL");
    easy.write_function(|data|
    {
        File::create(options::VERSIONS_FILE.replace("{HOME}", env::home_dir().expect("Could not determine home directory").to_str().expect("Invalid home directory"))).expect("Failed to create versions.json").write_all(data).expect("Failed to write versions.json");
        Ok(data.len())
    }).expect("Saving versions.json failed");
    easy.perform().expect("Downloading versions.json failed");

    ExitCode::Success
}

pub fn check_directory()
{
    let config = options::USER_CONFIG_DIR.replace("{HOME}", env::home_dir().expect("Could not determine home directory").to_str().expect("Invalid home directory"));

    if !Path::new(&(config.clone() + options::CONFIG_DIR)).is_dir() { fs::create_dir_all(config + options::CONFIG_DIR).expect("Failed to create WHY2 config directory"); } //CREATE WHY2 CONFIG DIRECTORY
}
