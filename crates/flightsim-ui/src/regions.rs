//! Bounded data-only regional package presentation for the world map.

use super::{
    ACCENT, BUTTON, MUTED, TEXT, WorldMapButton, WorldMapText, bounded_ascii, spawn_button,
    spawn_map_text,
};
use bevy::input::keyboard::Key;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;

const MAX_REGIONS: usize = 256;
const ROWS_PER_PAGE: usize = 5;
const CREDIT_COLUMNS: usize = 60;
const CREDIT_LINES: usize = 17;
const MAX_CREDIT_CHARACTERS: usize = 65_536;

/// Immutable package metadata supplied by the app. The key is an opaque
/// `ID@VERSION` selector, never a path, URL or instruction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegionSummary {
    pub key: String,
    pub name: String,
}

/// App-owned operation state. Selection and active flight identity are separate:
/// choosing a row cannot replace a live flight's source.
#[derive(Debug, Clone)]
pub struct RegionsState {
    pub visible: bool,
    pub busy: bool,
    pub operations_enabled: bool,
    pub selected: Option<String>,
    pub active: Option<String>,
    pub store: String,
    pub status: String,
    pub error: String,
    /// Optional percentage; presentation clamps it to 100.
    pub progress: Option<u8>,
    /// App-supplied package metadata, rendered as inert paginated source/license
    /// text. Listing metadata does not establish full payload inspection.
    pub credits: String,
    installed: Vec<RegionSummary>,
    page: usize,
    credits_page: usize,
}

impl Default for RegionsState {
    fn default() -> Self {
        Self {
            visible: false,
            busy: false,
            operations_enabled: true,
            selected: None,
            active: None,
            store: String::new(),
            status: "Refresh to list installed packages".into(),
            error: String::new(),
            progress: None,
            credits: String::new(),
            installed: Vec::new(),
            page: 0,
            credits_page: 0,
        }
    }
}

impl RegionsState {
    /// Retain at most 256 metadata rows, with bounded ASCII display names.
    /// Invalid selectors are discarded rather than silently rewritten.
    pub fn set_installed(&mut self, installed: impl IntoIterator<Item = RegionSummary>) {
        self.installed = installed
            .into_iter()
            .take(MAX_REGIONS)
            .filter(|region| valid_selector(&region.key))
            .map(|region| RegionSummary {
                key: region.key,
                name: bounded_ascii(&region.name, 80, 1),
            })
            .collect();
        self.page = self.page.min(self.page_count() - 1);
    }

    #[must_use]
    pub fn installed(&self) -> &[RegionSummary] {
        &self.installed
    }

    pub(super) fn show(&mut self) {
        self.visible = true;
        self.page = self.page.min(self.page_count() - 1);
        self.credits_page = 0;
    }

    pub(super) fn dismiss(&mut self, actions: &mut RegionsActions) {
        // Even idle dismissal is an explicit cancellation boundary. The app
        // must discard same-frame drops rather than reopen the closed panel.
        if self.visible {
            actions.pending = Some(RegionAction::Cancel);
        }
        self.visible = false;
    }

    fn page_count(&self) -> usize {
        self.installed.len().div_ceil(ROWS_PER_PAGE).max(1)
    }

    fn row(&self, row: usize) -> Option<&RegionSummary> {
        if row >= ROWS_PER_PAGE {
            return None;
        }
        self.installed
            .get(self.page.min(self.page_count() - 1) * ROWS_PER_PAGE + row)
    }

    pub(super) fn act(&mut self, button: RegionsButton, actions: &mut RegionsActions) {
        // Cancel supersedes any unconsumed command. Other commands occupy only
        // one slot and cannot be duplicated before the app consumes it.
        if button == RegionsButton::Cancel {
            if self.busy || actions.pending.is_some() {
                actions.pending = Some(RegionAction::Cancel);
            }
            return;
        }
        match button {
            RegionsButton::PreviousPage => self.page = self.page.saturating_sub(1),
            RegionsButton::NextPage => self.page = (self.page + 1).min(self.page_count() - 1),
            RegionsButton::PreviousCredits => {
                self.credits_page = self.credits_page.saturating_sub(1);
            }
            RegionsButton::NextCredits => {
                self.credits_page =
                    (self.credits_page + 1).min(credit_pages(&self.credits).len() - 1);
            }
            _ if self.busy || !self.operations_enabled || actions.pending.is_some() => {}
            RegionsButton::Refresh => actions.pending = Some(RegionAction::Refresh),
            RegionsButton::Base => {
                actions.pending = Some(RegionAction::Select(None));
                self.credits_page = 0;
            }
            RegionsButton::Row(index) => {
                if let Some(region) = self.row(index) {
                    actions.pending = Some(RegionAction::Select(Some(region.key.clone())));
                    self.credits_page = 0;
                }
            }
            RegionsButton::Close | RegionsButton::Cancel => {}
        }
    }

