//! The match lifecycle (M6): New Game, Play Again and Main Menu rebuild
//! the match in place, in the same window, so OBS's window capture
//! survives them (M6 owner decision 10).
//!
//! The first match is still built at launch (`app::build_match`), so
//! `SimPlugin::build` and the start-up readers that need a `SimHandle`
//! are unchanged. Any later match comes from a [`MatchRequest`]: New Game
//! (the main menu, `menu::new_game`), Play Again (the game-over screen,
//! same difficulty and style, a fresh seed, Random drawn again) and Main
//! Menu (a fresh unstarted match the menu shows). [`apply_match_request`]
//! takes it (exclusive) on entering `AppState::Skirmish` or
//! `AppState::Menu`, or at once while the match runs (Play Again), and
//! [`rebuild_match`]:
//!
//! 1. [`teardown_match`]: finish a live recorder (`finish_recorder`, so
//!    the old replay keeps its trailer), clear `Messages<SimEventMsg>` and
//!    `Messages<ReplayFinished>`, despawn every [`MatchScoped`] entity
//!    (units, buildings and sites, their banners and Seat flags, health
//!    bars, selection rings, order markers, attrition diamonds, supply
//!    rings and rubble: the marker is a required component of each), and
//!    reset the per-match resources ([`MatchResets`]);
//! 2. insert a new `SimHandle` (`try_from_setup`), `LocalInput`,
//!    `PendingCommands::for_setup`, `LocalSeat` and a `PendingRecording`
//!    under the session's replay directory, discard the fixed clock's
//!    overstep, unpause (the speed is kept, M6 owner decision 6), remove
//!    `MatchOver` and hide the game-over screen, point the camera at the
//!    local player's first units, print the `match:` line and write
//!    [`MatchStarted`] (an audio hook for M7).
//!
//! The ground, the terrain props and the field material stay: every match
//! is on the same map, and the field and the minimap repaint themselves
//! when the new sim's border sources differ from the ones they show.

use std::path::PathBuf;

use bevy::ecs::message::Messages;
use bevy::prelude::*;
use sim::MatchSetup;

use crate::menu::AppState;
use crate::menu::main_menu::PendingMatch;
use crate::menu::new_game::NewGameSettings;
use crate::sim_driver::{
    ActiveInput, DriverStats, LOCAL_PLAYER, LocalInput, LocalSeat, MatchOver, PendingCommands,
    PendingRecording, ReplayFinished, ReplayRecorder, SimEventMsg, SimHandle, SimStepped,
    finish_recorder, new_replay_path,
};

/// On every per-match entity; [`teardown_match`] despawns them all.
#[derive(Component, Debug, Default, Clone, Copy)]
pub struct MatchScoped;

/// A match to build in place of the current one (New Game, Play Again,
/// Main Menu).
#[derive(Resource, Debug, Clone)]
pub struct MatchRequest {
    /// Its header: seed, seats, map.
    pub setup: MatchSetup,
    /// Print its `match:` line (no state transition will).
    pub announce: bool,
}

/// Where this session's replays go (`--replay-dir`, else the platform
/// data directory); every rebuilt match records there.
#[derive(Resource, Debug, Clone)]
pub struct ReplayDirectory(pub PathBuf);

/// Written once a rebuilt match is ready (M7's audio cue).
#[derive(Message, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchStarted {
    /// The new match's seed.
    pub seed: u64,
}

/// The per-match resources a rebuild resets to their default, each
/// registered by [`MatchAppExt::init_match_resource`].
#[derive(Resource, Default)]
pub struct MatchResets(Vec<fn(&mut World)>);

/// `App::init_match_resource::<T>()`: init `T` and reset it to its
/// default on every rebuild.
pub trait MatchAppExt {
    /// Init `T` and register its reset.
    fn init_match_resource<T: Resource + Default>(&mut self) -> &mut Self;
}

fn reset<T: Resource + Default>(world: &mut World) {
    if world.contains_resource::<T>() {
        world.insert_resource(T::default());
    }
}

impl MatchAppExt for App {
    fn init_match_resource<T: Resource + Default>(&mut self) -> &mut Self {
        self.init_resource::<T>();
        self.init_resource::<MatchResets>();
        self.world_mut()
            .resource_mut::<MatchResets>()
            .0
            .push(reset::<T>);
        self
    }
}

/// The lifecycle: [`MatchScoped`] on every per-match entity kind, the
/// per-match resources, and [`apply_match_request`] on entering a state
/// and while the match runs.
pub struct MatchScopePlugin;

