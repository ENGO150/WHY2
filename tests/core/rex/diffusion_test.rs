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

// i would prefer to not answer questions about this test. it works. how? no idea

use why2::rex::
{
    encrypter,
    crypto,
    Grid,
};

use std::io::{ self, Write };

use rand::Rng;

//GRID SIZE
const W: usize = 8;
const H: usize = 8;

fn calculate_hamming_distance(g1: &[Grid<W, H>], g2: &[Grid<W, H>]) -> (usize, usize) //CALCULATES HAMMING DISTANCE
{
    let mut diff_bits = 0;
    let mut total_bits = 0;

    for (grid_a, grid_b) in g1.iter().zip(g2.iter())
    {
        for (cell_a, cell_b) in grid_a.clone().into_iter().zip(grid_b.clone().into_iter())
        {
            diff_bits += (cell_a ^ cell_b).count_ones() as usize;
            total_bits += 64; //(i64)
        }
    }

    (diff_bits, total_bits)
}

fn generate_random_data(len: usize) -> Vec<i64> //GENERATE RANDOM VECTOR OF i64
{
    let mut rng = rand::rng();
    (0..len).map(|_| rng.random()).collect()
}

#[test]
fn test_input_diffusion()
{
    let data_len = W * H;
    let mut total_diff_percent = 0.0;
    let iterations = 10;

    for _ in 0..iterations
    {
        let key = crypto::generate_key::<W, H>();
        let input = generate_random_data(data_len);

        let base_encrypted = encrypter::encrypt::<W, H>(input.clone(), Some(key.to_vec()))
            .expect("Encryption failed")
            .output;

        let mut sum_diff = 0;
        let mut sum_total_bits = 0;

        for i in 0..input.len()
        {
            for bit in 0..64
            {
                let mut modified_input = input.clone();
                modified_input[i] ^= 1 << bit;

                let modified_encrypted = encrypter::encrypt::<W, H>(modified_input, Some(key.to_vec()))
                    .expect("Encryption failed")
                    .output;

                let (diff, total) = calculate_hamming_distance(&base_encrypted, &modified_encrypted);

                sum_diff += diff;
                sum_total_bits += total;
            }
        }

        let percent = (sum_diff as f64 / sum_total_bits as f64) * 100.0;
        total_diff_percent += percent;
    }

    let final_avg = total_diff_percent / iterations as f64;
    writeln!(io::stdout(), "Average Input Diffusion: {:.4}% (Target: ~50%)", final_avg).unwrap();

    assert!(final_avg > 40.0 && final_avg < 60.0, "Input diffusion is weak or biased!");
}

#[test]
fn test_key_diffusion()
{
    let data_len = W * H;
    let mut total_diff_percent = 0.0;
    let iterations = 5;

    for _ in 0..iterations
    {
        let input = generate_random_data(data_len);
        let key = crypto::generate_key::<W, H>();

        let base_encrypted = encrypter::encrypt::<W, H>(input.clone(), Some(key.to_vec()))
            .expect("Encryption failed")
            .output;

        let mut sum_diff = 0;
        let mut sum_total_bits = 0;

        for i in 0..key.len()
        {
            for bit in 0..64
            {
                let mut modified_key = key.clone();
                modified_key[i] ^= 1 << bit;

                let modified_encrypted = encrypter::encrypt::<W, H>(input.clone(), Some(modified_key.to_vec()))
                    .expect("Encryption failed")
                    .output;

                let (diff, total) = calculate_hamming_distance(&base_encrypted, &modified_encrypted);

                sum_diff += diff;
                sum_total_bits += total;
            }
        }

        let percent = (sum_diff as f64 / sum_total_bits as f64) * 100.0;
        total_diff_percent += percent;
    }

    let final_avg = total_diff_percent / iterations as f64;
    writeln!(io::stdout(), "Average Key Diffusion: {:.4}% (Target: ~50%)", final_avg).unwrap();

    assert!(final_avg > 40.0 && final_avg < 60.0, "Key diffusion is weak!");
}
