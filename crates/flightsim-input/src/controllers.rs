//! Multi-device input sampling, mapping, and live diagnostics.
//!
//! Samples are taken from raw Bevy messages, before its built-in deadzone. The
//! same stored values feed calibration, diagnostics, and flight controls.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use bevy::input::gamepad::{GamepadConnection, RawGamepadEvent};
use bevy::prelude::*;

use crate::configuration::{
    AnalogInput, AxisId, BindingMode, BindingSource, ButtonId, ButtonSource, ControlBinding,
    DeviceSelector, InputConfiguration,
};
use crate::native::NativeInputEvent;
use crate::{PilotControls, ViewMode};

/// Input scheduling contract for app integration and diagnostic publication.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, SystemSet)]
pub enum InputSystems {
    Devices,
    Sample,
    Diagnostics,
}

/// Active configuration. The app loads and validates explicitly requested files.
#[derive(Resource, Debug, Clone, Default)]
pub struct InputSettings {
    pub configuration: InputConfiguration,
}

/// A model identity with a session-stable occurrence number for identical units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub name: String,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
    pub instance: u16,
}

/// Unfiltered device state. Missing channels have not yet been observed; they
/// are not fabricated as centered axes (which could set a throttle to 50%).
#[derive(Debug, Clone)]
pub struct DeviceSnapshot {
    pub identity: DeviceIdentity,
    pub connected: bool,
    pub axes: BTreeMap<AxisId, f64>,
    pub buttons: BTreeMap<ButtonId, f64>,
    entity: Entity,
}

/// Current and disconnected devices. Slots survive disconnect/reconnect, so
/// unplugging identical unit 0 never turns unit 1 into unit 0 mid-flight.
#[derive(Resource, Debug, Default)]
pub struct InputDevices {
    devices: Vec<DeviceSnapshot>,
    backend_label: &'static str,
}

impl InputDevices {
    pub(crate) fn set_backend_label(&mut self, label: &'static str) {
        self.backend_label = label;
    }

    fn apply_native_event(&mut self, event: &NativeInputEvent) {
        match *event {
            NativeInputEvent::Reset { entity } => {
                if let Some(device) = self
                    .devices
                    .iter_mut()
                    .find(|device| device.entity == entity)
                {
                    device
                        .axes
                        .retain(|axis, _| !matches!(axis, AxisId::Native(_)));
                    device
                        .buttons
                        .retain(|button, _| !matches!(button, ButtonId::Native(_)));
                }
            }
            NativeInputEvent::Sample {
                entity,
                input,
                value,
            } => {
                if let Some(device) = self.connected_mut(entity) {
                    match input {
                        AnalogInput::Axis(axis) => {
                            device.axes.insert(axis, value);
                        }
                        AnalogInput::Button(button) => {
                            device.buttons.insert(button, value);
                        }
                    }
                }
            }
        }
    }

    #[must_use]
    pub fn devices(&self) -> &[DeviceSnapshot] {
        &self.devices
    }

    fn connect(
        &mut self,
        entity: Entity,
        name: &str,
        vendor_id: Option<u16>,
        product_id: Option<u16>,
    ) {
        if let Some(device) = self
            .devices
            .iter_mut()
            .find(|device| device.entity == entity)
        {
            if !device.connected {
                device.axes.clear();
                device.buttons.clear();
            }
            device.connected = true;
            return;
        }
        // Backends may assign a fresh ECS entity after reconnect. Reuse the
        // earliest vacant slot of the same model, never a connected unit's slot.
        if let Some(device) = self.devices.iter_mut().find(|device| {
            !device.connected
                && device.identity.name == name
                && device.identity.vendor_id == vendor_id
                && device.identity.product_id == product_id
        }) {
            device.entity = entity;
            device.connected = true;
            device.axes.clear();
            device.buttons.clear();
            return;
        }
        // Bound memory even for a misbehaving backend repeatedly inventing IDs.
        if self.devices.len() >= 64 {
            warn!("input: 64 device identities already tracked; ignoring additional device");
            return;
        }
        let instance = (0..64_u16)
            .find(|instance| {
                !self.devices.iter().any(|device| {
                    device.identity.name == name
                        && device.identity.vendor_id == vendor_id
                        && device.identity.product_id == product_id
                        && device.identity.instance == *instance
                })
            })
            .unwrap_or(63);
        self.devices.push(DeviceSnapshot {
            identity: DeviceIdentity {
                name: name.to_owned(),
                vendor_id,
                product_id,
                instance,
            },
            connected: true,
            axes: BTreeMap::new(),
            buttons: BTreeMap::new(),
            entity,
        });
    }

    fn disconnect(&mut self, entity: Entity) {
        if let Some(device) = self
            .devices
            .iter_mut()
            .find(|device| device.entity == entity)
        {
            device.connected = false;
            device.axes.clear();
            device.buttons.clear();
        }
    }

