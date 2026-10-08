# Original parallax ray/height-field implementation

This is a locally prepared replacement function and verification harness. The
parent task owns vendoring, integration, release decisions, and publication.

## Integration boundary

`shader/parallaxed_uv.wgsl` contains only the new `parallaxed_uv` function. Keep
the containing shader's module/import declarations and independently retained
`sample_depth_map(uv: vec2<f32>, material_bind_group_slot: u32) -> f32` helper.
The helper must return the red depth channel, sampled at explicit LOD 0, using
the appropriate ordinary or bindless bindings. Do not replace that helper with
one of the validation stubs.

The public signature is unchanged:

```wgsl
fn parallaxed_uv(
    depth_scale: f32,
    max_layer_count: f32,
    max_steps: u32,
    original_uv: vec2<f32>,
    Vt: vec3<f32>,
    material_bind_group_slot: u32,
) -> vec2<f32>
```

`#ifdef RELIEF_MAPPING` / `#endif` are host-preprocessor directives, not WGSL.
The validation harness explicitly selects each branch before Naga parsing.

## Mathematical construction

For a nonzero view vector away from the grazing clamp, define

```text
delta = depth_scale * (Vt.x, -Vt.y) / abs(Vt.z)
uv(t) = original_uv + t * delta
F(t) = t - clamp(sample_depth_map(uv(t), slot), 0, 1)
```

The initial depth is `t=0`. A zero depth sample is an exact surface hit. Otherwise
`F(0)<0`. The search visits depths `i/N`, in order from `i=1` through `N`, and
selects the first interval whose sampled upper residual is nonnegative. A
finite, clamped height guarantees that the last sample at `t=1` closes a bracket.
Exact sampled roots return immediately.

With `RELIEF_MAPPING`, each refinement samples the midpoint and retains the
negative-to-positive bracket. Both variants finish with the root of the line
through the two bracket residuals. Consequently the ordinary variant is exact
in real arithmetic when the residual is affine over the selected bracket. An
affine raw height field is insufficient if clamping creates a kink inside that
bracket. For example, along `u(t)=t`, raw height `0.5-t` with one coarse layer
gives ordinary depth `1/3` after the endpoint height clamps to zero, whereas the
true root is `1/4`. The relief variant adds useful refinement for nonlinear or
piecewise-affine fields, and `max_steps=0` gives the ordinary result.

For `h(u,v)=a+b*u+c*v` along an unclamped ray, with no active height-clamp kink
in the selected bracket, the independent reference equation is:

```text
t* = (a + b*u0 + c*v0) / (1 - b*delta.x - c*delta.y)
```

The test oracle derives this equation directly using f64, independently of the
f32 bracket-search model. A quadratic test uses the independently solved root
of `t = 0.2 + 0.2*t*t`. Multi-root polynomial tests exercise the nearest
resolvable crossing and deliberately demonstrate a missed sub-layer crossing.

## Defensive bounds and behavior tradeoffs

- `max_layer_count < 1` or `depth_scale == 0` returns the original UV without a
  texture read. Zero view direction and exact normal incidence also return the
  original UV without a texture read, because the projected UV cannot move
- Finite view vectors are normalized after scaling by their maximum absolute
  component. This avoids squaring huge raw components and makes projection and
  layer choice invariant to view-vector magnitude for ordinary finite values
- With `c=abs(normalized_view.z)`, the integer coarse budget is
  `ceil(1 + (floor(min(max_layer_count, 1024))-1)*(1-c))`. Thus one layer is used
  at normal incidence, with more toward grazing. Fractional requested maxima
  are rounded down before interpolation. Values above 1024 are deliberately
  capped: predictable cost takes priority over honoring unusually large budgets
- The projection denominator is `max(c, 0.0001)`. Outside that narrow grazing
  region, normalization cancels and the required ray formula is preserved.
  Inside it, the UV displacement is bounded instead of following the singular
  geometric projection. This is a deliberate visual approximation at grazing
- Depth scale is clamped to `[-1e20, 1e20]`, far beyond useful material settings.
  Signed depth scales are supported. This guard limits pathological finite
  offsets; it does not establish meaningful behavior for NaN or infinity
- Finite sampled heights outside `[0,1]` are clamped into that interval
- Ordinary mode needs at most `1+N` texture reads; relief needs at most
  `1+N+min(max_steps,24)`. Exact roots and stationary UVs exit early. Refinement
  also stops when f32 cannot represent a strictly interior midpoint. Asking for
  more than 24 refinements intentionally cannot increase that cost
- UVs are not wrapped or clamped here. Texture sampler address modes remain
  responsible for behavior beyond a texture edge
- This is a finite-resolution search. It selects the nearest *sampled* crossing.
  A narrow positive residual lobe, an unsampled tangent contact, or several
  crossings within one layer may be missed. Binary refinement does not repair
  a wrong coarse interval. A universal nearest-intersection guarantee for an
  arbitrary height function is impossible from a fixed finite set of samples
- The input contract requires finite coordinates, finite material parameters,
  and finite sampled values. WGSL permits implementation-dependent handling of
  exceptional/subnormal floating-point values; this is not a NaN/Inf sanitizer

The example CPU defaults are scale `0.1`, 32 maximum layers and 5 relief steps.
The production caller still supplies every parameter; the function adds no
runtime material defaults or new shader definitions.

## Reproduce validation

With Rust/Cargo on PATH and the Naga 27.0.3 dependency cached:

```sh
cargo test --locked --offline --all-targets
cargo fmt --all -- --check
cargo clippy --locked --offline --all-targets -- -D warnings
```

The local `validate.sh` is a convenience wrapper that logs these checks. It does
not install dependencies. `CARGO_HOME` may select an existing cache, and
`CARGO_TARGET_DIR` may select a build directory. All new project files are in
this directory. Build products are excluded by `.gitignore`.

`src/lib.rs` is the reproducible f32 CPU model. Its 20 test cases compare analytic
roots, investigate nonlinear behavior, and exercise edge cases and workload
bounds. `tests/wgsl_validation.rs` contains two tests; each validates both the
ordinary and relief branches with Naga 27.0.3, once using an analytic stub and
once using an explicit-LOD ordinary texture helper, for four shader variants.

These checks establish CPU-model behavior and standalone WGSL syntax/type/
validation correctness. They do **not** execute the shader, prove CPU/GPU
equivalence, exercise Bevy's complete imports or bindless arrays, validate GPU
pixels, or establish Windows/render/release acceptance. The complete containing
shader and both actual binding paths still need integration checks.

## Authoring input and license record

This worker authored the replacement and test files from the parent task's
functional signature, ray equation, algorithm/edge-case requirements, and
binding-helper contract. This worker did not open the existing Bevy
`parallax_mapping.wgsl`, the Sun and Black Cat article, the upstream addon,
copied parallax examples, or the earlier rights dossier.

The language reference consulted was the official
[WGSL specification](https://www.w3.org/TR/WGSL/). Validation tooling uses the
already cached Naga 27.0.3 package; only its package metadata and public
`parse_str` / `Validator` API definitions were inspected for tool setup. Naga's
official project is [wgpu](https://github.com/gfx-rs/wgpu), as recorded in its
package metadata. Naga is a test-only dependency, and no crate source or archive
is part of this replacement deliverable.

These are AI-assisted materials prepared under the project's existing
`MIT OR Apache-2.0` licensing choice. SPDX markers identify that choice. No
invented human copyright notice is added. This is an authoring-input record,
not a formal clean-room certification, a legal opinion, or a claim that this
isolated replacement alone clears the entire project's release rights.
