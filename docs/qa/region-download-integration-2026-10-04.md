# Region catalog application integration — 2026-10-04

This milestone adds the opt-in download catalog to the existing map's Regions
panel. The [component record](app-region-downloads-2026-10-04.md) covers validation,
worker/cancellation tests and independent review. The existing default application
and Swift-only candidate continue to compile without regional network acquisition.
No public catalog or automatic raw-DEM/repository conversion is supplied.

## Native application evidence

The Linux cloud desktop uses llvmpipe CPU Vulkan. These are native keyboard and
actual rendered-image checks, not Windows, hardware-GPU or frame-rate claims.
Automatic captures used the app's normal screenshot path and exited successfully.
The displayed map can suspend its flight camera; map frame rates are not flight
performance measurements.

1. An empty offline cache failed cleanly without changing the active or pending
   global terrain. Placing the independently generated synthetic ZIP in that
   private cache did not trigger an automatic retry. Explicit retry verified and
   installed it; installation still left both selections global. Choosing its
   Installed row, returning to the map and pressing Start activated the package.
   Closing/reopening the map preserved the current-flight boundary. The existing
   package-backed replay guard rejected F9 recording.
2. The previously prepared Liechtenstein GLO-90 package was seeded into a private
   offline cache. Its catalog uses an explicitly labeled placeholder GitHub URL,
   which was never contacted. The actual 765-tile package installed, the credits
   panel displayed its original attribution/license references, and an explicit
   Installed selection followed by Start loaded regional terrain in a new flight.
   The terrain log reached 38 displayed/live/desired tiles with matching sets.
3. Refresh of an unchanged catalog retained its inert preview. Changing the
   provenance cleared that preview and required explicit reselection. Invalid
   JSON cleared the downloadable entries and showed the parse error. Restoring
   the catalog and explicitly selecting its row recovered the preview. None of
   these refreshes changed the active global flight or removed installed content.

The real-package archive SHA-256 was
`fc4de5f479d769b0aa6f028ab965a8798f9c11bdbae157bffc57ecd45df66b2a`.
This checks integrity of the existing prepared input; it is not new publisher or
redistribution-rights approval. The native run does not establish online download
success: that is separately exercised by the production CLI in Windows/Linux CI.

The first run used code `f08f1bbf95818af8fc4c51943923faf9b7bf1812`;
the real-area and refresh runs used `029c1b7ffe2ce87c38cca18713f94caef7787a6d`.
The latter binary SHA-256 was
`b9fed08318a44d6e4fbf908e1c1993602f303bcb99319ceaab0f79e65e92a9b3`.
Durable application captures:

| Case | PNG SHA-256 | App exit |
|---|---|---|
| Synthetic cache/install/start | `df50cd45946c727ff7fdd48c5e5a3d281ff244ad63fc56a6b29933f7fdea7637` | 0 |
| Real Liechtenstein regional flight | `909d5dc4a85cbab845099dec7894a64e5ec6630a2ec7c5f3e57a4e901ba9d21c` | 0 |
| Catalog restoration and explicit preview | `d477133cb1931b0460629b39c4b399473b3bf0cbe1e43af28638d25d6e2e1d08` | 0 |

Native review found that “NOT YET INSTALLED” could describe an already installed
catalog entry. The heading is now the accurate “PREVIEW ONLY”; it makes no claim
about installation state. This is the only UI wording correction after those runs.
The final code `fe7dae2f207878e9d27815de4f4a34a86fee9a1b` rebuilt successfully,
passed all 26 focused region runtime tests, and displayed the corrected preview
in a fourth native run (exit 0). Its executable SHA-256 is
`668dff95be166d946538423620cd87689549c5e501709e452fc9b84f96991bff`;
the final panel PNG SHA-256 is
`f88604fe033d884f565424347cd5caf8e84472d8fc26b1e4664644b3e57c29ed`.
An initial overly specific test filter selected zero tests and is not counted as
verification; the corrected module filter ran the 26 checks above.

The small cached install completed before the native cancel could take effect.
That sequence does not qualify in-flight cancellation; deterministic pending-worker
and late-completion tests provide that coverage. Mouse activation and file drag/drop
were not requalified in these keyboard runs. The regional flight starts airborne;
it is not a takeoff, landing or complete aircraft-handling qualification.

## Compiled feature and packaging identity

Real compiled distribution tests passed for default, `region-downloads`, and
`commercial-staging,region-downloads` builds, six checks per combination. The actual
enabled Linux executable reports `region_downloads: true`; default reports false.
The [separate optional dependency inventory](../release/region-download-dependencies.md)
records 408 Windows normal/build packages and 786 original notice files, with its
review status and four unresolved entries preserved. It is not an approved bundle
recipe. Default 359-package evidence and both asset allowlists remain unchanged.

The identity and gate change independently passed 135 focused Python checks:
59 commercial/stager, 51 release/workflow, and 25 Swift-candidate checks. Existing
offline recipes require a literal false network-feature identity and reject mixed
dependency inventories. The trusted authorized release build has an explicit
postbuild executable identity check. Generic archive verification remains
non-executing; this change adds no implicit execution of supplied archives.

Ordinary CI now additionally tests and lints the enabled app/content/assetgen feature
combination on Linux and Windows, retaining the default tests and real public
download/offline-reuse jobs. Final-head remote CI is a separate publication check;
this record does not predeclare it green.

## Remaining release boundary

This is a source milestone. The preceding Meadow source's ordinary CI passed all
10 jobs at `01bc7da9d44eb7813edda17495d49b26c82330c4`, but its separate Windows
candidate run 37180988108 failed its 180-second screenshot capture, as did its
alternate-launcher diagnostic. The evidence contains no PNG. Binary build and
publication in release run 37180988121 were skipped. That unresolved Windows
readback issue and the independent rights/dependency review gates remain open.
No new binary release, legal clearance or complete weather/aircraft expansion is
claimed by this catalog milestone.
