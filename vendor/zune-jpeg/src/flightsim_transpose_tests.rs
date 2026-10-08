// SPDX-License-Identifier: MIT OR Apache-2.0
#![cfg(any(target_arch = "x86", target_arch = "x86_64"))]
// Standalone validation scaffold; replacement.rs is the integration artifact.
#[cfg(target_arch = "x86")]
use core::arch::x86::{_mm256_loadu_si256, _mm256_storeu_si256};
#[cfg(target_arch = "x86_64")]
use core::arch::x86_64::{_mm256_loadu_si256, _mm256_storeu_si256};

use crate::unsafe_utils_avx2::YmmRegister;
type Reg = YmmRegister;
pub type Matrix = [[i32; 8]; 8];
pub type Registers = [Reg; 8];

#[allow(clippy::too_many_arguments)] // Same API for a fair baseline.
pub mod array {
    use super::Reg;
    include!("flightsim_array_transpose.rs");
}

/// Mathematical definition, deliberately separate from SIMD lane operations.
pub fn reference(input: &Matrix) -> Matrix {
    core::array::from_fn(|column| core::array::from_fn(|row| input[row][column]))
}

/// # Safety
/// Requires AVX2 support at runtime.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn encode(input: &Matrix) -> Registers {
    input.map(|row| Reg {
        mm256: _mm256_loadu_si256(row.as_ptr().cast()),
    })
}

/// # Safety
/// Requires AVX2 support at runtime.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn decode(registers: &Registers) -> Matrix {
    let mut rows = [[0i32; 8]; 8];
    for (row, register) in rows.iter_mut().zip(registers) {
        _mm256_storeu_si256(row.as_mut_ptr().cast(), register.mm256);
    }
    rows
}

/// # Safety
/// Requires AVX2 support at runtime.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn apply_simd(registers: &mut Registers) {
    let [v0, v1, v2, v3, v4, v5, v6, v7] = registers;
    crate::unsafe_utils_avx2::transpose(v0, v1, v2, v3, v4, v5, v6, v7);
}

/// # Safety
/// Requires AVX2 support at runtime.
#[target_feature(enable = "avx2")]
#[inline]
pub unsafe fn apply_array(registers: &mut Registers) {
    let [v0, v1, v2, v3, v4, v5, v6, v7] = registers;
    array::transpose(v0, v1, v2, v3, v4, v5, v6, v7);
}

#[path = "flightsim_transpose_tests_body.rs"]
mod tests;
