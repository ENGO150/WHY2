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
//! ## Backend selection
//!
//! The carry-less multiply is dispatched **once**, through [`backend`], rather than probed on
//! every product. Each backend is a `#[target_feature]` entry point, so the intrinsics inline
//! into the caller and the surrounding XOR work is vectorised with the same feature set. The
//! previous arrangement called `is_x86_feature_detected!` inside the multiply itself, which
//! both cost a branch per product and prevented that inlining.
//!
//! Backends, in preference order:
//! - **`Vclmul`** (x86-64, `vpclmulqdq` + `avx2`): four products per instruction pair.
//! - **`Clmul`** (x86-64, `pclmulqdq`): two products per instruction pair.
//! - **`Pmull`** (aarch64, `aes` + `neon`): two products per instruction pair.
//! - **`Soft`**: portable bitwise fallback, branchless and therefore constant-time.

use std::sync::LazyLock;

#[cfg(target_arch = "x86_64")]
use std::arch::x86_64::*;

#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;

/// Columns processed per pass. Four `u64` is exactly one 256-bit vector register, so a
/// lane is a whole register on the vector backends and the scratch space used by the
/// Karatsuba recursion stays bounded regardless of the grid width.
pub(crate) const LANE: usize = 4;

/// Which carry-less multiply implementation this CPU gets.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Backend
{
    /// Portable bitwise fallback.
    Soft,
    /// x86-64 `PCLMULQDQ`.
    #[cfg(target_arch = "x86_64")]
    Clmul,
    /// x86-64 `VPCLMULQDQ` over 256-bit vectors.
    #[cfg(target_arch = "x86_64")]
    Vclmul,
    /// aarch64 `PMULL`/`PMULL2`.
    #[cfg(target_arch = "aarch64")]
    Pmull,
}

//BACKEND IDS (const-generic selector, folded away at monomorphisation)
pub(crate) const B_SOFT: u8 = 0;
#[cfg(target_arch = "x86_64")]
pub(crate) const B_CLMUL: u8 = 1;
#[cfg(target_arch = "x86_64")]
pub(crate) const B_VCLMUL: u8 = 2;
#[cfg(target_arch = "aarch64")]
pub(crate) const B_PMULL: u8 = 3;

static BACKEND: LazyLock<Backend> = LazyLock::new(||
{
    #[cfg(target_arch = "x86_64")]
    {
        //VPCLMULQDQ IS USELESS WITHOUT AVX2 HERE, THE REDUCTION RUNS ON 256-BIT LANES
        if is_x86_feature_detected!("vpclmulqdq") && is_x86_feature_detected!("avx2")
        {
            return Backend::Vclmul;
        }

        if is_x86_feature_detected!("pclmulqdq") { return Backend::Clmul; }
    }

    #[cfg(target_arch = "aarch64")]
    {
        //PMULL LIVES IN THE CRYPTO EXTENSIONS AND IS NOT IMPLIED BY NEON
        if std::arch::is_aarch64_feature_detected!("aes")
            && std::arch::is_aarch64_feature_detected!("neon")
        {
            return Backend::Pmull;
        }
    }

    Backend::Soft
});

/// Returns the carry-less multiply backend for this CPU, resolved once per process.
#[inline(always)]
pub(crate) fn backend() -> Backend
{
    *BACKEND
}

/// Whether 256-bit integer vectors are available, resolved once per process.
///
/// This is the probe for [`subcell`](crate::grid::Grid::subcell) rather than for the field
/// arithmetic, but it lives here so every CPU-feature test in the crate sits in one module.
#[cfg(target_arch = "x86_64")]
static AVX2: LazyLock<bool> = LazyLock::new(|| is_x86_feature_detected!("avx2"));

/// See [`AVX2`].
#[cfg(target_arch = "x86_64")]
#[inline(always)]
pub(crate) fn has_avx2() -> bool
{
    *AVX2
}