impl Plugin for MatchScopePlugin {
    fn build(&self, app: &mut App) {
        // The marker rides on each per-match entity kind (required
        // components), so no spawn site has to remember it.
        app.register_required_components::<crate::present::UnitRef, MatchScoped>()
            .register_required_components::<crate::present::UnitBanner, MatchScoped>()
            .register_required_components::<crate::buildings::BuildingRef, MatchScoped>()
            .register_required_components::<crate::buildings::Banner, MatchScoped>()
            .register_required_components::<crate::buildings::SeatFlag, MatchScoped>()
            .register_required_components::<crate::buildings::Rubble, MatchScoped>()
            .register_required_components::<crate::health_bars::HealthBar, MatchScoped>()
            .register_required_components::<crate::markers::AttritionMarker, MatchScoped>()
            .register_required_components::<crate::markers::SupplyRing, MatchScoped>()
            .register_required_components::<crate::selection::SelectionRing, MatchScoped>()
            .register_required_components::<crate::selection::MoveMarker, MatchScoped>()
            .register_required_components::<crate::alert::AlertPing, MatchScoped>();
        app.init_match_resource::<crate::present::UnitEntities>()
            .init_match_resource::<crate::buildings::BuildingEntities>()
            .init_match_resource::<crate::buildings::BuildingExtras>()
            .init_match_resource::<crate::health_bars::HealthBars>()
            .init_match_resource::<crate::markers::AttritionMarkers>()
            .init_match_resource::<crate::markers::SupplyRings>()
            .init_match_resource::<crate::selection::SelectionRings>()
            .init_match_resource::<crate::selection::Selection>()
            .init_match_resource::<crate::selection::DragBox>()
            .init_match_resource::<crate::selection::LastClick>()
            .init_match_resource::<crate::orders::OrderMode>()
            .init_match_resource::<crate::orders::IssuedAttacks>()
            .init_match_resource::<crate::command_card::CardContent>()
            .init_match_resource::<crate::economy_hud::MessageState>()
            .init_match_resource::<crate::alert::Alerts>()
            .init_match_resource::<DriverStats>()
            .init_match_resource::<crate::speed::SpeedStats>()
            .init_match_resource::<SimStepped>()
            .add_message::<MatchStarted>()
            .add_systems(
                OnEnter(AppState::Skirmish),
                apply_match_request.before(crate::menu::announce_match),
            )
            .add_systems(
                OnEnter(AppState::Menu),
                apply_match_request.before(crate::menu::main_menu::spawn_main_menu),
            )
            .add_systems(
                Update,
                apply_match_request
                    .run_if(match_pending)
                    .run_if(in_state(AppState::Skirmish)),
            );
    }
}

/// Take a pending [`MatchRequest`], or a [`PendingMatch`] from the menu or
/// the game-over screen built from the `NewGameSettings` (New Game at the
/// menu's seed; Play Again and Main Menu at a fresh one, which the
/// settings then show), and [`rebuild_match`] with it.
pub fn apply_match_request(world: &mut World) {
    let (setup, announce) = if let Some(request) = world.remove_resource::<MatchRequest>() {
        (request.setup, request.announce)
    } else if let Some(pending) = world.remove_resource::<PendingMatch>() {
        let announce = pending == PendingMatch::PlayAgain;
        let Some(mut settings) = world.get_resource::<NewGameSettings>().cloned() else {
            return;
        };
        if pending != PendingMatch::NewGame {
            settings.offer_seed(crate::app::fresh_seed());
        }
        let rules = world.non_send::<SimHandle>().rules().clone();
        let built = settings.setup(&rules, settings.seed);
        world.insert_resource(crate::alert::StyleHidden(settings.hides_style()));
        world.insert_resource(settings);
        match built {
            Ok(setup) => (setup, announce),
            Err(e) => {
                eprintln!("eonmark: cannot start the match: {e}");
                return;
            }
        }
    } else {
        return;
    };
    if let Err(e) = rebuild_match(world, setup, announce) {
        eprintln!("eonmark: cannot start the match: {e}");
    }
}

/// Whether a match request of either kind is pending.
pub fn match_pending(
    request: Option<Res<MatchRequest>>,
    pending: Option<Res<PendingMatch>>,
) -> bool {
    request.is_some() || pending.is_some()
}

