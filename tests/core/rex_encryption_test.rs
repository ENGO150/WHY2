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
    time::Instant,
    io::{ self, Write },
};

use why2::core::rex::
{
    encrypter,
    decrypter,
};

//CONSTS
const TEST_TEXT: &str = "aAzZ(    )!?#\\/śŠ <3|420*;㍿㊓ㅅΔ♛👶🏿";  //TEST TEXT FOR ENCRYPTION

//FUNCTIONS
#[test]
fn rex_encrypt_decrypt() -> Result<(), Box<dyn std::error::Error>>
{
    //START MEASURING
    let measure_start = Instant::now();

    //ENCRYPT & DECRYPT
    let encrypted = encrypter::encrypt_string(&TEST_TEXT.to_owned(), None).expect("Encryption failed");
    let key = encrypted.key.clone();
    let decrypted_string = decrypter::decrypt_string(encrypted);

    //STOP MEASURING
    let measure_stop = measure_start.elapsed();

    //VARIABLES FOR PRINT
    let mut stream: Box<dyn Write>;
    let status: &str;
    let returning: Result<(), Box<dyn std::error::Error>>;

    //GET VALUES BASED ON RESULT
    if TEST_TEXT == decrypted_string
    {
        stream = Box::new(io::stdout());
        status = "successful";
        returning = Ok(());
    } else
    {
        stream = Box::new(io::stderr());
        status = "failed";
        returning = Err("Values do not match".into());
    }

    writeln!
    (
        stream,

        "Test {status}!\n
TEXT: \t\t\"{TEST_TEXT}\"
OUTPUT: \t\"{decrypted_string}\"
KEY: \t\t\"{:?}\"
TIME: \t\t{}ms",

        key, measure_stop.as_millis()
    ).unwrap();

    returning
}