    fn connected_mut(&mut self, entity: Entity) -> Option<&mut DeviceSnapshot> {
        self.devices
            .iter_mut()
            .find(|device| device.entity == entity && device.connected)
    }

    #[must_use]
    pub fn resolve(&self, selector: &DeviceSelector) -> Option<&DeviceSnapshot> {
        match selector {
            DeviceSelector::First => self.devices.iter().find(|device| device.connected),
            DeviceSelector::Named {
                name,
                vendor_id,
                product_id,
                instance,
            } => {
                let mut matches = self.devices.iter().filter(|device| {
                    device.connected
                        && device.identity.name == *name
                        && vendor_id.is_none_or(|id| device.identity.vendor_id == Some(id))
                        && product_id.is_none_or(|id| device.identity.product_id == Some(id))
                        && device.identity.instance == *instance
                });
                let first = matches.next()?;
                // Same display names from different models are possible. An
                // underspecified selector must not silently choose the wrong one.
                if matches.next().is_some() {
                    None
                } else {
                    Some(first)
                }
            }
        }
    }

    fn analog(&self, device: &DeviceSelector, input: AnalogInput) -> Option<f64> {
        let device = self.resolve(device)?;
        match input {
            AnalogInput::Axis(axis) => device.axes.get(&axis).copied(),
            AnalogInput::Button(button) => device.buttons.get(&button).copied(),
        }
        .filter(|value| value.is_finite())
    }

    fn button(&self, source: &ButtonSource) -> Option<f64> {
        // An unseen digital button is safely released; unlike an unseen lever,
        // it cannot imply a nonzero absolute position.
        let device = self.resolve(&source.device)?;
        Some(
            device
                .buttons
                .get(&source.button)
                .copied()
                .filter(|value| value.is_finite())
                .unwrap_or(0.0)
                .clamp(0.0, 1.0),
        )
    }
}

impl From<GamepadAxis> for AxisId {
    fn from(axis: GamepadAxis) -> Self {
        match axis {
            GamepadAxis::LeftStickX => Self::LeftStickX,
            GamepadAxis::LeftStickY => Self::LeftStickY,
            GamepadAxis::LeftZ => Self::LeftZ,
            GamepadAxis::RightStickX => Self::RightStickX,
            GamepadAxis::RightStickY => Self::RightStickY,
            GamepadAxis::RightZ => Self::RightZ,
            GamepadAxis::Other(index) => Self::Other(index),
        }
    }
}

impl From<GamepadButton> for ButtonId {
    fn from(button: GamepadButton) -> Self {
        match button {
            GamepadButton::South => Self::South,
            GamepadButton::East => Self::East,
            GamepadButton::North => Self::North,
            GamepadButton::West => Self::West,
            GamepadButton::C => Self::C,
            GamepadButton::Z => Self::Z,
            GamepadButton::LeftTrigger => Self::LeftTrigger,
            GamepadButton::LeftTrigger2 => Self::LeftTrigger2,
            GamepadButton::RightTrigger => Self::RightTrigger,
            GamepadButton::RightTrigger2 => Self::RightTrigger2,
            GamepadButton::Select => Self::Select,
            GamepadButton::Start => Self::Start,
            GamepadButton::Mode => Self::Mode,
            GamepadButton::LeftThumb => Self::LeftThumb,
            GamepadButton::RightThumb => Self::RightThumb,
            GamepadButton::DPadUp => Self::DPadUp,
            GamepadButton::DPadDown => Self::DPadDown,
            GamepadButton::DPadLeft => Self::DPadLeft,
            GamepadButton::DPadRight => Self::DPadRight,
            GamepadButton::Other(index) => Self::Other(index),
        }
    }
}

/// Read events in order, then reconcile with actually connected ECS devices.
pub fn collect_input_devices(
    mut events: MessageReader<RawGamepadEvent>,
    mut native_events: MessageReader<NativeInputEvent>,
    gamepads: Query<(Entity, &Gamepad, Option<&Name>)>,
    mut devices: ResMut<InputDevices>,
) {
    // Register startup devices before their first raw axis sample. Gilrs emits
    // a separate startup connection message, not necessarily RawGamepadEvent.
    let mut connected: Vec<_> = gamepads.iter().collect();
    connected.sort_by_key(|(entity, _, _)| entity.to_bits());
    for device in &mut devices.devices {
        if !connected
            .iter()
            .any(|(entity, _, _)| *entity == device.entity)
        {
            device.connected = false;
            device.axes.clear();
            device.buttons.clear();
        }
    }
    for (entity, gamepad, name) in &connected {
        devices.connect(
            *entity,
            name.map_or("Unnamed controller", Name::as_str),
            gamepad.vendor_id(),
            gamepad.product_id(),
        );
    }
    for event in events.read() {
        match event {
            RawGamepadEvent::Connection(event) => match &event.connection {
                GamepadConnection::Connected {
                    name,
                    vendor_id,
                    product_id,
                } => {
                    devices.connect(event.gamepad, name, *vendor_id, *product_id);
                }
                GamepadConnection::Disconnected => devices.disconnect(event.gamepad),
            },
            RawGamepadEvent::Axis(event) => {
                if let Some(device) = devices.connected_mut(event.gamepad) {
                    device
                        .axes
                        .insert(event.axis.into(), f64::from(event.value));
                }
            }
            RawGamepadEvent::Button(event) => {
                if let Some(device) = devices.connected_mut(event.gamepad) {
                    device
                        .buttons
                        .insert(event.button.into(), f64::from(event.value));
                }
            }
        }
    }
    for event in native_events.read() {
        devices.apply_native_event(event);
    }
    // Also handles a plugin added after connection, or direct component removal.
    for device in &mut devices.devices {
        if !connected
            .iter()
            .any(|(entity, _, _)| *entity == device.entity)
        {
            device.connected = false;
            device.axes.clear();
            device.buttons.clear();
        }
    }
}

