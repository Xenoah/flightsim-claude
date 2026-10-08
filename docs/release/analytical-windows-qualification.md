# Prepared analytical Swift Windows qualification path

Status: **local tooling proposal, native execution and independent boundary
review outstanding**. Nothing here is a successful Windows run, subjective
appearance acceptance, whole-target rights review or publication receipt.
The reduced Swift-only/Reinhard release choice is pending. No choice or approval
is inferred from silence. The authorized fuller release remains separately
subject to its existing two-aircraft gates.

This additive path preserves the ordinary build, all three existing frozen
contracts, the sole stager, both asset allowlists, all admission/replay guards,
and existing release/readiness/authorization commands. It does not call the
release workflow or create a positive review. It deliberately does not claim
that a new analytical build cures the ordinary Windows screenshot failure.

## Scope and evidence ownership

The exact build remains Rust 1.93.0, native `x86_64-pc-windows-msvc`, release,
locked/offline, jobs 2, `RUSTFLAGS=-D warnings`, incremental off, app defaults off,
and exactly `analytic-tonemapping,commercial-staging`. The unchanged
`capture-analytical-swift-msvc.py` builds both the analytical artifact and its
ordinary LUT-positive control and invokes the unchanged exact-recipe auditor.
Its frozen build trees are never reused for tests or fixture generation.

Included external aircraft files remain the original Swift GLB/profile pair.
Embedded global terrain and monthly climate and all stager-required notices
remain present. The unresolved Light Single model, other aircraft payloads,
and all three tonemapping LUT payloads are excluded. Optional network terrain
downloads remain off. The stager still copies its existing Tony licence notice;
that conservative notice does not mean the Tony LUT was embedded. Reinhard is
darker and changes highlights. This is not appearance parity or completion of
two-aircraft, physical-GPU, controller, speaker or store qualifications.

The isolated workflow first checks exact-source main CI, then has two independent
Windows jobs tied to the same exact `github.sha`:

1. `regressions`: run app/render tests and Clippy under combined analytical plus
   commercial features, ordinary app/render controls, actual `tonemapping_modes`
   in both render modes, all four exact fingerprint/legacy tests, and mixed/
   neither-mode rejection with the exact compile-error diagnostic. The mode-less
   render library remains a passing control. The two independent replay Python
   encoders also execute. Every exact acceptance test must report its full name,
   `running 1 test`, and one passing result. Zero filtered matches do not pass.
2. `runtime`: after the same source-CI gate, make fresh private dual builds using
   the unchanged capture. Use the unchanged stager. Verify its entire source
   and notice copy plan by bytes, the manifest, ZIP CRC/membership and extraction.
   Reject excluded Light bytes under any staged name. Verify the extracted
   deterministic Swift-only/offline/MSVC handshake twice from an unrelated CWD,
   with `BEVY_ASSET_ROOT` and `CARGO_MANIFEST_DIR` pointing at the source tree as
   deliberate poison. Build the legacy fixture into another fresh target tree.

The two Windows jobs may run concurrently on separate fresh hosts. Neither
consumes the other's artifacts or build directories. Both retain their original
budgets and exact-source checks. Overall qualification requires both successful
same-source results; the publication preparation checker independently requires
complete regression evidence and complete final-bundle runtime evidence. If
regressions fail, any independently collected runtime facts remain nonauthorizing
partial material, and the overall workflow cannot succeed. Parallel execution
may use runner time even when the other phase eventually fails.

Required production runtime outcomes remain:

- Default Swift: actual adjacent GLB, 7.12 m fit/scale 1.0000, complete decoded PNG,
  no logged ERROR/panic/asset fallback, and process exit 0
- Explicit absent Light Single: missing-model diagnostic and exit 2 before graphics
- Default Swift with actual frozen legacy Light replay: hexadecimal original
  partial-fingerprint mismatch and exit 2
- Explicit Light/no-model with legacy opt-in: PNG, exit 0, original partial
  fingerprint and full missing-historical-yaw disclosure; the PNG/replay stay private

There are no retries or diagnostic invocations. A timeout, missing PNG, shader
error, wrong model or changed hash fails the phase. The production executable is
exactly the one bound by the unchanged artifact auditor. The old diagnostic's
executable, instrumentation and observations cannot qualify it.

## Bounded appearance material and remaining manual work

Eight fixed Swift captures are prepared: default chase, daylight cockpit,
05:30 low sun, 23:00 cockpit, modeled fog cockpit, high cloud, high water over
Tokyo Bay, and tower daylight. Each must independently satisfy the same strict
model/PNG/exit gate. The image set is only material for reviewing atmosphere,
water, weather, cockpit/HUD and distinct camera starts. It cannot assert image
quality or reproduce interaction.

