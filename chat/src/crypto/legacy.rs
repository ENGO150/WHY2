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

//! # Previous-Release Decryption
//!
//! Frozen copy of the CTR keystream as core 1.5.2/1.5.3 produced it, before the current core
//! changed which round key drives `shift_rows`. The message history, the server's picture store
//! and the client's picture cache use it for their one-time migration of the previous release's
//! files: those still verify with the same HMAC key, but would decrypt to garbage without this.
//!
//! This module is deliberately self-contained and must not be "fixed" to follow core changes.
//! Remove it with the next version bump, once every stored file has been rewritten.

use zeroize::Zeroizing;

use why2::grid::Grid;

/// Applies the previous release's CTR keystream to `grids` in place.
///
/// The previous core precalculated a row shift for **every** round key, including both
/// whitening keys, and the keyed round that XORed `round_keys[i + 1]` used the shift of
/// `round_keys[i]`. The current core instead derives one shift per keyed round from
/// `round_keys[i + 1]`, which produces a different keystream.
///
/// # Parameters
/// - `grids`: The blocks to transform.
/// - `nonce`: The stream's initial counter grid.
/// - `round_keys`: The schedule derived from the file's key.
/// - `counter_offset`: The stream's block counter before this batch.
pub(super) fn apply_ctr<const W: usize, const H: usize>
(
    grids: &mut [Grid<W, H>],
    nonce: &Grid<W, H>,
    round_keys: &[Grid<W, H>],
    counter_offset: u64,
)
{
    //ONE SHIFT PER KEY, WHITENING KEYS INCLUDED
    let round_shifts = Zeroizing::new(round_keys.iter().map(Grid::precalculate_shifts).collect::<Vec<[usize; H]>>());

    for (index, grid) in grids.iter_mut().enumerate()
    {
        //COUNTER BLOCK
        let mut block = nonce.clone();
        block.increment(counter_offset.wrapping_add(index as u64));

        //INITIAL WHITENING
        block ^= &round_keys[0];

        //KEYED ROUNDS, USING THE SHIFT OF THE ROUND KEY ITSELF
        for (round, round_key) in round_keys[1..round_keys.len() - 1].iter().enumerate()
        {
            block ^= round_key;
            block.subcell(round);
            block.shift_rows(&round_shifts[round]);
            block.mix_columns();
        }

        //FINAL WHITENING
        block ^= &round_keys[round_keys.len() - 1];

        //XOR KEYSTREAM AND DATA
        *grid ^= &block;
    }
}