    pub(super) fn button_disabled(&self, button: RegionsButton) -> bool {
        match button {
            RegionsButton::Refresh | RegionsButton::Base => self.busy || !self.operations_enabled,
            RegionsButton::Row(index) => {
                self.busy || !self.operations_enabled || self.row(index).is_none()
            }
            RegionsButton::Cancel => !self.busy,
            RegionsButton::PreviousPage => self.page == 0,
            RegionsButton::NextPage => self.page >= self.page_count() - 1,
            RegionsButton::PreviousCredits => self.credits_page == 0,
            RegionsButton::NextCredits => {
                self.credits_page >= credit_pages(&self.credits).len() - 1
            }
            RegionsButton::Close => false,
        }
    }

    pub(super) fn button_selected(&self, button: RegionsButton) -> bool {
        match button {
            RegionsButton::Base => self.selected.is_none(),
            RegionsButton::Row(index) => self
                .row(index)
                .is_some_and(|region| self.selected.as_deref() == Some(region.key.as_str())),
            _ => false,
        }
    }
}

fn valid_selector(selector: &str) -> bool {
    if selector.len() > 112 {
        return false;
    }
    let Some((id, version)) = selector.split_once('@') else {
        return false;
    };
    !id.is_empty()
        && !version.is_empty()
        && id.len() <= 80
        && id
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || b".-".contains(&c))
        && version.bytes().all(|c| c.is_ascii_digit() || c == b'.')
}

