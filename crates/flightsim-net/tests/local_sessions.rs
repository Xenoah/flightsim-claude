//! Real loopback UDP integration tests. Each test binds port zero; no fixed port,
//! public interface, external service, credentials or physical controller needed.
use flightsim_core::{Ecef, Seconds};
use flightsim_net::{AircraftState, Client, ConnectionState, Host, Message, TrafficSource};
use glam::{DQuat, DVec3};
use std::net::{SocketAddr, UdpSocket};
fn local() -> SocketAddr {
    "127.0.0.1:0".parse().unwrap()
}
fn state(offset: f64) -> AircraftState {
    AircraftState {
        position: Ecef(DVec3::new(6_378_137.0, offset, 0.0)),
        orientation: DQuat::IDENTITY,
        velocity_mps: DVec3::ZERO,
    }
}
fn connect(host: &mut Host, client: &mut Client, time: f64) {
    for step in 0..10 {
        let t = Seconds(time + f64::from(step) * 0.01);
        client.poll(t, None).unwrap();
        host.poll(t, Some(state(0.0))).unwrap();
        client.poll(t, None).unwrap();
        if matches!(client.connection(), ConnectionState::Connected { .. }) {
            return;
        }
    }
    panic!("local join did not complete");
}
#[test]
fn create_join_sync_and_leave_are_observable() {
    let mut host = Host::bind(local(), 123).unwrap();
    let address = host.local_addr().unwrap();
    let mut a = Client::connect(local(), address, 1, "ALPHA").unwrap();
    let mut b = Client::connect(local(), address, 2, "BRAVO").unwrap();
    connect(&mut host, &mut a, 0.0);
    connect(&mut host, &mut b, 0.2);
    assert_eq!(host.participant_count(), 3);
    for n in 0..20 {
        let t = Seconds(1.0 + f64::from(n) * 0.1);
        a.poll(t, Some(state(100.0))).unwrap();
        b.poll(t, Some(state(200.0))).unwrap();
        host.poll(t, Some(state(0.0))).unwrap();
        a.poll(t, None).unwrap();
        b.poll(t, None).unwrap();
    }
    assert_eq!(host.sample(Seconds(2.9)).len(), 2);
    assert_eq!(a.sample(Seconds(2.9)).len(), 2);
    assert_eq!(b.sample(Seconds(2.9)).len(), 2);
    a.leave().unwrap();
    host.poll(Seconds(3.0), None).unwrap();
    b.poll(Seconds(3.0), None).unwrap();
    assert_eq!(host.participant_count(), 2);
    assert_eq!(a.connection(), ConnectionState::Left);
    assert!(a.sample(Seconds(3.0)).is_empty());
}
#[test]
fn packet_loss_times_out_and_reconnects_without_process_restart() {
    let mut host = Host::bind(local(), 1).unwrap();
    let mut c = Client::connect(local(), host.local_addr().unwrap(), 10, "REJOIN").unwrap();
    connect(&mut host, &mut c, 0.0);
    // Client receives nothing for more than five seconds. Host expires its old
    // peer, then consumes a retried join from the same running client.
    c.poll(Seconds(7.0), None).unwrap();
    assert_eq!(c.connection(), ConnectionState::Reconnecting);
    host.poll(Seconds(7.0), None).unwrap();
    c.poll(Seconds(7.01), None).unwrap();
    assert!(matches!(c.connection(), ConnectionState::Connected { .. }));
    assert_eq!(host.participant_count(), 2);
}
#[test]
fn malformed_and_spoofed_packets_do_not_replace_valid_state() {
    let mut host = Host::bind(local(), 42).unwrap();
    let mut c = Client::connect(local(), host.local_addr().unwrap(), 9, "SAFE").unwrap();
    connect(&mut host, &mut c, 0.0);
    c.poll(Seconds(1.0), Some(state(50.0))).unwrap();
    host.poll(Seconds(1.0), None).unwrap();
    let ConnectionState::Connected {
        session,
        participant,
    } = c.connection()
    else {
        panic!()
    };
    let hostile = UdpSocket::bind(local()).unwrap();
    hostile
        .send_to(&[255; 161], host.local_addr().unwrap())
        .unwrap();
    let bytes = Message::State {
        session,
        participant,
        sequence: 999,
        timestamp: Seconds(1.1),
        state: state(99999.0),
    }
    .encode()
    .unwrap();
    hostile.send_to(&bytes, host.local_addr().unwrap()).unwrap();
    host.poll(Seconds(1.1), None).unwrap();
    assert!((host.sample(Seconds(1.2))[0].state.position.0.y - 50.0).abs() < 1e-9);
}

