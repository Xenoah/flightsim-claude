# Actual JPEG decode regression for the original transpose

This separate validation workspace never changes application features. It uses
the modified local zune source in two builds: without its `x86` feature for the
scalar reference, and with `x86` for ordinary acceleration. The same explicit
fixture list is decoded in the same order and every output byte is compared.

```sh
cargo run --locked --offline --release --no-default-features \
  --manifest-path tools/validate-jpeg-replacement/Cargo.toml -- \
  write-reference /tmp/private-jpeg-reference docs/qa/images/*.jpg
cargo run --locked --offline --release \
  --manifest-path tools/validate-jpeg-replacement/Cargo.toml -- \
  compare-reference /tmp/private-jpeg-reference docs/qa/images/*.jpg
```

Both builds keep `std` and `neon`; only the first cfg-excludes all x86 routines.
Use an AVX2 host for the comparison and separately run the matrix tests, which
explicitly guard AVX2 entry. `DecoderOptions::new_safe()` alone was not used as
proof of scalar exclusion: observed core 0.5.1 options still permit AVX2 on the
validation host. The actual feature-disabled build is the reference here.

2026-10-08 result on the development x86_64 AVX2 host: 20 existing project JPEG
fixtures and 66,810,240 decoded channel bytes matched exactly. The fixtures were
not copied or generated anew. Keep decoded references private and untracked;
they are validation output, not release payload. This is a finite regression
corpus, not every valid/malformed JPEG or a Windows runtime qualification.
