# Lateral-trim reset sampling — 2026-10-04

Base: `bd24743f81357d8c49b76d18d15ed08721e5fc13`.
Scope: the `flightsim-input` keyboard sampler and regression tests only.

## Reproduced defect and correction

Using the real Bevy 0.18.1 `ButtonInput<KeyCode>`, pressing and releasing K before
`PilotKeys::from_keyboard` leaves `pressed(K) == false` and
`just_pressed(K) == true`. The old sampler inspected only `pressed`, so that
observation contained no lateral-trim reset command.

The new regression
`reset_tap_before_keyboard_poll_resets_both_trims_on_an_executed_step` failed at
`assert!(keys.lateral_trim_reset)` before the source correction (0 passed,
1 failed, 101 filtered out; build 8.48 s). With the correction it passes, including
applying the sampled command to nonzero aileron/rudder trims with a fixed-step
duration and checking both resulting positive-zero bit patterns.

The sampler accepts held K under the unchanged Ctrl/Alt/Super held-key
suppression. It additionally accepts `just_pressed(K)` only when no Ctrl/Alt/Super
modifier is held, pressed or released in that Bevy frame. Holding K still resets
on each applied step. Sampling does not consume the edge. The next Bevy frame
clears a released tap; no queued reset or additional persistent state is
introduced. J/L, U/O, Shift fine adjustment and every rate-based command keep
their existing mapping. Shift is allowed for both held K and released K taps.

Lead review of the initial fix found that Ctrl+K with both keys released before
polling could otherwise become a reset. The follow-up regression
`released_shortcut_modifier_edges_suppress_reset_taps_for_that_frame_only` failed
against that initial fix: the ControlLeft case produced `lateral_trim_reset: true`
(0 passed, 1 failed, 104 filtered out; build 6.77 s). The regression now covers both
sides of Ctrl/Alt/Super, including a modifier held since the previous frame so
only its release edge is present, and a later plain K tap after frame clearing.
Separate tests preserve the old held-K behavior when a shortcut modifier is
released and allow both Shift keys whether held or released before polling.

The existing physical-key mapping test now explicitly calls `ButtonInput::clear`
to advance past the press frame before checking release. Its original held-key,
independent-axis and both-Shift-key assertions remain in place.

## Checks

With the shared build environment (`CARGO_INCREMENTAL=0`, dev/test debug info
disabled, `RUSTFLAGS="-D warnings"`, existing shared target, `-j 2`):

- `cargo test -j 2 -p flightsim-input`: 107 passed, 0 failed; 0 doc tests.
  Build 8.46 s; unit tests 0.01 s, after the modifier-edge correction.
- `cargo clippy -j 2 -p flightsim-input --all-targets -- -D warnings`: passed,
  1.25 s, after the modifier-edge correction.
- `cargo fmt -p flightsim-input --check`: passed.
- `bash scripts/check-architecture.sh` and `git diff --check`: passed.

Coverage includes press/release before polling, repeat sampling within the same
frame, following-frame edge clearing, held K after its initial edge is cleared,
both sides of Ctrl/Alt/Super with held and released lateral keys, and the real ECS
sampling system with no control-update call. In that last case, the first sample
sees the tap without changing retained trim; the next frame replaces it with an
empty sample, and a subsequent fixed-step control update preserves the trim.
Existing exact legacy-control, signed-zero, input-configuration and transient
release regressions also pass.

## Boundaries and integration follow-up

This corrects only a reset edge already delivered to Bevy in the sampled frame.
It does not guarantee that every physical tap survives arbitrary OS delivery or
no-step scheduling. A released tap whose sampled frame executes no accepted
control step may expire; carrying it forward is deliberately outside this change.
The released-tap branch conservatively rejects the entire frame if a shortcut
modifier has any press/release edge, even if that modifier belonged to an earlier,
unrelated gesture in the same frame. The sampler cannot reconstruct their event
order. Held K retains its previous current-state semantics: releasing a shortcut
modifier while K remains held permits reset immediately. No event-order history
or deferred command is added.

No caller, simulation transaction, FDM law, profile, replay format or UI changes
are included. The input-only ECS test checks sampling without a control step; it
does not simulate the application's pause, focus, map or failed-physics paths.
Their existing app tests, including
`live_lateral_trim_survives_pause_focus_and_map_capture_without_hidden_commands`,
`no_step_keeps_controls_full_state_recorder_and_pending_parking_toggle`, and
`rejected_full_state_step_keeps_controller_trim_toggle_clock_and_exportable_terminal`,
remain for the integration gate. App tests, native execution and physical-device
checks were not run in this scoped lane. Native rechecking must use a binary built
from the integrated source; this CPU regression alone is not native acceptance.
