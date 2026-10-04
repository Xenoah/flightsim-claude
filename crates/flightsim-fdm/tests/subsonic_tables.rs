use flightsim_core::{Kelvin, Meters, MetersPerSecond, Pascals};
use flightsim_fdm::{
    AircraftConfig, Atmosphere,
    definition::AircraftDefinition,
    subsonic::{
        AxisStatus, DryJetDefinition, DryJetTable, DryThrottle, EvaluationError, JetConditions,
        MachAeroDefinition, MachAeroKnotDefinition, MachAeroSchedule, MachNumber,
        NetThrustCellDefinition, PressureRatio, TemperatureRatio,
    },
};

fn conditions(p: f64, t: f64, m: f64) -> JetConditions {
    JetConditions {
        pressure_ratio: PressureRatio(p),
        temperature_ratio: TemperatureRatio(t),
        mach: MachNumber(m),
    }
}

// An independently evaluable multilinear polynomial, not an aircraft dataset.
fn polynomial(p: f64, t: f64, m: f64, throttle: f64) -> f64 {
    let idle = p * (-100.0 + 30.0 * t - 20.0 * m + 10.0 * t * m);
    let dry = p * (10000.0 - 500.0 * t + 1200.0 * m - 100.0 * t * m);
    (1.0 - throttle) * idle + throttle * dry
}

fn definition() -> DryJetDefinition {
    let pressure_ratios = vec![0.0, 0.4, 1.0, 1.5];
    let temperature_ratios = vec![0.5, 0.8, 1.0, 1.4];
    let mach = vec![0.0, 0.2, 0.7, 0.95];
    let mut cells = Vec::new();
    for &p in &pressure_ratios {
        for &t in &temperature_ratios {
            for &m in &mach {
                cells.push(NetThrustCellDefinition {
                    idle_n: polynomial(p, t, m, 0.0),
                    maximum_dry_n: polynomial(p, t, m, 1.0),
                });
            }
        }
    }
    DryJetDefinition {
        schema: 1,
        pressure_ratios,
        temperature_ratios,
        mach,
        cells,
    }
}

fn close(a: f64, b: f64) {
    assert!((a - b).abs() <= 1e-8, "{a} differs from {b}");
}

#[test]
fn trilinear_table_reproduces_independent_multilinear_polynomial() {
    let table = DryJetTable::from_definition(definition()).unwrap();
    for p in [0.0, 0.07, 0.4, 0.67, 1.0, 1.2, 1.5] {
        for t in [0.5, 0.67, 0.8, 0.93, 1.0, 1.17, 1.4] {
            for m in [0.0, 0.06, 0.2, 0.52, 0.7, 0.89, 0.95] {
                for throttle in [0.0, 0.13, 0.5, 0.9, 1.0] {
                    let sample = table
                        .sample(conditions(p, t, m), DryThrottle(throttle))
                        .unwrap();
                    assert!(sample.domain.is_supported());
                    assert!(sample.net_thrust.is_finite());
                    close(sample.net_thrust.get(), polynomial(p, t, m, throttle));
                }
            }
        }
    }
}

#[test]
fn arbitrary_cells_match_independent_tensor_product_reference() {
    let mut def = definition();
    for (i, cell) in def.cells.iter_mut().enumerate().skip(16) {
        let value = f64::from(u32::try_from((i * 197) % 311).unwrap());
        cell.idle_n = value - 400.0;
        cell.maximum_dry_n = value * 13.0;
    }
    let table = DryJetTable::from_definition(def.clone()).unwrap();
    // Direct eight-corner tensor-product sum; production uses staged lerps.
    for pi in 0..3 {
        for ti in 0..3 {
            for mi in 0..3 {
                let fractions = [0.37, 0.61, 0.23];
                let [fp, ft, fm] = fractions;
                let query = conditions(
                    def.pressure_ratios[pi] * (1.0 - fp) + def.pressure_ratios[pi + 1] * fp,
                    def.temperature_ratios[ti] * (1.0 - ft) + def.temperature_ratios[ti + 1] * ft,
                    def.mach[mi] * (1.0 - fm) + def.mach[mi + 1] * fm,
                );
                let mut expected = 0.0;
                for dp in 0..=1 {
                    for dt in 0..=1 {
                        for dm in 0..=1 {
                            let weight = [dp, dt, dm]
                                .into_iter()
                                .zip(fractions)
                                .map(|(side, f)| if side == 0 { 1.0 - f } else { f })
                                .product::<f64>();
                            let cell = def.cells[((pi + dp) * 4 + (ti + dt)) * 4 + mi + dm];
                            expected += weight * (0.6 * cell.idle_n + 0.4 * cell.maximum_dry_n);
                        }
                    }
                }
                close(
                    table
                        .sample(query, DryThrottle(0.4))
                        .unwrap()
                        .net_thrust
                        .get(),
                    expected,
                );
            }
        }
    }
}

