//! The main menu (M5b owner decision 5): a full-window panel over the
//! not-yet-started match with the title, the opponent and map line, the
//! match's seed, and the New Game and Quit buttons in the command card's
//! style. It is spawned on entering `AppState::Menu` with
//! `DespawnOnExit(AppState::Menu)`, so New Game removes it.
//!
//! Clicks go through `Pointer<Click>` observers, as the HUD's Stop button
//! does (`hud::on_stop_click`): `Interaction` only drives the buttons'
//! colours. New Game sets `NextState(AppState::Skirmish)`; Quit writes
//! `AppExit::Success`, and since nothing has stepped no replay is written.
//!
//! `--new-game-after-seconds <s>` ([`click_new_game_after`]) is the New
//! Game proxy for automated runs: after `s` seconds it moves a synthetic
//! mouse to the centre of the real button, presses and releases there
//! (`hud::SyntheticMouse`, the `hud_click` check's path through
//! `bevy_picking`), prints `new-game: clicked at (x, y) after <t> s` and
//! puts the real cursor back (`hud::RealCursor`).
//!
//! M6 (owner decision 6): the menu is the New Game selector too
//! (`super::new_game`): a Difficulty row (Easy, Standard, Hard) with the
//! chosen level's disclosed bonus under it, an Opponent style row
//! (Random, Balanced, Rush, Boom, Tower), the seed readout with New seed
//! (digits typed while the menu is open edit it, the first one replacing
//! the seed shown, Backspace deletes), and
//! the disclosure "The bot knows where your Seat is". New Game then asks
//! `match_scope` for the chosen match ([`PendingMatch::NewGame`]).

use std::time::Duration;

use bevy::app::AppExit;
use bevy::camera::RenderTarget;
use bevy::picking::events::{Click, Pointer};
use bevy::picking::pointer::{PointerButton, PointerInput};
use bevy::prelude::*;
use bevy::window::{PrimaryWindow, WindowRef};
use rules::strings::fill;

use super::AppState;
use crate::command_card::CARD_GAP_PX;
use crate::economy_hud::UiStrings;
use crate::hud::{
    HUD_CLICK_STEP_FRAMES, HUD_FONT_PX, PADDING_PX, RealCursor, SyntheticMouse, node_centre_logical,
};
use crate::outcome::TITLE_FONT_PX;
use crate::palette;
use crate::sim_driver::SimHandle;

/// The menu's z index: above the HUD panels, the tooltip and the outcome
/// overlay's 50.
pub const MENU_Z: i32 = 60;

/// The menu's full-window root.
#[derive(Component, Debug, Clone, Copy)]
pub struct MainMenu;

/// One of the menu's three text lines, by index: 0 title, 1 opponent and
/// map, 2 seed.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuLine(pub usize);

/// The menu's buttons.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuButton {
    /// Start the match against the Standard bot.
    NewGame,
    /// Quit the game.
    Quit,
    /// A fresh wall-clock seed (M6).
    NewSeed,
}

/// The New Game selector's option buttons (M6): an index into
/// `new_game::DIFFICULTIES` or `new_game::STYLES`.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MenuOption {
    /// A difficulty.
    Difficulty(usize),
    /// An opponent style.
    Style(usize),
}

/// The disclosed bonus caption under the Difficulty row (M6).
#[derive(Component, Debug, Clone, Copy)]
pub struct BonusCaption;

/// What the lifecycle should build next (M6), set by the menu's New Game
/// and the game-over screen's Play Again and Main Menu; taken by
/// `match_scope::apply_match_request`.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingMatch {
    /// New Game: the menu's settings at the menu's seed.
    NewGame,
    /// Play Again: the same settings at a fresh seed.
    PlayAgain,
    /// Main Menu: a fresh, unstarted match the menu shows.
    MainMenu,
}

/// The menu's three lines for a match on `map` with `seed`: "Eonmark",
/// "Skirmish against the Standard bot on `plains_1v1`", "Seed 1234".
pub fn menu_texts(map: &str, seed: u64, strings: &UiStrings) -> [String; 3] {
    [
        strings.hud("menu.title").to_string(),
        fill(strings.hud("menu.opponent"), &[("name", map)]),
        fill(strings.hud("menu.seed"), &[("seed", &seed.to_string())]),
    ]
}

