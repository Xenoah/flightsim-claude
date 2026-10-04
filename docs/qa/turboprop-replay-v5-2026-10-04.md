# Turboprop simulation and replay v5 QA

Date: 2026-10-04 UTC. Isolated worktree: `flightsim-turboprop-replay`.
Base profile checkpoint `deb74ce`; final FDM correction `7fdf76a` consumed as
`35ecbbc`, final profile qualification `07febca` consumed as `9b21f00`.
Implementation checkpoint: `0ac6892`; the following qualification includes its
review fixes. The pure host/replay review accepted `47915f46cf0b9ec3b1427aa3ca5f97f0daf25602`
(runtime/tests at `e416fb6`, followed by a QA-wording correction). Its version-5
allocations are fixed at integration; app/native/preset gates remain separate.

## Executed evidence

The dedicated toolchain environment and shared target were used with `-j 2`.
Project library roots were touched after Cargo-lane handoff, and logs record
compilation paths in this isolated worktree. No concurrent Cargo build,
Bevy/GPU/desktop operation, publishing, or Library action was performed.

- Broad pure suite: `cargo test -j 2 -p flightsim-core -p flightsim-fdm -p flightsim-world -p flightsim-sim -p flightsim-tilegen -p flightsim-content -p flightsim-net`
  passed 1,220 tests across 91 suite summaries, including doctests; zero failed
  and zero ignored
- New v5 coverage: 3 sim unit tests, 5 codec integration tests and 19 numerical
  replay integration tests. The complete sim unit suite has 66 tests
- `cargo clippy -j 2 -p flightsim-sim --all-targets -- -D warnings`: passed
- `RUSTDOCFLAGS='-D warnings' cargo doc -j 2 -p flightsim-sim --no-deps`: passed
- `cargo fmt --all -- --check`, `bash scripts/check-architecture.sh`, and
  `git diff --check`: passed
- `python docs/qa/replay_v5_reference.py`: 21 independently written binary
  witnesses and 49,100,903-byte maximum arithmetic passed
- Unchanged `replay_v4_reference.py` and `replay_v3_reference.py` byte witnesses
  passed; no existing golden was edited

Logs in the parent scratch workspace use the prefix `turboprop-replay-` and
suffixes `pure-final.log`, `clippy.log`, `rustdoc.log`, `fmt.log`,
`architecture.log`, `python-v5.log`, `python-v4.log`, and `python-v3.log`.
Earlier failed draft checks were corrected before these final gates.

## Numerical and host evidence

Full state and replay are exact across 30/60/144 Hz frame partitions. A 481-tick
numerical trajectory reconstructs in 240+240+1 attempts; backwards seek, pause
preference and restart restore the complete committed snapshot. Physical engine
states evolve, and turbine response agrees with an independent analytic
exponential within 3e-14. This is a numerical-law test, not an aircraft claim.

Explicit actual failures at K2, K3, K4, Endpoint and K4 in the second internal
substep preserve all physical scalars, both endpoints, elapsed time, ground,
clearances, contact/log history and controller proposal. The endpoint fixture
uses the separately established FDM interval between K4 shaft
180.11406012828962 and weighted endpoint 180.11406014776125 rad/s. A genuine
terminal after 240 successes is deferred to the next public call, consuming one
attempt and no time.

A constructed interior disk-power violation, admitted at table nodes, produces
the correct full-state zero-cursor terminal and reproduces exactly. False
terminal success and altered diagnostic bits reject without committing a scratch
step. Every engine checkpoint scalar detects drift and rolls back the failed
replay attempt and cursor. Successful-report recorder continuity independently
checks all three engine scalars, including signed zero. The terminal-only
continuity regression changes shaft speed while holding the other state scalars
fixed; the same complete-state comparison is used for rejected attempts.

Parked geometry preserves explicit engine bits with zero elapsed time; failed
restart leaves the prior host intact. A real numerical ground contact records
touchdown/log history and reconstructs it from zero. Bundled terrain, climate,
deterministic wind/turbulence and authored rain weather reproduce at exact
committed clocks. Unknown bundled terrain or climate fingerprints remain
structurally inspectable/exportable and fail reproduction.

## Review fixes and independent wire evidence

Each report carries shared immutable exact canonical physical-model and complete
initial environment/terrain/weather bytes. The recorder checks source provenance
before any append, then complete before-state and cursor continuity, including
terminal-only reports. Same-state foreign model, strong wind and signed-zero
wind reports reject without changing the exportable prior prefix. Canonical
source bytes are prepared once; they are not rebuilt on each fixed step.

The recorder tracks exact cumulative accepted time privately and validates the
prospective visual-time product before appending a report. A huge finite rate
fails at the first report; a near-maximum Julian epoch fails after an existing
one-step prefix. Both leave a valid exportable prefix while live physics can
continue. This adds no old-v4 changes or new wire field.

A fixed turbulence-seed-zero witness has frame-zero crossflow ratio
0.00254737687272323 and prospective +dt ratio 0.00254727921162822, with authored
cap 0.002547328042175725. The actual first attempt succeeds even though the
zero-time query rejects. Nonempty player construction therefore validates its
first controls on scratch state at the prospective wind clock through the same
sim-owned environment path. Empty nonterminal records still validate at time
zero; explicit terminal-at-zero records prove their recorded attempt.

The Python fixtures include all reason categories, every weather shape and a
121-control structural witness with distinguishable controls, periodic cursor
120 and final cursor 121, and different turbine/shaft/pitch checkpoint values.
That structural witness does not claim a reproduced flight. Rust tests cut each
fixture at every byte and mutate counts/lengths, identity versions, engine
NaN/infinity/global bounds, terminal tags/details/stages, reserved mask bits,
required diagnostic groups, and physical signs. Old readers exclude v5 and old
v4 bytes remain exact.

## Limits

Independent integration acceptance remains separate. Native app dispatch,
presentation, aircraft preset handling, headless aircraft scenario tuning and
real-aircraft/measured-data qualification are outside this change. The numerical
axial descent fixture intentionally isolates replay from aircraft trim.
Exact reproduction assumes the same build and configuration, not cross-platform
libm equality. Maximum byte/step counts are structural limits, not measured
process-memory, wall-time or frame-rate guarantees.
