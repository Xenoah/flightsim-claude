# Isolated Windows screenshot readback trace

This is a dependency-instrumentation experiment, not a rendering fix or an
ordinary candidate acceptance result. The frozen ordinary source is
`afff4a7ccb35ce2f075ca6f404ffc2e664fceb4f`, tree
`f1bdbb74f4546219c2e7eef2b6f9f9f99412dcd3`. Its tracked application, workspace
manifests, lockfile, ordinary candidate runner, production workflows and release
checks remain byte-for-byte unchanged. This document describes the method;
Windows results must be read from the separately identified diagnostic artifact.
No Windows success or visual approval is asserted here.

## Question and evidence boundary

The ordinary screenshot path has not established a successful Windows capture.
The experiment records where its existing screenshot buffer progresses through
Bevy task scheduling, map registration, wgpu submission/fence retirement,
callback invocation, channel delivery, mapped-byte access and main-world
screenshot notification. It does not create a marker texture, additional camera,
GPU copy, queue submission, poll, additional callback registration, executor or retry. In particular the
application's existing `--windows-readback-diagnostic` probe is not enabled.

The baseline observation used Microsoft Basic Render Driver, vendor 5140,
device 140, type `Cpu`, driver `10.0.26100.33438`, empty driver information and
backend `Dx12`. Its runner image was `windows-2025-vs2026` version
`20260925.250.1`, Windows Server 2025 Datacenter 10.0.26100, runner 2.337.0,
provisioner 20260901.588. The baseline evidence archive SHA-256 is
`434712a7a03559186601ec264a4dc1a4fdc716cb0bc28b372c250a7756247317`; its raw
runtime log SHA-256 is
`e878812a9eed4cb0a48085deda98a5b4b0618d7e49b8c036e47c0bd72c54a5ec`.

`windows-latest` can change. The report retains available `ImageOS`,
`ImageVersion`, `RUNNER_OS`, `RUNNER_ARCH` and Windows platform/version values.
It parses the complete actual `AdapterInfo` and explicitly labels an exact
match, mismatch, missing observation or unparsed/ambiguous observation. It never
claims that a matching adapter proves an identical host. A mismatch does not
cause another launch or changes to the machine.

## Separate source, dependency and executable identity

The runner requires a clean committed diagnostic checkout whose full SHA equals
the workflow event SHA. Every ordinary path, Git mode and blob ID must match the
frozen baseline; the checkout bytes must produce the canonical Git blob hash.
Only new diagnostic files are admitted. The source evidence contains every
ordinary and diagnostic input's byte count, SHA-256, Git blob and mode, the full
ordinary and diagnostic commit/tree IDs, and both path sets. The evidence
validator reconstructs both Git trees rather than accepting a tree label alone.

A fresh private source tree receives only the ordinary files. The runner first
fetches the ordinary locked MSVC dependencies and captures its resolved metadata
and baseline dependency notices. `diagnostics/windows-readback/prepare.py` reads
the cached `bevy_render` 0.18.1 and `wgpu-core` 27.0.3 crate archives, verifies
archive checksums against the original lock and reviewed edits, extracts bounded
regular files to a separate directory and applies exact, hash-checked changes.
It never writes to the shared Cargo registry. The shared
`flightsim-readback-trace` 0.0.0 helper has no external dependency or feature.

The preparer writes a private Cargo patch configuration and changes only the
private lockfile: the two patched packages become path packages and gain the
helper dependency, and the helper package is added. The runner checks every
other locked package, version, source, checksum and dependency edge. Resolved
metadata must retain all ordinary features and edges except those helper edges.
App defaults, `commercial-staging` and the full `tonemapping_luts` dependency
feature remain enabled. No library or toolchain upgrade is part of this work.

The evidence binds archive checksums, the complete original and generated
registry-source manifests, committed patch and preparation hashes, helper bytes,
private patch configuration, original and final lock, both metadata hashes,
resolved package features/edges and executable SHA-256. Generated dependencies,
ordinary source and the package are rechecked after execution, including after
a screenshot timeout. This identifies an instrumented executable; it never
relabels it as the ordinary executable.

## Ordinary recipe and package fidelity

The existing candidate module supplies the build and capture commands. The
build keeps Rust 1.93.0, `x86_64-pc-windows-msvc`, `--locked --release -j 2`, package
`flightsim-app`, feature `commercial-staging`, default features and
`RUSTFLAGS=-D warnings`. The only added Cargo argument selects the private patch
configuration. The target directory is fresh and private; no binary cache is
restored or uploaded.

The unchanged ordinary stager consumes the baseline notices collected before
the dependency overlays. Its source copy set, model/profile bytes, inert notices,
distribution metadata, blocked readiness and package path layout are retained.
Its bundle manifest truthfully records the instrumented executable's new hash.
The package is zipped, extracted and verified before execution, just as in the
ordinary recipe. All package/archive contents remain private to the runner.

The baseline inventory and blocked readiness are preserved control inputs; they
do not describe, review or approve the diagnostic helper or changed dependency
sources. Their presence cannot qualify the modified executable. The separate
`package-inputs.json` records every packaged path/hash and this limitation.
Neither a gate approval nor a production acceptance record is written.

There is exactly one informational executable invocation by the existing
stager: `--distribution-info`. The pinned app handles it before app/renderer
initialization. It is counted separately from the single scene invocation.
There are no extracted-handshake repetitions, absent-aircraft launches, legacy
replay launches, secondary probe, alternate console or rerun.

The one scene command is the unchanged ordinary capture:

```text
flightsim-app.exe --screenshot <private-work>/default-swift.png --screenshot-delay 5 --exit-after-screenshot --view chase
```

