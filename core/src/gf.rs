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

//! # REX Galois Field Arithmetic
//!
//! Arithmetic in $\mathbb{F}_{2^{64}}$ modulo $p(x) = x^{64} + x^4 + x^3 + x + 1$, backing
//! [`mix_columns`](crate::grid::Grid::mix_columns).
//!
//! ## Why this module is a constant and two functions
//!
//! It used to hold a full carry-less multiply with four dispatched backends — `VPCLMULQDQ`,
//! `PCLMULQDQ`, `PMULL` and a portable bitwise fallback — plus a deferred-reduction scheme and
//! a Karatsuba split, all of it there to make the diffusion layer's $H^2$ field products per
//! column affordable (27 of them at the default height, after the split). None of that is
//! needed any more.
//!
//! The [MDS matrices](crate::consts) are now built from coefficients that are sums of powers of
//! $x$ with tiny exponents, so a product is a handful of [`xtime`] steps and an XOR. The general
//! multiply has no callers left, and with it went the CPU dispatch, the `unsafe` intrinsics and
//! the machine-dependent code paths: the diffusion layer is now the same portable shift-and-XOR
//! sequence everywhere, and the only thing left to detect is a wider register to run it in.

#[cfg(target_arch = "x86_64")]
use std::sync::LazyLock;

/// Columns processed per pass. Four `u64` is exactly one 256-bit vector register, so a lane is
/// a whole register once the shifts and XORs below are vectorised.
pub(crate) const LANE: usize = 4;

/// Multiplies by $x$ once, folding the bit that leaves the top back through $p(x)$.
///
/// $p(x)$ minus its leading term is $x^4 + x^3 + x + 1$, so the fold is a XOR with `0x1B` and
/// the whole step is branchless: the mask is the arithmetic negation of the departing bit.
///
/// Multiplication by any $x^e$ is $e$ of these, and every MDS coefficient is a sum of such
/// powers, so this is the only field operation the cipher performs.
#[inline(always)]
pub(crate) fn xtime(v: u64) -> u64
{
    (v << 1) ^ (0u64.wrapping_sub(v >> 63) & 0x1B)
}

/// Whether 256-bit integer vectors are available, resolved once per process.
///
/// Both [`subcell`](crate::grid::Grid::subcell) and
/// [`mix_columns`](crate::grid::Grid::mix_columns) want it; the detection lives here so every
/// CPU-feature test in the crate sits in one module.
#[cfg(target_arch = "x86_64")]
static AVX2: LazyLock<bool> = LazyLock::new(|| is_x86_feature_detected!("avx2"));

/// See [`AVX2`].
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub(crate) fn has_avx2() -> bool
{
    *AVX2
}
