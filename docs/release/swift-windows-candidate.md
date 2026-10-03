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
SHA-256/size of its actual checked-out bytes separately. The five legacy
identity pins are checked against raw Git blobs and then require byte-exact
checkout equality; semantic changes and newline differences both fail. The
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
   Swift-only MSVC identity
2. No aircraft argument, `--view chase --screenshot ... --screenshot-delay 5
   --exit-after-screenshot`, must exit 0, identify Swift Sport, load the model
   from the extracted adjacent assets, fit 7.12 m at scale 1.0000, and produce a
   complete decoded PNG. ERROR, panic, asset failure and placeholder logs fail
3. `--aircraft light-single` must exit 2 for the absent adjacent GLB, despite the
   developer model reachable through those poisoned environment variables
4. The existing exact app fingerprint test runs with the candidate's same
   release/MSVC/features. The same-head pure sim `record_takeoff` produces a
   private legacy-v1 Light Single replay. Its observed fingerprint must be
   `0505e6644bb29a53`; five unchanged profile/FDM/replay source hashes bind this
   to public baseline `5c5b2a3057549c7429236b93aa0cdc99e2de38d1`. Updating those constants
   needs a separate identity review; the fixture and consumer cannot silently
   drift together
5. Default Swift plus that replay must exit 2 for aircraft/FDM mismatch.
   `--aircraft light-single --no-model --replay ...` must instead capture a valid
   PNG, identify original Light Single without model loading, and exit 0

The legacy screenshot and replay stay private. This is identity and replay
startup acceptance, not a whole-flight bit-exact or cross-version guarantee.
D3D12 uses WARP/fallback; physical Windows GPU, controller, audio, performance
and store qualification remain separate. A human must inspect the actual Swift
image before calling its appearance reviewed.

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

Only explicit filenames from a separate evidence directory can be uploaded:
acceptance/source-input/dependency-inventory/readiness JSON, sanitized command
and four runtime logs, and `default-swift.png`. Every allowed file gets a size
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
