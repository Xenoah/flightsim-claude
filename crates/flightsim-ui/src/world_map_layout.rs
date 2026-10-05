//! Presentation-only map reflow and scrolling. The existing display system owns
//! this parameter, so hosts do not need a new input system or scheduling contract.
use super::{SIDEBAR_WIDTH, WorldMapMarker, WorldMapState};
use bevy::{
    ecs::{message::MessageCursor, system::SystemParam},
    input::mouse::{MouseScrollUnit, MouseWheel},
    prelude::*,
};

#[derive(Component, Clone, Copy)]
pub(super) enum LayoutRole {
    MapContent,
    MapColumn,
    Sidebar,
    RegionColumns,
    RegionList,
    RegionCredits,
    RegionCreditsText,
}

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(super) enum ScrollSurface {
    Map,
    Credits,
    Regions,
    Wind,
    Weather,
}

/// Presentation state used by [`super::update_world_map`].
#[derive(SystemParam)]
pub struct MapLayout<'w, 's> {
    cameras: Query<'w, 's, &'static Camera>,
    nodes: Query<'w, 's, (Ref<'static, LayoutRole>, &'static mut Node), Without<WorldMapMarker>>,
    scrolls: Query<
        'w,
        's,
        (
            &'static ScrollSurface,
            &'static mut ScrollPosition,
            &'static ComputedNode,
        ),
    >,
    // Some standalone data-only hosts do not install InputPlugin. Native hosts
    // provide MouseWheel; consume every frame even when the modal is hidden.
    wheel: Option<Res<'w, Messages<MouseWheel>>>,
    cursor: Local<'s, MessageCursor<MouseWheel>>,
    previous: Local<'s, Option<ScrollSurface>>,
    narrow: Local<'s, Option<bool>>,
}

impl std::fmt::Debug for MapLayout<'_, '_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.debug_struct("MapLayout").finish_non_exhaustive()
    }
}

impl MapLayout<'_, '_> {
    pub(super) fn update(&mut self, state: &WorldMapState) {
        let narrow = self
            .cameras
            .iter()
            .filter(|camera| camera.is_active)
            .filter_map(Camera::logical_viewport_size)
            .any(|size| size.x < 900.0 || size.y < 600.0);
        for (role, mut node) in &mut self.nodes {
            if *self.narrow == Some(narrow) && !role.is_added() {
                continue;
            }
            match *role {
                LayoutRole::MapContent => {
                    // Start desktop columns at the scroll origin. Centering
                    // can place a tall column above zero, where no scroll can
                    // expose its first labels (including with auto margins).
                    node.align_items = if narrow {
                        AlignItems::Center
                    } else {
                        AlignItems::Start
                    };
                    node.height = if narrow { Val::Auto } else { percent(100.0) };
                    node.flex_direction = if narrow {
                        FlexDirection::Column
                    } else {
                        FlexDirection::Row
                    };
                    node.justify_content = if narrow {
                        JustifyContent::Start
                    } else {
                        JustifyContent::Center
                    };
                }
                LayoutRole::MapColumn => {
                    node.width = if narrow { percent(100.0) } else { Val::Auto };
                    node.flex_basis = if narrow { Val::Auto } else { px(0.0) };
                    node.flex_grow = if narrow { 0.0 } else { 1.0 };
                }
                LayoutRole::Sidebar => {
                    node.width = if narrow {
                        percent(100.0)
                    } else {
                        px(SIDEBAR_WIDTH)
                    };
                    node.height = if narrow { Val::Auto } else { percent(100.0) };
                }
                LayoutRole::RegionColumns => {
                    node.flex_direction = if narrow {
                        FlexDirection::Column
                    } else {
                        FlexDirection::Row
                    };
                    node.flex_grow = if narrow { 0.0 } else { 1.0 };
                    node.min_height = if narrow { Val::Auto } else { px(0.0) };
                }
                LayoutRole::RegionList => {
                    node.width = percent(if narrow { 100.0 } else { 44.0 });
                }
                LayoutRole::RegionCredits => {
                    node.flex_basis = if narrow { Val::Auto } else { px(0.0) };
                    node.width = if narrow { percent(100.0) } else { Val::Auto };
                }
                LayoutRole::RegionCreditsText => {
                    node.min_height = if narrow { Val::Auto } else { px(0.0) };
                    node.overflow = if narrow {
                        Overflow::visible()
                    } else {
                        Overflow::clip()
                    };
                }
            }
        }
        *self.narrow = Some(narrow);
        let delta = self.wheel.as_ref().map_or(0.0, |messages| {
            self.cursor
                .read(messages)
                .map(|event| {
                    -event.y
                        * match event.unit {
                            MouseScrollUnit::Line => 42.0,
                            MouseScrollUnit::Pixel => 1.0,
                        }
                })
                .filter(|value| value.is_finite())
                .sum::<f32>()
        });
        // Even individually finite events can overflow during accumulation.
        // Ignore such a batch instead of passing non-finite offsets to layout.
        let delta = if delta.is_finite() { delta } else { 0.0 };
        let active = state.visible.then_some(if state.weather_editor.is_some() {
            ScrollSurface::Weather
        } else if state.wind_editor.is_some() {
            ScrollSurface::Wind
        } else if state.regions.visible {
            ScrollSurface::Regions
        } else if state.credits_visible {
            ScrollSurface::Credits
        } else {
            ScrollSurface::Map
        });
        for (surface, mut position, node) in &mut self.scrolls {
            let mut next = position.y;
            if active.is_none() || (active != Some(*surface) && *surface != ScrollSurface::Map) {
                next = 0.0;
            } else if active == Some(*surface) {
                // Opening a child owns the whole frame, including its wheel.
                // Keep the map's place beneath it; reopening the map starts at top.
                let opening = *self.previous != active;
                let max = ((node.content_size().y - node.size().y) * node.inverse_scale_factor())
                    .max(0.0);
                if !opening {
                    next = (position.y + delta).clamp(0.0, max);
                }
            }
            if next.to_bits() != position.y.to_bits() {
                position.y = next;
            }
        }
        *self.previous = active;
    }
}

/// Kept outside the clipped panel so its return/scroll route remains visible.
pub(super) fn spawn_scroll_hint(overlay: &mut ChildSpawnerCommands) {
    overlay.spawn((
        Text::new("Mouse wheel: scroll | Esc: return to map"),
        TextFont {
            font_size: 12.0,
            ..default()
        },
        TextColor(super::MUTED),
        TextLayout::new_with_justify(Justify::Center),
        Node {
            position_type: PositionType::Absolute,
            bottom: px(2.0),
            width: percent(100.0),
            ..default()
        },
        bevy::ui::FocusPolicy::Pass,
    ));
}
