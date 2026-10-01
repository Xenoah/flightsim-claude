//! Bounded nonblocking UDP sessions for a trusted local/LAN network.
//!
//! Socket construction is explicit; loading this crate never opens a listener.
//! Host clocks timestamp relayed observations, so clients with different start
//! times do not try to interpolate between unrelated clocks.
use crate::{AircraftState, InterpolatedTraffic, Message, TrafficObservation, TrafficSource};
use flightsim_core::Seconds;
use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::net::{SocketAddr, UdpSocket};

pub const MAX_PARTICIPANTS: usize = 16;
const MAX_PACKETS_PER_POLL: usize = 64;
const HEARTBEAT: Seconds = Seconds(1.0);
const TIMEOUT: Seconds = Seconds(5.0);
const SEND_INTERVAL: Seconds = Seconds(0.05);
// Suppress reordered joins for recently retired attempts without unbounded
// per-address history. This is a LAN replay guard, not authentication.
const MAX_RETIRED_JOINS: usize = MAX_PARTICIPANTS * 4;
const MAX_RETIRED_PARTICIPANTS: usize = MAX_PARTICIPANTS * 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEvent {
    Joined { participant: u32, callsign: String },
    Left { participant: u32 },
    Connected { session: u64, participant: u32 },
    Reconnecting,
    Rejected,
    IgnoredMalformedPacket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Joining,
    Connected { session: u64, participant: u32 },
    Reconnecting,
    Left,
    Rejected,
}

#[derive(Debug)]
struct Peer {
    nonce: u64,
    id: u32,
    callsign: String,
    last_seen: Seconds,
    last_sequence: Option<u32>,
}

fn newer(sequence: u32, last: u32) -> bool {
    let distance = sequence.wrapping_sub(last);
    distance > 0 && distance < 0x8000_0000
}
fn send(socket: &UdpSocket, target: SocketAddr, message: &Message) -> io::Result<()> {
    let bytes = message
        .encode()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
    match socket.send_to(&bytes, target) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(()),
        Err(e) => Err(e),
    }
}
fn valid_time(time: Seconds) -> io::Result<()> {
    if time.get().is_finite() && (0.0..=1e12).contains(&time.get()) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "session time must be finite and nonnegative",
        ))
    }
}

