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

use std::
{
    error::Error,
    time::Instant,
};

use why2::{ encrypter, decrypter };

//===============================================
// TEST DATA - Different sizes for different tests
//===============================================

//SMALL - 1 GRID
const TEST_TEXT_SMALL: &str = "aAzZ(    )!?#\\/śŠ <3|420*;㍿㊓ㅅΔ♛👶🏿";

//MEDIUM - ~2-3 GRIDS
const TEST_TEXT_MEDIUM: &str = "\
Lorem ipsum dolor sit amet, consectetur adipiscing elit. Sed do eiusmod tempor \
incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute \
irure dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla \
pariatur 🔒🔐🛡️";

//LARGE - 10+ GRIDS FOR PARALLEL ENCRYPTION TEST
const TEST_TEXT_LARGE: &str = "\
The WHY2 encryption system represents a modern approach to cryptographic security, \
combining grid-based transformations with ARX (Add-Rotate-XOR) operations. Unlike \
traditional block ciphers that rely on S-boxes for nonlinear mixing, WHY2 employs \
a deterministic pseudo-random number generator seeded from the key hash to achieve \
diffusion across multiple rounds. Each grid undergoes a series of transformations: \
subcell mixing introduces nonlinear properties through Feistel-like networks, while \
shift_rows and mix_columns provide linear diffusion horizontally and vertically. \
The mix_matrix operation applies an affine transformation by treating the grid as \
a matrix and multiplying it with a lower-triangular and upper-triangular key-dependent \
matrix. This ensures that changes in any input bit cascade throughout the entire grid. \
Mix_diagonals adds another layer of diffusion by XORing cells along diagonal lines, \
further obscuring patterns. The CTR mode of operation enables parallel encryption of \
multiple blocks, where each plaintext grid is XORed with a keystream block derived \
from an incrementing counter. This design allows for efficient parallelization using \
modern CPU features like SIMD instructions or frameworks like Rayon. Security analysis \
through diffusion tests confirms that both input and key changes produce approximately \
50% bit flips in the ciphertext, indicating strong avalanche properties. The constant-time \
implementation prevents timing attacks by ensuring cryptographic operations take the same \
amount of time regardless of input values. HMAC authentication protects against tampering, \
while HKDF derives separate encryption and MAC keys from the shared secret. 🚀🔐✨🌟💎🛡️🔒🎯";

//===============================================
// MAIN TEST - combines all subtests
//===============================================

#[test]
fn encrypt_decrypt() -> Result<(), Box<dyn Error>>
{
    //PART 1: ENCRYPTION TESTS
    println!("\n=== REX Encryption Tests ===\n");
    println!("┌──────────┬────────────┬──────────┬───────────┬───────────┬───────────┬────────┐");
    println!("│ Size     │ Input      │ Grids    │ Encrypt   │ Decrypt   │ Total     │ Status │");
    println!("├──────────┼────────────┼──────────┼───────────┼───────────┼───────────┼────────┤");

    test_encryption(TEST_TEXT_SMALL, "SMALL")?;
    test_encryption(TEST_TEXT_MEDIUM, "MEDIUM")?;
    test_encryption(TEST_TEXT_LARGE, "LARGE")?;

    println!("└──────────┴────────────┴──────────┴───────────┴───────────┴───────────┴────────┘\n");

    //PART 2: GRID SIZE COMPARISON
    let text = TEST_TEXT_LARGE.to_owned();
    
    println!("=== Grid Size Comparison ({} chars) ===\n", text.len());
    println!("┌───────────┬────────┬───────────┐");
    println!("│ Grid      │ Grids  │ Time      │");
    println!("├───────────┼────────┼───────────┤");

    //8x8 GRIDS (64 CELLS)
    let start = Instant::now();
    let enc_8x8 = encrypter::encrypt_string::<8, 8>(&text, None)?;
    let time_8x8 = start.elapsed();
    println!("│ 8x8  (64) │ {:6} │ {:7.2}ms │", enc_8x8.output.len(), time_8x8.as_secs_f64() * 1000.0);

    //11x7 GRIDS (77 CELLS)
    let start = Instant::now();
    let enc_11x7 = encrypter::encrypt_string::<11, 7>(&text, None)?;
    let time_11x7 = start.elapsed();
    println!("│ 11x7 (77) │ {:6} │ {:7.2}ms │", enc_11x7.output.len(), time_11x7.as_secs_f64() * 1000.0);

    //16x4 GRIDS (64 CELLS)
    let start = Instant::now();
    let enc_16x4 = encrypter::encrypt_string::<16, 4>(&text, None)?;
    let time_16x4 = start.elapsed();
    println!("│ 16x4 (64) │ {:6} │ {:7.2}ms │", enc_16x4.output.len(), time_16x4.as_secs_f64() * 1000.0);

    println!("└───────────┴────────┴───────────┘\n");

    Ok(())
}

//===============================================
// HELPER FUNCTION
//===============================================

fn test_encryption(text: &str, label: &str) -> Result<(), Box<dyn Error>>
{
    //START MEASURE
    let measure_start = Instant::now();

    //ENCRYPT & DECRYPT
    let encrypted = encrypter::encrypt_string::<11, 7>(&text.to_owned(), None)?;

    let num_grids = encrypted.output.len();
    let encrypter_measure = measure_start.elapsed();

    let decrypted_string = decrypter::decrypt_string(encrypted)?;

    //STOP MEASURE
    let measure_stop = measure_start.elapsed();

    //VERIFY
    let status = if text == *decrypted_string { "✓" } else { "✗" };

    let total_ms = measure_stop.as_secs_f64() * 1000.0;
    let encrypt_ms = encrypter_measure.as_secs_f64() * 1000.0;
    let decrypt_ms = total_ms - encrypt_ms;

    println!
    (
        "│ {:8} │ {:4} chars │ {:2} grids │ {:7.2}ms │ {:7.2}ms │ {:7.2}ms │   {}    │",
        label, text.chars().count(), num_grids,
        encrypt_ms, decrypt_ms, total_ms, status
    );

    if text != *decrypted_string
    {
        Err("Values do not match".into())
    } else
    {
        Ok(())
    }
}

//===============================================
// VERIFICATION TEST
//===============================================

#[test]
fn verify_multi_grid_overflow()
{
    let encrypted = encrypter::encrypt_string::<11, 7>(&TEST_TEXT_LARGE.to_owned(), None)
        .expect("Encryption failed");

    assert!
    (
        encrypted.output.len() >= 3,
        "Large text should overflow into at least 3 grids, got {}",
        encrypted.output.len()
    );
}
