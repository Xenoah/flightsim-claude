//! Bounded drafts for authored new-flight visibility and cloud base.
//! The app retains exact weather parameters and resolves departure-relative
//! heights only during its transactional Start.

use super::{
    MUTED, TEXT, WorldMapActions, WorldMapButton, WorldMapState, WorldMapText, bounded_ascii,
    spawn_button, spawn_coordinate_button, spawn_map_text,
};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use flightsim_core::Meters;

const MAX_DRAFT_BYTES: usize = 16;

/// App-formatted pending values. Untouched presentation strings are never
/// parsed back into the exact authored scenario.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct WeatherSettingsView {
    pub background_visibility: String,
    /// Metres above the departure ground reference; absent for cloudless weather.
    pub cloud_base: Option<String>,
    /// App-derived largest possible offset retaining the existing layer thickness.
    /// The actual departure can impose a smaller limit at Start.
    pub cloud_base_max: Option<Meters>,
    /// Read-only layer thickness, precipitation and independent wind retention.
    pub retained: String,
    /// App policy excludes Legacy, manual clouds, replay and LAN sessions.
    pub enabled: bool,
    pub error: String,
}

/// Only explicitly edited fields. `None` retains the app's exact parameter bits.
/// The app validates the complete candidate, including absolute layer bounds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct WorldMapWeatherEdit {
    pub background_visibility: Option<Meters>,
    /// Offset above the selected departure ground reference, never an absolute height.
    pub cloud_base: Option<Meters>,
}

#[derive(Component, Debug)]
pub struct WorldMapWeatherRoot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherSettingsButton {
    BackgroundVisibility,
    CloudBase,
    Apply,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeatherSettingsText {
    BackgroundVisibility,
    CloudBase,
    Retained,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WeatherField {
    BackgroundVisibility,
    CloudBase,
}

#[derive(Debug, Clone)]
pub(super) struct WeatherEditor {
    background_visibility: String,
    cloud_base: Option<String>,
    cloud_base_max: Option<Meters>,
    visibility_dirty: bool,
    base_dirty: bool,
    field: WeatherField,
    error: String,
}

impl WeatherEditor {
    fn new(view: &WeatherSettingsView) -> Self {
        Self {
            background_visibility: bounded_ascii(&view.background_visibility, MAX_DRAFT_BYTES, 1),
            cloud_base: view
                .cloud_base
                .as_ref()
                .map(|value| bounded_ascii(value, MAX_DRAFT_BYTES, 1)),
            cloud_base_max: view
                .cloud_base_max
                .filter(|value| value.get().is_finite() && value.get() >= 0.0)
                .map(|value| Meters(value.get().min(30_000.0))),
            visibility_dirty: false,
            base_dirty: false,
            field: WeatherField::BackgroundVisibility,
            error: String::new(),
        }
    }

    pub(super) fn button(&mut self, button: WeatherSettingsButton, actions: &mut WorldMapActions) {
        match button {
            WeatherSettingsButton::BackgroundVisibility => {
                self.field = WeatherField::BackgroundVisibility
            }
            WeatherSettingsButton::CloudBase if self.cloud_base.is_some() => {
                self.field = WeatherField::CloudBase
            }
            WeatherSettingsButton::CloudBase
            | WeatherSettingsButton::Apply
            | WeatherSettingsButton::Cancel => return,
        }
        self.error.clear();
        actions.invalidate_start();
    }

    /// Ordered events preserve click/Tab/Ctrl+A input and duplicate characters.
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
                WeatherField::BackgroundVisibility if self.cloud_base.is_some() => {
                    WeatherField::CloudBase
                }
                _ => WeatherField::BackgroundVisibility,
            };
            actions.invalidate_start();
            return false;
        }
        let (draft, dirty) = match self.field {
            WeatherField::BackgroundVisibility => {
                (&mut self.background_visibility, &mut self.visibility_dirty)
            }
            WeatherField::CloudBase => {
                let Some(base) = &mut self.cloud_base else {
                    return false;
                };
                (base, &mut self.base_dirty)
            }
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

    fn changes(&mut self) -> Option<WorldMapWeatherEdit> {
        let visibility = if self.visibility_dirty {
            let Some(value) = parse_bounded(&self.background_visibility, 10.0, 200_000.0) else {
                self.error = "Background visibility: enter 10 to 200000 m".into();
                return None;
            };
            Some(Meters(value))
        } else {
            None
        };
        let base = if self.base_dirty {
            let Some(maximum) = self.cloud_base_max else {
                self.error = "Cloud-base limit unavailable; reopen settings".into();
                return None;
            };
            let Some(value) = self
                .cloud_base
                .as_deref()
                .and_then(|text| parse_bounded(text, 0.0, maximum.get()))
            else {
                self.error = format!(
                    "Cloud base: enter 0 to {} m above departure ground reference",
                    maximum.get()
                );
                return None;
            };
            Some(Meters(value))
        } else {
            None
        };
        Some(WorldMapWeatherEdit {
            background_visibility: visibility,
            cloud_base: base,
        })
    }
}

fn parse_bounded(text: &str, minimum: f64, maximum: f64) -> Option<f64> {
    text.parse::<f64>()
        .ok()
        .filter(|value| value.is_finite() && (minimum..=maximum).contains(value))
}

impl WorldMapState {
    /// Opens an app-authorized draft without mutating the pending or live weather.
    pub fn show_weather_settings(&mut self) -> bool {
        if !self.weather_settings.enabled || !self.new_flight_modal_ready() {
            return false;
        }
        self.weather_editor = Some(WeatherEditor::new(&self.weather_settings));
        self.invalidate_start_pending = true;
        true
    }

