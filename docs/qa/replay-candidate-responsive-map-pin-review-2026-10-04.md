# Strict candidate binding for the responsive world map

Date: 2026-10-04 UTC. Reviewed integrated source:
`9474d059dd0a00fef46a214bbf85b9d4f1636acf`. Accepted pure-foundation and binding
baseline: `8dd34f77ea13f799dd9c268fcbbbe4f69307e494`.

This review binds the presentation path added by `e747c0e` and corrected by
`9474d05`. It changes only the required source set, manifest, additive Python
rejection tests and this record. App admission, production Rust, release gates,
commercial payload and historical evidence remain unchanged by this binding.

## Exact source and ownership review

The two map commits are integrated from author `bb7ba19618d56772aa1ce774b8eea5e576a4a207`
and its accepted correction `ee80cdeafff991de49257ab867060c59a210a97a`.
The complete five changed/added UI files, including all tests, have identical
raw Git blobs, SHA-256 values and checkout bytes at the accepted author and
integrated source. No separately developed near-static app changes are included.

The complete baseline-to-integration tree contains 2,306 baseline regular files:
2,301 are unchanged, five change, three are added and none are removed. The
five changes are `ARCHITECTURE.md`, `docs/global-map.md` and UI `regions.rs`,
`wind_settings.rs`, `world_map.rs`. The additions are `world_map_layout.rs`,
`world_map_layout_tests.rs` and the [small-window map QA record](world-map-small-window-2026-10-04.md).
All app, physical, transaction, replay, input, audio, renderer, profile, manifest,
lockfile, workflow, staging and release-policy files retain their baseline bytes.

Within the changed UI sources, the complete input/transaction handlers, credit
pagination, text formatting and bounds are byte-identical. Regions and wind code
outside their spawn functions is unchanged, apart from the region layout import.
The original 62-test world-map module and assertions remain intact after removing
only the additive six-test module include and real visibility/Mesh fixture setup.

The existing display owner calls the new presentation parameter every frame.
It selects stacked layout below 900 logical pixels wide or 600 high, keeps
desktop columns at the reachable scroll origin, and updates measured scroll
bounds without changing physical or pending-flight state. The map, Credits,
Regions and Wind retain separate scroll ownership. Hidden wheel input drains;
opening/closing frames do not scroll a different modal; whole-map close resets
positions while child dismissal retains the map's place. Finite-event sums and
logical-scale scroll bounds remain guarded. Start/Cancel generations, replay
locks, explicit download actions, fonts and the 2:1 map contract remain intact.

## Additive whole-file boundary

The required set grows from **114 to 117 whole files**. All previous paths remain.
Only two previous hashes advance to their reviewed integrated values:

| File under `crates/flightsim-ui/src/` | Previous SHA-256 | Reviewed SHA-256 |
|---|---|---|
| `world_map.rs` | `1e964d464cb9a09a4e891cf4e2577c0e479c972c1725d64ae6c0646e8409b709` | `7243c202471fced2397580e48f78d23036f277f0d1d22a90457166fd136bf2da` |
| `wind_settings.rs` | `d34838c953df5bc91f61e1206524dee90992bec5b16915b1c9e717336bae3e7a` | `4784a1484116fdd12100a80ace4782ad438f5fafb016d62ceb39b1cc7eada0a5` |

The other **112 previous source hashes and all 102 independent anchors remain
unchanged**, as do the five historical hashes, historical baseline, partial
fingerprint and strict FDM-root guard. Three complete sources join the boundary:

- `regions.rs`: the staged map's child presentation and explicit control route
- `world_map_layout.rs`: reflow, modal wheel ownership and measured bounds
- `world_map_layout_tests.rs`: actual glyph, ancestor-clip and pointer witnesses

These are direct dependencies/witnesses of the already pinned map and wind
route. They belong to that source boundary without creating app, replay,
physical-law or distribution admission. No source projection, normalized hash,
wildcard boundary or rewritten fixture is used. The reviewed-source pointer names
`9474d05`; the manifest SHA-256 is
`c8f309d413a097ace8a800e8d5d36421f543fff6c7085b6cea8dcf9d7cf76d92`.

AST comparison proves the checker's only code change is three explicit path
additions. All complete function sources, other constants, source/evidence
validators, acceptance commands, MSVC/default-feature requirements, Swift-only
identity, literal false release authorization, capture and diagnostic limits,
rights and release gates retain their original implementations.

## Executed checks and limits

All **183 original Python test functions and assertions are byte-preserved**.
Two additive tests bring the six-suite result to **185 passed**:

- Fourteen independently committed semantic mutations reject before compilation:
  omitted layout update, wrong map/child surface, reduced breakpoint, unreachable
  centered desktop origin, lost logical scale, unchecked overflow, hidden-modal
  wheel admission, lost parent position, opening-frame leakage, missing clamp,
  weakened clipped-pointer and finite-offset witnesses
- All five map-route sources reject missing file, missing reviewed record,
  altered canonical digest and removed contract row, including recomputed outer
  evidence hashes: **20 additional exported-evidence attacks**

The unchanged exhaustive source-drift test now covers all **219 source/anchor
paths**. Existing CRLF rejection, raw-notice preservation, original reference
encoders, old/new app admission mutations and synthetic success/failure export
checks remain passing. The full command is:

```sh
python3 -m unittest -v scripts.tests.test_swift_windows_candidate scripts.tests.test_stage_commercial_candidate scripts.tests.test_commercial_readiness scripts.tests.test_release_authorization scripts.tests.test_release_workflow scripts.tests.test_ci_smoke_workflow
```

Read-only Python/Git audits verify the exact tree scope, old Rust handler/test
bodies, accepted-author byte equivalence, every retained pin and all eight
author log-inventory entries plus 16 independent-review artifacts. The prior
independent acceptance receipt SHA-256 is
`5e330652297e967ec71e259cb598f7d5bb59aaabb0bdcc2212e75e7343071686`.
Its author-executed 245 UI tests, strict UI Clippy and seven-viewport evidence
remain prior evidence, not new execution by this binding review. The retained
formatting log is empty; it does not independently establish an exit status.

Whitespace checks pass. The local architecture command was attempted but exits
2 because this Python-only shell has no `cargo` on PATH; no Rust build/test/native
process was run. The integration owner's actual Rust/native checks remain a
separate gate. The clean final commit is checked by the unchanged `source_inputs`
collector and exported-source validator, and every tracked raw Git blob is
compared with its checkout size and SHA-256 in the external receipt.

The inherited covered pause-reference wheel behavior identified by independent
review remains unchanged. Final integrated native rendering, Windows scene
capture, hardware/speaker acceptance and release authorization are not established
here. Synthetic validator fixtures do not represent a Windows candidate, and no
publication is performed.
