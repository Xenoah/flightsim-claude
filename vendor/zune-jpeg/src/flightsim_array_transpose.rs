// SPDX-License-Identifier: MIT OR Apache-2.0
/// Array/store/load transpose used as an independently authored baseline.
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
    use core::arch::x86::{_mm256_loadu_si256, _mm256_storeu_si256};
    #[cfg(target_arch = "x86_64")]
    use core::arch::x86_64::{_mm256_loadu_si256, _mm256_storeu_si256};

    let registers = [v0, v1, v2, v3, v4, v5, v6, v7];
    let mut rows = [[0i32; 8]; 8];
    let mut columns = [[0i32; 8]; 8];
    for (row, register) in rows.iter_mut().zip(registers.iter()) {
        // Each row is initialized, writable, and exactly 32 bytes long.
        _mm256_storeu_si256(row.as_mut_ptr().cast(), register.mm256);
    }
    for (r, row) in rows.iter().enumerate() {
        for (c, value) in row.iter().enumerate() {
            columns[c][r] = *value;
        }
    }
    for (register, column) in registers.into_iter().zip(columns.iter()) {
        // The unaligned load reads exactly the initialized 32-byte column.
        register.mm256 = _mm256_loadu_si256(column.as_ptr().cast());
    }
}
