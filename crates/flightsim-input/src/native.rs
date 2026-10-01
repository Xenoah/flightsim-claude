//! Optional native-channel gilrs adapter for HOTAS hardware.
//!
//! Bevy 0.18.1's stock adapter drops `Axis::Unknown` and `Button::Unknown`.
//! This adapter preserves every native event code in addition to forwarding
//! standard gamepad aliases. Native codes are platform-specific, so a saved
//! Windows native-code mapping must not be assumed to work on Linux.
//!
//! Opt in by disabling `bevy::gilrs::GilrsPlugin` and adding
//! [`NativeControllersPlugin`]. The normal backend remains the default.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

use bevy::input::gamepad::{
    GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent,
    RawGamepadButtonChangedEvent, RawGamepadEvent,
};
use bevy::prelude::*;

use crate::InputDevices;
use crate::configuration::{AnalogInput, AxisId, ButtonId};

/// A channel that the stock Bevy adapter may not expose. Codes are losslessly
/// stored as u32, never assigned in movement/discovery order or truncated to u8.
#[derive(Message, Debug, Clone, Copy)]
pub enum NativeInputEvent {
    Sample {
        entity: Entity,
        input: AnalogInput,
        value: f64,
    },
    /// Preserve connection/sample ordering even when a disconnect and reconnect
    /// occur in the same frame. Only native-channel samples are reset.
    Reset { entity: Entity },
}

#[derive(Resource)]
struct NativeBackend(Mutex<gilrs::Gilrs>);

impl fmt::Debug for NativeBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeBackend")
            .finish_non_exhaustive()
    }
}

#[derive(Resource, Debug, Default)]
struct NativeGamepads(HashMap<gilrs::GamepadId, Entity>);

/// Initialization status for diagnostics. This reports software initialization,
/// not a claim that a particular physical controller has been tested.
#[derive(Resource, Debug, Clone)]
pub struct NativeControllerStatus {
    pub initialized: bool,
    pub message: String,
}

/// Opt-in replacement for Bevy's gilrs adapter; never runs alongside it.
#[derive(Debug, Default)]
pub struct NativeControllersPlugin;

impl Plugin for NativeControllersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InputDevices>();
        if app.is_plugin_added::<bevy::gilrs::GilrsPlugin>() {
            let message = "native input disabled: Bevy GilrsPlugin must be disabled first";
            error!("{message}");
            app.world_mut()
                .resource_mut::<InputDevices>()
                .set_backend_label("stock (native refused: duplicate backend)");
            app.insert_resource(NativeControllerStatus {
                initialized: false,
                message: message.into(),
            });
            return;
        }
        match gilrs::GilrsBuilder::new()
            .with_default_filters(false)
            .set_update_state(false)
            .build()
        {
            Ok(gilrs) => {
                app.world_mut()
                    .resource_mut::<InputDevices>()
                    .set_backend_label("native gilrs (opt-in)");
                app.insert_resource(NativeBackend(Mutex::new(gilrs)))
                    .init_resource::<NativeGamepads>()
                    .add_message::<NativeInputEvent>()
                    .insert_resource(NativeControllerStatus {
                        initialized: true,
                        message: "native gilrs initialized; hardware verification still required"
                            .into(),
                    })
                    .add_systems(PreStartup, connect_existing)
                    .add_systems(PreUpdate, poll_native.before(bevy::input::InputSystems));
            }
            Err(error) => {
                let message = format!("native input unavailable: {error}; keyboard remains active");
                warn!("{message}");
                app.world_mut()
                    .resource_mut::<InputDevices>()
                    .set_backend_label("native unavailable; keyboard active");
                app.insert_resource(NativeControllerStatus {
                    initialized: false,
                    message,
                });
            }
        }
    }
}

fn entity_for(
    id: gilrs::GamepadId,
    commands: &mut Commands,
    gamepads: &mut NativeGamepads,
) -> Entity {
    *gamepads
        .0
        .entry(id)
        .or_insert_with(|| commands.spawn_empty().id())
}

