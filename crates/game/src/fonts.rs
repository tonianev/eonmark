//! The UI font (art pass slice 5, owner decision 2): Inter 4.1
//! (`assets/fonts/InterVariable.ttf`, OFL-1.1, its licence beside it as
//! `assets/fonts/OFL-Inter.txt`) replaces Bevy's built-in Fira Mono subset
//! as the font every `TextFont` without a handle draws with.
//!
//! [`install_ui_font`] runs in `Startup`: it reads the file with `std::fs`
//! (no asset load, so nothing waits on the asset server) and inserts it
//! into `Assets<Font>` at `AssetId::default()`, the id `bevy_text`'s
//! `TextPlugin` put Fira Mono at and the id `TextFont::default()`'s handle
//! points to. Bevy 0.19.1's `load_font_assets_into_font_collection`
//! (`PostUpdate`) registers each font id once, in a `Local` set, so the
//! replacement must land before the first `PostUpdate`; `Startup` does
//! (`install_ui_font_replaces_the_default_before_the_first_post_update`).
//! A missing or unreadable file is a `warn!` and Fira Mono stays, so the
//! game still shows text.
//!
//! The HUD's fit tests measure text with Inter's real advance widths (the
//! test-only `measure` module, `skrifa` 0.42.1, the version parley
//! already locks): the sum of the nominal advances at the default
//! instance (weight 400, the weight `TextFont::default()` asks for), with
//! no kerning, which in Inter narrows far more pairs than it widens.

use std::path::{Path, PathBuf};

use bevy::prelude::*;

/// The UI font under the assets directory.
pub const UI_FONT_FILE: &str = "fonts/InterVariable.ttf";

/// Where [`install_ui_font`] reads the UI font from.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct UiFontFile(pub PathBuf);

/// Installs Inter as the default font in `Startup`.
pub struct FontsPlugin {
    /// The resolved assets directory (`Cli::resolve_assets_dir`).
    pub assets: PathBuf,
}

impl Plugin for FontsPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(UiFontFile(ui_font_path(&self.assets)))
            .add_systems(Startup, install_ui_font);
    }
}

/// The UI font's path under `assets`.
pub fn ui_font_path(assets: &Path) -> PathBuf {
    assets.join(UI_FONT_FILE)
}

/// Read [`UiFontFile`] and put it in `Assets<Font>` at `AssetId::default()`
/// in place of Bevy's Fira Mono. Logs `ui font: <path>` once installed, or
/// warns and keeps Fira Mono when the file cannot be read.
pub fn install_ui_font(file: Res<UiFontFile>, fonts: Option<ResMut<Assets<Font>>>) {
    let Some(mut fonts) = fonts else {
        return;
    };
    let path = file.0.display();
    match std::fs::read(&file.0) {
        Ok(bytes) => match fonts.insert(AssetId::default(), Font::from_bytes(bytes)) {
            Ok(()) => info!("ui font: {path}"),
            Err(err) => warn!("ui font {path}: {err}; keeping Bevy's built-in FiraMono"),
        },
        Err(err) => warn!("ui font {path}: {err}; keeping Bevy's built-in FiraMono"),
    }
}

/// Text widths with Inter's advances, for the HUD fit tests.
#[cfg(test)]
pub(crate) mod measure {
    use std::sync::OnceLock;

    use skrifa::instance::{LocationRef, Size};
    use skrifa::{FontRef, MetadataProvider};

