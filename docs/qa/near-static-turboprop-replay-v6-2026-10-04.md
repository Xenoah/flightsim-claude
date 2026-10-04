# Near-static complete-state host and replay v6 QA

Date: 2026-10-04 UTC. Scope: additive pure host and bounded codec for the accepted
near-static FDM law 2. Local qualification is complete; independent v6 review
remains pending. Qualified runtime/test source:
`f8ab7dcc1e3326703298fbc3c2354f7d8af0d46f`. Subsequent changes only record these
receipts and update documentation links.

## Source and compatibility boundary

Base: `6fa7aee254a43943928e4c26f16c99e64eecaaef`, the explicit profile-4 and
identity-4 foundation. The separate law-2 configuration, host types and replay-6
API do not widen any old profile/model/codec gate. The only existing production
source edit is two `lib.rs` module declarations. No FDM numerical source, app,
picker, GPU, Cargo dependency, old replay writer or old fixture changes.

The final SHA-256 audit checks 116 protected FDM/profile/identity/host/codec/
fixture files against the base and finds no changes, including all 42 existing
replay fixtures. Source files remain below 200 KiB; complete logs and manifests
live in the verified external QA archive rather than production source blobs.

## Independent wire evidence

`python3 docs/qa/replay_v6_reference.py` independently constructs 35 bounded
fixtures. No production Rust encoder or runtime output supplies expected bytes.
The witnesses cover seven weather shapes, legacy weather, all retained terminal
categories, all four new static-power details, all three near-static outside
axis combinations, both scale-failure diagnostic groups and five negative-flow
post-domain error families.

The N=121 structural witness contains distinguishable effective controls,
checkpoint cursor 120 and final cursor 121 with distinct full engine triples.
It demonstrates wire order and final-tail persistence, not a physical trajectory.
The unchanged v3/v4/v5 independent encoders also pass. The new maximum serialized
length is independently checked as 49,100,927 bytes; terminal maximum is 158.

Codec tests exercise every truncation, malformed counts/lengths, closed reason
and detail tags, reserved masks/stages, reason/mask/value consistency, the fixed
0.10 axis boundary and signed-zero/subnormal diagnostics. Unknown nonzero
identity allocations and dataset fingerprints can inspect/export exactly while
the player rejects reproduction. Zero allocation IDs and unknown host revisions
reject. Old readers reject version 6 and the v5 writer retains byte identity.

## Host and numerical evidence

The new suite retains the accepted v5 host's regression scenarios under the
explicit law-2 type, including full-state cadence invariance, controller rollback,
K2/K3/K4/Endpoint/later-substep failure, flight log/contact reconstruction,
checkpoint mismatch rollback and bounded frame-zero seeking. Terminal scratch
attempts share the 240-attempt call cap and never commit unexpected success.

Report admission compares full canonical configuration bytes, including the
new negative cells, and exact environment/terrain/weather/source/clock origin.
Tests reject same-state/cursor reports whose only difference is a negative cell,
and separate foreign terrain/weather/turbulence/epoch/rate cases. Successful and
terminal-only engine-state mismatches are rejected. Prospective visual overflow
preserves both empty and nonempty exportable prefixes while live physics continues.

The fixed first-query turbulence regression distinguishes zero-time support from
the authentic first attempt's prospective +dt wind clock. The player uses +dt
without a hidden state warmup or zero-time force rejection.

An additional 601-step numerical flight crosses negative J into forward flow,
records complete state and all controls, replays at 30/60/144 Hz and reconstructs
several complete host snapshots by rewind. Genuine adverse/transverse domain
failures verify exact new reason/stage/diagnostic groups and rollback. Two actual
underflow cases independently cover invalid scale: an axial subnormal whose J
underflows before insertion (mask 0x1f7), and a positive transverse subnormal
whose ratio underflows after negative J is established (mask 0x1ff). Structural
subnormal/zero bit preservation is separately tested; no codec repair is allowed.

