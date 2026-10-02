# Opt-in commercial staging QA — 2026-10-02

## Scope

This validates an optional application/packaging path on top of source base
`08717b0`. It does not release the application or establish commercial rights.
The existing developer build and Windows release workflow remain separate.
See [the staging instructions](../release/commercial-candidate-staging.md) and
[the rights audit](../release/commercial-distribution-audit.md).

## Code checks

Executed with Rust 1.93.0, Cargo 1.93.0, `--offline --locked -j2`, warnings denied,
dev/test debug information disabled, and incremental compilation disabled:

- `cargo test -p flightsim-app`: **126 passed**
- `cargo test -p flightsim-app --features commercial-staging`: **126 passed**
- App `cargo clippy --all-targets -- -D warnings`, with and without the feature:
  **passed**
- `cargo fmt --all --check`: **passed**
- `bash scripts/check-architecture.sh`: **passed**
- `python -m unittest discover -s scripts/tests -p test_stage_commercial_candidate.py`:
  **20 passed**
- Combined finalized rights collector/checker plus staging tests: **52 passed**

Focused regressions cover compiled profile metadata, selected model/axes/length,
default versus explicit missing-model handling, no developer ancestor search,
absolute/parent model paths and canonical symlink escapes, and unchanged Light
Single fingerprints with no silent replay substitution. Existing profile/FDM,
replay, world-map, restart and presentation tests ran in both app configurations.
Python tests cover allowlisted copying, notice inclusion, all-file hashes,
blocked review candidates, integrity failures, GNU/MSVC separation, malformed
metadata, symlinks, path traversal, unreferenced/renamed/binary notice payloads,
and preserving existing candidate directories.

## Actual executable checks

The feature-built executable was copied into a fresh runtime directory containing
only `assets/aircraft/swift_sport.glb` and `swift_sport.json`. The Meshy model was
not present. The executable was invoked directly from an unrelated working
directory, without `--aircraft`:

```sh
flightsim-app --headless-screenshot swift-only-chase.png --screenshot-delay 5 \
  --view chase --fly 300 --cloud-cover 0 --time 12:00
```

Observed on 2026-10-02 at 06:31 UTC:

- Structured metadata reported `commercial-staging`, Swift Sport, Linux x86_64
  GNU, and `release_authorized: false`
- Log reported the Swift Sport profile and piston sound, the adjacent original
  GLB, and a 7.12 m fitted length at scale 1.0000
- The terrain reached a complete 38-surface/95-bridge displayed cut before capture
- The 1280×720 PNG had a complete PNG terminator and the process exited **0**
- Pixels were inspected: the original aircraft exterior was visible in chase
  view over the runway/global scene. An independent reviewer inspected the same
  capture and log
- Explicit absent Light Single selection returned **2**, as did parent/absolute
  model overrides. The developer asset environment was deliberately present for
  these negative checks; it did not bypass adjacent-directory isolation
- The stager accepted the actual feature executable's JSON identity and rejected
  the actual ordinary developer executable, whose default remained Light Single

Evidence identifiers:

- Feature executable SHA-256:
  `97c696e3864eb60ab2e7c298f06476282ce760f94d30dcb14ba96aeb9b1515f4`
- `swift-only-chase.png` SHA-256:
  `46d2620b7077cb4241eb4c8038248e8b6d92903124f8030a97c691d0dc9a2032`
- Companion files: `exact-build-evidence.json`, `distribution-info.json`,
  `development-distribution-info.json`, `swift-only-chase.log`, and individual
  test/negative-CLI logs retained with the local QA artifact

The executable is the optimized **dev** profile, not a production release build.
The exact-build record includes source-file hashes and the feature/flag/compiler
combination. The metadata handshake is not independent source authentication.

## Runtime boundary

This is filesystem portability only. No Steam Linux Runtime, Steam Deck, older
distribution, Windows GPU, controller or audio qualification is claimed.

The actual scene used Mesa llvmpipe 25.0.7 (LLVM 19.1.7), Vulkan, and glibc 2.41.
The task-local native package set included ALSA 1.2.14, libudev 257.13,
libgcc 14.2.0, Vulkan loader 1.4.309.0, X11 1.8.12 and Wayland 1.23.1.
The executable's direct dynamic requirements were `libudev.so.1`,
`libasound.so.2`, `libgcc_s.so.1`, `libm.so.6`, `libc.so.6` and
`ld-linux-x86-64.so.2`. Its highest referenced GLIBC symbol version was 2.35;
that fact alone does not establish compatibility with a glibc 2.35 host or its
other libraries. Dynamic driver/windowing requirements also need deployment QA.

No OS libraries were copied into the candidate. No audio device was available,
so the piston log does not prove audible output. A screenshot and software
renderer exit do not establish a frame-rate target or flight-quality assessment.

## Complete candidate integration

The finalized rights audit (`db5be58`) and attribution clarification (`830b44e`)
were combined with the runtime/stager and exact-feature Linux notice inventory
(v3). Checker-only hardening `f94c08a` was then applied and rerun against the
same unchanged candidate; it derives unresolved questions from underlying
package/asset records and rejects inconsistent summary lists. The complete candidate included **694 hashed files, 155,608,528 bytes**
excluding the manifest's own bytes: the executable, Swift-only external assets,
world-data terms/provenance, font/LUT notices, audit/manifest, and exact dependency
notice texts. Every manifest-listed hash and size was checked after the smoke.

