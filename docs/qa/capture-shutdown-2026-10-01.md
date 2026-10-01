# One-shot capture shutdown, 2026-10-01

Windows software-D3D12 run [36898555726](https://github.com/Xenoah/flightsim-claude/actions/runs/36898555726)
checked out `5782496024b872e5b54eeb45c0c021fe41203201` (alpha.19).
The bundled model fitted, capture was requested, and the actual PNG was saved
at 17:39:09 UTC, approximately 104 seconds after launch. The image was downloaded
and inspected: a complete cockpit, runway, HUD and instruments. Frames stopped
after `AppExit`, but the process still had not terminated at the 180-second deadline.
Graphics/window teardown is a plausible explanation, not a stack-trace diagnosis.
The equivalent native Linux windowed capture exited with status 0 after saving.

The explicit `--exit-after-screenshot` batch option now exits directly after
encoding, flushing, syncing and closing the PNG, and flushing both output streams.
It does not wait for graphics destructors. Capture or flush errors exit with status
1. Without this flag, saving a screenshot does not terminate the interactive app.
`--headless-screenshot` remains a one-shot option and enables the same behavior.
PNG is the supported capture output format.

Regression tests cover a complete PNG signature/end chunk, actual file-write
failure, success/error exit codes, flushed stdout/stderr, and continued execution
without the batch flag. CI still requires a successful process exit, a complete PNG,
model-fit and save markers, the new batch-completion marker, and no fatal log output.
None of those gates were removed or weakened.

Release workflow runs now reject stale source events whose tested SHA differs from
the workflow's main SHA. This avoids running a newer batch protocol against an older
binary when main CI runs finish out of order. Tags remain immutable; the next build
uses alpha.20, not a replacement alpha.19 tag.

This does not establish normal interactive Windows shutdown or physical-GPU behavior.
Those remain separate checks from a software-rendered one-shot release smoke.
