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

use why2::encrypter;

use std::collections::HashMap;

#[test]
fn test_ciphertext_entropy()
{
    //LONG LOW-ENTROPY TEXT
    let low_entropy_input = "A".repeat(10_000);

    //ENCRYPT
    let encrypted = encrypter::encrypt_string::<8, 8>(&low_entropy_input, None)
        .expect("Encryption failed");

    //ANALYZE ENTROPY
    let mut byte_counts: HashMap<u8, usize> = HashMap::new();
    let mut total_bytes = 0;

    for grid in encrypted.output.iter()
    {
        for row in grid.iter()
        {
            for &val in row
            {
                for byte in val.to_be_bytes()
                {
                    *byte_counts.entry(byte).or_insert(0) += 1;
                    total_bytes += 1;
                }
            }
        }
    }

    //CALCULATE SHANNON ENTROPY
    let mut entropy = 0.0;
    for &count in byte_counts.values()
    {
        let p = count as f64 / total_bytes as f64;
        entropy -= p * p.log2();
    }

    println!("Ciphertext Entropy: {:.4} bits/byte", entropy);

    //RANDOM DATA SHOULD BE AROUND 8
    assert!
    (
        entropy > 7.95,
        "Cipher text entropy too low! ({:.4})",
        entropy
    );
}