/// A button's label: "New Game" or "Quit".
pub fn button_label(button: MenuButton, strings: &UiStrings) -> String {
    match button {
        MenuButton::NewGame => strings.hud("menu.new_game"),
        MenuButton::Quit => strings.hud("menu.quit"),
        MenuButton::NewSeed => strings.hud("menu.new_seed"),
    }
    .to_string()
}

fn text(value: &str, size: f32) -> impl Bundle {
    super::widgets::label(value, size)
}

/// `OnEnter(AppState::Menu)`: spawn the menu, a full-window column (all
/// four edges at 0 px) on the `palette::OUTCOME_SCRIM` slate at
/// [`MENU_Z`], with the three lines of [`menu_texts`] for the match already
/// built (its map and seed) and the two buttons in a row, each
/// `command_card::CARD_BUTTON_PX` like a card button. The root keeps the
/// `bevy_ui` picking default, so it blocks clicks on the world and the HUD
/// under it.
pub fn spawn_main_menu(
    mut commands: Commands,
    sim: NonSend<SimHandle>,
    strings: Option<Res<UiStrings>>,
    settings: Option<ResMut<super::new_game::NewGameSettings>>,
) {
    let Some(strings) = strings else {
        return;
    };
    // The seed shown is one set for the player: typing replaces it.
    if let Some(mut settings) = settings {
        settings.seed_typed = false;
    }
    let lines = menu_texts(&sim.setup().map, sim.seed(), &strings);
    commands
        .spawn((
            Name::new("Main menu"),
            MainMenu,
            DespawnOnExit(AppState::Menu),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(PADDING_PX),
                ..default()
            },
            BackgroundColor(palette::OUTCOME_SCRIM),
            GlobalZIndex(MENU_Z),
        ))
        .with_children(|menu| {
            for (i, line) in lines.iter().enumerate() {
                let size = if i == 0 { TITLE_FONT_PX } else { HUD_FONT_PX };
                menu.spawn((MenuLine(i), text(line, size)));
            }
            // M6: the New Game selector.
            let rules = sim.rules();
            super::widgets::option_row(
                menu,
                strings.hud("menu.difficulty"),
                HUD_FONT_PX,
                super::new_game::DIFFICULTIES
                    .iter()
                    .enumerate()
                    .map(|(i, id)| {
                        (
                            MenuOption::Difficulty(i),
                            super::new_game::difficulty_label(rules, id),
                        )
                    })
                    .collect(),
                on_option_click,
            );
            menu.spawn((BonusCaption, text("", HUD_FONT_PX)));
            super::widgets::option_row(
                menu,
                strings.hud("menu.style"),
                HUD_FONT_PX,
                super::new_game::STYLES
                    .iter()
                    .enumerate()
                    .map(|(i, id)| {
                        (
                            MenuOption::Style(i),
                            super::new_game::style_label(rules, id),
                        )
                    })
                    .collect(),
                on_option_click,
            );
            menu.spawn(text(strings.hud("menu.disclosure"), HUD_FONT_PX));
            menu.spawn((
                Name::new("Menu buttons"),
                Node {
                    flex_direction: FlexDirection::Row,
                    column_gap: Val::Px(PADDING_PX),
                    margin: UiRect::top(Val::Px(PADDING_PX + CARD_GAP_PX)),
                    ..default()
                },
                Pickable::IGNORE,
            ))
            .with_children(|row| {
                for button in [MenuButton::NewGame, MenuButton::NewSeed, MenuButton::Quit] {
                    let label = button_label(button, &strings);
                    let mut entity = super::widgets::spawn_button(
                        row,
                        format!("Menu button {label}"),
                        button,
                        text(&label, HUD_FONT_PX),
                    );
                    match button {
                        MenuButton::NewGame => entity.observe(on_new_game_click),
                        MenuButton::Quit => entity.observe(on_quit_click),
                        MenuButton::NewSeed => entity.observe(on_new_seed_click),
                    };
                }
            });
        });
}