/// Resolved one-frame input for a single flight-control action.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MappedControl {
    pub value: f64,
    pub mode: BindingMode,
    /// Centered sticks/buttons yield to keyboard controls while untouched.
    /// Absolute lever endpoints, including zero, still count as active.
    pub active: bool,
}

/// Apply calibration and the user curve once, using raw backend values.
#[must_use]
pub fn evaluate_binding(
    binding: &ControlBinding,
    devices: &InputDevices,
    unipolar: bool,
) -> Option<MappedControl> {
    let mut curve = binding.curve;
    // Configuration files are validated on load. Public in-memory settings can
    // still be changed by callers; invalid values must never poison flight.
    if !curve.deadzone.is_finite()
        || !(0.0..1.0).contains(&curve.deadzone)
        || !curve.response.is_finite()
        || curve.response <= 0.0
    {
        return None;
    }
    match &binding.source {
        BindingSource::Axis {
            device,
            input,
            calibration,
        } => {
            if calibration.validate().is_err() {
                return None;
            }
            let raw = devices.analog(device, *input)?;
            let is_lever = unipolar && binding.mode == BindingMode::Absolute;
            let value = if is_lever {
                let mut value = calibration.unipolar(raw);
                if curve.invert {
                    value = 1.0 - value;
                }
                curve.invert = false;
                curve.apply(value)
            } else {
                curve.apply(calibration.centered(raw))
            };
            Some(MappedControl {
                value,
                mode: binding.mode,
                active: is_lever || value.abs() > 0.0,
            })
        }
        BindingSource::Buttons { positive, negative } => {
            let positive = devices.button(positive);
            let negative = negative.as_ref().and_then(|button| devices.button(button));
            if positive.is_none() && negative.is_none() {
                return None;
            }
            let invert = curve.invert;
            curve.invert = false;
            let positive = curve.apply(positive.unwrap_or(0.0));
            let negative = curve.apply(negative.unwrap_or(0.0));
            let value = (positive - negative) * if invert { -1.0 } else { 1.0 };
            Some(MappedControl {
                value: if unipolar && binding.mode == BindingMode::Absolute {
                    value.max(0.0)
                } else {
                    value
                },
                mode: binding.mode,
                active: (unipolar && binding.mode == BindingMode::Absolute)
                    || positive > 0.0
                    || negative > 0.0,
            })
        }
    }
}

/// Toggle a remappable controller shortcut on a rising edge. Keyboard C always
/// remains available; set `view_cycle` to null to use every button for flight.
pub fn cycle_mapped_view_mode(
    keyboard: Res<ButtonInput<KeyCode>>,
    settings: Res<InputSettings>,
    devices: Res<InputDevices>,
    mut mode: ResMut<ViewMode>,
    mut was_pressed: Local<bool>,
) {
    let pressed = settings
        .configuration
        .view_cycle
        .as_ref()
        .and_then(|source| devices.button(source))
        .is_some_and(|value| value >= 0.5);
    if keyboard.just_pressed(KeyCode::KeyC) || (pressed && !*was_pressed) {
        *mode = mode.next();
    }
    *was_pressed = pressed;
}

/// App-facing text and visibility. UI receives a copy across the app boundary.
#[derive(Resource, Debug, Clone, Default)]
pub struct InputDiagnostics {
    pub visible: bool,
    pub text: String,
}

/// Per-system display refresh state, independent of the device sample rate.
#[derive(Debug, Default)]
pub struct DiagnosticsRefresh {
    page: usize,
    elapsed: f32,
}