fn connected_event(entity: Entity, gamepad: gilrs::Gamepad<'_>) -> GamepadConnectionEvent {
    GamepadConnectionEvent::new(
        entity,
        GamepadConnection::Connected {
            name: gamepad.name().into(),
            vendor_id: gamepad.vendor_id(),
            product_id: gamepad.product_id(),
        },
    )
}

fn connect_existing(
    mut commands: Commands,
    mut backend: ResMut<NativeBackend>,
    mut gamepads: ResMut<NativeGamepads>,
    mut connections: MessageWriter<GamepadConnectionEvent>,
    mut raw: MessageWriter<RawGamepadEvent>,
    mut native: MessageWriter<NativeInputEvent>,
) {
    let Ok(gilrs) = backend.0.get_mut() else {
        return;
    };
    for (id, gamepad) in gilrs.gamepads() {
        let entity = entity_for(id, &mut commands, &mut gamepads);
        let event = connected_event(entity, gamepad);
        connections.write(event.clone());
        raw.write(event.into());
        native.write(NativeInputEvent::Reset { entity });
    }
}

fn poll_native(
    mut commands: Commands,
    mut backend: ResMut<NativeBackend>,
    mut gamepads: ResMut<NativeGamepads>,
    mut connections: MessageWriter<GamepadConnectionEvent>,
    mut raw: MessageWriter<RawGamepadEvent>,
    mut native: MessageWriter<NativeInputEvent>,
) {
    let Ok(gilrs) = backend.0.get_mut() else {
        return;
    };
    // Bound a pathological device's frame cost; remaining events stay queued.
    for _ in 0..4096 {
        let Some(event) = gilrs.next_event() else {
            break;
        };
        gilrs.update(&event);
        if matches!(event.event, gilrs::EventType::Connected) {
            let entity = entity_for(event.id, &mut commands, &mut gamepads);
            let event = connected_event(entity, gilrs.gamepad(event.id));
            connections.write(event.clone());
            raw.write(event.into());
            native.write(NativeInputEvent::Reset { entity });
            continue;
        }
        let Some(entity) = gamepads.0.get(&event.id).copied() else {
            // A delayed event for a vanished/unannounced device is harmless.
            continue;
        };
        match event.event {
            gilrs::EventType::Disconnected => {
                let event = GamepadConnectionEvent::new(entity, GamepadConnection::Disconnected);
                connections.write(event.clone());
                raw.write(event.into());
                native.write(NativeInputEvent::Reset { entity });
            }
            gilrs::EventType::AxisChanged(axis, value, code) => {
                native.write(native_axis(entity, code.into_u32(), value));
                if let Some(axis) = standard_axis(axis) {
                    raw.write(RawGamepadAxisChangedEvent::new(entity, axis, value).into());
                } else {
                    for (button, value) in dpad_buttons(axis, value) {
                        raw.write(RawGamepadButtonChangedEvent::new(entity, button, value).into());
                    }
                }
            }
            gilrs::EventType::ButtonChanged(button, value, code) => {
                native.write(native_button(entity, code.into_u32(), value));
                if let Some(button) = standard_button(button) {
                    raw.write(RawGamepadButtonChangedEvent::new(entity, button, value).into());
                }
            }
            _ => {}
        }
    }
    gilrs.inc();
}

fn native_axis(entity: Entity, code: u32, value: f32) -> NativeInputEvent {
    NativeInputEvent::Sample {
        entity,
        input: AnalogInput::Axis(AxisId::Native(code)),
        value: f64::from(value),
    }
}

fn native_button(entity: Entity, code: u32, value: f32) -> NativeInputEvent {
    NativeInputEvent::Sample {
        entity,
        input: AnalogInput::Button(ButtonId::Native(code)),
        value: f64::from(value),
    }
}

fn standard_axis(axis: gilrs::Axis) -> Option<GamepadAxis> {
    match axis {
        gilrs::Axis::LeftStickX => Some(GamepadAxis::LeftStickX),
        gilrs::Axis::LeftStickY => Some(GamepadAxis::LeftStickY),
        gilrs::Axis::LeftZ => Some(GamepadAxis::LeftZ),
        gilrs::Axis::RightStickX => Some(GamepadAxis::RightStickX),
        gilrs::Axis::RightStickY => Some(GamepadAxis::RightStickY),
        gilrs::Axis::RightZ => Some(GamepadAxis::RightZ),
        _ => None,
    }
}