    /// The repository's `assets/fonts/InterVariable.ttf`, read once.
    pub fn inter() -> &'static [u8] {
        static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
        BYTES.get_or_init(|| {
            let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../assets")
                .join(super::UI_FONT_FILE);
            std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        })
    }

    /// The width of `text` on one line at `font_px`, logical px: the sum of
    /// Inter's advance widths at its default instance. Panics on a
    /// character Inter has no glyph for (it would draw as a fallback).
    pub fn text_width_px(text: &str, font_px: f32) -> f32 {
        let font = FontRef::new(inter()).expect("Inter parses");
        let charmap = font.charmap();
        let metrics = font.glyph_metrics(Size::new(font_px), LocationRef::default());
        text.chars()
            .map(|c| {
                let glyph = charmap
                    .map(c)
                    .unwrap_or_else(|| panic!("Inter has no glyph for {c:?} in {text:?}"));
                metrics
                    .advance_width(glyph)
                    .unwrap_or_else(|| panic!("no advance for {c:?}"))
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::measure::text_width_px;
    use super::*;
    use bevy::asset::AssetPlugin;
    use bevy::text::{FontCx, load_font_assets_into_font_collection};

    /// An app with the asset server, `Assets<Font>` holding Fira Mono at the
    /// default id and `bevy_text`'s font registration in `PostUpdate`, as
    /// `TextPlugin` sets them up (without its layout and atlas systems).
    fn text_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, AssetPlugin::default()))
            .init_asset::<Font>()
            .init_resource::<FontCx>()
            .add_systems(PostUpdate, load_font_assets_into_font_collection);
        app.world_mut()
            .resource_mut::<Assets<Font>>()
            .insert(
                AssetId::default(),
                Font::from_bytes(bevy::text::DEFAULT_FONT_DATA.to_vec()),
            )
            .unwrap();
        app
    }

    fn has_family(app: &mut App, name: &str) -> bool {
        app.world_mut()
            .resource_mut::<FontCx>()
            .collection
            .family_id(name)
            .is_some()
    }

    fn repo_assets() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets")
    }

    /// One update: `Startup` puts Inter at the default id before the
    /// first `PostUpdate` registers it, so the default family is Inter and
    /// Fira Mono is never registered. Without the plugin (the positive
    /// control) Fira Mono is.
    #[test]
    fn install_ui_font_replaces_the_default_before_the_first_post_update() {
        let mut control = text_app();
        control.update();
        assert!(has_family(&mut control, "Fira Mono"), "the control");
        assert!(!has_family(&mut control, "Inter Variable"));

        let mut app = text_app();
        app.add_plugins(FontsPlugin {
            assets: repo_assets(),
        });
        app.update();
        assert!(has_family(&mut app, "Inter Variable"));
        assert!(!has_family(&mut app, "Fira Mono"));
        let fonts = app.world().resource::<Assets<Font>>();
        let installed = fonts.get(AssetId::default()).expect("the default font");
        assert_eq!(installed.data.data(), measure::inter());
    }

    /// A missing file keeps Fira Mono (a `warn!`, no panic).
    #[test]
    fn a_missing_font_file_keeps_fira_mono() {
        let mut app = text_app();
        app.add_plugins(FontsPlugin {
            assets: PathBuf::from("/nonexistent/eonmark-assets"),
        });
        app.update();
        assert!(has_family(&mut app, "Fira Mono"));
        assert!(!has_family(&mut app, "Inter Variable"));
    }

    /// The measure is Inter's, not a monospace one: a narrow glyph is
    /// narrower than a wide one, a space adds width, and the HUD's longest
    /// lines (`command_card::INFO_WIDTH_PX`, `CARD_BUTTON_PX`) fit with
    /// room to spare. The fit tests in `command_card` and `age_hud` use
    /// this measure.
    #[test]
    fn hud_text_fits_with_inter_advances() {
        let (i, w) = (text_width_px("i", 14.0), text_width_px("W", 14.0));
        assert!(i > 0.0 && w > 2.0 * i, "i={i} W={w}");
        assert!(text_width_px("a b", 14.0) > text_width_px("ab", 14.0));
        // Twice the size, twice the width.
        let one = text_width_px("Lumber", 12.0);
        assert!((text_width_px("Lumber", 24.0) - 2.0 * one).abs() < 1e-3);
        let queue = "Queue: Statecraft III 99%, Letters III, Trade III, Arms III, Scribe";
        let panel = text_width_px(queue, crate::hud::HUD_FONT_PX);
        assert!(
            panel < crate::command_card::INFO_WIDTH_PX,
            "{panel} px of {}",
            crate::command_card::INFO_WIDTH_PX
        );
        let button = text_width_px("Needs Statecraft II", crate::command_card::CARD_FONT_PX);
        assert!(
            button < crate::command_card::CARD_BUTTON_PX.x,
            "{button} px"
        );
    }
}
