//! Background mode for automated windowed runs (`--background`, or
//! `EONMARK_BACKGROUND=1`, see [`crate::cli::BACKGROUND_ENV`]): scripts and
//! agents run the real window for their checks, and without this it pops in
//! front of whatever the owner is working on.
//!
//! What it changes:
//! - The primary window is created unfocused (`Window::focused = false`,
//!   which `bevy_winit` 0.19.1 passes to winit's `with_active`), at
//!   `WindowLevel::AlwaysOnBottom` (winit: `kCGNormalWindowLevel - 1`, below
//!   every normal window) and at the top-left corner of the primary monitor
//!   (`WindowPosition::At(IVec2::ZERO)`, physical pixels).
//! - `WinitSettings::continuous()`: Bevy's default `WinitSettings::game()`
//!   throttles an unfocused window to `reactive_low_power` at 60 Hz, so
//!   without this a background window could never be faster than 16.7 ms a
//!   frame and the frame-time proxy would measure the throttle, not the
//!   frame. A focused playtest window runs `Continuous` either way.
//! - macOS: winit activates the app in `applicationDidFinishLaunching`
//!   (`activateIgnoringOtherApps`), before any Bevy system runs, and
//!   `bevy_winit` 0.19.1 exposes neither `with_activation_policy` nor
//!   `with_activate_ignoring_other_apps`. So a `Startup` system switches the
//!   activation policy to `Accessory` (no Dock icon, no menu bar) and, for
//!   the first [`HAND_BACK_SECS`], an `Update` system hands activation back
//!   to the application that was frontmost when the game started whenever
//!   the game finds itself active. All of these `AppKit` calls are safe in
//!   objc2-app-kit 0.3.2; no `unsafe` is needed.
//!
//! Background mode never stops rendering on its own, but macOS does when
//! the window is completely covered: wgpu-hal 29 checks
//! `NSWindow::occlusionState` before `nextDrawable` and skips the frame
//! while the window is occluded, so nothing is presented and a
//! `--screenshot` cannot be captured. The moment of being covered is also
//! where the one-off ~1 s frame of earlier windowed runs came from: `AppKit`
//! updates `occlusionState` some milliseconds after the window is covered,
//! a frame that acquires in that gap waits in `-[CAMetalLayer
//! nextDrawable]` until `CoreAnimation` gives up (1 s), and the 250 ms
//! `max_delta` clamp turns that into ~15 lost ticks. A window that came to
//! the front at launch and was then covered by the owner's next click hit
//! it about one session in three. The background window never comes to the
//! front and sits in a corner, so it is rarely covered; [`log_occlusion`]
//! prints every change so a run can tell, and `frame_stats` counts the
//! occluded frames. Manual playtests do not use this mode.

use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowLevel, WindowOccluded, WindowPosition};
use bevy::winit::WinitSettings;

/// How long after start-up the macOS hand-back keeps watching for the game
/// becoming the active app. winit's activation request is asynchronous, so
/// it can land a few frames after the first `Update`.
pub const HAND_BACK_SECS: f64 = 3.0;

/// Apply background mode to the primary window before it is created.
pub fn configure_window(window: &mut Window) {
    window.focused = false;
    window.window_level = WindowLevel::AlwaysOnBottom;
    window.position = WindowPosition::At(IVec2::ZERO);
}

/// Background mode: continuous updates, occlusion logging and, on macOS,
/// the activation-policy switch and focus hand-back. The window settings
/// themselves are applied with [`configure_window`] before `DefaultPlugins`.
pub struct BackgroundPlugin;

impl Plugin for BackgroundPlugin {
    fn build(&self, app: &mut App) {
        // Added after `DefaultPlugins`: `WinitPlugin` only `init_resource`s
        // its settings, so this replaces the `game()` default.
        app.insert_resource(WinitSettings::continuous())
            .add_systems(First, log_occlusion);
        #[cfg(target_os = "macos")]
        macos::build(app);
        info!("background mode: unfocused, always-on-bottom window in the top-left corner");
    }
}

/// Print the primary window's occlusion changes (`WindowOccluded`).
pub fn log_occlusion(
    mut messages: MessageReader<WindowOccluded>,
    primary: Query<(), With<PrimaryWindow>>,
    time: Res<Time<Real>>,
) {
    for msg in messages.read() {
        if !primary.contains(msg.window) {
            continue;
        }
        let secs = time.elapsed_secs_f64();
        if msg.occluded {
            warn!(
                "background: the window is fully covered at {secs:.2} s; macOS stops presenting its frames (no rendering, no screenshot) until part of it is visible"
            );
        } else {
            info!("background: the window is visible again at {secs:.2} s");
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use bevy::ecs::system::NonSendMarker;
    use bevy::prelude::*;
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy,
        NSRunningApplication, NSWorkspace,
    };

    use super::HAND_BACK_SECS;

    /// The application that was frontmost before the event loop started
    /// (and so before winit activated the game). `NSRunningApplication` is
    /// `Send + Sync` in objc2-app-kit 0.3.2.
    #[derive(Resource)]
    struct PreviousFrontmost(Option<Retained<NSRunningApplication>>);

    /// Hand-backs performed, for the log line.
    #[derive(Default)]
    struct HandBacks(u32);

    pub(super) fn build(app: &mut App) {
        // `Plugin::build` runs in `app::run`, before `App::run` starts the
        // NSApplication, so this is still the user's app.
        let previous = NSWorkspace::sharedWorkspace().frontmostApplication();
        if let Some(name) = previous.as_ref().and_then(|p| p.localizedName()) {
            info!("background: will hand focus back to {name}");
        }
        app.insert_resource(PreviousFrontmost(previous))
            .add_systems(Startup, become_accessory)
            .add_systems(Update, hand_focus_back);
    }

    /// `NSApplicationActivationPolicyAccessory`: no Dock icon, no menu bar,
    /// not in Cmd-Tab. Main thread only (`NonSendMarker`).
    fn become_accessory(_main_thread: NonSendMarker) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        if !app.setActivationPolicy(NSApplicationActivationPolicy::Accessory) {
            warn!("background: NSApplication refused the Accessory activation policy");
        }
    }

    /// For the first [`HAND_BACK_SECS`]: whenever the game is the active
    /// app, ask the previously frontmost app to activate (allowed under
    /// macOS 14+ cooperative activation because the game is active), or
    /// deactivate when nothing was frontmost.
    fn hand_focus_back(
        _main_thread: NonSendMarker,
        time: Res<Time<Real>>,
        previous: Res<PreviousFrontmost>,
        mut done: Local<HandBacks>,
    ) {
        if time.elapsed_secs_f64() > HAND_BACK_SECS {
            return;
        }
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        if !app.isActive() {
            return;
        }
        let accepted = match &previous.0 {
            Some(prev) => prev.activateWithOptions(NSApplicationActivationOptions::empty()),
            None => {
                app.deactivate();
                true
            }
        };
        done.0 += 1;
        if done.0 == 1 {
            info!(
                "background: handed focus back at {:.2} s (request accepted: {accepted})",
                time.elapsed_secs_f64()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_window_is_unfocused_bottom_and_in_the_corner() {
        let mut window = Window::default();
        assert!(window.focused, "Bevy's default window asks for focus");
        configure_window(&mut window);
        assert!(!window.focused);
        assert_eq!(window.window_level, WindowLevel::AlwaysOnBottom);
        assert_eq!(window.position, WindowPosition::At(IVec2::ZERO));
    }
}
