# Procedural water quality, first slice

Water quality is independent of Graphics and Clouds quality. Light is the
existing terrain material and has no water texture, material or proxy entities.
High/Ultra shade the existing curved terrain surface; they do not replace the
planet with a flat ocean plane, change heights, or affect flight physics/replay.
The original shader and its mesh attributes are unchanged in Light.

The application must add `WaterSurfacePlugin` after `TerrainDetailPlugin`, and
supply `WaterAtlas(global.clone())` only while global terrain is enabled. Water
quality defaults to Light; High resolves six procedural bands, Ultra eight. The
shortest omitted bands still contribute to the far-field statistical roughness.

## Geographic source and allocation

A single upper-only 512×512×6 RG8 cube map stores ocean in R and independently
mapped inland water in G. It costs exactly 3,145,728 texture bytes. Classification
uses the validated bundled global atlas's land fraction and inland-water flag,
never blue vertex colors or elevation sign. Negative-elevation dry land remains
land; the Dead Sea and elevated lakes keep their existing geometry and height.
The cube is built using core's ECEF-to-geodetic conversion in batches of at most
8,192 texels per update (192 batches). It is shared across all terrain, bridges,
and High/Ultra. There is no per-frame regeneration after completion.

Cube faces use WebGPU's +X/-X/+Y/-Y/+Z/-Z conventions and seamless linear cube
filtering. There is no longitude wrap or pole UV singularity. One mip level is
used. Texel angular width is nonuniform: roughly 25 km at face centres and less
near edges. The source atlas is itself roughly 20 km. This is a coarse geographic
mask, not surveyed shorelines, coastal bathymetry or local OSM water. Conservative
coverage blending leaves mixed land/water edge samples on the original material.
Small islands/lakes and regional DEM coast details can be unresolved. A matching
high-resolution shoreline source is required before claiming detailed coasts.

Generated CPU pixels occupy one 3 MiB staging buffer; the completed image requests
render-world-only residency. Light/cancel drops staging, image and water material
owners, and despawns proxies. Each upper proxy shares the original terrain mesh
handle, has identity local transform as its baseline's child, and is restricted
to the displayed terrain/bridge cohort. No new vertex buffers are generated.
Bevy's shared pipeline cache may keep compiled shader pipelines after returning
to Light; this is distinct from owned images/materials/geometry.

## Shading and transitions

Wave normals are original procedural sums in Earth-fixed Cartesian axes. The CPU
reduces each spatial and simulation-time phase separately in f64 before narrowing
to f32, so tiles, LOD IDs, longitude, poles and render-origin rebases cannot seed
or restart the wave field. The finite set of wave slopes changes shading only.

The pixel footprint suppresses unresolved wave normals before Nyquist and moves
their squared slope energy into the microfacet distribution. Thus distant sun
reflection remains broadened by unresolved waves instead of reverting to a flat
mirror or aliasing into subpixel dots. Water uses air/water Fresnel reflectance,
a GGX sun glint with horizon and approximate atmosphere/cloud attenuation, and
an analytic sky response that follows sun elevation and cloud fraction. Graphics
Light retains these effects without an atmosphere-generated environment map.
Sun glint is an analytic response; this slice does not sample terrain shadows
into that explicit reflection. Sky reflection is an approximation, not a reflection of actual cloud shapes,
terrain, aircraft, buildings, or scene pixels. Night has only a small bounded sky
radiance and no below-horizon sun reflection. Lakes use smaller wave slopes.

Both material variants are specialized normally. Before each camera's main and
prepass queue, the water gate checks the exact current material, mesh, view/entity
ticks and pipeline state. Cold, absent or failed upper variants retain the
original terrain draw; ready pairs suppress the baseline draw. Exactly one
same-mesh forward/depth cohort is queued per camera. The unchanged baseline
remains the sole shadow caster. There is no elapsed-time readiness assumption.

This slice excludes displacement, wave geometry, foam/surf, shallow-water depth,
refraction, SSR, scene-object reflection, observed winds, water physics and
additional high-resolution coastline data. Visual acceptance and runtime proof
are recorded separately; code and numeric checks alone do not establish it.

## Controls and integrated evidence

Use F1 to cycle Light/High/Ultra and Shift+F1 to restore Light; startup accepts
`--water-quality light|high|ultra`. The HUD and pause menu show the current choice.
See [integrated QA](qa/expansion-integration-2026-10-04.md) for actual matched
views, native cancellation/rebase/cleanup and the remaining crossed-wave pattern,
coarse-coastline and performance-measurement limits.