Before appearance acceptance, a reviewer must inspect these actual Windows
images and explicitly assess the darker Reinhard tradeoff. A separate native
interactive session on the exact extracted executable must cover camera changes,
map open/edit/cancel/reopen/start, repeated Start/Cancel, pause/resume/restart,
narrow/resized cockpit HUD and legacy disclosure, and normal interactive close.
Record source/executable/bundle hashes, environment, actions, observations and
bounded relevant screenshots. The separate owned-window driver prepares that
interaction sequence; the initial batch harness does not claim its witness. Fallback D3D12 captures also do not verify physical
GPU drivers, controller reconnection or speaker listening.

The concrete interactive procedure is to use a connected native Windows QA
desktop, keep the exact private build/extraction on that machine, and verify
the executable and bundle hashes before starting from an unrelated CWD with
the same poisoned asset environment. Use C to cycle views; M to open the map;
edit a coordinate, apply only the edit with Enter, then Escape to cancel; reopen
and verify the old flight is intact; select the available Swift row and explicitly
Start with Enter; repeat Start/Cancel around preparation while observing the
actual result. Then exercise Escape pause/resume, R restart, resize to 1024x720
and 640x480 and back, replay's persistent partial-identity notice, and the normal
window close button. Record each actual outcome, errors and final process exit.
Use bounded captures of this application's client area only; do not capture
unrelated desktop windows. If the source is rebuilt on that desktop, its new
executable needs its own exact build audit and cannot borrow a previous hash.

If an interactive Windows executor is unavailable, the same-machine runner
route is the separately reviewed external Win32 owned-window driver.
It must select a single window by the production process ID, verify foreground
ownership before every bounded input, observe map/pause/start completion in
actual UI/log witnesses, and reject wrong focus, missing state or timeout.
Fixed timing alone is insufficient. The separate
[owned-window driver](analytical-final-bundle-and-ui.md) now implements that
bounded path for the unchanged production app, with actual state/log/size
validation; no probe flag or instrumentation is allowed. Its Windows capability
and actual appearance/lifecycle execution remain unqualified. Lack of a working
Windows interaction route remains a real blocker. The separate prior Linux
witness is useful procedure, not Windows proof.

## Native dependency review projection

`project-analytical-native-evidence.py` revalidates the authoritative private
capture before projecting at most 2 MiB of canonical ASCII JSON. It exports:

- Source/tree, compiler release/commit/host/target, exact feature identity and
  lock/metadata/inventory/graph/compiler-message/executable hash+byte bindings
- Every conservative package with name/version, registry checksum or workspace
  origin, recorded upstream revision, declared licence, conservative and exact
  features kept separate, bounded notice-relative paths and hashes, and bounded
  normal/build edges with target conditions
- Fixed, tracked source-header spans whose full source and exact byte ranges
  match the native package source; the bounded manifest includes cursor-icon W3C,
  bevy_mikktspace Zlib and the independently reviewed additional source notices.
  It is not exhaustive nested-source clearance
- PE32+ machine/subsystem and ordinary/delay-import DLL basenames, the entire
  actual stager shipped-file/hash set, and build-script requested native libraries

No absolute paths, arbitrary linker output, environment dump, source excerpt,
notice text, raw metadata, replay, model, LUT, executable, ZIP or target payload
is exported. A native DLL/import is an observed dependency, not automatically a
redistributed file or proof of an OS-version prerequisite. Only the application
PE is allowed in the unchanged bundle; any added native file needs separate
packaging and rights review.

Cargo linker requests and PE imports do not prove final static archive members.
That state is explicitly `not_established`. A complete, exact-version conservative
licence/notice set may establish runtime coverage without an exact member map;
the tool does not invent a requirement for `/MAP` or `/VERBOSE`, or add either
to the frozen recipe. The additive `collect-analytical-runtime-facts.py` now
collects a separate bounded factual packet: actual queried Rust identity and
sysroot notice/library hashes, default and supplied-settings cfg probes, and
installed VS/toolset/SDK notice and library candidates. Private raw command
streams and notice snapshots remain on the runner. Installed candidates are not
assertions about what the compiler selected; cfg probes do not observe the full
compiler invocation.

Only the build-capture environment gains the reviewed diagnostic variables
`RUSTC_LOG=rustc_codegen_ssa::back::link=info` and `RUSTC_LOG_COLOR=never`.
Compiler flags, profile, target, capture command and artifact checks are unchanged.
The strict parser binds exactly one constructed final link command to the audited
application `/OUT` and, where applicable, byte-identical Cargo deps output. Its
resolved `link.exe` identity is associated with an installed toolset only by exact
path. The Windows logger does not expose implicit `LIB` search paths. Missing,
ambiguous or unparseable observations stay unknown; exact SDK selection, selected
static archive members, loaded dynamic-module closure and whole-platform coverage
remain unestablished. No licence agreement is accepted and no DLL is packaged.

