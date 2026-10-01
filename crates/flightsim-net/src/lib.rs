//! Local/LAN multiplayer foundations and deterministic traffic.
//!
//! The protocol is deliberately bounded and has no dependency on Bevy or FDM.
//! This is a trusted-session alpha protocol, not an authenticated Internet service.
//! Public matchmaking, NAT traversal, encryption and collision authority are out
//! of scope. Never expose a host to the Internet without a separate security layer.

pub mod protocol;
pub mod session;
pub mod traffic;

pub use protocol::{AircraftState, Message, PROTOCOL_VERSION, ProtocolError};
pub use session::{Client, ConnectionState, Host, SessionEvent};
pub use traffic::{InterpolatedTraffic, SyntheticTraffic, TrafficObservation, TrafficSource};
