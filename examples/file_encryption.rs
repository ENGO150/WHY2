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

//! File Encryption Example
//!
//! Demonstrates encrypting and decrypting text files using WHY2.
//! Creates a temporary file, encrypts it, and then decrypts it back.
//!
//! Run with: cargo run --example file_encryption

use why2::
{
    encrypter,
    decrypter,
    crypto,
    types::EncryptedData,
    grid::Grid,
};

use std::
{
    fs,
    io::Write,
};

fn main() -> Result<(), Box<dyn std::error::Error>>
{
    println!("=== WHY2 File Encryption ===\n");

    //CREATE TEST FILES
    let input_file = "test_input.txt";
    let encrypted_file = "test_encrypted.bin";
    let decrypted_file = "test_decrypted.txt";
    let key_file = "test_key.bin";

    let original_content = "This is a secret document.\n\
                           It contains multiple lines.\n\
                           And should be encrypted securely! 🔐";

    println!("Creating test file...");
    fs::write(input_file, original_content)?;
    println!("✓ Created: {}", input_file);

    //STEP 1: READ FILE
    let content = fs::read_to_string(input_file)?;
    println!("\n✓ Read {} bytes", content.len());

    //STEP 2: ENCRYPT
    println!("\nEncrypting...");
    let key = crypto::generate_key::<8, 8>();
    let encrypted = encrypter::encrypt_string::<8, 8>(&content, Some(&key))?;
    println!("✓ Encrypted into {} grids", encrypted.output.len());

    //STEP 3: SAVE ENCRYPTED DATA
    let mut enc_file = fs::File::create(encrypted_file)?;

    //WRITE NONCE
    for row in encrypted.nonce.iter()
    {
        for &val in row
        {
            enc_file.write_all(&val.to_be_bytes())?;
        }
    }

    //WRITE ENCRYPTED GRIDS
    for grid in &encrypted.output
    {
        for row in grid.iter()
        {
            for &val in row
            {
                enc_file.write_all(&val.to_be_bytes())?;
            }
        }
    }

    println!("✓ Saved encrypted data to: {}", encrypted_file);

    //STEP 4: SAVE KEY SEPARATELY
    let key_bytes: Vec<u8> = key.iter()
        .flat_map(|&v| v.to_be_bytes())
        .collect();
    fs::write(key_file, key_bytes)?;
    println!("✓ Saved key to: {}", key_file);

    // --- SIMULATE LOADING FROM DISK ---
    println!("\n--- Decryption Process ---");

    //STEP 5: LOAD KEY
    let key_bytes = fs::read(key_file)?;
    let loaded_key: Vec<i64> = key_bytes
        .chunks(8)
        .map(|chunk|
        {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(chunk);
            i64::from_be_bytes(bytes)
        })
        .collect();
    println!("✓ Loaded key ({} elements)", loaded_key.len());

    //STEP 6: LOAD ENCRYPTED DATA
    let enc_bytes = fs::read(encrypted_file)?;
    let grids = Grid::<8, 8>::from_bytes(&enc_bytes)?;
    println!("✓ Loaded {} grids", grids.len());

    //STEP 7: DECRYPT
    let decrypted = decrypter::decrypt_string(EncryptedData
    {
        output: grids[1..].to_vec(), //SKIP NONCE
        key: Grid::from_key(&loaded_key)?,
        nonce: grids[0].clone(),
    })?;
    println!("✓ Decrypted");

    //STEP 8: SAVE DECRYPTED CONTENT
    fs::write(decrypted_file, &*decrypted)?;
    println!("✓ Saved to: {}", decrypted_file);

    //VERIFY
    assert_eq!(original_content, *decrypted);
    println!("\n✓ Success! Content verified.");

    //CLEANUP - IS COMMENTED OUT SO YOU CAN VIEW THE FILES
    /*
    println!("\nCleaning up temporary files...");
    fs::remove_file(input_file)?;
    fs::remove_file(encrypted_file)?;
    fs::remove_file(decrypted_file)?;
    fs::remove_file(key_file)?;
    println!("✓ Done");
    */

    Ok(())
}