/// A single app-consumed command slot. No IO or package activation occurs here.
#[derive(Debug, Clone, Default)]
pub struct RegionsActions {
    pub pending: Option<RegionAction>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegionAction {
    Refresh,
    /// `None` chooses the app's baseline/global source for the next flight.
    Select(Option<String>),
    Cancel,
}

#[derive(Component, Debug)]
pub struct WorldMapRegionsRoot;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionsButton {
    Close,
    Refresh,
    Cancel,
    Base,
    Row(usize),
    PreviousPage,
    NextPage,
    PreviousCredits,
    NextCredits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionsText {
    Status,
    Selection,
    Store,
    Row(usize),
    Page,
    Credits,
    CreditsPage,
}

// One keyboard action per frame. Cancellation and page navigation win over
// selection, so a simultaneous page/row press never selects a different page's
// package accidentally. All actions use the same busy and single-slot gates as
// the visible buttons. Numeric shortcuts use the labelled top-row keys only.
const KEYBOARD_BUTTONS: [(KeyCode, RegionsButton); 12] = [
    (KeyCode::KeyX, RegionsButton::Cancel),
    (KeyCode::PageUp, RegionsButton::PreviousPage),
    (KeyCode::PageDown, RegionsButton::NextPage),
    (KeyCode::ArrowLeft, RegionsButton::PreviousCredits),
    (KeyCode::ArrowRight, RegionsButton::NextCredits),
    (KeyCode::KeyR, RegionsButton::Refresh),
    (KeyCode::Digit0, RegionsButton::Base),
    (KeyCode::Digit1, RegionsButton::Row(0)),
    (KeyCode::Digit2, RegionsButton::Row(1)),
    (KeyCode::Digit3, RegionsButton::Row(2)),
    (KeyCode::Digit4, RegionsButton::Row(3)),
    (KeyCode::Digit5, RegionsButton::Row(4)),
];

pub(super) fn shortcuts_allowed(
    keys: &ButtonInput<KeyCode>,
    logical: Option<&ButtonInput<Key>>,
) -> bool {
    !keys.any_pressed([
        KeyCode::ControlLeft,
        KeyCode::ControlRight,
        KeyCode::AltLeft,
        KeyCode::AltRight,
        KeyCode::SuperLeft,
        KeyCode::SuperRight,
    ]) && logical
        .is_none_or(|keys| !keys.any_pressed([Key::Control, Key::Alt, Key::Meta, Key::Super]))
}

fn logical_navigation(button: RegionsButton) -> Option<Key> {
    match button {
        RegionsButton::PreviousPage => Some(Key::PageUp),
        RegionsButton::NextPage => Some(Key::PageDown),
        RegionsButton::PreviousCredits => Some(Key::ArrowLeft),
        RegionsButton::NextCredits => Some(Key::ArrowRight),
        _ => None,
    }
}

pub(super) fn keyboard_button(
    keys: &ButtonInput<KeyCode>,
    logical: Option<&ButtonInput<Key>>,
) -> Option<RegionsButton> {
    if !shortcuts_allowed(keys, logical) {
        return None;
    }
    KEYBOARD_BUTTONS.iter().find_map(|(key, button)| {
        // Logical navigation covers NumLock-off keypad and remapped keys.
        // OR the two representations before dispatch so a normal arrow
        // reported in both channels still advances exactly once.
        let semantic = logical.is_some_and(|keys| {
            logical_navigation(*button).is_some_and(|key| keys.just_pressed(key))
        });
        (keys.just_pressed(*key) || semantic).then_some(*button)
    })
}

pub(super) fn consume_keyboard_shortcuts(
    keys: &mut ButtonInput<KeyCode>,
    logical: Option<&mut ButtonInput<Key>>,
) {
    keys.clear_just_pressed(KeyCode::KeyG);
    for (key, _) in KEYBOARD_BUTTONS {
        keys.clear_just_pressed(key);
    }
    if let Some(logical) = logical {
        for key in [Key::PageUp, Key::PageDown, Key::ArrowLeft, Key::ArrowRight] {
            logical.clear_just_pressed(key);
        }
    }
}

pub(super) fn spawn_regions(root: &mut ChildSpawnerCommands) {
    root.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0.0), top: px(0.0), width: percent(100.0), height: percent(100.0),
            align_items: AlignItems::Center, justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(Color::srgba(0.015, 0.025, 0.04, 0.98)),
        GlobalZIndex(111), FocusPolicy::Block, Visibility::Hidden,
        WorldMapRegionsRoot,
    )).with_children(|overlay| {
        overlay.spawn((Node {
            width: px(1100.0), max_width: percent(96.0),
            height: px(620.0), max_height: percent(94.0),
            padding: UiRect::all(px(20.0)),
            flex_direction: FlexDirection::Column, row_gap: px(8.0),
            overflow: Overflow::clip(), ..default()
        }, BackgroundColor(Color::srgb(0.045, 0.073, 0.103)))).with_children(|panel| {
            panel.spawn(Node {
                width: percent(100.0), height: px(32.0), flex_shrink: 0.0,
                align_items: AlignItems::Center, column_gap: px(8.0),
                ..default()
            }).with_children(|header| {
                header.spawn((Text::new("LOCAL REGIONS"), TextFont { font_size: 22.0, ..default() }, TextColor(TEXT), Node { flex_grow: 1.0, ..default() }));
                spawn_region_button(header, "Refresh [R]", RegionsButton::Refresh, px(95.0));
                spawn_region_button(header, "Cancel [X]", RegionsButton::Cancel, px(135.0));
                spawn_region_button(header, "Return [Esc]", RegionsButton::Close, px(120.0));
            });
            panel.spawn((Text::new("Drop a prepared region ZIP here; raw DEMs/repository ZIPs unsupported. CLI: --import-region PATH.zip\nKeyboard: 0 base | 1-5 rows | PgUp/PgDn packages | Left/Right credits | R refresh | X cancel"), TextFont { font_size: 12.0, ..default() }, TextColor(MUTED), Node { flex_shrink: 0.0, ..default() }));
            spawn_region_text(panel, RegionsText::Status, 13.0, ACCENT);
            spawn_region_text(panel, RegionsText::Selection, 12.0, TEXT);
            panel.spawn(Node {
                width: percent(100.0), flex_grow: 1.0, min_height: px(0.0), column_gap: px(18.0),
                ..default()
            }).with_children(|columns| {
                columns.spawn(Node {
                    width: percent(44.0), min_width: px(0.0),
                    flex_direction: FlexDirection::Column, row_gap: px(6.0),
                    ..default()
                }).with_children(|list| {
                    list.spawn((Text::new("INSTALLED PACKAGES / NEXT FLIGHT"), TextFont { font_size: 13.0, ..default() }, TextColor(ACCENT)));
                    spawn_region_button(list, "0  Use global / base terrain", RegionsButton::Base, percent(100.0));
                    for row in 0..ROWS_PER_PAGE {
                        list.spawn((
                            Button,
                            Node {
                                width: percent(100.0), height: px(43.0), flex_shrink: 0.0,
                                padding: UiRect::axes(px(8.0), px(5.0)),
                                align_items: AlignItems::Center, overflow: Overflow::clip(),
                                ..default()
                            },
                            BackgroundColor(BUTTON), WorldMapButton::Regions(RegionsButton::Row(row)),
                        )).with_children(|button| spawn_region_text(button, RegionsText::Row(row), 12.0, TEXT));
                    }
                    list.spawn(Node {
                        width: percent(100.0), column_gap: px(8.0), align_items: AlignItems::Center,
                        ..default()
                    }).with_children(|pages| {
                        spawn_region_button(pages, "Previous", RegionsButton::PreviousPage, px(90.0));
                        pages.spawn(Node { flex_grow: 1.0, min_width: px(0.0), ..default() }).with_children(|text| spawn_region_text(text, RegionsText::Page, 12.0, MUTED));
                        spawn_region_button(pages, "Next", RegionsButton::NextPage, px(90.0));
                    });
                });
                columns.spawn(Node {
                    flex_grow: 1.0, flex_basis: px(0.0), min_width: px(0.0),
                    flex_direction: FlexDirection::Column, row_gap: px(6.0),
                    ..default()
                }).with_children(|credits| {
                    credits.spawn((Text::new("SELECTED PACKAGE CREDITS / LICENSE"), TextFont { font_size: 13.0, ..default() }, TextColor(ACCENT)));
                    credits.spawn(Node {
                        width: percent(100.0), flex_grow: 1.0, min_height: px(0.0),
                        overflow: Overflow::clip(), ..default()
                    }).with_children(|text| spawn_region_text(text, RegionsText::Credits, 12.0, TEXT));
                    credits.spawn(Node {
                        width: percent(100.0), column_gap: px(8.0), align_items: AlignItems::Center,
                        ..default()
                    }).with_children(|pages| {
                        spawn_region_button(pages, "Previous", RegionsButton::PreviousCredits, px(90.0));
                        pages.spawn(Node { flex_grow: 1.0, min_width: px(0.0), ..default() }).with_children(|text| spawn_region_text(text, RegionsText::CreditsPage, 12.0, MUTED));
                        spawn_region_button(pages, "Next", RegionsButton::NextCredits, px(90.0));
                    });
                });
            });
            spawn_region_text(panel, RegionsText::Store, 11.0, MUTED);
            panel.spawn((Text::new("Selection is pending until START NEW FLIGHT on the map. The current flight stays unchanged.\nRegion-backed flights cannot save or load legacy v1/v2 replays. Esc returns to map; M closes map and cancels pending work."), TextFont { font_size: 12.0, ..default() }, TextColor(MUTED), Node { flex_shrink: 0.0, ..default() }));
        });
    });
}

