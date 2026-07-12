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

//! # REX Stream Processing
//!
//! This module defines the stateful interface for streamed encryption and decryption
//! in the WHY2 system. It allows seamless processing of arbitrarily long data in smaller
//! chunks, making it ideal for network communication (e.g., TCP streams) or handling large files.
//!
//! # Overview
//! Unlike the [`encrypter`](crate::encrypter) and [`decrypter`](crate::decrypter) modules,
//! which require a one-shot loading of all data into memory, the [`RexStream`] structure
//! acts as a state machine. Because CTR (Counter) mode is symmetric, the identical internal
//! logic processes both plaintext encryption and ciphertext decryption.
//!
//! The stream lifecycle consists of three steps:
//!
//! 1. **Initialization (`new`)**: Generation of round keys from the master key and allocation
//!    of the internal buffer.
//! 2. **Continuous Processing (`update`)**: Incremental data consumption. Once the buffer
//!    reaches the capacity of a single block ([`Grid`]), the data is transformed in parallel
//!    and flushed to the output.
//! 3. **Stream Finalization (`finalize`)**: Processing of any remaining elements in the buffer
//!    that did not fill an entire grid area by generating a final truncated keystream.
//!
//! # Security and Nonce Reuse
//! For stream ciphers based on CTR mode, it is critically important to ensure that an identical
//! keystream is never generated for different blocks of data under the same key.
//!
//! The internal state therefore strictly enforces counter continuity. The global `block_counter`
//! does not correspond to the number of `update` method calls, but rather the total cumulative
//! number of processed [`Grid`] blocks since the stream's initialization. The keystream for each
//! block $G_i$ within any given chunk is thus derived as:
//!
//! $$ G_i \leftarrow G_i \oplus E_K(\text{Nonce} + \text{block}_{\text{counter}} + i) $$

use std::slice;

use zeroize::Zeroizing;

use crate::
{
    crypto,
    grid::{ Grid, GridError },
};

//STRUCTS
/// A stateful container for incremental encryption and decryption using the REX cipher in CTR mode.
///
/// `RexStream` maintains the cryptographic context and internal buffering required to process
/// data chunks of arbitrary length. It buffers incoming elements until a complete [`Grid`]
/// can be formed, making it suitable for streaming I/O operations such as network sockets
/// or file streams.
///
/// Since CTR mode is symmetric, the same stream instance is used for both encryption
/// and decryption operations.
///
/// # Type Parameters
/// - `W`: The width of the REX [`Grid`].
/// - `H`: The height of the REX [`Grid`].
///
/// # Cryptographic Safety
/// This structure enforces strict block counter continuity across consecutive updates.
/// To maintain semantic security, a unique `nonce` must be paired with the master key
/// for every separate stream lifecycle to prevent keystream reuse.
pub struct RexStream<const W: usize, const H: usize>
{
    round_keys: Zeroizing<Vec<Grid<W, H>>>,
    nonce: Grid<W, H>,
    block_counter: u64,
    buffer: Zeroizing<Vec<i64>>,
}

//IMPLEMENTATIONS
impl<const W: usize, const H: usize> RexStream<W, H>
{
    /// Creates a new stateful `RexStream` instance.
    ///
    /// This initializes the internal buffer and triggers the key expansion phase,
    /// deriving the full set of round keys from the provided master key grid.
    ///
    /// # Parameters
    /// - `key_grid`: A reference to the master [`Grid`] used as the base symmetric key.
    /// - `nonce`: A unique counter initialization [`Grid`] (Initialization Vector).
    ///            Must be distinct for every message encrypted under the same key.
    ///
    /// # Returns
    /// - `Ok(Self)` containing the initialized stream state machine.
    /// - `Err(GridError)` if the round key generation fails (e.g., due to invalid grid dimensions).
    pub fn new(key_grid: &Grid<W, H>, nonce: Grid<W, H>) -> Result<Self, GridError>
    {
        let round_keys = crypto::generate_round_keys(key_grid)?;

        Ok(Self
        {
            round_keys,
            nonce,
            block_counter: 0,
            buffer: Zeroizing::new(Vec::with_capacity(W * H)),
        })
    }