#[test]
fn table_knots_preserve_authored_bits_and_cell_boundary_is_continuous() {
    let def = definition();
    let table = DryJetTable::from_definition(def.clone()).unwrap();
    for (pi, &p) in def.pressure_ratios.iter().enumerate() {
        for (ti, &t) in def.temperature_ratios.iter().enumerate() {
            for (mi, &m) in def.mach.iter().enumerate() {
                let cell = def.cells[(pi * 4 + ti) * 4 + mi];
                for (throttle, expected) in [(0.0, cell.idle_n), (1.0, cell.maximum_dry_n)] {
                    assert_eq!(
                        table
                            .sample(conditions(p, t, m), DryThrottle(throttle))
                            .unwrap()
                            .net_thrust
                            .get()
                            .to_bits(),
                        expected.to_bits()
                    );
                }
            }
        }
    }
    for query in [conditions(0.4, 0.8, 0.7), conditions(1.0, 1.0, 0.2)] {
        let center = table
            .sample(query, DryThrottle(0.6))
            .unwrap()
            .net_thrust
            .get();
        for epsilon in [-1e-10, 1e-10] {
            for neighbor in [
                conditions(
                    query.pressure_ratio.0 + epsilon,
                    query.temperature_ratio.0,
                    query.mach.0,
                ),
                conditions(
                    query.pressure_ratio.0,
                    query.temperature_ratio.0 + epsilon,
                    query.mach.0,
                ),
                conditions(
                    query.pressure_ratio.0,
                    query.temperature_ratio.0,
                    query.mach.0 + epsilon,
                ),
            ] {
                assert!(
                    (table
                        .sample(neighbor, DryThrottle(0.6))
                        .unwrap()
                        .net_thrust
                        .get()
                        - center)
                        .abs()
                        < 2e-6
                );
            }
        }
    }
}

#[test]
fn vacuum_static_idle_and_signed_dry_thrust_have_explicit_semantics() {
    let table = DryJetTable::from_definition(definition()).unwrap();
    for t in [0.5, 1.0, 1.4] {
        for m in [0.0, 0.5, 0.95] {
            for throttle in [0.0, 0.7, 1.0] {
                close(
                    table
                        .sample(conditions(0.0, t, m), DryThrottle(throttle))
                        .unwrap()
                        .net_thrust
                        .get(),
                    0.0,
                );
            }
        }
    }
    close(
        table
            .sample(conditions(1.0, 1.0, 0.0), DryThrottle(1.0))
            .unwrap()
            .net_thrust
            .get(),
        9500.0,
    );
    close(
        table
            .sample(conditions(1.0, 1.0, 0.0), DryThrottle(0.0))
            .unwrap()
            .net_thrust
            .get(),
        -70.0,
    );
    let mut def = definition();
    for cell in def.cells.iter_mut().skip(16) {
        cell.idle_n = -500.0;
        cell.maximum_dry_n = -100.0;
    }
    let signed = DryJetTable::from_definition(def).unwrap();
    close(
        signed
            .sample(conditions(1.0, 1.0, 0.0), DryThrottle(1.0))
            .unwrap()
            .net_thrust
            .get(),
        -100.0,
    );
}

