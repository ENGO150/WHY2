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

use argon2::
{
    Argon2,
    PasswordHasher,
    PasswordVerifier,
};

pub fn hash_password(password: &str) -> String //HASH PASSWORD USING ARGON2
{
    //HASH
    Argon2::default().hash_password(password.as_bytes()).unwrap().to_string()
}

pub fn compare_password_hash(hashed: &str, password: &str) -> bool //COMPARE ARGON2 HASH WITH UNHASHED PASSWORD
{
    //COMPARE (PARSES THE PHC STRING ITSELF)
    Argon2::default().verify_password(password.as_bytes(), hashed).is_ok()
}
