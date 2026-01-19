/*
This is part of WHY2
Copyright (C) 2022-2026 Václav Šmejkal

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

use p521::elliptic_curve::rand_core::OsRng;

use argon2::
{
    Argon2,
    PasswordHasher,
    PasswordVerifier,
    password_hash::{ PasswordHash, SaltString },
};

pub fn hash_password(password: &str) -> String //HASH PASSWORD USING ARGON2
{
    //GENERATE RANDOM SALT
    let salt = SaltString::generate(&mut OsRng);

    //HASH
    Argon2::default().hash_password(password.as_bytes(), &salt).unwrap().to_string()
}

pub fn compare_password_hash(hashed: &str, password: &str) -> bool //COMPARE ARGON2 HASH WITH UNHASHED PASSWORD
{
    //PARSE HASH STRING
    let parsed_hash = PasswordHash::new(hashed).unwrap();

    //COMPARE
    Argon2::default().verify_password(password.as_bytes(), &parsed_hash).is_ok()
}