#[test]
fn reordered_and_wrapped_sequence_numbers_do_not_rewind_aircraft() {
    let mut host = Host::bind(local(), 77).unwrap();
    let sender = UdpSocket::bind(local()).unwrap();
    sender
        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
        .unwrap();
    sender
        .send_to(
            &Message::Join {
                nonce: 23,
                callsign: "ORDER".into(),
            }
            .encode()
            .unwrap(),
            host.local_addr().unwrap(),
        )
        .unwrap();
    host.poll(Seconds(0.0), None).unwrap();
    let mut buffer = [0u8; 160];
    let (len, _) = sender.recv_from(&mut buffer).unwrap();
    let Message::Welcome {
        session,
        participant,
        ..
    } = Message::decode(&buffer[..len]).unwrap()
    else {
        panic!()
    };
    for (sequence, position, t) in [
        (u32::MAX - 1, 10.0, 1.0),
        (0, 20.0, 1.1),
        (u32::MAX, 999.0, 1.2),
        (0, 888.0, 1.3),
        (1, 30.0, 1.4),
    ] {
        sender
            .send_to(
                &Message::State {
                    session,
                    participant,
                    sequence,
                    timestamp: Seconds(t),
                    state: state(position),
                }
                .encode()
                .unwrap(),
                host.local_addr().unwrap(),
            )
            .unwrap();
        host.poll(Seconds(t), None).unwrap();
    }
    assert!((host.sample(Seconds(1.6))[0].state.position.0.y - 30.0).abs() < 1e-9);
}

#[test]
fn session_capacity_rejects_new_clients_without_evicting_active_members() {
    let mut host = Host::bind(local(), 10).unwrap();
    let mut clients = Vec::new();
    for n in 0..15 {
        let mut client =
            Client::connect(local(), host.local_addr().unwrap(), n + 1, "MEMBER").unwrap();
        connect(&mut host, &mut client, 0.0);
        clients.push(client);
    }
    assert_eq!(host.participant_count(), 16);
    let mut extra = Client::connect(local(), host.local_addr().unwrap(), 99, "EXTRA").unwrap();
    extra.poll(Seconds(0.5), None).unwrap();
    host.poll(Seconds(0.5), None).unwrap();
    extra.poll(Seconds(0.51), None).unwrap();
    assert_eq!(extra.connection(), ConnectionState::Rejected);
    assert_eq!(host.participant_count(), 16);
}

fn receive(socket: &UdpSocket) -> Message {
    socket
        .set_read_timeout(Some(std::time::Duration::from_secs(1)))
        .unwrap();
    let mut buffer = [0_u8; 160];
    let (length, _) = socket.recv_from(&mut buffer).unwrap();
    Message::decode(&buffer[..length]).unwrap()
}

#[allow(
    clippy::needless_pass_by_value,
    reason = "test helper consumes one-use packet fixtures"
)]
fn send(socket: &UdpSocket, destination: SocketAddr, message: Message) {
    socket
        .send_to(&message.encode().unwrap(), destination)
        .unwrap();
}

#[test]
fn delayed_negotiation_replies_cannot_replace_a_connected_session() {
    let server = UdpSocket::bind(local()).unwrap();
    let mut client = Client::connect(local(), server.local_addr().unwrap(), 91, "ORDER").unwrap();
    client.poll(Seconds(0.0), None).unwrap();
    assert!(matches!(receive(&server), Message::Join { nonce: 91, .. }));
    let destination = client.local_addr().unwrap();
    send(
        &server,
        destination,
        Message::Welcome {
            nonce: 91,
            session: 10,
            participant: 1,
        },
    );
    client.poll(Seconds(0.1), None).unwrap();
    send(
        &server,
        destination,
        Message::Welcome {
            nonce: 91,
            session: 99,
            participant: 2,
        },
    );
    send(
        &server,
        destination,
        Message::Reject {
            nonce: 91,
            reason: 1,
        },
    );
    client.poll(Seconds(0.2), None).unwrap();
    assert_eq!(
        client.connection(),
        ConnectionState::Connected {
            session: 10,
            participant: 1
        }
    );
}

