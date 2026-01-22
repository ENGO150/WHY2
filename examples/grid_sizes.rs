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

//! Grid Size Comparison Example
//!
//! Demonstrates how different grid dimensions affect encryption
//! performance and the number of grids generated.
//!
//! Run with: cargo run --example grid_sizes --release

use why2::{ encrypter, decrypter };
use std::time::Instant;

fn main() -> Result<(), Box<dyn std::error::Error>>
{
    println!("=== WHY2 Grid Size Comparison ===\n");

    let message = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(50);
    println!("Testing with {} characters\n", message.len());

    // Test different grid configurations
    println!("┌─────────────┬────────┬──────────────┬──────────────┐");
    println!("│ Grid Size   │ Grids  │ Encrypt (ms) │ Decrypt (ms) │");
    println!("├─────────────┼────────┼──────────────┼──────────────┤");

    test_grid_size::<8, 8>(&message, "8x8 (64)")?;
    test_grid_size::<11, 7>(&message, "11x7 (77)")?;
    test_grid_size::<16, 4>(&message, "16x4 (64)")?;
    test_grid_size::<13, 5>(&message, "13x5 (65)")?;
    test_grid_size::<10, 10>(&message, "10x10 (100)")?;

    println!("└─────────────┴────────┴──────────────┴──────────────┘");

    println!("\nNote: Grid size affects:");
    println!("  • Performance (larger grids = more computation per grid)");
    println!("  • Number of grids (smaller grids = more grids needed)");
    println!("  • Memory usage (total cells = grids × width × height)");

    Ok(())
}

fn test_grid_size<const W: usize, const H: usize
(
    message: &str,
    label: &str
) -> Result<(), Box<dyn std::error::Error>>
{
    //ENCRYPTION
    let start = Instant::now();
    let encrypted = encrypter::encrypt_string::<W, H>(message, None)?;
    let encrypt_time = start.elapsed().as_secs_f64() * 1000.0;

    let num_grids = encrypted.output.len();

    //DECRYPTION
    let start = Instant::now();
    let decrypted = decrypter::decrypt_string(encrypted)?;
    let decrypt_time = start.elapsed().as_secs_f64() * 1000.0;

    //VERIFY
    assert_eq!(message, *decrypted);

    println!("│ {:10} {}│ {:6} │ {:12.2} │  {:11.2} │",
        label,
        if W * H != 100 {" "} else {""}, //THIS IS JUST FOR FORMATTING
        num_grids, encrypt_time, decrypt_time);

    Ok(())
}
