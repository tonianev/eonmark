//! Developer-only overlays, compiled only with `--features dev`:
//! Bevy's FPS overlay, the egui world inspector and a small sim panel.

use std::time::Duration;

use bevy::dev_tools::fps_overlay::{FpsOverlayConfig, FpsOverlayPlugin, FrameTimeGraphConfig};
use bevy::prelude::*;
use bevy::text::FontSize;
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use bevy_inspector_egui::quick::WorldInspectorPlugin;

use crate::camera::RtsCamera;
use crate::present::UnitVisuals;
use crate::selection::Selection;
use crate::sim_driver::{DriverStats, PendingCommands, SimHandle};

/// Adds every dev overlay. Never shipped.
pub struct DevToolsPlugin;

impl Plugin for DevToolsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            FpsOverlayPlugin {
                config: FpsOverlayConfig {
                    text_config: TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    },
                    text_color: Color::srgb(0.96, 0.96, 0.92),
                    enabled: true,
                    refresh_interval: Duration::from_millis(250),
                    frame_time_graph_config: FrameTimeGraphConfig {
                        enabled: false,
                        ..default()
                    },
                },
            },
            // The inspector asserts that EguiPlugin was added before it.
            EguiPlugin::default(),
            WorldInspectorPlugin::new(),
        ))
        .register_type::<RtsCamera>()
        .add_systems(EguiPrimaryContextPass, sim_panel);
    }
}

#[allow(clippy::too_many_arguments)] // one egui panel reads many resources
fn sim_panel(
    mut contexts: EguiContexts,
    sim: NonSend<SimHandle>,
    fixed: Res<Time<Fixed>>,
    stats: Res<DriverStats>,
    pending: Res<PendingCommands>,
    selection: Res<Selection>,
    visuals: Res<UnitVisuals>,
    cameras: Query<&RtsCamera>,
) -> Result {
    let ctx = contexts.ctx_mut()?;
    egui::Window::new("Sim")
        .default_pos([12.0, 48.0])
        .resizable(false)
        .show(ctx, |ui| {
            egui::Grid::new("sim-grid").num_columns(2).show(ui, |ui| {
                ui.label("tick");
                ui.monospace(sim.tick().to_string());
                ui.end_row();
                ui.label("hash");
                ui.monospace(format!("{:#018x}", sim.hash()));
                ui.end_row();
                ui.label("seed");
                ui.monospace(sim.seed().to_string());
                ui.end_row();
                ui.label("tick rate");
                ui.monospace(format!("{} Hz", sim.tick_rate_hz()));
                ui.end_row();
                ui.label("overstep");
                ui.monospace(format!("{:.2}", fixed.overstep_fraction()));
                ui.end_row();
                ui.label("ticks / frame");
                ui.monospace(stats.ticks_last_frame.to_string());
                ui.end_row();
                ui.label("dropped ticks");
                ui.monospace(stats.dropped_ticks.to_string());
                ui.end_row();
                ui.label("stalled ticks");
                ui.monospace(stats.stalled_ticks.to_string());
                ui.end_row();
                ui.label("units");
                ui.monospace(sim.view().unit_count().to_string());
                ui.end_row();
                ui.label("selected");
                ui.monospace(selection.units.len().to_string());
                ui.end_row();
                ui.label("pending cmds");
                ui.monospace(pending.len().to_string());
                ui.end_row();
                ui.label("recorded cmds");
                ui.monospace(stats.commands_recorded.to_string());
                ui.end_row();
                let (meshes, materials) = visuals.counts();
                ui.label("meshes / materials");
                ui.monospace(format!("{meshes} / {materials}"));
                ui.end_row();
                if let Ok(cam) = cameras.single() {
                    ui.label("focus");
                    ui.monospace(format!("{:.1}, {:.1}", cam.focus.x, cam.focus.z));
                    ui.end_row();
                    ui.label("zoom");
                    ui.monospace(format!("{:.1} m", cam.zoom));
                    ui.end_row();
                }
            });
        });
    Ok(())
}
