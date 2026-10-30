//! The under-attack alert and the top bar's match readouts (M6 slice 3).
//!
//! **Alert** (game-design note 8: raids on Yeomen went unreported until
//! "Lumber Yard lost"): a `SimEvent::Hit` whose target is the local
//! player's and whose attacker is another player's shows "Yeomen under
//! attack" (a civilian unit), "Army under attack" (any other unit) or
//! "`<Building>` under attack" on the message line, at most once per
//! [`ALERT_THROTTLE_SECS`] of `speed::UiClock` per [`ALERT_AREA_TILES`]
//! square area, with a [`PING_SECS`] amber ping on the minimap at the hit.
//! Backspace centres the camera on the last alert.
//!
//! **Readouts** in the top bar: "vs Standard · ?" (the opponent's
//! difficulty; a Random style stays "?" until the game-over screen reveals
//! it, owner decision 6), and from 35:00 the tiebreak race, "Tiebreak
//! 45:00 · You 51% · Bot 49%" (territory shares of the two players,
//! sampled once a second from `SimView::territory`).

use std::collections::BTreeMap;

use bevy::prelude::*;
use rules::strings::fill;
use sim::{PlayerId, SimEvent, SimView, TargetId};

use crate::camera::RtsCamera;
use crate::economy_hud::{MessageState, UiStrings};
use crate::palette;
use crate::sim_driver::{LOCAL_PLAYER, MatchOver, SimEventMsg, SimHandle, match_running};
use crate::speed::UiNow;

/// The side of an alert area, tiles.
pub const ALERT_AREA_TILES: u16 = 16;
/// One alert per area per this many seconds of `UiClock`.
pub const ALERT_THROTTLE_SECS: f64 = 20.0;
/// How long a minimap ping shows, seconds of `UiClock`.
pub const PING_SECS: f64 = 3.0;
/// The ping's side on the minimap, logical px.
pub const PING_PX: f32 = 10.0;
/// From this match time on the top bar shows the tiebreak race (35:00).
pub const TIEBREAK_READOUT_FROM_SECS: u32 = 35 * 60;

/// The alerts raised so far: when each area last raised one, and where the
/// last one was (Backspace).
#[derive(Resource, Debug, Default, Clone, PartialEq)]
pub struct Alerts {
    /// `UiClock` seconds of the last alert per area `(x, y) /
    /// ALERT_AREA_TILES`.
    pub last_by_area: BTreeMap<(u16, u16), f64>,
    /// Sim tile coordinates of the last alert.
    pub last_at: Option<Vec2>,
}

/// A minimap ping; despawned at `until` (`UiClock` seconds).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct AlertPing {
    /// When it goes.
    pub until: f64,
}

/// The top bar's opponent label.
#[derive(Component, Debug, Clone, Copy)]
pub struct OpponentLabel;

/// The top bar's tiebreak race readout.
#[derive(Component, Debug, Clone, Copy)]
pub struct TiebreakReadout;

/// What a hit on the local player is, for the alert text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertKind {
    /// A civilian unit (Yeoman, Scribe).
    Civilian,
    /// Any other unit.
    Army,
    /// A building, by display name.
    Building(String),
}

/// The alert for `event` as `local` sees it: what was hit and where (sim
/// tile coordinates), or `None` when it is not an enemy's hit on `local`.
pub fn alert_for(
    view: &SimView<'_>,
    event: &SimEvent,
    local: PlayerId,
) -> Option<(AlertKind, Vec2)> {
    let SimEvent::Hit {
        attacker, target, ..
    } = event
    else {
        return None;
    };
    let owner = |t: &TargetId| match t {
        TargetId::Unit(u) => view.unit(*u).map(|u| u.owner),
        TargetId::Building(b) => view.building(*b).map(|b| b.owner),
    };
    if owner(target)? != local || owner(attacker)? == local {
        return None;
    }
    let rules = view.rules();
    let fx = crate::present::fx_to_f32;
    match target {
        TargetId::Unit(id) => {
            let u = view.unit(*id)?;
            let civilian = rules
                .unit_kind(u.kind.0)
                .is_some_and(|k| k.class == rules::units::UnitClass::Civilian);
            let kind = if civilian {
                AlertKind::Civilian
            } else {
                AlertKind::Army
            };
            Some((kind, Vec2::new(fx(u.pos.x), fx(u.pos.y))))
        }
        TargetId::Building(id) => {
            let b = view.building(*id)?;
            let kind = rules.building_kind(b.kind.0);
            let name = kind.map_or_else(String::new, |k| k.name.clone());
            let (w, h) = kind.map_or((1, 1), |k| (k.width_tiles, k.height_tiles));
            // The footprint's centre (`tile` is its north-west corner).
            let c = Vec2::new(
                f32::from(b.tile.x) + w as f32 / 2.0,
                f32::from(b.tile.y) + h as f32 / 2.0,
            );
            Some((AlertKind::Building(name), c))
        }
    }
}