//REDUCTION
/// Folds a 128-bit carry-less product back into $\mathbb{F}_{2^{64}}$.
///
/// With $p(x) = x^{64} + x^4 + x^3 + x + 1$ the high half contributes
/// $\text{hi} \cdot (x^4 + x^3 + x + 1)$, which itself overflows by at most four bits and so
/// needs one further fold.
#[inline(always)]
fn reduce(lo: u64, hi: u64) -> u64
{
    let mid = (hi << 4) ^ (hi << 3) ^ (hi << 1) ^ hi;
    let overflow = (hi >> 60) ^ (hi >> 61) ^ (hi >> 63);
    let extra = (overflow << 4) ^ (overflow << 3) ^ (overflow << 1) ^ overflow;

    lo ^ mid ^ extra
}

//SOFTWARE MULTIPLY
/// Branchless bitwise carry-less multiply, returning the full 128-bit product.
///
/// Both the partial-product accumulation and the shifting are driven by arithmetic masks
/// rather than branches, so the running time does not depend on either operand. The previous
/// implementation branched on a bit of the (secret) left operand while folding the modulus,
/// which the `constant-time` feature was supposed to rule out.
#[inline(always)]
fn mul_soft_raw(a: u64, b: u64) -> (u64, u64)
{
    let mut lo: u64 = 0;
    let mut hi: u64 = 0;

    for i in 0..64
    {
        //MASK IS ALL-ONES WHEN BIT i OF b IS SET
        let mask = 0u64.wrapping_sub((b >> i) & 1);

        lo ^= (a << i) & mask;

        //THE i == 0 CASE WOULD BE A 64-BIT SHIFT, WHICH IS UNDEFINED
        if i > 0 { hi ^= (a >> (64 - i)) & mask; }
    }

    (lo, hi)
}

//LANE SCALING
/// An unreduced lane: the 128-bit carry-less products of [`LANE`] elements, in whatever
/// layout the selected backend finds natural.
///
/// Reduction modulo $p(x)$ is $\mathbb{F}_2$-linear, so
/// $\text{reduce}(a) \oplus \text{reduce}(b) = \text{reduce}(a \oplus b)$ and a whole XOR-sum
/// of products can be folded **once** at the end instead of after every product. The
/// convolution in [`mix_columns`](crate::grid::Grid::mix_columns) is exactly such a sum, which
/// takes the reduction count for an $H = 8$ grid from 27 down to 8.
pub(crate) const RAW: usize = 2 * LANE;

/// Carry-less products of one lane against a single coefficient, left unreduced.
///
/// The layout of the returned array is backend-private; the only contract is that XOR is
/// elementwise (so partial sums can be accumulated) and that [`reduce_lane`] with the same `B`
/// interprets it.
///
/// # Safety
/// `B` must name a backend whose CPU features are available; callers reach this only through
/// the `#[target_feature]` entry points guarded by [`backend`].
#[inline(always)]
pub(crate) unsafe fn scale_raw<const B: u8>(src: &[u64; LANE], coeff: u64) -> [u64; RAW]
{
    match B
    {
        #[cfg(target_arch = "x86_64")]
        B_VCLMUL => unsafe { scale_raw_vclmul(src, coeff) },

        #[cfg(target_arch = "x86_64")]
        B_CLMUL => unsafe { scale_raw_clmul(src, coeff) },

        #[cfg(target_arch = "aarch64")]
        B_PMULL => unsafe { scale_raw_pmull(src, coeff) },

        _ =>
        {
            let mut out = [0u64; RAW];
            for i in 0..LANE
            {
                let (lo, hi) = mul_soft_raw(src[i], coeff);
                out[i] = lo;
                out[LANE + i] = hi;
            }

            out
        },
    }
}

