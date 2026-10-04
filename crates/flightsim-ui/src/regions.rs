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
    /// App opt-in and catalog availability. False keeps acquisition inert.
    pub downloads_enabled: bool,
    pub downloads_visible: bool,
    pub selected: Option<String>,
    pub active: Option<String>,
    /// Preview candidate only; independent of installed and active selection.
    pub download_selected: Option<String>,
    pub store: String,
    pub status: String,
    pub error: String,
    /// Optional percentage; presentation clamps it to 100.
    pub progress: Option<u8>,
    /// App-supplied package metadata, rendered as inert paginated source/license
    /// text. Listing metadata does not establish full payload inspection.
    pub credits: String,
    /// App-supplied source URL, pinned archive hash and declared provenance
    /// for the preview candidate. Displaying it never starts acquisition.
    pub download_credits: String,
    installed: Vec<RegionSummary>,
    downloads: Vec<RegionSummary>,
    page: usize,
    credits_page: usize,
}

impl Default for RegionsState {
    fn default() -> Self {
        Self {
            visible: false,
            busy: false,
            operations_enabled: true,
            downloads_enabled: false,
            downloads_visible: false,
            selected: None,
            active: None,
            download_selected: None,
            store: String::new(),
            status: "Refresh to list installed packages".into(),
            error: String::new(),
            progress: None,
            credits: String::new(),
            download_credits: String::new(),
            installed: Vec::new(),
            downloads: Vec::new(),
            page: 0,
            credits_page: 0,
        }
    }
}

impl RegionsState {
    /// Retain at most 256 metadata rows, with bounded ASCII display names.
    /// Invalid selectors are discarded rather than silently rewritten.
    pub fn set_installed(&mut self, installed: impl IntoIterator<Item = RegionSummary>) {
        self.installed = bounded_regions(installed);
        self.page = self.page.min(self.page_count() - 1);
    }

    #[must_use]
    pub fn installed(&self) -> &[RegionSummary] {
        &self.installed
    }

    /// Set bounded catalog metadata without selecting or acquiring a package.
    pub fn set_downloads(&mut self, downloads: impl IntoIterator<Item = RegionSummary>) {
        self.downloads = bounded_regions(downloads);
        if !self.download_candidate_available() {
            self.download_selected = None;
            self.download_credits.clear();
        }
        self.page = self.page.min(self.page_count() - 1);
    }

    #[must_use]
    pub fn downloads(&self) -> &[RegionSummary] {
        &self.downloads
    }

    /// Return to installed packages after acquisition; this does not select one.
    pub fn show_installed(&mut self) {
        self.downloads_visible = false;
        self.page = 0;
        self.credits_page = 0;
    }

    fn viewing_downloads(&self) -> bool {
        self.downloads_enabled && self.downloads_visible
    }

    fn visible_regions(&self) -> &[RegionSummary] {
        if self.viewing_downloads() {
            &self.downloads
        } else {
            &self.installed
        }
    }

    fn visible_credits(&self) -> &str {
        if self.viewing_downloads() {
            if self.download_credits.trim().is_empty() {
                "Select a download to inspect its source, archive SHA-256 and declared provenance. Then choose Download / Retry or Cached only."
            } else {
                &self.download_credits
            }
        } else {
            &self.credits
        }
    }

    fn download_candidate_available(&self) -> bool {
        self.download_selected
            .as_ref()
            .is_some_and(|selected| self.downloads.iter().any(|region| &region.key == selected))
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
        self.visible_regions().len().div_ceil(ROWS_PER_PAGE).max(1)
    }