/// The message-line text of an alert.
pub fn alert_text(kind: &AlertKind, strings: &UiStrings) -> String {
    match kind {
        AlertKind::Civilian => strings.hud("alert.civilian").to_string(),
        AlertKind::Army => strings.hud("alert.army").to_string(),
        AlertKind::Building(name) => fill(strings.hud("alert.building"), &[("building", name)]),
    }
}

/// The area key of a sim position.
pub fn area_of(at: Vec2) -> (u16, u16) {
    let cell = |v: f32| (v.max(0.0) as u16) / ALERT_AREA_TILES;
    (cell(at.x), cell(at.y))
}

impl Alerts {
    /// Whether an alert at `at` may show now, recording it if so.
    pub fn admit(&mut self, at: Vec2, now: f64) -> bool {
        let key = area_of(at);
        if self
            .last_by_area
            .get(&key)
            .is_some_and(|t| now - t < ALERT_THROTTLE_SECS)
        {
            return false;
        }
        self.last_by_area.insert(key, now);
        self.last_at = Some(at);
        true
    }
}

/// The alert, the ping, Backspace and the top bar readouts (windowed app).
pub struct AlertPlugin;

impl Plugin for AlertPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Alerts>()
            .add_systems(
                Startup,
                spawn_readouts.after(crate::economy_hud::spawn_economy_hud),
            )
            .add_systems(
                Update,
                (
                    raise_alerts.after(crate::economy_hud::collect_messages),
                    expire_pings,
                    jump_to_alert
                        .run_if(match_running)
                        .run_if(not(resource_exists::<MatchOver>)),
                    update_readouts,
                ),
            );
    }
}

/// Raise the alert of every enemy hit on the local player this frame
/// (throttled per area), ping the minimap there.
#[allow(clippy::too_many_arguments)] // Bevy system: each parameter is one resource or query
pub fn raise_alerts(
    mut events: MessageReader<SimEventMsg>,
    sim: NonSend<SimHandle>,
    strings: Option<Res<UiStrings>>,
    now: UiNow,
    mut alerts: ResMut<Alerts>,
    mut message: ResMut<MessageState>,
    minimap: Query<Entity, With<crate::minimap::Minimap>>,
    mut commands: Commands,
) {
    let Some(strings) = strings else {
        events.clear();
        return;
    };
    let view = sim.view();
    let now = now.secs();
    for msg in events.read() {
        let Some((kind, at)) = alert_for(&view, &msg.event, LOCAL_PLAYER) else {
            continue;
        };
        if !alerts.admit(at, now) {
            continue;
        }
        message.set(alert_text(&kind, &strings), now);
        let (w, h) = (
            f32::from(view.map().width()).max(1.0),
            f32::from(view.map().height()).max(1.0),
        );
        if let Ok(map) = minimap.single() {
            let px = crate::minimap::MINIMAP_PX;
            commands.entity(map).with_children(|m| {
                m.spawn((
                    Name::new("Alert ping"),
                    AlertPing {
                        until: now + PING_SECS,
                    },
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(at.x / w * px - PING_PX / 2.0),
                        top: Val::Px(at.y / h * px - PING_PX / 2.0),
                        width: Val::Px(PING_PX),
                        height: Val::Px(PING_PX),
                        border: UiRect::all(Val::Px(2.0)),
                        ..default()
                    },
                    BorderColor::all(palette::HUD_AMBER),
                    Pickable::IGNORE,
                ));
            });
        }
    }
}

fn expire_pings(now: UiNow, pings: Query<(Entity, &AlertPing)>, mut commands: Commands) {
    let now = now.secs();
    for (e, ping) in &pings {
        if now >= ping.until {
            commands.entity(e).despawn();
        }
    }
}

/// Backspace: centre the camera on the last alert.
pub fn jump_to_alert(
    keys: Res<ButtonInput<KeyCode>>,
    alerts: Res<Alerts>,
    mut cameras: Query<&mut RtsCamera>,
) {
    if !keys.just_pressed(KeyCode::Backspace) {
        return;
    }
    let Some(at) = alerts.last_at else {
        return;
    };
    let focus = Vec3::new(
        at.x - crate::ground::HALF_EXTENT,
        0.0,
        at.y - crate::ground::HALF_EXTENT,
    );
    for mut cam in &mut cameras {
        cam.focus = focus;
    }
}

