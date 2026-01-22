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

//! Custom Key Encryption Example
//!
//! Demonstrates how to use your own pre-generated key for encryption.
//! This is useful when you need deterministic encryption or want to
//! store/share keys separately.
//!
//! Run with: cargo run --example custom_key

use why2::
{
    encrypter,
    decrypter,
    crypto,
    grid::Grid,
    types::EncryptedData,
};

fn main() -> Result<(), Box<dyn std::error::Error>>
{
    println!("=== WHY2 Custom Key Encryption ===\n");

    //GENERATE RANDOM KEY (YOU COULD ALSO LOAD THIS FROM A FILE)
    let custom_key = crypto::generate_key::<8, 8>();
    println!("Generated custom key ({} elements)", custom_key.len());

    let message = "Secret data encrypted with my own key!";
    println!("Original: {}\n", message);

    //ENCRYPT
    let encrypted = encrypter::encrypt_string::<8, 8>(message, Some(&custom_key))?;
    println!("✓ Encrypted with custom key");

    //DECRYPT
    let decrypted = decrypter::decrypt_string(encrypted)?;
    println!("✓ Decrypted: {}", *decrypted);

    //VERIFY
    assert_eq!(message, *decrypted);
    println!("\n✓ Success: Custom key works perfectly!");

    //TRY TO DECRYPT WITH INVALID KEY
    println!("\n--- Demonstrating key importance ---");

    let encrypted_again = encrypter::encrypt_string::<8, 8>(message, Some(&custom_key))?;
    let wrong_key = crypto::generate_key::<8, 8>(); //DIFFERENT KEY

    let wrong_decryption = decrypter::decrypt_string
    (
        EncryptedData
        {
            output: encrypted_again.output,
            key: Grid::from_key(&wrong_key)?,
            nonce: encrypted_again.nonce,
        }
    ).unwrap_or_else(|e| e.to_string().into());

    println!("With wrong key: {}", *wrong_decryption);
    println!("(Should error or be garbled/different from original)");

    Ok(())
}