#[test]
fn reconnect_uses_a_new_nonce_and_ignores_previous_attempt_replies() {
    let server = UdpSocket::bind(local()).unwrap();
    let mut client =
        Client::connect(local(), server.local_addr().unwrap(), u64::MAX, "REJOIN").unwrap();
    client.poll(Seconds(0.0), None).unwrap();
    assert!(matches!(
        receive(&server),
        Message::Join {
            nonce: u64::MAX,
            ..
        }
    ));
    let destination = client.local_addr().unwrap();
    send(
        &server,
        destination,
        Message::Welcome {
            nonce: u64::MAX,
            session: 10,
            participant: 1,
        },
    );
    client.poll(Seconds(0.1), None).unwrap();
    client.poll(Seconds(7.0), None).unwrap();
    let Message::Join { nonce, .. } = receive(&server) else {
        panic!("expected retry")
    };
    assert_ne!(nonce, u64::MAX);
    assert_ne!(nonce, 0);
    send(
        &server,
        destination,
        Message::Welcome {
            nonce: u64::MAX,
            session: 10,
            participant: 1,
        },
    );
    send(
        &server,
        destination,
        Message::Reject {
            nonce: u64::MAX,
            reason: 1,
        },
    );
    client.poll(Seconds(7.1), None).unwrap();
    assert_eq!(client.connection(), ConnectionState::Reconnecting);
    send(
        &server,
        destination,
        Message::Welcome {
            nonce,
            session: 11,
            participant: 2,
        },
    );
    client.poll(Seconds(7.2), None).unwrap();
    assert_eq!(
        client.connection(),
        ConnectionState::Connected {
            session: 11,
            participant: 2
        }
    );
}

#[test]
fn a_state_delayed_past_leave_does_not_resurrect_departed_traffic() {
    let server = UdpSocket::bind(local()).unwrap();
    let mut client = Client::connect(local(), server.local_addr().unwrap(), 91, "ORDER").unwrap();
    client.poll(Seconds(0.0), None).unwrap();
    let _ = receive(&server);
    let destination = client.local_addr().unwrap();
    send(
        &server,
        destination,
        Message::Welcome {
            nonce: 91,
            session: 10,
            participant: 1,
        },
    );
    send(
        &server,
        destination,
        Message::State {
            session: 10,
            participant: 2,
            sequence: 1,
            timestamp: Seconds(0.1),
            state: state(10.0),
        },
    );
    client.poll(Seconds(0.1), None).unwrap();
    assert!(client.traffic().contains(2));
    send(
        &server,
        destination,
        Message::Leave {
            session: 10,
            participant: 2,
        },
    );
    send(
        &server,
        destination,
        Message::State {
            session: 10,
            participant: 2,
            sequence: 2,
            timestamp: Seconds(0.15),
            state: state(200.0),
        },
    );
    client.poll(Seconds(0.2), None).unwrap();
    assert!(!client.traffic().contains(2));
    assert!(client.sample(Seconds(0.3)).is_empty());
}

#[test]
fn new_attempt_retires_old_identity_and_reordered_join_cannot_restore_it() {
    let mut host = Host::bind(local(), 22).unwrap();
    let address = host.local_addr().unwrap();
    let mut observer = Client::connect(local(), address, 1, "OBSERVER").unwrap();
    connect(&mut host, &mut observer, 0.0);
    let sender = UdpSocket::bind(local()).unwrap();
    send(
        &sender,
        address,
        Message::Join {
            nonce: 20,
            callsign: "OLD".into(),
        },
    );
    host.poll(Seconds(0.1), None).unwrap();
    let Message::Welcome {
        participant: old, ..
    } = receive(&sender)
    else {
        panic!()
    };
    send(
        &sender,
        address,
        Message::State {
            session: 22,
            participant: old,
            sequence: 100,
            timestamp: Seconds(1.0),
            state: state(10.0),
        },
    );
    host.poll(Seconds(1.0), None).unwrap();
    observer.poll(Seconds(1.0), None).unwrap();
    assert!(observer.traffic().contains(old));

    send(
        &sender,
        address,
        Message::Join {
            nonce: 21,
            callsign: "NEW".into(),
        },
    );
    host.poll(Seconds(1.1), None).unwrap();
    let Message::Welcome {
        participant: new, ..
    } = receive(&sender)
    else {
        panic!()
    };
    assert_ne!(old, new);
    send(
        &sender,
        address,
        Message::State {
            session: 22,
            participant: new,
            sequence: 1,
            timestamp: Seconds(1.2),
            state: state(200.0),
        },
    );
    host.poll(Seconds(1.2), None).unwrap();
    observer.poll(Seconds(1.2), None).unwrap();
    assert!(!observer.traffic().contains(old));
    let current = observer
        .sample(Seconds(1.4))
        .into_iter()
        .find(|track| track.id == new)
        .unwrap();
    assert!((current.state.position.0.y - 200.0).abs() < 1e-9);

    send(
        &sender,
        address,
        Message::Join {
            nonce: 20,
            callsign: "OLD".into(),
        },
    );
    send(
        &sender,
        address,
        Message::State {
            session: 22,
            participant: old,
            sequence: 101,
            timestamp: Seconds(1.3),
            state: state(999.0),
        },
    );
    let events = host.poll(Seconds(1.3), None).unwrap();
    assert!(
        events.is_empty(),
        "retired attempt affected active incarnation: {events:?}"
    );
    assert_eq!(host.participant_count(), 3);
    assert!(host.traffic().contains(new));
    assert!(!host.traffic().contains(old));
}