The factual packet is collected immediately after the original frozen build
audit, before staging or scenes. Then two 30-second OCR/desktop probes record
available/unavailable/unknown states. Missing capability preserves the independent
build/platform facts and does not stop batch screenshots. The overall state still
requires review and every lifecycle/appearance/qualification approval remains
false. The actual owned-window UI run separately requires both capabilities.
The final supplemented-bundle recheck collects its own facts and probes against
the same existing frozen build, without rebuilding or changing inventory identity.
These facts support conditional review; they cannot create whole-target approval.

The two narrow constgebra/hexf grants remain separate assessments. They cannot
substitute for the whole dependency/platform/font/shader/terrain/climate/marks
review. A source publication instruction is not third-party grant evidence.

The current audited collector tree is deliberately retained unchanged and is not
the final supplemental notice package. Source-header obligations and the Rust
runtime review have identified additional accompanying texts. The existing sole
stager can already accept those through a genuine inventory-bound
`--dependency-review` and its hash-bound evidence references; no bypass or change
to the allowlist is needed. The reviewed supplemental notice set must first be
prepared separately, then staged and verified with that existing option. This
initial harness has no positive-review input and therefore checks only its
unreviewed candidate bundle. The separate
[final supplemented-bundle recheck](analytical-final-bundle-and-ui.md) is now
implemented to consume a real completed reviewer record and exact supplemental
notice tree while retaining the audited build. Its actual Windows run and genuine
review inputs remain outstanding. A previously captured executable/PNG cannot
silently qualify an altered final bundle. No positive receipt is created here.

## Run plan and budgets

Before a remote launch, independently review this additive boundary and the
final exact source. Integrate any demonstrated readback repair first. A repair
that changes frozen files must undergo the documented semantic boundary review
and explicit whole-file pin migration, never an automatic hash refresh. Review
the actual delta against [the prior migration procedure](../qa/replay-candidate-tonemapping-cockpit-pin-review-2026-10-05.md).
This proposal makes no pin changes and does not launch a job.

The selected execution route is to publish the independently reviewed additive
tooling to main, wait for CI on that exact actual main commit, and only then point
`qualification/analytical-swift-reviewed` at the identical commit (or dispatch
that exact ref). The source built is `github.sha`, including the tooling. A bounded
read-only gate requires completed successful `.github/workflows/ci.yml` / `CI`
on the same SHA, same repository, `push` event and `main` branch. Ancestor, pull
request, foreign-repository and incomplete runs are rejected. Successful CI on
base `764b290f` or initial tooling `9e48220` does not cover a later integration.

No broader branch trigger is accepted. Qualification is not automatically
connected to main pushes, PRs, the ordinary candidate or release. It has read-only
contents/Actions permissions and no cache. `source-ci.json` is a bounded factual
CI binding, never a publication or visual acceptance receipt.

The regression phase has a 180-minute process budget (185-minute job cap).
The build/runtime phase has a 350-minute process budget (360-minute job cap),
including at most 300 minutes for the unchanged dual-build capture. Each
production screenshot retains its existing 180-second watchdog. Ordinary
commands have bounded timeouts, subprocess trees are terminated on timeout,
and actual statuses and exact streams remain private. Start needs 12 GiB free;
each command needs 2 GiB free. These are hard failure bounds, not performance
predictions or permission to silently reduce test scope.

The export directory permits only `qualification.json` (2 MiB maximum), optional
`native-review.json` (2 MiB maximum), `runtime-facts.json` (512 KiB maximum),
`ui-capabilities.json` (32 KiB maximum), and eight named Swift PNGs (64 MiB aggregate,
existing per-PNG decoding bounds). Independent revalidation ties these bytes to
the private journals, canonical source, exact build receipts and frozen trees.
The workflow lists every upload filename explicitly. If runtime fails after a
valid factual packet is prepared, those validated text facts may still export for conditional
review; no partial image set or passed runtime check is exported. A failed or
missing build never creates a native packet.

All authorization/qualification booleans remain literal false, including after
engineering evidence completes. The two jobs supply separate observations for
the first four gates; actual appearance acceptance, whole-target rights review,
the user's reduced-release choice and the genuine inventory-bound publication
receipt remain independent. A future distributable-release recipe needs separate
review and must not weaken or repurpose the fuller release's requirements.

Local checks, with no Windows execution:

```sh
python -m unittest discover -s scripts/tests -p test_analytical_swift_qualification.py -v
python scripts/qualify-analytical-swift-windows.py --describe
```

The unit tests use hostile synthetic PE/receipt fixtures and a tiny real Python
subprocess. They verify boundaries, never native application behavior.
