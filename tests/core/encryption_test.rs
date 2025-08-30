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

use why2::core::
{
    crypto,
    encrypter,
    decrypter,
    options::{ self, Options },
};

//CONSTS
const TEST_TEXT: &str = "aAzZ(    )!?#\\/śŠ <3|420*;㍿㊓ㅅΔ♛👶🏿";  //TEST TEXT FOR ENCRYPTION

//TEST XOR IN ENCRYPTION
fn encryption_operation(x: i64, y: i64) -> i64
{
    x ^ y
}

#[test]
fn encrypt_decrypt()
{
    //OPTIONS
    options::set_core_options
    (
        Options
        {
            key_length: 100, //USE 2x LARGER KEY
            padding: crypto::recommended_padding_rate(TEST_TEXT.len()), //USE RECOMMENDED PADDING
            encryption_operation: encryption_operation, //USE XOR FOR ENCRYPTING
            ..Options::default() //DEFAULT OTHER SETTINGS
        }
    );

    //START MEASURING
    let measure_start = Instant::now();

    //ENCRYPT & DECRYPT
    let encrypted = encrypter::encrypt_text(TEST_TEXT, None);
    let decrypted = decrypter::decrypt_text(encrypted);

    //STOP MEASURING
    let measure_stop = measure_start.elapsed();

    //OUTPUT VARIABLES
    let decrypted_text = decrypted.output.expect("Decryption failed");
    let key = decrypted.key.expect("Decryption failed");

    //VARIABLES FOR PRINT
    let mut stream: Box<dyn Write>;
    let status: &str;

    if TEST_TEXT == decrypted_text
    {
        stream = Box::new(io::stdout());
        status = "successful";
    } else
    {
        stream = Box::new(io::stderr());
        status = "failed";
    }

    writeln!
    (
        stream,

        "Test {status}!\n
TEXT: \t\t\"{TEST_TEXT}\"
OUTPUT: \t\"{decrypted_text}\"
KEY: \t\t\"{key}\"
TIME: \t\t{}ms",

        measure_stop.as_millis()
    ).unwrap();
}
