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

//PRIVATE
#[inline(always)]
fn mul2(a0: u64, b0: u64, a1: u64, b1: u64) -> (u64, u64)
{
    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("pclmulqdq")
    {
        return unsafe
        {
            use std::arch::x86_64::*;

            let a_vec = _mm_set_epi64x(a1 as i64, a0 as i64);
            let b_vec = _mm_set_epi64x(b1 as i64, b0 as i64);

            let lo_vec = _mm_clmulepi64_si128(a_vec, b_vec, 0x00); // a0 * b0
            let hi_vec = _mm_clmulepi64_si128(a_vec, b_vec, 0x11); // a1 * b1

            let lo0 = _mm_extract_epi64(lo_vec, 0) as u64;
            let hi0 = _mm_extract_epi64(lo_vec, 1) as u64;
            let lo1 = _mm_extract_epi64(hi_vec, 0) as u64;
            let hi1 = _mm_extract_epi64(hi_vec, 1) as u64;

            (reduce(lo0, hi0), reduce(lo1, hi1))
        };
    }

    #[cfg(target_arch = "aarch64")]
    return unsafe
    {
        use std::arch::aarch64::*;

        let a_vec = vcombine_u64(vcreate_u64(a0), vcreate_u64(a1));
        let b_vec = vcombine_u64(vcreate_u64(b0), vcreate_u64(b1));

        let lo_vec = vmull_p64
        (
            vgetq_lane_u64(a_vec, 0),
            vgetq_lane_u64(b_vec, 0)
        );

        let hi_vec = vmull_high_p64
        (
            vreinterpretq_p64_u64(a_vec),
            vreinterpretq_p64_u64(b_vec)
        );

        let lo0 = vgetq_lane_u64(vreinterpretq_u64_p128(lo_vec), 0);
        let hi0 = vgetq_lane_u64(vreinterpretq_u64_p128(lo_vec), 1);
        let lo1 = vgetq_lane_u64(vreinterpretq_u64_p128(hi_vec), 0);
        let hi1 = vgetq_lane_u64(vreinterpretq_u64_p128(hi_vec), 1);

        (reduce(lo0, hi0), reduce(lo1, hi1))
    };

    //SW FALLBACK
    #[cfg(not(target_arch = "aarch64"))]
    (mul_soft(a0, b0), mul_soft(a1, b1))
}

#[cfg(not(target_arch = "aarch64"))]
#[inline(always)]
fn mul_soft(a: u64, b: u64) -> u64
{
    let mut result: u64 = 0;
    let mut a = a;
    let mut b = b;
    let poly = 0x1Bu64;

    for _ in 0..64
    {
        if b & 1 == 1 { result ^= a; }
        let carry = (a >> 63) & 1;
        a <<= 1;
        if carry == 1 { a ^= poly; }
        b >>= 1;
    }

    result
}

#[inline(always)]
fn reduce(lo: u64, hi: u64) -> u64
{
    let mid = (hi << 4) ^ (hi << 3) ^ (hi << 1) ^ hi;
    let overflow = (hi >> 60) ^ (hi >> 61) ^ (hi >> 63);
    let extra = (overflow << 4) ^ (overflow << 3) ^ (overflow << 1) ^ overflow;

    lo ^ mid ^ extra
}

//PUBLIC
#[inline(always)]
pub fn mul(a: u64, b: u64) -> u64
{
    mul2(a, b, 0, 0).0
}

#[inline(always)]
pub fn mul_const2(a0: u64, a1: u64, coeff: u64) -> (u64, u64)
{
    #[cfg(target_arch = "x86_64")]
    if is_x86_feature_detected!("pclmulqdq")
    {
        return unsafe
        {
            use std::arch::x86_64::*;

            let a_vec = _mm_set_epi64x(a1 as i64, a0 as i64);
            let b_vec = _mm_set1_epi64x(coeff as i64); //BROADCAST

            let lo_vec = _mm_clmulepi64_si128(a_vec, b_vec, 0x00); // a0 * coeff
            let hi_vec = _mm_clmulepi64_si128(a_vec, b_vec, 0x11); // a1 * coeff

            let lo0 = _mm_extract_epi64(lo_vec, 0) as u64;
            let hi0 = _mm_extract_epi64(lo_vec, 1) as u64;
            let lo1 = _mm_extract_epi64(hi_vec, 0) as u64;
            let hi1 = _mm_extract_epi64(hi_vec, 1) as u64;

            (reduce(lo0, hi0), reduce(lo1, hi1))
        };
    }

    #[cfg(target_arch = "aarch64")]
    return unsafe
    {
        use std::arch::aarch64::*;

        let a_vec = vcombine_u64(vcreate_u64(a0), vcreate_u64(a1));
        let b_vec = vdupq_n_u64(coeff); //BROADCAST

        let lo_vec = vmull_p64
        (
            vgetq_lane_u64(a_vec, 0),
            vgetq_lane_u64(b_vec, 0)
        );

        let hi_vec = vmull_high_p64
        (
            vreinterpretq_p64_u64(a_vec),
            vreinterpretq_p64_u64(b_vec)
        );

        let lo0 = vgetq_lane_u64(vreinterpretq_u64_p128(lo_vec), 0);
        let hi0 = vgetq_lane_u64(vreinterpretq_u64_p128(lo_vec), 1);
        let lo1 = vgetq_lane_u64(vreinterpretq_u64_p128(hi_vec), 0);
        let hi1 = vgetq_lane_u64(vreinterpretq_u64_p128(hi_vec), 1);

        (reduce(lo0, hi0), reduce(lo1, hi1))
    };

    #[cfg(not(target_arch = "aarch64"))]
    (mul_soft(a0, coeff), mul_soft(a1, coeff))
}
