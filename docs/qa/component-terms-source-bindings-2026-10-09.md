# Component-terms source bindings, 2026-10-09

This bounded source migration follows the component-terms implementation approved
by the user. It establishes exact source identity only. It neither reuses old
build evidence nor supplies Windows execution, dialog interaction, rights review,
archive verification or publication evidence.

## Exact changed and retained inputs

Base source is `4d40f9abbfa90be38c91f2a1379d8355530dc2f5`. The previous full runtime
composition remains `960c3126e6a4bc22b8d4acc6e6737f8f2b473bef`, tree
`b3bbd57bc3819b1b1eda35ce4f8b476c52d1b24e`. All current contracts label these
values `base_reviewed_source` and `base_reviewed_runtime_tree`; these are not
claims that the changed runtime has the previous tree.

`scripts/component-terms-source-migration.json` is an exact-hash manifest:

- Only `crates/flightsim-app/src/main.rs` changes among the 497 preserved inputs
- Its previous complete bytes remain at
  `scripts/history/4d40f9a-flightsim-app-main.rs`, SHA-256
  `160cc900acbb0029452c4175fee9ba24bc26611ce41faaa57a97a8426b97811a`
- The manifest records the sole old-to-new main hash transition and exactly two
  added compiled inputs: `component_terms.rs` and `component_terms_dialog.ps1`
  under `crates/flightsim-app/src/`
- Exactly three complete added documents are bound under
  `docs/release/components/`: `MICROSOFT-COMPONENT-TERMS.txt`,
  `MICROSOFT-COMPONENT-TERMS.ja.txt` and `MICROSOFT-COMPONENT-NOTICE.txt`
- Existing unchanged `LICENSE-MIT` and `LICENSE-APACHE` become exact source pins
  because the dialog now embeds and includes their complete bytes in its receipt

The frozen `scripts/full-two-aircraft-runtime-pins.json` bytes and all 497 old
hashes remain unchanged. Its only relocated identity is the old main; the other
496 stay at their original paths. All 404 historical source boundary members,
three `d918943` historical contract files, 102 independent replay anchors and 55
core-pipeline members remain unchanged. There is no fixture/golden regeneration,
byte projection, normalization or broad pin refresh.

The current replay boundary expands from 918 to 927 complete files: seven newly admitted
runtime/document/license inputs, the migration manifest and retained historical main.
The crate boundary remains closed at the old 492 paths plus exactly the two new
inputs. The three component documents also form a closed directory. Existing
vendor, witness, case-alias, symlink/reparse, ignored/untracked input and
Cargo-auto-discovered target rejection rules remain in force.

## Dependent contracts

Replay, analytical and capture admission have separate component-terms identities.
Their `source_migration_sha256` binds the same complete migration manifest. The
replay checker requires the literal sole relocation, original main hash, final
main hash, exact seven added pins and all frozen runtime identities. The ordinary
legacy fingerprints, FDM laws, replay rules and independent anchors are intact.

The analytical contract retains its original 35 path members and adds the seven
component inputs, focused migration test, real Windows dialog/control test and
this document, totaling 45. Only
actually changed source/tool/test/document hashes advance; all other hashes are
retained. The capture boundary remains its existing four files, and its runner
and contract inherit the two exact updated contract hashes. Build recipes,
features, source-LUT exclusions and capture qualification semantics are unchanged.

The normal copy plan separately adds the three exact component documents. It
does not add a new runtime dependency or expand the binary/model member set.

## Verification scope

Focused source tests exercise the genuine `capture.source_evidence` chain on a
clean committed disposable source fixture, then reject committed changes to
main, both new runtime files, all three terms documents, both project licenses, historical main and the
migration manifest. Exported evidence attacks cover omitted files/evidence,
canonical or checkout substitutions, and resealed contract pins. Literal path
checks reject omitted members, case aliases and extra inputs; an actual ignored
alternative terms file is rejected in a clean checkout.

These fixtures establish rejection behavior only and cannot be production
native, build, inventory or publication evidence. A final-source acceptance call
must run separately on the actual consolidated clean commit. The complete final
Windows/MSVC build and extracted-bundle gate must bind that same final source;
`55094fa` evidence remains historical reference only.
