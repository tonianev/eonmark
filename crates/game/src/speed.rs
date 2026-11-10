//! Game speed and pause (ROADMAP M6 line 5; M6 owner decision 7): x1, x2
//! and x4, Space, `-` and `=` in every build, and an ACTIVE pause: the
//! camera, selection, orders and the command card keep working while the
//! sim stands still, and orders given while paused go out with the first
//! tick after it.
//!
//! [`GameSpeed`] is the one switch. [`apply_speed`] runs in `First`,
//! before Bevy's `time_system`, and sets `Time<Virtual>`'s relative speed
//! (1, 2 or 4) and its pause, so the frame that follows a change already
//! runs at the new rate: ticks per frame are `20 x speed x min(frame,
//! 250 ms)`, still capped at `sim_driver::MAX_TICKS_PER_FRAME` (8), whose
//! overflow `DriverStats::dropped_ticks` counts. `step_sim` asks
//! [`GameSpeed::hold`] before every step: while paused it steps nothing
//! and discards the fixed clock's overstep, so a pause lands on an exact
//! tick (`--pause-at-tick T` stops with `sim.tick() == T`) and the unpause
//! does not burst. The sim never sees the clock: the replay stays
//! tick-stamped, so a match played at x4 with pauses records the same
//! replay as at x1 with the same inputs per tick.
//!
//! Clocks (the audit of every `Res<Time>` reader, docs/design/ui.md):
//!
//! | Reader | Clock |
//! | --- | --- |
//! | camera pan, zoom, `apply_camera`; the double-click window | `Time<Real>` (works paused and at x4 at the x1 rate) |
//! | the message line (`MessageState::since`), order markers, alert pings | [`UiClock`] through [`UiNow`]: real seconds that stop while paused |
//! | gait bob, rubble expiry | `Time<Virtual>`: world presentation, frozen when paused |
//! | combat FX, health bars, markers, interpolation | `Time<Fixed>`: game speed, frozen when paused |
//!
//! Lines printed: `speed: x4 tick=<n>`, `pause: on tick=<n>`, `pause: off
//! tick=<n> after_s=<s>`, and at exit `driver: ticks_frame_max=<n>
//! dropped_ticks=<d> x1_s=.. x2_s=.. x4_s=.. paused_s=..
//! orders_while_paused=<n>`.

use std::time::Duration;

use bevy::app::AppExit;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::time::TimeSystems;

use crate::palette;
use crate::sim_driver::{DriverStats, MatchOver, PendingCommands, SimHandle, match_running};

/// A game speed: the multiplier of `Time<Virtual>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Speed {
    /// 20 ticks a second.
    #[default]
    X1,
    /// 40 ticks a second.
    X2,
    /// 80 ticks a second (bounded by the 8-tick cap per frame).
    X4,
}

impl Speed {
    /// The multiplier: 1, 2 or 4.
    pub fn factor(self) -> u32 {
        match self {
            Speed::X1 => 1,
            Speed::X2 => 2,
            Speed::X4 => 4,
        }
    }

    /// `x1`, `x2` or `x4`.
    pub fn label(self) -> &'static str {
        match self {
            Speed::X1 => "x1",
            Speed::X2 => "x2",
            Speed::X4 => "x4",
        }
    }

    /// The speed of `--speed <1|2|4>`.
    pub fn from_factor(n: u32) -> Option<Speed> {
        match n {
            1 => Some(Speed::X1),
            2 => Some(Speed::X2),
            4 => Some(Speed::X4),
            _ => None,
        }
    }

    /// One step faster (`=`), saturating at x4.
    pub fn faster(self) -> Speed {
        match self {
            Speed::X1 => Speed::X2,
            Speed::X2 | Speed::X4 => Speed::X4,
        }
    }

    /// One step slower (`-`), saturating at x1.
    pub fn slower(self) -> Speed {
        match self {
            Speed::X4 => Speed::X2,
            Speed::X2 | Speed::X1 => Speed::X1,
        }
    }

    /// The chip's click: x1, x2, x4, then x1 again.
    pub fn cycle(self) -> Speed {
        match self {
            Speed::X1 => Speed::X2,
            Speed::X2 => Speed::X4,
            Speed::X4 => Speed::X1,
        }
    }
}

/// Why the game is paused. `Key` (Space, `--pause-at-tick`) is the active
/// pause: world input stays on. `Menu` is M7's Esc pause menu, which also
/// gates `hud::WorldInputSet` ([`world_input_allowed`], wired into
/// `hud::gate_world_input`); nothing sets it before M7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseReason {
    /// Space or the `--pause-at-tick` automation aid.
    Key,
    /// The pause menu (M7).
    Menu,
}

