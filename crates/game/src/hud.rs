//! HUD skeleton (M2 decision 7): a top bar and a bottom bar in `bevy_ui`,
//! one placeholder Stop button, and the `PointerOverUi` resource that keeps
//! world clicks from falling through the HUD. Layout-only root nodes get
//! `Pickable::IGNORE`; panels keep the `bevy_ui` default (block lower), so a
//! click on a panel never reaches the ground or a unit.
//!
//! `PointerOverUi` is derived in `PreUpdate` after the picking hover map is
//! built: it is `true` when any entity hovered by the mouse pointer has a
//! `Node`. `orders.rs` and `selection.rs` read it in `Update`.
//!
//! `--scenario hud_click` turns on [`HudClickCheck`]: an automated check
//! that selects the 20 spawned units, writes synthetic
//! `bevy_picking::pointer::PointerInput` messages (move, press, release)
//! over the bottom panel and then over the Stop button, and asserts that no
//! Move was queued and the selection is unchanged. It prints
//! `hud_click: ok` and exits 0, or `hud_click: FAIL <why>` and exits 1.
//!
//! Ownership (M2 contract): agent B owns this file. `PointerOverUi`,
//! the plugin wiring and the layout constants are final; `spawn_hud`, the
//! Stop button and the `hud_click` state machine have signatures and
//! descriptions, bodies are B's.
// M2-B: remove this allow once spawn_hud, stop_button and run_hud_click_check
// use the layout constants, markers and the check state.
#![allow(dead_code)]

use std::collections::BTreeSet;

use bevy::app::AppExit;
use bevy::picking::PickingSystems;
use bevy::picking::hover::HoverMap;
use bevy::picking::pointer::PointerId;
use bevy::prelude::*;
use sim::UnitId;

use crate::selection::Selection;
use crate::sim_driver::{PendingCommands, SimHandle};

/// Bottom bar height, logical px (authored in `Val::Px`: `UiScale` multiplies Px only).
pub const BOTTOM_BAR_HEIGHT_PX: f32 = 96.0;
/// Top bar height, logical px.
pub const TOP_BAR_HEIGHT_PX: f32 = 28.0;
/// Stop button size, logical px.
pub const BUTTON_SIZE_PX: Vec2 = Vec2::new(72.0, 28.0);
/// HUD padding, logical px.
pub const PADDING_PX: f32 = 8.0;

/// `true` while the mouse pointer is over any `bevy_ui` node. World input
/// (selection, orders, drag box) is ignored while set.
#[derive(Resource, Default, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerOverUi(pub bool);

/// The full-window layout root (`Pickable::IGNORE`).
#[derive(Component, Debug, Clone, Copy)]
pub struct HudRoot;
/// The bottom panel (blocks picking).
#[derive(Component, Debug, Clone, Copy)]
pub struct BottomBar;
/// The top panel (blocks picking).
#[derive(Component, Debug, Clone, Copy)]
pub struct TopBar;
/// The placeholder Stop button on the bottom bar.
#[derive(Component, Debug, Clone, Copy)]
pub struct StopButton;

/// Stages of the automated `hud_click` check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudClickStage {
    /// Wait until the sim has spawned the 20 units and the HUD has laid out.
    WaitForUnits,
    /// Select every unit (write `Selection` directly) and record the
    /// selection and the pending-command count.
    SelectAll,
    /// Synthetic move + press over the bottom bar's empty area.
    PressPanel,
    /// Release over the bottom bar; a frame later assert nothing changed.
    ReleasePanel,
    /// Synthetic move + press over the Stop button.
    PressButton,
    /// Release over the Stop button; a frame later assert the selection is
    /// unchanged and the only queued command (if any) is a Stop.
    ReleaseButton,
    /// Print the verdict and write `AppExit`.
    Verdict,
    /// Finished (the exit message is in flight).
    Done,
}

/// State of the automated `hud_click` check (inserted only under
/// `--scenario hud_click`).
#[derive(Resource, Debug, Clone)]
pub struct HudClickCheck {
    /// Current stage.
    pub stage: HudClickStage,
    /// Frames spent in the current stage (settling time between stages).
    pub frames_in_stage: u32,
    /// Selection captured in `SelectAll`.
    pub selection_before: BTreeSet<UnitId>,
    /// Moves seen in `PendingCommands` (counted before each tick drains it).
    pub moves_seen: u32,
    /// Stops seen in `PendingCommands`.
    pub stops_seen: u32,
    /// Failure reason, if any.
    pub failure: Option<String>,
}

