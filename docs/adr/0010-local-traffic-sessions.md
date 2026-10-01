# ADR-0010: Pure traffic providers and bounded local sessions

Status: accepted (2026-10-01)

## Decision

`flightsim-net` depends on `flightsim-core` and `glam`, not Bevy, FDM, terrain or UI.
Its shared `TrafficSource` emits callsigns, f64 ECEF position, body-to-ECEF
orientation, velocity and explicit stale state. App alone renders these observations.
Network packets never override the local aircraft's physics.

The first sources are deterministic synthetic circuits and an opt-in version-1 UDP
session. Host binds loopback by default; LAN binding must be explicit. Up to 15 clients
plus the host exchange state at 20 Hz. Packets are fixed-bounded (160 bytes), validated
before use, and processed with a 64-datagram per-poll budget. Wrong versions, invalid
coordinates, non-unit quaternions, invalid callsigns and malformed lengths are rejected.

Arrival times use a monotonic clock. Remote visual state keeps a bounded eight-snapshot history and interpolates 100 ms behind
the latest observations, freezes after 2 seconds without a sample, and expires at 5
seconds. There is no unbounded extrapolation. Participants rejoin after timeout with a
fresh nonzero handshake nonce. Retired participant and join identities are bounded
rings that suppress delayed packets. Nonces identify sessions; they are not credentials.

## Boundaries and limits

This is a trusted local/LAN alpha feature, with no authentication, encryption, NAT
traversal, public matchmaking, authoritative collisions or anti-cheat. Do not expose
it to the Internet. Loopback loss, reordering, malformed packets, restart and leave
behavior are tested; this does not establish WAN or hostile-public-network readiness.
All traffic currently uses a generic proxy mesh; transmitting selected models or
physics definitions is not part of version 1. Synthetic traffic is clearly labeled,
not presented as live aircraft data.

The core CI group and architecture script include net, keeping its test loop free of
rendering dependencies. UI receives plain display data, preserving sibling separation.

The library component supplies providers, transport and protocol validation. The
opt-in app CLI, peer rendering and traffic panel are integrated separately.
