# Independently authored AVX2 8×8 i32 transpose candidate

The integration candidate is `src/replacement.rs`. It preserves the supplied
`pub unsafe fn transpose(v0: &mut Reg, …, v7: &mut Reg)` API, `#[inline]`,
`#[target_feature(enable = "avx2")]`, and the requirement that each argument
contains a row of eight 32-bit lanes. Copy this function into the existing module
where `Reg` names the supplied `YmmRegister` type. Its intrinsic imports are local
to the function and support both x86 and x86_64. No dependency is added.

`src/array_candidate.rs` is a separate store/array/load implementation of the
same API for comparison. `src/lib.rs` is only the standalone test scaffold.
Nothing here changes the project checkout, third-party notices, or release state.

## Mathematical construction

A cell has row index `(r2, r1, r0)` and column/lane index `(c2, c1, c0)`.
Transposition exchanges these two three-bit indices. This implementation performs
three separate swaps: `(r0, c0)`, then `(r1, c1)`, then `(r2, c2)`.

For an index bit `k`, let `d = 1 << k`. Pair the current rows whose row indices
differ only in bit `k`, calling the bit-zero row `L` and the bit-one row `H`.
Their new values at lane `c` must be:

- `L'[c] = L[c]` when `c & d == 0`, otherwise `H[c ^ d]`
- `H'[c] = L[c ^ d]` when `c & d == 0`, otherwise `H[c]`

These expressions are an exchange of the row and lane bits, not arithmetic on
matrix values. All other address bits remain fixed.

For `k = 0`, the within-half lane permutation is `[1, 0, 3, 2]`, encoded as
`0xb1`; lane-selection mask `0xaa` chooses odd lanes. For `k = 1`, the
permutation is `[2, 3, 0, 1]`, encoded as `0x4e`; mask `0xcc` chooses lanes
whose bit 1 is set. Both permutations repeat in each 128-bit half.

For `k = 2`, the same equations say to join the low halves of the pair and then
the high halves of the pair. `permute2x128` immediates `0x20` and `0x31` do that.

An explicit invariant is useful for review. After exchanging the bit positions
in mask `S`, output location `(r,c)` contains the original input at:

`row = (r & ~S) | (c & S)`

`column = (c & ~S) | (r & S)`

The three stages use `S = 1`, then `3`, then `7`. Since all indices are between
0 and 7, the final location is original input `(c,r)`. Every operation selects
or rearranges complete 32-bit lanes, so all 32 payload bits are preserved.
There are no data-dependent branches or allocations in the candidate.

## Validation performed

On x86_64 with runtime AVX2 support, all seven test groups passed in debug,
release, and a direct `rustc --test` build:

1. All 64 lane-basis matrices, each with one all-ones lane
2. All 2,048 single-bit basis matrices: 64 positions × 32 payload bits
3. Unique row/column position labels
4. Eight uniform extreme/alternating bit patterns and eight mixed-pattern matrices
5. 20,000 deterministic pseudorandom matrices against the mathematical reference
6. 4,096 pseudorandom involution checks (`T(T(M)) = M`)
7. 1,024 pseudorandom XOR-linearity checks, also checked against the reference

Tests 1–6 exercise both candidate implementations. Test 7 additionally exercises
the recommended SIMD candidate. `reference` is defined directly as
`output[column][row] = input[row][column]`, independently of the shuffle network.

Each SIMD test first checks `is_x86_feature_detected!("avx2")`. Unsupported CPUs
return before any AVX2 function call and print a skip message. The forced-skip
path was also exercised via `TRANSPOSE_TEST_DISABLE_AVX2=1`. Rust's built-in test
runner reports such early-return tests as passed; that forced-skip run is evidence
of the guard path, not evidence of SIMD correctness on a non-AVX2 CPU.

`cargo fmt --check` and `cargo clippy --all-targets -- -D warnings` passed. The
scaffold permits Clippy's `too_many_arguments` lint only on the two modules with
the required eight-reference integration API. There are no added dependencies.

The x86 import path is provided, but this environment only has x86_64 Rust
standard libraries installed: i686 compilation/execution was not performed.
This standalone validation is not a replacement for the application's full tests
and integration/performance checks after vendoring.

## Measured performance

Environment: Rust 1.93.0, LLVM 21.1.8, x86_64 Linux, AMD EPYC 9V74 virtualized
host. Builds use optimization level 3, one codegen unit, no LTO, no
`target-cpu=native`, and no AVX-512 requirement. CPU affinity was not pinned.

The benchmark uses 15 alternating-order samples per size, warmup passes,
`black_box` on each matrix reference, and correctness checks before and after
timing. Each sample performs approximately 2.1 million transposes. The one-matrix
case repeatedly depends on its previous result; the other cases cycle through a
16 KiB or 1 MiB corpus. The figures include loop/load/store/barrier overhead.

Final run (`evidence/bench-2.txt`), median nanoseconds per 8×8 transpose:

| Matrix count | SIMD candidate | Array/store/load | Array time / SIMD time |
| ---: | ---: | ---: | ---: |
| 1 | 6.2849 ns | 10.1584 ns | 1.6163× |
| 64 | 3.3656 ns | 8.0958 ns | 2.4055× |
| 4,096 | 3.7016 ns | 7.8314 ns | 2.1157× |

A preceding run (`evidence/bench-1.txt`) measured ratios of 1.6979×, 2.4723×,
and 2.0731×, respectively. These are local microbenchmarks against the newly
authored array baseline, **not against the existing third-party routine**, and
not a promise of application-level speedup. Virtualization, shared-host load,
compiler choices, and CPU model can affect the measurements.

The generated SIMD benchmark inner loop was inspected. LLVM retains the lane
exchanges but combines some operations, including the bit-1 stage into
64-bit unpacks. The source contains 40 shuffle/blend/half-select intrinsics;
this optimized kernel contains 32 lane-movement instructions, plus loads and
stores. Some use floating-point shuffle instruction names, but they perform
bit movement only, without floating-point arithmetic or NaN conversion.
The checked assembly excerpt is `evidence/benchmark-kernels.s`.

## Reproduce

Set `PATH` to a Rust 1.93-compatible toolchain, then run from this directory:

```sh
cargo fmt --check
cargo clippy --offline --all-targets -- -D warnings
cargo test --offline -- --nocapture
cargo test --release --offline -- --nocapture
TRANSPOSE_TEST_DISABLE_AVX2=1 cargo test --offline -- --nocapture
cargo bench --offline --bench transpose
rustc --edition=2021 --test src/lib.rs -o /tmp/original-transpose-tests
/tmp/original-transpose-tests --nocapture
```

See `PROVENANCE.md` for authoring inputs and limitations. Source-file hashes and
text evidence hashes are recorded in `SHA256SUMS`.
