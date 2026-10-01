//! Deterministic synthetic traffic and bounded delayed-snapshot interpolation.
use crate::AircraftState;
use flightsim_core::{Attitude, Ecef, Geodetic, LocalFrame, Meters, Ned, Seconds};
use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, Clone, PartialEq)]
pub struct TrafficObservation {
    pub id: u32,
    pub callsign: String,
    pub state: AircraftState,
    pub stale: bool,
}

/// Common boundary for synthetic, session and future external traffic providers.
pub trait TrafficSource: Send + Sync {
    fn sample(&self, now: Seconds) -> Vec<TrafficObservation>;
}

#[derive(Debug, Clone)]
pub struct SyntheticTraffic {
    frame: LocalFrame,
}
impl SyntheticTraffic {
    #[must_use]
    pub fn new(airport: Geodetic) -> Self {
        Self {
            frame: LocalFrame::new(airport),
        }
    }
}
impl TrafficSource for SyntheticTraffic {
    fn sample(&self, now: Seconds) -> Vec<TrafficObservation> {
        if !now.get().is_finite() || now.get() < 0.0 {
            return Vec::new();
        }
        [
            (1, "SYN-ALPHA", 900.0, 220.0, 42.0, 0.0),
            (2, "SYN-BRAVO", 1400.0, 350.0, 48.0, 2.0),
            (3, "SYN-CHARLIE", 2100.0, 550.0, 55.0, 4.1),
        ]
        .into_iter()
        .map(|(id, name, radius, height, speed, phase)| {
            // A circle in the existing local NED frame, not an independent
            // geodetic conversion. All geographic transforms remain in core.
            let angle = (now.get() * speed / radius + phase).rem_euclid(std::f64::consts::TAU);
            let (s, c) = angle.sin_cos();
            let position =
                self.frame
                    .ned_to_ecef_position(Ned::new(radius * c, radius * s, -height));
            let velocity = Ned::new(-speed * s, speed * c, 0.0);
            let local = LocalFrame::new(position.to_geodetic());
            let heading = local
                .ecef_to_ned_vector(self.frame.ned_to_ecef_vector(velocity))
                .bearing();
            let bank = (speed * speed / (radius * 9.80665)).atan();
            let orientation = local.ned_to_ecef_rotation()
                * Attitude::new(
                    flightsim_core::Radians(bank),
                    flightsim_core::Radians::ZERO,
                    heading,
                )
                .to_quaternion();
            TrafficObservation {
                id,
                callsign: name.into(),
                state: AircraftState {
                    position,
                    orientation,
                    velocity_mps: self.frame.ned_to_ecef_vector(velocity),
                },
                stale: false,
            }
        })
        .collect()
    }
}

#[derive(Debug, Clone)]
struct Track {
    callsign: String,
    samples: VecDeque<(Seconds, AircraftState)>,
}

/// Bounded timestamped observations covering the interpolation delay; missing updates freeze before
/// expiring rather than extrapolating an aircraft forever into arbitrary space.
#[derive(Debug, Clone, Default)]
pub struct InterpolatedTraffic {
    tracks: BTreeMap<u32, Track>,
}
impl InterpolatedTraffic {
    pub const MAX_TRACKS: usize = 32;
    /// Eight snapshots cover 350 ms at the normal 20 Hz send rate.
    pub const MAX_SAMPLES_PER_TRACK: usize = 8;
    pub const INTERPOLATION_DELAY: Seconds = Seconds(0.1);
    pub const STALE_AFTER: Seconds = Seconds(2.0);
    pub const EXPIRE_AFTER: Seconds = Seconds(5.0);

