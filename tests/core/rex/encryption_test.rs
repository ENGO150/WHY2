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

use why2::rex::
{
    encrypter,
    decrypter,
};

use crate::core as test_core;

//FUNCTIONS
#[test]
fn rex_encrypt_decrypt() -> Result<(), Box<dyn std::error::Error>>
{
    //START MEASURING
    let measure_start = Instant::now();

    //ENCRYPT & DECRYPT
    let encrypted = encrypter::encrypt_string::<11, 7>(&test_core::TEST_TEXT.to_owned(), None).expect("Encryption failed");
    let key = encrypted.key.clone();
    let encrypter_measure = measure_start.elapsed();
    let decrypted_string = decrypter::decrypt_string(encrypted).expect("Decryption failed");

    //STOP MEASURING
    let measure_stop = measure_start.elapsed();

    //VARIABLES FOR PRINT
    let mut stream: Box<dyn Write>;
    let status: &str;
    let returning: Result<(), Box<dyn std::error::Error>>;

    //GET VALUES BASED ON RESULT
    if test_core::TEST_TEXT == decrypted_string
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

    let measure_stop_nanos = measure_stop.as_nanos() as f64;
    let encrypter_measure_nanos = encrypter_measure.as_nanos() as f64;

    writeln!
    (
        stream,

        "Test {status}!\n\
        TEXT: \t\t\"{}\"\
        OUTPUT: \t\"{decrypted_string}\"\
        KEY: \t\t\n{}\
        TIME: \t\t{:.3}ms ({:.3}ms to encrypt [{}%])",

        test_core::TEST_TEXT, key,
        measure_stop_nanos / 1_000_000.,
        encrypter_measure_nanos / 1_000_000.,
        (encrypter_measure_nanos / measure_stop_nanos * 100.).round()
    ).unwrap();

    returning
}
