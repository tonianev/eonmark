//! Orders (M2 decision 6): a right click on the ground becomes
//! `Command::Move { units: selection, target, queue: shift }`, S is Stop,
//! A then left click is the `AttackMove` placeholder (the sim applies it like
//! Move until M4a). The ground hit is one ray-plane intersection with
//! y = 0 (`Camera::viewport_to_world`, then solve for t): never a mesh
//! raycast. World clicks are ignored while the pointer is over the HUD
//! (`PointerOverUi`).
//!
//! Every order goes through `PendingCommands::push_local`, which stamps the
//! seq; nothing here touches the sim.
//!
//! Ownership (M2 contract): agent B owns this file. `ground_hit` and the
//! `issue_*` helpers are final and tested; the input systems have their
//! signatures and a description, bodies are B's.
// M2-B: remove this allow once order_mode_keys and issue_pointer_orders call
// ground_hit and the issue_* helpers.
#![allow(dead_code)]

use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use sim::{Command, FxVec2};

use crate::camera::RtsCamera;
use crate::hud::PointerOverUi;
use crate::present::sim_from_world;
use crate::selection::{RingAssets, Selection};
use crate::sim_driver::PendingCommands;

/// What the next left click on the ground means.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderMode {
    /// Left click selects, right click moves.
    #[default]
    Normal,
    /// After A: the next left click on the ground is an `AttackMove`; Esc or
    /// a right click cancels back to `Normal`.
    AttackMove,
}

/// Where the cursor ray meets the ground plane y = 0, in world metres, or
/// `None` when the ray misses (looking at the sky) or the camera has no
/// viewport yet.
pub fn ground_hit(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    cursor: Vec2,
) -> Option<Vec3> {
    let ray = camera.viewport_to_world(camera_transform, cursor).ok()?;
    let t = ray.intersect_plane(Vec3::ZERO, InfinitePlane3d::new(Dir3::Y))?;
    Some(ray.get_point(t))
}

/// Queue a Move for the selection; returns the seq, or `None` when nothing
/// is selected (no command is queued).
pub fn issue_move(
    pending: &mut PendingCommands,
    selection: &Selection,
    target: FxVec2,
    queue: bool,
) -> Option<u32> {
    if selection.is_empty() {
        return None;
    }
    Some(pending.push_local(Command::Move {
        units: selection.units.iter().copied().collect(),
        target,
        queue,
    }))
}

/// Queue a Stop for the selection.
pub fn issue_stop(pending: &mut PendingCommands, selection: &Selection) -> Option<u32> {
    if selection.is_empty() {
        return None;
    }
    Some(pending.push_local(Command::Stop {
        units: selection.units.iter().copied().collect(),
    }))
}

/// Queue an `AttackMove` for the selection (applied like `Move` until M4a).
pub fn issue_attack_move(
    pending: &mut PendingCommands,
    selection: &Selection,
    target: FxVec2,
) -> Option<u32> {
    if selection.is_empty() {
        return None;
    }
    Some(pending.push_local(Command::AttackMove {
        units: selection.units.iter().copied().collect(),
        target,
    }))
}

/// Right click, S and A handling.
pub struct OrdersPlugin;

impl Plugin for OrdersPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OrderMode>()
            .add_systems(Update, (order_mode_keys, issue_pointer_orders).chain());
    }
}

/// A enters [`OrderMode::AttackMove`] when something is selected; Esc or
/// right click returns to `Normal`; S issues a Stop (`issue_stop`).
///
/// M2-B: body to implement.
pub fn order_mode_keys(
    _keys: Res<ButtonInput<KeyCode>>,
    _buttons: Res<ButtonInput<MouseButton>>,
    _mode: ResMut<OrderMode>,
    _selection: Res<Selection>,
    _pending: ResMut<PendingCommands>,
) {
    // M2-B: see the doc comment.
}