/// The top bar's "vs `<Difficulty>` · `<style>`" for `setup` as `local` sees it
/// before the outcome: a Random style is "?" (owner decision 6), which the
/// game-over screen reveals. Spectating: "Spectating `<a>` vs `<b>`".
pub fn opponent_label(
    setup: &sim::MatchSetup,
    rules: &rules::Rules,
    local: PlayerId,
    hidden: bool,
) -> String {
    let name = |bot: &str| {
        rules
            .ai
            .difficulty(bot)
            .map_or_else(|| title_case(bot), |d| d.name.clone())
    };
    let local_is_ai = setup.players.iter().any(|p| p.id == local && p.is_ai);
    if local_is_ai {
        let seats: Vec<String> = setup
            .ai_seats
            .iter()
            .map(|s| {
                format!(
                    "{} · {}",
                    name(&s.bot),
                    crate::menu::new_game::style_label(rules, &s.personality)
                )
            })
            .collect();
        return format!("Spectating {}", seats.join(" vs "));
    }
    let Some(seat) = setup.ai_seats.iter().find(|s| s.player != local) else {
        return String::new();
    };
    if seat.personality.is_empty() {
        return format!("vs {}", name(&seat.bot));
    }
    let style = if hidden {
        "?".to_string()
    } else {
        crate::menu::new_game::style_label(rules, &seat.personality)
    };
    format!("vs {} · {style}", name(&seat.bot))
}

/// `rush` -> `Rush`.
pub fn title_case(id: &str) -> String {
    let mut c = id.chars();
    c.next().map_or_else(String::new, |f| {
        f.to_uppercase().collect::<String>() + c.as_str()
    })
}

/// Whether the opponent's style is hidden in this match: New Game's Random
/// (`menu::new_game::StyleHidden`) until the outcome.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StyleHidden(pub bool);

/// The tiebreak readout for `view` from 35:00, else `None`: "Tiebreak
/// 45:00 · You 51% · Bot 49%" (the two players' shares of the land they
/// own between them).
pub fn tiebreak_text(view: &SimView<'_>, strings: &UiStrings, local: PlayerId) -> Option<String> {
    let rate = view.rules().tick_rate_hz.max(1);
    if view.tick / rate < TIEBREAK_READOUT_FROM_SECS {
        return None;
    }
    let mine = view.territory().owned_tile_count(local);
    let theirs = view
        .players()
        .iter()
        .filter(|p| p.id != local)
        .map(|p| view.territory().owned_tile_count(p.id))
        .max()
        .unwrap_or(0);
    let total = (mine + theirs).max(1);
    let pct = |n: u32| (u64::from(n) * 100 / u64::from(total)).to_string();
    let cap = view.rules().match_cap_ticks() / rate;
    Some(fill(
        strings.hud("top.tiebreak"),
        &[
            ("time", &crate::markers::clock(cap)),
            ("count", &pct(mine)),
            ("cap", &pct(theirs)),
        ],
    ))
}

fn hud_text(size: f32) -> impl Bundle {
    (
        Text::new(""),
        crate::fonts::hud_font(size),
        TextColor(palette::HUD_TEXT),
        TextLayout::no_wrap(),
    )
}

fn spawn_readouts(mut commands: Commands, top: Query<Entity, With<crate::hud::TopBar>>) {
    let Ok(bar) = top.single() else {
        return;
    };
    commands.entity(bar).with_children(|bar| {
        bar.spawn((Name::new("Opponent"), OpponentLabel, hud_text(16.0)));
        bar.spawn((Name::new("Tiebreak race"), TiebreakReadout, hud_text(16.0)));
    });
}