fn spawn_region_button(
    parent: &mut ChildSpawnerCommands,
    text: &str,
    button: RegionsButton,
    width: Val,
) {
    spawn_button(parent, text, WorldMapButton::Regions(button), width);
}

fn spawn_region_text(
    parent: &mut ChildSpawnerCommands,
    kind: RegionsText,
    size: f32,
    color: Color,
) {
    spawn_map_text(parent, WorldMapText::Regions(kind), size, color);
}

pub(super) fn format_regions_text(kind: RegionsText, state: &RegionsState) -> String {
    match kind {
        RegionsText::Status => {
            let operation = if let Some(progress) = state.progress.filter(|_| state.busy) {
                format!(
                    "Working: {}% | {}",
                    progress.min(100),
                    bounded_ascii(&state.status, 76, 1)
                )
            } else if !state.operations_enabled {
                "Region operations unavailable for this flight mode".into()
            } else {
                bounded_ascii(&state.status, 100, 1)
            };
            format!("{operation}\n{}", bounded_ascii(&state.error, 100, 1))
        }
        RegionsText::Selection => format!(
            "Next flight: {}\nActive flight: {}",
            bounded_ascii(
                state.selected.as_deref().unwrap_or("Global / base terrain"),
                90,
                1
            ),
            bounded_ascii(
                state.active.as_deref().unwrap_or("Global / base terrain"),
                90,
                1
            ),
        ),
        RegionsText::Store => format!("Local store: {}", bounded_ascii(&state.store, 110, 1)),
        RegionsText::Row(index) => state.row(index).map_or_else(
            || {
                if index == 0 && state.installed.is_empty() {
                    "No installed packages".into()
                } else {
                    "-".into()
                }
            },
            |region| {
                format!(
                    "{}  {}\n{}",
                    index + 1,
                    bounded_ascii(&region.name, 45, 1),
                    bounded_ascii(&region.key, 52, 1)
                )
            },
        ),
        RegionsText::Page => format!(
            "{} / {}  ({} total)",
            state.page.min(state.page_count() - 1) + 1,
            state.page_count(),
            state.installed.len()
        ),
        RegionsText::Credits => {
            let pages = credit_pages(&state.credits);
            pages[state.credits_page.min(pages.len() - 1)].clone()
        }
        RegionsText::CreditsPage => {
            let count = credit_pages(&state.credits).len();
            format!("Page {} / {count}", state.credits_page.min(count - 1) + 1)
        }
    }
}