The original Cedar full-state boundary is copied bit-for-bit from the accepted
fixture. The original bare-FDM `still_air` oracle retains J bits
`bed475ed0aff54c1` (-4.8781986805208105e-6). The unchanged v5 host instead samples
an explicit held ground plane; its corresponding oracle retains J bits
`bed475ed0b07234a` (-4.878198680954174e-6). Both reject at K2/substep 2. These are
distinct exact environmental contexts, not a tolerance comparison or a physics
change. The complete held-plane error is checked against direct old-law FDM
evaluation and reproduced exactly by schema-3/v5 playback. Law 2 accepts the
same host initial state, controls and held-plane context, records under schema
4/version 6 and reproduces it. A separate 3,600-step Cedar continuation records
20 seconds of braked idle and 10 seconds of brake release with a gradual 0..0.4
throttle ramp, then replays and rewinds complete host history.

## Qualification and limits

The complete command uses the prescribed `build-env.sh`, existing shared target,
`-j2`, warnings denied and no second cache:

```
cargo test -j2 -p flightsim-core -p flightsim-fdm -p flightsim-world \
  -p flightsim-sim -p flightsim-tilegen -p flightsim-content -p flightsim-net
cargo clippy -j2 -p flightsim-sim --all-targets -- -D warnings
RUSTDOCFLAGS='-D warnings' cargo doc -j2 -p flightsim-sim --no-deps
cargo fmt --all -- --check
bash scripts/check-architecture.sh
```

All commands exit 0. The full suite passes **1,298 tests/doctests across 95 suite
summaries**, zero failures and zero ignored. It includes 69 sim unit tests,
11 new codec cases and 28 new numerical host/replay cases. Initial compile
failures and the first runtime run (which exposed the environment-context and
unknown-identity test assumptions) are retained as diagnostic evidence.
The final literal held-plane environment assertion is covered by the full run.

`FLIGHTSIM_V6_QA_OUTPUT` optionally exports test-only authentic recording files;
ordinary tests write nothing. The archive contains a law-1 held-plane v5
terminal, same-input law-2 v6 success, 601-step crossing and 3,600-step Cedar v6
records. A separate independent Python inspector checks every control/checkpoint
boundary and final tail, and exact old/new initial-state/environment/control
bytes. Its structural audit is distinct from the Rust numerical reproduction.

External archive: `flightsim-qa/nearstatic-fullstate-replay6-evidence.tar.gz`
(73,944 bytes), SHA-256
`2d485eb586ef1d4eac0bb3a656acb237e51300aa8c14a4f47bdc5e2c42ab2b66`.
Every one of its 24 evidence files was reopened from the tar archive and checked
against the stored byte length and SHA-256 manifest. The archive also preserves
the exact runtime-source manifest, all independent reference outputs, raw
qualification logs, initial failures and separately named ground/environment
bit witness. The archive is external QA evidence, not a production source blob.

| Receipt | SHA-256 |
| --- | --- |
| pure-final | `64db60acf638e0e89017e5ea345dc58cd1221b4a510e281546c8aa3873d3b7a1` |
| clippy-final | `220c0589aa8694b060520c93626b64f2935ecaeb4a05ecbf3dff4aa6c17b6d16` |
| rustdoc-final | `33096651796b9d9df7068fa98dab640f1dd0f019cf86eff1245c044093102244` |
| python-v6 | `8d1d7a80e8e8c1bc28b843c57c3d764f42ff0dbfc7620ab1d6de357c0200a2f0` |
| runtime-witness audit | `638b50916c5635d9c27b404aba565400f5af8e2e4ff160a7f6de7946c9a37e85` |

These are same-build/configuration deterministic checks, not cross-platform
libm trajectory promises or performance benchmarks. Existing bounded Cedar
experimental evidence does not establish stationary parking: persistent brake
creep and tailwind/domain limitations remain. No aircraft performance, app,
native/GPU, Windows or production-publication acceptance is claimed.
