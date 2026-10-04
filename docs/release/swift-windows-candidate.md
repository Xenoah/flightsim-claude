# Swift-only Windows engineering candidate

This separate candidate checks the existing `commercial-staging` distribution
policy on Windows MSVC. It does not change `release.yml`, its two-aircraft
payload/acceptance, any rights record, or publication authorization. A green
candidate result means **engineering checks passed**, never permission to ship.
The ordinary eight source-CI checks and this separate Windows run are distinct.

## Exact build and notice recipe

The supported appearance stays unchanged: ordinary default features plus
`commercial-staging`, including Bevy 0.18.1's full tonemapping LUT bundle.
AgX/Filmic and crate-review blockers remain recorded. A Tony-only build is not
claimed: without `tonemapping_luts`, upstream's HDR fullscreen pipeline emits
an unconditional error even if a Tony texture is supplied through public
`TonemappingLuts`. Our atmosphere forces HDR. No engine fork, error filtering,
magenta fallback, or altered appearance is introduced to bypass that check.

Run from a clean exact-source Windows checkout with Rust 1.93.0 and Python 3.12:

```powershell
python scripts/check-swift-windows-candidate.py `
  --expected-source (git rev-parse HEAD) `
  --work "$env:TEMP/swift-private-unique" `
  --evidence "$env:TEMP/swift-evidence-unique"
python scripts/check-swift-windows-candidate.py `
  --validate-evidence "$env:TEMP/swift-evidence-unique"
```

Both output directories must be new, outside the checkout, and separate. The
script fixes `RUSTFLAGS=-D warnings`, uses its own target directory, and executes:

```text
cargo +1.93.0 build --locked --release -j 2 --target x86_64-pc-windows-msvc -p flightsim-app --features commercial-staging
cargo +1.93.0 metadata --locked --format-version 1 --filter-platform x86_64-pc-windows-msvc --features flightsim-app/commercial-staging
```

It collects fresh notices from that actual metadata using the existing collector,
then checks metadata/lock/asset-manifest hashes and resolved candidate features.
The committed default-only 347-package/684-notice inventory cannot attest this
candidate, even if counts happen to agree. The collector always retains
`not_reviewed`; this recipe supplies neither a dependency-review record nor an
authorization receipt. Final-manifest changes require recollection.

Every tracked source input records its canonical Git blob identity and the
SHA-256/size of its actual checked-out bytes separately. The versioned
`scripts/replay-candidate-contract.json` freezes the reviewed implementation as
whole-file SHA-256 values, including replay codecs/identity/player/weather,
app acceptance/runtime/main/profile wiring, new-flight/weather/region helpers,
the actual persistent UI/map ownership and the regression tests used by this
recipe. The exact required path set is checked;
missing/extra entries fail. All listed raw Git blobs and the contract file itself
require byte-exact checkout equality; semantic changes and newline differences
both fail. Independently authored Python reference encoders and every existing
v1/v2/v3 golden fixture have separate unchanged pins in the checker. The original
five historical hashes remain provenance under their original baseline commit;
the obsolete monolithic replay hash is never relabeled as the migrated source.
The unchanged Light profile and FDM source pins remain enforced directly. The
workflow fixes process-local `core.autocrlf=false` **and** `core.eol=lf`, because
`text=auto` can otherwise use native Windows CRLF even with automatic conversion
disabled. Upstream notice paths marked `-text` retain their original bytes;
no source or notice content is rewritten or normalized by the checker.

The source SHA/tree, source inventory,
compiler identity/flags, commands, executable, private archive, bundle manifest,
metadata and dependency-inventory digests are recorded. Dirty/untracked source
changes fail both before and after the run. Raw Cargo metadata contains local
paths and stays private to the runner; the exported dependency inventory records
resolved features and package/source identity without those paths.

## Packaging and extracted acceptance

`stage-commercial-candidate.py` remains the sole copy recipe. Its explicit
external asset set is exactly `assets/aircraft/swift_sport.glb` and
`assets/aircraft/swift_sport.json`; it preserves required dependency, font/LUT,
project and world-data notices. No recursive `assets/` copy, Light Single GLB,
Light Single external JSON, Swift Blender source, secrets or regional data pack
is added. The Light Single profile remains embedded for legacy identity.

