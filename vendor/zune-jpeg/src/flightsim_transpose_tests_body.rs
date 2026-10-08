// SPDX-License-Identifier: MIT OR Apache-2.0
use super::*;

fn avx2_available() -> bool {
    let available = std::is_x86_feature_detected!("avx2")
        && std::env::var_os("TRANSPOSE_TEST_DISABLE_AVX2").is_none();
    if !available {
        eprintln!("SKIP: AVX2 tests require runtime AVX2 support (or forced skip is set)");
    }
    available
}

// All calls into the AVX2 scaffold occur only after avx2_available().
unsafe fn check(input: Matrix) {
    let expected = reference(&input);
    let mut simd = encode(&input);
    let mut array = simd;
    apply_simd(&mut simd);
    apply_array(&mut array);
    assert_eq!(decode(&simd), expected, "bit-exchange transpose");
    assert_eq!(decode(&array), expected, "array/store/load transpose");
}

// Small reproducible generator, not a cryptographic primitive.
fn next(state: &mut u64) -> i32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    (*state >> 32) as u32 as i32
}

fn random_matrix(state: &mut u64) -> Matrix {
    core::array::from_fn(|_| core::array::from_fn(|_| next(state)))
}

#[test]
fn all_64_lane_basis_matrices() {
    if !avx2_available() {
        return;
    }
    for row in 0..8 {
        for column in 0..8 {
            let mut matrix = [[0; 8]; 8];
            matrix[row][column] = -1;
            unsafe { check(matrix) };
        }
    }
}

#[test]
fn all_2048_single_bit_basis_matrices() {
    if !avx2_available() {
        return;
    }
    for row in 0..8 {
        for column in 0..8 {
            for bit in 0..32 {
                let mut matrix = [[0; 8]; 8];
                matrix[row][column] = (1u32 << bit) as i32;
                unsafe { check(matrix) };
            }
        }
    }
}

#[test]
fn position_labels_prove_row_column_direction() {
    if !avx2_available() {
        return;
    }
    let matrix = core::array::from_fn(|r| core::array::from_fn(|c| (100 * r + c) as i32));
    unsafe { check(matrix) };
}

#[test]
fn dense_bit_pattern_matrices() {
    if !avx2_available() {
        return;
    }
    let patterns = [
        0,
        -1,
        i32::MIN,
        i32::MAX,
        0x5555_5555,
        0xaaaa_aaaau32 as i32,
        0x0123_4567,
        0x89ab_cdefu32 as i32,
    ];
    for pattern in patterns {
        unsafe { check([[pattern; 8]; 8]) };
    }
    for offset in 0..8 {
        let matrix = core::array::from_fn(|r| {
            core::array::from_fn(|c| {
                patterns[(r * 3 + c + offset) % patterns.len()].rotate_left((r * 8 + c) as u32)
            })
        });
        unsafe { check(matrix) };
    }
}

#[test]
fn deterministic_random_scalar_reference_equivalence() {
    if !avx2_available() {
        return;
    }
    let mut state = 0x67d0_cafe_5319_24a1u64;
    for _ in 0..20_000 {
        unsafe { check(random_matrix(&mut state)) };
    }
}

#[test]
fn involution_restores_every_input_bit() {
    if !avx2_available() {
        return;
    }
    let mut state = 0x08fa_f301_9231_6bd5u64;
    for _ in 0..4_096 {
        let input = random_matrix(&mut state);
        unsafe {
            let mut simd = encode(&input);
            let mut array = simd;
            apply_simd(&mut simd);
            apply_simd(&mut simd);
            apply_array(&mut array);
            apply_array(&mut array);
            assert_eq!(decode(&simd), input);
            assert_eq!(decode(&array), input);
        }
    }
}

#[test]
fn xor_linearity_preserves_combined_bit_patterns() {
    if !avx2_available() {
        return;
    }
    let mut state = 0xbed0_6291_891f_a33du64;
    for _ in 0..1_024 {
        let left = random_matrix(&mut state);
        let right = random_matrix(&mut state);
        let combined = core::array::from_fn(|r| core::array::from_fn(|c| left[r][c] ^ right[r][c]));
        unsafe {
            let mut a = encode(&left);
            let mut b = encode(&right);
            let mut c = encode(&combined);
            apply_simd(&mut a);
            apply_simd(&mut b);
            apply_simd(&mut c);
            let a = decode(&a);
            let b = decode(&b);
            let c = decode(&c);
            for r in 0..8 {
                for column in 0..8 {
                    assert_eq!(c[r][column], a[r][column] ^ b[r][column]);
                }
            }
            assert_eq!(c, reference(&combined));
        }
    }
}
