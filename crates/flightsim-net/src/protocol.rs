//! Fixed, versioned wire format. Unknown versions and malformed data are errors.
use flightsim_core::{Ecef, Seconds};
use glam::{DQuat, DVec3};

pub const PROTOCOL_VERSION: u16 = 1;
pub const MAX_PACKET_BYTES: usize = 160;
const MAGIC: &[u8; 4] = b"FSN1";

/// World state; orientation maps body axes to ECEF, never a floating origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AircraftState {
    pub position: Ecef,
    pub orientation: DQuat,
    /// ECEF metres per second.
    pub velocity_mps: DVec3,
}
impl AircraftState {
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.position.0.is_finite()
            && (4e6..=1e8).contains(&self.position.0.length())
            && self.orientation.is_finite()
            && (self.orientation.length_squared() - 1.0).abs() < 1e-6
            && self.velocity_mps.is_finite()
            && self.velocity_mps.length() <= 2000.0
    }
    #[must_use]
    pub fn interpolate(self, next: Self, amount: f64) -> Self {
        let a = if amount.is_finite() {
            amount.clamp(0.0, 1.0)
        } else {
            0.0
        };
        Self {
            position: Ecef(self.position.0.lerp(next.position.0, a)),
            orientation: self.orientation.slerp(next.orientation, a).normalize(),
            velocity_mps: self.velocity_mps.lerp(next.velocity_mps, a),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Join {
        nonce: u64,
        callsign: String,
    },
    Welcome {
        nonce: u64,
        session: u64,
        participant: u32,
    },
    State {
        session: u64,
        participant: u32,
        sequence: u32,
        timestamp: Seconds,
        state: AircraftState,
    },
    Leave {
        session: u64,
        participant: u32,
    },
    Ping {
        session: u64,
        participant: u32,
    },
    Reject {
        nonce: u64,
        reason: u8,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolError(pub &'static str);
impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for ProtocolError {}

impl Message {
    /// Encode one complete UDP payload.
    /// # Errors
    /// Invalid callsign, state or timestamp.
    pub fn encode(&self) -> Result<Vec<u8>, ProtocolError> {
        let mut out = Vec::with_capacity(MAX_PACKET_BYTES);
        out.extend(MAGIC);
        out.extend(PROTOCOL_VERSION.to_le_bytes());
        let kind = match self {
            Self::Join { .. } => 1,
            Self::Welcome { .. } => 2,
            Self::State { .. } => 3,
            Self::Leave { .. } => 4,
            Self::Ping { .. } => 5,
            Self::Reject { .. } => 6,
        };
        out.push(kind);
        out.push(0);
        match self {
            Self::Join { nonce, callsign } => {
                if callsign.is_empty()
                    || callsign.len() > 16
                    || !callsign
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                {
                    return Err(ProtocolError(
                        "callsign must be 1..=16 ASCII letters, digits or hyphens",
                    ));
                }
                out.extend(nonce.to_le_bytes());
                out.push(u8::try_from(callsign.len()).expect("bounded callsign"));
                out.extend(callsign.as_bytes());
            }
            Self::Welcome {
                nonce,
                session,
                participant,
            } => {
                out.extend(nonce.to_le_bytes());
                out.extend(session.to_le_bytes());
                out.extend(participant.to_le_bytes());
            }
            Self::State {
                session,
                participant,
                sequence,
                timestamp,
                state,
            } => {
                if !timestamp.get().is_finite()
                    || !(0.0..=1e12).contains(&timestamp.get())
                    || !state.is_valid()
                {
                    return Err(ProtocolError("invalid aircraft state or timestamp"));
                }
                out.extend(session.to_le_bytes());
                out.extend(participant.to_le_bytes());
                out.extend(sequence.to_le_bytes());
                out.extend(timestamp.get().to_le_bytes());
                for value in state
                    .position
                    .0
                    .to_array()
                    .into_iter()
                    .chain(state.orientation.to_array())
                    .chain(state.velocity_mps.to_array())
                {
                    out.extend(value.to_le_bytes());
                }
            }
            Self::Leave {
                session,
                participant,
            }
            | Self::Ping {
                session,
                participant,
            } => {
                out.extend(session.to_le_bytes());
                out.extend(participant.to_le_bytes());
            }
            Self::Reject { nonce, reason } => {
                out.extend(nonce.to_le_bytes());
                out.push(*reason);
            }
        }
        debug_assert!(out.len() <= MAX_PACKET_BYTES);
        Ok(out)
    }

    /// Decode without allocations controlled by untrusted lengths.
    /// # Errors
    /// Oversize, truncation, trailing bytes, version/kind mismatch or invalid state.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_PACKET_BYTES || bytes.len() < 8 {
            return Err(ProtocolError("invalid packet length"));
        }
        let mut r = Reader { bytes, at: 0 };
        if r.take::<4>()? != *MAGIC {
            return Err(ProtocolError("invalid protocol magic"));
        }
        if r.u16()? != PROTOCOL_VERSION {
            return Err(ProtocolError("incompatible protocol version"));
        }
        let kind = r.u8()?;
        if r.u8()? != 0 {
            return Err(ProtocolError("nonzero reserved flags"));
        }
        let message = match kind {
            1 => {
                let nonce = r.u64()?;
                let len = usize::from(r.u8()?);
                if len == 0 || len > 16 {
                    return Err(ProtocolError("invalid callsign length"));
                }
                let end =
                    r.at.checked_add(len)
                        .ok_or(ProtocolError("length overflow"))?;
                let raw = r
                    .bytes
                    .get(r.at..end)
                    .ok_or(ProtocolError("truncated callsign"))?;
                let callsign = std::str::from_utf8(raw)
                    .map_err(|_| ProtocolError("invalid callsign encoding"))?
                    .to_owned();
                r.at = end;
                Self::Join { nonce, callsign }
            }
            2 => Self::Welcome {
                nonce: r.u64()?,
                session: r.u64()?,
                participant: r.u32()?,
            },
            3 => {
                let session = r.u64()?;
                let participant = r.u32()?;
                let sequence = r.u32()?;
                let timestamp = Seconds(r.f64()?);
                let position = Ecef(DVec3::new(r.f64()?, r.f64()?, r.f64()?));
                let orientation = DQuat::from_xyzw(r.f64()?, r.f64()?, r.f64()?, r.f64()?);
                let velocity_mps = DVec3::new(r.f64()?, r.f64()?, r.f64()?);
                Self::State {
                    session,
                    participant,
                    sequence,
                    timestamp,
                    state: AircraftState {
                        position,
                        orientation,
                        velocity_mps,
                    },
                }
            }
            4 => Self::Leave {
                session: r.u64()?,
                participant: r.u32()?,
            },
            5 => Self::Ping {
                session: r.u64()?,
                participant: r.u32()?,
            },
            6 => Self::Reject {
                nonce: r.u64()?,
                reason: r.u8()?,
            },
            _ => return Err(ProtocolError("unknown message kind")),
        };
        if r.at != bytes.len() {
            return Err(ProtocolError("trailing packet bytes"));
        }
        // Same semantic validation on both sides of the boundary.
        message.encode()?;
        Ok(message)
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], ProtocolError> {
        let end = self
            .at
            .checked_add(N)
            .ok_or(ProtocolError("length overflow"))?;
        let result = self
            .bytes
            .get(self.at..end)
            .ok_or(ProtocolError("truncated packet"))?
            .try_into()
            .map_err(|_| ProtocolError("truncated packet"))?;
        self.at = end;
        Ok(result)
    }
    fn u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.take::<1>()?[0])
    }
    fn u16(&mut self) -> Result<u16, ProtocolError> {
        Ok(u16::from_le_bytes(self.take()?))
    }
    fn u32(&mut self) -> Result<u32, ProtocolError> {
        Ok(u32::from_le_bytes(self.take()?))
    }
    fn u64(&mut self) -> Result<u64, ProtocolError> {
        Ok(u64::from_le_bytes(self.take()?))
    }
    fn f64(&mut self) -> Result<f64, ProtocolError> {
        Ok(f64::from_le_bytes(self.take()?))
    }
}