#[test]
fn ambient_conversion_uses_si_static_conditions_and_local_sound_speed() {
    let sea = Atmosphere::standard().sample(Meters(0.0));
    let jet = JetConditions::from_atmosphere(sea, MetersPerSecond(sea.speed_of_sound.get() * 0.5))
        .unwrap();
    close(jet.pressure_ratio.0, 1.0);
    close(jet.temperature_ratio.0, 1.0);
    close(jet.mach.0, 0.5);
    let high = Atmosphere::standard().sample(Meters(10000.0));
    let query = JetConditions::from_atmosphere(high, MetersPerSecond(200.0)).unwrap();
    close(query.pressure_ratio.0, high.pressure.get() / 101325.0);
    close(query.temperature_ratio.0, high.temperature.get() / 288.15);
    close(query.mach.0, 200.0 / high.speed_of_sound.get());
    let mut changed_density = sea;
    changed_density.density.0 = f64::NAN;
    assert_eq!(
        JetConditions::from_atmosphere(changed_density, MetersPerSecond(0.0)).unwrap(),
        JetConditions::from_atmosphere(sea, MetersPerSecond(0.0)).unwrap()
    );
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        assert!(JetConditions::from_atmosphere(sea, MetersPerSecond(bad)).is_err());
        let mut sample = sea;
        sample.pressure = Pascals(bad);
        assert!(JetConditions::from_atmosphere(sample, MetersPerSecond(0.0)).is_err());
        let mut sample = sea;
        sample.temperature = Kelvin(bad);
        assert!(JetConditions::from_atmosphere(sample, MetersPerSecond(0.0)).is_err());
        let mut sample = sea;
        sample.speed_of_sound = MetersPerSecond(bad);
        assert!(JetConditions::from_atmosphere(sample, MetersPerSecond(0.0)).is_err());
    }
    let mut sample = sea;
    sample.temperature = Kelvin(0.0);
    assert!(JetConditions::from_atmosphere(sample, MetersPerSecond(0.0)).is_err());
    let mut sample = sea;
    sample.speed_of_sound = MetersPerSecond(0.0);
    assert!(JetConditions::from_atmosphere(sample, MetersPerSecond(0.0)).is_err());
    let mut sample = sea;
    sample.speed_of_sound = MetersPerSecond(f64::MIN_POSITIVE);
    assert!(JetConditions::from_atmosphere(sample, MetersPerSecond(f64::MAX)).is_err());
}

#[test]
fn invalid_queries_are_distinct_from_outside_domain_without_clamps() {
    let table = DryJetTable::from_definition(definition()).unwrap();
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0] {
        for query in [
            conditions(bad, 1.0, 0.5),
            conditions(1.0, bad, 0.5),
            conditions(1.0, 1.0, bad),
        ] {
            assert!(matches!(
                table.sample(query, DryThrottle(0.5)),
                Err(EvaluationError::InvalidInput(_))
            ));
        }
        assert!(matches!(
            table.sample(conditions(1.0, 1.0, 0.5), DryThrottle(bad)),
            Err(EvaluationError::InvalidInput("throttle"))
        ));
    }
    assert!(
        table
            .sample(conditions(1.0, 1.0, 0.5), DryThrottle(1.01))
            .is_err()
    );
    assert!(
        table
            .sample(conditions(1.0, 0.0, 0.5), DryThrottle(0.5))
            .is_err()
    );
    let Err(EvaluationError::OutsideJetDomain(domain)) =
        table.sample(conditions(2.0, 0.1, 1.0), DryThrottle(0.5))
    else {
        panic!("expected explicit domain error")
    };
    assert_eq!(domain.pressure, AxisStatus::Above);
    assert_eq!(domain.temperature, AxisStatus::Below);
    assert_eq!(domain.mach, AxisStatus::Above);
    assert!(!domain.is_supported());
    for p in [1.5_f64.next_up(), f64::MAX] {
        assert!(
            table
                .sample(conditions(p, 1.0, 0.5), DryThrottle(0.5))
                .is_err()
        );
    }
    assert!(
        table
            .sample(conditions(1.0, 1.4_f64.next_up(), 0.5), DryThrottle(0.5))
            .is_err()
    );
    assert!(
        table
            .sample(conditions(1.0, 0.5_f64.next_down(), 0.5), DryThrottle(0.5))
            .is_err()
    );
    assert!(
        table
            .sample(conditions(1.0, 1.0, 0.95_f64.next_up()), DryThrottle(0.5))
            .is_err()
    );
}

