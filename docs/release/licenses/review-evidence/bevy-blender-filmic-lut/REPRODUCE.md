# Reproduce the one pinned candidate

This evidence packet contains scripts and text only. The existing application and reference LUT are read-only inputs to verification. Running these steps generates candidate images in new private directories; it does not publish or replace anything.

1. Obtain `blender-3.4.1-linux-x64.tar.xz` from the exact official URL in `source-bindings.json`. Verify the recorded SHA-256 against the included official `blender-3.4.1.sha256`, and unpack it beside these scripts as `blender-3.4.1-linux-x64/`. The reproduction script also checks the executable and config/table/notice hashes.
2. With the recorded Rust 1.93.0 compiler, run `rustc -O stimulus.rs -o stimulus-generator`, then `./stimulus-generator stimulus.rgba32f`. The 4,194,304-byte result must have SHA-256 `34259aa96f64dab1427ed9af0d6f4ae2842673147d0c18376c01dc828ea6bdb7`.
3. Run `python run_pinned.py --output-directory fresh-run-a`, then the same command with a different new directory `fresh-run-b`. The driver sets the pinned OCIO configuration before Blender startup, requires its explicit selection acknowledgement, and records its successful exit. It never uses a pre-existing output directory.
4. On a host with Python and libzstd, run `python verify_reproduction.py --reference <unchanged-Bevy-0.18.1-Blender_-11_12.ktx2> --run fresh-run-a --run fresh-run-b --output fresh-comparison.json`. The reference container's exact identity and layout are required before decoding. Both decoded byte streams must match exactly.
5. Run `python -m unittest discover -s . -p test_comparison.py -v` for the six explicitly synthetic comparator tests. These tests are not runtime or license approval.

`run-5/` and `run-6/` hold the retained bounded text observations and invocations of the final actual evidence pair. The generated EXRs and float streams are deliberately omitted; their hashes and sizes are recorded, and the same candidate can regenerate them. Those retained run directories therefore cannot be passed directly to the comparison script without the original locally retained outputs.

See `ASSESSMENT.md` for the documented source-notice route and its exact scope, and `independent-review/` for the separate technical review. No field in this packet authorizes a release or represents a final whole-application rights decision.
