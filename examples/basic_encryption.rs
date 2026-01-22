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

//! Basic String Encryption Example
//!
//! This example demonstrates the simplest use case of WHY2:
//! encrypting and decrypting a text string with automatic key generation.
//!
//! Run with: cargo run --example basic_encryption

use why2::{ encrypter, decrypter };

fn main() -> Result<(), Box<dyn std::error::Error>>
{
    println!("=== WHY2 Basic String Encryption ===\n");

    //ORIGINAL MESSAGE
    let message = "Hello, World! This is a secret message. 🔒";
    println!("Original message: {}", message);

    //ENCRYPT WITH A NEWLY GENERATED SECURE KEY
    let encrypted = encrypter::encrypt_string::<8, 8>(message, None)?;

    println!("\n✓ Encrypted successfully!");
    println!("  - Number of encrypted grids: {}", encrypted.output.len());
    println!("  - Key (first 32 bytes): {:x}", encrypted.key);

    //DECRYPT
    let decrypted = decrypter::decrypt_string(encrypted)?;

    println!("\n✓ Decrypted successfully!");
    println!("  Decrypted message: {}", *decrypted);

    //VERIFY
    assert_eq!(message, *decrypted);
    println!("\n✓ Verification passed: Messages match!");

    Ok(())
}