#[test]
fn hostile_tables_fail_before_evaluation() {
    let base = definition();
    let mut mutations = Vec::new();
    let mut d = base.clone();
    d.schema = 2;
    mutations.push(d);
    let mut d = base.clone();
    d.cells.pop();
    mutations.push(d);
    let mut d = base.clone();
    d.cells.push(d.cells[0]);
    mutations.push(d);
    for axis in 0..3 {
        for values in [
            vec![],
            vec![0.0],
            vec![0.0; 33],
            vec![0.0, 0.0],
            vec![0.0, 1e-10],
            vec![1.0, 0.0],
            vec![0.0, f64::NAN],
            vec![0.0, f64::INFINITY],
            vec![-1.0, 0.0],
            vec![0.0, 3.0],
        ] {
            let mut d = base.clone();
            match axis {
                0 => d.pressure_ratios = values,
                1 => d.temperature_ratios = values,
                _ => d.mach = values,
            };
            mutations.push(d);
        }
    }
    let mut d = base.clone();
    d.pressure_ratios[0] = 0.1;
    mutations.push(d);
    let mut d = base.clone();
    d.mach[0] = 0.1;
    mutations.push(d);
    let mut d = base.clone();
    d.cells[0].idle_n = 1.0;
    mutations.push(d);
    let mut d = base.clone();
    d.cells[0].maximum_dry_n = 1.0;
    mutations.push(d);
    let mut d = base.clone();
    d.cells[32].idle_n = d.cells[32].maximum_dry_n + 1.0;
    mutations.push(d);
    for bad in [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        -1000001.0,
        1000001.0,
    ] {
        let mut d = base.clone();
        d.cells[32].idle_n = bad;
        mutations.push(d);
        let mut d = base.clone();
        d.cells[32].maximum_dry_n = bad;
        mutations.push(d);
    }
    for d in mutations {
        assert!(DryJetTable::from_definition(d).is_err());
    }
    let mut d = base;
    d.pressure_ratios = (0..32).map(|i| f64::from(i) / 31.0).collect();
    d.temperature_ratios = (0..32).map(|i| 0.5 + f64::from(i) / 32.0).collect();
    d.mach = (0..32).map(|i| f64::from(i) / 40.0).collect();
    d.cells = vec![
        NetThrustCellDefinition {
            idle_n: 0.0,
            maximum_dry_n: 0.0
        };
        32768
    ];
    assert!(DryJetTable::from_definition(d).is_err());
}

fn schedule_definition() -> MachAeroDefinition {
    let base = AircraftDefinition::from_config(&AircraftConfig::light_single()).aero;
    MachAeroDefinition {
        schema: 1,
        knots: [0.0, 0.4, 0.95]
            .into_iter()
            .map(|mach| {
                let mut aero = base;
                aero.drag_min = 0.03 + 0.08 * mach;
                aero.lift_alpha = 5.0 + 2.0 * mach;
                aero.pitch_alpha = -1.0 - 0.2 * mach;
                aero.yaw_rate_p = -0.1 - 0.05 * mach;
                MachAeroKnotDefinition { mach, aero }
            })
            .collect(),
    }
}

#[test]
fn complete_aero_schedule_is_continuous_and_finite() {
    let schedule = MachAeroSchedule::from_definition(schedule_definition()).unwrap();
    for mach in [0.0, 0.1, 0.4 - 1e-10, 0.4, 0.4 + 1e-10, 0.72, 0.95] {
        let sample = schedule.sample(MachNumber(mach)).unwrap();
        assert_eq!(sample.domain, AxisStatus::Within);
        close(sample.coefficients.drag_min, 0.03 + 0.08 * mach);
        close(sample.coefficients.lift_alpha, 5.0 + 2.0 * mach);
        close(sample.coefficients.pitch_alpha, -1.0 - 0.2 * mach);
        close(sample.coefficients.yaw_rate_p, -0.1 - 0.05 * mach);
    }
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -0.1] {
        assert!(matches!(
            schedule.sample(MachNumber(bad)),
            Err(EvaluationError::InvalidInput("mach"))
        ));
    }
    for bad in [0.95_f64.next_up(), 1.0, 2.0, f64::MAX] {
        assert!(matches!(
            schedule.sample(MachNumber(bad)),
            Err(EvaluationError::OutsideMachDomain(AxisStatus::Above))
        ));
    }
}