/// Update diagnostics at 10 Hz; F10 toggles, F11 advances through every device
/// and channel page. The simulation's flight keys are not captured by the panel.
pub fn update_input_diagnostics(
    keyboard: Res<ButtonInput<KeyCode>>,
    devices: Res<InputDevices>,
    settings: Res<InputSettings>,
    controls: Res<PilotControls>,
    time: Res<Time>,
    mut diagnostics: ResMut<InputDiagnostics>,
    mut refresh: Local<DiagnosticsRefresh>,
) {
    let toggled = keyboard.just_pressed(KeyCode::F10);
    let advanced = diagnostics.visible && keyboard.just_pressed(KeyCode::F11);
    if toggled {
        diagnostics.visible = !diagnostics.visible;
    }
    if advanced {
        refresh.page = refresh.page.wrapping_add(1);
    }
    refresh.elapsed += time.delta_secs();
    if diagnostics.visible
        && (toggled || advanced || refresh.elapsed >= 0.1 || diagnostics.text.is_empty())
    {
        diagnostics.text =
            format_input_diagnostics(&devices, &settings.configuration, &controls, refresh.page);
        refresh.elapsed = 0.0;
    }
}

fn ascii_name(name: &str) -> String {
    name.chars()
        .take(64)
        .map(|character| {
            if character.is_ascii() && !character.is_control() {
                character
            } else {
                '?'
            }
        })
        .collect()
}

fn channel_label<T: serde::Serialize>(channel: &T) -> String {
    serde_json::to_string(channel)
        .unwrap_or_else(|_| "unknown".into())
        .replace('"', "")
}

