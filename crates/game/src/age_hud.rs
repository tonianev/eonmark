//! The age readout and the Advance button (M4b), on the top bar's
//! [`TopBarRight`] row before the pop readout.
//!
//! The readout names the local player's age (`SimView::age`, the name from
//! `ages.ron`): "Hearth Age". The Advance button is shown in every age but
//! the last, greyed, not hidden, while the sim would reject `AdvanceAge`
//! (owner decision 7, ROADMAP M4b Notes; `ART_STYLE.md`: "greyed, not hidden"):
//! `SimView::check_advance_age` alone decides, the game copies none of its
//! checks. Its one line names the next age ("Advance to Masonry Age"), or
//! is the tech count while that is what blocks it ("Advance: 1 / 2
//! techs"). The cost (`Rules::amounts` of the next age in `ages.ron`) is in
//! the tooltip, which shows under the top bar: "Advance to Masonry Age:
//! Costs 250 Grain 100 Lore" when enabled, else the rejection's message,
//! "n / m techs researched" and the cost. The label stays short so the row
//! fits the 1280 px top bar beside four resource readouts with a capped
//! rate (the M3b amber readout). A click issues `AdvanceAge { player:
//! LOCAL_PLAYER }` from a `Pointer<Click>` observer, or puts the tooltip
//! on the message line when greyed. In the last age the button is hidden
//! (`Display::None`). The hover colours and the tooltip are the command
//! card's (`command_card::card_button_colors`,
//! `command_card::update_tooltip`), which follow any entity with a
//! [`CardState`].

use bevy::picking::events::{Click, Pointer};
use bevy::picking::pointer::PointerButton;
use bevy::prelude::*;
use bevy::text::FontSize;
use rules::Rules;
use rules::strings::fill;
use sim::{Command, RejectReason, SimView};

use crate::command_card::{CARD_FONT_PX, CardState, TooltipPlace, card_button_colors, cost_text};
use crate::economy_hud::{MessageState, TopBarRight, UiStrings, reject_text, spawn_economy_hud};
use crate::hud::HUD_FONT_PX;
use crate::palette;
use crate::sim_driver::{LOCAL_PLAYER, PendingCommands, SimHandle};

/// Advance button height, logical px (one line inside the 28 px top bar).
pub const ADVANCE_BUTTON_HEIGHT_PX: f32 = 24.0;
/// Advance button horizontal padding, logical px.
pub const ADVANCE_PADDING_PX: f32 = 6.0;

/// The age readout's text.
#[derive(Component, Debug, Clone, Copy)]
pub struct AgeReadout;

/// The Advance button.
#[derive(Component, Debug, Clone, Copy)]
pub struct AdvanceButton;

/// The Advance button's label.
#[derive(Component, Debug, Clone, Copy)]
pub struct AdvanceLabel;

/// What the Advance button shows now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvanceView {
    /// Shown; `false` in the last age.
    pub visible: bool,
    /// Its one line.
    pub label: String,
    /// Enabled (the sim would accept `AdvanceAge`) and the tooltip.
    pub state: CardState,
}

/// The readout's text: the local player's age, "Hearth Age".
pub fn age_text(view: &SimView<'_>, rules: &Rules, strings: &UiStrings) -> String {
    let age = &rules.ages[usize::from(view.age(LOCAL_PLAYER))];
    fill(strings.hud("age"), &[("name", &age.name)])
}

/// The Advance button now: enabled exactly when
/// `SimView::check_advance_age` accepts; labelled "Advance to Masonry
/// Age", or "Advance: 1 / 2 techs" while the tech count is what the sim
/// rejects (`NotEnoughTechs`); hidden in the last age. The tooltip is
/// "Advance to Masonry Age: Costs 250 Grain 100 Lore" when enabled, else
/// the rejection's message, "n / m techs researched" and the cost.
pub fn advance_view(view: &SimView<'_>, rules: &Rules, strings: &UiStrings) -> AdvanceView {
    let verdict = view.check_advance_age(LOCAL_PLAYER);
    let Some(next) = rules.ages.get(usize::from(view.age(LOCAL_PLAYER)) + 1) else {
        return AdvanceView {
            visible: false,
            label: String::new(),
            state: CardState {
                enabled: false,
                tooltip: reject_text(strings, rules, &RejectReason::FinalAge),
            },
        };
    };
    let counts = [
        ("count", view.researched(LOCAL_PLAYER).len().to_string()),
        ("cap", next.techs_required.to_string()),
    ];
    let counts = counts.each_ref().map(|(k, v)| (*k, v.as_str()));
    let label = match &verdict {
        Err(RejectReason::NotEnoughTechs) => fill(strings.hud("button.advance_techs"), &counts),
        _ => fill(strings.hud("button.advance"), &[("name", &next.name)]),
    };
    let costs = fill(
        strings.hud("tooltip.cost"),
        &[("cost", &cost_text(rules, &rules.amounts(&next.cost)))],
    );
    let tooltip = match &verdict {
        Ok(_) => format!("{label}: {costs}"),
        Err(reason) => format!(
            "{}. {}. {costs}",
            reject_text(strings, rules, reason),
            fill(strings.hud("tooltip.techs"), &counts)
        ),
    };
    AdvanceView {
        visible: true,
        label,
        state: CardState {
            enabled: verdict.is_ok(),
            tooltip,
        },
    }
}