/// Step 1 of a rebuild (and of nothing else): finish a live recorder,
/// clear the sim's messages, despawn every [`MatchScoped`] entity and
/// reset the registered per-match resources.
pub fn teardown_match(world: &mut World) {
    if world.contains_resource::<ReplayRecorder>() && world.contains_non_send::<SimHandle>() {
        world.resource_scope(|world, mut recorder: Mut<ReplayRecorder>| {
            let sim = world.non_send::<SimHandle>();
            finish_recorder(sim, &mut recorder);
        });
    }
    if let Some(mut m) = world.get_resource_mut::<Messages<SimEventMsg>>() {
        m.clear();
    }
    if let Some(mut m) = world.get_resource_mut::<Messages<ReplayFinished>>() {
        m.clear();
    }
    let scoped: Vec<Entity> = world
        .query_filtered::<Entity, With<MatchScoped>>()
        .iter(world)
        .collect();
    for entity in scoped {
        if let Ok(e) = world.get_entity_mut(entity) {
            e.despawn();
        }
    }
    let resets = world
        .get_resource::<MatchResets>()
        .map(|r| r.0.clone())
        .unwrap_or_default();
    for reset in resets {
        reset(world);
    }
}

/// Tear the current match down and build `setup` in its place (see the
/// module docs); `announce` prints its `match:` line (a rebuild entering
/// `Skirmish` is announced by the transition, `menu::announce_match`).
///
/// # Errors
///
/// An unknown bot or personality in `setup`; the old match is then torn
/// down and nothing replaces it.
pub fn rebuild_match(world: &mut World, setup: MatchSetup, announce: bool) -> Result<(), String> {
    teardown_match(world);
    let rules = world.non_send::<SimHandle>().rules().clone();
    let handle = SimHandle::try_from_setup(setup, rules)?;
    let path = world
        .get_resource::<ReplayDirectory>()
        .map(|d| new_replay_path(&d.0, handle.seed()));
    let seed = handle.seed();
    let pending = PendingCommands::for_setup(handle.setup());
    let seat = LocalSeat::of(handle.setup());
    let hidden = world
        .get_resource::<crate::alert::StyleHidden>()
        .is_some_and(|h| h.0);
    let line = crate::menu::match_line(handle.setup(), hidden);
    let focus = first_units_centroid(&handle);
    world.insert_non_send(handle);
    world.insert_resource(ActiveInput(Box::new(LocalInput)));
    world.insert_resource(pending);
    world.insert_resource(seat);
    world.insert_resource(PendingRecording(path));
    world.remove_resource::<MatchOver>();
    if let Some(mut fixed) = world.get_resource_mut::<Time<Fixed>>() {
        fixed.discard_overstep(std::time::Duration::MAX);
    }
    if let Some(mut speed) = world.get_resource_mut::<crate::speed::GameSpeed>() {
        speed.paused = None;
        speed.pause_at = None;
    }
    crate::outcome::hide_game_over(world);
    if let Some(focus) = focus {
        let mut cams = world.query::<&mut crate::camera::RtsCamera>();
        for mut cam in cams.iter_mut(world) {
            cam.focus = Vec3::new(focus.x, 0.0, focus.z);
        }
    }
    // A rebuilt match is framed on its own units, not on a scenario's.
    world.remove_resource::<crate::camera::StartFraming>();
    // Play Again stays in `Skirmish`: no state transition announces it.
    if announce && let Some(line) = line {
        println!("{line}");
    }
    world.write_message(MatchStarted { seed });
    Ok(())
}

