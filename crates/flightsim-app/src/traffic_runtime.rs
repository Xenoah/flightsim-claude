//! Connect pure traffic/session providers to the scene and generic UI panel.
use crate::FlightSimulation;
use bevy::prelude::*;
use flightsim_core::{Geodetic, LocalFrame, Meters, Seconds};
use flightsim_net::{
    AircraftState, Client, ConnectionState, Host, SessionEvent, SyntheticTraffic,
    TrafficObservation, TrafficSource,
};
use flightsim_render::{WorldOrientation, WorldPosition};
use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub(super) struct Options {
    pub synthetic: bool,
    pub host: Option<SocketAddr>,
    pub join: Option<SocketAddr>,
    pub callsign: String,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            synthetic: false,
            host: None,
            join: None,
            callsign: "PILOT".into(),
        }
    }
}

#[derive(Debug)]
enum Session {
    None,
    Host(Host),
    Client(Client),
}

#[derive(Resource, Debug)]
pub(super) struct TrafficRuntime {
    synthetic: Option<SyntheticTraffic>,
    session: Session,
    entities: BTreeMap<u32, Entity>,
    last_error: Option<String>,
}
impl TrafficRuntime {
    pub fn new(options: &Options, airport: Geodetic) -> Result<Self, String> {
        if options.host.is_some() && options.join.is_some() {
            return Err("--host and --join are mutually exclusive".into());
        }
        // Ephemeral process/session identity, never an authentication token.
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        let nonce = u64::try_from(nonce % u128::from(u64::MAX))
            .expect("bounded nonce")
            .max(1);
        let session = if let Some(address) = options.host {
            Session::Host(
                Host::bind(address, nonce).map_err(|e| format!("cannot host at {address}: {e}"))?,
            )
        } else if let Some(address) = options.join {
            let bind: SocketAddr = if address.is_ipv4() {
                "0.0.0.0:0"
            } else {
                "[::]:0"
            }
            .parse()
            .expect("constant bind address");
            Session::Client(
                Client::connect(bind, address, nonce, &options.callsign)
                    .map_err(|e| format!("cannot join {address}: {e}"))?,
            )
        } else {
            Session::None
        };
        Ok(Self {
            synthetic: options.synthetic.then(|| SyntheticTraffic::new(airport)),
            session,
            entities: BTreeMap::new(),
            last_error: None,
        })
    }
}

#[derive(Resource)]
pub(super) struct TrafficMeshes {
    parts: Vec<(Handle<Mesh>, Transform)>,
    normal: Handle<StandardMaterial>,
    stale: Handle<StandardMaterial>,
}

pub(super) fn setup_traffic_meshes(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    simulation: Res<FlightSimulation>,
    mut runtime: ResMut<TrafficRuntime>,
) {
    // Keep the test pattern above the actual terrain-supported start, including
    // real DEM elevations and airborne replay/approach starts.
    if runtime.synthetic.is_some() {
        runtime.synthetic = Some(SyntheticTraffic::new(simulation.0.state().geodetic()));
    }
    let parts = flightsim_render::aircraft::placeholder_parts(simulation.0.config())
        .into_iter()
        .map(|p| (meshes.add(p.mesh), p.transform))
        .collect();
    let normal = materials.add(StandardMaterial {
        base_color: Color::srgb(0.95, 0.42, 0.07),
        perceptual_roughness: 0.5,
        ..default()
    });
    let stale = materials.add(StandardMaterial {
        base_color: Color::srgb(0.40, 0.40, 0.40),
        perceptual_roughness: 0.7,
        ..default()
    });
    commands.insert_resource(TrafficMeshes {
        parts,
        normal,
        stale,
    });
}

