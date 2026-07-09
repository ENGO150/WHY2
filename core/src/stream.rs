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
//! $$ G_i \leftarrow G_i \oplus E_K(\text{Nonce} + \text{block\_counter} + i) $$
