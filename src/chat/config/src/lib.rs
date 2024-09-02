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
    os::raw::c_char,

    fs::
    {
        read_to_string,
        write,
    },
    ffi::
    {
        CString,
        CStr,
    },
};

use toml::Value;

fn toml_read(path_r: String, key_r: String) -> String
{
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
    match data.get(&key_r)
    {
        Some(value) => value.to_string().replace("\"", "").trim().to_string(), //TRIM AND SHIT
        None => panic!("Key \"{}\" not found in TOML config: {}", key_r, path_r),
    }
}

#[no_mangle]
pub extern "C" fn why2_toml_read(path: *const c_char, key: *const c_char) -> *mut c_char
{
    //CONVERT C STRINGS TO RUST STRINGS
    let path_r = unsafe { CStr::from_ptr(path).to_string_lossy().into_owned() };
    let key_r = unsafe { CStr::from_ptr(key).to_string_lossy().into_owned() };

    CString::new(toml_read(path_r, key_r)).unwrap().into_raw() //GET
}

#[no_mangle]
pub extern "C" fn why2_toml_write(path: *const c_char, key: *const c_char, value: *const c_char)
{
    //CONVERT C STRINGS TO RUST STRINGS
    let path_r = unsafe { CStr::from_ptr(path).to_string_lossy().into_owned() };
    let key_r = unsafe { CStr::from_ptr(key).to_string_lossy().into_owned() };
    let value_r = unsafe { CStr::from_ptr(value).to_string_lossy().into_owned() };

    //GET FILE CONTENT
    let file_raw = match read_to_string(&path_r)
    {
        Ok(raw) => raw,
        Err(e) =>
        {
            eprintln!("Could not read TOML config: {}\n{}", path_r, e);
            return;
        },
    };

    //PARSE FILE
    let mut data: Value = match toml::from_str(&file_raw)
    {
        Ok(data) => data,
        Err(e) =>
        {
            eprintln!("Could not parse TOML config: {}\n{}", path_r, e);
            return;
        },
    };

    //INSERT VALUE (OR UPDATE)
    if let Some(table) = data.as_table_mut()
    {
        table.insert(key_r, Value::String(value_r));
    } else
    {
        eprintln!("Failed to get TOML table from file: {}", path_r);
        return;
    }

    //CONVERT NEW DATA TO STRING
    let updated_data = match toml::to_string(&data)
    {
        Ok(data) => data,
        Err(e) =>
        {
            eprintln!("Failed to convert TOML data to string: {}\n{}", path_r, e);
            return;
        },
    };

    //WRITE NEW DATA
    if let Err(e) = write(&path_r, updated_data)
    {
        eprintln!("Could not write to TOML config: {}\n{}", path_r, e);
    }
}

#[no_mangle]
pub extern "C" fn why2_toml_contains(path: *const c_char, key: *const c_char) -> bool
{
    //CONVERT C STRINGS TO RUST STRINGS
    let path_r = unsafe { CStr::from_ptr(path).to_string_lossy().into_owned() };
    let key_r = unsafe { CStr::from_ptr(key).to_string_lossy().into_owned() };

    //GET FILE CONTENT
    let file_raw = match read_to_string(&path_r)
    {
        Ok(raw) => raw,
        Err(e) =>
        {
            eprintln!("Could not read TOML config: {}\n{}", path_r, e);
            return false;
        },
    };

    //PARSE FILE
    let data: Value = match toml::from_str(&file_raw)
    {
        Ok(data) => data,
        Err(e) =>
        {
            eprintln!("Could not parse TOML config: {}\n{}", path_r, e);
            return false;
        },
    };

    data.get(&key_r).is_some()
}

#[no_mangle]
pub extern "C" fn why2_toml_equals(path: *const c_char, key: *const c_char, value: *const c_char) -> bool
{
    //CONVERT C STRINGS TO RUST STRINGS
    let path_r = unsafe { CStr::from_ptr(path).to_string_lossy().into_owned() };
    let key_r = unsafe { CStr::from_ptr(key).to_string_lossy().into_owned() };
    let value_r = unsafe { CStr::from_ptr(value).to_string_lossy().into_owned() };

    toml_read(path_r, key_r) == value_r //RESULT
}

#[no_mangle]
pub extern "C" fn why2_toml_read_free(s: *mut c_char) //BECAUSE THIS IS RUST MODULE I HAVE TO CREATE A DEALLOCATING FUNCTION
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