    /// Accept a newer valid snapshot. Invalid/stale data never replaces a good one.
    pub fn observe(
        &mut self,
        id: u32,
        callsign: &str,
        time: Seconds,
        state: AircraftState,
    ) -> bool {
        if !state.is_valid()
            || !time.get().is_finite()
            || time.get() < 0.0
            || callsign.is_empty()
            || callsign.len() > 16
            || !callsign.is_ascii()
            || callsign.chars().any(char::is_control)
        {
            return false;
        }
        if let Some(track) = self.tracks.get_mut(&id) {
            let latest = track.samples.back().expect("a track always has a snapshot");
            match time.get().total_cmp(&latest.0.get()) {
                std::cmp::Ordering::Less => return false,
                std::cmp::Ordering::Equal => {
                    // Several ordered packets can be drained in one poll. Replace
                    // only that timestamp, preserving enough delayed history.
                    *track.samples.back_mut().expect("existing snapshot") = (time, state);
                }
                std::cmp::Ordering::Greater => {
                    track.samples.push_back((time, state));
                    if track.samples.len() > Self::MAX_SAMPLES_PER_TRACK {
                        track.samples.pop_front();
                    }
                }
            }
            track.callsign = callsign.into();
        } else {
            if self.tracks.len() >= Self::MAX_TRACKS {
                return false;
            }
            self.tracks.insert(
                id,
                Track {
                    callsign: callsign.into(),
                    samples: VecDeque::from([(time, state)]),
                },
            );
        }
        true
    }
    #[must_use]
    pub fn contains(&self, id: u32) -> bool {
        self.tracks.contains_key(&id)
    }

    pub fn remove(&mut self, id: u32) {
        self.tracks.remove(&id);
    }
    pub fn clear(&mut self) {
        self.tracks.clear();
    }
    pub fn expire(&mut self, now: Seconds) {
        self.tracks.retain(|_, t| {
            now.get() - t.samples.back().expect("nonempty track").0.get()
                <= Self::EXPIRE_AFTER.get()
        });
    }
    #[must_use]
    pub fn len(&self) -> usize {
        self.tracks.len()
    }
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }
}
impl TrafficSource for InterpolatedTraffic {
    fn sample(&self, now: Seconds) -> Vec<TrafficObservation> {
        if !now.get().is_finite() {
            return Vec::new();
        }
        self.tracks
            .iter()
            .filter_map(|(&id, t)| {
                let age = now.get() - t.samples.back().expect("nonempty track").0.get();
                if age > Self::EXPIRE_AFTER.get() || age < -1.0 {
                    return None;
                }
                let target = now.get() - Self::INTERPOLATION_DELAY.get();
                let mut state = t.samples.back().expect("nonempty track").1;
                if target <= t.samples.front().expect("nonempty track").0.get() {
                    state = t.samples.front().expect("nonempty track").1;
                } else {
                    for (previous, latest) in t.samples.iter().zip(t.samples.iter().skip(1)) {
                        if target <= latest.0.get() {
                            let alpha =
                                (target - previous.0.get()) / (latest.0.get() - previous.0.get());
                            state = previous.1.interpolate(latest.1, alpha);
                            break;
                        }
                    }
                }
                Some(TrafficObservation {
                    id,
                    callsign: t.callsign.clone(),
                    state,
                    stale: age > Self::STALE_AFTER.get(),
                })
            })
            .collect()
    }
}

/// Filter in canonical ECEF so a floating-origin rebase cannot change visibility.
#[must_use]
pub fn within_range(observer: Ecef, target: Ecef, max_distance: Meters) -> bool {
    max_distance.is_finite()
        && max_distance.get() >= 0.0
        && observer.0.distance(target.0) <= max_distance.get()
}

#[cfg(test)]
mod history_tests {
    use super::*;
    #[test]
    fn history_memory_remains_bounded_under_many_updates() {
        let position = Geodetic::from_degrees(35.0, 139.0, 100.0).to_ecef();
        let state = AircraftState {
            position,
            orientation: glam::DQuat::IDENTITY,
            velocity_mps: glam::DVec3::ZERO,
        };
        let mut traffic = InterpolatedTraffic::default();
        for i in 0..10_000 {
            assert!(traffic.observe(1, "BOUNDED", Seconds(f64::from(i) / 20.0), state));
        }
        assert_eq!(
            traffic.tracks[&1].samples.len(),
            InterpolatedTraffic::MAX_SAMPLES_PER_TRACK
        );
    }
}