/// Folds an accumulated [`RAW`] lane back into `LANE` field elements.
///
/// # Safety
/// As [`scale_raw`]; `B` must match the value the lane was produced with.
#[inline(always)]
pub(crate) unsafe fn reduce_lane<const B: u8>(raw: &[u64; RAW]) -> [u64; LANE]
{
    match B
    {
        #[cfg(target_arch = "x86_64")]
        B_VCLMUL => unsafe { reduce_lane_vclmul(raw) },

        #[cfg(target_arch = "x86_64")]
        B_CLMUL => unsafe { reduce_lane_clmul(raw) },

        _ => std::array::from_fn(|i| reduce(raw[i], raw[LANE + i])),
    }
}

//x86-64: 256-BIT CARRY-LESS MULTIPLY
/// Four products in two `VPCLMULQDQ` instructions.
///
/// Each 128-bit half of the vector multiplies its own pair, so `0x00` covers elements 0 and 2
/// and `0x11` covers 1 and 3. The two result vectors are stored back to back and regrouped by
/// [`reduce_lane_vclmul`].
///
/// # Safety
/// Requires `vpclmulqdq` and `avx2`.
#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn scale_raw_vclmul(src: &[u64; LANE], coeff: u64) -> [u64; RAW]
{
    unsafe
    {
        let a = _mm256_loadu_si256(src.as_ptr() as *const __m256i);
        let b = _mm256_set1_epi64x(coeff as i64);

        let p0 = _mm256_clmulepi64_epi128(a, b, 0x00);
        let p1 = _mm256_clmulepi64_epi128(a, b, 0x11);

        let mut out = [0u64; RAW];
        _mm256_storeu_si256(out.as_mut_ptr() as *mut __m256i, p0);
        _mm256_storeu_si256(out.as_mut_ptr().add(LANE) as *mut __m256i, p1);
        out
    }
}

/// Reduction half of [`scale_raw_vclmul`].
///
/// # Safety
/// Requires `vpclmulqdq` and `avx2`, and a lane produced by [`scale_raw_vclmul`].
#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn reduce_lane_vclmul(raw: &[u64; RAW]) -> [u64; LANE]
{
    unsafe
    {
        let p0 = _mm256_loadu_si256(raw.as_ptr() as *const __m256i);
        let p1 = _mm256_loadu_si256(raw.as_ptr().add(LANE) as *const __m256i);

        //REGROUP INTO LOW AND HIGH HALVES OF THE FOUR PRODUCTS
        let lo = _mm256_unpacklo_epi64(p0, p1);
        let hi = _mm256_unpackhi_epi64(p0, p1);

        let mid = _mm256_xor_si256
        (
            _mm256_xor_si256(_mm256_slli_epi64(hi, 4), _mm256_slli_epi64(hi, 3)),
            _mm256_xor_si256(_mm256_slli_epi64(hi, 1), hi),
        );

        let overflow = _mm256_xor_si256
        (
            _mm256_xor_si256(_mm256_srli_epi64(hi, 60), _mm256_srli_epi64(hi, 61)),
            _mm256_srli_epi64(hi, 63),
        );

        let extra = _mm256_xor_si256
        (
            _mm256_xor_si256(_mm256_slli_epi64(overflow, 4), _mm256_slli_epi64(overflow, 3)),
            _mm256_xor_si256(_mm256_slli_epi64(overflow, 1), overflow),
        );

        let res = _mm256_xor_si256(_mm256_xor_si256(lo, mid), extra);

        let mut out = [0u64; LANE];
        _mm256_storeu_si256(out.as_mut_ptr() as *mut __m256i, res);
        out
    }
}

