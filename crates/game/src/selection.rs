//! Unit selection (M2 decision 5): click, shift-add, double-click same
//! kind on screen, screen-space drag box, control groups, and the retained
//! gizmo rings and move markers.
//!
//! Picking rules (fixed on day one, `docs/design/ui.md`): `MeshPickingPlugin`
//! with `MeshPickingSettings { require_markers: true }`, `MeshPickingCamera`
//! on the RTS camera only, `Pickable::default()` only on unit entities
//! (`present::sync_lifecycle` adds it). The drag box never raycasts: it
//! projects every sim unit position with `Camera::world_to_viewport` and
//! tests the rectangle. Rings and markers are SEPARATE flat entities keyed
//! by `UnitId`, never children of unit entities, so unit transforms stay
//! flat and a despawned unit never drags a child with it.
//!
//! Ownership (M2 contract): agent B owns this file. The `Selection` state
//! machine is final and tested; systems have their signatures and a
//! description, bodies are B's.
// M2-B: remove this allow once the selection systems use the constants,
// DragBox, LastClick and the ring helpers below.
#![allow(dead_code)]

use std::collections::{BTreeSet, HashMap};

use bevy::picking::events::{Click, Pointer};
use bevy::picking::mesh_picking::{MeshPickingCamera, MeshPickingPlugin, MeshPickingSettings};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use sim::UnitId;

use crate::camera::RtsCamera;
use crate::hud::PointerOverUi;
use crate::present::{Interp, UnitEntities, UnitRef};
use crate::sim_driver::SimHandle;

/// A left press that moves less than this (logical px) before release is a
/// click, not a drag box.
pub const DRAG_THRESHOLD_PX: f32 = 4.0;
/// Two clicks on the same unit within this many seconds are a double click.
pub const DOUBLE_CLICK_SECS: f32 = 0.3;
/// Selection ring radius as a multiple of the unit's collision radius.
pub const RING_RADIUS_PER_UNIT_RADIUS: f32 = 1.4;
/// Move marker radius, metres.
pub const MOVE_MARKER_RADIUS: f32 = 0.5;
/// Move marker lifetime, seconds (wall clock; purely cosmetic).
pub const MOVE_MARKER_SECS: f32 = 0.6;
/// Rings float this far above the ground to avoid z-fighting.
pub const RING_Y: f32 = 0.03;

/// The selected units and the nine control groups. Render-side state: the
/// sim knows nothing about selection.
#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    /// Currently selected units, in id order.
    pub units: BTreeSet<UnitId>,
    /// Control groups 1..=9 at indices 0..=8.
    pub groups: [BTreeSet<UnitId>; 9],
}

impl Selection {
    /// Replace the selection.
    pub fn set(&mut self, units: impl IntoIterator<Item = UnitId>) {
        self.units = units.into_iter().collect();
    }

    /// Deselect everything.
    pub fn clear(&mut self) {
        self.units.clear();
    }

    /// Shift-click: add if absent, remove if present.
    pub fn toggle(&mut self, unit: UnitId) {
        if !self.units.remove(&unit) {
            self.units.insert(unit);
        }
    }

    /// Add without removing.
    pub fn extend(&mut self, units: impl IntoIterator<Item = UnitId>) {
        self.units.extend(units);
    }

    /// `true` when `unit` is selected.
    pub fn contains(&self, unit: UnitId) -> bool {
        self.units.contains(&unit)
    }

    /// `true` when nothing is selected.
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// Ctrl+`digit`: store the selection in group `digit` (1..=9). Ignores
    /// other digits.
    pub fn assign_group(&mut self, digit: u8) {
        if let Some(slot) = Self::slot(digit) {
            self.groups[slot] = self.units.clone();
        }
    }

    /// `digit`: recall group `digit` (1..=9) into the selection. Returns
    /// `false` and leaves the selection alone when the group is empty or
    /// the digit is out of range.
    pub fn recall_group(&mut self, digit: u8) -> bool {
        match Self::slot(digit) {
            Some(slot) if !self.groups[slot].is_empty() => {
                self.units = self.groups[slot].clone();
                true
            }
            _ => false,
        }
    }

