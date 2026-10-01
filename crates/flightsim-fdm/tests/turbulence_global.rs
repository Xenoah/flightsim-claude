//! Global physical-vector continuity regressions for issue #5.
//! NED components may rotate at a pole; compare physical gusts in the common ECEF frame.
use flightsim_core::{Ecef, Geodetic, LocalFrame, Meters, Seconds};
use flightsim_fdm::Turbulence;
use glam::DVec3;

fn world_gust(turbulence: Turbulence, time: Seconds, position: Geodetic) -> DVec3 {
    LocalFrame::new(position).ned_to_ecef_vector(turbulence.gust_at(time, position))
}

#[test]
fn neighboring_points_across_dateline_and_poles_have_neighboring_physical_gusts() {
    for seed in [1, 7, 4242] {
        for time in [0.0, 10.0, 77.25] {
            for (latitude, longitude_a, longitude_b) in [
                (0.0, 179.999_999, -179.999_999),
                (35.55, 179.999_999, -179.999_999),
                (89.999_999, 0.0, 180.0),
                (-89.999_999, 0.0, 180.0),
            ] {
                let a = Geodetic::from_degrees(latitude, longitude_a, 1000.0);
                let b = Geodetic::from_degrees(latitude, longitude_b, 1000.0);
                let separation = (a.to_ecef().as_vec() - b.to_ecef().as_vec()).length();
                assert!(separation < 0.25);
                let turbulence = Turbulence::severe(seed);
                let difference = (world_gust(turbulence, Seconds(time), a)
                    - world_gust(turbulence, Seconds(time), b))
                .length();
                // Same continuity budget as the existing 0.42 m spatial-step test.
                assert!(
                    difference < 0.2,
                    "seed {seed} t={time} latitude={latitude}: {separation} m separation produced {difference} m/s gust jump"
                );
            }
        }
    }
}

#[test]
fn equivalent_dateline_and_pole_coordinates_give_the_same_physical_field() {
    for latitude in [-90.0_f64, 0.0, 35.55, 90.0] {
        let longitudes = if latitude.abs() > 89.0 {
            [0.0, 90.0, 180.0, -90.0]
        } else {
            [180.0, -180.0, 540.0, -540.0]
        };
        let turbulence = Turbulence::severe(7);
        let reference = world_gust(
            turbulence,
            Seconds(10.0),
            Geodetic::from_degrees(latitude, longitudes[0], 1000.0),
        );
        for longitude in longitudes {
            let actual = world_gust(
                turbulence,
                Seconds(10.0),
                Geodetic::from_degrees(latitude, longitude, 1000.0),
            );
            assert!(
                (actual - reference).length() < 1e-8,
                "same location latitude {latitude} longitude {longitude}: {actual:?} vs {reference:?}"
            );
        }
    }
}

#[test]
fn motion_through_the_wrapped_dateline_is_smooth() {
    let start = Geodetic::from_degrees(35.55, 179.999, 1000.0);
    let turbulence = Turbulence::severe(1);
    let mut previous = world_gust(turbulence, Seconds::ZERO, start);
    for step in 1..1000 {
        let moved = start.offset_by(Meters::ZERO, Meters(f64::from(step) * 0.42));
        // Follow the same canonical geodetic conversion used by a moving FDM state.
        let position = moved.to_ecef().to_geodetic();
        let gust = world_gust(turbulence, Seconds(f64::from(step) / 120.0), position);
        let change = (gust - previous).length();
        assert!(
            change < 0.2,
            "step {step}: physical gust jumped by {change} m/s"
        );
        previous = gust;
    }
}

#[test]
fn preset_component_bounds_hold_around_the_globe() {
    for latitude in [
        -90.0, -89.999, -60.0, -35.55, 0.0, 35.55, 60.0, 89.999, 90.0,
    ] {
        for longitude in [-180.0, -120.0, -30.0, 0.0, 90.0, 139.78, 180.0] {
            for altitude in [0.0, 1000.0, 12000.0] {
                for seed in [1, 7, 4242] {
                    for turbulence in [
                        Turbulence::light(seed),
                        Turbulence::moderate(seed),
                        Turbulence::severe(seed),
                    ] {
                        for step in 0..50 {
                            let gust = turbulence.gust_at(
                                Seconds(f64::from(step) * 0.37),
                                Geodetic::from_degrees(latitude, longitude, altitude),
                            );
                            let limit = turbulence.intensity.get();
                            assert!(
                                gust.north().is_finite()
                                    && gust.east().is_finite()
                                    && gust.down().is_finite()
                            );
                            assert!(gust.north().abs() <= limit * (1.0 + 1e-12));
                            assert!(gust.east().abs() <= limit * (1.0 + 1e-12));
                            assert!(gust.down().abs() <= 0.7 * limit * (1.0 + 1e-12));
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn ordinary_small_movements_are_smooth_in_every_spatial_axis() {
    let turbulence = Turbulence::severe(4242);
    for latitude in [-80.0, -35.55, 0.0, 35.55, 80.0] {
        for longitude in [-179.0, 0.0, 139.78] {
            let start = Geodetic::from_degrees(latitude, longitude, 1000.0);
            for axis in 0..3 {
                let mut previous = world_gust(turbulence, Seconds::ZERO, start);
                for step in 1..500 {
                    let distance = Meters(f64::from(step) * 0.42);
                    let position = match axis {
                        0 => start.offset_by(distance, Meters::ZERO),
                        1 => start.offset_by(Meters::ZERO, distance),
                        _ => Geodetic {
                            altitude: start.altitude + distance,
                            ..start
                        },
                    };
                    let gust = world_gust(turbulence, Seconds(f64::from(step) / 120.0), position);
                    assert!(
                        (gust - previous).length() < 0.2,
                        "latitude {latitude}, longitude {longitude}, axis {axis}, step {step}"
                    );
                    previous = gust;
                }
            }
        }
    }
}

#[test]
fn lattice_corner_vectors_are_capped_before_the_local_rotation() {
    // Noise lattice corners have no interpolative attenuation. An ECEF corner near Haneda
    // exercises the vector cap without relying on a pole's axis-aligned local frame.
    let position = Ecef(DVec3::new(-26_442.0, 22_361.0, 24_605.0) * 150.0).to_geodetic();
    let mut cap_exercised = false;
    for seed in 0..64 {
        let turbulence = Turbulence::severe(seed);
        let gust = turbulence.gust_at(Seconds::ZERO, position);
        let before_vertical_reduction = DVec3::new(gust.north(), gust.east(), gust.down() / 0.7);
        let length = before_vertical_reduction.length();
        let limit = turbulence.intensity.get();
        assert!(
            length <= limit * (1.0 + 1e-12),
            "seed {seed}: vector length {length} exceeded {limit}"
        );
        cap_exercised |= (length - limit).abs() < 1e-10;
    }
    assert!(
        cap_exercised,
        "the fixture must exercise the cap, not just small interpolated values"
    );
}
