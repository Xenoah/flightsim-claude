# Authoring provenance and limitations

Prepared on 2026-10-08 UTC by an OpenAI assistant in an isolated task directory
as local release-remediation preparation for Xenoah/flightsim-claude.

## Inputs used

- The delegating task's public API specification: `YmmRegister` containing a
  `pub(crate) mm256: __m256i` field, alias `Reg`, eight distinct mutable row
  references, and the `#[target_feature(enable = "avx2")]` / `#[inline]` function
  contract
- The mathematical definition of an 8×8 transpose: output row `c` contains
  input column `c`, with every 32-bit payload preserved
- General programming and SIMD knowledge; the row/column index-bit derivation
  documented in `README.md` was developed for this task
- Official Rust documentation describing individual primitives, read on
  2026-10-08, and the recovered Rust 1.93.0 compiler/toolchain for validation
- The local CPU's feature/identity information and assembly emitted from this
  newly authored source, solely to verify runtime behavior and performance

No existing project transpose body, external transpose example, Stack Overflow
answer, prior origin dossier, or project implementation source was read by this
authoring worker. The only project-specific code knowledge supplied was the
integration interface above. All authored source, tests, benchmark, and report files are confined to the new
isolated task directory. There were no external writes or publications.

The source was written for this task from the stated interface and index
mapping. The array candidate, mathematical reference, test inputs, and benchmark
were also authored for this task without copying a transpose implementation.
The official documentation's intrinsic signatures and behavioral descriptions
were consulted; example transpose code was neither sought nor used. The shuffle
primitive's official page contains individual-intrinsic examples, which are not
matrix-transpose implementations; none was copied into the deliverable.

## Official documentation consulted

- [32-bit within-half lane shuffle](https://doc.rust-lang.org/core/arch/x86_64/fn._mm256_shuffle_epi32.html)
- [32-bit lane selection](https://doc.rust-lang.org/core/arch/x86_64/fn._mm256_blend_epi32.html)
- [128-bit half selection](https://doc.rust-lang.org/core/arch/x86_64/fn._mm256_permute2x128_si256.html)
- [Unaligned 256-bit load](https://doc.rust-lang.org/core/arch/x86_64/fn._mm256_loadu_si256.html)
- [Unaligned 256-bit store](https://doc.rust-lang.org/core/arch/x86_64/fn._mm256_storeu_si256.html)
- [Runtime x86 feature detection](https://doc.rust-lang.org/std/macro.is_x86_feature_detected.html)

The current documentation pages identify newer Rust versions, while these
stable primitives and the delivered syntax were compiled and executed with
Rust 1.93.0. No package dependency or code was downloaded from these pages.

## Scope of claim

This is an honest record of this worker's authoring inputs and actions, not
formal clean-room certification, independent legal review, or a guarantee about
training-data provenance or the absence of every coincidentally similar routine.
The mathematical operation and low-level primitives constrain possible solutions.
No fictional human author or copyright holder is assigned.

The proposed replacement and scaffold are marked `MIT OR Apache-2.0` to match
the existing project terms specified by the parent task. Final acceptance,
vendoring, notice/license integration, decisions about removing any old notices,
full-project testing, and any publication belong to the project maintainer and
the parent task. This worker did not alter those project records.