/// The centroid of the local player's units in a fresh match (the camera
/// framing every rebuilt match starts with).
fn first_units_centroid(handle: &SimHandle) -> Option<Vec3> {
    let view = handle.view();
    let (sum, n) = view
        .units()
        .filter(|u| u.owner == LOCAL_PLAYER)
        .map(|u| crate::present::world_from_sim(u.pos, 0.0))
        .fold((Vec3::ZERO, 0u32), |(s, n), p| (s + p, n + 1));
    (n > 0).then(|| sum / n as f32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::new_game::{NewGameSettings, StyleChoice};
    use crate::sim_driver::SimPlugin;
    use bevy::time::TimeUpdateStrategy;
    use rules::Rules;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    fn data() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    fn rules() -> Rules {
        Rules::load(data()).unwrap()
    }

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("eonmark-match-scope-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The lifecycle on `MinimalPlugins` with states, the sim recording
    /// under `replays`, the match scope and the speed plugin, after
    /// start-up; each later update is 50 ms, one tick.
    fn lifecycle_app(handle: SimHandle, state: AppState, replays: &Path) -> App {
        let path = new_replay_path(replays, handle.seed());
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::ZERO))
            .init_resource::<ButtonInput<KeyCode>>()
            .add_plugins(SimPlugin::new(handle, Box::new(LocalInput)).recording(Some(path)))
            .insert_state(state)
            .insert_resource(ReplayDirectory(replays.to_path_buf()))
            .add_plugins(crate::speed::SpeedPlugin::default())
            .add_plugins(MatchScopePlugin);
        app.update();
        app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_millis(
            50,
        )));
        app
    }

    fn run_to(app: &mut App, tick: u32) {
        while app.world().non_send::<SimHandle>().tick() < tick {
            app.update();
        }
    }

    fn scoped(app: &mut App) -> usize {
        app.world_mut()
            .query_filtered::<(), With<MatchScoped>>()
            .iter(app.world())
            .count()
    }

    fn header_bots() -> Box<sim::replay::ControllerFactory<'static>> {
        Box::new(|setup: &MatchSetup, rules: &Rules| {
            ai::controller_for(setup, rules).map_err(|e| e.to_string())
        })
    }

    fn verify_ok(path: &Path) -> (u64, u32) {
        match sim::replay::verify_with(path, &data(), header_bots()).unwrap() {
            sim::replay::VerifyOutcome::Ok { final_hash, ticks } => (final_hash, ticks),
            other => panic!("{}: {other}", path.display()),
        }
    }

    fn replays_in(dir: &Path) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        v.sort();
        v
    }

    /// Play Again leaves nothing of the old match: app A plays seed 5 for
    /// 300 ticks, rebuilds to seed 9 and plays 300 ticks; app B plays seed
    /// 9 for 300 ticks from a fresh start. The hashes agree, no per-match
    /// entity or selection survives, no stale event is read, and both of
    /// A's replays verify (the first was finished at the rebuild).
    #[test]
    fn play_again_leaves_nothing_of_the_old_match() {
        let r = rules();
        let da = dir("a");
        let mut a = lifecycle_app(
            SimHandle::vs_bot(r.clone(), 5, "standard", "rush"),
            AppState::Skirmish,
            &da,
        );
        run_to(&mut a, 300);
        // Per-match entities and state of the old match.
        a.world_mut().spawn(crate::present::UnitRef(sim::UnitId(1)));
        a.world_mut()
            .spawn(crate::selection::MoveMarker { expires_at: 9.0 });
        a.world_mut()
            .resource_mut::<crate::selection::Selection>()
            .units
            .insert(sim::UnitId(1));
        assert_eq!(scoped(&mut a), 2);
        let old_hash = a.world().non_send::<SimHandle>().hash();
        // An event of the old match still queued when the rebuild comes (a
        // hit on P0's first Yeoman: `UnitId(1)` exists again in the new
        // match, so a reader would raise a false alert from it).
        a.world_mut().write_message(SimEventMsg {
            tick: 300,
            event: sim::SimEvent::Hit {
                attacker: sim::TargetId::Unit(sim::UnitId(6)),
                target: sim::TargetId::Unit(sim::UnitId(1)),
                damage: 1,
            },
        });
        assert!(
            !a.world().resource::<Messages<SimEventMsg>>().is_empty(),
            "the old match has an event to leak"
        );
        let speed_before = *a.world().resource::<crate::speed::SpeedStats>();
        assert!(
            speed_before.at_speed_s[0] > 0.0,
            "the old match ran at x1: {speed_before:?}"
        );
        a.world_mut().insert_resource(MatchRequest {
            setup: MatchSetup::vs_bot(&r, 9, "standard", "rush"),
            announce: true,
        });
        a.update();
        assert_eq!(scoped(&mut a), 0, "every per-match entity despawned");
        assert!(
            a.world()
                .resource::<crate::selection::Selection>()
                .units
                .is_empty()
        );
        // Nothing steps between the `Update` rebuild and here, so any queued
        // event is the old match's.
        assert_eq!(a.world().non_send::<SimHandle>().tick(), 0);
        assert!(
            a.world().resource::<Messages<SimEventMsg>>().is_empty(),
            "no stale SimEventMsg survives the rebuild"
        );
        assert_eq!(
            *a.world().resource::<crate::speed::SpeedStats>(),
            crate::speed::SpeedStats::default(),
            "the speed and pause totals restart with the match"
        );
        assert!(!a.world().contains_resource::<MatchOver>());
        run_to(&mut a, 300);
        let db = dir("b");
        let mut b = lifecycle_app(
            SimHandle::vs_bot(r.clone(), 9, "standard", "rush"),
            AppState::Skirmish,
            &db,
        );
        run_to(&mut b, 300);
        let hash = |app: &App| app.world().non_send::<SimHandle>().hash();
        assert_eq!(hash(&a), hash(&b), "the rebuilt match is a fresh one");
        assert_eq!(
            a.world()
                .resource::<crate::sim_driver::DriverStats>()
                .events_seen,
            b.world()
                .resource::<crate::sim_driver::DriverStats>()
                .events_seen,
            "the counters restarted"
        );
        a.world_mut().write_message(bevy::app::AppExit::Success);
        a.update();
        let files = replays_in(&da);
        assert_eq!(files.len(), 2, "{files:?}");
        // The rebuild's frame stepped the old match once more (FixedUpdate
        // runs before Update), so its replay ends at 301; the exit frame
        // stepped the new one to 301 as well.
        let mut old_at_301 = SimHandle::vs_bot(r.clone(), 5, "standard", "rush");
        for _ in 0..301 {
            old_at_301.step_once();
        }
        assert_ne!(old_at_301.hash(), old_hash);
        let finished: Vec<(u64, u32)> = files.iter().map(|f| verify_ok(f)).collect();
        let new_hash = a.world().non_send::<SimHandle>().hash();
        let mut want = vec![(old_at_301.hash(), 301), (new_hash, 301)];
        let mut got = finished.clone();
        want.sort_unstable();
        got.sort_unstable();
        assert_eq!(got, want, "both replays verify to their matches' hashes");
        let _ = std::fs::remove_dir_all(&da);
        let _ = std::fs::remove_dir_all(&db);
    }

    /// Main Menu: the old match's replay is finished with its trailer and
    /// verifies, the menu shows a fresh unstarted match at a new seed with
    /// the same settings, and nothing steps while it is open.
    #[test]
    fn main_menu_tears_down_and_writes_a_finished_replay() {
        let r = rules();
        let d = dir("menu");
        let mut app = lifecycle_app(
            SimHandle::vs_bot(r.clone(), 5, "hard", "tower"),
            AppState::Skirmish,
            &d,
        );
        app.insert_resource(NewGameSettings {
            difficulty: "hard".into(),
            style: StyleChoice::Id("tower".into()),
            seed: 5,
            spectate: None,
            seed_typed: false,
        });
        run_to(&mut app, 120);
        let old = app.world().non_send::<SimHandle>().hash();
        app.insert_resource(PendingMatch::MainMenu);
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::Menu);
        app.update();
        assert_eq!(
            *app.world().resource::<State<AppState>>().get(),
            AppState::Menu
        );
        for _ in 0..10 {
            app.update();
        }
        let sim = app.world().non_send::<SimHandle>();
        assert_eq!(sim.tick(), 0, "the menu's match waits");
        assert_eq!(sim.setup().ai_seats[0].bot, "hard");
        assert_eq!(sim.setup().ai_seats[0].personality, "tower");
        assert_eq!(app.world().resource::<NewGameSettings>().seed, sim.seed());
        let files = replays_in(&d);
        assert_eq!(files.len(), 1, "nothing recorded in the menu");
        let file = sim::replay::ReplayReader::open(&files[0]).unwrap();
        assert!(!file.truncated, "finished with its trailer");
        assert_eq!(verify_ok(&files[0]), (old, 120));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// New Game from the menu replaces the launch match with the
    /// selector's difficulty, style and seed, and hides a Random style.
    #[test]
    fn new_game_from_menu_replaces_the_launch_match() {
        let r = rules();
        let d = dir("new");
        let mut app = lifecycle_app(
            SimHandle::vs_bot(r.clone(), 1234, "standard", "standard"),
            AppState::Menu,
            &d,
        );
        app.insert_resource(NewGameSettings {
            difficulty: "easy".into(),
            style: StyleChoice::Random,
            seed: 42,
            spectate: None,
            seed_typed: false,
        });
        for _ in 0..5 {
            app.update();
        }
        assert_eq!(app.world().non_send::<SimHandle>().tick(), 0);
        app.insert_resource(PendingMatch::NewGame);
        app.world_mut()
            .resource_mut::<NextState<AppState>>()
            .set(AppState::Skirmish);
        app.update();
        let want = NewGameSettings {
            difficulty: "easy".into(),
            style: StyleChoice::Random,
            seed: 42,
            spectate: None,
            seed_typed: false,
        }
        .setup(&r, 42)
        .unwrap();
        assert_eq!(*app.world().non_send::<SimHandle>().setup(), want);
        assert!(app.world().resource::<crate::alert::StyleHidden>().0);
        run_to(&mut app, 30);
        let _ = std::fs::remove_dir_all(&d);
    }
}