    /// Drop ids that no longer exist (dead or despawned units) from the
    /// selection and every group.
    pub fn prune(&mut self, alive: impl Fn(UnitId) -> bool) {
        self.units.retain(|u| alive(*u));
        for g in &mut self.groups {
            g.retain(|u| alive(*u));
        }
    }

    fn slot(digit: u8) -> Option<usize> {
        (1..=9).contains(&digit).then(|| usize::from(digit - 1))
    }

    /// The digit for a number-row key, if it is one.
    pub fn digit_of(key: KeyCode) -> Option<u8> {
        Some(match key {
            KeyCode::Digit1 => 1,
            KeyCode::Digit2 => 2,
            KeyCode::Digit3 => 3,
            KeyCode::Digit4 => 4,
            KeyCode::Digit5 => 5,
            KeyCode::Digit6 => 6,
            KeyCode::Digit7 => 7,
            KeyCode::Digit8 => 8,
            KeyCode::Digit9 => 9,
            _ => return None,
        })
    }
}

/// A left drag in progress, in logical window pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DragRect {
    /// Where the button went down.
    pub start: Vec2,
    /// Where the cursor is now.
    pub current: Vec2,
}

impl DragRect {
    /// The normalised rectangle.
    pub fn rect(&self) -> Rect {
        Rect::from_corners(self.start, self.current)
    }

    /// `true` while the cursor has not moved past [`DRAG_THRESHOLD_PX`].
    pub fn is_click(&self) -> bool {
        self.start.distance(self.current) < DRAG_THRESHOLD_PX
    }
}

/// The current drag box, if the left button is held on the world.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct DragBox(pub Option<DragRect>);

/// Remembers the last click for double-click detection.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq)]
pub struct LastClick {
    /// Entity and wall-clock time of the previous primary click on a unit.
    pub previous: Option<(Entity, f32)>,
}

/// A retained-gizmo ring under a selected unit; one entity per selected
/// `UnitId`, tracked in [`SelectionRings`].
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectionRing(pub UnitId);

/// Ring entities by unit id.
#[derive(Resource, Default, Debug)]
pub struct SelectionRings(pub HashMap<UnitId, Entity>);

/// A short-lived ring at an order's target point.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct MoveMarker {
    /// `Time::elapsed_secs()` after which the marker despawns.
    pub expires_at: f32,
}

/// The two `GizmoAsset`s shared by every ring and marker (one circle each,
/// unit radius; entities scale their `Transform`).
#[derive(Resource, Debug, Clone)]
pub struct RingAssets {
    /// Unit-radius circle in `palette::SELECTION_RING`.
    pub selection: Handle<GizmoAsset>,
    /// Unit-radius circle in `palette::MOVE_MARKER`.
    pub marker: Handle<GizmoAsset>,
}

/// Picking, selection input, rings and markers.
pub struct SelectionPlugin;

impl Plugin for SelectionPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MeshPickingPlugin)
            .insert_resource(MeshPickingSettings {
                require_markers: true,
                ..default()
            })
            .init_resource::<Selection>()
            .init_resource::<DragBox>()
            .init_resource::<LastClick>()
            .init_resource::<SelectionRings>()
            .add_systems(Startup, (mark_picking_camera, create_ring_assets))
            .add_observer(on_unit_click)
            .add_systems(
                Update,
                (
                    update_drag_box,
                    control_groups,
                    clear_selection_on_escape,
                    expire_move_markers,
                )
                    .chain(),
            )
            .add_systems(PostUpdate, (prune_dead, sync_selection_rings).chain());
    }
}

/// Adds `MeshPickingCamera` to the RTS camera (`camera.rs` spawns it in its
/// own Startup system; ordering is not needed because this runs as a
/// command-flushed query on the next frame if the camera is not there yet).
fn mark_picking_camera(
    mut commands: Commands,
    cameras: Query<Entity, (With<RtsCamera>, Without<MeshPickingCamera>)>,
) {
    for cam in &cameras {
        commands.entity(cam).insert(MeshPickingCamera);
    }
}

