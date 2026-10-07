//! The macOS application menu (M2 decision 8, implementer rule 12).
//!
//! winit's default menu has a Quit item that calls `NSApp terminate:`;
//! Bevy's runner then returns without running any schedule, so no
//! `AppExit` is written and the replay never gets its clean-exit trailer.
//! This module installs a muda 0.21 menu instead: an application submenu
//! with About / Services / Hide / Hide Others / Show All and a CUSTOM
//! `MenuItem` "Quit Eonmark" (Cmd+Q, id [`QUIT_ID`]). The item only emits a
//! `MenuEvent`; [`drain_menu_events`] turns it into `AppExit::Success`,
//! which flows through `Last` (`sim_driver::finish_recorder_on_exit`) like
//! the red close button does (`bevy_window::exit_on_all_closed`, also in
//! `Last`, from `WindowPlugin`'s default `ExitCondition::OnAllClosed`).
//!
//! Never `PredefinedMenuItem::quit`: it also calls `terminate:` directly.
//! Both systems take `NonSendMarker` so Bevy runs them on the `AppKit` main
//! thread. The menu is built in Startup, not at plugin build time, because
//! winit installs its own menu in `applicationDidFinishLaunching`, which
//! runs after plugin construction and before the first schedule; building
//! ours afterwards is what replaces it.
//!
//! Automated proxy for the owner's Cmd-Q check (synthetic key presses are
//! not available to the implementer): `--quit-via-menu-after-seconds <s>`
//! (dev builds) makes [`drain_menu_events`] hand a `MenuEvent` carrying the
//! Quit id to the same handler a real click or Cmd-Q reaches. muda's
//! `MenuEvent::send` is `pub(crate)`, so the event cannot be pushed into
//! muda's own channel; everything after the channel (id match, `AppExit`,
//! recorder finish, exit code) is shared with the real path.

use std::time::Duration;

use bevy::app::AppExit;
use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{AboutMetadata, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};

/// Id of the custom Quit item (the only id this module reacts to).
pub const QUIT_ID: &str = "quit";
/// Label of the custom Quit item.
pub const QUIT_LABEL: &str = "Quit Eonmark";
/// Application name shown in the About panel and the app submenu.
pub const APP_NAME: &str = "Eonmark";
/// Copyright line of the About panel. Mirrors `NSHumanReadableCopyright`
/// in `scripts/bundle.sh` (`docs/design/macos-packaging.md`); update both.
pub const COPYRIGHT: &str = "Copyright 2026 Toni Anev and Eonmark contributors. MIT OR Apache-2.0.";

/// The installed menu, kept alive for the app's lifetime. Non-send: muda
/// menus are `Rc`-based `AppKit` objects.
pub struct MacosMenu {
    /// `None` until [`install_menu`] ran.
    pub menu: Option<Menu>,
    /// The id the Quit item fires.
    pub quit_id: MenuId,
}

impl Default for MacosMenu {
    fn default() -> Self {
        Self {
            menu: None,
            quit_id: MenuId::new(QUIT_ID),
        }
    }
}

/// `--quit-via-menu-after-seconds`: fire a synthetic Quit menu event once
/// `after` has elapsed on the real clock.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyntheticQuit {
    /// Wall-clock delay since app start.
    pub after: Duration,
    /// Set once the event was handed to the handler (exactly once).
    pub fired: bool,
}

/// Installs the menu and routes Quit to `AppExit`.
#[derive(Default)]
pub struct MacosMenuPlugin {
    /// `Some(delay)` installs the [`SyntheticQuit`] proxy (dev builds only;
    /// release builds print a note and ignore it).
    pub quit_via_menu_after: Option<Duration>,
}

impl Plugin for MacosMenuPlugin {
    fn build(&self, app: &mut App) {
        app.world_mut().insert_non_send(MacosMenu::default());
        app.add_systems(Startup, install_menu)
            .add_systems(Update, drain_menu_events);
        if let Some(after) = self.quit_via_menu_after {
            #[cfg(feature = "dev")]
            app.insert_resource(SyntheticQuit {
                after,
                fired: false,
            });
            #[cfg(not(feature = "dev"))]
            eprintln!(
                "eonmark: --quit-via-menu-after-seconds {} is a dev-build flag; ignored",
                after.as_secs_f64()
            );
        }
    }
}