/// The age readout and the Advance button.
pub struct AgeHudPlugin;

impl Plugin for AgeHudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_age_hud.after(spawn_economy_hud))
            .add_systems(Update, update_age_hud.before(card_button_colors));
    }
}

fn text(value: &str, size: f32) -> impl Bundle {
    (
        Text::new(value),
        TextFont {
            font_size: FontSize::Px(size),
            ..default()
        },
        TextColor(palette::HUD_TEXT),
    )
}

/// Puts the age readout and the Advance button at the start of the
/// [`TopBarRight`] row, before the pop readout.
pub fn spawn_age_hud(mut commands: Commands, row: Query<Entity, With<TopBarRight>>) {
    let Ok(row) = row.single() else {
        return;
    };
    let readout = commands
        .spawn((Name::new("Age"), AgeReadout, text("", HUD_FONT_PX)))
        .id();
    let button = commands
        .spawn((
            Name::new("Advance button"),
            AdvanceButton,
            CardState::default(),
            TooltipPlace::BelowTopBar,
            Button,
            Node {
                height: Val::Px(ADVANCE_BUTTON_HEIGHT_PX),
                padding: UiRect::horizontal(Val::Px(ADVANCE_PADDING_PX)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(palette::HUD_BUTTON),
        ))
        .observe(on_advance_click)
        .with_children(|button| {
            button.spawn((AdvanceLabel, text("", CARD_FONT_PX), Pickable::IGNORE));
        })
        .id();
    commands.entity(row).insert_children(0, &[readout, button]);
}

/// Every frame: the readout, and the button's label, state and visibility.
/// Writes only what changed.
pub fn update_age_hud(
    sim: NonSend<SimHandle>,
    strings: Option<Res<UiStrings>>,
    mut readout: Query<&mut Text, (With<AgeReadout>, Without<AdvanceLabel>)>,
    mut button: Query<(&mut CardState, &mut Node), With<AdvanceButton>>,
    mut label: Query<&mut Text, (With<AdvanceLabel>, Without<AgeReadout>)>,
) {
    let Some(strings) = strings else {
        return;
    };
    let view = sim.view();
    if view.player(LOCAL_PLAYER).is_none() {
        return;
    }
    let rules = sim.rules();
    let age = age_text(&view, rules, &strings);
    for mut t in &mut readout {
        if t.0 != age {
            t.0.clone_from(&age);
        }
    }
    let want = advance_view(&view, rules, &strings);
    let display = if want.visible {
        Display::Flex
    } else {
        Display::None
    };
    for (mut state, mut node) in &mut button {
        if *state != want.state {
            *state = want.state.clone();
        }
        if node.display != display {
            node.display = display;
        }
    }
    for mut t in &mut label {
        if t.0 != want.label {
            t.0.clone_from(&want.label);
        }
    }
}

/// A primary click on the Advance button: `AdvanceAge` for the local
/// player, or, when greyed, its tooltip (the rejection's message first) on
/// the message line, as a greyed card button does.
pub fn on_advance_click(
    click: On<Pointer<Click>>,
    buttons: Query<&CardState, With<AdvanceButton>>,
    mut pending: ResMut<PendingCommands>,
    mut message: ResMut<MessageState>,
    time: Res<Time>,
) {
    if click.event().event.button != PointerButton::Primary {
        return;
    }
    let Ok(state) = buttons.get(click.event().entity) else {
        return;
    };
    if !state.enabled {
        message.text.clone_from(&state.tooltip);
        message.since = time.elapsed_secs_f64();
        return;
    }
    let seq = pending.push_local(Command::AdvanceAge {
        player: LOCAL_PLAYER,
    });
    info!("Advance: AdvanceAge seq {seq}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::economy_hud::PopReadout;
    use crate::hud::spawn_hud;

    fn data_dir() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data")
    }

    fn strings() -> UiStrings {
        UiStrings(rules::strings::Strings::load(data_dir(), "en").expect("strings load"))
    }

    /// A two-player match from the standard start (150 Grain, 150
    /// Lumber, no Ore or Lore).
    fn standard() -> SimHandle {
        let rules = Rules::load(data_dir()).expect("rules load");
        let mut setup = sim::MatchSetup::scenario(&rules, 1);
        setup.start = sim::StartKind::Standard;
        SimHandle::from_setup(setup, rules)
    }

    /// The HUD with the economy bar and the age readout, on a standard
    /// match, after Startup and one frame.
    fn hud_app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_message::<crate::sim_driver::SimEventMsg>()
            .insert_resource(strings())
            .init_resource::<PendingCommands>()
            .add_systems(Startup, spawn_hud)
            .add_plugins((crate::economy_hud::EconomyHudPlugin, AgeHudPlugin));
        app.world_mut().insert_non_send(standard());
        app.update();
        app
    }

    /// The next seq [`run`] stamps (the click's commands go through
    /// `PendingCommands` instead).
    #[derive(Resource, Default)]
    struct NextSeq(u32);

    /// P0 commands with their own seqs, stepped until they have applied;
    /// none may be rejected.
    fn run(app: &mut App, cmds: Vec<Command>) {
        let world = app.world_mut();
        let mut next = world.get_resource_or_insert_with(NextSeq::default);
        let first = 1000 + next.0;
        next.0 += u32::try_from(cmds.len()).unwrap();
        let mut sim = world.non_send_mut::<SimHandle>();
        let batch: Vec<_> = (first..)
            .zip(cmds)
            .map(|(seq, c)| sim::PlayerCommand::new(LOCAL_PLAYER, seq, c))
            .collect();
        sim.step(&batch);
        for _ in 0..sim.setup().cmd_delay {
            sim.step(&[]);
        }
        let events = sim.drain_events();
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, sim::SimEvent::CommandRejected { .. })),
            "{events:?}"
        );
        app.update();
    }

    fn tech(app: &App, id: &str) -> sim::TechId {
        let sim = app.world().non_send::<SimHandle>();
        sim::TechId(sim.rules().tech_by_id(id).unwrap().0)
    }

    fn grant(app: &App, id: &str, amount: u32) -> Command {
        let sim = app.world().non_send::<SimHandle>();
        Command::DebugGrant {
            owner: LOCAL_PLAYER,
            resource: u8::try_from(sim.rules().resource_index(id).unwrap()).unwrap(),
            amount,
        }
    }

    fn research(app: &App, id: &str) -> Command {
        Command::DebugResearch {
            owner: LOCAL_PLAYER,
            tech: tech(app, id),
        }
    }

    /// What the HUD shows: the readout, the label, the button's state and
    /// whether it is laid out.
    fn shown(app: &mut App) -> (String, String, CardState, bool) {
        let world = app.world_mut();
        let readout = world
            .query_filtered::<&Text, With<AgeReadout>>()
            .single(world)
            .unwrap()
            .0
            .clone();
        let label = world
            .query_filtered::<&Text, With<AdvanceLabel>>()
            .single(world)
            .unwrap()
            .0
            .clone();
        let (state, node) = world
            .query_filtered::<(&CardState, &Node), With<AdvanceButton>>()
            .single(world)
            .unwrap();
        (readout, label, state.clone(), node.display != Display::None)
    }

    /// The pure view, the sim's verdict and the HUD agree.
    fn check(app: &mut App) -> AdvanceView {
        let shown = shown(app);
        let sim = app.world().non_send::<SimHandle>();
        let view = sim.view();
        let want = advance_view(&view, sim.rules(), &strings());
        assert_eq!(
            want.state.enabled,
            view.check_advance_age(LOCAL_PLAYER).is_ok(),
            "{want:?}"
        );
        assert_eq!(
            !want.visible,
            view.check_advance_age(LOCAL_PLAYER) == Err(RejectReason::FinalAge)
        );
        assert_eq!(
            shown,
            (
                age_text(&view, sim.rules(), &strings()),
                want.label.clone(),
                want.state.clone(),
                want.visible
            )
        );
        want
    }

    /// A primary click on `button`, as the picking pipeline sends it.
    fn click(app: &mut App, button: Entity) {
        app.world_mut().trigger(Pointer::new(
            bevy::picking::pointer::PointerId::Mouse,
            bevy::picking::pointer::Location {
                target: bevy::camera::NormalizedRenderTarget::None {
                    width: 1,
                    height: 1,
                },
                position: Vec2::ZERO,
            },
            Click {
                button: PointerButton::Primary,
                hit: bevy::picking::backend::HitData::new(Entity::PLACEHOLDER, 0.0, None, None),
                duration: std::time::Duration::ZERO,
                count: 1,
            },
            button,
        ));
    }

    /// The readout and the Advance button sit in the top bar's right row,
    /// before the pop readout.
    #[test]
    fn the_age_readout_and_button_precede_pop_on_the_top_bar() {
        let mut app = hud_app();
        let world = app.world_mut();
        let row = world
            .query_filtered::<&Children, With<TopBarRight>>()
            .single(world)
            .unwrap()
            .to_vec();
        assert_eq!(row.len(), 3);
        assert!(world.get::<AgeReadout>(row[0]).is_some());
        assert!(world.get::<AdvanceButton>(row[1]).is_some());
        assert!(world.get::<PopReadout>(row[2]).is_some());
        assert_eq!(
            world.get::<TooltipPlace>(row[1]),
            Some(&TooltipPlace::BelowTopBar)
        );
    }

    /// Greyed with the tech count at the start, greyed with the cost when
    /// only the stockpile is short, enabled with the cost once the sim
    /// accepts `AdvanceAge`, and greyed again for the Charter Age after a
    /// click advanced; each state is `check_advance_age`'s. A greyed click
    /// sends nothing and shows the reason; an enabled one sends
    /// `AdvanceAge`.
    #[test]
    fn advance_button_follows_check_advance_age() {
        let mut app = hud_app();
        let button = app
            .world_mut()
            .query_filtered::<Entity, With<AdvanceButton>>()
            .single(app.world())
            .unwrap();
        let want = check(&mut app);
        assert_eq!(shown(&mut app).0.as_str(), "Hearth Age", "the start's age");
        assert_eq!(
            (want.label.as_str(), want.state.enabled),
            ("Advance: 0 / 2 techs", false)
        );
        assert_eq!(
            want.state.tooltip,
            "Research more techs first. 0 / 2 techs researched. Costs 250 Grain 100 Lore"
        );
        click(&mut app, button);
        assert!(app.world().resource::<PendingCommands>().is_empty());
        assert_eq!(
            app.world().resource::<MessageState>().text,
            want.state.tooltip
        );

        // Two techs, but 150 Grain of 250.
        let cmds = vec![research(&app, "letters_1"), research(&app, "trade_1")];
        run(&mut app, cmds);
        let want = check(&mut app);
        assert_eq!(
            (want.label.as_str(), want.state),
            (
                "Advance to Masonry Age",
                CardState {
                    enabled: false,
                    tooltip: "Not enough Grain. 2 / 2 techs researched. Costs 250 Grain 100 Lore"
                        .into(),
                }
            )
        );

        let cmds = vec![grant(&app, "grain", 100), grant(&app, "lore", 100)];
        run(&mut app, cmds);
        let want = check(&mut app);
        assert_eq!(
            (want.label.as_str(), want.state),
            (
                "Advance to Masonry Age",
                CardState {
                    enabled: true,
                    tooltip: "Advance to Masonry Age: Costs 250 Grain 100 Lore".into(),
                }
            )
        );
        click(&mut app, button);
        let sent = app
            .world_mut()
            .resource_mut::<PendingCommands>()
            .drain_sorted();
        assert_eq!(
            sent.iter().map(|c| &c.cmd).collect::<Vec<_>>(),
            [&Command::AdvanceAge {
                player: LOCAL_PLAYER
            }]
        );
        let cmds: Vec<Command> = sent.into_iter().map(|c| c.cmd).collect();
        run(&mut app, cmds);
        let want = check(&mut app);
        assert_eq!(shown(&mut app).0.as_str(), "Masonry Age");
        assert_eq!(
            (want.label.as_str(), want.state.enabled),
            ("Advance: 2 / 5 techs", false)
        );
        assert_eq!(
            want.state.tooltip,
            "Research more techs first. 2 / 5 techs researched. \
             Costs 500 Grain 300 Lumber 300 Lore"
        );
    }

    /// In the last age the readout names it and the button is hidden, as
    /// the sim answers `FinalAge`.
    #[test]
    fn the_button_is_hidden_in_the_final_age() {
        let mut app = hud_app();
        let mut cmds: Vec<Command> = [
            "letters_1",
            "trade_1",
            "arms_1",
            "statecraft_1",
            "letters_2",
        ]
        .iter()
        .map(|id| research(&app, id))
        .collect();
        cmds.extend([
            grant(&app, "grain", 750),
            grant(&app, "lumber", 300),
            grant(&app, "lore", 400),
            Command::AdvanceAge {
                player: LOCAL_PLAYER,
            },
            Command::AdvanceAge {
                player: LOCAL_PLAYER,
            },
        ]);
        run(&mut app, cmds);
        let want = check(&mut app);
        let (readout, _, _, laid_out) = shown(&mut app);
        assert_eq!(readout, "Charter Age");
        assert!(!want.visible && !laid_out, "{want:?}");
        assert_eq!(
            app.world()
                .non_send::<SimHandle>()
                .view()
                .check_advance_age(LOCAL_PLAYER),
            Err(RejectReason::FinalAge)
        );
    }
}
