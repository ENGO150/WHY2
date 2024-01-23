/*
This is part of WHY2
Copyright (C) 2022 Václav Šmejkal

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
    fs::read_to_string,
    os::raw::c_char,

    ffi::
    {
        CString,
        CStr,
    },
};

#[no_mangle]
pub extern "C" fn why2_config_read(path: *const c_char, key: *const c_char) -> *mut c_char
{
    //CONVERT C STRINGS TO
    let path_r = unsafe { CStr::from_ptr(path).to_string_lossy().into_owned() };
    let key_r = unsafe { CStr::from_ptr(key).to_string_lossy().into_owned() };

    //GET FILE CONTENT
    let file_raw = match read_to_string(&path_r)
    {
        Ok(raw) => raw,
        Err(e) => panic!("Could not read TOML config: {}\n{}", path_r, e),
    };

    //PARSE FILE
    let data: toml::Value = match toml::from_str(&file_raw)
    {
        Ok(data) => data,
        Err(e) => panic!("Could not parse TOML config: {}\n{}", path_r, e),
    };

    //GET VALUE BY key_r
    let mut value = match data.get(&key_r)
    {
        Some(value) => value.to_string(),
        None => panic!("Key \"{}\" not found in TOML config: {}", key_r, path_r),
    };

    value = value.replace("\"", "").trim().to_string(); //REMOVE QUOTES

    CString::new(value).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn why2_config_read_free(s: *mut c_char) //BECAUSE THIS IS RUST MODULE I HAVE TO CREATE A DEALLOCATING FUNCTION
{
    unsafe //DON'T TRUST THIS, I DEFINITELY KNOW WHAT I'M DOING (idk)
    {
        if s.is_null() //bro
        {
            return;
        }

        drop(CString::from_raw(s)); //DROP THE PTR
    }
}