/// The game speed and pause (M6). Speed persists across Play Again and New
/// Game (M6 owner decision 6); the pause does not.
#[derive(Resource, Debug, Clone, PartialEq, Default)]
pub struct GameSpeed {
    /// The current speed.
    pub speed: Speed,
    /// `Some` while paused.
    pub paused: Option<PauseReason>,
    /// `--pause-at-tick`: pause when the sim reaches this tick.
    pub pause_at: Option<u32>,
    /// `--pause-seconds`: how long an automatic pause lasts (real time).
    pub pause_for: Option<Duration>,
    /// `Time<Real>` seconds when the current pause started.
    pub paused_since: f64,
    /// The tick the current pause holds.
    pub paused_tick: u32,
    /// `true` when the current pause came from `--pause-at-tick`.
    auto: bool,
    /// Set by [`GameSpeed::hold`] when it paused; [`print_pause_lines`]
    /// stamps the start.
    just_paused: bool,
}

impl GameSpeed {
    /// A speed with no pause.
    pub fn at(speed: Speed) -> Self {
        Self { speed, ..default() }
    }

    /// `true` while paused for any reason.
    pub fn is_paused(&self) -> bool {
        self.paused.is_some()
    }

    /// Called by `step_sim` before each step at `tick`: pauses when `tick`
    /// is the `--pause-at-tick` target, and returns `true` while paused (no
    /// step, the overstep is discarded by the caller).
    pub fn hold(&mut self, tick: u32) -> bool {
        if self.paused.is_none() && self.pause_at == Some(tick) {
            self.pause_at = None;
            self.paused = Some(PauseReason::Key);
            self.auto = true;
            self.just_paused = true;
            self.paused_tick = tick;
            println!("pause: on tick={tick}");
        }
        self.paused.is_some()
    }

    /// Space: toggle the `Key` pause at `tick` (a `Menu` pause is left to
    /// its menu). Prints the pause line.
    pub fn toggle_key_pause(&mut self, tick: u32, now: f64) {
        match self.paused {
            None => {
                self.paused = Some(PauseReason::Key);
                self.auto = false;
                self.paused_since = now;
                self.paused_tick = tick;
                println!("pause: on tick={tick}");
            }
            Some(PauseReason::Key) => self.unpause(tick, now),
            Some(PauseReason::Menu) => {}
        }
    }

    /// Leave the pause at `tick`, printing `pause: off tick=<n>
    /// after_s=<s>`.
    pub fn unpause(&mut self, tick: u32, now: f64) {
        if self.paused.take().is_some() {
            self.auto = false;
            println!(
                "pause: off tick={tick} after_s={:.1}",
                (now - self.paused_since).max(0.0)
            );
        }
    }

    /// Set the speed at `tick`, printing `speed: x4 tick=<n>` on a change.
    pub fn set_speed(&mut self, speed: Speed, tick: u32) {
        if speed != self.speed {
            self.speed = speed;
            println!("speed: {} tick={tick}", speed.label());
        }
    }
}

/// Real seconds that stop while the game is paused (M6): the message line,
/// order markers and alert pings expire on it, so a message shown before a
/// pause is still shown after it.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct UiClock {
    elapsed: Duration,
}

impl UiClock {
    /// Seconds so far.
    pub fn secs(&self) -> f64 {
        self.elapsed.as_secs_f64()
    }

    /// Advance by `by`.
    pub fn advance(&mut self, by: Duration) {
        self.elapsed += by;
    }
}

/// The UI's "now": [`UiClock`] when the [`SpeedPlugin`] runs, else
/// `Time<Real>` (tests that build one HUD plugin alone).
#[derive(SystemParam)]
pub struct UiNow<'w> {
    clock: Option<Res<'w, UiClock>>,
    real: Option<Res<'w, Time<Real>>>,
}

impl UiNow<'_> {
    /// Seconds on the UI clock.
    pub fn secs(&self) -> f64 {
        match (&self.clock, &self.real) {
            (Some(c), _) => c.secs(),
            (None, Some(r)) => r.elapsed_secs_f64(),
            (None, None) => 0.0,
        }
    }

    /// [`UiNow::secs`] as `f32`, for the marker timers.
    pub fn secs_f32(&self) -> f32 {
        self.secs() as f32
    }
}

/// What the `driver:` exit line reports beyond `DriverStats` (M6). Like
/// `DriverStats` it covers the current match only: a rebuilt match (New
/// Game, Play Again, Main Menu) resets both (`match_scope`), so each
/// match's `match_log:` line carries its own speed and pause times.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq)]
pub struct SpeedStats {
    /// Real seconds of the running match spent at each speed, unpaused.
    pub at_speed_s: [f64; 3],
    /// Real seconds spent paused.
    pub paused_s: f64,
    /// Commands queued while paused.
    pub orders_while_paused: u64,
    last_pending: usize,
}

