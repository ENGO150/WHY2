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

use std::error::Error;

use rand::RngExt;

use why2::
{
    grid::Grid,
    stream::RexStream,
};

#[test]
fn test_stream_encryption_decryption_symmetric() -> Result<(), Box<dyn Error>>
{
    const W: usize = 4;
    const H: usize = 4;
    let mut rng = rand::rng();

    //INIT KEY & NONCE
    let key_data: Vec<i64> = (0..(W * H * 2)).map(|_| rng.random()).collect();
    let key_grid = Grid::<W, H>::from_key(&key_data)?;
    let nonce_data: Vec<i64> = (0..(W * H * 2)).map(|_| rng.random()).collect();
    let nonce_grid = Grid::<W, H>::from_key(&nonce_data)?;

    //CREATE REX STREAMS
    let mut encrypt_stream = RexStream::<W, H>::new(&key_grid, nonce_grid.clone())?;
    let mut decrypt_stream = RexStream::<W, H>::new(&key_grid, nonce_grid)?;

    //GENERATE RANDOM DATA
    let data: Vec<i64> = (0..100).map(|_| rng.random()).collect();

    //ENCRYPT IN CHUNKS OF 7
    let mut ciphertext = Vec::new();
    for chunk in data.chunks(7)
    {
        ciphertext.extend(encrypt_stream.update(chunk)?);
    }
    ciphertext.extend(encrypt_stream.finalize()?);

    assert_eq!(ciphertext.len(), data.len(), "Ciphertext length should match plaintext length in CTR mode");

    //DECRYPT IN CHUNKS OF 13
    let mut plaintext = Vec::new();
    for chunk in ciphertext.chunks(13)
    {
        plaintext.extend(decrypt_stream.update(chunk)?);
    }
    plaintext.extend(decrypt_stream.finalize()?);

    assert_eq!(data, plaintext, "Decrypted data should match the original plaintext");
    Ok(())
}

#[test]
fn test_stream_large_chunks() -> Result<(), Box<dyn Error>>
{
    const W: usize = 8;
    const H: usize = 8;
    let mut rng = rand::rng();

    //INIT KEY & NONCE
    let key_data: Vec<i64> = (0..(W * H * 2)).map(|_| rng.random()).collect();
    let key_grid = Grid::<W, H>::from_key(&key_data)?;
    let nonce_data: Vec<i64> = (0..(W * H * 2)).map(|_| rng.random()).collect();
    let nonce_grid = Grid::<W, H>::from_key(&nonce_data)?;

    let mut stream = RexStream::<W, H>::new(&key_grid, nonce_grid.clone())?;

    //PROCESS 200 ELEMENTS IN ONE GO
    let data: Vec<i64> = (0..200).map(|_| rng.random()).collect();
    let mut ciphertext = stream.update(&data)?;
    ciphertext.extend(stream.finalize()?);

    let mut decrypt_stream = RexStream::<W, H>::new(&key_grid, nonce_grid)?;
    let mut plaintext = decrypt_stream.update(&ciphertext)?;
    plaintext.extend(decrypt_stream.finalize()?);

    assert_eq!(data, plaintext);
    Ok(())
}
