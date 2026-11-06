//! The victory, defeat or draw overlay (M5a). When the sim decides the
//! match, `sim_driver::step_sim` finishes the replay, prints the `outcome:`
//! line and inserts [`MatchOver`]; [`show_outcome`] then fills and shows a
//! full-window panel over the frozen world with five lines: the result for
//! the local player (Victory, Defeat or Draw), the reason, the match time
//! and deciding tick, the final state hash and the replay's file name.
//! The hash is the one `sim-cli verify` prints for that replay, so the
//! owner can check it against the overlay (ROADMAP M5a acceptance 3).
//!
//! M6 (slice 3) grows it into the game-over screen ([`GameOverPlugin`];
//! `MatchOver` stays a resource, no new state): under the five lines, a
//! table of the statistics of M6 owner decision 8 in two columns, "You" and
//! "Hard · Rush" (Blue and Red when spectating; [`game_over_rows`]), built
//! from the live `MatchStats` and `ai::bot_stats` through
//! `ai::match_log::MatchReport`, so it reads the same numbers as `sim-cli
//! match-log`; and Play Again, Main Menu and Quit. At the outcome it prints
//! the `match_log:` line, `sim-cli match-log`'s row for the replay (the
//! Verify cell names the hash still to verify) plus the speed and pause
//! times ([`match_log_line`]). A Random style is revealed here. It keeps
//! the `bevy_ui` picking default, so
//! it blocks every click on the world, the HUD's panels and the command
//! card under it; `hud::WorldInputSet` and the command card's systems stop
//! too (`run_if(not(resource_exists::<MatchOver>))`). The camera still
//! moves. Registered only by the windowed app, never by `--headless-run`.

use bevy::prelude::*;
use rules::Rules;
use rules::strings::fill;
use sim::{Outcome, OutcomeReason, PlayerId, SimView};

use crate::economy_hud::UiStrings;
use crate::hud::{PADDING_PX, spawn_hud};
use crate::markers::clock;
use crate::orders::OrderMode;
use crate::palette;
use crate::placement::PlacementGhost;
use crate::selection::DragBox;
use crate::sim_driver::{LOCAL_PLAYER, MatchOver, SimHandle};

/// The overlay's z index: above every HUD panel, the drag box (10) and the
/// tooltip.
pub const OUTCOME_Z: i32 = 50;
/// The title's font size (Victory, Defeat, Draw), logical px.
pub const TITLE_FONT_PX: f32 = 32.0;
/// The game-over title's font size (M6: the stream's end card), logical px.
pub const GAME_OVER_TITLE_PX: f32 = 40.0;
/// The game-over screen's text size: nothing on it is under 16 px
/// logical, so it reads on a 1280 x 720 stream (M6 owner decision 10).
pub const GAME_OVER_FONT_PX: f32 = 16.0;

/// The statistics table of the game-over screen (M6).
#[derive(Component, Debug, Clone, Copy)]
pub struct GameOverGrid;

/// The game-over screen's buttons (M6).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOverButton {
    /// The same difficulty and style at a fresh seed.
    PlayAgain,
    /// Back to the main menu (a fresh unstarted match).
    MainMenu,
    /// Quit the game.
    Quit,
}

/// The overlay's full-window root node, hidden until the match is decided.
#[derive(Component, Debug, Clone, Copy)]
pub struct OutcomeOverlay;

/// One of the overlay's five lines, by index: 0 title, 1 reason, 2 time,
/// 3 hash, 4 replay.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutcomeLine(pub usize);

/// The game-over screen (windowed app only; M5a's outcome overlay grown in
/// M6).
pub struct GameOverPlugin {
    /// `--play-again-after-seconds`: ask for Play Again this long after the
    /// screen first shows (once; the `PendingMatch` the button inserts, not
    /// a picking click).
    pub play_again_after: Option<std::time::Duration>,
}

impl Plugin for GameOverPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_outcome_overlay.after(spawn_hud))
            .add_systems(
                Update,
                (
                    show_outcome
                        .run_if(resource_added::<MatchOver>)
                        // A `--screenshot-at` the deciding tick captures the
                        // frame that first shows the overlay.
                        .before(crate::app::take_tick_screenshots),
                    crate::menu::widgets::button_colors::<GameOverButton>,
                ),
            );
        if let Some(after) = self.play_again_after {
            app.insert_resource(PlayAgainAfter {
                after,
                at: None,
                done: false,
            })
            .add_systems(Update, play_again_after);
        }
    }
}

/// The Play Again proxy's state (`--play-again-after-seconds`).
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct PlayAgainAfter {
    /// The delay after the screen shows.
    pub after: std::time::Duration,
    /// `Time<Real>` seconds when the screen showed.
    pub at: Option<f64>,
    /// Pressed already.
    pub done: bool,
}

fn play_again_after(
    time: Res<Time<Real>>,
    over: Option<Res<MatchOver>>,
    mut proxy: ResMut<PlayAgainAfter>,
    mut commands: Commands,
) {
    if proxy.done || over.is_none() {
        return;
    }
    let now = time.elapsed_secs_f64();
    let at = *proxy.at.get_or_insert(now);
    if now - at >= proxy.after.as_secs_f64() {
        proxy.done = true;
        println!("play-again: pressed after {:.2} s", now - at);
        commands.insert_resource(crate::menu::main_menu::PendingMatch::PlayAgain);
    }
}