/// A primary click on New Game: enter [`AppState::Skirmish`]; the sim
/// starts stepping, and the recorder starts, in that frame's fixed update.
/// M6: with the selector's settings present, the match is first rebuilt
/// from them ([`PendingMatch::NewGame`], applied on entering `Skirmish`).
pub fn on_new_game_click(
    click: On<Pointer<Click>>,
    mut next: ResMut<NextState<AppState>>,
    settings: Option<Res<super::new_game::NewGameSettings>>,
    mut commands: Commands,
) {
    if click.event().event.button != PointerButton::Primary {
        return;
    }
    info!("menu: New Game");
    if settings.is_some() {
        commands.insert_resource(PendingMatch::NewGame);
    }
    next.set(AppState::Skirmish);
}

/// A primary click on New seed: a fresh wall-clock seed.
pub fn on_new_seed_click(
    click: On<Pointer<Click>>,
    settings: Option<ResMut<super::new_game::NewGameSettings>>,
) {
    if click.event().event.button != PointerButton::Primary {
        return;
    }
    if let Some(mut settings) = settings {
        settings.offer_seed(crate::app::fresh_seed());
    }
}

/// A primary click on a difficulty or style option.
pub fn on_option_click(
    click: On<Pointer<Click>>,
    options: Query<&MenuOption>,
    settings: Option<ResMut<super::new_game::NewGameSettings>>,
) {
    if click.event().event.button != PointerButton::Primary {
        return;
    }
    let (Ok(option), Some(mut settings)) = (options.get(click.event().entity), settings) else {
        return;
    };
    match *option {
        MenuOption::Difficulty(i) => {
            if let Some(id) = super::new_game::DIFFICULTIES.get(i) {
                settings.difficulty = (*id).to_string();
            }
        }
        MenuOption::Style(i) => {
            settings.style = match super::new_game::STYLES.get(i) {
                Some(&"random") | None => super::new_game::StyleChoice::Random,
                Some(id) => super::new_game::StyleChoice::Id((*id).to_string()),
            };
        }
    }
}

/// Whether `option` is the chosen one in `settings`.
pub fn option_selected(option: MenuOption, settings: &super::new_game::NewGameSettings) -> bool {
    match option {
        MenuOption::Difficulty(i) => {
            super::new_game::DIFFICULTIES.get(i) == Some(&settings.difficulty.as_str())
        }
        MenuOption::Style(i) => match (&settings.style, super::new_game::STYLES.get(i)) {
            (super::new_game::StyleChoice::Random, Some(&"random")) => true,
            (super::new_game::StyleChoice::Id(id), Some(s)) => id == s,
            _ => false,
        },
    }
}

/// The selector follows [`super::new_game::NewGameSettings`]: the chosen
/// options in amber, the bonus caption, the seed line; and digits typed
/// while the menu is open edit the seed.
#[allow(clippy::type_complexity)]
pub fn update_selector(
    settings: Option<ResMut<super::new_game::NewGameSettings>>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    sim: NonSend<SimHandle>,
    strings: Option<Res<UiStrings>>,
    mut options: Query<(&MenuOption, &mut BackgroundColor, &Interaction)>,
    mut caption: Query<&mut Text, (With<BonusCaption>, Without<MenuLine>)>,
    mut lines: Query<(&MenuLine, &mut Text), Without<BonusCaption>>,
) {
    let (Some(mut settings), Some(strings)) = (settings, strings) else {
        return;
    };
    if let Some(keys) = keys {
        for key in keys.get_just_pressed() {
            settings.type_seed_key(*key);
        }
    }
    for (option, mut bg, interaction) in &mut options {
        bg.0 = if option_selected(*option, &settings) {
            palette::HUD_AMBER
        } else if *interaction == Interaction::Hovered {
            palette::HUD_BUTTON_HOVER
        } else {
            palette::HUD_BUTTON
        };
    }
    let want = super::new_game::bonus_caption(sim.rules(), &settings.difficulty);
    for mut t in &mut caption {
        if t.0 != want {
            t.0.clone_from(&want);
        }
    }
    let seed = fill(
        strings.hud("menu.seed"),
        &[("seed", &settings.seed.to_string())],
    );
    for (line, mut t) in &mut lines {
        if line.0 == 2 && t.0 != seed {
            t.0.clone_from(&seed);
        }
    }
}