impl SpeedStats {
    /// The `driver:` line printed at exit.
    pub fn driver_line(&self, driver: &DriverStats) -> String {
        format!(
            "driver: ticks_frame_max={} dropped_ticks={} x1_s={:.1} x2_s={:.1} x4_s={:.1} paused_s={:.1} orders_while_paused={}",
            driver.ticks_frame_max,
            driver.dropped_ticks,
            self.at_speed_s[0],
            self.at_speed_s[1],
            self.at_speed_s[2],
            self.paused_s,
            self.orders_while_paused
        )
    }
}

/// Marks the top bar's speed chip.
#[derive(Component, Debug, Clone, Copy)]
pub struct SpeedChip;

/// Marks the centred "PAUSED" banner and its scrim.
#[derive(Component, Debug, Clone, Copy)]
pub struct PausedBanner;

/// Size of the "PAUSED" banner text, logical px (stream-legible).
pub const PAUSED_FONT_PX: f32 = 32.0;

/// Installs speed and pause: [`GameSpeed`] (from `--speed` and
/// `--pause-at-tick`/`--pause-seconds`), [`UiClock`], the keys, the chip,
/// the banner and the `driver:` exit line.
#[derive(Default)]
pub struct SpeedPlugin {
    /// The starting speed and the automatic pause.
    pub start: GameSpeed,
    /// Spawn the chip and the banner (the windowed game; not in tests).
    pub ui: bool,
}

impl Plugin for SpeedPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(self.start.clone())
            .init_resource::<UiClock>()
            .init_resource::<SpeedStats>()
            .add_systems(First, apply_speed.before(TimeSystems))
            .add_systems(
                PreUpdate,
                (advance_ui_clock, count_speed_time, print_pause_lines),
            )
            .add_systems(
                Update,
                (
                    speed_keys
                        .run_if(match_running)
                        .run_if(not(resource_exists::<MatchOver>)),
                    end_timed_pause,
                    count_orders_while_paused,
                ),
            )
            .add_systems(Last, print_driver_line);
        if self.ui {
            app.add_systems(Startup, spawn_speed_ui.after(crate::hud::spawn_hud))
                .add_systems(Update, update_speed_ui);
        }
    }
}

/// `First`, before `time_system`: `Time<Virtual>` follows [`GameSpeed`],
/// so this frame's virtual delta already has the new speed or pause.
pub fn apply_speed(speed: Res<GameSpeed>, mut time: ResMut<Time<Virtual>>) {
    let factor = speed.speed.factor() as f32;
    if time.relative_speed() != factor {
        time.set_relative_speed(factor);
    }
    match (speed.is_paused(), time.is_paused()) {
        (true, false) => time.pause(),
        (false, true) => time.unpause(),
        _ => {}
    }
}

fn advance_ui_clock(speed: Res<GameSpeed>, real: Res<Time<Real>>, mut clock: ResMut<UiClock>) {
    if !speed.is_paused() {
        clock.advance(real.delta());
    }
}

fn count_speed_time(
    speed: Res<GameSpeed>,
    real: Res<Time<Real>>,
    over: Option<Res<MatchOver>>,
    state: Option<Res<State<crate::menu::AppState>>>,
    mut stats: ResMut<SpeedStats>,
) {
    if over.is_some() || state.is_some_and(|s| *s.get() != crate::menu::AppState::Skirmish) {
        return;
    }
    let dt = real.delta_secs_f64();
    if speed.is_paused() {
        stats.paused_s += dt;
    } else {
        let i = match speed.speed {
            Speed::X1 => 0,
            Speed::X2 => 1,
            Speed::X4 => 2,
        };
        stats.at_speed_s[i] += dt;
    }
}

/// Stamp the start of a pause [`GameSpeed::hold`] began inside the fixed
/// step (it has no clock there).
fn print_pause_lines(mut speed: ResMut<GameSpeed>, real: Res<Time<Real>>) {
    if speed.just_paused {
        speed.just_paused = false;
        speed.paused_since = real.elapsed_secs_f64();
    }
}

/// Space toggles the pause; `-` and `=` (and the numpad's) step the speed.
/// Active in the running match before its outcome, spectating included.
pub fn speed_keys(
    keys: Res<ButtonInput<KeyCode>>,
    real: Res<Time<Real>>,
    sim: NonSend<SimHandle>,
    mut speed: ResMut<GameSpeed>,
) {
    let tick = sim.tick();
    if keys.just_pressed(KeyCode::Space) {
        speed.toggle_key_pause(tick, real.elapsed_secs_f64());
    }
    if keys.any_just_pressed([KeyCode::Minus, KeyCode::NumpadSubtract]) {
        let s = speed.speed.slower();
        speed.set_speed(s, tick);
    }
    if keys.any_just_pressed([KeyCode::Equal, KeyCode::NumpadAdd]) {
        let s = speed.speed.faster();
        speed.set_speed(s, tick);
    }
}

