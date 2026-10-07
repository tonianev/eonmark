//! The macOS application menu (M2 decision 8, implementer rule 12).
//!
//! winit's default menu has a Quit item that calls `NSApp terminate:`;
//! Bevy's runner then returns without running any schedule, so no
//! `AppExit` is written and the replay never gets its clean-exit trailer.
//! This module installs a muda 0.21 menu instead: an application submenu
//! with About / Services / Hide / Hide Others / Show All and a CUSTOM
//! `MenuItem` "Quit Eonmark" (Cmd+Q). The item only emits a `MenuEvent`;
//! [`drain_menu_events`] turns it into `AppExit::Success`, which flows
//! through `Last` (recorder finish) like the close button does.
//!
//! Never `PredefinedMenuItem::quit`: it also calls `terminate:` directly.
//! Both systems take `NonSendMarker` so Bevy runs them on the `AppKit` main
//! thread. The menu is built in Startup, not at plugin build time, because
//! winit installs its own menu in `applicationDidFinishLaunching`, which
//! runs after plugin construction and before the first schedule.
//!
//! Ownership (M2 contract): agent C owns this file (About metadata,
//! PLAYTEST.md Cmd-Q section, design doc updates). The bodies here are the
//! minimal working version the skeleton needs; C verifies them against a
//! running app and the kill -9 / Cmd-Q replay checks with A.

use bevy::app::AppExit;
use bevy::ecs::system::NonSendMarker;
use bevy::prelude::*;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{AboutMetadata, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};

/// Id of the custom Quit item.
pub const QUIT_ID: &str = "quit";
/// Label of the custom Quit item.
pub const QUIT_LABEL: &str = "Quit Eonmark";
/// Application name shown in the About panel and the app submenu.
pub const APP_NAME: &str = "Eonmark";

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

/// Installs the menu and routes Quit to `AppExit`.
pub struct MacosMenuPlugin;

impl Plugin for MacosMenuPlugin {
    fn build(&self, app: &mut App) {
        app.world_mut().insert_non_send(MacosMenu::default());
        app.add_systems(Startup, install_menu)
            .add_systems(Update, drain_menu_events);
    }
}

/// Build the menu bar: one application submenu with the predefined items
/// and the custom Quit item. Returns the menu and the Quit item's id.
pub fn build_menu() -> muda::Result<(Menu, MenuId)> {
    let about = AboutMetadata {
        name: Some(APP_NAME.to_string()),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
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

/// Update, main thread: drain `MenuEvent::receiver().try_recv()`; the Quit
/// id writes `AppExit::Success`.
pub fn drain_menu_events(
    _main_thread: NonSendMarker,
    state: NonSend<MacosMenu>,
    mut exit: MessageWriter<AppExit>,
) {
    while let Ok(event) = MenuEvent::receiver().try_recv() {
        if event.id == state.quit_id {
            info!("Quit Eonmark (Cmd-Q) -> AppExit::Success");
            exit.write(AppExit::Success);
        }
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
}
