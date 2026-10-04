//! Bounded presentation and draft editing for explicitly authored new-flight wind.
//! The application owns exact physical values, validation and their eventual Start.

use super::{
    ACCENT, MUTED, TEXT, WorldMapActions, WorldMapButton, WorldMapState, WorldMapText,
    bounded_ascii, spawn_button, spawn_coordinate_button, spawn_map_text,
};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use flightsim_core::{Degrees, Knots};

const MAX_DRAFT_BYTES: usize = 16;

/// App-formatted pending values. Numeric strings are presentation only: opening
/// or applying an untouched draft must never round-trip them into physical values.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindSettingsView {
    pub wind_from: String,
    pub wind_speed: String,
    /// Honest current/custom horizontal-component amplitude and deterministic seed.
    pub turbulence: String,
    /// Exact deterministic seed, retained when choosing a different strength.
    pub turbulence_seed: u64,
    pub note: String,
    pub error: String,
    /// Independently owned by the app; replay/LAN disable these physical controls.
    pub enabled: bool,
}

/// Authored simulation presets: horizontal-component bounds 0/1.5/3/6 m/s.
/// These are neither FAA intensity categories nor observed/METAR gust peaks.
/// An explicit choice changes intensity only, preserving the app-owned seed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldMapTurbulence {
    Calm,
    Light,
    Moderate,
    Severe,
}

impl WorldMapTurbulence {
    fn label(self) -> &'static str {
        match self {
            Self::Calm => "Calm: 0 m/s",
            Self::Light => "Light: 1.5 m/s",
            Self::Moderate => "Moderate: 3 m/s",
            Self::Severe => "Severe: 6 m/s",
        }
    }
}

/// Individually authored fields only. `None` preserves the app's exact value,
/// including a custom turbulence amplitude and seed. App revalidates all fields.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WorldMapConditionsEdit {
    pub wind_from: Option<Degrees>,
    pub wind_speed: Option<Knots>,
    pub turbulence: Option<WorldMapTurbulence>,
}

