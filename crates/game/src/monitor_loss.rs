//! Keep the window open when its monitor disappears (display sleep, screen
//! lock, an unplugged display).
//!
//! The chain in Bevy 0.19.1 that ends the session:
//! - winit stops listing the monitor, and `bevy_winit::system::create_monitors`
//!   (run from `about_to_wait`) despawns its `Monitor` entity.
//! - `Monitor` requires `HasWindows`, the relationship target of the
//!   window's `OnMonitor` (which `bevy_winit`'s `changed_windows`
//!   inserts), and `HasWindows` is declared `linked_spawn`: its
//!   `on_despawn` hook queues `try_despawn` for every window in it.
//! - With the window entity gone, `exit_on_all_closed` (in `Last`) writes
//!   `AppExit::Success`, although the OS window was never closed.
//!
//! [`detach_windows_from_lost_monitor`] breaks the chain without patching
//! Bevy. `bevy_ecs` 0.19.1 (`EntityWorldMut::despawn_no_free_with_caller`)
//! triggers `Despawn` observers before the `on_despawn` hooks, so the
//! observer empties `HasWindows` while the monitor still exists; the hook
//! then finds no windows to despawn. It also queues the removal of each
//! window's `OnMonitor`: `changed_windows` only replaces a link whose
//! target it still lists and only drops one while winit reports no current
//! monitor, so a link to a dead entity would keep the window off the
//! monitor once it is back. Removing the link runs `OnMonitor`'s
//! `on_discard` hook against a target that no longer exists, which is a
//! no-op. When the display wakes, winit lists the monitor again,
//! `create_monitors` spawns a new `Monitor`, and the next `Window` change
//! links the window to it (`changed_windows`' no-`OnMonitor` branch).
//!
//! Every window is detached, not only the primary one: an OS window whose
//! monitor went away is still open, so despawning its entity is never what
//! the game wants.
//!
//! The other option, keeping `OnMonitor` off the window altogether, would
//! have to undo `changed_windows` on every `Window` change (cursor moves,
//! focus, resizes), since its no-`OnMonitor` branch inserts the link again,
//! and nothing could read which monitor the window is on.

use bevy::prelude::*;
use bevy::window::{HasWindows, OnMonitor};

/// Adds [`detach_windows_from_lost_monitor`].
pub struct MonitorLossPlugin;

impl Plugin for MonitorLossPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(detach_windows_from_lost_monitor);
    }
}