    fn row(&self, row: usize) -> Option<&RegionSummary> {
        if row >= ROWS_PER_PAGE {
            return None;
        }
        self.visible_regions()
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
            RegionsButton::ToggleDownloads => {
                if self.downloads_enabled {
                    self.downloads_visible = !self.downloads_visible;
                    self.page = 0;
                    self.credits_page = 0;
                }
            }
            RegionsButton::PreviousPage => self.page = self.page.saturating_sub(1),
            RegionsButton::NextPage => self.page = (self.page + 1).min(self.page_count() - 1),
            RegionsButton::PreviousCredits => {
                self.credits_page = self.credits_page.saturating_sub(1);
            }
            RegionsButton::NextCredits => {
                self.credits_page =
                    (self.credits_page + 1).min(credit_pages(self.visible_credits()).len() - 1);
            }
            _ if self.busy || !self.operations_enabled || actions.pending.is_some() => {}
            RegionsButton::Refresh => actions.pending = Some(RegionAction::Refresh),
            RegionsButton::Base if !self.viewing_downloads() => {
                actions.pending = Some(RegionAction::Select(None));
                self.credits_page = 0;
            }
            RegionsButton::Row(index) => {
                if let Some(region) = self.row(index) {
                    actions.pending = Some(if self.viewing_downloads() {
                        RegionAction::SelectDownload(region.key.clone())
                    } else {
                        RegionAction::Select(Some(region.key.clone()))
                    });
                    self.credits_page = 0;
                }
            }
            RegionsButton::Download | RegionsButton::CachedOnly
                if self.viewing_downloads() && self.download_candidate_available() =>
            {
                actions.pending = Some(RegionAction::Download {
                    offline: button == RegionsButton::CachedOnly,
                });
            }
            RegionsButton::Close
            | RegionsButton::Cancel
            | RegionsButton::Base
            | RegionsButton::Download
            | RegionsButton::CachedOnly => {}
        }
    }

    pub(super) fn button_disabled(&self, button: RegionsButton) -> bool {
        match button {
            RegionsButton::Refresh => self.busy || !self.operations_enabled,
            RegionsButton::Base => {
                self.busy || !self.operations_enabled || self.viewing_downloads()
            }
            RegionsButton::ToggleDownloads => !self.downloads_enabled,
            RegionsButton::Download | RegionsButton::CachedOnly => {
                self.busy
                    || !self.operations_enabled
                    || !self.viewing_downloads()
                    || !self.download_candidate_available()
            }
            RegionsButton::Row(index) => {
                self.busy || !self.operations_enabled || self.row(index).is_none()
            }
            RegionsButton::Cancel => !self.busy,
            RegionsButton::PreviousPage => self.page == 0,
            RegionsButton::NextPage => self.page >= self.page_count() - 1,
            RegionsButton::PreviousCredits => self.credits_page == 0,
            RegionsButton::NextCredits => {
                self.credits_page >= credit_pages(self.visible_credits()).len() - 1
            }
            RegionsButton::Close => false,
        }
    }

    pub(super) fn button_selected(&self, button: RegionsButton) -> bool {
        match button {
            RegionsButton::Base => !self.viewing_downloads() && self.selected.is_none(),
            RegionsButton::ToggleDownloads => self.viewing_downloads(),
            RegionsButton::Row(index) => self.row(index).is_some_and(|region| {
                let selected = if self.viewing_downloads() {
                    &self.download_selected
                } else {
                    &self.selected
                };
                selected.as_deref() == Some(region.key.as_str())
            }),
            _ => false,
        }
    }
}