/// Spawns the hidden overlay: a full-window column (all four edges at
/// 0 px, so no percent units, `docs/design/ui.md`) on the
/// `palette::GAME_OVER_SCRIM` scrim at [`OUTCOME_Z`], with five empty lines
/// centred in it.
pub fn spawn_outcome_overlay(mut commands: Commands) {
    commands
        .spawn((
            Name::new("Outcome overlay"),
            OutcomeOverlay,
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                bottom: Val::Px(0.0),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: Val::Px(PADDING_PX),
                ..default()
            },
            BackgroundColor(palette::GAME_OVER_SCRIM),
            GlobalZIndex(OUTCOME_Z),
        ))
        .with_children(|overlay| {
            for i in 0..5 {
                let size = if i == 0 {
                    GAME_OVER_TITLE_PX
                } else {
                    GAME_OVER_FONT_PX
                };
                overlay.spawn((
                    OutcomeLine(i),
                    Text::new(""),
                    crate::fonts::hud_font(size),
                    TextColor(palette::HUD_TEXT),
                ));
            }
            // M6: the statistics table, filled at the outcome.
            overlay.spawn((
                Name::new("Game-over table"),
                GameOverGrid,
                Node {
                    display: Display::Grid,
                    grid_template_columns: vec![
                        GridTrack::px(230.0),
                        GridTrack::px(380.0),
                        GridTrack::px(520.0),
                    ],
                    column_gap: Val::Px(PADDING_PX * 2.0),
                    row_gap: Val::Px(2.0),
                    margin: UiRect::vertical(Val::Px(PADDING_PX)),
                    ..default()
                },
                Pickable::IGNORE,
            ));
            overlay
                .spawn((
                    Name::new("Game-over buttons"),
                    Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(PADDING_PX),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|row| {
                    for button in [
                        GameOverButton::PlayAgain,
                        GameOverButton::MainMenu,
                        GameOverButton::Quit,
                    ] {
                        // The label is filled at the outcome (`show_outcome`).
                        crate::menu::widgets::spawn_button(
                            row,
                            format!("Game-over {button:?}"),
                            button,
                            (
                                GameOverButtonLabel(button),
                                crate::menu::widgets::label("", GAME_OVER_FONT_PX),
                            ),
                        )
                        .observe(on_game_over_click);
                    }
                });
        });
}

/// The overlay's five lines for `over`, as `local` sees it:
///
/// 0. "Victory" when `local` won, "Defeat" when another player did, "Draw";
///    spectating (`local`'s seat is a bot), "Blue wins" or "Red wins";
/// 1. the reason: "The Seat was captured", "Every Town was lost",
///    "Surrender", or "Territory at the time limit: 1248 tiles to 1245"
///    (the local player's owned tiles first, then the most any other
///    player owns);
/// 2. the match time and the deciding tick: "1:21, tick 1628";
/// 3. "Final hash 0x2c9a9ef52a5184d0", the hash `sim-cli verify` prints
///    for the replay;
/// 4. "Replay 20261023-120000-31.eonreplay", or empty when nothing was
///    recorded.
pub fn overlay_texts(
    over: &MatchOver,
    local: PlayerId,
    view: &SimView<'_>,
    rules: &Rules,
    strings: &UiStrings,
) -> [String; 5] {
    // Spectating (the local seat is a bot), nobody is "you": the title names
    // the winning seat, Blue (P0) or Red (P1).
    let spectating = view
        .setup()
        .players
        .iter()
        .any(|p| p.id == local && p.is_ai);
    let title = match over.result.outcome {
        Outcome::Victory { player, .. } if spectating && player == PlayerId(0) => {
            strings.hud("outcome.blue_wins")
        }
        Outcome::Victory { .. } if spectating => strings.hud("outcome.red_wins"),
        Outcome::Victory { player, .. } if player == local => strings.hud("outcome.victory"),
        Outcome::Victory { .. } => strings.hud("outcome.defeat"),
        Outcome::Draw => strings.hud("outcome.draw"),
    }
    .to_string();
    let reason = match over.result.reason {
        OutcomeReason::SeatCaptured => strings.hud("outcome.seat_captured").to_string(),
        OutcomeReason::AllTownsLost => strings.hud("outcome.all_towns_lost").to_string(),
        OutcomeReason::Surrender => strings.hud("outcome.surrender").to_string(),
        OutcomeReason::Territory => {
            let tiles = |p| view.territory().owned_tile_count(p);
            let others = view
                .players()
                .iter()
                .filter(|p| p.id != local)
                .map(|p| tiles(p.id))
                .max()
                .unwrap_or(0);
            fill(
                strings.hud("outcome.territory"),
                &[
                    ("count", &tiles(local).to_string()),
                    ("cap", &others.to_string()),
                ],
            )
        }
    };
    let tick = over.result.tick;
    let time = fill(
        strings.hud("outcome.time"),
        &[
            ("time", &clock(tick / rules.tick_rate_hz.max(1))),
            ("count", &tick.to_string()),
        ],
    );
    let hash = fill(
        strings.hud("outcome.hash"),
        &[("hash", &format!("{:#018x}", over.hash))],
    );
    let replay = over
        .replay
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|name| {
            fill(
                strings.hud("outcome.replay"),
                &[("name", &name.to_string_lossy())],
            )
        })
        .unwrap_or_default();
    [title, reason, time, hash, replay]
}