fn update_readouts(
    sim: NonSend<SimHandle>,
    strings: Option<Res<UiStrings>>,
    hidden: Option<Res<StyleHidden>>,
    over: Option<Res<MatchOver>>,
    mut labels: Query<&mut Text, (With<OpponentLabel>, Without<TiebreakReadout>)>,
    mut race: Query<&mut Text, (With<TiebreakReadout>, Without<OpponentLabel>)>,
    mut last_second: Local<Option<u32>>,
) {
    let Some(strings) = strings else {
        return;
    };
    let hide = hidden.is_some_and(|h| h.0) && over.is_none();
    let want = opponent_label(sim.setup(), sim.rules(), LOCAL_PLAYER, hide);
    for mut t in &mut labels {
        if t.0 != want {
            t.0.clone_from(&want);
        }
    }
    let view = sim.view();
    let second = view.tick / view.rules().tick_rate_hz.max(1);
    if *last_second == Some(second) {
        return;
    }
    *last_second = Some(second);
    let text = tiebreak_text(&view, &strings, LOCAL_PLAYER).unwrap_or_default();
    for mut t in &mut race {
        if t.0 != text {
            t.0.clone_from(&text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn data() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    fn strings() -> UiStrings {
        UiStrings(rules::strings::Strings::load(data(), "en").unwrap())
    }

    /// An enemy Bowman shooting P0's Yeomen raises "Yeomen under attack"
    /// with a ping, once per area per 20 s; a hit on a building names it;
    /// P0's own hits and P1's losses raise nothing.
    #[test]
    fn under_attack_notice_is_throttled_per_area_and_names_the_target() {
        let r = rules::Rules::load(data()).unwrap();
        let mut setup = sim::MatchSetup::scenario(&r, 1);
        setup.start = sim::StartKind::Standard;
        let sim = SimHandle::from_setup(setup, r);
        let view = sim.view();
        let s = strings();
        let yeoman = TargetId::Unit(sim::UnitId(1));
        let enemy = TargetId::Unit(sim::UnitId(6));
        let seat = TargetId::Building(sim::BuildingId(1));
        let hit = |attacker, target| SimEvent::Hit {
            attacker,
            target,
            damage: 3,
        };
        let (kind, at) = alert_for(&view, &hit(enemy, yeoman), LOCAL_PLAYER).unwrap();
        assert_eq!(kind, AlertKind::Civilian);
        assert_eq!(alert_text(&kind, &s), "Yeomen under attack");
        let (kind, seat_at) = alert_for(&view, &hit(enemy, seat), LOCAL_PLAYER).unwrap();
        assert_eq!(alert_text(&kind, &s), "Town under attack");
        assert_eq!(alert_text(&AlertKind::Army, &s), "Army under attack");
        assert_eq!(alert_for(&view, &hit(yeoman, enemy), LOCAL_PLAYER), None);
        assert_eq!(
            alert_for(&view, &hit(yeoman, seat), LOCAL_PLAYER),
            None,
            "own fire"
        );
        assert_eq!(seat_at, Vec2::new(24.5, 64.5), "the Seat's centre");
        assert_eq!(area_of(seat_at), (1, 4));
        let mut alerts = Alerts::default();
        assert!(alerts.admit(at, 0.0));
        assert!(!alerts.admit(at, 5.0), "same area within 20 s");
        let (a, b) = (Vec2::new(17.0, 65.0), Vec2::new(31.0, 79.0));
        assert_eq!(area_of(a), area_of(b));
        assert!(alerts.admit(a, 30.0));
        assert!(!alerts.admit(b, 49.9), "same 16-tile area");
        assert!(
            alerts.admit(Vec2::new(33.0, 79.0), 40.0),
            "the next area east"
        );
        assert!(alerts.admit(b, 50.0), "20 s later");
        assert_eq!(alerts.last_at, Some(b));
    }

    /// The opponent label hides a Random style until the outcome and names
    /// the difficulty; spectating names both seats.
    #[test]
    fn opponent_label_hides_the_style_until_revealed() {
        let r = rules::Rules::load(data()).unwrap();
        let setup = sim::MatchSetup::vs_bot(&r, 1, "hard", "rush");
        assert_eq!(
            opponent_label(&setup, &r, LOCAL_PLAYER, true),
            "vs Hard · ?"
        );
        assert_eq!(
            opponent_label(&setup, &r, LOCAL_PLAYER, false),
            "vs Hard · Rush"
        );
        let both = sim::MatchSetup::bots(&r, 1, ("easy", "boom"), ("standard", "tower"));
        assert_eq!(
            opponent_label(&both, &r, LOCAL_PLAYER, true),
            "Spectating Easy · Boom vs Standard · Tower"
        );
    }

    /// The tiebreak race shows from 35:00 only, the local share first.
    #[test]
    fn tiebreak_readout_from_35_minutes_with_shares() {
        let r = rules::Rules::load(data()).unwrap();
        let mut setup = sim::MatchSetup::scenario(&r, 1);
        setup.start = sim::StartKind::Standard;
        let mut sim = SimHandle::from_setup(setup, r);
        let s = strings();
        assert_eq!(tiebreak_text(&sim.view(), &s, LOCAL_PLAYER), None);
        let target = TIEBREAK_READOUT_FROM_SECS * sim.tick_rate_hz();
        while sim.tick() < target {
            sim.step_once();
            let _ = sim.drain_events();
        }
        assert_eq!(
            tiebreak_text(&sim.view(), &s, LOCAL_PLAYER).as_deref(),
            Some("Tiebreak 45:00 · You 50% · Bot 50%")
        );
    }
}