#[derive(Component, Debug)]
pub struct WorldMapWindRoot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindSettingsButton {
    WindFrom,
    WindSpeed,
    CurrentTurbulence,
    Turbulence(WorldMapTurbulence),
    Apply,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindSettingsText {
    WindFrom,
    WindSpeed,
    Turbulence,
    Note,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindField {
    From,
    Speed,
}

#[derive(Debug, Clone)]
pub(super) struct WindEditor {
    from: String,
    speed: String,
    from_dirty: bool,
    speed_dirty: bool,
    field: WindField,
    turbulence: Option<WorldMapTurbulence>,
    current_turbulence: String,
    turbulence_seed: u64,
    error: String,
}

impl WindEditor {
    fn new(view: &WindSettingsView) -> Self {
        Self {
            from: bounded_ascii(&view.wind_from, MAX_DRAFT_BYTES, 1),
            speed: bounded_ascii(&view.wind_speed, MAX_DRAFT_BYTES, 1),
            from_dirty: false,
            speed_dirty: false,
            field: WindField::From,
            turbulence: None,
            current_turbulence: bounded_ascii(&view.turbulence, 54, 2),
            turbulence_seed: view.turbulence_seed,
            error: String::new(),
        }
    }

    pub(super) fn button(&mut self, button: WindSettingsButton, actions: &mut WorldMapActions) {
        match button {
            WindSettingsButton::WindFrom => self.field = WindField::From,
            WindSettingsButton::WindSpeed => self.field = WindField::Speed,
            WindSettingsButton::CurrentTurbulence => self.turbulence = None,
            WindSettingsButton::Turbulence(preset) => self.turbulence = Some(preset),
            WindSettingsButton::Apply | WindSettingsButton::Cancel => return,
        }
        self.error.clear();
        actions.invalidate_start();
    }

    /// Ordered key handling, including Ctrl press/release within one frame.
    /// Returns true only for Enter; the caller applies once and owns the frame.
    pub(super) fn key(
        &mut self,
        event: &KeyboardInput,
        control: bool,
        actions: &mut WorldMapActions,
    ) -> bool {
        if event.state != ButtonState::Pressed {
            return false;
        }
        if matches!(event.key_code, KeyCode::Enter | KeyCode::NumpadEnter) {
            return true;
        }
        if event.key_code == KeyCode::Tab && !control {
            self.field = match self.field {
                WindField::From => WindField::Speed,
                WindField::Speed => WindField::From,
            };
            actions.invalidate_start();
            return false;
        }
        let (draft, dirty) = match self.field {
            WindField::From => (&mut self.from, &mut self.from_dirty),
            WindField::Speed => (&mut self.speed, &mut self.speed_dirty),
        };
        if control {
            if event.key_code == KeyCode::KeyA
                || matches!(&event.logical_key, Key::Character(character) if character.eq_ignore_ascii_case("a"))
            {
                draft.clear();
                *dirty = true;
                self.error.clear();
                actions.invalidate_start();
            }
            return false;
        }
        match event.key_code {
            KeyCode::Backspace | KeyCode::Delete => {
                if event.key_code == KeyCode::Backspace {
                    draft.pop();
                } else {
                    draft.clear();
                }
                *dirty = true;
                self.error.clear();
                actions.invalidate_start();
                return false;
            }
            _ => {}
        }
        let text = event.text.as_deref().or(match &event.logical_key {
            Key::Character(character) => Some(character.as_str()),
            _ => None,
        });
        if let Some(text) = text.filter(|text| !text.is_empty()) {
            *dirty = true;
            self.error.clear();
            actions.invalidate_start();
            for character in text.chars() {
                if draft.len() >= MAX_DRAFT_BYTES {
                    draft.pop();
                    draft.push('?');
                    self.error = "Input too long; clear or edit the value".into();
                    break;
                }
                draft.push(match character {
                    '0'..='9' | '+' | '-' | '.' => character,
                    '\u{2212}' | '\u{ff0d}' => '-',
                    ',' => '.',
                    _ => '?',
                });
            }
            if draft.contains('?') && self.error.is_empty() {
                self.error = "Use digits, +/- and . or ,".into();
            }
        }
        false
    }

    fn changes(&mut self) -> Option<WorldMapConditionsEdit> {
        let from = if self.from_dirty {
            let Some(value) = parse_bounded(&self.from, 360.0) else {
                self.error = "Wind FROM: enter 0 to 360 degrees true".into();
                return None;
            };
            Some(Degrees(value))
        } else {
            None
        };
        let speed = if self.speed_dirty {
            let Some(value) = parse_bounded(&self.speed, 300.0) else {
                self.error = "Wind speed: enter 0 to 300 knots".into();
                return None;
            };
            Some(Knots(value))
        } else {
            None
        };
        Some(WorldMapConditionsEdit {
            wind_from: from,
            wind_speed: speed,
            turbulence: self.turbulence,
        })
    }
}

fn parse_bounded(text: &str, maximum: f64) -> Option<f64> {
    text.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && (0.0..=maximum).contains(value))
}

impl WorldMapState {
    /// Opens a draft of app-fed conditions, without touching exact app values.
    pub fn show_wind_settings(&mut self) -> bool {
        if !self.wind_settings.enabled || !self.new_flight_modal_ready() {
            return false;
        }
        self.wind_editor = Some(WindEditor::new(&self.wind_settings));
        self.invalidate_start_pending = true;
        true
    }