/// Once, when [`MatchOver`] appears: fill the five lines
/// ([`overlay_texts`]), show the overlay and leave any order mode (the
/// attack-move crosshair would otherwise stay over it). World input stops
/// with this frame, and with it the systems that would end a drag box or
/// hide the placement ghost, so a drag box held at the outcome is dropped
/// here (`selection::draw_drag_box` then hides its node) and the ghost is
/// hidden.
#[allow(clippy::too_many_arguments)]
pub fn show_outcome(
    over: Res<MatchOver>,
    sim: NonSend<SimHandle>,
    strings: Option<Res<UiStrings>>,
    mut overlay: Query<&mut Node, With<OutcomeOverlay>>,
    mut lines: Query<(&OutcomeLine, &mut Text), Without<GameOverButtonLabel>>,
    mode: Option<ResMut<OrderMode>>,
    drag: Option<ResMut<DragBox>>,
    mut ghosts: Query<&mut Visibility, With<PlacementGhost>>,
    extra: GameOverExtra,
    mut commands: Commands,
) {
    let Some(strings) = strings else {
        return;
    };
    let texts = overlay_texts(&over, LOCAL_PLAYER, &sim.view(), sim.rules(), &strings);
    for (line, mut text) in &mut lines {
        if let Some(want) = texts.get(line.0) {
            text.0.clone_from(want);
        }
    }
    // M6: the statistics table, the buttons' labels and the match-log line.
    let report = report_of(&sim);
    let territory = {
        let view = sim.view();
        let t = view.territory();
        let other = view
            .players()
            .iter()
            .filter(|p| p.id != LOCAL_PLAYER)
            .map(|p| t.owned_tile_count(p.id))
            .max()
            .unwrap_or(0);
        Some((t.owned_tile_count(LOCAL_PLAYER), other))
    };
    let rows = game_over_rows(&report, LOCAL_PLAYER, territory, &strings);
    for grid in &extra.grid {
        commands.entity(grid).despawn_related::<Children>();
        commands.entity(grid).with_children(|g| {
            for (r, row) in rows.iter().enumerate() {
                let cells: Vec<(String, Color)> = row
                    .iter()
                    .enumerate()
                    .map(|(c, cell)| {
                        let color = match (r, c) {
                            (0, _) => palette::HUD_AMBER,
                            (_, 0) => palette::GAME_OVER_LABEL,
                            _ => palette::HUD_TEXT,
                        };
                        (cell.clone(), color)
                    })
                    .collect();
                crate::menu::widgets::stat_row(g, &cells, GAME_OVER_FONT_PX);
            }
        });
    }
    for (GameOverButtonLabel(button), mut text) in extra.labels {
        let key = match button {
            GameOverButton::PlayAgain => "game_over.play_again",
            GameOverButton::MainMenu => "game_over.main_menu",
            GameOverButton::Quit => "game_over.quit",
        };
        text.0 = strings.hud(key).to_string();
    }
    let speed = extra.speed.map(|s| *s).unwrap_or_default();
    println!("{}", match_log_line(&report, &over, &speed));
    for mut node in &mut overlay {
        node.display = Display::Flex;
    }
    if let Some(mut mode) = mode {
        *mode = OrderMode::Normal;
    }
    if let Some(mut drag) = drag {
        drag.0 = None;
    }
    for mut visibility in &mut ghosts {
        *visibility = Visibility::Hidden;
    }
    // The minimap sits under the label column; its outline would show
    // through the scrim (`hide_game_over` brings it back).
    for mut visibility in extra.minimap {
        *visibility = Visibility::Hidden;
    }
}

/// The game-over screen's extra parameters (M6).
#[derive(bevy::ecs::system::SystemParam)]
pub struct GameOverExtra<'w, 's> {
    grid: Query<'w, 's, Entity, With<GameOverGrid>>,
    labels: Query<'w, 's, (&'static GameOverButtonLabel, &'static mut Text), Without<OutcomeLine>>,
    speed: Option<Res<'w, crate::speed::SpeedStats>>,
    minimap: Query<
        'w,
        's,
        &'static mut Visibility,
        (With<crate::minimap::MinimapFrame>, Without<PlacementGhost>),
    >,
}

/// A game-over button's label text.
#[derive(Component, Debug, Clone, Copy)]
pub struct GameOverButtonLabel(pub GameOverButton);

/// The match so far as an `ai::match_log::MatchReport`: the header, the
/// rules, the live `MatchStats`, each seat's bot statistics and the
/// result, the numbers `sim-cli match-log` re-derives from the replay.
pub fn report_of(sim: &SimHandle) -> ai::match_log::MatchReport {
    let view = sim.view();
    ai::match_log::MatchReport {
        setup: sim.setup().clone(),
        rules: sim.rules().clone(),
        stats: sim.stats().clone(),
        bots: view
            .players()
            .iter()
            .map(|p| ai::bot_stats(&view, p.id))
            .collect(),
        result: sim.result(),
        last_tick: sim.tick(),
    }
}

/// Today's UTC date, `yyyy-mm-dd`.
pub fn today() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let name = crate::sim_driver::replay_file_name(0, secs);
    format!("{}-{}-{}", &name[0..4], &name[4..6], &name[6..8])
}

/// The `match_log:` line printed at the outcome (M6): the docs/PLAYTEST.md
/// row `sim-cli match-log` prints for the replay, with today's date, `-`
/// for the commit (the header carries none; `just match-log` fills it),
/// and `to verify 0x<hash>` in the Verify cell, then the time spent at each
/// speed and paused and the orders given while paused.
pub fn match_log_line(
    report: &ai::match_log::MatchReport,
    over: &MatchOver,
    speed: &crate::speed::SpeedStats,
) -> String {
    let name = over
        .replay
        .as_ref()
        .and_then(|p| p.file_name())
        .map_or_else(|| "-".to_string(), |n| n.to_string_lossy().into_owned());
    let row = report.row(
        &today(),
        "-",
        &name,
        &format!("to verify 0x{:016x}", over.hash),
    );
    format!(
        "match_log: {row} speed x1={:.0}s x2={:.0}s x4={:.0}s paused_s={:.0} orders_while_paused={}",
        speed.at_speed_s[0],
        speed.at_speed_s[1],
        speed.at_speed_s[2],
        speed.paused_s,
        speed.orders_while_paused
    )
}

fn seat_label(report: &ai::match_log::MatchReport, p: PlayerId) -> String {
    let Some(seat) = report.setup.ai_seats.iter().find(|s| s.player == p) else {
        return String::new();
    };
    let name = crate::menu::new_game::difficulty_label(&report.rules, &seat.bot);
    if seat.personality.is_empty() {
        name
    } else {
        format!(
            "{name} · {}",
            crate::menu::new_game::style_label(&report.rules, &seat.personality)
        )
    }
}

/// `text` with each space a no-break space (U+00A0): a count stays on the
/// line of its kind or resource when a statistics cell wraps.
fn nbsp(text: &str) -> String {
    text.replace(' ', "\u{a0}")
}

fn kinds_text(
    report: &ai::match_log::MatchReport,
    m: &std::collections::BTreeMap<sim::UnitKindId, u32>,
) -> String {
    let total: u32 = m.values().sum();
    let by: Vec<String> = m
        .iter()
        .filter(|(_, n)| **n > 0)
        .map(|(k, n)| {
            let name = report
                .rules
                .unit_kind(k.0)
                .map_or_else(|| format!("kind {}", k.0), |u| u.name.clone());
            nbsp(&format!("{name} {n}"))
        })
        .collect();
    if by.is_empty() {
        total.to_string()
    } else {
        format!("{total} ({})", by.join(", "))
    }
}

