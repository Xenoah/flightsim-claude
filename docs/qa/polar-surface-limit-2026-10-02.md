# Polar surface diagnosis: remaining visual limit

## Actual scene evidence

The settled software-Vulkan South-Pole capture used the current Swift replay,
December 21 climate, solar epoch 2461396.0, approximately 2800.079 m ellipsoidal
ground and 350 m AGL, with procedural surface detail **off**. Its log confirmed
4094 displayed/live/desired surfaces and 8185 visible bridges, no pending work,
and successful screenshot/process completion. Radial ground striping remains
visible. An earlier 25-second capture still displayed two fallback roots and is
not evidence of fine-cut acceptance. Private images/logs are retained outside
Git; this addendum does not publish raw telemetry.

The cut reaches the existing 4096-leaf detail budget (`truncated=true`) while
retaining complete globe coverage. Its level counts are:

- L1–L8: 4, 8, 16, 32, 64, 128, 256, 512
- L9: 1706; L10: 1368

The immediate polar bands are **L9/L10**, not L13. The separately measured L13
normal-quantization extreme therefore cannot establish this scene's cause.
The committed exact-pole endpoint/normal correction remains valid, but it does
not solve the interior radial appearance.

## Source-preserving numerical checks

A read-only probe selected that same default 350 m-AGL cut and evaluated 4644
non-pole mesh vertices within 5 km, sampling polar longitude tiles at a fixed
16-tile stride. It compared generated normals, a wider within-tile 1 m east-west
stencil, and local f64 atlas samples at 100/250 m physical offsets. Angles below
are radians; the wider samples are **filtered comparisons, not surveyed truth**.

| Comparison | Median | 95th percentile | Maximum |
| --- | ---: | ---: | ---: |
| Original normal versus 100 m atlas stencil | 0.001705 | 0.032543 | 0.035677 |
| Within-tile 1 m stencil versus 100 m atlas stencil | 0.001698 | 0.031302 | 0.035677 |
| 100 m versus 250 m atlas stencils | 0.000669 | 0.002278 | 0.006384 |
| Adjacent same-level boundary normal jump, 516 pairs | 0.000032 | 0.000063 | 0.001186 |

A second control rebuilt the **same triangles** directly from f64 atlas samples,
removing generated-DEM f32 height quantization while preserving the topology.
The angular difference from the current normals was median 0.000000042, p95
0.000000177 and maximum 0.001051 rad. The f64-only triangles still reached
0.033194 rad (about 1.90 degrees) of slope. Thus quantization contributes some
outliers, but most of the angular relief is already present before that narrowing.

The bundled atlas's polar cap blends longitude-dependent last-row heights toward
one pole mean. Longitude-grid spacing contracts physically near the pole, while
those source-height variations remain. The numerical controls support a remaining
**polar-cap/source-projection contribution**, rather than isolating a safe general
seam-normal or f32-rounding fix. They do not prove every visible stripe has one
cause. The within-tile stencil does not test cross-tile source continuity, and
the boundary sample excludes mixed-level/bridge normals. Per-vertex slope also
feeds the biome palette; changing normals alone need not remove baked colour
variation. Skirt/bridge depiction and independent lighting/shadow effects remain
separate diagnostic possibilities.

## Rejected changes and colour check

- The proposed 1 m normal stencil barely improved the broad comparison and did
  not reduce its maximum. It is not an evidence-backed fix for this frame.
- Most same-level boundary jumps were much smaller than the interior source
  gradients. Generic seam smoothing would target the wrong dominant effect.
- A much wider normal filter would intentionally suppress retained source
  gradients. No source-aware model, transition policy, cost bound and native
  regression set has established that change as appropriate for this delivery.

The December climate/palette probe at the South Pole gave −21.85 °C,
`snow_fraction=1`, land fraction 1, authored sRGB `[0.90, 0.94, 0.97]` and linear
RGB `[0.7874, 0.8689, 0.9331]`. Neighbouring samples agree. Solar elevation was
23.44 degrees. Replay startup copies the climate date before terrain preparation,
and the detail-off material leaves the base palette unchanged. No climate-data
or startup-palette wiring error was demonstrated. The screenshot's warmer grey
appearance needs actual uploaded-colour/unlit-lighting comparisons before any
palette correction; colours were not arbitrarily brightened.

Geometry, physical sampling, DEM data, normal policy and palette remain unchanged
by this diagnosis. Radial polar striping is an **unresolved visual limitation**.
Final integration/native QA and release-rights gates remain separate authorities.


## 2026-10-03 continuation

This earlier unresolved finding is retained as the diagnostic record. Fresh
source-quantization isolation and a 505 m render-only normal filter remove the
radial striping in a matched, settled South-Pole frame without changing source,
geometry or palette inputs. This is a visual filtering policy with explicit
accuracy/cost tradeoffs, not recovered fine terrain. See the
[production-candidate record](polar-visual-normals-2026-10-03.md); North-Pole and
combined-shadow acceptance remain separate gates.
