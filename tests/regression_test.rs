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

use std::mem::ManuallyDrop;

use sha2::{ Digest, Sha256 };

use why2::
{
    crypto,
    decrypter,
    encrypter,
    grid::{ Grid, GridError },
    stream::RexStream,
};

/// Pins the corrected keyed-round shift schedule and the stream path that uses it.
///
/// The digest covers the ciphertext produced by `apply_ctr` with a fixed key, nonce and
/// counter offset. A change to the shift schedule changes the keystream and therefore
/// this digest.
#[test]
fn test_shift_schedule_known_answer()
{
    const W: usize = 4;
    const H: usize = 4;

    let key_data: Vec<i64> = (0..2 * W * H)
        .map(|i| (i as i64).wrapping_mul(0x0101_0101_0101_0101))
        .collect();
    let master = Grid::<W, H>::from_key(&key_data).expect("valid master key");
    let round_keys = crypto::generate_round_keys(&master).expect("round keys");
    let nonce = Grid::<W, H>::from_key(&vec![0x0123_4567_89ab_cdefi64; 2 * W * H])
        .expect("valid nonce");

    let mut plaintext = Vec::new();
    for block in 0..6
    {
        let mut grid = Grid::<W, H>::new().expect("valid grid");
        for r in 0..H
        {
            for c in 0..W
            {
                grid[r][c] = ((block * W * H + r * W + c) as i64)
                    .wrapping_mul(0x9E37_79B9_7F4A_7C15u64 as i64);
            }
        }
        plaintext.push(grid);
    }

    //KNOWN-ANSWER CIPHERTEXT FOR offset = 7
    let mut ciphertext = plaintext.clone();
    crypto::apply_ctr(&mut ciphertext, &nonce, &round_keys, Some(7));

    assert_eq!(ciphertext[0][0][0] as u64, 0xc553_3ebc_379b_df97);
    assert_eq!(ciphertext[5][3][3] as u64, 0x1ca9_ce89_c556_ef24);

    let mut hasher = Sha256::new();
    for grid in &ciphertext
    {
        for row in grid.iter()
        {
            for value in row
            {
                hasher.update(value.to_be_bytes());
            }
        }
    }

    let digest: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(digest, "16f7734714cafef9f4b8a14562f414b405b360c6ea2f0aaee6f926a15fd1f2af");

    //THE STREAM MUST PRODUCE THE SAME KEYSTREAM AS THE ONE-SHOT PATH (offset = 0)
    let mut one_shot = plaintext.clone();
    crypto::apply_ctr(&mut one_shot, &nonce, &round_keys, None);

    let flattened: Vec<i64> = plaintext.iter().flat_map(Grid::to_flat).collect();
    let expected: Vec<i64> = one_shot.iter().flat_map(Grid::to_flat).collect();

    let mut stream = RexStream::<W, H>::new(&master, nonce).expect("stream");
    let mut streamed = stream.update(&flattened).expect("stream update");
    streamed.extend(stream.finalize().expect("stream finalize"));

    assert_eq!(streamed, expected, "RexStream and apply_ctr must share the shift schedule");
}

#[test]
fn test_string_terminator_roundtrip()
{
    let cases = ["", "\0", "A\0B", "AB\0", "\0\0\0", "Hello, world!", "👶🏿㍿㊓ㅅΔ♛", "a\0b\0c\0d"];

    for text in cases
    {
        let encrypted = encrypter::encrypt_string::<8, 8>(text, None).expect("encryption");
        let decrypted = decrypter::decrypt_string(encrypted).expect("decryption");

        assert_eq!(text, &*decrypted, "roundtrip failed for {text:?}");
    }

    //LENGTHS AROUND CELL AND GRID BOUNDARIES, INCLUDING EMBEDDED NULS
    for n in 0..(8 * 8 * 2 + 3)
    {
        let text: String = (0..n)
            .map(|i| if i % 5 == 0 { '\0' } else { char::from_u32(0x100 + i as u32).unwrap() })
            .collect();

        let encrypted = encrypter::encrypt_string::<8, 8>(&text, None).expect("encryption");
        let decrypted = decrypter::decrypt_string(encrypted).expect("decryption");

        assert_eq!(text, *decrypted, "boundary roundtrip failed at length {n}");
    }

    //AN EMPTY STRING MUST STILL CARRY THE TERMINATOR
    let encrypted = encrypter::encrypt_string::<8, 8>("", None).expect("encryption");
    assert_eq!(encrypted.output.len(), 1);
}

#[test]
fn test_string_wire_format_and_missing_terminator()
{
    let encrypted = encrypter::encrypt_string::<8, 8>("AB", None).expect("encryption");
    let decrypted = decrypter::decrypt(encrypted).expect("decryption");

    //HIGH UNIT FIRST, THEN LOW UNIT, THEN THE TERMINATOR
    assert_eq!(decrypted.output[0] as u64, (('A' as u64) << 32) | 'B' as u64);
    assert_eq!((decrypted.output[1] as u64) >> 32, u32::MAX as u64);

    //RAW CELLS WITHOUT A TERMINATOR MUST FAIL WITHOUT EXPOSING PLAINTEXT
    let encrypted = encrypter::encrypt::<8, 8>(&[0x0041_0042, 0x0043_0044], None)
        .expect("encryption");
    let error = decrypter::decrypt_string(encrypted).expect_err("missing terminator must fail");

    assert_eq!(error, GridError::InvalidUnicode);
    assert!(!error.to_string().contains("0x"), "error must not leak a value");
}

#[test]
fn test_from_key_strict_length()
{
    let exact = vec![0i64; 2 * 8 * 8];
    assert!(Grid::<8, 8>::from_key(&exact).is_ok());

    let short = vec![0i64; 2 * 8 * 8 - 1];
    assert!(matches!(
        Grid::<8, 8>::from_key(&short),
        Err(GridError::InvalidKeyLength { .. })
    ));

    let long = vec![0i64; 2 * 8 * 8 + 1];
    assert!(matches!(
        Grid::<8, 8>::from_key(&long),
        Err(GridError::InvalidKeyLength { .. })
    ));
}

#[test]
fn test_dimension_contract()
{
    assert!(Grid::<1, 4>::new().is_err(), "width 1 must be rejected");
    assert!(Grid::<0, 4>::new().is_err(), "width 0 must be rejected");
    assert!(Grid::<2, 2>::new().is_err(), "height 2 is unsupported");
    assert!(Grid::<2, 4>::new().is_ok());
    assert!(Grid::<8, 8>::new().is_ok());
    assert!(Grid::<16, 16>::new().is_ok());
}

#[test]
fn test_grid_drop_zeroizes()
{
    //KEEP THE ALLOCATION ALIVE SO THE DROPPED STORAGE CAN BE INSPECTED
    let mut grid = ManuallyDrop::new(Grid::<8, 8>::new().expect("valid grid"));
    for row in 0..8
    {
        for column in 0..8
        {
            grid[row][column] = i64::MAX;
        }
    }

    let pointer = &grid[0][0] as *const i64;

    //SAFETY: test-only; the allocation is still alive inside ManuallyDrop.
    unsafe
    {
        std::ptr::drop_in_place(&mut *grid);
    }

    //SAFETY: reading the dropped but still allocated storage to verify zeroization.
    let words = unsafe { std::slice::from_raw_parts(pointer, 64) };
    assert!(words.iter().all(|word| *word == 0), "Grid drop must zeroize its cells");
}