fn standard_button(button: gilrs::Button) -> Option<GamepadButton> {
    match button {
        gilrs::Button::South => Some(GamepadButton::South),
        gilrs::Button::East => Some(GamepadButton::East),
        gilrs::Button::North => Some(GamepadButton::North),
        gilrs::Button::West => Some(GamepadButton::West),
        gilrs::Button::C => Some(GamepadButton::C),
        gilrs::Button::Z => Some(GamepadButton::Z),
        gilrs::Button::LeftTrigger => Some(GamepadButton::LeftTrigger),
        gilrs::Button::LeftTrigger2 => Some(GamepadButton::LeftTrigger2),
        gilrs::Button::RightTrigger => Some(GamepadButton::RightTrigger),
        gilrs::Button::RightTrigger2 => Some(GamepadButton::RightTrigger2),
        gilrs::Button::Select => Some(GamepadButton::Select),
        gilrs::Button::Start => Some(GamepadButton::Start),
        gilrs::Button::Mode => Some(GamepadButton::Mode),
        gilrs::Button::LeftThumb => Some(GamepadButton::LeftThumb),
        gilrs::Button::RightThumb => Some(GamepadButton::RightThumb),
        gilrs::Button::DPadUp => Some(GamepadButton::DPadUp),
        gilrs::Button::DPadDown => Some(GamepadButton::DPadDown),
        gilrs::Button::DPadLeft => Some(GamepadButton::DPadLeft),
        gilrs::Button::DPadRight => Some(GamepadButton::DPadRight),
        _ => None,
    }
}

fn dpad_buttons(axis: gilrs::Axis, value: f32) -> Vec<(GamepadButton, f32)> {
    let value = if value.is_finite() {
        value.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    match axis {
        gilrs::Axis::DPadX => vec![
            (GamepadButton::DPadRight, value.max(0.0)),
            (GamepadButton::DPadLeft, (-value).max(0.0)),
        ],
        gilrs::Axis::DPadY => vec![
            (GamepadButton::DPadUp, value.max(0.0)),
            (GamepadButton::DPadDown, (-value).max(0.0)),
        ],
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_codes_are_lossless_and_do_not_collide_with_standard_or_other_channels() {
        let event = native_axis(Entity::from_bits(1), 0x1234_abcd, -0.75);
        let NativeInputEvent::Sample { input, value, .. } = event else {
            panic!("sample expected")
        };
        assert_eq!(input, AnalogInput::Axis(AxisId::Native(0x1234_abcd)));
        assert!((value + 0.75).abs() < 1e-12);
        let button = native_button(Entity::from_bits(1), 0xffff_fffe, 1.0);
        assert!(matches!(
            button,
            NativeInputEvent::Sample {
                input: AnalogInput::Button(ButtonId::Native(0xffff_fffe)),
                ..
            }
        ));
        assert_ne!(AxisId::Native(7), AxisId::Other(7));
    }

    #[test]
    fn unknown_channels_keep_native_identity_without_fabricating_gamepad_aliases() {
        assert!(standard_axis(gilrs::Axis::Unknown).is_none());
        assert!(standard_button(gilrs::Button::Unknown).is_none());
        assert!(matches!(
            native_axis(Entity::from_bits(1), 99, 0.5),
            NativeInputEvent::Sample {
                input: AnalogInput::Axis(AxisId::Native(99)),
                ..
            }
        ));
    }

    #[test]
    fn hats_release_opposite_direction_when_crossing_center() {
        assert_eq!(
            dpad_buttons(gilrs::Axis::DPadX, 1.0),
            vec![
                (GamepadButton::DPadRight, 1.0),
                (GamepadButton::DPadLeft, 0.0)
            ]
        );
        assert_eq!(
            dpad_buttons(gilrs::Axis::DPadX, -1.0),
            vec![
                (GamepadButton::DPadRight, 0.0),
                (GamepadButton::DPadLeft, 1.0)
            ]
        );
        assert!(
            dpad_buttons(gilrs::Axis::DPadY, 0.0)
                .iter()
                .all(|(_, value)| value.abs() < f32::EPSILON)
        );
    }
}