    /// Processes a chunk of data and returns the resulting encrypted or decrypted values.
    ///
    /// This method consumes the input `data`, combining it with any leftover elements
    /// from previous calls. It processes data strictly in full [`Grid`] blocks. Any
    /// incomplete block at the end of the input is buffered internally until the next
    /// call to `update` or until `finalize` is called.
    ///
    /// Because CTR mode is symmetric, this identical method is used for both encryption
    /// and decryption streams.
    ///
    /// # Parameters
    /// - `data`: A slice of `i64` values representing the incoming plaintext or ciphertext chunk.
    ///
    /// # Returns
    /// - `Ok(Vec<i64>)`: A vector containing the processed output. Note that the length
    ///   of this vector may be shorter than the input slice if the data is buffered,
    ///   or longer if previously buffered data now completes a grid.
    /// - `Err(GridError)`: If grid creation or internal processing fails.
    ///
    /// # State Management
    /// This method automatically handles the CTR mode block counter. The internal counter
    /// is incremented strictly by the number of *full grids* processed during this specific
    /// invocation, ensuring cryptographic safety and preventing nonce reuse across batches.
    pub fn update(&mut self, mut data: &[i64]) -> Result<Vec<i64>, GridError>
    {
        let grid_area = W * H;
        let mut output = Vec::new();

        //PROCESS LEFTOVERS FROM PREVIOUS CALL
        if !self.buffer.is_empty()
        {
            //FIX SIZE
            let needed = grid_area - self.buffer.len();
            let take = needed.min(data.len());
            self.buffer.extend_from_slice(&data[..take]);
            data = &data[take..];

            //IF BUFFER REACHED CORRECT SIZE, ENCRYPT
            if self.buffer.len() == grid_area
            {
                //FILL GRID
                let mut grid = Grid::from_flat(&self.buffer)?;

                //ENCRYPT
                crypto::apply_ctr(slice::from_mut(&mut grid), &self.nonce, &self.round_keys, Some(self.block_counter));
                self.block_counter += 1; //INCREMENT COUNTER

                //APPEND TO OUTPUT
                output.extend(grid.to_flat());
                self.buffer.clear(); //CLEAR BUFFER
            }
        }

        //PROCESS FULL GRIDS FROM CURRENT CALL
        let full_grids_count = data.len() / grid_area;
        if full_grids_count > 0 //CONTINUE ONLY IF FULL GRID PASSED
        {
            let chunk_size = full_grids_count * grid_area;

            //GATHER GRIDS
            let mut grids: Vec<Grid<W, H>> = data[..chunk_size].chunks(grid_area)
                .map(Grid::from_flat).collect::<Result<Vec<_>, _>>()?;

            //ENCRYPT GRIDS
            crypto::apply_ctr(&mut grids, &self.nonce, &self.round_keys, Some(self.block_counter));
            self.block_counter += full_grids_count as u64; //INCREMENT COUNTER

            //APPEND TO OUTPUT
            output.extend(grids.iter().flat_map(Grid::to_flat));
            data = &data[chunk_size..];
        }

        //STORE ANY REMAINDER FOR NEXT CALL
        self.buffer.extend_from_slice(data);

        Ok(output)
    }

    /// Finalizes the stream, processing any remaining buffered data.
    ///
    /// In CTR mode, if the total data length is not a multiple of the block size,
    /// the final partial block is XORed with a truncated keystream block. This method
    /// applies that final transformation and returns the remaining elements.
    ///
    /// # Returns
    /// - `Ok(Vec<i64>)`: The final chunk of encrypted or decrypted data.
    /// - `Err(GridError)`: If grid operations fail.
    pub fn finalize(&mut self) -> Result<Vec<i64>, GridError>
    {
        //RETURN EMPTY VECTOR ON EMPTY BUFFER
        if self.buffer.is_empty() { return Ok(Vec::new()) }

        //FILL GRID WITH LEFTOVERS (THE REST WILL REMAIN ZEROES)
        let mut grid = Zeroizing::new(Grid::from_flat(&self.buffer)?);

        //ENCRYPT
        crypto::apply_ctr
        (
            slice::from_mut(&mut *grid),
            &self.nonce,
            &self.round_keys,
            Some(self.block_counter)
        );

        //INCREMENT COUNTER (OPTIONAL, BUT *STATE CONTINUITY*)
        self.block_counter += 1;

        //EXTRACT ONLY THE EXACT AMOUNT OF ELEMENTS WE NEED
        let mut output = grid.to_flat();
        output.truncate(self.buffer.len()); //TRUNCATE TO ORIGINAL BUFFER LENGTH

        //CLEAR BUFFER
        self.buffer.clear();

        Ok(output)
    }
}