/// Owns one UDP socket. Participant zero is the host's own aircraft.
#[derive(Debug)]
pub struct Host {
    socket: UdpSocket,
    session: u64,
    peers: BTreeMap<SocketAddr, Peer>,
    retired_joins: VecDeque<(SocketAddr, u64)>,
    next_id: u32,
    traffic: InterpolatedTraffic,
    last_send: Seconds,
    sequence: u32,
    last_poll: Seconds,
}
impl Host {
    /// Bind explicitly. The application defaults to loopback, never a public
    /// interface. `session` is an instance identifier, not authentication.
    pub fn bind(address: SocketAddr, session: u64) -> io::Result<Self> {
        if session == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "session id zero is reserved",
            ));
        }
        let socket = UdpSocket::bind(address)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            session,
            peers: BTreeMap::new(),
            retired_joins: VecDeque::new(),
            next_id: 1,
            traffic: InterpolatedTraffic::default(),
            last_send: Seconds(-1.0),
            sequence: 0,
            last_poll: Seconds(0.0),
        })
    }
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }
    #[must_use]
    pub fn participant_count(&self) -> usize {
        self.peers.len() + 1
    }
    #[must_use]
    pub fn traffic(&self) -> &InterpolatedTraffic {
        &self.traffic
    }

    /// Process at most 64 datagrams and publish own state at most 20 Hz.
    pub fn poll(
        &mut self,
        now: Seconds,
        own: Option<AircraftState>,
    ) -> io::Result<Vec<SessionEvent>> {
        valid_time(now)?;
        if now.get() < self.last_poll.get() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "session poll time moved backwards",
            ));
        }
        self.last_poll = now;
        let mut events = Vec::new();
        let expired: Vec<_> = self
            .peers
            .iter()
            .filter(|(_, p)| now.get() - p.last_seen.get() > TIMEOUT.get())
            .map(|(&addr, _)| addr)
            .collect();
        for addr in expired {
            if let Some(peer) = self.retire_peer(addr) {
                self.broadcast(
                    &Message::Leave {
                        session: self.session,
                        participant: peer.id,
                    },
                    None,
                )?;
                events.push(SessionEvent::Left {
                    participant: peer.id,
                });
            }
        }
        let mut buffer = [0u8; crate::protocol::MAX_PACKET_BYTES + 1];
        for _ in 0..MAX_PACKETS_PER_POLL {
            let (len, address) = match self.socket.recv_from(&mut buffer) {
                Ok(x) => x,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            };
            let message = match Message::decode(&buffer[..len]) {
                Ok(m) => m,
                Err(_) => {
                    events.push(SessionEvent::IgnoredMalformedPacket);
                    continue;
                }
            };
            match message {
                Message::Join { nonce, callsign } => {
                    if nonce == 0 || self.retired_joins.contains(&(address, nonce)) {
                        continue;
                    }
                    // A new attempt at an existing address must have a new
                    // participant identity, so relayed sequence numbers cannot
                    // collide with snapshots from the retired incarnation.
                    if self
                        .peers
                        .get(&address)
                        .is_some_and(|peer| peer.nonce != nonce)
                    {
                        let old = self.retire_peer(address).expect("existing peer");
                        self.broadcast(
                            &Message::Leave {
                                session: self.session,
                                participant: old.id,
                            },
                            None,
                        )?;
                        events.push(SessionEvent::Left {
                            participant: old.id,
                        });
                    }
                    if !self.peers.contains_key(&address)
                        && self.peers.len() >= MAX_PARTICIPANTS - 1
                    {
                        send(&self.socket, address, &Message::Reject { nonce, reason: 1 })?;
                        continue;
                    }
                    let joined = !self.peers.contains_key(&address);
                    if joined {
                        let id = self.allocate_participant();
                        self.peers.insert(
                            address,
                            Peer {
                                nonce,
                                id,
                                callsign: callsign.clone(),
                                last_seen: now,
                                last_sequence: None,
                            },
                        );
                    }
                    let peer = self.peers.get_mut(&address).expect("inserted peer");
                    peer.last_seen = now;
                    send(
                        &self.socket,
                        address,
                        &Message::Welcome {
                            nonce,
                            session: self.session,
                            participant: peer.id,
                        },
                    )?;
                    if joined {
                        events.push(SessionEvent::Joined {
                            participant: peer.id,
                            callsign,
                        });
                    }
                }
                Message::State {
                    session,
                    participant,
                    sequence,
                    state,
                    ..
                } => {
                    if session != self.session {
                        continue;
                    }
                    let Some(peer) = self.peers.get_mut(&address) else {
                        continue;
                    };
                    if participant != peer.id
                        || peer.last_sequence.is_some_and(|s| !newer(sequence, s))
                    {
                        continue;
                    }
                    peer.last_sequence = Some(sequence);
                    peer.last_seen = now;
                    self.traffic
                        .observe(participant, &peer.callsign, now, state);
                    self.broadcast(
                        &Message::State {
                            session,
                            participant,
                            sequence,
                            timestamp: now,
                            state,
                        },
                        Some(address),
                    )?;
                }
                Message::Ping {
                    session,
                    participant,
                } => match self.peers.get_mut(&address) {
                    Some(peer) if session == self.session && participant == peer.id => {
                        peer.last_seen = now;
                        send(
                            &self.socket,
                            address,
                            &Message::Ping {
                                session,
                                participant,
                            },
                        )?;
                    }
                    _ => {}
                },

                Message::Leave {
                    session,
                    participant,
                } => {
                    if session == self.session
                        && self
                            .peers
                            .get(&address)
                            .is_some_and(|p| p.id == participant)
                    {
                        self.retire_peer(address);
                        self.broadcast(
                            &Message::Leave {
                                session,
                                participant,
                            },
                            None,
                        )?;
                        events.push(SessionEvent::Left { participant });
                    }
                }
                _ => {}
            }
        }
        if now.get() - self.last_send.get() >= SEND_INTERVAL.get() {
            self.last_send = now;
            if let Some(state) = own.filter(|s| s.is_valid()) {
                self.sequence = self.sequence.wrapping_add(1);
                self.broadcast(
                    &Message::State {
                        session: self.session,
                        participant: 0,
                        sequence: self.sequence,
                        timestamp: now,
                        state,
                    },
                    None,
                )?;
            }
        }
        self.traffic.expire(now);
        Ok(events)
    }
    fn broadcast(&self, message: &Message, except: Option<SocketAddr>) -> io::Result<()> {
        for &address in self.peers.keys() {
            if Some(address) != except {
                send(&self.socket, address, message)?;
            }
        }
        Ok(())
    }
    fn retire_peer(&mut self, address: SocketAddr) -> Option<Peer> {
        let peer = self.peers.remove(&address)?;
        self.traffic.remove(peer.id);
        if self.retired_joins.len() == MAX_RETIRED_JOINS {
            self.retired_joins.pop_front();
        }
        self.retired_joins.push_back((address, peer.nonce));
        Some(peer)
    }
    fn allocate_participant(&mut self) -> u32 {
        loop {
            let id = self.next_id;
            // Keep the high bit reserved for application synthetic traffic.
            self.next_id = if id == 0x7fff_ffff { 1 } else { id + 1 };
            if !self.peers.values().any(|peer| peer.id == id) {
                return id;
            }
        }
    }
}