/// The game-over table (M6 owner decision 8) as `local` sees it: a header
/// row (blank, "You" or "Blue", the bot's "Hard · Rush" or "Red · ..."),
/// then per side units trained, units lost by kind, Towns taken and lost,
/// ages reached, minutes at the Yield Cap, peak army, the bot's disclosed
/// bonus (total, share of its income, the bundle and its interval) and
/// its waves, raids and retreats; then the match facts in the first
/// column: decisive or the tiebreak's territory shares, the enemy first on
/// your land, your lowest Town and first contact (spectating: Red first on
/// Blue's land, Blue's lowest Town). A count and its kind are joined by a
/// no-break space, so a wrapped cell never splits them. `territory` is the
/// tiles `local` and the other player own at the end (the tiebreak's
/// shares).
pub fn game_over_rows(
    report: &ai::match_log::MatchReport,
    local: PlayerId,
    territory: Option<(u32, u32)>,
    strings: &UiStrings,
) -> Vec<[String; 3]> {
    let rules = &report.rules;
    let rate = rules.tick_rate_hz.max(1);
    let me = local;
    let them = report
        .setup
        .players
        .iter()
        .map(|s| s.id)
        .find(|p| *p != me)
        .unwrap_or(PlayerId(1));
    let spectating = report.setup.players.iter().any(|s| s.id == me && s.is_ai);
    let h = |k: &str| strings.hud(k).to_string();
    let header_me = if spectating {
        format!("{} · {}", h("game_over.blue"), seat_label(report, me))
    } else {
        h("game_over.you")
    };
    let header_them = if spectating {
        format!("{} · {}", h("game_over.red"), seat_label(report, them))
    } else {
        seat_label(report, them)
    };
    let clock = |t: u32| clock(t / rate);
    let side = |p: PlayerId| -> Vec<String> {
        let s = report.stats.player(p);
        let ages: Vec<String> = s
            .ages
            .iter()
            .filter_map(|(age, t)| {
                rules
                    .ages
                    .get(usize::from(*age))
                    .map(|a| format!("{} {}", a.name, clock(*t)))
            })
            .collect();
        let bonus = if s.bonus_total() > 0 {
            let d = report
                .setup
                .ai_seats
                .iter()
                .find(|x| x.player == p)
                .and_then(|x| rules.ai.difficulty(&x.bot));
            let every = d.map_or(0, |d| d.income_interval_ds / 10);
            let bundle: Vec<String> = d.map_or_else(Vec::new, |d| {
                rules
                    .resources
                    .resources
                    .iter()
                    .filter_map(|r| {
                        let n = *d.income.get(&r.id)?;
                        (n > 0).then(|| nbsp(&format!("{n} {}", r.name)))
                    })
                    .collect()
            });
            format!(
                "+{} ({}% of income), {} {}",
                s.bonus_total(),
                report.stats.bonus_share_permille(p) / 10,
                bundle.join(", "),
                nbsp(&format!("every {every} s"))
            )
        } else {
            "-".to_string()
        };
        let waves = report
            .bots
            .get(usize::from(p.0))
            .copied()
            .flatten()
            .map_or_else(
                || "-".to_string(),
                |b| format!("{} · {} · {}", b.waves_launched, b.raids, b.retreats),
            );
        vec![
            kinds_text(report, &s.units_trained),
            kinds_text(report, &s.units_lost),
            format!("{} / {}", s.towns_taken, s.towns_lost),
            if ages.is_empty() {
                "-".into()
            } else {
                ages.join(", ")
            },
            format!("{}.{} min", s.cap_seconds / 60, s.cap_seconds % 60 / 6),
            s.peak_army.to_string(),
            bonus,
            waves,
        ]
    };
    let labels = [
        "game_over.units_trained",
        "game_over.units_lost",
        "game_over.towns",
        "game_over.ages",
        "game_over.capped",
        "game_over.peak_army",
        "game_over.bonus",
        "game_over.waves",
    ];
    let mut rows = vec![[String::new(), header_me, header_them]];
    for ((label, a), b) in labels.iter().zip(side(me)).zip(side(them)) {
        rows.push([h(label), a, b]);
    }
    let result = match report.result {
        Some(r) if r.reason == sim::OutcomeReason::Territory => {
            let (a, b) = territory.unwrap_or((0, 0));
            let total = u64::from(a + b).max(1);
            fill(
                strings.hud("game_over.tiebreak"),
                &[
                    ("count", &(u64::from(a) * 100 / total).to_string()),
                    ("cap", &(u64::from(b) * 100 / total).to_string()),
                ],
            )
        }
        Some(_) => h("game_over.decisive"),
        None => "-".to_string(),
    };
    rows.push([h("game_over.result"), result, String::new()]);
    let mine = report.stats.player(me);
    let enemy = report.stats.player(them);
    let never = h("game_over.never");
    let (on_land, lowest_town) = if spectating {
        ("game_over.red_on_blue_land", "game_over.blue_lowest_town")
    } else {
        ("game_over.enemy_on_land", "game_over.lowest_town")
    };
    rows.push([
        h(on_land),
        enemy
            .first_on_land_tick
            .map_or_else(|| never.clone(), clock),
        String::new(),
    ]);
    let lowest = if mine.towns_lost > 0 && mine.min_own_town_hp_permille == 0 {
        h("game_over.annexed")
    } else {
        format!("{}%", mine.min_own_town_hp_permille / 10)
    };
    rows.push([h(lowest_town), lowest, String::new()]);
    rows.push([
        h("game_over.first_contact"),
        report.stats.first_contact_tick().map_or(never, clock),
        String::new(),
    ]);
    rows
}

/// Hide the game-over screen (a rebuilt match, `match_scope`) and show the
/// minimap again (it is not match-scoped, so it lasts the session).
pub fn hide_game_over(world: &mut World) {
    let mut q = world.query_filtered::<&mut Node, With<OutcomeOverlay>>();
    for mut node in q.iter_mut(world) {
        node.display = Display::None;
    }
    let mut q = world.query_filtered::<&mut Visibility, With<crate::minimap::MinimapFrame>>();
    for mut v in q.iter_mut(world) {
        *v = Visibility::Inherited;
    }
}

