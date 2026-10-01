use flightsim_core::{Ecef, Geodetic, Seconds};
use flightsim_net::{AircraftState, InterpolatedTraffic, Message, SyntheticTraffic, TrafficSource};
use glam::{DQuat, DVec3};
fn state(x: f64) -> AircraftState {
    AircraftState {
        position: Ecef(DVec3::new(6_378_137.0, x, 0.0)),
        orientation: DQuat::IDENTITY,
        velocity_mps: DVec3::new(0.0, 50.0, 0.0),
    }
}

#[test]
fn protocol_round_trips_all_variants_and_rejects_every_truncation() {
    let messages = [
        Message::Join {
            nonce: 9,
            callsign: "TEST-1".into(),
        },
        Message::Welcome {
            nonce: 9,
            session: 99,
            participant: 1,
        },
        Message::State {
            session: 99,
            participant: 1,
            sequence: 7,
            timestamp: Seconds(123.0),
            state: state(0.0),
        },
        Message::Leave {
            session: 99,
            participant: 1,
        },
        Message::Ping {
            session: 99,
            participant: 1,
        },
        Message::Reject {
            nonce: 9,
            reason: 1,
        },
    ];
    for message in messages {
        let bytes = message.encode().unwrap();
        assert_eq!(Message::decode(&bytes).unwrap(), message);
        for end in 0..bytes.len() {
            assert!(Message::decode(&bytes[..end]).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(Message::decode(&extra).is_err());
    }
}
#[test]
fn independent_wire_layout_has_expected_little_endian_header() {
    // Encoded independently from the format table using Python struct.pack('<4sHBBQQI',...).
    let expected = [
        70, 83, 78, 49, 1, 0, 2, 0, 9, 0, 0, 0, 0, 0, 0, 0, 99, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0,
    ];
    assert_eq!(
        Message::Welcome {
            nonce: 9,
            session: 99,
            participant: 1
        }
        .encode()
        .unwrap(),
        expected
    );
    assert_eq!(
        Message::decode(&expected).unwrap(),
        Message::Welcome {
            nonce: 9,
            session: 99,
            participant: 1
        }
    );
}
#[test]
fn incompatible_version_nonfinite_state_and_hostile_lengths_are_rejected() {
    let message = Message::State {
        session: 1,
        participant: 2,
        sequence: 3,
        timestamp: Seconds(0.0),
        state: state(0.0),
    };
    let mut b = message.encode().unwrap();
    b[4] = 2;
    assert!(Message::decode(&b).is_err());
    let mut b = message.encode().unwrap();
    b[32..40].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(Message::decode(&b).is_err());
    assert!(Message::decode(&vec![0; 10000]).is_err());
    for len in 0..200 {
        let bytes = vec![255; len];
        assert!(std::panic::catch_unwind(|| Message::decode(&bytes)).is_ok());
    }
}
#[test]
fn interpolation_is_smooth_bounded_and_expires_lost_contacts() {
    let mut traffic = InterpolatedTraffic::default();
    assert!(traffic.observe(1, "TEST-1", Seconds(1.0), state(0.0)));
    assert!(traffic.observe(1, "TEST-1", Seconds(1.2), state(10.0)));
    let at = traffic.sample(Seconds(1.2));
    assert!((at[0].state.position.0.y - 5.0).abs() < 1e-9);
    assert!(!traffic.observe(1, "OLD", Seconds(1.1), state(999.0)));
    assert!(traffic.sample(Seconds(3.5))[0].stale);
    assert!((traffic.sample(Seconds(3.5))[0].state.position.0.y - 10.0).abs() < 1e-9);
    assert!(traffic.sample(Seconds(6.3)).is_empty());
    traffic.expire(Seconds(6.3));
    assert!(traffic.is_empty());
}
#[test]
fn synthetic_traffic_is_deterministic_finite_and_moves() {
    for origin in [
        Geodetic::from_degrees(35.55, 139.78, 100.0),
        Geodetic::from_degrees(89.9, 179.99, 0.0),
    ] {
        let source = SyntheticTraffic::new(origin);
        for t in [0.0, 1.0, 100.0, 3600.0] {
            let a = source.sample(Seconds(t));
            let b = source.sample(Seconds(t));
            assert_eq!(a, b);
            assert_eq!(a.len(), 3);
            assert!(a.iter().all(|o| o.state.is_valid()));
        }
        assert_ne!(source.sample(Seconds(1.0)), source.sample(Seconds(2.0)));
    }
}

#[test]
fn twenty_hertz_packets_interpolate_at_sixty_hertz_with_a_hundred_ms_delay() {
    let mut traffic = InterpolatedTraffic::default();
    for packet in 0..20 {
        let time = f64::from(packet) / 20.0;
        assert!(traffic.observe(1, "SMOOTH", Seconds(time), state(time * 50.0)));
        if packet >= 3 {
            for render in 0..3 {
                let now = time + f64::from(render) / 60.0;
                let sample = traffic.sample(Seconds(now));
                let expected = (now - 0.1) * 50.0;
                assert!(
                    (sample[0].state.position.0.y - expected).abs() < 1e-8,
                    "packet {packet}, render {render}: {} vs expected {expected}",
                    sample[0].state.position.0.y
                );
            }
        }
    }
}

#[test]
fn duplicate_timestamps_replace_only_the_latest_observation() {
    let mut traffic = InterpolatedTraffic::default();
    traffic.observe(1, "FIRST", Seconds(0.0), state(0.0));
    traffic.observe(1, "FIRST", Seconds(0.1), state(5.0));
    traffic.observe(1, "NEWEST", Seconds(0.2), state(10.0));
    traffic.observe(1, "NEWEST", Seconds(0.2), state(20.0));
    let sample = traffic.sample(Seconds(0.25));
    assert_eq!(sample[0].callsign, "NEWEST");
    assert!((sample[0].state.position.0.y - 12.5).abs() < 1e-8);
}