/// Builds the two shared ring assets: a unit circle laid flat
/// (`present::flat_on_ground()`), resolution 48.
///
/// M2-B: body to implement (`GizmoAsset::new()`, `.circle(..)`, `gizmo_assets.add`).
fn create_ring_assets(mut commands: Commands, mut gizmo_assets: ResMut<Assets<GizmoAsset>>) {
    // M2-B: replace these empty assets with the two circles.
    let selection = gizmo_assets.add(GizmoAsset::new());
    let marker = gizmo_assets.add(GizmoAsset::new());
    commands.insert_resource(RingAssets { selection, marker });
}

/// Primary click on a unit entity: plain click selects it alone; shift
/// toggles it; a second primary click on the same entity within
/// [`DOUBLE_CLICK_SECS`] selects every unit of the same kind whose
/// projected position is inside the camera's logical viewport rect.
/// Ignored while [`PointerOverUi`] is set or the target has no [`UnitRef`]
/// (the event propagates to the window entity too).
///
/// M2-B: body to implement.
#[allow(clippy::too_many_arguments)] // Bevy system: each parameter is one resource or query
pub fn on_unit_click(
    _click: On<Pointer<Click>>,
    _units: Query<&UnitRef>,
    _selection: ResMut<Selection>,
    _over_ui: Res<PointerOverUi>,
    _keys: Res<ButtonInput<KeyCode>>,
    _last: ResMut<LastClick>,
    _time: Res<Time>,
    _sim: NonSend<SimHandle>,
    _camera: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
) {
    // M2-B: see the doc comment.
}

/// Left button: on press (not over UI) start a [`DragRect`] at the cursor;
/// while held, update `current`; on release, if `!is_click()`, select the
/// units whose sim position projects inside the rect (shift extends) via
/// [`units_in_rect`]; a release that is still a click on empty ground
/// clears the selection unless shift is held. Units clicked directly are
/// handled by [`on_unit_click`], which fires on the same release.
///
/// M2-B: body to implement.
#[allow(clippy::too_many_arguments)] // Bevy system: each parameter is one resource or query
pub fn update_drag_box(
    _buttons: Res<ButtonInput<MouseButton>>,
    _keys: Res<ButtonInput<KeyCode>>,
    _windows: Query<&Window, With<PrimaryWindow>>,
    _over_ui: Res<PointerOverUi>,
    _drag: ResMut<DragBox>,
    _selection: ResMut<Selection>,
    _sim: NonSend<SimHandle>,
    _camera: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
) {
    // M2-B: see the doc comment.
}

/// The ids whose world positions project inside `rect` (logical pixels).
/// Positions behind the camera (`world_to_viewport` errors) are skipped.
pub fn units_in_rect(
    rect: Rect,
    camera: &Camera,
    camera_transform: &GlobalTransform,
    positions: impl IntoIterator<Item = (UnitId, Vec3)>,
) -> BTreeSet<UnitId> {
    positions
        .into_iter()
        .filter_map(|(id, pos)| {
            camera
                .world_to_viewport(camera_transform, pos)
                .ok()
                .filter(|p| rect.contains(*p))
                .map(|_| id)
        })
        .collect()
}

/// Ctrl+1..9 assigns, 1..9 recalls (`Selection::assign_group` /
/// `recall_group`). Cmd is not used: it is the menu modifier on macOS.
pub fn control_groups(keys: Res<ButtonInput<KeyCode>>, mut selection: ResMut<Selection>) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    for key in keys.get_just_pressed() {
        if let Some(digit) = Selection::digit_of(*key) {
            if ctrl {
                selection.assign_group(digit);
            } else {
                selection.recall_group(digit);
            }
        }
    }
}

/// Esc clears the selection (the pause menu takes Esc over from M6).
pub fn clear_selection_on_escape(
    keys: Res<ButtonInput<KeyCode>>,
    mut selection: ResMut<Selection>,
) {
    if keys.just_pressed(KeyCode::Escape) {
        selection.clear();
    }
}

/// Drop ids that left the sim from the selection and groups.
pub fn prune_dead(sim: NonSend<SimHandle>, mut selection: ResMut<Selection>) {
    if selection.is_empty() && selection.groups.iter().all(BTreeSet::is_empty) {
        return;
    }
    let view = sim.view();
    selection.prune(|id| view.unit(id).is_some());
}