The stager's review-blocked exit 1 is allowed only with a coherent blocked report
containing exclusively outstanding review records. Any unexpected status or
integrity failure stops the candidate. A fresh ZIP is created and extracted
inside the private work directory; every file, byte size/hash, member name and
the executable are checked against the original manifest and built executable.
The readiness checker runs again on the extraction and retains its blockers.

Acceptance then uses the extracted executable from an unrelated working
directory. `BEVY_ASSET_ROOT` and `CARGO_MANIFEST_DIR` deliberately point at the
checkout containing the excluded developer model, to expose asset-search leaks:

1. Two `--distribution-info` runs must be byte-identical and match staged
   Swift-only MSVC identity. Both the executable and staged metadata must report
   the literal boolean `region_downloads: false`; missing, null, numeric and
   network-enabled identities fail. The optional network feature and its separate
   inventory are outside this unchanged offline recipe
2. No aircraft argument, `--view chase --screenshot ... --screenshot-delay 5
   --exit-after-screenshot`, must exit 0, identify Swift Sport, load the model
   from the extracted adjacent assets, fit 7.12 m at scale 1.0000, and produce a
   complete decoded PNG. ERROR, panic, asset failure and placeholder logs fail
3. `--aircraft light-single` must exit 2 for the absent adjacent GLB, despite the
   developer model reachable through those poisoned environment variables
4. The existing exact app fingerprint test runs with the candidate's same
   release/MSVC/features. Three additional exact app tests enforce legacy policy,
   persistent notice through playback/pause/seek/completion/fault, and real-font
   layout at the tested window widths. Each must actually execute one passing
   test. The independently pinned Python reference encoders also run without
   regenerating anything, before compilation. The same-head pure sim
   `record_takeoff` still creates a private legacy-v1 Light replay with observed
   partial fingerprint `0505e6644bb29a53`. The reviewed source contract binds that
   producer and consumer; independent golden bytes and identity vectors prevent
   both from silently moving to an unreviewed meaning
5. Default Swift plus that replay must exit 2 for aircraft/FDM mismatch, naming
   the actual `legacy partial fingerprint 0505e6644bb29a53` in hexadecimal. This
   negative case has no compatibility flag.
   `--aircraft light-single --no-model --legacy-replay-compatibility --replay ...`
   must capture a valid PNG, identify Light Single without model loading, and
   exit 0. Its log must include the full `LEGACY PARTIAL IDENTITY: historical
   yaw_rate_p was not recorded or verified` notice. The report records explicit
   opt-in, partial evidence, unverified historical yaw, and the persistence/layout
   test outcomes; evidence validation rechecks that disclosure and the log

The opt-in assumes the supported frozen complete Light baseline. It cannot prove
which omitted yaw coefficient the historical recording used. No old file is
rewritten, upgraded or treated as complete evidence. A startup warning and passing
layout/system tests do not alone qualify persistent native on-screen readability;
the actual integrated Windows run and inspection remain necessary.

The legacy screenshot and replay stay private. This is partial-identity and replay
startup acceptance, not a whole-flight bit-exact or cross-version guarantee.
D3D12 uses WARP/fallback; physical Windows GPU, controller, audio, performance
and store qualification remain separate. A human must inspect the actual Swift
image before calling its appearance reviewed.

## Reviewed source contract maintenance

The contract's `reviewed_source` names the coherent source reviewed for its file
pins. The qualification still binds its own exact clean HEAD, complete tree and
all tracked input bytes before and after the run, including the checker/tests
and contract. These are distinct guarantees: a run inventory identifies what ran;
the frozen contract rejects implementation drift beyond the reviewed boundary.
The historical commit need not exist in a shallow Windows checkout.

When implementation changes, inspect the full delta from the recorded reviewed
source, include any newly extracted helpers in the required path set, review the
identity/codec/app/UI behavior and retained independent goldens, then update only
the affected source pins and the reviewed-source pointer. Do not regenerate a
fixture, rewrite the historical hashes, normalize source bytes, hash selected
function fragments, or automatically refresh all pins to make a check green.
Changing an independent anchor requires its own compatibility review. An approved
pin migration is source acceptance, not Windows qualification; run this complete
recipe on the exact integrated source again. See the
[migration evidence](../qa/replay-candidate-contract-2026-10-04.md).