    pub(super) fn apply_wind_settings(&mut self, actions: &mut WorldMapActions) {
        actions.invalidate_start();
        if !self.wind_settings.enabled {
            self.wind_editor = None;
            return;
        }
        if let Some(editor) = &mut self.wind_editor
            && let Some(changes) = editor.changes()
        {
            actions.conditions = Some(changes);
            self.wind_editor = None;
        }
    }
}

pub(super) fn spawn_wind_settings(root: &mut ChildSpawnerCommands) {
    root.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0.0), top: px(0.0), width: percent(100.0), height: percent(100.0),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.015, 0.025, 0.04, 0.98)),
        GlobalZIndex(112), FocusPolicy::Block, Visibility::Hidden,
        WorldMapWindRoot,
    )).with_children(|overlay| {
        overlay.spawn((Node {
            width: px(760.0), max_width: percent(94.0),
            height: px(520.0), max_height: percent(94.0),
            padding: UiRect::all(px(22.0)), flex_direction: FlexDirection::Column,
            row_gap: px(12.0), overflow: Overflow::clip(), ..default()
        }, BackgroundColor(Color::srgb(0.045, 0.073, 0.103)))).with_children(|panel| {
            panel.spawn((Text::new("NEW FLIGHT: WIND / TURBULENCE"), TextFont { font_size: 22.0, ..default() }, TextColor(TEXT)));
            panel.spawn((Text::new("Authored conditions; applied only when you Start a new flight."), TextFont { font_size: 13.0, ..default() }, TextColor(MUTED)));
            spawn_coordinate_button(panel, WorldMapButton::Wind(WindSettingsButton::WindFrom), WorldMapText::Wind(WindSettingsText::WindFrom));
            spawn_coordinate_button(panel, WorldMapButton::Wind(WindSettingsButton::WindSpeed), WorldMapText::Wind(WindSettingsText::WindSpeed));
            panel.spawn((Text::new("Click a field or Tab; type a value; Ctrl+A clears. Wind comes FROM the bearing."), TextFont { font_size: 12.0, ..default() }, TextColor(MUTED)));
            spawn_map_text(panel, WorldMapText::Wind(WindSettingsText::Turbulence), 13.0, ACCENT);
            panel.spawn(Node { width: percent(100.0), column_gap: px(6.0), ..default() }).with_children(|presets| {
                for (label, choice, width) in [
                    ("Keep current/custom", WindSettingsButton::CurrentTurbulence, 158.0),
                    ("Calm", WindSettingsButton::Turbulence(WorldMapTurbulence::Calm), 86.0),
                    ("Light", WindSettingsButton::Turbulence(WorldMapTurbulence::Light), 86.0),
                    ("Moderate", WindSettingsButton::Turbulence(WorldMapTurbulence::Moderate), 100.0),
                    ("Severe", WindSettingsButton::Turbulence(WorldMapTurbulence::Severe), 86.0),
                ] {
                    spawn_button(presets, label, WorldMapButton::Wind(choice), px(width));
                }
            });
            panel.spawn((Text::new("Horizontal-component bounds: 0 / 1.5 / 3 / 6 m/s. Current seed is preserved.\nAuthored levels, not FAA intensity categories or METAR gust peaks.\nWind speed 0-300 kt is an input limit, not an aircraft operating limit."), TextFont { font_size: 12.0, ..default() }, TextColor(MUTED)));
            spawn_map_text(panel, WorldMapText::Wind(WindSettingsText::Note), 12.0, TEXT);
            spawn_map_text(panel, WorldMapText::Wind(WindSettingsText::Error), 12.0, Color::srgb(1.0, 0.73, 0.34));
            panel.spawn(Node { flex_grow: 1.0, min_height: px(0.0), ..default() });
            panel.spawn(Node { width: percent(100.0), column_gap: px(12.0), justify_content: JustifyContent::End, flex_shrink: 0.0, ..default() }).with_children(|footer| {
                spawn_button(footer, "Cancel [Esc]", WorldMapButton::Wind(WindSettingsButton::Cancel), px(142.0));
                spawn_button(footer, "Apply [Enter]", WorldMapButton::Wind(WindSettingsButton::Apply), px(142.0));
            });
        });
    });
}

pub(super) fn format_text(kind: WindSettingsText, state: &WorldMapState) -> String {
    let Some(editor) = &state.wind_editor else {
        return String::new();
    };
    match kind {
        WindSettingsText::WindFrom | WindSettingsText::WindSpeed => {
            let (label, value, focused) = if kind == WindSettingsText::WindFrom {
                (
                    "Wind FROM (degrees true, 0-360)",
                    &editor.from,
                    editor.field == WindField::From,
                )
            } else {
                (
                    "Wind speed (knots, 0-300)",
                    &editor.speed,
                    editor.field == WindField::Speed,
                )
            };
            format!("{label}: {value}{}", if focused { "_" } else { "" })
        }
        WindSettingsText::Turbulence => {
            if let Some(preset) = editor.turbulence {
                format!(
                    "Turbulence: {} horizontal bound, seed {}",
                    preset.label(),
                    editor.turbulence_seed
                )
            } else {
                format!("Keeping current/custom:\n{}", editor.current_turbulence)
            }
        }
        WindSettingsText::Note => bounded_ascii(&state.wind_settings.note, 60, 2),
        WindSettingsText::Error => bounded_ascii(
            if editor.error.is_empty() {
                &state.wind_settings.error
            } else {
                &editor.error
            },
            60,
            2,
        ),
    }
}

pub(super) fn selected(button: WindSettingsButton, state: &WorldMapState) -> bool {
    state
        .wind_editor
        .as_ref()
        .is_some_and(|editor| match button {
            WindSettingsButton::WindFrom => editor.field == WindField::From,
            WindSettingsButton::WindSpeed => editor.field == WindField::Speed,
            WindSettingsButton::CurrentTurbulence => editor.turbulence.is_none(),
            WindSettingsButton::Turbulence(preset) => editor.turbulence == Some(preset),
            WindSettingsButton::Apply | WindSettingsButton::Cancel => false,
        })
}