fn bounded_regions(regions: impl IntoIterator<Item = RegionSummary>) -> Vec<RegionSummary> {
    regions
        .into_iter()
        .take(MAX_REGIONS)
        .filter(|region| valid_selector(&region.key))
        .map(|region| RegionSummary {
            key: region.key,
            name: bounded_ascii(&region.name, 80, 1),
        })
        .collect()
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
    /// Preview catalog metadata; never changes next-flight or active selection.
    SelectDownload(String),
    /// Explicitly acquire the app-owned preview candidate; no implicit retries.
    Download {
        offline: bool,
    },
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
    ToggleDownloads,
    Download,
    CachedOnly,
    Row(usize),
    PreviousPage,
    NextPage,
    PreviousCredits,
    NextCredits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegionsText {
    ListTitle,
    CreditsTitle,
    Help,
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
const KEYBOARD_BUTTONS: [(KeyCode, RegionsButton); 15] = [
    (KeyCode::KeyX, RegionsButton::Cancel),
    (KeyCode::KeyD, RegionsButton::ToggleDownloads),
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
    (KeyCode::KeyF, RegionsButton::Download),
    (KeyCode::KeyC, RegionsButton::CachedOnly),
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
                header.spawn((Text::new("REGION PACKAGES"), TextFont { font_size: 22.0, ..default() }, TextColor(TEXT), Node { flex_grow: 1.0, ..default() }));
                spawn_region_button(header, "Installed / Downloads [D]", RegionsButton::ToggleDownloads, px(210.0));
                spawn_region_button(header, "Refresh [R]", RegionsButton::Refresh, px(95.0));
                spawn_region_button(header, "Cancel [X]", RegionsButton::Cancel, px(135.0));
                spawn_region_button(header, "Return [Esc]", RegionsButton::Close, px(120.0));
            });
            spawn_region_text(panel, RegionsText::Help, 12.0, MUTED);
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
                    spawn_region_text(list, RegionsText::ListTitle, 13.0, ACCENT);
                    list.spawn(Node {
                        width: percent(100.0), column_gap: px(6.0), flex_shrink: 0.0,
                        ..default()
                    }).with_children(|controls| {
                        spawn_region_button(controls, "0 Base", RegionsButton::Base, percent(20.0));
                        spawn_region_button(controls, "Download / Retry [F]", RegionsButton::Download, percent(43.0));
                        spawn_region_button(controls, "Cached only [C]", RegionsButton::CachedOnly, percent(34.0));
                    });
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
                    spawn_region_text(credits, RegionsText::CreditsTitle, 13.0, ACCENT);
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
            panel.spawn((Text::new("After import/download: select an INSTALLED package, then START NEW FLIGHT on the map. The current flight stays unchanged.\nRegion flights cannot save/load replays. Esc returns to map; M closes map and cancels work."), TextFont { font_size: 12.0, ..default() }, TextColor(MUTED), Node { flex_shrink: 0.0, ..default() }));
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
        RegionsText::ListTitle => if state.viewing_downloads() {
            "DOWNLOAD CATALOG / PREVIEW ONLY"
        } else {
            "INSTALLED PACKAGES / NEXT FLIGHT"
        }
        .into(),
        RegionsText::CreditsTitle => if state.viewing_downloads() {
            "DOWNLOAD SOURCE / HASH / PROVENANCE"
        } else {
            "SELECTED PACKAGE CREDITS / LICENSE"
        }
        .into(),
        RegionsText::Help => {
            let first = if state.downloads_enabled {
                "D switches Installed / Downloads. Preview a row, then F to download/retry or C for cached only."
            } else {
                "Downloads disabled: opt-in and a prepared catalog are required. Drop a prepared ZIP here to import."
            };
            format!(
                "{first}\n0 base | 1-5 rows | PgUp/PgDn packages | Left/Right credits | R refresh | X cancel | No raw DEM/repository ZIPs"
            )
        }
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
                if index == 0 && state.visible_regions().is_empty() {
                    if state.viewing_downloads() {
                        "No download catalog entries".into()
                    } else {
                        "No installed packages".into()
                    }
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
            state.visible_regions().len()
        ),
        RegionsText::Credits => {
            let pages = credit_pages(state.visible_credits());
            pages[state.credits_page.min(pages.len() - 1)].clone()
        }
        RegionsText::CreditsPage => {
            let count = credit_pages(state.visible_credits()).len();
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
        let reminder =
            "Some characters need the original UTF-8 catalog or installed manifest/license files.";
        if sanitized.trim().is_empty() {
            sanitized = reminder.to_owned();
        } else {
            sanitized = format!("{reminder}\n\n{sanitized}");
        }
    }
    if characters.next().is_some() {
        sanitized.push_str("\nAdditional text exceeds the display limit. Read the original catalog and installed package's source/license files for complete notices.");
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
        assert!(
            pages
                .last()
                .unwrap()
                .replace('\n', "")
                .contains("complete notices")
        );
        assert_eq!(credit_pages("").len(), 1);
    }

    #[test]
    fn credits_explain_unsupported_characters_even_for_entirely_cjk_notices() {
        let pages = credit_pages("著作権\n許諾\u{1b}");
        assert_eq!(pages.len(), 1);
        let notice = pages[0].replace('\n', "");
        assert_eq!(
            notice,
            "Some characters need the original UTF-8 catalog or installed manifest/license files."
        );
        assert!(notice.is_ascii());
        assert!(!notice.contains("Select an installed package"));
        assert!(credit_pages("Source: example\n著作権")[0].starts_with("Some characters need"));
    }

    #[test]
    fn catalog_selection_preview_and_explicit_buttons_never_change_installed_selection() {
        let mut state = RegionsState {
            downloads_enabled: true,
            active: Some("active@1.0.0".into()),
            selected: Some("existing@1.0.0".into()),
            ..default()
        };
        state.set_installed(installed(2));
        state.set_downloads(installed(6));
        let mut actions = RegionsActions::default();
        assert!(state.button_disabled(RegionsButton::Download));
        state.act(RegionsButton::ToggleDownloads, &mut actions);
        assert!(actions.pending.is_none());
        assert!(state.button_disabled(RegionsButton::Base));
        state.act(RegionsButton::Row(0), &mut actions);
        assert_eq!(
            actions.pending.take(),
            Some(RegionAction::SelectDownload("region-0@1.0.0".into()))
        );
        // The application supplies the selected preview only after validation.
        assert!(state.button_disabled(RegionsButton::Download));
        state.download_selected = Some("region-0@1.0.0".into());
        for (button, offline) in [
            (RegionsButton::Download, false),
            (RegionsButton::CachedOnly, true),
        ] {
            state.act(button, &mut actions);
            state.act(RegionsButton::Row(1), &mut actions);
            assert_eq!(
                actions.pending.take(),
                Some(RegionAction::Download { offline })
            );
        }
        state.show_installed();
        assert!(state.button_disabled(RegionsButton::Download));
        assert_eq!(state.selected.as_deref(), Some("existing@1.0.0"));
        assert_eq!(state.active.as_deref(), Some("active@1.0.0"));
        state.set_downloads([]);
        assert!(state.download_selected.is_none());
    }

    #[test]
    fn catalog_download_actions_are_disabled_when_busy_unavailable_or_feature_absent() {
        for (busy, operations_enabled, downloads_enabled) in [
            (true, true, true),
            (false, false, true),
            (false, true, false),
        ] {
            let mut state = RegionsState {
                busy,
                operations_enabled,
                downloads_enabled,
                downloads_visible: true,
                ..default()
            };
            state.set_downloads(installed(1));
            state.download_selected = Some("region-0@1.0.0".into());
            let mut actions = RegionsActions::default();
            for button in [RegionsButton::Download, RegionsButton::CachedOnly] {
                assert!(state.button_disabled(button));
                state.act(button, &mut actions);
                assert!(actions.pending.is_none());
            }
        }
    }

    #[test]
    fn catalog_keyboard_keys_are_consumed_and_cancel_wins_over_download() {
        for (key, button) in [
            (KeyCode::KeyD, RegionsButton::ToggleDownloads),
            (KeyCode::KeyF, RegionsButton::Download),
            (KeyCode::KeyC, RegionsButton::CachedOnly),
        ] {
            let mut keys = ButtonInput::default();
            keys.press(key);
            assert_eq!(keyboard_button(&keys, None), Some(button));
            keys.press(KeyCode::ControlLeft);
            assert!(keyboard_button(&keys, None).is_none());
            keys.release(KeyCode::ControlLeft);
            keys.press(KeyCode::KeyX);
            assert_eq!(keyboard_button(&keys, None), Some(RegionsButton::Cancel));
            consume_keyboard_shortcuts(&mut keys, None);
            assert!(keyboard_button(&keys, None).is_none());
        }
        let mut state = RegionsState {
            downloads_enabled: true,
            downloads_visible: true,
            ..default()
        };
        state.set_downloads(installed(1));
        state.download_selected = Some("region-0@1.0.0".into());
        let mut actions = RegionsActions::default();
        state.act(RegionsButton::Download, &mut actions);
        state.act(RegionsButton::Cancel, &mut actions);
        state.act(RegionsButton::CachedOnly, &mut actions);
        assert_eq!(actions.pending, Some(RegionAction::Cancel));
    }
}
