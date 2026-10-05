# CPU-ready screenshots: native acceptance

The ordinary screenshot request could run before budgeted terrain and airport
overlays became visible. Both Windows and native Linux evidence reproduced the
problem: the aircraft and HUD appeared over an incomplete blue foreground.
This change delays the request until a coherent available CPU scene has had a
prior frame for extraction. It does not change terrain quality, flight physics,
the requested minimum delay, or the Windows 180-second watchdog.

## Reproduced ordering

The exact `cdbb9f7` Windows candidate passed its existing package/startup tests.
Its screenshot request at `02:09:35.346` preceded the first settled terrain log
at `02:09:42.647`. The immediately preceding log reported 2 displayed tiles,
41 live tiles, active stitching and four hidden airport overlays. Saving the PNG
at `02:10:14.768` did not establish which scene was ready at the earlier request.

Native Linux reproduced this with an ordinary five-second request: request
`02:24:52.999`, first settled periodic log `02:24:55.252`. A separate diagnostic
30-second capture showed the runway. The longer delay is retained evidence;
the implementation uses readiness rather than a longer fixed wait.

## Reviewed implementation

Capture runs in `Last`, after deferred Update changes and PostUpdate propagation.
It requires matching displayed/live tile IDs, completed bridge/overlay work,
relevant runway visibility, and the same scene signature on consecutive frames.
The signature includes tile entities, overlay revision, model entities, startup
revision and floating origin. Existing minimum time/frame and model-fit checks
remain. The request logs its exact CPU admission state.

An optional selector observer distinguishes an examined available cut from
unfinished discovery. This supports sparse/empty sources, real coarse ancestors,
capped global ancestors and polar retry churn without changing read priorities,
budgets or the original retry map. Outcome history is allocated only for pending
capture, pruned with active dependencies, reset on frame wrap, and discarded
after capture. Normal selection retains one nullable pointer and performs no
observer allocation or traversal. This is a source-level cost boundary, not a
frame-performance benchmark.

The existing 15 km active-airport relevance rule permits an intentionally distant
runway to remain hidden. It does not exempt the relevant default runway.
CPU admission and one extraction opportunity are not a general GPU/pipeline fence.

## Actual native checks

The frozen runtime source is local checkpoint
`d00bfced040e86809d3e1057518fe2c9cf07adeb`, tree
`17853fb43eb651031e1a759a51fd95331647e62d`.
Executable SHA-256:
`6088615e6df2c9b45a8c02e7613b16d4f6a315d35a7d2038b9bc91feb66e38a7`
(151,526,176 bytes). The build receipt binds all 2,340 source files and the actual
Cargo artifact. Later integration adds source bindings and documentation only.

All cases used the real desktop, Swift Sport/chase view, a five-second requested
delay and a fresh task-local shader-cache directory. Each produced an inspected
1180×812 PNG, logged CPU admission before request before save, and exited 0 after
normal window close. No error or panic was logged. The adapter was software
Vulkan llvmpipe; this is not physical-GPU, Windows, audio or controller acceptance.

| Native case | Committed CPU cut | Observed image |
|---|---|---|
| Default start | 41 displayed/live/desired; overlay revision 4/4; none hidden | Runway, markings and adjacent terrain visible |
| Swiss `--start 46.58,8.00 --fly 1500` | 38 displayed/live/desired; revision 4/4; four distant airport overlays intentionally hidden | Relocated airborne view completes; existing coarse terrain/cloud appearance retained |
| `--global-terrain off` | Empty displayed/live cut; available-cut readiness; revision 0/0 | Standalone runway and markings visible; existing no-terrain-data notice retained |

The before/default-after images use the same ordinary requested delay and view;
their simulation states advance normally, so this is readiness evidence rather
than a pixel-identical flight comparison. The Swiss image does not establish
high-detail global terrain or improved cloud realism.

Image SHA-256 values:

- Before default: `6f859e331d664febd8d7e81a01de9f3c2cd28cad6964a21d763b7409216f8361`
- Corrected default: `b58b61ef31e55f006efe7d404df35b3356062e98a95fad31a59c3726c424b53a`
- Swiss: `d5d0ca3342f9d20647f304f6d0927f74c4577d3bff9a79dc32b2fb24cbd61428`
- Flat: `e31c5bb6bd3196f13a1b20894838e1113cc8f1d8057488a76cc150f3610e166d`

## Automated and distribution boundaries

Independent review approved the implementation. App checks passed 389 unit tests
plus the model hierarchy test; render checks passed 350 unit and 82 integration
tests. Three documented optional/manual fixtures were ignored. The complete suite
preceded a reviewed, behavior-preserving Option guard rewrite; final Clippy with
warnings denied checked the committed source. A subsequent exact-source readiness
rerun passed all 10 selected app/render tests. Formatting and architecture passed.

The [candidate binding review](replay-candidate-capture-readiness-pin-review-2026-10-05.md)
passed 193 focused Python tests. It advances only the existing main.rs hash, adds
screen_capture.rs and terrain_selection.rs, and retains the other 126 hashes and
all 102 frozen anchors. The boundary now has 129 reviewed files and 231 total
bindings. Existing checker function bodies, save/flush/sync/exit behavior and
acceptance watchdog remain unchanged.

CI and the separate Windows candidate must be checked against the new published
head. The preceding `cdbb9f7` candidate's success is not transferred to this change.
Ordinary binary release, dependency/rights review and Steam qualification remain
independent; no new binary release is claimed.