/// On right-button release (Normal) or left-button release (`AttackMove`),
/// not over UI: `ground_hit` at the cursor, `sim_from_world`, then
/// `issue_move(.., queue = shift held)` or `issue_attack_move`; on success
/// `selection::spawn_move_marker` at the hit and the mode returns to
/// `Normal`. A right click in `AttackMove` mode only cancels the mode.
///
/// M2-B: body to implement.
#[allow(clippy::too_many_arguments)] // Bevy system: each parameter is one resource or query
pub fn issue_pointer_orders(
    _buttons: Res<ButtonInput<MouseButton>>,
    _keys: Res<ButtonInput<KeyCode>>,
    _windows: Query<&Window, With<PrimaryWindow>>,
    _camera: Query<(&Camera, &GlobalTransform), With<RtsCamera>>,
    _over_ui: Res<PointerOverUi>,
    _mode: ResMut<OrderMode>,
    _selection: Res<Selection>,
    _pending: ResMut<PendingCommands>,
    _ring_assets: Option<Res<RingAssets>>,
    _time: Res<Time>,
    _commands: Commands,
) {
    // M2-B: see the doc comment. Target conversion: `sim_from_world(hit)`.
    let _ = sim_from_world;
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::UnitId;

    fn selection(n: u32) -> Selection {
        let mut s = Selection::default();
        s.set((1..=n).map(UnitId));
        s
    }

    #[test]
    fn orders_go_through_pending_commands_with_the_selection() {
        let mut pending = PendingCommands::default();
        let empty = Selection::default();
        assert_eq!(issue_move(&mut pending, &empty, FxVec2::ZERO, false), None);
        assert!(pending.is_empty());
        let sel = selection(3);
        let target = FxVec2::from_ints(10, 20);
        assert_eq!(issue_move(&mut pending, &sel, target, true), Some(0));
        assert_eq!(issue_stop(&mut pending, &sel), Some(1));
        assert_eq!(issue_attack_move(&mut pending, &sel, target), Some(2));
        let cmds = pending.drain_sorted();
        assert_eq!(cmds.len(), 3);
        assert_eq!(
            cmds[0].cmd,
            Command::Move {
                units: vec![UnitId(1), UnitId(2), UnitId(3)],
                target,
                queue: true
            }
        );
        assert!(matches!(cmds[1].cmd, Command::Stop { .. }));
        assert!(matches!(cmds[2].cmd, Command::AttackMove { .. }));
    }

    #[test]
    fn ground_hit_solves_the_y0_plane() {
        // A camera 10 m up looking straight down at the origin with a
        // known viewport: the viewport centre hits the origin, a corner
        // misses nothing but lands away from it; a camera looking up misses.
        let mut camera = Camera::default();
        // Fake the computed viewport so viewport_to_world has a size.
        camera.computed.target_info = Some(bevy::camera::RenderTargetInfo {
            physical_size: UVec2::new(800, 600),
            scale_factor: 1.0,
        });
        let projection = Projection::Perspective(PerspectiveProjection::default());
        camera.computed.clip_from_view = projection.get_clip_from_view();
        let down = GlobalTransform::from(
            Transform::from_xyz(0.0, 10.0, 0.0).looking_at(Vec3::ZERO, Vec3::NEG_Z),
        );
        let hit = ground_hit(&camera, &down, Vec2::new(400.0, 300.0)).expect("centre hits");
        assert!(hit.length() < 1e-3, "{hit}");
        assert!(hit.y.abs() < 1e-4);
        let corner = ground_hit(&camera, &down, Vec2::new(0.0, 0.0)).expect("corner hits");
        assert!(corner.length() > 1.0);
        assert!(corner.y.abs() < 1e-3);
        let up = GlobalTransform::from(
            Transform::from_xyz(0.0, 10.0, 0.0).looking_at(Vec3::new(0.0, 20.0, 0.0), Vec3::Z),
        );
        assert_eq!(ground_hit(&camera, &up, Vec2::new(400.0, 300.0)), None);
    }
}
