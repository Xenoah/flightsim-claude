# Host-independent dependency-notice discovery order

This is a bounded collector correction for alpha.22 preparation. It changes no
inventory comparison, acceptance rule, notice bytes, licence choice or release
permission. No Windows application build or execution is supplied by this record.

## Observed difference

The previous collector uses `sorted(source.rglob("*"))`. Python's Windows Path
ordering folds case, while POSIX Path ordering does not. Real Rust 1.93.0 ordinary
metadata filtered for `x86_64-pc-windows-msvc`, collected on Linux, produced the
same complete notice entries but a different array order from the historical
source550 Windows inventory for `cosmic-text@0.16.0` and `zune-jpeg@0.5.15`.
The original Windows inventory remains historical evidence with SHA-256
`ee835131ecbe10f52d7a35034183486e68b8e73e2efbfc29e4dd1536eed9da74`.
The exact inventory comparison correctly rejects these array differences; its
policy is unchanged.

The current discovery sort uses each path's exact case-sensitive POSIX relative
spelling. It neither case-folds names nor sorts/normalizes an existing inventory.
Generated README/inventory text explicitly uses LF. This also removes the
observed default-newline difference for generated README bytes, which the
unchanged exact notice comparison correctly rejects. It never re-encodes copied
upstream notices. All candidate discovery, primary-notice checks, supplements, original-byte copies,
hashes, unresolved observations and `review_status: not_reviewed` remain intact.

## Real collection and regression evidence

The original collector and changed collector were both executed with the same
actual target-filtered metadata and source bytes. Each produced 359 package
records, the same two unresolved constgebra/hexf primary-notice observations,
and 737 original files. Their entire output trees are byte-identical on this
Linux host. Both original and regenerated output trees are retained separately.
Their inventory is 407,610 bytes, SHA-256
`9028d70de8e818f2e72666a2686b198424cf4f78986b47c5d5683091f1949730`.
This is a genuine Linux-host target inventory, not a native Windows observation.

Five focused tests exercise the production sort key with both PurePosixPath and
PureWindowsPath. They reproduce the old flavor-dependent order, require exact
new cross-flavor order, preserve case/spelling, ignore checkout-prefix changes,
and run the actual collector against notice fixtures with an emulated Windows
default-CRLF writer. Explicit LF must survive that writer for both generated
outputs while upstream CRLF bytes stay exact. The latter also checks every
output byte/hash, unchanged inputs, retained `not_reviewed` status and rejection
of reuse of a nonempty output directory. These tests are synthetic behavioral
witnesses, not production native or approval receipts.

The version-only source migration separately covers just Cargo.toml/Cargo.lock.
This collector correction and its test/document are explicitly bound by the
analytical source contract; prior contract bytes remain preserved. The fresh
Windows producer must independently recollect all originals and satisfy the
unchanged exact inventory, same-build native, applicability, archive and smoke
gates. Historical Windows arrays are not rewritten or relabeled.