    pub(super) fn apply_weather_settings(&mut self, actions: &mut WorldMapActions) {
        actions.invalidate_start();
        if !self.weather_settings.enabled {
            self.weather_editor = None;
            return;
        }
        if let Some(editor) = &mut self.weather_editor
            && let Some(changes) = editor.changes()
        {
            actions.weather = Some(changes);
            self.weather_editor = None;
        }
    }
}

pub(super) fn spawn_weather_settings(root: &mut ChildSpawnerCommands) {
    root.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0.0), top: px(0.0), width: percent(100.0), height: percent(100.0),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.015, 0.025, 0.04, 0.98)),
        GlobalZIndex(112), FocusPolicy::Block, Visibility::Hidden,
        WorldMapWeatherRoot,
    )).with_children(|overlay| {
        overlay.spawn((Node {
            width: px(760.0), max_width: percent(94.0),
            height: px(470.0), max_height: percent(90.0),
            padding: UiRect::all(px(22.0)), flex_direction: FlexDirection::Column,
            row_gap: px(12.0), overflow: Overflow::scroll_y(), ..default()
        }, BackgroundColor(Color::srgb(0.045, 0.073, 0.103)), super::ScrollSurface::Weather, ScrollPosition::default())).with_children(|panel| {
            panel.spawn((Text::new("NEW FLIGHT: VISIBILITY / CLOUD BASE"), TextFont { font_size: 22.0, ..default() }, TextColor(TEXT), Node { flex_shrink: 0.0, ..default() }));
            panel.spawn((Text::new("Authored, not live. Applies on Start."), TextFont { font_size: 13.0, ..default() }, TextColor(MUTED), Node { flex_shrink: 0.0, ..default() }));
            spawn_coordinate_button(panel, WorldMapButton::Weather(WeatherSettingsButton::BackgroundVisibility), WorldMapText::WeatherSettings(WeatherSettingsText::BackgroundVisibility));
            spawn_coordinate_button(panel, WorldMapButton::Weather(WeatherSettingsButton::CloudBase), WorldMapText::WeatherSettings(WeatherSettingsText::CloudBase));
            panel.spawn((Text::new("Click a field or Tab; type a value; Ctrl+A clears. Mouse wheel: scroll."), TextFont { font_size: 12.0, ..default() }, TextColor(MUTED), Node { flex_shrink: 0.0, ..default() }));
            panel.spawn((Text::new("Cloud and fog add obscuration to background visibility.\nCloud base is resolved at Start against the departure ground reference.\nDeparture height and retained thickness can make Start invalid; values are not clamped."), TextFont { font_size: 12.0, ..default() }, TextColor(MUTED), Node { flex_shrink: 0.0, ..default() }));
            spawn_map_text(panel, WorldMapText::WeatherSettings(WeatherSettingsText::Retained), 12.0, TEXT);
            spawn_map_text(panel, WorldMapText::WeatherSettings(WeatherSettingsText::Error), 12.0, Color::srgb(1.0, 0.73, 0.34));
            panel.spawn(Node { flex_grow: 1.0, min_height: px(0.0), ..default() });
            panel.spawn(Node { width: percent(100.0), column_gap: px(12.0), justify_content: JustifyContent::End, flex_shrink: 0.0, ..default() }).with_children(|footer| {
                spawn_button(footer, "Cancel [Esc]", WorldMapButton::Weather(WeatherSettingsButton::Cancel), px(142.0));
                spawn_button(footer, "Apply [Enter]", WorldMapButton::Weather(WeatherSettingsButton::Apply), px(142.0));
            });
        });
        super::layout::spawn_scroll_hint(overlay);
    });
}

pub(super) fn format_text(kind: WeatherSettingsText, state: &WorldMapState) -> String {
    let Some(editor) = &state.weather_editor else {
        return String::new();
    };
    match kind {
        WeatherSettingsText::BackgroundVisibility => format!(
            "Background visibility (m, 10-200000): {}{}",
            editor.background_visibility,
            if editor.field == WeatherField::BackgroundVisibility {
                "_"
            } else {
                ""
            }
        ),
        WeatherSettingsText::CloudBase => editor.cloud_base.as_ref().map_or_else(
            || "Cloud base: no cloud layer in this preset".into(),
            |base| {
                format!(
                    "Cloud base (m, 0-{}) above departure ground reference: {base}{}",
                    editor
                        .cloud_base_max
                        .map_or_else(|| "unavailable".into(), |maximum| maximum.get().to_string()),
                    if editor.field == WeatherField::CloudBase {
                        "_"
                    } else {
                        ""
                    }
                )
            },
        ),
        WeatherSettingsText::Retained => bounded_ascii(&state.weather_settings.retained, 64, 4),
        WeatherSettingsText::Error => bounded_ascii(
            if editor.error.is_empty() {
                &state.weather_settings.error
            } else {
                &editor.error
            },
            64,
            3,
        ),
    }
}

pub(super) fn selected(button: WeatherSettingsButton, state: &WorldMapState) -> bool {
    state
        .weather_editor
        .as_ref()
        .is_some_and(|editor| match button {
            WeatherSettingsButton::BackgroundVisibility => {
                editor.field == WeatherField::BackgroundVisibility
            }
            WeatherSettingsButton::CloudBase => {
                editor.cloud_base.is_some() && editor.field == WeatherField::CloudBase
            }
            WeatherSettingsButton::Apply | WeatherSettingsButton::Cancel => false,
        })
}

pub(super) fn disabled(button: WeatherSettingsButton, state: &WorldMapState) -> bool {
    button == WeatherSettingsButton::CloudBase
        && state
            .weather_editor
            .as_ref()
            .is_none_or(|editor| editor.cloud_base.is_none())
}