/// Build the menu bar: one application submenu with the predefined items
/// and the custom Quit item. Returns the menu and the Quit item's id.
pub fn build_menu() -> muda::Result<(Menu, MenuId)> {
    let about = AboutMetadata {
        name: Some(APP_NAME.to_string()),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
        copyright: Some(COPYRIGHT.to_string()),
        // muda shows `website` and `license` on Windows and GTK only; set
        // for completeness so a port needs no change here.
        website: Some(env!("CARGO_PKG_REPOSITORY").to_string()),
        website_label: Some("Source on GitHub".to_string()),
        license: Some("MIT OR Apache-2.0".to_string()),
        ..Default::default()
    };
    let quit = MenuItem::with_id(
        QUIT_ID,
        QUIT_LABEL,
        true,
        Some(Accelerator::new(Modifiers::META, Code::KeyQ)),
    );
    let app_menu = Submenu::with_items(
        APP_NAME,
        true,
        &[
            &PredefinedMenuItem::about(None, Some(about)),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::services(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::show_all(None),
            &PredefinedMenuItem::separator(),
            &quit,
        ],
    )?;
    let menu = Menu::with_items(&[&app_menu])?;
    Ok((menu, quit.id().clone()))
}

/// Startup, main thread: build the menu and `init_for_nsapp()`.
pub fn install_menu(_main_thread: NonSendMarker, mut state: NonSendMut<MacosMenu>) {
    match build_menu() {
        Ok((menu, quit_id)) => {
            menu.init_for_nsapp();
            state.quit_id = quit_id;
            state.menu = Some(menu);
            info!("macOS menu installed; Cmd-Q routes through AppExit");
        }
        Err(e) => eprintln!("eonmark: cannot install the macOS menu: {e}"),
    }
}

/// `true` when `event` is the custom Quit item. The only decision this
/// module makes about a menu event.
pub fn is_quit(event: &MenuEvent, quit_id: &MenuId) -> bool {
    event.id == *quit_id
}

/// The event the Quit item emits, built by hand for the proxy.
pub fn synthetic_quit_event(quit_id: &MenuId) -> MenuEvent {
    MenuEvent {
        id: quit_id.clone(),
    }
}

/// Shared tail of the real and the synthetic path: the Quit id writes
/// `AppExit::Success`; every other id is ignored. Returns whether it quit.
fn handle_menu_event(
    event: &MenuEvent,
    quit_id: &MenuId,
    exit: &mut MessageWriter<AppExit>,
    origin: &str,
) -> bool {
    if is_quit(event, quit_id) {
        info!("{QUIT_LABEL} ({origin}) -> AppExit::Success");
        exit.write(AppExit::Success);
        true
    } else {
        false
    }
}

/// Update, main thread: drain `MenuEvent::receiver().try_recv()`; the Quit
/// id writes `AppExit::Success`. With [`SyntheticQuit`] present, a Quit
/// event is synthesised once `after` has elapsed and takes the same path.
pub fn drain_menu_events(
    _main_thread: NonSendMarker,
    state: NonSend<MacosMenu>,
    mut exit: MessageWriter<AppExit>,
    time: Res<Time<Real>>,
    synthetic: Option<ResMut<SyntheticQuit>>,
) {
    while let Ok(event) = MenuEvent::receiver().try_recv() {
        handle_menu_event(&event, &state.quit_id, &mut exit, "menu");
    }
    if let Some(mut synthetic) = synthetic
        && !synthetic.fired
        && time.elapsed() >= synthetic.after
    {
        synthetic.fired = true;
        let event = synthetic_quit_event(&state.quit_id);
        println!(
            "quit-via-menu: firing {QUIT_ID:?} after {:.2} s",
            time.elapsed().as_secs_f64()
        );
        handle_menu_event(&event, &state.quit_id, &mut exit, "synthetic");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quit_id_is_stable() {
        let state = MacosMenu::default();
        assert_eq!(state.quit_id, MenuId::new(QUIT_ID));
        assert!(state.menu.is_none());
    }

    #[test]
    fn only_the_quit_id_quits() {
        let quit_id = MenuId::new(QUIT_ID);
        assert!(is_quit(&synthetic_quit_event(&quit_id), &quit_id));
        assert!(!is_quit(
            &MenuEvent {
                id: MenuId::new("about")
            },
            &quit_id
        ));
    }

    #[test]
    fn plugin_default_has_no_proxy() {
        assert!(MacosMenuPlugin::default().quit_via_menu_after.is_none());
        let proxy = SyntheticQuit {
            after: Duration::from_secs(3),
            fired: false,
        };
        assert!(!proxy.fired);
    }
}