#[test]
fn all_aero_fields_are_validated_interpolated_and_identified() {
    let base = schedule_definition();
    let original = MachAeroSchedule::from_definition(base.clone())
        .unwrap()
        .canonical_bytes();
    let json = serde_json::to_value(&base).unwrap();
    let fields: Vec<_> = json["knots"][1]["aero"]
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    assert_eq!(fields.len(), 25);
    for field in fields {
        let mut mutation = json.clone();
        let old = mutation["knots"][1]["aero"][&field].as_f64().unwrap();
        mutation["knots"][1]["aero"][&field] = serde_json::json!(old * 0.9);
        let def: MachAeroDefinition = serde_json::from_value(mutation).unwrap();
        let schedule = MachAeroSchedule::from_definition(def).unwrap();
        assert_ne!(
            schedule.canonical_bytes(),
            original,
            "{field} absent from identity"
        );
        let sampled = schedule.sample(MachNumber(0.2)).unwrap().coefficients;
        let mut config = AircraftConfig::light_single();
        config.aero = sampled;
        let sampled_json =
            serde_json::to_value(AircraftDefinition::from_config(&config).aero).unwrap();
        let low = json["knots"][0]["aero"][&field].as_f64().unwrap();
        close(
            sampled_json[&field].as_f64().unwrap(),
            (low + old * 0.9) / 2.0,
        );
        let mut invalid = json.clone();
        invalid["knots"][1]["aero"][&field] = serde_json::json!(101.0);
        assert!(
            MachAeroSchedule::from_definition(serde_json::from_value(invalid).unwrap()).is_err(),
            "{field} not validated"
        );
    }
    let mut d = base.clone();
    d.knots[1].aero.yaw_rate_p = f64::NAN;
    assert!(MachAeroSchedule::from_definition(d).is_err());
    let mut d = base.clone();
    d.knots[1].aero.oswald_efficiency = 0.0;
    assert!(MachAeroSchedule::from_definition(d).is_err());
    let mut d = base.clone();
    d.knots[1].aero.stall_angle_rad = 0.0;
    assert!(MachAeroSchedule::from_definition(d).is_err());
    let mut d = base.clone();
    d.knots[1].mach = d.knots[0].mach;
    assert!(MachAeroSchedule::from_definition(d).is_err());
    let mut d = base.clone();
    d.knots.clear();
    assert!(MachAeroSchedule::from_definition(d).is_err());
    let mut d = base.clone();
    d.knots[0].mach = 0.1;
    assert!(MachAeroSchedule::from_definition(d).is_err());
    let mut d = base;
    d.schema = 2;
    assert!(MachAeroSchedule::from_definition(d).is_err());
}

#[test]
fn exact_number_json_roundtrip_retains_bits_and_rejects_unknown_fields() {
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/subsonic-identity-v1.json")).unwrap();
    let table =
        DryJetTable::from_definition(serde_json::from_value(fixture["jet"].clone()).unwrap())
            .unwrap();
    let json = serde_json::to_string(table.definition()).unwrap();
    let decoded = DryJetTable::from_definition(serde_json::from_str(&json).unwrap()).unwrap();
    assert_eq!(table.canonical_bytes(), decoded.canonical_bytes());
    let schedule =
        MachAeroSchedule::from_definition(serde_json::from_value(fixture["aero"].clone()).unwrap())
            .unwrap();
    let json = serde_json::to_string(schedule.definition()).unwrap();
    let decoded = MachAeroSchedule::from_definition(serde_json::from_str(&json).unwrap()).unwrap();
    assert_eq!(schedule.canonical_bytes(), decoded.canonical_bytes());
    let mut json = serde_json::to_value(definition()).unwrap();
    json["density_exponent"] = serde_json::json!(1.0);
    assert!(serde_json::from_value::<DryJetDefinition>(json).is_err());
    let mut json = serde_json::to_value(schedule_definition()).unwrap();
    json["knots"][0]["aero"]["unknown"] = serde_json::json!(1.0);
    assert!(serde_json::from_value::<MachAeroDefinition>(json).is_err());
}