/// One ring entity per selected unit: spawn `(SelectionRing(id), Gizmo {
/// handle: assets.selection, .. }, Transform)` for newly selected ids,
/// despawn rings whose id left the selection, and every frame place each
/// ring at the unit entity's interpolated position (`Interp::sample` with
/// the overstep fraction) at [`RING_Y`], scaled by the kind's radius times
/// [`RING_RADIUS_PER_UNIT_RADIUS`]. Rings are never children of units.
///
/// M2-B: body to implement.
#[allow(clippy::too_many_arguments)] // Bevy system: each parameter is one resource or query
pub fn sync_selection_rings(
    _commands: Commands,
    _selection: Res<Selection>,
    _rings: ResMut<SelectionRings>,
    _assets: Option<Res<RingAssets>>,
    _entities: Res<UnitEntities>,
    _interps: Query<&Interp>,
    _ring_transforms: Query<&mut Transform, With<SelectionRing>>,
    _fixed: Res<Time<Fixed>>,
    _sim: NonSend<SimHandle>,
) {
    // M2-B: see the doc comment.
}

/// Spawn a move marker ring at `at` (world) that expires after
/// [`MOVE_MARKER_SECS`]. Called by `orders.rs` on a successful ground order.
pub fn spawn_move_marker(commands: &mut Commands, assets: &RingAssets, at: Vec3, now_secs: f32) {
    commands.spawn((
        Name::new("Move marker"),
        MoveMarker {
            expires_at: now_secs + MOVE_MARKER_SECS,
        },
        Gizmo {
            handle: assets.marker.clone(),
            ..default()
        },
        Transform::from_translation(Vec3::new(at.x, RING_Y, at.z))
            .with_scale(Vec3::splat(MOVE_MARKER_RADIUS)),
    ));
}

/// Shrinks and despawns move markers past their expiry.
pub fn expire_move_markers(
    mut commands: Commands,
    time: Res<Time>,
    mut markers: Query<(Entity, &MoveMarker, &mut Transform)>,
) {
    let now = time.elapsed_secs();
    for (entity, marker, mut transform) in &mut markers {
        if now >= marker.expires_at {
            commands.entity(entity).despawn();
        } else {
            let left = ((marker.expires_at - now) / MOVE_MARKER_SECS).clamp(0.0, 1.0);
            transform.scale = Vec3::splat(MOVE_MARKER_RADIUS * (0.4 + 0.6 * left));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[u32]) -> Vec<UnitId> {
        v.iter().copied().map(UnitId).collect()
    }

    #[test]
    fn selection_set_toggle_and_groups() {
        let mut s = Selection::default();
        assert!(s.is_empty());
        s.set(ids(&[3, 1, 2]));
        assert_eq!(s.units.iter().copied().collect::<Vec<_>>(), ids(&[1, 2, 3]));
        s.toggle(UnitId(2));
        assert!(!s.contains(UnitId(2)));
        s.toggle(UnitId(9));
        assert!(s.contains(UnitId(9)));
        s.assign_group(1);
        s.clear();
        assert!(s.is_empty());
        assert!(s.recall_group(1));
        assert_eq!(s.units.iter().copied().collect::<Vec<_>>(), ids(&[1, 3, 9]));
        assert!(!s.recall_group(2), "empty group leaves the selection alone");
        assert_eq!(s.units.len(), 3);
        assert!(!s.recall_group(0));
        s.assign_group(10);
        assert!(s.groups.iter().filter(|g| !g.is_empty()).count() == 1);
        s.prune(|u| u.0 != 9);
        assert!(!s.contains(UnitId(9)));
        assert!(!s.groups[0].contains(&UnitId(9)));
        assert_eq!(Selection::digit_of(KeyCode::Digit7), Some(7));
        assert_eq!(Selection::digit_of(KeyCode::KeyA), None);
    }

    #[test]
    fn drag_rect_distinguishes_clicks() {
        let click = DragRect {
            start: Vec2::new(10.0, 10.0),
            current: Vec2::new(12.0, 11.0),
        };
        assert!(click.is_click());
        let drag = DragRect {
            start: Vec2::new(100.0, 50.0),
            current: Vec2::new(20.0, 90.0),
        };
        assert!(!drag.is_click());
        let r = drag.rect();
        assert_eq!(r.min, Vec2::new(20.0, 50.0));
        assert_eq!(r.max, Vec2::new(100.0, 90.0));
    }
}