//x86-64: 128-BIT CARRY-LESS MULTIPLY
/// Two products per `PCLMULQDQ` pair, twice to cover a lane.
///
/// # Safety
/// Requires `pclmulqdq` (`sse2` is baseline on x86-64).
#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn scale_raw_clmul(src: &[u64; LANE], coeff: u64) -> [u64; RAW]
{
    unsafe
    {
        let b = _mm_set1_epi64x(coeff as i64);
        let mut out = [0u64; RAW];

        let mut i = 0;
        while i < LANE
        {
            let a = _mm_loadu_si128(src.as_ptr().add(i) as *const __m128i);

            let p0 = _mm_clmulepi64_si128(a, b, 0x00);
            let p1 = _mm_clmulepi64_si128(a, b, 0x11);

            _mm_storeu_si128(out.as_mut_ptr().add(i * 2) as *mut __m128i, p0);
            _mm_storeu_si128(out.as_mut_ptr().add(i * 2 + 2) as *mut __m128i, p1);

            i += 2;
        }

        out
    }
}

/// Reduction half of [`scale_raw_clmul`].
///
/// # Safety
/// Requires `pclmulqdq`, and a lane produced by [`scale_raw_clmul`].
#[cfg(target_arch = "x86_64")]
#[inline(always)]
unsafe fn reduce_lane_clmul(raw: &[u64; RAW]) -> [u64; LANE]
{
    unsafe
    {
        let mut out = [0u64; LANE];

        let mut i = 0;
        while i < LANE
        {
            let p0 = _mm_loadu_si128(raw.as_ptr().add(i * 2) as *const __m128i);
            let p1 = _mm_loadu_si128(raw.as_ptr().add(i * 2 + 2) as *const __m128i);

            let lo = _mm_unpacklo_epi64(p0, p1);
            let hi = _mm_unpackhi_epi64(p0, p1);

            let mid = _mm_xor_si128
            (
                _mm_xor_si128(_mm_slli_epi64(hi, 4), _mm_slli_epi64(hi, 3)),
                _mm_xor_si128(_mm_slli_epi64(hi, 1), hi),
            );

            let overflow = _mm_xor_si128
            (
                _mm_xor_si128(_mm_srli_epi64(hi, 60), _mm_srli_epi64(hi, 61)),
                _mm_srli_epi64(hi, 63),
            );

            let extra = _mm_xor_si128
            (
                _mm_xor_si128(_mm_slli_epi64(overflow, 4), _mm_slli_epi64(overflow, 3)),
                _mm_xor_si128(_mm_slli_epi64(overflow, 1), overflow),
            );

            let res = _mm_xor_si128(_mm_xor_si128(lo, mid), extra);
            _mm_storeu_si128(out.as_mut_ptr().add(i) as *mut __m128i, res);

            i += 2;
        }

        out
    }
}

//aarch64: PMULL
/// Two products per `PMULL`/`PMULL2` pair, stored in the portable low/high layout.
///
/// # Safety
/// Requires the `aes` crypto extension and `neon`. The previous code invoked `vmull_p64`
/// with no feature test at all, which is undefined behaviour on an aarch64 CPU without the
/// crypto extensions; [`backend`] now gates it.
#[cfg(target_arch = "aarch64")]
#[inline(always)]
unsafe fn scale_raw_pmull(src: &[u64; LANE], coeff: u64) -> [u64; RAW]
{
    unsafe
    {
        let mut out = [0u64; RAW];

        let mut i = 0;
        while i < LANE
        {
            let a = vcombine_u64(vcreate_u64(src[i]), vcreate_u64(src[i + 1]));
            let b = vdupq_n_u64(coeff);

            let p0 = vreinterpretq_u64_p128(vmull_p64(vgetq_lane_u64(a, 0), vgetq_lane_u64(b, 0)));
            let p1 = vreinterpretq_u64_p128(vmull_high_p64(vreinterpretq_p64_u64(a), vreinterpretq_p64_u64(b)));

            out[i] = vgetq_lane_u64(p0, 0);
            out[LANE + i] = vgetq_lane_u64(p0, 1);
            out[i + 1] = vgetq_lane_u64(p1, 0);
            out[LANE + i + 1] = vgetq_lane_u64(p1, 1);

            i += 2;
        }

        out
    }
}