/// `--pause-seconds`: end the `--pause-at-tick` pause after that many real
/// seconds.
fn end_timed_pause(real: Res<Time<Real>>, sim: NonSend<SimHandle>, mut speed: ResMut<GameSpeed>) {
    let Some(len) = speed.pause_for else {
        return;
    };
    if speed.auto && !speed.just_paused {
        let now = real.elapsed_secs_f64();
        if now - speed.paused_since >= len.as_secs_f64() {
            let tick = sim.tick();
            speed.unpause(tick, now);
        }
    }
}

fn count_orders_while_paused(
    speed: Res<GameSpeed>,
    pending: Option<Res<PendingCommands>>,
    mut stats: ResMut<SpeedStats>,
) {
    let Some(pending) = pending else {
        return;
    };
    let len = pending.len();
    if speed.is_paused() && len > stats.last_pending {
        stats.orders_while_paused += (len - stats.last_pending) as u64;
    }
    stats.last_pending = len;
}

fn print_driver_line(
    mut exits: MessageReader<AppExit>,
    stats: Res<SpeedStats>,
    driver: Option<Res<DriverStats>>,
) {
    if exits.is_empty() {
        return;
    }
    exits.clear();
    let driver = driver.map(|d| *d).unwrap_or_default();
    println!("{}", stats.driver_line(&driver));
}

/// Run condition for M7's pause menu: world input is off only while the
/// menu pauses the game (the `Key` pause is active).
pub fn world_input_allowed(speed: Option<Res<GameSpeed>>) -> bool {
    speed.is_none_or(|s| s.paused != Some(PauseReason::Menu))
}