#[expect(
    clippy::too_many_arguments,
    reason = "Traffic provider, world state, input and presentation bridge"
)]
pub(super) fn update_traffic(
    time: Res<Time<Real>>,
    simulation: Res<FlightSimulation>,
    keyboard: Res<ButtonInput<KeyCode>>,
    mut runtime: ResMut<TrafficRuntime>,
    assets: Res<TrafficMeshes>,
    mut commands: Commands,
    mut panel: ResMut<flightsim_ui::TrafficPanel>,
    mut transforms: Query<(&mut WorldPosition, &mut WorldOrientation)>,
    children: Query<&Children>,
    mut materials: Query<&mut MeshMaterial3d<StandardMaterial>>,
) {
    let now = Seconds(time.elapsed_secs_f64());
    let state = simulation.0.state();
    let own = AircraftState {
        position: state.position,
        orientation: state.orientation,
        velocity_mps: state.velocity,
    };
    if keyboard.just_pressed(KeyCode::F12)
        && let Session::Client(client) = &mut runtime.session
        && let Err(error) = client.leave()
    {
        warn!("leave session: {error}");
    }
    let result = match &mut runtime.session {
        Session::None => Ok(Vec::new()),
        Session::Host(host) => host.poll(now, Some(own)),
        Session::Client(client) => client.poll(now, Some(own)),
    };
    match result {
        Ok(events) => {
            for event in events {
                if event != SessionEvent::IgnoredMalformedPacket {
                    info!("session: {event:?}");
                }
            }
            runtime.last_error = None;
        }
        Err(error) => {
            let message = error.to_string();
            if runtime.last_error.as_deref() != Some(&message) {
                warn!("session: {message}");
                runtime.last_error = Some(message);
            }
        }
    }
    let mut observations = runtime
        .synthetic
        .as_ref()
        .map_or_else(Vec::new, |source| source.sample(simulation.0.elapsed()));
    // Synthetic IDs occupy a different range than host-assigned participants.
    for observation in &mut observations {
        observation.id |= 0x8000_0000;
    }
    let session_label = match &runtime.session {
        Session::None => {
            if runtime.synthetic.is_some() {
                "SYNTHETIC TRAFFIC".to_owned()
            } else {
                String::new()
            }
        }
        Session::Host(host) => {
            observations.extend(host.sample(now));
            format!("LAN HOST | {} participants", host.participant_count())
        }
        Session::Client(client) => {
            observations.extend(client.sample(now));
            match client.connection() {
                ConnectionState::Joining => "LAN JOINING".into(),
                ConnectionState::Reconnecting => "LAN RECONNECTING".into(),
                ConnectionState::Connected { .. } => "LAN CONNECTED | F12 leave".into(),
                ConnectionState::Left => "LAN LEFT".into(),
                ConnectionState::Rejected => "LAN REJECTED (session full)".into(),
            }
        }
    };
    observations.retain(|o| {
        flightsim_net::traffic::within_range(state.position, o.state.position, Meters(10_000.0))
    });
    observations.sort_by(|a, b| {
        state
            .position
            .0
            .distance_squared(a.state.position.0)
            .total_cmp(&state.position.0.distance_squared(b.state.position.0))
            .then(a.id.cmp(&b.id))
    });
    let visible_ids: BTreeSet<_> = observations.iter().map(|o| o.id).collect();
    let removed: Vec<_> = runtime
        .entities
        .keys()
        .filter(|id| !visible_ids.contains(id))
        .copied()
        .collect();
    for id in removed {
        if let Some(entity) = runtime.entities.remove(&id) {
            commands.entity(entity).despawn();
        }
    }
    for observation in &observations {
        if let Some(&entity) = runtime.entities.get(&observation.id) {
            if let Ok((mut position, mut orientation)) = transforms.get_mut(entity) {
                position.0 = observation.state.position;
                orientation.0 = observation.state.orientation;
            }
            if let Ok(parts) = children.get(entity) {
                for child in parts.iter() {
                    if let Ok(mut material) = materials.get_mut(child) {
                        material.0 = if observation.stale {
                            assets.stale.clone()
                        } else {
                            assets.normal.clone()
                        };
                    }
                }
            }
        } else {
            let entity = commands
                .spawn((
                    WorldPosition(observation.state.position),
                    WorldOrientation(observation.state.orientation),
                    Transform::default(),
                    Visibility::Inherited,
                    Name::new(format!("traffic {}", observation.callsign)),
                ))
                .with_children(|parent| {
                    for (mesh, transform) in &assets.parts {
                        parent.spawn((
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(assets.normal.clone()),
                            *transform,
                        ));
                    }
                })
                .id();
            runtime.entities.insert(observation.id, entity);
        }
    }
    panel.visible = !session_label.is_empty();
    panel.text = traffic_text(
        &session_label,
        state.position.to_geodetic(),
        &observations,
        runtime.last_error.as_deref(),
    );
}

fn traffic_text(
    title: &str,
    observer: Geodetic,
    observations: &[TrafficObservation],
    error: Option<&str>,
) -> String {
    let mut text = title.to_owned();
    let frame = LocalFrame::new(observer);
    for observation in observations.iter().take(8) {
        let relative = frame.ecef_to_ned_position(observation.state.position);
        let contact_frame = LocalFrame::new(observation.state.position.to_geodetic());
        let heading = flightsim_core::Attitude::from_quaternion(
            contact_frame.ned_to_ecef_rotation().inverse() * observation.state.orientation,
        )
        .yaw
        .to_degrees()
        .get()
        .rem_euclid(360.0);
        text.push_str(&format!(
            "\n{} BRG{:03.0} {:0.1}km {:+.0}m HDG{:03.0}{}",
            observation.callsign,
            relative.bearing().to_degrees().get(),
            relative.0.length() / 1000.0,
            -relative.down(),
            heading,
            if observation.stale { " STALE" } else { "" }
        ));
    }
    if observations.len() > 8 {
        text.push_str(&format!("\n+{} other contacts", observations.len() - 8));
    }
    if let Some(error) = error {
        text.push_str(&format!("\nERROR: {error}"));
    }
    text
}