#[test]
fn every_thrust_axis_and_cell_changes_canonical_identity_and_signed_zero_survives() {
    let def = definition();
    let original = DryJetTable::from_definition(def.clone())
        .unwrap()
        .canonical_bytes();
    for axis in 0..3 {
        let mut d = def.clone();
        match axis {
            0 => d.pressure_ratios[1] += 0.001,
            1 => d.temperature_ratios[1] += 0.001,
            _ => d.mach[1] += 0.001,
        };
        assert_ne!(
            DryJetTable::from_definition(d).unwrap().canonical_bytes(),
            original
        );
    }
    for index in 0..def.cells.len() {
        for dry in [false, true] {
            let mut d = def.clone();
            let cell = &mut d.cells[index];
            let field = if dry {
                &mut cell.maximum_dry_n
            } else {
                &mut cell.idle_n
            };
            *field = if index < 16 {
                if field.is_sign_negative() { 0.0 } else { -0.0 }
            } else {
                *field + 0.001
            };
            assert_ne!(
                DryJetTable::from_definition(d).unwrap().canonical_bytes(),
                original,
                "cell {index}, dry {dry}"
            );
        }
    }
}

#[test]
fn independent_python_golden_encodes_every_component_byte() {
    fn unhex(hex: &str) -> Vec<u8> {
        hex.as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect()
    }
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/subsonic-identity-v1.json")).unwrap();
    let table =
        DryJetTable::from_definition(serde_json::from_value(fixture["jet"].clone()).unwrap())
            .unwrap();
    let schedule =
        MachAeroSchedule::from_definition(serde_json::from_value(fixture["aero"].clone()).unwrap())
            .unwrap();
    assert_eq!(
        table.canonical_bytes(),
        unhex(fixture["jet_hex"].as_str().unwrap())
    );
    assert_eq!(
        schedule.canonical_bytes(),
        unhex(fixture["aero_hex"].as_str().unwrap())
    );
    // Verify signed zero at exact knots, not just in stored identity bytes.
    assert_eq!(
        table
            .sample(conditions(0.0, 0.5, 0.0), DryThrottle(0.0))
            .unwrap()
            .net_thrust
            .get()
            .to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(
        schedule
            .sample(MachNumber(0.0))
            .unwrap()
            .coefficients
            .pitch_zero
            .to_bits(),
        (-0.0_f64).to_bits()
    );
}

#[test]
fn resource_limits_are_accepted_at_the_limit_and_reject_above_it() {
    let mut def = definition();
    def.pressure_ratios = (0..16).map(|i| f64::from(i) / 8.0).collect();
    def.temperature_ratios = (0..16).map(|i| 0.25 + f64::from(i) / 10.0).collect();
    def.mach = (0..16).map(|i| f64::from(i) / 20.0).collect();
    def.cells = vec![
        NetThrustCellDefinition {
            idle_n: 0.0,
            maximum_dry_n: 0.0
        };
        4096
    ];
    let table = DryJetTable::from_definition(def.clone()).unwrap();
    close(
        table
            .sample(conditions(1.0, 1.0, 0.7), DryThrottle(1.0))
            .unwrap()
            .net_thrust
            .get(),
        0.0,
    );
    def.pressure_ratios.push(2.0);
    def.cells.resize(
        4352,
        NetThrustCellDefinition {
            idle_n: 0.0,
            maximum_dry_n: 0.0,
        },
    );
    assert!(DryJetTable::from_definition(def).is_err());
    let mut aero = schedule_definition();
    aero.knots = (0..32)
        .map(|i| MachAeroKnotDefinition {
            mach: f64::from(i) / 40.0,
            aero: aero.knots[0].aero,
        })
        .collect();
    assert!(MachAeroSchedule::from_definition(aero.clone()).is_ok());
    aero.knots.push(MachAeroKnotDefinition {
        mach: 0.8,
        aero: aero.knots[0].aero,
    });
    assert!(MachAeroSchedule::from_definition(aero).is_err());
}