## CI trigger and evidence boundary

`.github/workflows/swift-windows-candidate.yml` runs only after successful source
CI for a same-repository main push whose exact SHA is still the current main
head. It explicitly checks out that SHA, keeps credentials unpersisted and
permissions read-only, and cannot tag or release. The check runs automatically
after successful source CI. A newer main head can cause an older run to skip;
inspect the candidate run for the actual head separately from ordinary CI.

The job has a **90-minute limit**. It performs a cold release build and a focused
release test binary build, so it can take longer than source CI. Individual
Cargo commands allow at most 60 minutes; screenshot runs allow 180 seconds and
startup-negative/metadata handshakes 30 seconds. There is deliberately no Cargo
or Actions cache, because cached compiled output could also distribute a blocked
binary. Runtime/GPU checks are never inferred from the Python boundary tests.

### Bounded screenshot diagnostics

Windowed screenshot launches retain the original Python startup behavior and
180-second deadline. Existing rendering/task-pool tracing is enabled through
the following exact `RUST_LOG` selectors, recorded with the run:

```text
info,wgpu_core::device::global=trace,wgpu_core::device::queue=trace,wgpu_core::command::transfer=trace,wgpu_hal::dx12=debug,bevy_app::task_pool_plugin=trace
```

Only a timeout of the default Swift capture triggers **one** fresh diagnostic
process. It uses the same extracted executable, scene, WARP settings, unrelated
working directory, asset-isolation environment, trace selectors and 180-second
limit. The only launch-setting difference is `CREATE_NEW_CONSOLE` plus
`STARTF_USESHOWWINDOW`/`SW_SHOWNORMAL`, matching the console/show requests in
the release's PowerShell `Start-Process` path. Python pipe redirection remains
in use; this is not complete harness equivalence. Requested flags, actual
executable SHA-256 and the probe result are recorded separately.

This tests a launcher hypothesis, not an established cause of a missing capture
callback. Continued CPU world updates do not prove swapchain rendering. Process
order, driver/shader/filesystem cache warm-up, trace overhead and CPU scheduling
can confound a comparison. There is no timeout increase, offscreen replacement,
repeated focus operation, rendering change or task-pool change.

The original timeout remains the engineering failure even if the diagnostic
process saves a valid image. The diagnostic cannot populate any of the four
required checks; remaining acceptance stops after the failed primary capture.
Primary success and failures other than timeout do not run the probe. The
optional `diagnostic-release-launch.log` retains its complete sanitized output.
Only successful model/path/fit, exit-zero, unchanged-executable and full PNG
validation can expose `diagnostic-release-launch.png`, with its own digest and
proof. A partial/unproven image remains in the private work directory.

### Evidence export

Only explicit filenames from a separate evidence directory can be uploaded:
acceptance/source-input/dependency-inventory/readiness JSON, sanitized command
and four runtime logs, `default-swift.png`, and the optional diagnostic log/image
pair above. Complete runtime logs appear once in their own files; `commands.log`
records the command, exit status and log reference/hash without duplicating
verbose trace output. Every allowed file gets a size
and SHA-256 in the report; the report excludes its own digest. Text must be
bounded UTF-8 without NUL bytes. The PNG requires signature, chunk CRCs, bounded
dimensions, complete decoded rows and terminal IEND without trailing data or
ancillary attachments; its hash and successful Swift log/fit/exit evidence must
match. An upload step runs only after this validator passes, including failures.
On failure, only validated available diagnostics may be uploaded; a partial or
unproven image stays private.

The executable, archive, extracted/staged trees, Cargo target directory, raw
metadata, notice directory and replay are **never upload paths**. They expire
with the hosted runner. Recollect and reverify the full package and notices for
a later distribution review; a JSON inventory is evidence of collection, not delivery
of a complete redistribution package or legal review. This workflow does not
provide a downloadable candidate executable through Actions artifacts.

The [ordinary release policy](binary-release-assets.md) and
[commercial rights audit](commercial-distribution-audit.md) still govern any
future binary distribution. No successful check weakens those gates.
