/*
 * Copyright (c) 2023.
 *
 * This software is free software;
 *
 * You can redistribute it or modify it under terms of the MIT, Apache License or Zlib license
 */

#![cfg(all(feature = "x86", any(target_arch = "x86", target_arch = "x86_64")))]
//! This module provides unsafe ways to do some things
#![allow(clippy::wildcard_imports)]

#[cfg(target_arch = "x86")]
use core::arch::x86::*;
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::*;
use core::ops::{Add, AddAssign, Mul, MulAssign, Sub};

/// An abstraction of an AVX ymm register that
///allows some things to not look ugly
#[derive(Clone, Copy)]
pub struct YmmRegister {
    /// An AVX register
    pub(crate) mm256: __m256i
}

impl Add for YmmRegister {
    type Output = YmmRegister;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        unsafe {
            return YmmRegister {
                mm256: _mm256_add_epi32(self.mm256, rhs.mm256)
            };
        }
    }
}

impl Add<i32> for YmmRegister {
    type Output = YmmRegister;

    #[inline]
    fn add(self, rhs: i32) -> Self::Output {
        unsafe {
            let tmp = _mm256_set1_epi32(rhs);

            return YmmRegister {
                mm256: _mm256_add_epi32(self.mm256, tmp)
            };
        }
    }
}

impl Sub for YmmRegister {
    type Output = YmmRegister;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        unsafe {
            return YmmRegister {
                mm256: _mm256_sub_epi32(self.mm256, rhs.mm256)
            };
        }
    }
}

impl AddAssign for YmmRegister {
    #[inline]
    fn add_assign(&mut self, rhs: Self) {
        unsafe {
            self.mm256 = _mm256_add_epi32(self.mm256, rhs.mm256);
        }
    }
}

impl AddAssign<i32> for YmmRegister {
    #[inline]
    fn add_assign(&mut self, rhs: i32) {
        unsafe {
            let tmp = _mm256_set1_epi32(rhs);

            self.mm256 = _mm256_add_epi32(self.mm256, tmp);
        }
    }
}

impl Mul for YmmRegister {
    type Output = YmmRegister;

    #[inline]
    fn mul(self, rhs: Self) -> Self::Output {
        unsafe {
            YmmRegister {
                mm256: _mm256_mullo_epi32(self.mm256, rhs.mm256)
            }
        }
    }
}

impl Mul<i32> for YmmRegister {
    type Output = YmmRegister;

    #[inline]
    fn mul(self, rhs: i32) -> Self::Output {
        unsafe {
            let tmp = _mm256_set1_epi32(rhs);

            YmmRegister {
                mm256: _mm256_mullo_epi32(self.mm256, tmp)
            }
        }
    }
}

impl MulAssign for YmmRegister {
    #[inline]
    fn mul_assign(&mut self, rhs: Self) {
        unsafe {
            self.mm256 = _mm256_mullo_epi32(self.mm256, rhs.mm256);
        }
    }
}

impl MulAssign<i32> for YmmRegister {
    #[inline]
    fn mul_assign(&mut self, rhs: i32) {
        unsafe {
            let tmp = _mm256_set1_epi32(rhs);

            self.mm256 = _mm256_mullo_epi32(self.mm256, tmp);
        }
    }
}

impl MulAssign<__m256i> for YmmRegister {
    #[inline]
    fn mul_assign(&mut self, rhs: __m256i) {
        unsafe {
            self.mm256 = _mm256_mullo_epi32(self.mm256, rhs);
        }
    }
}

type Reg = YmmRegister;

// Locally replaced with an independently authored index-bit transpose.
// See FLIGHTSIM-PATCHES.md for original-source hashes and authoring evidence.
#[allow(clippy::too_many_arguments)]
// SPDX-License-Identifier: MIT OR Apache-2.0
/// Transpose the eight rows in place, preserving every 32-bit lane's bits.
///
/// # Safety
/// The caller must execute this function only on a CPU with AVX2 enabled.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn transpose(
    v0: &mut Reg,
    v1: &mut Reg,
    v2: &mut Reg,
    v3: &mut Reg,
    v4: &mut Reg,
    v5: &mut Reg,
    v6: &mut Reg,
    v7: &mut Reg,
) {
    #[cfg(target_arch = "x86")]
    use core::arch::x86::{_mm256_blend_epi32, _mm256_permute2x128_si256, _mm256_shuffle_epi32};
    #[cfg(target_arch = "x86_64")]
    use core::arch::x86_64::{_mm256_blend_epi32, _mm256_permute2x128_si256, _mm256_shuffle_epi32};

    // Exchange one row-index bit with the corresponding lane-index bit.
    // The shuffle flips that lane bit. The blend keeps the entries whose
    // two bits already agree, and takes the exchanged entries otherwise.
    macro_rules! exchange_bit {
        ($low:expr, $high:expr, $shuffle:expr, $mask:expr) => {{
            let low = $low;
            let high = $high;
            (
                _mm256_blend_epi32::<$mask>(low, _mm256_shuffle_epi32::<$shuffle>(high)),
                _mm256_blend_epi32::<$mask>(_mm256_shuffle_epi32::<$shuffle>(low), high),
            )
        }};
    }

    // Swap row bit 0 and lane bit 0: lane permutation [1, 0, 3, 2].
    let (a0, a1) = exchange_bit!(v0.mm256, v1.mm256, 0xb1, 0xaa);
    let (a2, a3) = exchange_bit!(v2.mm256, v3.mm256, 0xb1, 0xaa);
    let (a4, a5) = exchange_bit!(v4.mm256, v5.mm256, 0xb1, 0xaa);
    let (a6, a7) = exchange_bit!(v6.mm256, v7.mm256, 0xb1, 0xaa);

    // Swap row bit 1 and lane bit 1: lane permutation [2, 3, 0, 1].
    let (b0, b2) = exchange_bit!(a0, a2, 0x4e, 0xcc);
    let (b1, b3) = exchange_bit!(a1, a3, 0x4e, 0xcc);
    let (b4, b6) = exchange_bit!(a4, a6, 0x4e, 0xcc);
    let (b5, b7) = exchange_bit!(a5, a7, 0x4e, 0xcc);

    // Swap row bit 2 and lane bit 2 by joining corresponding 128-bit halves.
    // 0x20 selects [low(first), low(second)]; 0x31 selects their high halves.
    v0.mm256 = _mm256_permute2x128_si256::<0x20>(b0, b4);
    v1.mm256 = _mm256_permute2x128_si256::<0x20>(b1, b5);
    v2.mm256 = _mm256_permute2x128_si256::<0x20>(b2, b6);
    v3.mm256 = _mm256_permute2x128_si256::<0x20>(b3, b7);
    v4.mm256 = _mm256_permute2x128_si256::<0x31>(b0, b4);
    v5.mm256 = _mm256_permute2x128_si256::<0x31>(b1, b5);
    v6.mm256 = _mm256_permute2x128_si256::<0x31>(b2, b6);
    v7.mm256 = _mm256_permute2x128_si256::<0x31>(b3, b7);
}