/// On a monitor's despawn, before `HasWindows`' linked-spawn hook runs: take
/// its windows out of `HasWindows` and queue the removal of their
/// `OnMonitor`, so they outlive the monitor.
pub fn detach_windows_from_lost_monitor(
    despawn: On<Despawn, HasWindows>,
    mut monitors: Query<&mut HasWindows>,
    mut commands: Commands,
) {
    let monitor = despawn.entity;
    let Ok(mut has_windows) = monitors.get_mut(monitor) else {
        return;
    };
    let windows = std::mem::take(&mut *has_windows);
    for window in windows.iter() {
        info!("monitor {monitor} removed; keeping window {window} open");
        commands.entity(window).try_remove::<OnMonitor>();
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::SystemState;
    use bevy::window::{Monitor, PrimaryWindow, WindowCloseRequested};

    use super::*;

    fn test_monitor() -> Monitor {
        Monitor {
            name: Some("test".into()),
            physical_height: 1080,
            physical_width: 1920,
            physical_position: IVec2::ZERO,
            refresh_rate_millihertz: Some(60_000),
            scale_factor: 1.0,
            video_modes: Vec::new(),
        }
    }

    /// An app with Bevy's `WindowPlugin` (the primary window and
    /// `exit_on_all_closed`) whose primary window is on a monitor; returns
    /// the app, the window and the monitor.
    fn app_with_window_on_monitor(detach: bool) -> (App, Entity, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::window::WindowPlugin::default()));
        if detach {
            app.add_plugins(MonitorLossPlugin);
        }
        let world = app.world_mut();
        let window = world
            .query_filtered::<Entity, With<PrimaryWindow>>()
            .single(world)
            .unwrap();
        let monitor = world.spawn(test_monitor()).id();
        // As `bevy_winit`'s `changed_windows` does once the window exists.
        world.entity_mut(window).insert(OnMonitor(monitor));
        app.update();
        assert!(app.should_exit().is_none());
        (app, window, monitor)
    }

    /// Despawn `monitor` the way `bevy_winit`'s `about_to_wait` does: a
    /// `despawn` command from a `SystemState`'s `Commands`, then `apply`,
    /// outside any schedule; then run one frame so `exit_on_all_closed`
    /// sees the result.
    fn remove_monitor(app: &mut App, monitor: Entity) {
        let world = app.world_mut();
        let mut state = SystemState::<Commands>::new(world);
        state.get_mut(world).unwrap().entity(monitor).despawn();
        state.apply(world);
        app.update();
    }

    #[test]
    fn window_survives_its_monitor() {
        let (mut app, window, monitor) = app_with_window_on_monitor(true);
        let world = app.world();
        assert_eq!(
            world.get::<HasWindows>(monitor).unwrap().iter().next(),
            Some(window)
        );

        remove_monitor(&mut app, monitor);

        let world = app.world();
        assert!(world.get_entity(monitor).is_err(), "monitor despawned");
        let window_ref = world.get_entity(window).expect("window entity survives");
        assert!(window_ref.contains::<Window>());
        assert!(
            !window_ref.contains::<OnMonitor>(),
            "the link to the dead monitor is removed"
        );
        assert_eq!(app.should_exit(), None);

        // Wake, then sleep again: the monitor comes back as a new entity,
        // the window is linked to it (standing in for `changed_windows`'
        // no-`OnMonitor` branch, whose precondition is asserted above), and
        // the second loss is survived the same way.
        let world = app.world_mut();
        let monitor = world.spawn(test_monitor()).id();
        world.entity_mut(window).insert(OnMonitor(monitor));
        app.update();
        remove_monitor(&mut app, monitor);
        let window_ref = app
            .world()
            .get_entity(window)
            .expect("window survives a second loss");
        assert!(!window_ref.contains::<OnMonitor>());
        assert_eq!(app.should_exit(), None);
    }

    #[test]
    fn every_window_on_the_lost_monitor_survives_and_others_keep_their_link() {
        let (mut app, primary, monitor) = app_with_window_on_monitor(true);
        let world = app.world_mut();
        let secondary = world.spawn((Window::default(), OnMonitor(monitor))).id();
        let other = world.spawn(test_monitor()).id();
        let elsewhere = world.spawn((Window::default(), OnMonitor(other))).id();
        app.update();
        assert_eq!(app.world().get::<HasWindows>(monitor).unwrap().len(), 2);

        remove_monitor(&mut app, monitor);

        let world = app.world();
        for window in [primary, secondary] {
            let window_ref = world
                .get_entity(window)
                .expect("every window on the lost monitor survives");
            assert!(!window_ref.contains::<OnMonitor>());
        }
        assert_eq!(world.get::<OnMonitor>(elsewhere).map(|l| l.0), Some(other));
        assert_eq!(app.should_exit(), None);
    }

    /// The plugin must not keep the app alive when the window really
    /// closes: `close_when_requested` marks it `ClosingWindow` in one frame
    /// and despawns it in the next, then `exit_on_all_closed` exits.
    #[test]
    fn closing_the_window_still_exits() {
        let (mut app, window, monitor) = app_with_window_on_monitor(true);
        app.world_mut()
            .write_message(WindowCloseRequested { window });
        let mut exit = None;
        for _ in 0..3 {
            app.update();
            exit = app.should_exit();
            if exit.is_some() {
                break;
            }
        }
        assert_eq!(exit, Some(AppExit::Success));
        assert!(app.world().get_entity(window).is_err());
        assert!(
            app.world().get_entity(monitor).is_ok(),
            "closing a window leaves its monitor"
        );
    }

    /// Positive control: without the plugin, the same despawn takes the
    /// window with it and the app exits, which is the bug.
    #[test]
    fn without_the_plugin_the_monitor_takes_the_window() {
        let (mut app, window, monitor) = app_with_window_on_monitor(false);

        remove_monitor(&mut app, monitor);

        assert!(app.world().get_entity(window).is_err());
        assert_eq!(app.should_exit(), Some(AppExit::Success));
    }
}
