# Local original-equivalent transpose replacement

Upstream: zune-jpeg 0.5.15, revision 31d81fed7551c8ccea456d9d8e2b1fd8bebb6995.
Archive SHA-256: 27bc9d5b815bc103f142aa054f561d9187d191692ec7c2d1e2b4737f8dbd7296.
Original upstream expression: MIT OR Apache-2.0 OR Zlib. The unchanged
Cargo.toml.orig and FLIGHTSIM-UPSTREAM-SOURCE.json preserve that original fact.
The modified package selects/offers MIT OR Apache-2.0 because the new
project-authored replacement and tests use those existing project terms.
All upstream license choices/notices for retained code remain present; no Zlib
grant is asserted for the new replacement. The additional project licence copies
are named LICENSE-FLIGHTSIM-MIT and LICENSE-FLIGHTSIM-APACHE.

The AVX2 transpose in src/unsafe_utils_avx2.rs is replaced by an independently
authored row/column index-bit exchange implementation. The obsolete private
shuffle helper is removed. It retains the eight mutable register references,
AVX2 target feature, row/column semantics, and every 32-bit lane payload. No
JPEG features, decoder APIs or other runtime implementations are changed.
See FLIGHTSIM-TRANSPOSE-PROVENANCE.md and FLIGHTSIM-TRANSPOSE-README.md for the
bounded authoring record, mathematical derivation and measured standalone tests.
This is not a formal clean-room certification or a human legal opinion.

src/lib.rs adds only a test module. The three flightsim_* test files validate
the actual integrated transpose against a separate scalar mathematical oracle
and independently authored array candidate. Their runtime AVX2 guard skips
execution where AVX2 is unavailable. No copied prior transpose implementation
is present in the replacement, tests, patch evidence or source archive.

The original .cargo_vcs_info.json is omitted from this modified tree, because
its upstream revision would falsely describe a pristine current source snapshot.
The upstream revision/archive/member hashes are retained as provenance only;
the complete modified vendored tree must be independently hash-bound in the
final target inventory. Cargo.lock and all other original source members are
retained byte-for-byte except the explicitly listed modifications.

The third-party-notices directory preserves independently established notices
for retained stb MIT, libjpeg-turbo/IJG, libultrahdr Apache and Stanford individual
public-domain code. The IJG acknowledgement and observed Rust adaptation notice
are included. The separate Adobe upstream patent notice is retained without
claiming product-level patent coverage. No unrelated original author notices
are removed merely because the transpose implementation was replaced.

This local proposal is not a publication approval. Final integration requires
an independently reviewed modified-source manifest, fresh Cargo lock/metadata,
actual native inventory and applicable runtime/notice review, shader and JPEG
regressions, the unchanged release gates and an authorized publication receipt.