impl Default for HudClickCheck {
    fn default() -> Self {
        Self {
            stage: HudClickStage::WaitForUnits,
            frames_in_stage: 0,
            selection_before: BTreeSet::new(),
            moves_seen: 0,
            stops_seen: 0,
            failure: None,
        }
    }
}

/// The HUD skeleton and, optionally, the automated `hud_click` check.
pub struct HudPlugin {
    /// Run the `hud_click` automated check and exit with its verdict.
    pub hud_click_check: bool,
}

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PointerOverUi>()
            .add_systems(Startup, spawn_hud)
            .add_systems(
                PreUpdate,
                update_pointer_over_ui.after(PickingSystems::Hover),
            )
            .add_systems(Update, stop_button);
        if self.hud_click_check {
            app.init_resource::<HudClickCheck>()
                .add_systems(Update, run_hud_click_check.after(stop_button));
        }
    }
}

/// Spawns the layout: a full-window root `Node` with `Pickable::IGNORE`
/// (`HudRoot`), an absolute-positioned bottom bar (`BottomBar`,
/// `BackgroundColor(palette::HUD_PANEL)`, height [`BOTTOM_BAR_HEIGHT_PX`])
/// holding one `Button` (`StopButton`, [`BUTTON_SIZE_PX`], text "Stop"),
/// and a top bar (`TopBar`, height [`TOP_BAR_HEIGHT_PX`]) with a
/// placeholder label. Panels and the button keep `Pickable::default()`.
///
/// M2-B: body to implement.
pub fn spawn_hud(_commands: Commands) {
    // M2-B: see the doc comment.
}

/// `PointerOverUi = any hovered entity (mouse pointer) has a Node`.
pub fn update_pointer_over_ui(
    hover: Res<HoverMap>,
    nodes: Query<(), With<Node>>,
    mut over: ResMut<PointerOverUi>,
) {
    let hovering_ui = hover
        .get(&PointerId::Mouse)
        .is_some_and(|hits| hits.keys().any(|e| nodes.contains(*e)));
    if over.0 != hovering_ui {
        over.0 = hovering_ui;
    }
}

/// `Interaction::Pressed` on the Stop button issues `orders::issue_stop`
/// for the selection. Hover/pressed colours come from `palette`.
///
/// M2-B: body to implement.
pub fn stop_button(
    _buttons: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<StopButton>)>,
    _selection: Res<Selection>,
    _pending: ResMut<PendingCommands>,
) {
    // M2-B: see the doc comment.
}

/// The `hud_click` state machine (see the module docs and [`HudClickStage`]).
/// Synthetic input: `PointerInput::new(PointerId::Mouse, Location { target:
/// NormalizedRenderTarget::Window(primary.normalize(..)), position },
/// PointerAction::Move { delta } / Press(Primary) / Release(Primary))`,
/// positions taken from the panels' `ComputedNode` + `UiGlobalTransform`
/// (or the layout constants). Give each stage at least two frames so the
/// hover map and `Interaction` update between press and release.
/// Verdict: `moves_seen == 0 && selection == selection_before`, and after
/// the button stage `stops_seen >= 1`.
///
/// M2-B: body to implement.
#[allow(clippy::too_many_arguments)] // Bevy system: each parameter is one resource or query
pub fn run_hud_click_check(
    _check: ResMut<HudClickCheck>,
    _sim: NonSend<SimHandle>,
    _selection: ResMut<Selection>,
    _pending: Res<PendingCommands>,
    _pointer_inputs: MessageWriter<bevy::picking::pointer::PointerInput>,
    _primary: Query<Entity, With<bevy::window::PrimaryWindow>>,
    _bars: Query<(&ComputedNode, &UiGlobalTransform), With<BottomBar>>,
    _button: Query<(&ComputedNode, &UiGlobalTransform), With<StopButton>>,
    _exit: MessageWriter<AppExit>,
) {
    // M2-B: see the doc comment. On the verdict:
    //   println!("hud_click: ok"); exit.write(AppExit::Success)
    //   println!("hud_click: FAIL {why}"); exit.write(AppExit::error())
}

/// Format the verdict line exactly as the scripts grep for it.
pub fn verdict_line(failure: Option<&str>) -> String {
    match failure {
        None => "hud_click: ok".to_string(),
        Some(why) => format!("hud_click: FAIL {why}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_lines_are_exact() {
        assert_eq!(verdict_line(None), "hud_click: ok");
        assert_eq!(
            verdict_line(Some("a Move was queued")),
            "hud_click: FAIL a Move was queued"
        );
    }
}
