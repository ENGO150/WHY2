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

pub fn encrypt_text(text: &String, key: &String) -> Data
{
    //CHECK FOR ACTIVE WHY2 VERSION
    misc::check_version();

    //CHECK FOR INVALID text
    if text.is_empty() { return Data::empty(ExitCode::InvalidText); }

    //GET key_used
    let key_used: String;
    if !key.is_empty()
    {
        //CHECK FOR INVALID [SHORT] key
        if options::get_core_options().key_length as usize > key.len() { return Data::empty(ExitCode::InvalidKey); }
        key_used = key.clone();
    } else
    {
        key_used = misc::generate_key(options::get_core_options().key_length);
    }

    Data::empty(ExitCode::Success)
}