/// A primary click on Quit: `AppExit::Success`. Nothing has been recorded.
pub fn on_quit_click(click: On<Pointer<Click>>, mut exit: MessageWriter<AppExit>) {
    if click.event().event.button != PointerButton::Primary {
        return;
    }
    println!("menu: quit");
    exit.write(AppExit::Success);
}

/// Hover and pressed colours for the menu's buttons (`palette::HUD_BUTTON*`,
/// `widgets::button_colors`).
pub fn menu_button_colors(
    buttons: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<MenuButton>)>,
) {
    super::widgets::button_colors::<MenuButton>(buttons);
}

/// The New Game click proxy's state (`--new-game-after-seconds`).
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct NewGameAfter {
    /// Wall time after start-up before the click begins.
    pub after: Duration,
    /// Frames since the click began (move, press, release).
    pub frame: u32,
    /// The release was written.
    pub done: bool,
}

impl NewGameAfter {
    /// A proxy that clicks once `after` has passed.
    pub fn new(after: Duration) -> Self {
        Self {
            after,
            frame: 0,
            done: false,
        }
    }
}

/// `--new-game-after-seconds`: once the delay has passed and the New Game
/// button is laid out, move the synthetic mouse to the button's centre,
/// press the primary button [`HUD_CLICK_STEP_FRAMES`] frames later and
/// release it as many frames after that (the hover map updates in the next
/// frame's `PreUpdate`, and `Pointer<Click>` fires the frame after the
/// release), with the window cursor at the pointer for each of those
/// frames and the real one put back in `PostUpdate`. Runs only in
/// `AppState::Menu`.
#[allow(clippy::too_many_arguments)] // Bevy system: each parameter is one resource or query
pub fn click_new_game_after(
    time: Res<Time<Real>>,
    mut proxy: ResMut<NewGameAfter>,
    mut pointer_inputs: MessageWriter<PointerInput>,
    mut buttons: ResMut<ButtonInput<MouseButton>>,
    mut windows: Query<(Entity, &mut Window), With<PrimaryWindow>>,
    menu_buttons: Query<(&ComputedNode, &UiGlobalTransform, &MenuButton)>,
    mut real_cursor: ResMut<RealCursor>,
) {
    if proxy.done || time.elapsed() < proxy.after {
        return;
    }
    let Some((node, transform, _)) = menu_buttons
        .iter()
        .find(|(_, _, b)| **b == MenuButton::NewGame)
    else {
        return;
    };
    if node.size.length_squared() <= 0.0 {
        return;
    }
    let Ok((window_entity, mut window)) = windows.single_mut() else {
        return;
    };
    let Some(target) = RenderTarget::Window(WindowRef::Primary).normalize(Some(window_entity))
    else {
        return;
    };
    let at = node_centre_logical(node, transform);
    real_cursor.0 = Some(window.physical_cursor_position());
    let frame = proxy.frame;
    proxy.frame += 1;
    if frame > 0 {
        window.set_cursor_position(Some(at));
    }
    let mut mouse = SyntheticMouse {
        target,
        pointer_inputs: &mut pointer_inputs,
        buttons: &mut buttons,
        window: &mut window,
    };
    const STEP: u32 = HUD_CLICK_STEP_FRAMES;
    match frame {
        0 => mouse.move_from(Vec2::ZERO, at),
        STEP => mouse.press_at(at, PointerButton::Primary),
        f if f == 2 * STEP => {
            mouse.release_at(at, PointerButton::Primary);
            proxy.done = true;
            println!(
                "new-game: clicked at ({:.0}, {:.0}) after {:.2} s",
                at.x,
                at.y,
                time.elapsed().as_secs_f64()
            );
        }
        _ => {}
    }
}