fn credit_pages(text: &str) -> Vec<String> {
    let mut characters = text.chars();
    let mut simplified = false;
    let mut sanitized: String = characters
        .by_ref()
        .take(MAX_CREDIT_CHARACTERS)
        .map(|c| {
            if c == '\n' || (c.is_ascii() && !c.is_control()) {
                c
            } else {
                simplified = true;
                ' '
            }
        })
        .collect();
    if simplified {
        // A notice made entirely of unsupported glyphs must not become a set
        // of empty pages or look like missing attribution. Put the reminder
        // first so long source/notice fields cannot hide it on the last page.
        let reminder = "Some characters need the original UTF-8 manifest/license files in the installed package.";
        if sanitized.trim().is_empty() {
            sanitized = reminder.to_owned();
        } else {
            sanitized = format!("{reminder}\n\n{sanitized}");
        }
    }
    if characters.next().is_some() {
        sanitized.push_str("\nAdditional text exceeds the display limit. Read the installed package's source and license files for complete notices.");
    }
    if sanitized.trim().is_empty() {
        sanitized = "Select an installed package to read its source and license notices. Global data credits are available from the map header.".into();
    }
    let mut lines = Vec::new();
    for paragraph in sanitized.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
        } else {
            lines.extend(paragraph.as_bytes().chunks(CREDIT_COLUMNS).map(|line| {
                // Sanitization above guarantees ASCII and valid chunk boundaries.
                String::from_utf8(line.to_vec()).expect("ASCII region credits")
            }));
        }
    }
    lines
        .chunks(CREDIT_LINES)
        .map(|page| page.join("\n"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed(count: usize) -> Vec<RegionSummary> {
        (0..count)
            .map(|n| RegionSummary {
                key: format!("region-{n}@1.0.0"),
                name: format!("Region {n}"),
            })
            .collect()
    }

    #[test]
    fn installed_storage_and_visible_rows_are_bounded_and_selectors_not_rewritten() {
        let mut state = RegionsState::default();
        state.set_installed(installed(300));
        assert_eq!(state.installed().len(), 256);
        assert_eq!(state.page_count(), 52);
        assert!(state.row(5).is_none());
        let mut actions = RegionsActions::default();
        for _ in 0..60 {
            state.act(RegionsButton::NextPage, &mut actions);
        }
        assert_eq!(state.page, 51);
        assert_eq!(state.row(0).unwrap().key, "region-255@1.0.0");
        assert!(state.row(1).is_none());
        state.set_installed([
            RegionSummary {
                key: "bad\n@1.0.0".into(),
                name: "Bad".into(),
            },
            RegionSummary {
                key: "region@1.0.0".into(),
                name: "世界\u{1b}\nHidden".repeat(100),
            },
        ]);
        assert_eq!(state.installed().len(), 1);
        assert_eq!(state.installed()[0].key, "region@1.0.0");
        assert_eq!(state.installed()[0].name, "???");
        assert_eq!(state.page, 0);
    }

    #[test]
    fn busy_and_unavailable_states_disable_mutations_but_cancel_supersedes_pending() {
        for (busy, operations_enabled) in [(true, true), (false, false)] {
            let mut state = RegionsState {
                busy,
                operations_enabled,
                ..default()
            };
            state.set_installed(installed(6));
            let mut actions = RegionsActions::default();
            for button in [
                RegionsButton::Refresh,
                RegionsButton::Base,
                RegionsButton::Row(0),
            ] {
                state.act(button, &mut actions);
                assert!(actions.pending.is_none());
                assert!(state.button_disabled(button));
            }
            state.act(RegionsButton::NextPage, &mut actions);
            assert_eq!(state.page, 1);
            actions.pending = Some(RegionAction::Select(Some("region-0@1.0.0".into())));
            state.act(RegionsButton::Cancel, &mut actions);
            assert_eq!(actions.pending, Some(RegionAction::Cancel));
            state.act(RegionsButton::Refresh, &mut actions);
            assert_eq!(actions.pending, Some(RegionAction::Cancel));
        }
    }

    #[test]
    fn idle_panel_dismissal_is_a_cancel_boundary_but_hidden_panel_is_not() {
        let mut state = RegionsState::default();
        let mut actions = RegionsActions::default();
        state.dismiss(&mut actions);
        assert!(actions.pending.is_none());
        state.show();
        state.dismiss(&mut actions);
        assert_eq!(actions.pending.take(), Some(RegionAction::Cancel));
        state.dismiss(&mut actions);
        assert!(actions.pending.is_none());
    }

    #[test]
    fn one_slot_actions_do_not_replace_an_unconsumed_selection_or_activate_a_flight() {
        let mut state = RegionsState {
            active: Some("active@1.0.0".into()),
            ..default()
        };
        state.set_installed(installed(2));
        let mut actions = RegionsActions::default();
        state.act(RegionsButton::Row(0), &mut actions);
        state.act(RegionsButton::Row(1), &mut actions);
        state.act(RegionsButton::Refresh, &mut actions);
        assert_eq!(
            actions.pending.take(),
            Some(RegionAction::Select(Some("region-0@1.0.0".into())))
        );
        assert!(state.selected.is_none());
        assert_eq!(state.active.as_deref(), Some("active@1.0.0"));
        state.act(RegionsButton::Base, &mut actions);
        assert_eq!(actions.pending, Some(RegionAction::Select(None)));
    }

    #[test]
    fn credits_keep_complete_ascii_notices_paginated_and_disclose_truncation() {
        let original = (0..40)
            .map(|n| format!("License notice {n:02}"))
            .collect::<Vec<_>>()
            .join("\n");
        let pages = credit_pages(&original);
        assert_eq!(pages.len(), 3);
        assert_eq!(pages.join("\n"), original);
        assert!(pages[2].contains("License notice 39"));
        let pages = credit_pages(&"©\u{1b}x".repeat(MAX_CREDIT_CHARACTERS));
        assert!(
            pages
                .iter()
                .all(|page| page.is_ascii() && page.lines().count() <= CREDIT_LINES)
        );
        assert!(
            pages
                .iter()
                .flat_map(|page| page.lines())
                .all(|line| line.len() <= CREDIT_COLUMNS)
        );
        assert!(pages.last().unwrap().contains("complete notices"));
        assert_eq!(credit_pages("").len(), 1);
    }

    #[test]
    fn credits_explain_unsupported_characters_even_for_entirely_cjk_notices() {
        let pages = credit_pages("著作権\n許諾\u{1b}");
        assert_eq!(pages.len(), 1);
        let notice = pages[0].replace('\n', "");
        assert_eq!(
            notice,
            "Some characters need the original UTF-8 manifest/license files in the installed package."
        );
        assert!(notice.is_ascii());
        assert!(!notice.contains("Select an installed package"));
        assert!(credit_pages("Source: example\n著作権")[0].starts_with("Some characters need"));
    }
}
