# Runtime graphics quality

The default **LIGHT** preset preserves the existing renderer and scene. Use
`--graphics-quality light|high|ultra` at startup or **F4** to cycle during flight,
including while paused or replaying. **Shift+F4** returns directly to LIGHT;
the HUD shows the selected preset. The map captures these keys while open and
on its closing frame. The setting is session-local and is not saved into replay.
These tiers provide a modest foundation for lighting and shadow improvements
on the existing scenery.

```sh
cargo run -p flightsim-app --release -- --graphics-quality high --view chase
```

All tiers use the same aircraft, buildings, trees, runways, terrain meshes,
textures, material properties, physics and streaming/upload budgets.
`--surface-detail off` stays off in every tier. Higher settings improve how the
existing materials receive the simulated sky's diffuse light and reflections;
this is not photographed scenery, ray tracing or a photorealism guarantee.

| Rendering resource | LIGHT (default) | HIGH | ULTRA |
|---|---:|---:|---:|
| Sky LUT ray samples | 16 | 24 | 32 |
| Aerial LUT samples per slice | 10 | 16 | 24 |
| Directional shadow map side | 2048 | 2048 | 4096 if supported |
| Generated sky cubemap side (6 faces) | none | 128 | 256 |
| Generated diffuse cubemap side | none | 32 | 32 |
| Generated specular mip levels | none | 8 | 9 |
| Additional generator entities | 0 | 1 | 1 |
| Additional generated image assets | 0 | 3 | 3 |

Sky/aerial LUT dimensions remain 400×200 and 32³. Transmittance and
multiscattering settings, earthlike scattering coefficients, MSAA4, Gaussian
shadow filtering, exposure, tone mapping, one sun, four shadow cascades and the
2 km shadow reach remain unchanged. ULTRA has four times as many shadow texels
per cascade as LIGHT; these are resource budgets, not measured GPU timings.

The upper tiers use the simulated sky's illumination with intensity 0.35.
Existing ambient light is retained in every tier, including while Bevy compiles
shaders and produces the maps asynchronously. A selected tier can appear
gradually after a switch. Retaining ambient light alone does not establish
continuous mesh visibility during first-use pipeline compilation. A render-world
gate asks Bevy to prepare the exact variants needed by the current visible
materials and retains their baseline draw pipelines until those variants are
ready. Missing or failed variants never qualify through a frame-count timeout.
A fresh-cache native retest retained the complete scene in all 160 observed
samples. First use still incurs compilation delay and can stall frames; the gate
does not make switching instant or hitch-free. No fixed number of CPU frames is
treated as proof of GPU completion.
The generated map contains the sun-driven atmospheric sky, without an explicit
solar disk or reflections of local objects.
Generation runs every active render frame. Bevy also uses a bounded intermediate
mipmapped cubemap and its texture cache: source, filtered images and intermediate
texture total approximately 2.80 MiB for HIGH and 11.05 MiB for ULTRA in RGBA16F.
Those figures exclude existing LUTs, allocator/driver overhead and other render
scratch; they are not total GPU-memory measurements. Unused texture-cache entries
can survive three render updates, so asset tests allow additional settling.

LIGHT restores the exact camera settings and directional shadow-map size saved
before the first switch. It removes the added view environment and disposes of
the generator entity so Bevy's private generation state can be reclaimed. The
map suspends this generator while the flight view is inactive and recreates it
when the map closes. A missing or zero-sized render target or viewport also
suspends generation; restoring a valid target resumes the selected preset. The
flight camera's identity, pose, target and controls do not change.
Asset/render-cache reclamation can take several frames.

If the adapter lacks the required compute/storage capabilities, the selected
upper preset retains legacy ambient illumination and logs that sky IBL is
unavailable. If 4096 shadows are unsupported, ULTRA retains the original shadow
size and logs that fallback. Shader or validation errors remain errors to fix;
they are not suppressed or declared successful fallback.

This change downloads no runtime textures and needs no account or service.
Live texture acquisition is a separate future feature. It does not change the
ordinary release gates or resolve the existing default Bevy tone-mapping LUT
rights review. See [graphics QA](qa/graphics-quality-2026-10-03.md) for the actual
verification state and hardware limitations.

With `--render-stats`, lifecycle diagnostics report flight-camera/helper identities,
owned image IDs and main/render asset counts every five seconds. Counts of active
GPU images exclude retained texture-cache and driver allocations. They are useful
for checking repeated-switch cleanup, not measuring GPU memory or frame time.

The fixed candidate passed focused CPU checks, observed native cold-switch
visibility, warmup cancellation/retry and repeated tier changes. Map-start and
pause controls were also exercised. Day/dusk/night images retain the existing
geometry: daylight adds cooler fill on shaded faces and ULTRA changes shadow
edges, dusk shows a modest lift, and the tested night view is effectively
unchanged. Higher settings do not universally brighten the image or guarantee
photorealism. LIGHT restored practical appearance with small pixel differences;
exact pixel equality was not met. Independent review found no practical geometry
or lighting blocker in the
nine images. The linked QA record distinguishes the earlier cold-switch failure,
fixed-candidate evidence and remaining limits.

Pipeline gating changes only the environment-map shader key for the existing
flight view. It uses Bevy's public specialization/cache APIs for current prepared
meshes and materials, including later-loaded variants, and preserves other views
and any authored baseline environment. While compilation is pending, it can add
one specialization pass over the current visible scene; this is not a measured
frame-time bound. LIGHT requests no prewarming. Bevy's shared compiled shader and
pipeline caches can remain after upper-tier use, independently of the reclaimed
helper and generated images.