pub fn spawn_speed_ui(
    mut commands: Commands,
    top: Query<Entity, With<crate::hud::TopBar>>,
    root: Query<Entity, With<crate::hud::HudRoot>>,
    strings: Option<Res<crate::economy_hud::UiStrings>>,
) {
    let paused = strings.as_deref().map_or_else(
        || "PAUSED".to_string(),
        |s| s.hud("speed.paused").to_string(),
    );
    if let Ok(bar) = top.single() {
        commands.entity(bar).with_children(|bar| {
            bar.spawn((
                Name::new("Speed chip"),
                SpeedChip,
                Button,
                Node {
                    padding: UiRect::horizontal(Val::Px(8.0)),
                    height: Val::Px(crate::hud::TOP_BAR_HEIGHT_PX - 4.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(palette::HUD_BUTTON),
            ))
            .observe(on_speed_chip_click)
            .with_children(|chip| {
                chip.spawn((
                    Text::new("x1"),
                    crate::fonts::hud_font(16.0),
                    TextColor(palette::HUD_TEXT),
                    Pickable::IGNORE,
                ));
            });
        });
    }
    if let Ok(root) = root.single() {
        commands.entity(root).with_children(|root| {
            root.spawn((
                Name::new("Paused banner"),
                PausedBanner,
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                // A light scrim; the world stays visible and every click
                // goes through to it (the active pause).
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
                Pickable::IGNORE,
                Visibility::Hidden,
            ))
            .with_children(|b| {
                b.spawn((
                    Text::new(paused),
                    crate::fonts::hud_font(PAUSED_FONT_PX),
                    TextColor(palette::HUD_TEXT),
                    Pickable::IGNORE,
                ));
            });
        });
    }
}

fn on_speed_chip_click(
    click: On<bevy::picking::events::Pointer<bevy::picking::events::Click>>,
    sim: NonSend<SimHandle>,
    mut speed: ResMut<GameSpeed>,
) {
    if click.event().event.button != bevy::picking::pointer::PointerButton::Primary {
        return;
    }
    let s = speed.speed.cycle();
    speed.set_speed(s, sim.tick());
}

fn update_speed_ui(
    speed: Res<GameSpeed>,
    chips: Query<&Children, With<SpeedChip>>,
    mut texts: Query<(&mut Text, &mut TextColor)>,
    mut banner: Query<&mut Visibility, With<PausedBanner>>,
) {
    if !speed.is_changed() {
        return;
    }
    for children in &chips {
        for child in children.iter() {
            if let Ok((mut text, mut color)) = texts.get_mut(child) {
                let label = speed.speed.label();
                if text.0 != label {
                    text.0 = label.to_owned();
                }
                color.0 = if speed.speed == Speed::X4 {
                    palette::HUD_AMBER
                } else {
                    palette::HUD_TEXT
                };
            }
        }
    }
    for mut v in &mut banner {
        *v = if speed.is_paused() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim_driver::{LocalInput, ScenarioInput, SimPlugin};
    use bevy::time::TimeUpdateStrategy;
    use rules::Rules;
    use std::path::{Path, PathBuf};

    fn data() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    fn rules() -> Rules {
        Rules::load(data()).unwrap()
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("eonmark-speed-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    /// A `MinimalPlugins` app with the sim, the speed plugin (no UI) and a
    /// manual clock, after its first update; every later update is
    /// `frame` long.
    fn speed_app(handle: SimHandle, start: GameSpeed, record: Option<PathBuf>) -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(SimPlugin::new(handle, Box::new(LocalInput)).recording(record))
            .add_plugins(SpeedPlugin { start, ui: false });
        app.update();
        app
    }

    fn frame(app: &mut App, by: Duration) {
        app.insert_resource(TimeUpdateStrategy::ManualDuration(by));
        app.update();
    }

    fn tick(app: &App) -> u32 {
        app.world().non_send::<SimHandle>().tick()
    }

    fn skirmish() -> SimHandle {
        SimHandle::skirmish(rules(), 1)
    }

    fn press(app: &mut App, key: KeyCode) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release_all();
        keys.clear();
        keys.press(key);
    }

    fn release_all(app: &mut App) {
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.release_all();
        keys.clear();
    }

    /// ROADMAP M6 line 5, "x4 works": at x1, x2 and x4 a 50 ms frame
    /// steps 1, 2 and 4 ticks.
    #[test]
    fn x1_x2_x4_step_1_2_4_ticks_per_50ms() {
        for (speed, per_frame) in [(Speed::X1, 1), (Speed::X2, 2), (Speed::X4, 4)] {
            let mut app = speed_app(skirmish(), GameSpeed::at(speed), None);
            for n in 1..=10u32 {
                frame(&mut app, Duration::from_millis(50));
                assert_eq!(tick(&app), n * per_frame, "{speed:?} frame {n}");
            }
            let stats = *app.world().resource::<DriverStats>();
            assert_eq!(stats.dropped_ticks, 0, "{speed:?}");
        }
    }

    /// "Pause works": `--pause-at-tick 37` stops on tick 37 exactly at x4,
    /// a second of frames steps nothing, and the unpause steps one frame's
    /// worth (no burst of the paused time).
    #[test]
    fn pause_steps_nothing_lands_exactly_and_unpause_does_not_burst() {
        let mut start = GameSpeed::at(Speed::X4);
        start.pause_at = Some(37);
        let mut app = speed_app(skirmish(), start, None);
        for _ in 0..20 {
            frame(&mut app, Duration::from_millis(50));
        }
        assert_eq!(tick(&app), 37, "landed on the tick, not a frame boundary");
        assert!(app.world().resource::<GameSpeed>().is_paused());
        assert!(app.world().resource::<Time<Virtual>>().is_paused());
        for _ in 0..4 {
            frame(&mut app, Duration::from_millis(250));
        }
        assert_eq!(tick(&app), 37, "nothing steps while paused");
        app.world_mut().resource_mut::<GameSpeed>().unpause(37, 0.0);
        frame(&mut app, Duration::from_millis(50));
        assert_eq!(tick(&app), 41, "one x4 frame, no burst");
        frame(&mut app, Duration::from_millis(50));
        assert_eq!(tick(&app), 45);
    }

    /// The 8-tick cap per frame binds at x4: a 250 ms frame asks for 20
    /// ticks, 8 run and 12 are counted dropped.
    #[test]
    fn x4_with_250ms_frames_runs_8_and_counts_12_dropped() {
        let mut app = speed_app(skirmish(), GameSpeed::at(Speed::X4), None);
        frame(&mut app, Duration::from_millis(250));
        assert_eq!(tick(&app), crate::sim_driver::MAX_TICKS_PER_FRAME);
        let stats = *app.world().resource::<DriverStats>();
        assert_eq!(stats.dropped_ticks, 12);
        frame(&mut app, Duration::from_millis(250));
        assert_eq!(tick(&app), 16);
        let stats = *app.world().resource::<DriverStats>();
        assert_eq!(stats.dropped_ticks, 24);
        assert_eq!(stats.ticks_frame_max, 8);
    }

    /// Space toggles the pause; `-` and `=` step the speed and saturate.
    #[test]
    fn space_key_toggles_pause_and_minus_equals_step_speed() {
        let mut app = speed_app(skirmish(), GameSpeed::default(), None);
        frame(&mut app, Duration::from_millis(50));
        press(&mut app, KeyCode::Space);
        frame(&mut app, Duration::from_millis(50));
        let paused_at = tick(&app);
        assert!(app.world().resource::<GameSpeed>().is_paused());
        release_all(&mut app);
        for _ in 0..5 {
            frame(&mut app, Duration::from_millis(50));
        }
        assert_eq!(tick(&app), paused_at);
        press(&mut app, KeyCode::Space);
        frame(&mut app, Duration::from_millis(50));
        assert!(!app.world().resource::<GameSpeed>().is_paused());
        for (key, want) in [
            (KeyCode::Equal, Speed::X2),
            (KeyCode::NumpadAdd, Speed::X4),
            (KeyCode::Equal, Speed::X4),
            (KeyCode::Minus, Speed::X2),
            (KeyCode::NumpadSubtract, Speed::X1),
            (KeyCode::Minus, Speed::X1),
        ] {
            press(&mut app, key);
            frame(&mut app, Duration::from_millis(50));
            assert_eq!(app.world().resource::<GameSpeed>().speed, want, "{key:?}");
        }
    }

    /// The keys do nothing in the main menu and after the outcome.
    #[test]
    fn speed_keys_ignored_in_menu_and_after_outcome() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(SimPlugin::new(skirmish(), Box::new(LocalInput)))
            .insert_state(crate::menu::AppState::Menu)
            .add_plugins(SpeedPlugin::default());
        app.update();
        for key in [KeyCode::Space, KeyCode::Equal] {
            press(&mut app, key);
            frame(&mut app, Duration::from_millis(50));
        }
        assert_eq!(*app.world().resource::<GameSpeed>(), GameSpeed::default());
        app.world_mut()
            .resource_mut::<NextState<crate::menu::AppState>>()
            .set(crate::menu::AppState::Skirmish);
        release_all(&mut app);
        frame(&mut app, Duration::from_millis(50));
        app.world_mut().insert_resource(MatchOver {
            result: sim::MatchResult {
                outcome: sim::Outcome::Draw,
                reason: sim::OutcomeReason::Surrender,
                tick: 1,
            },
            hash: 0,
            replay: None,
        });
        for key in [KeyCode::Space, KeyCode::Equal] {
            press(&mut app, key);
            frame(&mut app, Duration::from_millis(50));
        }
        assert_eq!(*app.world().resource::<GameSpeed>(), GameSpeed::default());
    }

    /// The active pause: the camera pans while paused, and at x4, at the
    /// x1 rate (it reads `Time<Real>`).
    #[test]
    fn camera_pans_while_paused_at_the_x1_rate() {
        for (speed, paused) in [(Speed::X1, false), (Speed::X1, true), (Speed::X4, false)] {
            let mut start = GameSpeed::at(speed);
            if paused {
                start.paused = Some(PauseReason::Key);
            }
            let mut app = speed_app(skirmish(), start, None);
            app.add_systems(Update, crate::camera::pan_keyboard);
            let cam = app
                .world_mut()
                .spawn(crate::camera::RtsCamera::default())
                .id();
            press(&mut app, KeyCode::KeyD);
            frame(&mut app, Duration::from_millis(100));
            let c = app.world().get::<crate::camera::RtsCamera>(cam).unwrap();
            let want = c.zoom * crate::camera::PAN_SPEED_PER_ZOOM * 0.1;
            assert!(
                (c.focus.x - want).abs() < 1e-4,
                "{speed:?} paused={paused}: {} vs {want}",
                c.focus.x
            );
        }
    }

    /// The double-click window and the camera read `Time<Real>`, which runs
    /// at the frame rate whatever the speed and while paused, while
    /// `Time<Virtual>` (world presentation) runs x4 or stops, and the
    /// `UiClock` stops while paused.
    #[test]
    fn real_time_runs_paused_and_at_x4_while_virtual_and_ui_clocks_follow_the_game() {
        let mut app = speed_app(skirmish(), GameSpeed::at(Speed::X4), None);
        let real0 = app.world().resource::<Time<Real>>().elapsed_secs_f64();
        let virt0 = app.world().resource::<Time<Virtual>>().elapsed_secs_f64();
        frame(&mut app, Duration::from_millis(100));
        let real = app.world().resource::<Time<Real>>().elapsed_secs_f64() - real0;
        let virt = app.world().resource::<Time<Virtual>>().elapsed_secs_f64() - virt0;
        assert!(
            (real - 0.1).abs() < 1e-9 && (virt - 0.4).abs() < 1e-9,
            "{real} {virt}"
        );
        app.world_mut().resource_mut::<GameSpeed>().paused = Some(PauseReason::Key);
        let ui0 = app.world().resource::<UiClock>().secs();
        let virt0 = app.world().resource::<Time<Virtual>>().elapsed_secs_f64();
        let real0 = app.world().resource::<Time<Real>>().elapsed_secs_f64();
        frame(&mut app, Duration::from_millis(100));
        assert_eq!(app.world().resource::<UiClock>().secs(), ui0);
        assert_eq!(
            app.world().resource::<Time<Virtual>>().elapsed_secs_f64(),
            virt0
        );
        let real = app.world().resource::<Time<Real>>().elapsed_secs_f64() - real0;
        assert!((real - 0.1).abs() < 1e-9);
    }

    /// The double-click window reads `Time<Real>` (M6 owner decision 7:
    /// selection works during the active pause): two primary clicks on the
    /// same unit 100 ms of real time apart are a double click while paused
    /// (the game clock stands still) and at x4 (it runs 400 ms).
    #[test]
    fn double_click_works_paused_and_at_x4() {
        use crate::selection::{DragBox, LastClick, Selection, on_unit_click};
        use bevy::camera::NormalizedRenderTarget;
        use bevy::picking::backend::HitData;
        use bevy::picking::events::{Click, Pointer};
        use bevy::picking::pointer::{Location, PointerButton, PointerId};
        let mut paused = GameSpeed::at(Speed::X1);
        paused.paused = Some(PauseReason::Key);
        for (name, start) in [("paused", paused), ("x4", GameSpeed::at(Speed::X4))] {
            let mut app = speed_app(skirmish(), start, None);
            app.init_resource::<Selection>()
                .init_resource::<LastClick>()
                .init_resource::<crate::hud::PointerOverUi>()
                .init_resource::<DragBox>()
                .init_resource::<crate::orders::OrderMode>()
                .add_observer(on_unit_click);
            app.world_mut().spawn((
                crate::camera::RtsCamera::default(),
                Camera::default(),
                GlobalTransform::default(),
            ));
            let unit = app
                .world_mut()
                .spawn(crate::present::UnitRef(sim::UnitId(1)))
                .id();
            let click = |app: &mut App| {
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
                        duration: Duration::ZERO,
                        count: 1,
                    },
                    unit,
                ));
            };
            frame(&mut app, Duration::from_millis(50));
            click(&mut app);
            assert_eq!(
                app.world().resource::<LastClick>().previous.map(|(e, _)| e),
                Some(unit),
                "{name}: the first click is remembered"
            );
            assert!(
                app.world()
                    .resource::<Selection>()
                    .units
                    .contains(&sim::UnitId(1)),
                "{name}"
            );
            frame(&mut app, Duration::from_millis(100));
            click(&mut app);
            assert_eq!(
                app.world().resource::<LastClick>().previous,
                None,
                "{name}: the second click 100 ms later is a double click"
            );
        }
    }

    /// `hud::WorldInputSet` stops while M7's pause menu pauses the game
    /// (`PauseReason::Menu`) and runs during the active `Key` pause.
    #[test]
    fn the_menu_pause_gates_world_input_and_the_key_pause_does_not() {
        use crate::hud::WorldInputSet;
        #[derive(Resource, Default)]
        struct WorldInputRuns(u32);
        for (reason, want) in [
            (None, 1),
            (Some(PauseReason::Key), 1),
            (Some(PauseReason::Menu), 0),
        ] {
            let mut start = GameSpeed::at(Speed::X1);
            start.paused = reason;
            let mut app = speed_app(skirmish(), start, None);
            app.init_resource::<WorldInputRuns>().add_systems(
                Update,
                (|mut runs: ResMut<WorldInputRuns>| runs.0 += 1).in_set(WorldInputSet),
            );
            crate::hud::gate_world_input(&mut app);
            frame(&mut app, Duration::from_millis(50));
            assert_eq!(
                app.world().resource::<WorldInputRuns>().0,
                want,
                "{reason:?}"
            );
        }
    }

    /// The banner's text is the strings file's `speed.paused` (the
    /// stream-legibility copy pass edits it there).
    #[test]
    fn the_paused_banner_reads_its_string() {
        let mut strings = rules::strings::Strings::load(data(), "en").unwrap();
        strings
            .hud
            .insert("speed.paused".into(), "PAUSE TEST".into());
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
            .init_resource::<ButtonInput<KeyCode>>()
            .insert_resource(crate::economy_hud::UiStrings(strings))
            .add_plugins(SimPlugin::new(skirmish(), Box::new(LocalInput)))
            .add_systems(Startup, crate::hud::spawn_hud)
            .add_plugins(SpeedPlugin {
                start: GameSpeed::default(),
                ui: true,
            });
        app.update();
        let mut q = app.world_mut().query::<(&PausedBanner, &Children)>();
        let children: Vec<Entity> = q.iter(app.world()).flat_map(|(_, c)| c.to_vec()).collect();
        let texts: Vec<String> = children
            .iter()
            .filter_map(|c| app.world().get::<Text>(*c).map(|t| t.0.clone()))
            .collect();
        assert_eq!(texts, ["PAUSE TEST"]);
    }

    /// A message shown before a 5 s pause is still shown after it, and goes
    /// once its own 4 s of unpaused time are up.
    #[test]
    fn a_message_shown_before_a_5s_pause_is_still_shown_after() {
        use crate::economy_hud::{MESSAGE_SECONDS, MessageState, UiStrings, collect_messages};
        let mut app = speed_app(skirmish(), GameSpeed::default(), None);
        app.insert_resource(UiStrings(
            rules::strings::Strings::load(data(), "en").unwrap(),
        ))
        .init_resource::<MessageState>()
        .add_systems(Update, collect_messages);
        frame(&mut app, Duration::from_millis(50));
        let now = app.world().resource::<UiClock>().secs();
        app.world_mut()
            .resource_mut::<MessageState>()
            .set("Not enough Ore".into(), now);
        app.world_mut().resource_mut::<GameSpeed>().paused = Some(PauseReason::Key);
        for _ in 0..20 {
            frame(&mut app, Duration::from_millis(250));
        }
        assert_eq!(
            app.world().resource::<MessageState>().text,
            "Not enough Ore",
            "5 s paused did not age it"
        );
        app.world_mut().resource_mut::<GameSpeed>().paused = None;
        let steps = (MESSAGE_SECONDS / 0.25) as usize - 2;
        for _ in 0..steps {
            frame(&mut app, Duration::from_millis(250));
        }
        assert_eq!(
            app.world().resource::<MessageState>().text,
            "Not enough Ore"
        );
        for _ in 0..4 {
            frame(&mut app, Duration::from_millis(250));
        }
        assert_eq!(app.world().resource::<MessageState>().text, "", "expired");
    }

    /// Orders given while paused queue and apply after the unpause; the
    /// `driver:` line counts them.
    #[test]
    fn orders_given_while_paused_apply_after_unpause() {
        let mut app = speed_app(skirmish(), GameSpeed::default(), None);
        frame(&mut app, Duration::from_millis(50));
        app.world_mut().resource_mut::<GameSpeed>().paused = Some(PauseReason::Key);
        frame(&mut app, Duration::from_millis(50));
        let at = tick(&app);
        let applied = |app: &App| {
            app.world()
                .non_send::<SimHandle>()
                .view()
                .commands_applied()
        };
        let before = applied(&app);
        app.world_mut()
            .resource_mut::<PendingCommands>()
            .push_local(sim::Command::Stop {
                units: vec![sim::UnitId(1)],
            });
        for _ in 0..10 {
            frame(&mut app, Duration::from_millis(50));
        }
        assert_eq!(tick(&app), at);
        assert_eq!(app.world().resource::<PendingCommands>().len(), 1, "queued");
        assert_eq!(app.world().resource::<SpeedStats>().orders_while_paused, 1);
        app.world_mut().resource_mut::<GameSpeed>().paused = None;
        let delay = app.world().non_send::<SimHandle>().setup().cmd_delay;
        for _ in 0..=delay {
            frame(&mut app, Duration::from_millis(50));
        }
        assert!(app.world().resource::<PendingCommands>().is_empty());
        assert_eq!(applied(&app), before + 1, "applied after the unpause");
        let line = app
            .world()
            .resource::<SpeedStats>()
            .driver_line(app.world().resource::<DriverStats>());
        assert!(
            line.starts_with("driver: ticks_frame_max=1 dropped_ticks=0 x1_s="),
            "{line}"
        );
        assert!(line.ends_with(" orders_while_paused=1"), "{line}");
    }

    /// ROADMAP M6 line 5 headless: `scripted_match` recorded through the
    /// real `Recorder` at x1, and at x4 with a pause at tick 600, gives two
    /// replays that both verify to `scripted_match.hash`.
    #[test]
    fn x4_with_a_pause_records_the_x1_replay() {
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let golden = std::fs::read_to_string(fixtures.join("scripted_match.hash")).unwrap();
        let golden = u64::from_str_radix(golden.trim().trim_start_matches("0x"), 16).unwrap();
        for (name, speed, pause) in [("x1", Speed::X1, None), ("x4p", Speed::X4, Some(600))] {
            let path = temp(&format!("scripted_{name}.eonreplay"));
            let _ = std::fs::remove_file(&path);
            let rules = rules();
            let (setup, stream, _) = sim::scenarios::by_name("scripted_match", &rules).unwrap();
            let mut start = GameSpeed::at(speed);
            start.pause_at = pause;
            start.pause_for = pause.map(|_| Duration::from_millis(300));
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
                .init_resource::<ButtonInput<KeyCode>>()
                .add_plugins(
                    SimPlugin::new(
                        SimHandle::from_setup(setup, rules),
                        Box::new(ScenarioInput::new(stream)),
                    )
                    .recording(Some(path.clone())),
                )
                .add_plugins(SpeedPlugin { start, ui: false });
            app.update();
            let mut paused_frames = 0;
            for _ in 0..2000 {
                if app.world().contains_resource::<MatchOver>() {
                    break;
                }
                if app.world().resource::<GameSpeed>().is_paused() {
                    paused_frames += 1;
                    assert_eq!(tick(&app), 600, "held on the pause tick");
                }
                frame(&mut app, Duration::from_millis(50));
            }
            if pause.is_some() {
                assert!(paused_frames >= 5, "{paused_frames} paused frames");
            }
            let over = app.world().resource::<MatchOver>().clone();
            assert_eq!(over.hash, golden, "{name}");
            let data = data();
            let outcome = sim::replay::verify_with(
                &path,
                &data,
                Box::new(|setup: &sim::MatchSetup, rules: &Rules| {
                    ai::controller_for(setup, rules).map_err(|e| e.to_string())
                }),
            )
            .unwrap();
            assert_eq!(
                outcome,
                sim::replay::VerifyOutcome::Ok {
                    final_hash: golden,
                    ticks: over.result.tick
                },
                "{name}"
            );
            let _ = std::fs::remove_file(&path);
        }
    }
}
