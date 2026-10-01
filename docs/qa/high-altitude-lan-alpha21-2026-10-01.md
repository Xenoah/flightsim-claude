# Two native application instances — local-session QA

Additional full-size native PNGs are retained locally and are not included in this
publication snapshot. Observations, binary hashes, commands and actual logs are
provided below; no missing image links are substituted for public evidence.

Date: 2026-10-01. Executable SHA-256:
`be80a129cf2fbfe4d488673b7be3561d27c1a8ec5bf7aee2a81bbdec694923b6`.
Both instances were the final alpha.21 candidate, rendered by CPU Vulkan/llvmpipe
on the same cloud native desktop. This checks actual application integration on
**127.0.0.1**, not two physical LAN machines, WAN, authentication or hostile networks.

## Observed lifecycle

1. Host and client started as separate native processes, using real normalized DEM
   around 35.55,139.78 and 35.551,139.781. The host logged `Joined` for `CLIENTQA`;
   the client logged `Connected` to session 1790884477329194537.
2. The host screenshot showed **LAN HOST | 2 participants**, the client's callsign,
   range/bearing/relative altitude, and its actual orange remote-aircraft proxy.
3. The host's explicit capture batch exited 0 at 19:54:54.674 UTC. It was kept
   offline for a deliberate eight-second interval before restarting.
4. The client logged `Reconnecting` at 19:54:59.628 UTC, about five seconds after
   the last host process ended, and joined the restarted host automatically.
   Its second `Connected` event was at 19:55:03.639 UTC, for a **different** session
   1790884502749525263. The restarted host also logged `Joined` for CLIENTQA.
5. A saved client image showed LAN CONNECTED after the second connection. F12 was
   then exercised after the second host ended; the native UI settled to **LAN LEFT**.
   This does not establish reception of an explicit Leave by an already-closed host.
6. Both host batch processes returned 0. The client closed normally through the
   native window's close action and returned 0.

The brief two-second stale-display interval was not captured, so **a visible STALE
badge is not claimed** here. Loss/expiry and automatic reconnect are separately
supported by the timestamped actual app logs. The pure network regressions cover
staleness and bounded interpolation; do not substitute that for an uncaptured UI
state. The host currently appears as `NET-0` in the client traffic row.

## Actual images and logs

Host joined image (local capture; full-size PNG not published in this snapshot)



Client after reconnect (local capture; full-size PNG not published in this snapshot)

An initial reduced-size review suspected missing glyph fragments. Independent
original-resolution inspection and three fresh captures (serial headless, serial
native, and native after resizing from 1180×812 to 1280×720) did not establish
a missing-glyph defect. The help block was complete. White needles cross tiny
PWR, V/S and TAS labels/readouts, making those overlapping strokes hard to read;
this also occurs live and in single-instance captures, independently of reconnect.
All three new capture processes exited 0 with the unchanged candidate binary.
This correction does not claim that every UI pixel or device configuration is tested.

Detailed process logs are retained locally; this report publishes aggregate outcomes only.

| File | SHA-256 |
|---|---|
| Host joined PNG | `03d69db6a6fd23f49ce1972fcd5200a72029f442571006a2a032a0e5cdfec2cd` |
| Client reconnected PNG | `b9828123580f85f539ada224485c4e3f08c0d1fd428e16a47486dda6ba2ccfe6` |
| Combined logs/statuses, ANSI escapes removed | `477eae309c9852c85503a43f19e732ef5a72e11a0139e4927859a36f5286e8bd` |

## Reproduction

Use two terminals on the **same** machine/network namespace, with the same build.
`$TILES` is the normalized dataset described in [data QA](data-boundaries-2026-10-01.md).

```sh
# Terminal A
cargo run -p flightsim-app -- --tiles "$TILES" --start 35.55,139.78 \
  --host --callsign HOSTQA --time 09:00 --difficulty beginner

# Terminal B
cargo run -p flightsim-app -- --tiles "$TILES" --start 35.551,139.781 \
  --join 127.0.0.1:41520 --callsign CLIENTQA --time 09:00 --difficulty beginner
```

Inspect the host's participant count and client connection state. Close the host,
leave it absent for eight seconds, then restart it. Verify a new session ID and
automatic reconnection. Press F12 in the client and verify LAN LEFT. No firewall,
security setting, public listener or credentials were changed for this test.

A mixed attempt using a headless shell process and a native desktop process did
not connect: their localhost environments were separate. That attempt was not used
as passing evidence. Running both through the ordinary native desktop was the
supported same-environment test. Two offscreen processes launched together also
connected; its host removed the client approximately five seconds after client exit.