At 06:48 UTC the **actual complete candidate** was launched from an unrelated
working directory with the same chase-capture command and no aircraft override.
It selected Swift Sport, loaded/fitted the adjacent 7.12 m GLB, reached the complete
38-surface/95-bridge cut, wrote a complete 1280×720 PNG, and exited **0**.
Pixels were inspected. The executable hash was unchanged from the earlier runtime
smoke. `full-candidate-v2-chase.png` SHA-256:
`9647cc1b0999879d72a9921135f38b2c4866fe2e7032a77d6882a07abc3dfede`.

The real checker was rerun on the full candidate after execution. It reported
**zero integrity errors** and retained **five review blockers**:

1. Dependency license/nested/platform review has not been completed
2. `constgebra` 0.1.4 primary license text remains unresolved
3. `hexf-parse` 0.2.1 primary license text remains unresolved
4. The exact embedded AgX LUT grant remains unresolved
5. Embedded Blender Filmic LUT version/license provenance remains unresolved

The stager correctly returned **1**, wrote **READINESS BLOCKED**, and did not
claim or authorize a release. The checker's blocker exit was **2**. No positive
review-evidence file was manufactured or supplied.

A separate temporary copy was deliberately given the **actual excluded Meshy
GLB bytes**, renamed `third-party/licenses/extra/NOTICE`. The real checker
rejected it as an **integrity** failure. Injecting those bytes into the input
notice tree also made the real stager return **2 without creating a candidate**.
The original source and clean candidate were unchanged. Temporary payloads were
removed after the negative tests.

### Final documentation refresh

After documentation-only audit update `6b7ffa0`, candidate v3 was regenerated:
**694 indexed files, 155,608,739 bytes**, all hashes/sizes verified, with the same
zero-integrity/five-review result. Comparing every file to the smoked v2 proved
that only the audit Markdown, readiness report (blocker order), and bundle
manifest changed. Every executable/runtime asset byte is identical. The v2
renderer proof therefore applies to that identical runtime payload; v3 was not
claimed as a separately rerun graphics smoke. The comparison is preserved in
`candidate-v3-identity-comparison.json`, with `candidate-v3-final-gate.json`.

## Remaining release gates

The complete local candidate is verified for the bounded engineering behavior
above, and is still blocked from clearance. Target-platform release builds,
exact platform dependency review, real hardware QA, all outstanding rights
questions and Steam store/account/legal work remain separate. The optional
positive review-evidence path has synthetic test coverage but was not used for
this real candidate. Nothing here authorizes publishing.

## Exact frozen-source verification — c66eb98

The earlier source-base/runtime/v2/v3 results above are historical checkpoints.
The final executable and restaged candidate below bind the current proof to
frozen source `c66eb9838ee1c93a0aee0f74639018e0a468faa4`.

- Final `final-0734` run: **all 17 gates passed**; **909 headless + 701 native
  default Rust cases**, zero failures and two known native exclusions; **128
  commercial-feature app cases separately**, five doctests and **104 Python
  cases**. Strict default/commercial lint and docs, formatting/diff/architecture,
  both native builds and both Windows GNU all-target checks passed. Feature
  configurations are separate runs, not additional unique default cases
- `commercial-20261002/candidate-c66eb98` was staged from a clean detached source
  snapshot: **694 indexed files / 155,617,290 bytes**, excluding the manifest's
  own bytes. Every size and SHA-256 was verified; no extra payload or symlink
  existed. The Meshy GLB remained excluded and original Swift Sport remained the
  commercial default
- Exact executable SHA-256:
  `ea36a56eb5df5074dfda950e1de6629100a0902ae10e4c9ef56c86e88110e94a`
- The actual staged executable was launched directly from `/tmp`, without an
  aircraft override. It loaded the adjacent Swift Sport GLB, fitted 7.12 m,
  reached **38 exact displayed/live/last-desired surfaces and 95 visible bridges**,
  saved a complete **1280×720 PNG**, and exited **0** at 07:44:48 UTC. Pixels were
  inspected; this is an actual full-candidate smoke, not hash equivalence alone
- `candidate-c66eb98-chase.png` SHA-256:
  `b1199f2d8e3c3f8f19d0801d066d8ebc731866ad32de2e4b919bf05df599a11c`
- Final gate retained **zero integrity errors / five review blockers**: dependency
  license/nested/platform review, `constgebra` 0.1.4 primary notice, `hexf-parse`
  0.2.1 primary notice, embedded AgX grant, and Blender Filmic provenance.
  Stager exit **1** and checker exit **2** correctly kept **READINESS BLOCKED**

Companion evidence under `commercial-20261002/` is
`candidate-c66eb98-evidence.json`, `candidate-c66eb98-final-gate.json` and
`candidate-c66eb98-chase.log/.png`. This remains an optimized dev-profile Linux
software-Vulkan run with no audio device. It does not qualify Windows execution,
MSVC linking, physical GPU/controller/audio, Steam Runtime/Deck compatibility,
commercial rights or release approval. No executable/raw replay upload or
publication was performed. See [the integrated QA record](global-map-integration-2026-10-02.md)
for final world-map and polar-scene evidence and its remaining limitations.