/// Format one bounded, ASCII-only diagnostic page. All channels and all devices
/// are reachable, including nonstandard `other` axes/buttons. Values here are
/// raw, not filtered by the configured deadzone or sensitivity curve.
#[must_use]
pub fn format_input_diagnostics(
    devices: &InputDevices,
    configuration: &InputConfiguration,
    controls: &PilotControls,
    page: usize,
) -> String {
    const CHANNELS_PER_PAGE: usize = 14;
    let mut pages = Vec::new();
    for device in &devices.devices {
        let mut channels = Vec::new();
        let mut axes: BTreeMap<_, Option<f64>> = GamepadAxis::all()
            .map(|axis| (AxisId::from(axis), None))
            .into_iter()
            .collect();
        for (axis, value) in &device.axes {
            axes.insert(*axis, Some(*value));
        }
        let mut buttons: BTreeMap<_, Option<f64>> = GamepadButton::all()
            .map(|button| (ButtonId::from(button), None))
            .into_iter()
            .collect();
        for (button, value) in &device.buttons {
            buttons.insert(*button, Some(*value));
        }
        for (axis, value) in axes {
            channels.push((format!("axis {}", channel_label(&axis)), value));
        }
        for (button, value) in buttons {
            channels.push((format!("button {}", channel_label(&button)), value));
        }
        for chunk in channels.chunks(CHANNELS_PER_PAGE) {
            pages.push((device, chunk.to_vec()));
        }
    }
    let connected = devices
        .devices
        .iter()
        .filter(|device| device.connected)
        .count();
    let mut text = format!(
        "INPUT DIAGNOSTICS  F10 hide / F11 next\n{connected} connected / {} known  page {}/{}\n",
        devices.devices.len(),
        if pages.is_empty() {
            1
        } else {
            page % pages.len() + 1
        },
        pages.len().max(1)
    );
    let backend = if devices.backend_label.is_empty() {
        "stock Bevy/gilrs"
    } else {
        devices.backend_label
    };
    let _ = writeln!(text, "backend: {backend}");
    if !pages.is_empty() {
        let (device, channels) = &pages[page % pages.len()];
        let id = &device.identity;
        let _ = writeln!(
            text,
            "{} [{}] instance {}",
            ascii_name(&id.name),
            if device.connected {
                "connected"
            } else {
                "DISCONNECTED"
            },
            id.instance
        );
        let _ = writeln!(
            text,
            "vendor {:?} product {:?}",
            id.vendor_id, id.product_id
        );
        for (channel, value) in channels {
            match value {
                Some(value) if value.is_finite() => {
                    let _ = writeln!(text, "{channel}: {value:+.4}");
                }
                Some(_) => {
                    let _ = writeln!(text, "{channel}: INVALID");
                }
                None => {
                    let _ = writeln!(text, "{channel}: --");
                }
            }
        }
    } else {
        text.push_str("No controllers connected. Keyboard remains active.\n");
    }
    text.push_str("-- = no sample yet; move each control\n");
    let output = controls.to_control_inputs();
    let _ = writeln!(
        text,
        "OUT pitch+trim {:+.2} roll {:+.2} yaw {:+.2}",
        output.elevator(),
        output.aileron(),
        output.rudder()
    );
    let _ = writeln!(
        text,
        "throttle {:.2} brake {:.2} flaps {:.2}",
        output.throttle(),
        output.brakes(),
        output.flaps()
    );
    for (name, binding) in configuration.bindings() {
        let status = binding.map_or("keyboard", |binding| {
            if evaluate_binding(
                binding,
                devices,
                matches!(name, "throttle" | "brake" | "flaps"),
            )
            .is_some()
            {
                "ready"
            } else {
                "waiting for device/sample"
            }
        });
        let _ = writeln!(text, "{name}: {status}");
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::configuration::AxisCalibration;
    use crate::{AxisCurve, PilotKeys};
    use bevy::input::InputPlugin;
    use bevy::input::gamepad::{
        GamepadConnectionEvent, RawGamepadAxisChangedEvent, RawGamepadButtonChangedEvent,
    };
    use flightsim_core::Seconds;

    fn fixture() -> InputDevices {
        let mut devices = InputDevices::default();
        devices.connect(Entity::from_bits(1), "Stick", Some(1), Some(2));
        devices.connect(Entity::from_bits(2), "Throttle", Some(3), Some(4));
        devices
    }

    fn ecs_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, InputPlugin, crate::FlightsimInputPlugin))
            .add_systems(Update, advance_sample_for_test.after(InputSystems::Sample));
        app
    }

    fn advance_sample_for_test(
        sample: Res<crate::SampledPilotInput>,
        mut controls: ResMut<PilotControls>,
    ) {
        // The application now owns advancement inside the simulation loop.
        controls.update_from_sample(Seconds(1.0 / 120.0), &sample);
    }

    fn connection(app: &mut App, entity: Entity, connection: GamepadConnection) {
        let event = GamepadConnectionEvent::new(entity, connection);
        app.world_mut()
            .resource_mut::<Messages<GamepadConnectionEvent>>()
            .write(event.clone());
        app.world_mut()
            .resource_mut::<Messages<RawGamepadEvent>>()
            .write(event.into());
    }

    fn axis_event(app: &mut App, entity: Entity, axis: GamepadAxis, value: f32) {
        app.world_mut()
            .resource_mut::<Messages<RawGamepadEvent>>()
            .write(RawGamepadAxisChangedEvent::new(entity, axis, value).into());
    }

    #[test]
    fn ecs_sampling_preserves_values_inside_bevys_builtin_deadzone() {
        let mut app = ecs_app();
        let entity = app.world_mut().spawn_empty().id();
        connection(
            &mut app,
            entity,
            GamepadConnection::Connected {
                name: "Stick".into(),
                vendor_id: Some(1),
                product_id: Some(2),
            },
        );
        axis_event(&mut app, entity, GamepadAxis::LeftStickX, 0.025);
        app.world_mut()
            .resource_mut::<InputSettings>()
            .configuration
            .roll = Some(lever("Stick", AxisId::LeftStickX));
        app.update();
        let raw = app.world().resource::<InputDevices>().devices()[0].axes[&AxisId::LeftStickX];
        assert!((raw - 0.025).abs() < 1e-8);
        assert!((app.world().resource::<PilotControls>().aileron.value() - 0.025).abs() < 1e-8);
    }

    #[test]
    fn native_event_codes_reach_controls_through_the_real_ecs_sampling_path() {
        let mut app = ecs_app();
        let entity = app.world_mut().spawn_empty().id();
        connection(
            &mut app,
            entity,
            GamepadConnection::Connected {
                name: "Stick".into(),
                vendor_id: Some(1),
                product_id: Some(2),
            },
        );
        app.world_mut()
            .resource_mut::<InputSettings>()
            .configuration
            .roll = Some(lever("Stick", AxisId::Native(0x1234_abcd)));
        app.world_mut()
            .resource_mut::<Messages<NativeInputEvent>>()
            .write(NativeInputEvent::Sample {
                entity,
                input: AnalogInput::Axis(AxisId::Native(0x1234_abcd)),
                value: 0.375,
            });
        app.update();
        assert!((app.world().resource::<PilotControls>().aileron.value() - 0.375).abs() < 1e-12);
        assert!(
            (app.world().resource::<InputDevices>().devices()[0].axes
                [&AxisId::Native(0x1234_abcd)]
                - 0.375)
                .abs()
                < 1e-12
        );
    }

    #[test]
    fn same_frame_native_reconnect_cannot_replay_pre_disconnect_samples() {
        let mut devices = fixture();
        devices.devices[0].axes.insert(AxisId::LeftStickX, 0.25);
        let entity = Entity::from_bits(1);
        devices.apply_native_event(&NativeInputEvent::Sample {
            entity,
            input: AnalogInput::Axis(AxisId::Native(99)),
            value: 1.0,
        });
        devices.apply_native_event(&NativeInputEvent::Reset { entity });
        devices.apply_native_event(&NativeInputEvent::Reset { entity });
        assert!(!devices.devices[0].axes.contains_key(&AxisId::Native(99)));
        // Resetting native samples does not erase separately processed standard aliases.
        assert!((devices.devices[0].axes[&AxisId::LeftStickX] - 0.25).abs() < 1e-12);
        devices.apply_native_event(&NativeInputEvent::Sample {
            entity,
            input: AnalogInput::Axis(AxisId::Native(99)),
            value: -0.125,
        });
        assert!((devices.devices[0].axes[&AxisId::Native(99)] + 0.125).abs() < 1e-12);
        devices.disconnect(entity);
        devices.apply_native_event(&NativeInputEvent::Sample {
            entity,
            input: AnalogInput::Axis(AxisId::Native(99)),
            value: 1.0,
        });
        assert!(devices.devices[0].axes.is_empty());
    }

    #[test]
    fn ecs_hotplug_discards_stale_samples_and_recovers_without_panicking() {
        let mut app = ecs_app();
        app.world_mut()
            .resource_mut::<InputSettings>()
            .configuration
            .roll = Some(lever("Stick", AxisId::Other(5)));
        let entity = app.world_mut().spawn_empty().id();
        let connected = GamepadConnection::Connected {
            name: "Stick".into(),
            vendor_id: None,
            product_id: None,
        };
        connection(&mut app, entity, connected.clone());
        axis_event(&mut app, entity, GamepadAxis::Other(5), 0.75);
        app.world_mut()
            .resource_mut::<Messages<RawGamepadEvent>>()
            .write(RawGamepadButtonChangedEvent::new(entity, GamepadButton::South, 1.0).into());
        app.update();
        assert!(
            (app.world()
                .resource::<PilotControls>()
                .to_control_inputs()
                .brakes()
                - 1.0)
                .abs()
                < 1e-12
        );

        connection(&mut app, entity, GamepadConnection::Disconnected);
        axis_event(&mut app, entity, GamepadAxis::Other(5), -0.75);
        app.update();
        let device = &app.world().resource::<InputDevices>().devices()[0];
        assert!(!device.connected);
        assert!(device.axes.is_empty() && device.buttons.is_empty());
        assert!(
            app.world()
                .resource::<PilotControls>()
                .to_control_inputs()
                .brakes()
                .abs()
                < 1e-12
        );

        connection(&mut app, entity, connected);
        app.update();
        assert!(
            app.world().resource::<InputDevices>().devices()[0]
                .axes
                .is_empty()
        );
        axis_event(&mut app, entity, GamepadAxis::Other(5), 0.125);
        app.update();
        assert!((app.world().resource::<PilotControls>().aileron.value() - 0.125).abs() < 1e-12);
        assert!(
            (app.world().resource::<InputDevices>().devices()[0].axes[&AxisId::Other(5)] - 0.125)
                .abs()
                < 1e-12
        );
    }

    #[test]
    fn a_named_selector_does_not_guess_between_different_models_with_the_same_name() {
        let mut devices = fixture();
        devices.connect(Entity::from_bits(3), "Stick", Some(9), Some(10));
        assert!(devices.resolve(&named("Stick")).is_none());
        let exact = DeviceSelector::Named {
            name: "Stick".into(),
            vendor_id: Some(9),
            product_id: Some(10),
            instance: 0,
        };
        assert_eq!(
            devices.resolve(&exact).expect("exact match").entity,
            Entity::from_bits(3)
        );
    }

    fn named(name: &str) -> DeviceSelector {
        DeviceSelector::Named {
            name: name.into(),
            vendor_id: None,
            product_id: None,
            instance: 0,
        }
    }

    fn lever(name: &str, axis: AxisId) -> ControlBinding {
        ControlBinding {
            source: BindingSource::Axis {
                device: named(name),
                input: AnalogInput::Axis(axis),
                calibration: AxisCalibration::default(),
            },
            mode: BindingMode::Absolute,
            curve: AxisCurve::new(0.0, 1.0, false),
        }
    }

    #[test]
    fn separate_devices_drive_independent_actions_in_the_same_frame() {
        let mut devices = fixture();
        devices.devices[0].axes.insert(AxisId::Other(7), -0.75);
        devices.devices[1].axes.insert(AxisId::Other(19), 0.5);
        let configuration = InputConfiguration {
            pitch: Some(lever("Stick", AxisId::Other(7))),
            throttle: Some(lever("Throttle", AxisId::Other(19))),
            ..InputConfiguration::default()
        };
        let mut controls = PilotControls::default();
        controls.trim.set(0.0);
        controls.update_with_bindings(Seconds(0.1), PilotKeys::default(), &configuration, &devices);
        assert!((controls.to_control_inputs().elevator() + 0.75).abs() < 1e-12);
        assert!((controls.to_control_inputs().throttle() - 0.75).abs() < 1e-12);
    }

    #[test]
    fn absolute_throttle_idle_is_not_treated_as_untouched() {
        let mut devices = fixture();
        devices.devices[1].axes.insert(AxisId::Other(19), -1.0);
        let configuration = InputConfiguration {
            throttle: Some(lever("Throttle", AxisId::Other(19))),
            ..InputConfiguration::default()
        };
        let mut controls = PilotControls::default();
        controls.throttle.set_absolute(0.9);
        controls.update_with_bindings(Seconds(0.1), PilotKeys::default(), &configuration, &devices);
        assert!(controls.throttle.value().abs() < 1e-12);
    }

    #[test]
    fn releasing_an_absolute_throttle_button_returns_it_to_zero() {
        let mut devices = fixture();
        let configuration = InputConfiguration {
            throttle: Some(ControlBinding {
                source: BindingSource::Buttons {
                    positive: ButtonSource {
                        device: named("Throttle"),
                        button: ButtonId::Other(42),
                    },
                    negative: None,
                },
                mode: BindingMode::Absolute,
                curve: AxisCurve::default(),
            }),
            ..InputConfiguration::default()
        };
        let mut controls = PilotControls::default();
        devices.devices[1].buttons.insert(ButtonId::Other(42), 1.0);
        controls.update_with_bindings(Seconds(0.1), PilotKeys::default(), &configuration, &devices);
        assert!((controls.throttle.value() - 1.0).abs() < 1e-12);
        devices.devices[1].buttons.insert(ButtonId::Other(42), 0.0);
        controls.update_with_bindings(Seconds(0.1), PilotKeys::default(), &configuration, &devices);
        assert!(controls.throttle.value().abs() < 1e-12);
    }

    #[test]
    fn arbitrary_button_pairs_can_drive_all_six_flight_actions() {
        let mut devices = fixture();
        let button_binding = |mode| {
            Some(ControlBinding {
                source: BindingSource::Buttons {
                    positive: ButtonSource {
                        device: named("Stick"),
                        button: ButtonId::Other(42),
                    },
                    negative: Some(ButtonSource {
                        device: named("Throttle"),
                        button: ButtonId::Other(43),
                    }),
                },
                mode,
                curve: AxisCurve::default(),
            })
        };
        let configuration = InputConfiguration {
            pitch: button_binding(BindingMode::Absolute),
            roll: button_binding(BindingMode::Absolute),
            yaw: button_binding(BindingMode::Absolute),
            throttle: button_binding(BindingMode::Rate),
            brake: button_binding(BindingMode::Absolute),
            flaps: button_binding(BindingMode::Rate),
            ..InputConfiguration::default()
        };
        let mut controls = PilotControls::default();
        controls.trim.set(0.0);
        devices.devices[0].buttons.insert(ButtonId::Other(42), 1.0);
        controls.update_with_bindings(Seconds(1.0), PilotKeys::default(), &configuration, &devices);
        let output = controls.to_control_inputs();
        for value in [
            output.elevator(),
            output.aileron(),
            output.rudder(),
            output.brakes(),
        ] {
            assert!((value - 1.0).abs() < 1e-12);
        }
        assert!((output.throttle() - 0.25).abs() < 1e-12);
        assert!((output.flaps() - 0.2).abs() < 1e-12);
        devices.devices[0].buttons.insert(ButtonId::Other(42), 0.0);
        devices.devices[1].buttons.insert(ButtonId::Other(43), 1.0);
        controls.update_with_bindings(Seconds(1.0), PilotKeys::default(), &configuration, &devices);
        let output = controls.to_control_inputs();
        for value in [output.elevator(), output.aileron(), output.rudder()] {
            assert!((value + 1.0).abs() < 1e-12);
        }
        for value in [output.throttle(), output.flaps(), output.brakes()] {
            assert!(value.abs() < 1e-12);
        }
    }

    #[test]
    fn inverted_lever_swaps_endpoints_without_negative_throttle() {
        let mut devices = fixture();
        let mut binding = lever("Throttle", AxisId::Other(19));
        binding.curve.invert = true;
        for (raw, expected) in [(-1.0, 1.0), (0.0, 0.5), (1.0, 0.0)] {
            devices.devices[1].axes.insert(AxisId::Other(19), raw);
            let sample = evaluate_binding(&binding, &devices, true).expect("available");
            assert!((sample.value - expected).abs() < 1e-12);
            assert!(sample.active);
        }
    }

    #[test]
    fn unobserved_analog_position_does_not_create_half_throttle() {
        let devices = fixture();
        assert!(evaluate_binding(&lever("Throttle", AxisId::LeftZ), &devices, true).is_none());
    }

    #[test]
    fn disconnect_clears_stale_values_and_reconnect_requires_fresh_sample() {
        let mut devices = fixture();
        devices.devices[0].axes.insert(AxisId::LeftStickX, 0.8);
        let binding = lever("Stick", AxisId::LeftStickX);
        assert!(evaluate_binding(&binding, &devices, false).is_some());
        devices.disconnect(Entity::from_bits(1));
        assert!(evaluate_binding(&binding, &devices, false).is_none());
        devices.connect(Entity::from_bits(1), "Stick", Some(1), Some(2));
        assert!(evaluate_binding(&binding, &devices, false).is_none());
        devices.devices[0].axes.insert(AxisId::LeftStickX, -0.4);
        assert!(
            (evaluate_binding(&binding, &devices, false)
                .expect("fresh")
                .value
                + 0.4)
                .abs()
                < 1e-12
        );
        assert_eq!(devices.devices.len(), 2);
    }

    #[test]
    fn unplugging_an_identical_device_never_renumbers_the_other() {
        let mut devices = fixture();
        devices.connect(Entity::from_bits(3), "Stick", Some(1), Some(2));
        devices.disconnect(Entity::from_bits(1));
        assert!(devices.resolve(&named("Stick")).is_none());
        let second = DeviceSelector::Named {
            name: "Stick".into(),
            vendor_id: Some(1),
            product_id: Some(2),
            instance: 1,
        };
        assert_eq!(
            devices.resolve(&second).expect("second unit").entity,
            Entity::from_bits(3)
        );
    }

    #[test]
    fn reconnect_with_a_new_backend_entity_reuses_the_vacant_model_slot() {
        let mut devices = fixture();
        devices.devices[0].axes.insert(AxisId::LeftStickX, 0.8);
        devices.disconnect(Entity::from_bits(1));
        devices.connect(Entity::from_bits(9), "Stick", Some(1), Some(2));
        let device = devices.resolve(&named("Stick")).expect("reconnected model");
        assert_eq!(device.identity.instance, 0);
        assert_eq!(device.entity, Entity::from_bits(9));
        assert!(device.axes.is_empty());
        assert_eq!(devices.devices.len(), 2);
    }

    #[test]
    fn sampled_controller_state_is_immutable_until_explicitly_resampled() {
        let mut devices = fixture();
        devices.devices[0].axes.insert(AxisId::Other(7), -0.75);
        devices.devices[1].axes.insert(AxisId::Other(19), 0.5);
        let configuration = InputConfiguration {
            pitch: Some(lever("Stick", AxisId::Other(7))),
            throttle: Some(lever("Throttle", AxisId::Other(19))),
            ..InputConfiguration::default()
        };
        let sample =
            crate::SampledPilotInput::from_bindings(PilotKeys::default(), &configuration, &devices);
        devices.devices[0].axes.insert(AxisId::Other(7), 0.25);
        devices.devices[1].axes.insert(AxisId::Other(19), -1.0);
        let mut controls = PilotControls::default();
        for _ in 0..30 {
            controls.update_from_sample(Seconds(1.0 / 120.0), &sample);
            assert!((controls.elevator.value() + 0.75).abs() < 1e-12);
            assert!((controls.throttle.value() - 0.75).abs() < 1e-12);
        }
        controls.release_transient_controls();
        assert!(controls.elevator.value().abs() < 1e-12);
        let resumed =
            crate::SampledPilotInput::from_bindings(PilotKeys::default(), &configuration, &devices);
        controls.update_from_sample(Seconds(1.0 / 120.0), &resumed);
        assert!((controls.elevator.value() - 0.25).abs() < 1e-12);
        assert!(controls.throttle.value().abs() < 1e-12);
        devices.disconnect(Entity::from_bits(1));
        devices.disconnect(Entity::from_bits(2));
        let disconnected = crate::SampledPilotInput::from_bindings(
            PilotKeys {
                pitch_down: true,
                throttle_up: true,
                ..PilotKeys::default()
            },
            &configuration,
            &devices,
        );
        controls.release_transient_controls();
        controls.update_from_sample(Seconds(0.1), &disconnected);
        assert!((controls.elevator.value() + 0.25).abs() < 1e-12);
        assert!((controls.throttle.value() - 0.025).abs() < 1e-12);
    }

    #[test]
    fn keyboard_recovers_after_disconnect_and_brakes_release() {
        let mut devices = fixture();
        devices.devices[0].axes.insert(AxisId::LeftStickX, 1.0);
        devices.devices[0].buttons.insert(ButtonId::South, 1.0);
        let configuration = InputConfiguration::default();
        let mut controls = PilotControls::default();
        controls.update_with_bindings(Seconds(0.1), PilotKeys::default(), &configuration, &devices);
        assert!((controls.to_control_inputs().brakes() - 1.0).abs() < 1e-12);
        devices.disconnect(Entity::from_bits(1));
        for _ in 0..20 {
            controls.update_with_bindings(
                Seconds(0.1),
                PilotKeys {
                    roll_left: true,
                    ..PilotKeys::default()
                },
                &configuration,
                &devices,
            );
        }
        assert!((controls.aileron.value() + 1.0).abs() < 1e-12);
        assert!(controls.to_control_inputs().brakes().abs() < 1e-12);
    }

    #[test]
    fn raw_values_and_nonstandard_channels_are_visible_and_ascii() {
        let mut devices = fixture();
        devices.devices[0].identity.name = "Stick 日本語".into();
        devices.devices[0].axes.insert(AxisId::Other(17), 0.03125);
        devices.devices[0].buttons.insert(ButtonId::Other(42), 0.8);
        let pages: String = (0..8)
            .map(|page| {
                format_input_diagnostics(
                    &devices,
                    &InputConfiguration::default(),
                    &PilotControls::default(),
                    page,
                )
            })
            .collect();
        assert!(pages.is_ascii());
        assert!(pages.contains("+0.0312"));
        assert!(pages.contains("other:17"));
        assert!(pages.contains("other:42"));
    }
}