#[derive(Debug)]
pub struct Client {
    socket: UdpSocket,
    server: SocketAddr,
    nonce: u64,
    callsign: String,
    connection: ConnectionState,
    last_received: Seconds,
    last_heartbeat: Seconds,
    last_send: Seconds,
    sequence: u32,
    last_poll: Seconds,
    remote_sequences: BTreeMap<u32, u32>,
    retired_participants: VecDeque<u32>,
    traffic: InterpolatedTraffic,
}
impl Client {
    /// `nonce` identifies the initial connection attempt and must be nonzero.
    /// Automatic reconnect advances it to reject delayed negotiation replies;
    /// it is not a password or authorization capability.
    pub fn connect(
        bind: SocketAddr,
        server: SocketAddr,
        nonce: u64,
        callsign: &str,
    ) -> io::Result<Self> {
        if nonce == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "client nonce must be nonzero",
            ));
        }
        Message::Join {
            nonce,
            callsign: callsign.into(),
        }
        .encode()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let socket = UdpSocket::bind(bind)?;
        socket.set_nonblocking(true)?;
        Ok(Self {
            socket,
            server,
            nonce,
            callsign: callsign.into(),
            connection: ConnectionState::Joining,
            last_received: Seconds(0.0),
            last_heartbeat: Seconds(-1.0),
            last_send: Seconds(-1.0),
            sequence: 0,
            last_poll: Seconds(0.0),
            remote_sequences: BTreeMap::new(),
            retired_participants: VecDeque::new(),
            traffic: InterpolatedTraffic::default(),
        })
    }
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.socket.local_addr()
    }
    #[must_use]
    pub fn connection(&self) -> ConnectionState {
        self.connection
    }
    #[must_use]
    pub fn traffic(&self) -> &InterpolatedTraffic {
        &self.traffic
    }

    pub fn poll(
        &mut self,
        now: Seconds,
        state: Option<AircraftState>,
    ) -> io::Result<Vec<SessionEvent>> {
        valid_time(now)?;
        if now.get() < self.last_poll.get() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "session poll time moved backwards",
            ));
        }
        self.last_poll = now;
        let mut events = Vec::new();
        if matches!(
            self.connection,
            ConnectionState::Left | ConnectionState::Rejected
        ) {
            return Ok(events);
        }
        if matches!(self.connection, ConnectionState::Connected { .. })
            && now.get() - self.last_received.get() > TIMEOUT.get()
        {
            self.connection = ConnectionState::Reconnecting;
            self.nonce = self.nonce.wrapping_add(1).max(1);
            self.traffic.clear();
            self.remote_sequences.clear();
            self.retired_participants.clear();
            self.last_heartbeat = Seconds(-1.0);
            events.push(SessionEvent::Reconnecting);
        }
        if now.get() - self.last_heartbeat.get() >= HEARTBEAT.get() {
            self.last_heartbeat = now;
            let message = match self.connection {
                ConnectionState::Connected {
                    session,
                    participant,
                } => Message::Ping {
                    session,
                    participant,
                },
                _ => Message::Join {
                    nonce: self.nonce,
                    callsign: self.callsign.clone(),
                },
            };
            send(&self.socket, self.server, &message)?;
        }
        let mut buffer = [0u8; crate::protocol::MAX_PACKET_BYTES + 1];
        for _ in 0..MAX_PACKETS_PER_POLL {
            let (len, address) = match self.socket.recv_from(&mut buffer) {
                Ok(x) => x,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            };
            if address != self.server {
                continue;
            }
            let message = match Message::decode(&buffer[..len]) {
                Ok(m) => m,
                Err(_) => {
                    events.push(SessionEvent::IgnoredMalformedPacket);
                    continue;
                }
            };
            match message {
                Message::Welcome {
                    nonce,
                    session,
                    participant,
                } if matches!(
                    self.connection,
                    ConnectionState::Joining | ConnectionState::Reconnecting
                ) && nonce == self.nonce
                    && session != 0
                    && participant > 0
                    && participant < 0x8000_0000 =>
                {
                    let changed = self.connection
                        != ConnectionState::Connected {
                            session,
                            participant,
                        };
                    if changed {
                        self.remote_sequences.clear();
                        self.retired_participants.clear();
                        self.traffic.clear();
                        events.push(SessionEvent::Connected {
                            session,
                            participant,
                        });
                    }
                    self.connection = ConnectionState::Connected {
                        session,
                        participant,
                    };
                    self.last_received = now;
                }
                Message::State {
                    session,
                    participant,
                    sequence,
                    state,
                    ..
                } => {
                    let ConnectionState::Connected {
                        session: active,
                        participant: own,
                    } = self.connection
                    else {
                        continue;
                    };
                    if session != active
                        || participant == own
                        || participant >= 0x8000_0000
                        || self.retired_participants.contains(&participant)
                    {
                        continue;
                    }
                    if self
                        .remote_sequences
                        .get(&participant)
                        .is_some_and(|&last| !newer(sequence, last))
                    {
                        continue;
                    }
                    if !self.remote_sequences.contains_key(&participant)
                        && self.remote_sequences.len() >= MAX_PARTICIPANTS
                    {
                        continue;
                    }
                    self.remote_sequences.insert(participant, sequence);
                    self.last_received = now;
                    // Receipt time provides one consistent local clock; packet
                    // timestamps remain metadata, not a cross-clock assumption.
                    self.traffic
                        .observe(participant, &format!("NET-{participant}"), now, state);
                }
                Message::Ping {
                    session,
                    participant,
                } if self.connection
                    == ConnectionState::Connected {
                        session,
                        participant,
                    } =>
                {
                    self.last_received = now;
                }
                Message::Leave {
                    session,
                    participant,
                } => {
                    if matches!(self.connection,ConnectionState::Connected{session:s,..} if s==session)
                    {
                        self.traffic.remove(participant);
                        self.remote_sequences.remove(&participant);
                        if !self.retired_participants.contains(&participant) {
                            if self.retired_participants.len() == MAX_RETIRED_PARTICIPANTS {
                                self.retired_participants.pop_front();
                            }
                            self.retired_participants.push_back(participant);
                        }
                        events.push(SessionEvent::Left { participant });
                    }
                }
                Message::Reject { nonce, .. }
                    if matches!(
                        self.connection,
                        ConnectionState::Joining | ConnectionState::Reconnecting
                    ) && nonce == self.nonce =>
                {
                    self.connection = ConnectionState::Rejected;
                    events.push(SessionEvent::Rejected);
                }
                _ => {}
            }
        }
        match self.connection {
            ConnectionState::Connected {
                session,
                participant,
            } if now.get() - self.last_send.get() >= SEND_INTERVAL.get() => {
                if let Some(state) = state.filter(|s| s.is_valid()) {
                    self.sequence = self.sequence.wrapping_add(1);
                    send(
                        &self.socket,
                        self.server,
                        &Message::State {
                            session,
                            participant,
                            sequence: self.sequence,
                            timestamp: now,
                            state,
                        },
                    )?;
                    self.last_send = now;
                }
            }
            _ => {}
        }
        self.traffic.expire(now);
        self.remote_sequences
            .retain(|id, _| self.traffic.contains(*id));
        Ok(events)
    }
    pub fn leave(&mut self) -> io::Result<()> {
        let result = if let ConnectionState::Connected {
            session,
            participant,
        } = self.connection
        {
            send(
                &self.socket,
                self.server,
                &Message::Leave {
                    session,
                    participant,
                },
            )
        } else {
            Ok(())
        };
        // Local disconnect must succeed even when the best-effort UDP notice
        // cannot be transmitted. Preserve the send error for the caller's log.
        self.connection = ConnectionState::Left;
        self.traffic.clear();
        self.remote_sequences.clear();
        self.retired_participants.clear();
        result
    }
}
impl TrafficSource for Host {
    fn sample(&self, now: Seconds) -> Vec<TrafficObservation> {
        self.traffic.sample(now)
    }
}
impl TrafficSource for Client {
    fn sample(&self, now: Seconds) -> Vec<TrafficObservation> {
        self.traffic.sample(now)
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.leave();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_leave_notice_still_clears_local_connection() {
        // An IPv4 socket cannot send to an IPv6 destination; no remote service
        // or network failure timing is needed to exercise the error path.
        let mut client = Client::connect(
            "127.0.0.1:0".parse().unwrap(),
            "[::1]:1".parse().unwrap(),
            1,
            "LEAVE",
        )
        .unwrap();
        client.connection = ConnectionState::Connected {
            session: 10,
            participant: 1,
        };
        client.remote_sequences.insert(2, 1);
        client.retired_participants.push_back(3);
        assert!(client.leave().is_err());
        assert_eq!(client.connection(), ConnectionState::Left);
        assert!(client.traffic.is_empty());
        assert!(client.remote_sequences.is_empty());
        assert!(client.retired_participants.is_empty());
    }
}