It runs from an unrelated working directory against the extracted adjacent
Swift-only package. The ordinary window and scene defaults remain in force.
Environment is `WGPU_BACKEND=dx12`, `WGPU_FORCE_FALLBACK_ADAPTER=1`, with the same
`BEVY_ASSET_ROOT`/`CARGO_MANIFEST_DIR` source values and ordinary `RUST_LOG` filter.
The Python subprocess launcher has no custom creation flags or startup info.
The scene watchdog remains 180 seconds. Its timeout terminates and waits for
that process and saves already captured stdout/stderr. No exception, timeout,
missing screenshot or successful capture grants permission for a second scene.

## Reading the trace conservatively

The exact event table and integer-field meanings are in
[`events.json`](../../diagnostics/windows-readback/events.json). Raw lines use:

```text
FS_READBACK_TRACE schema=1 seq=<u64> us=<u64> event=<name> a=<u64> b=<u64> c=<u64>
FS_READBACK_TRACE_STATUS schema=1 reserved=<usize> drained=<usize> dropped=<usize> io_errors=<usize> capacity=2048
```

The helper reserves at most 2,048 fixed atomic slots. The sequence number is
reservation order, and timestamps are elapsed microseconds from the first
capture preparation. Concurrent writers can reserve and sample timestamps in
different orders; decreasing timestamps do not establish time travel or a
corrupt log. The observer drains contiguous published slots from the main update
outside the instrumented engine locks. Recording does not print while those locks are held.

A callback can execute before registration returns; a receiver can resume before
the sender returns. The parser therefore does not infer causality from those
return-event orderings. Per-event IDs and field meanings provide the basis for
manual causal analysis. It rejects unknown event names, malformed or overflowing
integers, duplicate/reversed sequences and impossible basic counter bounds.
Observed sequence gaps remain explicit rather than being silently filled.

Status counters are concurrent snapshots and may undercount later events or
losses. On a timeout, a blocked main update, a pending earlier slot, output I/O
failure, overflow or abrupt process termination, events may have occurred without
being exported. No last line proves a process-final complete trace. Missing
callback, retirement or receive observations remain unknown. A pending
submission does not by itself identify a driver bug, and callback delivery time
is not a GPU-duration benchmark. Instrumentation itself perturbs timing.

Both output channels are retained in the bounded runtime log, with ANSI/control
bytes sanitized and local source/work prefixes replaced. Channel concatenation
is not a cross-stream chronological ordering. A log exceeding 32 MiB retains
whole preceding lines and an explicit truncation marker; the JSON projection
and report record the truncation. The total upload bound is 64 MiB. Truncation
or malformed trace cannot be presented as a completed diagnostic. Failed trace
parsing retains the raw text and the original process failure, not a retry.

## Workflow, artifacts and interpretation

`.github/workflows/windows-readback-trace.yml` responds only to a push to
`diagnostic/windows-readback-afff4a7c` and refuses GitHub rerun attempts greater
than one. It has read-only repository permissions, SHA-pinned ordinary checkout,
Python and artifact actions, no persisted Git credentials and no cache. Publishing
this branch and the single Windows observation are coordinated separately;
local harness tests do not launch the simulator.

The explicit artifact allowlist contains only:

- `diagnostic-report.json`
- `source-inputs.json`
- `dependency-inputs.json`
- `package-inputs.json`
- `trace-events.json`
- `commands.log`
- `runtime.log`

The validation step runs even when preparation, build or capture fails. The
upload runs only if validation succeeds. It rejects extra paths, directories,
symlinks, oversized/non-UTF-8/control-bearing text, duplicate JSON keys, changed
file hashes, inconsistent identity, changed scene/launcher/environment or
re-sealed contradictory trace projections. No PNG, GLB, LUT, executable,
archive, crate source or notice file is exported. A successful private PNG may
be validated and identified by dimensions and hash, but is never uploaded.

`diagnostic_capture_completed` means this instrumented single capture exited
zero with the ordinary smoke tokens, private PNG validation, an exact observed
adapter match and bound trace/input evidence. It is not visual acceptance or
ordinary candidate success. Timeout/build/capture/integrity failures remain
failed even when their partial evidence uploads successfully. All
`qualifies_ordinary_candidate`, `qualifies_visual_acceptance`, `qualifies_release`
and `release_authorized` fields are always `false`.

## Local verification

Run the CPU-only harness checks without launching the application:

```text
python -B -m unittest discover -s scripts/tests -p test_windows_readback_trace.py
python -B -m unittest discover -s diagnostics/windows-readback -p test_prepare.py
```

The runner tests exercise byte-exact baseline tree reconstruction, changed
checkout bytes, hostile paths, forbidden lock/version/feature changes,
concurrent/partial/overflow trace interpretation, unknown or forged events,
unchanged scene flags/environment/watchdog, one attempt with no retry,
timeout output preservation, line-bounded truncation, re-sealed false evidence
and text-only workflow uploads. The preparer tests separately cover hostile
archives and checksum/lock boundaries. Results establish harness behavior;
Windows execution remains a separately recorded observation.

Local checks on 2026-10-08 passed: the 318 existing Python harness tests (one
Windows-only junction test skipped on Linux), 18 diagnostic runner tests, three
preparer tests, the recorder's concurrent publication/gap/overflow/BrokenPipe
test, helper Clippy with `-D warnings`, and ordinary workspace formatting. The
actual patched `bevy_render` and `wgpu-core` crates compiled with Rust 1.93.0,
`--offline --locked -j 2` and `RUSTFLAGS=-D warnings` on Linux. The final compiled
dependency/helper bytes were checked against their preparation manifests.

These are targeted diagnostic checks. A complete Linux application build/test
was not run: this executor lacks ALSA, libudev and X11 development metadata.
No MSVC build, Windows launch, screenshot, ordinary qualification or release
approval is claimed by these local results.
