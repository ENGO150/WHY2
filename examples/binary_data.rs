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

//! Binary Data Encryption Example
//!
//! Shows how to encrypt raw binary data (Vec<i64>) instead of strings.
//! This is useful for encrypting structured data, serialized objects,
//! or any non-text content.
//!
//! IMPORTANT: WHY2 pads data to fill complete grids. You must track the
//! original data length separately to recover the exact original data.
//!
//! Run with: cargo run --example binary_data

use why2::
{
    encrypter,
    decrypter,
    crypto,
};

fn main() -> Result<(), Box<dyn std::error::Error>>
{
    println!("=== WHY2 Binary Data Encryption ===\n");

    //EXAMPLE: ENCRYPTING NUMERIC DATA
    let data: Vec<i64> = vec!
    [
        1234567890,
        -9876543210,
        42,
        0,
        i64::MAX,
        i64::MIN,
    ];

    println!("Original data: {:?}", data);

    let key = crypto::generate_key::<8, 8>();

    //ENCRYPT THE RAW i64 VECTOR
    let encrypted = encrypter::encrypt::<8, 8>(&data, Some(&key))?;

    println!("\n✓ Encrypted {} values into {} grids", data.len(), encrypted.output.len());

    //SHOW ENCRYPTED GRID (FIRST FEW VALUES)
    println!("  First encrypted grid:");
    for (i, row) in encrypted.output[0].iter().take(2).enumerate()
    {
        println!("    Row {}: {:?}...", i, &row[..3]);
    }

    //DECRYPT
    let decrypted = decrypter::decrypt(encrypted)?;

    //TRIM PADDING
    let decrypted_trimmed = &decrypted.output[..data.len()];

    println!("\n✓ Decrypted data: {:?}", decrypted_trimmed);

    //VERIFY
    assert_eq!(data, decrypted_trimmed);
    println!("\n✓ Verification passed!");

    //EXAMPLE 2: ENCRYPTING LARGER DATASET
    println!("\n--- Large Dataset Example ---");

    let large_data: Vec<i64> = (0..1000).map(|i| i * 13 + 7).collect();
    let encrypted_large = encrypter::encrypt::<11, 7>(&large_data, None)?;

    println!("Encrypted {} values into {} grids (11x7)", large_data.len(), encrypted_large.output.len());

    let decrypted_large = decrypter::decrypt(encrypted_large)?;

    //TRIM PADDING
    let decrypted_large_trimmed = &decrypted_large.output[..large_data.len()];

    assert_eq!(large_data, decrypted_large_trimmed);
    println!("✓ Large dataset verified!");

    //EXAMPLE 3: STORING ORIGINAL LENGTH WITH ENCRYTPED DATA
    println!("\n--- Proper Length Handling ---");

    let data3: Vec<i64> = vec![ 100, 200, 300 ];
    let original_len = data3.len();

    //ENCRYPT
    let encrypted3 = encrypter::encrypt::<8, 8>(&data3, None)?;
    println!("Original length: {}", original_len);
    println!("Encrypted grids: {}", encrypted3.output.len());

    //STORE LENGTH SEPARATELY (PREPEND TO ENCRYPTED DATA IN PRACTICE)
    let decrypted3 = decrypter::decrypt(encrypted3)?;
    let recovered = &decrypted3.output[..original_len];

    assert_eq!(data3, recovered);
    println!("✓ Recovered correct data using stored length");

    Ok(())
}
