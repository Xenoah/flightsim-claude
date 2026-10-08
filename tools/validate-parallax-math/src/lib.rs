// SPDX-License-Identifier: MIT OR Apache-2.0
//! A readable f32 CPU model of the new shader, not a GPU execution oracle.
//! Tests compare this model with independently derived analytic intersections.

#[derive(Clone, Copy, Debug)]
pub struct Parameters {
    pub depth_scale: f32,
    pub max_layer_count: f32,
    pub max_steps: u32,
    pub original_uv: [f32; 2],
    pub view: [f32; 3],
    pub slot: u32,
}

impl Default for Parameters {
    fn default() -> Self {
        Self {
            depth_scale: 0.1,
            max_layer_count: 32.0,
            max_steps: 5,
            original_uv: [0.25, 0.5],
            view: [0.6, 0.0, -0.8],
            slot: 7,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Outcome {
    pub uv: [f32; 2],
    pub depth: f32,
    pub layers: u32,
    pub samples: u32,
    pub refinements: u32,
    pub bracket: [f32; 2],
}

pub fn trace(p: Parameters, relief: bool, mut height: impl FnMut([f32; 2], u32) -> f32) -> Outcome {
    let mut result = Outcome {
        uv: p.original_uv,
        depth: 0.0,
        layers: 0,
        samples: 0,
        refinements: 0,
        bracket: [0.0, 0.0],
    };
    if p.max_layer_count < 1.0 || p.depth_scale == 0.0 {
        return result;
    }
    let extent = p.view.iter().copied().map(f32::abs).fold(0.0, f32::max);
    if extent == 0.0 || (p.view[0] == 0.0 && p.view[1] == 0.0) {
        return result;
    }
    let scaled = p.view.map(|v| v / extent);
    let length = scaled.iter().map(|v| v * v).sum::<f32>().sqrt();
    let direction = scaled.map(|v| v / length);
    let incidence = direction[2].abs();
    let denominator = incidence.max(0.0001);
    let scale = p.depth_scale.clamp(-1.0e20, 1.0e20);
    let ray_uv = [
        scale * (direction[0] / denominator),
        scale * (-direction[1] / denominator),
    ];
    let budget = p.max_layer_count.min(1024.0).floor();
    result.layers = (1.0 + (budget - 1.0) * (1.0 - incidence)).ceil() as u32;
    let uv_at = |t: f32| {
        [
            p.original_uv[0] + t * ray_uv[0],
            p.original_uv[1] + t * ray_uv[1],
        ]
    };
    let mut sample = |uv| {
        result.samples += 1;
        height(uv, p.slot).clamp(0.0, 1.0)
    };
    let first_height = sample(p.original_uv);
    if first_height == 0.0 {
        return result;
    }
    let mut lo = 0.0;
    let mut flo = -first_height;
    let mut hi = 1.0;
    let mut fhi = 0.0;
    for layer in 1..=result.layers {
        let t = layer as f32 / result.layers as f32;
        let uv = uv_at(t);
        let f = t - sample(uv);
        if f >= 0.0 {
            if f == 0.0 {
                result.uv = uv;
                result.depth = t;
                result.bracket = [t, t];
                return result;
            }
            hi = t;
            fhi = f;
            break;
        }
        lo = t;
        flo = f;
    }
    if relief {
        for _ in 0..p.max_steps.min(24) {
            let mid = lo + 0.5 * (hi - lo);
            if mid <= lo || mid >= hi {
                break;
            }
            let uv = uv_at(mid);
            let f = mid - sample(uv);
            result.refinements += 1;
            if f == 0.0 {
                result.uv = uv;
                result.depth = mid;
                result.bracket = [mid, mid];
                return result;
            }
            if f > 0.0 {
                hi = mid;
                fhi = f;
            } else {
                lo = mid;
                flo = f;
            }
        }
    }
    let span = fhi - flo;
    let fraction = if span > 0.0 {
        (-flo / span).clamp(0.0, 1.0)
    } else {
        0.5
    };
    result.depth = lo + fraction * (hi - lo);
    result.uv = uv_at(result.depth);
    result.bracket = [lo, hi];
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn near(actual: f32, expected: f64, tolerance: f64) {
        assert!(
            (f64::from(actual) - expected).abs() <= tolerance,
            "actual {actual:?}, expected {expected:?}, tolerance {tolerance:?}"
        );
    }

    // Independent reference: substitute the ray into h(u,v)=a+bu+cv and
    // solve t = a+b(u0+dx*t)+c(v0+dy*t) algebraically, with f64 arithmetic.
    fn affine_root(p: Parameters, a: f64, b: f64, c: f64) -> (f64, [f64; 2]) {
        let scale = f64::from(p.depth_scale);
        let z = f64::from(p.view[2]).abs();
        let delta = [
            scale * f64::from(p.view[0]) / z,
            -scale * f64::from(p.view[1]) / z,
        ];
        let initial = [f64::from(p.original_uv[0]), f64::from(p.original_uv[1])];
        let root = (a + b * initial[0] + c * initial[1]) / (1.0 - b * delta[0] - c * delta[1]);
        (
            root,
            [initial[0] + root * delta[0], initial[1] + root * delta[1]],
        )
    }

    #[test]
    fn constants_have_closed_form_depth_and_uv_in_both_modes() {
        for relief in [false, true] {
            for h in [0.0, 0.001, 0.125, 0.37, 0.999, 1.0] {
                for steps in [0, 1, 5, 24, u32::MAX] {
                    let p = Parameters {
                        max_steps: steps,
                        ..Parameters::default()
                    };
                    let out = trace(p, relief, |_, _| h);
                    let (depth, uv) = affine_root(p, f64::from(h), 0.0, 0.0);
                    near(out.depth, depth, 2.0e-7);
                    near(out.uv[0], uv[0], 2.0e-7);
                    near(out.uv[1], uv[1], 2.0e-7);
                }
            }
        }
    }

    #[test]
    fn affine_slopes_match_independent_equations() {
        for relief in [false, true] {
            for b in [-0.5, -0.1, 0.0, 0.3, 0.8] {
                for c in [-0.25, 0.0, 0.25] {
                    for scale in [-0.4, 0.02, 0.4] {
                        let p = Parameters {
                            depth_scale: scale,
                            view: [0.6, 0.4, -0.7],
                            original_uv: [0.2, 0.3],
                            ..Parameters::default()
                        };
                        let (root, expected_uv) = affine_root(p, 0.4, b, c);
                        assert!(root > 0.0 && root < 1.0);
                        let out =
                            trace(p, relief, |uv, _| 0.4 + b as f32 * uv[0] + c as f32 * uv[1]);
                        near(out.depth, root, 5.0e-7);
                        near(out.uv[0], expected_uv[0], 5.0e-7);
                        near(out.uv[1], expected_uv[1], 5.0e-7);
                    }
                }
            }
        }
    }

    // p gives u(t)=t and v(t)=0, so hand-authored scalar fields can be used.
    fn unit_ray(layers: f32, steps: u32) -> Parameters {
        Parameters {
            depth_scale: 1.0,
            max_layer_count: layers,
            max_steps: steps,
            original_uv: [0.0, 0.0],
            view: [1.0, 0.0, -1.0],
            slot: 3,
        }
    }

    #[test]
    fn quadratic_field_has_known_root_and_relief_improves_it() {
        // h(t)=0.2+0.2t^2; the first root is (1-sqrt(0.84))/0.4.
        let expected = (1.0_f64 - 0.84_f64.sqrt()) / 0.4;
        let p = unit_ray(1.0, 8);
        let field = |uv: [f32; 2], _: u32| 0.2 + 0.2 * uv[0] * uv[0];
        let ordinary = trace(p, false, field);
        let relief = trace(p, true, field);
        near(ordinary.depth, 0.25, 1.0e-7);
        near(relief.depth, expected, 2.0e-6);
        assert!(
            (f64::from(relief.depth) - expected).abs()
                < (f64::from(ordinary.depth) - expected).abs()
        );
        assert_eq!(relief.refinements, 8);
        assert!(relief.bracket[1] - relief.bracket[0] <= 1.0 / 256.0);
    }

    #[test]
    fn relief_brackets_a_transverse_nonlinear_root_with_error_bound() {
        let expected = (1.0_f64 - 0.84_f64.sqrt()) / 0.4;
        for steps in 0..=15 {
            let out = trace(unit_ray(5.0, steps), true, |uv, _| {
                0.2 + 0.2 * uv[0] * uv[0]
            });
            let tolerance = 1.0 / f64::from(out.layers) / 2.0_f64.powi(steps as i32) + 2.0e-7;
            near(out.depth, expected, tolerance);
            assert!(f64::from(out.bracket[0]) <= expected + 2.0e-7);
            assert!(f64::from(out.bracket[1]) >= expected - 2.0e-7);
        }
    }

    #[test]
    fn ordered_search_selects_first_resolvable_crossing() {
        // F(t)=0.2(t-.2)(t-.5)(t-.8), with roots .2,.5,.8.
        // h=t-F is in [0,1] and the first positive lobe is sampled.
        for relief in [false, true] {
            let out = trace(unit_ray(64.0, 12), relief, |uv, _| {
                let t = uv[0];
                t - 0.2 * (t - 0.2) * (t - 0.5) * (t - 0.8)
            });
            near(out.depth, 0.2, if relief { 3.0e-6 } else { 0.003 });
            assert!(out.depth < 0.5);
        }
    }

    #[test]
    fn narrow_unsampled_crossing_is_explicitly_not_guaranteed() {
        // The .20..25 positive lobe fits between the two coarse endpoints.
        // Even binary refinement of that coarse interval finds the later .8.
        let out = trace(unit_ray(1.0, 16), true, |uv, _| {
            let t = uv[0];
            t - 0.2 * (t - 0.2) * (t - 0.25) * (t - 0.8)
        });
        near(out.depth, 0.8, 4.0e-6);
        assert!(out.depth > 0.25);
    }

    #[test]
    fn disabled_depth_and_layer_budget_never_sample() {
        for relief in [false, true] {
            for layers in [-100.0, 0.0, 0.999] {
                let p = Parameters {
                    max_layer_count: layers,
                    ..Parameters::default()
                };
                assert_eq!(
                    trace(p, relief, |_, _| panic!("disabled")).uv,
                    p.original_uv
                );
            }
            let p = Parameters {
                depth_scale: 0.0,
                ..Parameters::default()
            };
            assert_eq!(
                trace(p, relief, |_, _| panic!("disabled")).uv,
                p.original_uv
            );
        }
    }

    #[test]
    fn normal_and_zero_directions_are_identity_without_sampling() {
        for view in [[0.0, 0.0, 1.0], [0.0, 0.0, -1.0], [0.0, 0.0, 0.0]] {
            let p = Parameters {
                view,
                ..Parameters::default()
            };
            for relief in [false, true] {
                let out = trace(p, relief, |_, _| panic!("stationary uv"));
                assert_eq!(out.uv, p.original_uv);
                assert_eq!(out.samples, 0);
            }
        }
    }

    #[test]
    fn zero_height_is_an_exact_surface_hit() {
        for relief in [false, true] {
            let p = Parameters::default();
            let out = trace(p, relief, |_, _| 0.0);
            assert_eq!(out.uv, p.original_uv);
            assert_eq!(out.depth, 0.0);
            assert_eq!(out.samples, 1);
        }
    }

    #[test]
    fn endpoint_one_is_exact_and_budget_is_bounded() {
        for relief in [false, true] {
            let out = trace(unit_ray(100_000.0, u32::MAX), relief, |_, _| 1.0);
            assert_eq!(out.depth, 1.0);
            assert_eq!(out.samples, out.layers + 1);
            assert!(out.layers <= 1024);
            assert_eq!(out.refinements, 0);
        }
    }

    #[test]
    fn samples_keep_the_material_binding_slot() {
        let p = Parameters {
            slot: 12345,
            ..Parameters::default()
        };
        let mut calls = 0;
        let out = trace(p, true, |_, slot| {
            assert_eq!(slot, 12345);
            calls += 1;
            0.37
        });
        assert_eq!(out.samples, calls);
    }

    #[test]
    fn ray_direction_has_requested_y_flip_and_z_sign_symmetry() {
        let p = Parameters {
            view: [0.3, 0.4, -0.5],
            ..Parameters::default()
        };
        let a = trace(p, false, |_, _| 0.5);
        let b = trace(
            Parameters {
                view: [0.3, 0.4, 0.5],
                ..p
            },
            false,
            |_, _| 0.5,
        );
        assert_eq!(a.uv, b.uv);
        assert!(a.uv[0] > p.original_uv[0]);
        assert!(a.uv[1] < p.original_uv[1]);
        near(a.uv[0], 0.28, 1.0e-7);
        near(a.uv[1], 0.46, 1.0e-7);
    }

    #[test]
    fn view_magnitude_does_not_change_projection_or_budget() {
        let p = Parameters::default();
        let baseline = trace(p, true, |_, _| 0.37);
        for magnitude in [1.0e-20, 0.01, 100.0, 1.0e30] {
            let out = trace(
                Parameters {
                    view: p.view.map(|v| magnitude * v),
                    ..p
                },
                true,
                |_, _| 0.37,
            );
            near(out.uv[0], f64::from(baseline.uv[0]), 1.0e-7);
            assert_eq!(out.layers, baseline.layers);
        }
    }

    #[test]
    fn normal_to_grazing_layer_counts_are_monotone_and_bounded() {
        let mut previous = 0;
        for z in [10000.0, 100.0, 10.0, 3.0, 1.0, 0.1, 0.0] {
            let out = trace(
                Parameters {
                    view: [1.0, 0.0, z],
                    ..Parameters::default()
                },
                false,
                |_, _| 0.5,
            );
            assert!(out.layers >= previous && out.layers >= 1 && out.layers <= 32);
            previous = out.layers;
        }
        assert_eq!(previous, 32);
    }

    #[test]
    fn fractional_budget_never_rounds_above_requested_maximum() {
        for max_layer_count in [1.0, 1.5, 2.9, 32.9] {
            let p = Parameters {
                max_layer_count,
                view: [1.0, 0.0, 0.0],
                ..Parameters::default()
            };
            let out = trace(p, false, |_, _| 0.5);
            assert_eq!(out.layers, max_layer_count.floor() as u32);
        }
    }

    #[test]
    fn grazing_and_very_large_finite_inputs_stay_finite() {
        for relief in [false, true] {
            for z in [0.0, 1.0e-10, -1.0e-10] {
                let p = Parameters {
                    view: [1.0, 0.0, z],
                    ..Parameters::default()
                };
                let out = trace(p, relief, |_, _| 0.5);
                assert!(out.uv.iter().all(|v| v.is_finite()));
                near(out.uv[0], 500.25, 0.001);
            }
            let p = Parameters {
                depth_scale: f32::MAX,
                original_uv: [f32::MAX, -f32::MAX],
                view: [f32::MAX, f32::MAX, 0.0],
                ..Parameters::default()
            };
            let out = trace(p, relief, |_, _| 0.5);
            assert!(out.uv.iter().all(|v| v.is_finite()));
        }
    }

    #[test]
    fn uv_edges_are_not_wrapped_or_clamped_by_the_search() {
        let p = Parameters {
            original_uv: [0.99, 0.01],
            view: [1.0, 1.0, 0.1],
            ..Parameters::default()
        };
        let out = trace(p, false, |_, _| 1.0);
        assert!(out.uv[0] > 1.0 && out.uv[1] < 0.0);
    }

    #[test]
    fn out_of_range_finite_heights_are_clamped() {
        for relief in [false, true] {
            assert_eq!(trace(unit_ray(32.0, 5), relief, |_, _| -1.0).depth, 0.0);
            assert_eq!(trace(unit_ray(32.0, 5), relief, |_, _| 2.0).depth, 1.0);
        }
    }

    #[test]
    fn zero_refinement_steps_match_ordinary_interpolation() {
        let p = unit_ray(5.0, 0);
        let field = |uv: [f32; 2], _: u32| 0.2 + 0.2 * uv[0] * uv[0];
        let a = trace(p, false, field);
        let b = trace(p, true, field);
        assert_eq!(a.uv, b.uv);
        assert_eq!(a.samples, b.samples);
        assert_eq!(b.refinements, 0);
    }

    #[test]
    fn ordinary_ignores_max_steps_and_relief_has_a_fixed_work_ceiling() {
        let field = |uv: [f32; 2], _: u32| 0.2 + 0.2 * uv[0] * uv[0];
        let p = unit_ray(32.0, u32::MAX);
        let ordinary = trace(p, false, field);
        assert_eq!(
            ordinary.uv,
            trace(Parameters { max_steps: 0, ..p }, false, field).uv
        );
        let relief = trace(p, true, field);
        assert!(relief.refinements <= 24);
        assert!(relief.samples <= relief.layers + 1 + 24);
        assert!(relief.uv.iter().all(|v| v.is_finite()));
    }
}
