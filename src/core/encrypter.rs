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

use crate::core::
{
    misc,
    options,
    options::{ ExitCode, Data },
};

pub fn encrypt_text(text: &str, key: &str) -> Data
{
    //CHECK FOR ACTIVE WHY2 VERSION
    misc::check_version();

    //CHECK FOR INVALID text
    if text.is_empty() { return Data::empty(ExitCode::InvalidText); }

    let core_options = options::get_core_options(); //CORE OPTIONS

    //GET key_used
    let key_used = if !key.is_empty() //key WAS PASSED TO FUNCTION
    {
        //CHECK FOR INVALID [SHORT] key
        if key.len() < core_options.key_length { return Data::empty(ExitCode::InvalidKey); }

        key.to_owned()
    } else //NO key, GENERATE ONE
    {
        misc::generate_key(core_options.key_length)
    };

    Data::empty(ExitCode::Success)
}