/// A primary click on a game-over button: Play Again and Main Menu ask
/// `match_scope` for the next match ([`crate::menu::main_menu::PendingMatch`];
/// Main Menu also enters `AppState::Menu`), Quit exits.
pub fn on_game_over_click(
    click: On<bevy::picking::events::Pointer<bevy::picking::events::Click>>,
    buttons: Query<&GameOverButton>,
    next: Option<ResMut<NextState<crate::menu::AppState>>>,
    mut exit: MessageWriter<bevy::app::AppExit>,
    mut commands: Commands,
) {
    if click.event().event.button != bevy::picking::pointer::PointerButton::Primary {
        return;
    }
    let Ok(button) = buttons.get(click.event().entity) else {
        return;
    };
    use crate::menu::main_menu::PendingMatch;
    match button {
        GameOverButton::PlayAgain => {
            println!("game-over: play again");
            commands.insert_resource(PendingMatch::PlayAgain);
        }
        GameOverButton::MainMenu => {
            println!("game-over: main menu");
            commands.insert_resource(PendingMatch::MainMenu);
            if let Some(mut next) = next {
                next.set(crate::menu::AppState::Menu);
            }
        }
        GameOverButton::Quit => {
            println!("game-over: quit");
            exit.write(bevy::app::AppExit::Success);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sim::{MatchResult, MatchSetup};
    use std::path::{Path, PathBuf};

    fn data() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    fn strings() -> UiStrings {
        UiStrings(rules::strings::Strings::load(data(), "en").unwrap())
    }

    fn over(outcome: Outcome, reason: OutcomeReason, tick: u32) -> MatchOver {
        MatchOver {
            result: MatchResult {
                outcome,
                reason,
                tick,
            },
            hash: 0x2c9a_9ef5_2a51_84d0,
            replay: Some(PathBuf::from("/r/20261023-120000-31.eonreplay")),
        }
    }

    #[test]
    fn overlay_texts_for_each_reason_and_side() {
        let rules = Rules::load(data()).unwrap();
        let s = strings();
        // The standard start gives each player a Seat that owns 1245 tiles
        // (m3a's `economy_standard_start_layout`); a complete P0 Watchtower
        // at (43, 64) adds 3 to P0 (m3a's two-Towns-and-a-Watchtower
        // golden), so the two counts differ and their order shows.
        let mut setup = MatchSetup::scenario(&rules, 1);
        setup.start = sim::StartKind::Standard;
        let delay = setup.cmd_delay;
        let mut sim = SimHandle::from_setup(setup, rules.clone());
        let (tower, tower_def) = rules.building_kind_by_id("watchtower").unwrap();
        let golden_radii =
            (rules.town_radius_tiles, tower_def.border_radius_tiles) == (20, Some(2));
        let (p0, p1) = (PlayerId(0), PlayerId(1));
        sim.step(&[sim::PlayerCommand::new(
            p0,
            0,
            sim::Command::DebugPlaceBuilding {
                owner: p0,
                kind: sim::BuildingKindId(tower),
                tile: sim::Tile::new(43, 64),
                complete: true,
            },
        )]);
        for _ in 0..delay {
            sim.step(&[]);
        }
        let view = sim.view();
        let tiles = |p| view.territory().owned_tile_count(PlayerId(p));
        assert!(tiles(0) > tiles(1), "{} : {}", tiles(0), tiles(1));
        if golden_radii {
            assert_eq!((tiles(0), tiles(1)), (1248, 1245));
        }
        let win = |player| Outcome::Victory {
            player,
            decisive: true,
        };
        let seat = over(win(p0), OutcomeReason::SeatCaptured, 1628);
        assert_eq!(
            overlay_texts(&seat, p0, &view, &rules, &s),
            [
                "Victory",
                "The Seat was captured",
                "1:21, tick 1628",
                "Final hash 0x2c9a9ef52a5184d0",
                "Replay 20261023-120000-31.eonreplay",
            ]
        );
        // The same result seen by the other player.
        assert_eq!(overlay_texts(&seat, p1, &view, &rules, &s)[0], "Defeat");
        let lost = over(win(p1), OutcomeReason::AllTownsLost, 2401);
        let texts = overlay_texts(&lost, p0, &view, &rules, &s);
        assert_eq!(texts[0], "Defeat");
        assert_eq!(texts[1], "Every Town was lost");
        assert_eq!(texts[2], "2:00, tick 2401");
        let quit = over(win(p0), OutcomeReason::Surrender, 3);
        assert_eq!(overlay_texts(&quit, p0, &view, &rules, &s)[1], "Surrender");
        let draw = over(Outcome::Draw, OutcomeReason::Surrender, 3);
        assert_eq!(overlay_texts(&draw, p0, &view, &rules, &s)[0], "Draw");
        // The tiebreak names both counts, the local player's first.
        let cap = over(
            Outcome::Victory {
                player: p0,
                decisive: false,
            },
            OutcomeReason::Territory,
            54000,
        );
        let line = |first: u32, second: u32| {
            format!("Territory at the time limit: {first} tiles to {second}")
        };
        let texts = overlay_texts(&cap, p0, &view, &rules, &s);
        assert_eq!(texts[0], "Victory");
        assert_eq!(texts[1], line(tiles(0), tiles(1)), "P0's count first");
        if golden_radii {
            assert_eq!(texts[1], "Territory at the time limit: 1248 tiles to 1245");
        }
        assert_eq!(texts[2], "45:00, tick 54000");
        let texts = overlay_texts(&cap, p1, &view, &rules, &s);
        assert_eq!(texts[0], "Defeat");
        assert_eq!(texts[1], line(tiles(1), tiles(0)), "P1's count first");
        if golden_radii {
            assert_eq!(texts[1], "Territory at the time limit: 1245 tiles to 1248");
        }
        // No recording: no replay line.
        let unrecorded = MatchOver {
            replay: None,
            ..seat.clone()
        };
        assert_eq!(overlay_texts(&unrecorded, p0, &view, &rules, &s)[4], "");
    }

    /// M5b: the local player against the Standard bot. The bot taking the
    /// Seat, or the human surrendering, reads "Defeat" with its reason,
    /// the time, the hash and the replay; the human winning reads
    /// "Victory".
    #[test]
    fn defeat_by_the_bot_texts() {
        let rules = Rules::load(data()).unwrap();
        let s = strings();
        let sim = SimHandle::vs_bot(rules.clone(), 1, ai::BOT_STANDARD, "standard");
        let view = sim.view();
        let bot = PlayerId(1);
        let won = |player| Outcome::Victory {
            player,
            decisive: true,
        };
        assert_eq!(
            overlay_texts(
                &over(won(bot), OutcomeReason::SeatCaptured, 26012),
                LOCAL_PLAYER,
                &view,
                &rules,
                &s
            ),
            [
                "Defeat",
                "The Seat was captured",
                "21:40, tick 26012",
                "Final hash 0x2c9a9ef52a5184d0",
                "Replay 20261023-120000-31.eonreplay",
            ]
        );
        let quit = overlay_texts(
            &over(won(bot), OutcomeReason::Surrender, 603),
            LOCAL_PLAYER,
            &view,
            &rules,
            &s,
        );
        assert_eq!(quit[..3], ["Defeat", "Surrender", "0:30, tick 603"]);
        let lost = overlay_texts(
            &over(won(bot), OutcomeReason::AllTownsLost, 40000),
            LOCAL_PLAYER,
            &view,
            &rules,
            &s,
        );
        assert_eq!(lost[..2], ["Defeat", "Every Town was lost"]);
        let beat = overlay_texts(
            &over(won(LOCAL_PLAYER), OutcomeReason::SeatCaptured, 30000),
            LOCAL_PLAYER,
            &view,
            &rules,
            &s,
        );
        assert_eq!(beat[0], "Victory");
    }

    /// The overlay is hidden until `MatchOver` appears, then shows the
    /// five lines once; world input stops, an order mode is left, a drag
    /// box held at the outcome is dropped and its node hidden, a showing
    /// placement ghost is hidden, and a move marker placed just before the
    /// outcome still shrinks and expires (its timer is not world input).
    #[test]
    fn the_overlay_shows_once_the_match_is_over_and_input_stops() {
        use crate::hud::WorldInputSet;
        use crate::selection::{DragBoxNode, DragRect, MOVE_MARKER_SECS, MoveMarker};
        use bevy::time::TimeUpdateStrategy;
        use std::time::Duration;
        let rules = Rules::load(data()).unwrap();
        let setup = MatchSetup::scenario(&rules, 1);
        #[derive(Resource, Default)]
        struct WorldInputRuns(u32);
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
                100,
            )))
            .insert_resource(strings())
            .init_resource::<OrderMode>()
            .init_resource::<DragBox>()
            .init_resource::<WorldInputRuns>()
            .add_systems(Startup, spawn_outcome_overlay)
            .add_systems(
                Update,
                (
                    show_outcome.run_if(resource_added::<MatchOver>),
                    (|mut runs: ResMut<WorldInputRuns>| runs.0 += 1).in_set(WorldInputSet),
                ),
            );
        crate::hud::gate_world_input(&mut app);
        crate::selection::add_world_input_feedback(&mut app);
        app.world_mut()
            .insert_non_send(SimHandle::from_setup(setup, rules));
        let drag_node = app
            .world_mut()
            .spawn((
                DragBoxNode,
                Node {
                    display: Display::None,
                    ..default()
                },
            ))
            .id();
        let ghost = app
            .world_mut()
            .spawn((PlacementGhost, Visibility::Inherited))
            .id();
        *app.world_mut().resource_mut::<OrderMode>() = OrderMode::AttackMove;
        app.update();
        let display = |app: &mut App| {
            let mut q = app
                .world_mut()
                .query_filtered::<&Node, With<OutcomeOverlay>>();
            q.single(app.world()).unwrap().display
        };
        assert_eq!(display(&mut app), Display::None);
        assert_eq!(app.world().resource::<WorldInputRuns>().0, 1);
        // The player is box-dragging and has just right-clicked a move.
        app.world_mut().resource_mut::<DragBox>().0 = Some(DragRect {
            start: Vec2::new(10.0, 10.0),
            current: Vec2::new(200.0, 150.0),
        });
        let now = app.world().resource::<Time>().elapsed_secs();
        let marker = app
            .world_mut()
            .spawn((
                MoveMarker {
                    expires_at: now + MOVE_MARKER_SECS,
                },
                Transform::default(),
            ))
            .id();
        app.update();
        let node_display = |app: &App| app.world().get::<Node>(drag_node).unwrap().display;
        assert_eq!(node_display(&app), Display::Flex, "the drag box shows");
        assert_eq!(app.world().resource::<WorldInputRuns>().0, 2);
        app.insert_resource(over(
            Outcome::Victory {
                player: LOCAL_PLAYER,
                decisive: true,
            },
            OutcomeReason::SeatCaptured,
            1628,
        ));
        app.update();
        assert_eq!(display(&mut app), Display::Flex);
        let mut q = app.world_mut().query::<(&OutcomeLine, &Text)>();
        let mut lines: Vec<(usize, String)> = q
            .iter(app.world())
            .map(|(l, t)| (l.0, t.0.clone()))
            .collect();
        lines.sort();
        assert_eq!(lines[0], (0, "Victory".to_string()));
        assert_eq!(lines[3], (3, "Final hash 0x2c9a9ef52a5184d0".to_string()));
        assert_eq!(*app.world().resource::<OrderMode>(), OrderMode::Normal);
        assert_eq!(
            app.world().resource::<DragBox>().0,
            None,
            "the drag box held at the outcome is dropped"
        );
        assert_eq!(
            app.world().get::<Visibility>(ghost),
            Some(&Visibility::Hidden),
            "the placement ghost is hidden"
        );
        app.update();
        assert_eq!(
            node_display(&app),
            Display::None,
            "the drag box node is hidden"
        );
        assert_eq!(
            app.world().resource::<WorldInputRuns>().0,
            2,
            "world input stopped at the outcome"
        );
        // The marker keeps shrinking and is gone once its time is up.
        for _ in 0..10 {
            app.update();
        }
        assert!(
            app.world().get_entity(marker).is_err(),
            "the move marker expired after the outcome"
        );
    }

    /// A match of `setup` driven to its outcome with the scripted
    /// `stream` through the real `SimPlugin` and recorder (one tick per
    /// 50 ms update), the replay under `path`.
    fn played(setup: MatchSetup, stream: sim::scenarios::Stream, path: &Path) -> App {
        use crate::sim_driver::{ScenarioInput, SimPlugin};
        use bevy::time::TimeUpdateStrategy;
        use std::time::Duration;
        let rules = Rules::load(data()).unwrap();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
            .add_plugins(
                SimPlugin::new(
                    SimHandle::from_setup(setup, rules),
                    Box::new(ScenarioInput::new(stream)),
                )
                .recording(Some(path.to_path_buf())),
            );
        app.update();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            50,
        )));
        for _ in 0..3000 {
            if app.world().contains_resource::<MatchOver>() {
                break;
            }
            app.update();
        }
        assert!(app.world().contains_resource::<MatchOver>());
        app
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eonmark-outcome-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    /// The `match_log:` line's row equals the row `sim-cli match-log`
    /// re-derives from the game's replay (`ai::match_log::collect`), and
    /// the live statistics equal the re-simulated ones: `scripted_match`.
    #[test]
    fn match_log_equals_sim_cli_match_log() {
        let rules = Rules::load(data()).unwrap();
        let (setup, stream, _) = sim::scenarios::by_name("scripted_match", &rules).unwrap();
        let path = temp("match_log.eonreplay");
        let _ = std::fs::remove_file(&path);
        let app = played(setup, stream, &path);
        let over = app.world().resource::<MatchOver>().clone();
        let live = report_of(app.world().non_send::<SimHandle>());
        let (replayed, outcome) = ai::match_log::collect(&path, &data()).unwrap().unwrap();
        assert_eq!(
            outcome,
            sim::VerifyOutcome::Ok {
                final_hash: over.hash,
                ticks: over.result.tick
            }
        );
        assert_eq!(live.stats, replayed.stats, "the same statistics");
        let row =
            |r: &ai::match_log::MatchReport| r.row("2026-10-30", "abc1234", "x.eonreplay", "V");
        assert_eq!(row(&live), row(&replayed));
        let line = match_log_line(&live, &over, &crate::speed::SpeedStats::default());
        assert!(line.starts_with("match_log: | "), "{line}");
        assert!(
            line.contains(&format!("| to verify 0x{:016x} |", over.hash)),
            "{line}"
        );
        assert!(
            line.ends_with(" speed x1=0s x2=0s x4=0s paused_s=0 orders_while_paused=0"),
            "{line}"
        );
        assert!(line.contains(" | loss decisive | "), "{line}");
        let _ = std::fs::remove_file(&path);
    }

    /// The table's text for a defeat by the Standard bot (`scripted_match`),
    /// as the human sees it and as a spectator sees the same match, and a
    /// tiebreak's result row.
    #[test]
    fn game_over_rows_for_defeat_tiebreak_and_spectate() {
        let rules = Rules::load(data()).unwrap();
        let (setup, stream, _) = sim::scenarios::by_name("scripted_match", &rules).unwrap();
        let path = temp("rows.eonreplay");
        let app = played(setup, stream, &path);
        let mut report = report_of(app.world().non_send::<SimHandle>());
        let s = strings();
        let rows = game_over_rows(&report, LOCAL_PLAYER, Some((1300, 1250)), &s);
        assert_eq!(
            rows[0],
            ["".to_string(), "You".into(), "Standard · Balanced".into()]
        );
        let label = |r: &[String; 3]| r[0].clone();
        let labels: Vec<String> = rows.iter().skip(1).map(label).collect();
        assert_eq!(
            labels,
            [
                "Units trained",
                "Units lost",
                "Towns taken / lost",
                "Ages reached",
                "Income capped",
                "Peak army",
                "Bonus income",
                "Waves · raids · retreats",
                "Result",
                "Enemy first on your land",
                "Your lowest Town",
                "First contact",
            ]
        );
        // A count stays with its kind when the cell wraps (no-break space).
        assert!(rows[1][1].starts_with("2 (Yeoman\u{a0}2)"), "{:?}", rows[1]);
        assert_eq!(rows[7][1], "-", "the human gets no bonus");
        assert_eq!(rows[8][2], "0 · 0 · 0", "no wave by tick 1203");
        assert_eq!(rows[9][1], "Decisive");
        assert_eq!(rows[10][1], "never");
        assert_eq!(rows[11][1], "100%");
        // A tiebreak names the territory shares, the local player's first.
        report.result = Some(sim::MatchResult {
            outcome: Outcome::Victory {
                player: LOCAL_PLAYER,
                decisive: false,
            },
            reason: OutcomeReason::Territory,
            tick: 54000,
        });
        let rows = game_over_rows(&report, LOCAL_PLAYER, Some((1300, 1250)), &s);
        assert_eq!(rows[9][1], "Tiebreak, territory 50% to 49%");
        // Spectating: Blue and Red, each with its seat.
        report.setup.players[0].is_ai = true;
        report.setup.ai_seats.insert(
            0,
            sim::AiSeat {
                player: PlayerId(0),
                bot: "hard".into(),
                personality: "rush".into(),
            },
        );
        let rows = game_over_rows(&report, LOCAL_PLAYER, None, &s);
        assert_eq!(
            rows[0],
            [
                "".to_string(),
                "Blue · Hard · Rush".into(),
                "Red · Standard · Balanced".into()
            ]
        );
        // Nobody is "you": the facts name the seats.
        assert_eq!(rows[10][0], "Red first on Blue's land");
        assert_eq!(rows[11][0], "Blue's lowest Town");
        let _ = std::fs::remove_file(&path);
    }

    /// Spectating, the title names the winning seat ("Blue wins", "Red
    /// wins") instead of Victory or Defeat for P0.
    #[test]
    fn a_spectated_end_card_names_the_winning_seat() {
        let rules = Rules::load(data()).unwrap();
        let s = strings();
        let setup = MatchSetup::bots(&rules, 1, ("passive", ""), ("passive", ""));
        let sim = SimHandle::from_setup(setup, rules.clone());
        let view = sim.view();
        let won = |player| Outcome::Victory {
            player,
            decisive: true,
        };
        let title = |o| {
            overlay_texts(
                &over(o, OutcomeReason::SeatCaptured, 600),
                LOCAL_PLAYER,
                &view,
                &rules,
                &s,
            )[0]
            .clone()
        };
        assert_eq!(title(won(PlayerId(1))), "Red wins");
        assert_eq!(title(won(PlayerId(0))), "Blue wins");
        assert_eq!(title(Outcome::Draw), "Draw");
    }

    /// The minimap (not match-scoped, under the overlay) is hidden while
    /// the game-over screen shows, so its outline does not show through
    /// the label column, and comes back when the screen is hidden (Play
    /// Again, Main Menu).
    #[test]
    fn the_minimap_is_hidden_under_the_game_over_screen_and_comes_back() {
        use crate::minimap::MinimapFrame;
        let rules = Rules::load(data()).unwrap();
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(strings())
            .add_systems(Startup, spawn_outcome_overlay)
            .add_systems(Update, show_outcome.run_if(resource_added::<MatchOver>));
        app.world_mut().insert_non_send(SimHandle::from_setup(
            MatchSetup::scenario(&rules, 1),
            rules,
        ));
        let frame = app
            .world_mut()
            .spawn((MinimapFrame, Visibility::Inherited))
            .id();
        app.update();
        app.insert_resource(over(Outcome::Draw, OutcomeReason::Surrender, 3));
        app.update();
        assert_eq!(
            app.world().get::<Visibility>(frame),
            Some(&Visibility::Hidden)
        );
        hide_game_over(app.world_mut());
        assert_eq!(
            app.world().get::<Visibility>(frame),
            Some(&Visibility::Inherited)
        );
    }

    /// The game-over screen of a long Hard-against-Hard match laid out at
    /// 1280 x 720 with Inter: every text is inside the window, at 16 px or
    /// more, with tabular figures.
    #[test]
    fn game_over_panel_fits_1280x720_with_inter_tabular_figures() {
        use crate::ui_test::{layout_app, settle};
        let rules = Rules::load(data()).unwrap();
        let setup = MatchSetup::bots(&rules, 3, ("hard", "rush"), ("hard", "tower"));
        let mut handle = SimHandle::from_setup(setup, rules);
        handle.fast_forward(20_000).unwrap();
        let mut app = layout_app(UVec2::new(1280, 720));
        app.insert_resource(strings())
            .add_systems(Startup, spawn_outcome_overlay)
            .add_systems(Update, show_outcome.run_if(resource_added::<MatchOver>));
        app.world_mut().insert_non_send(handle);
        app.update();
        app.insert_resource(MatchOver {
            result: sim::MatchResult {
                outcome: Outcome::Victory {
                    player: PlayerId(1),
                    decisive: true,
                },
                reason: OutcomeReason::SeatCaptured,
                tick: 20_000,
            },
            hash: 0x0123_4567_89ab_cdef,
            replay: Some(PathBuf::from("/r/20261030-120000-3.eonreplay")),
        });
        settle(&mut app);
        settle(&mut app);
        let window = Rect::new(0.0, 0.0, 1280.0, 720.0);
        let mut q = app
            .world_mut()
            .query_filtered::<(Entity, &Text, &TextFont), With<ComputedNode>>();
        let texts: Vec<(Entity, String, TextFont)> = q
            .iter(app.world())
            .map(|(e, t, f)| (e, t.0.clone(), f.clone()))
            .collect();
        let mut cells = 0;
        for (e, text, font) in texts {
            if text.is_empty() {
                continue;
            }
            cells += 1;
            let r = crate::ui_test::rect(&app, e);
            assert!(
                window.contains(r.min) && window.contains(r.max),
                "{text:?} at {r:?} is off the 1280 x 720 window"
            );
            let FontSize::Px(px) = font.font_size else {
                panic!("{text:?}: not px");
            };
            assert!(px >= GAME_OVER_FONT_PX, "{text:?} is {px} px");
            assert_eq!(
                font.font_features,
                bevy::text::FontFeatures::from([bevy::text::FontFeatureTag::TABULAR_FIGURES]),
                "{text:?}"
            );
        }
        assert!(cells > 30, "{cells} cells");
    }

    /// The buttons: Play Again asks for a rebuild in place, Main Menu also
    /// returns to the menu, Quit exits.
    #[test]
    fn game_over_buttons_rebuild_or_return_to_menu() {
        use crate::menu::{AppState, main_menu::PendingMatch};
        use bevy::camera::NormalizedRenderTarget;
        use bevy::picking::backend::HitData;
        use bevy::picking::events::{Click, Pointer};
        use bevy::picking::pointer::{Location, PointerButton, PointerId};
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .insert_state(AppState::Skirmish)
            .add_systems(Startup, spawn_outcome_overlay);
        app.update();
        let click = |app: &mut App, button: GameOverButton| {
            let mut q = app.world_mut().query::<(Entity, &GameOverButton)>();
            let entity = q
                .iter(app.world())
                .find(|(_, b)| **b == button)
                .map(|(e, _)| e)
                .unwrap();
            app.world_mut().trigger(Pointer::new(
                PointerId::Mouse,
                Location {
                    target: NormalizedRenderTarget::None {
                        width: 1,
                        height: 1,
                    },
                    position: Vec2::ZERO,
                },
                Click {
                    button: PointerButton::Primary,
                    hit: HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                    duration: std::time::Duration::ZERO,
                    count: 1,
                },
                entity,
            ));
            app.update();
        };
        click(&mut app, GameOverButton::PlayAgain);
        assert_eq!(
            app.world().get_resource::<PendingMatch>(),
            Some(&PendingMatch::PlayAgain)
        );
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Skirmish
        );
        app.world_mut().remove_resource::<PendingMatch>();
        click(&mut app, GameOverButton::MainMenu);
        assert_eq!(
            app.world().get_resource::<PendingMatch>(),
            Some(&PendingMatch::MainMenu)
        );
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Menu
        );
        click(&mut app, GameOverButton::Quit);
        let exits = app.world().resource::<Messages<bevy::app::AppExit>>();
        assert!(!exits.is_empty(), "Quit wrote AppExit");
    }
}
