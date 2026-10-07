//! Frame-time statistics and the `units200_auto` arrival proxy, printed on
//! exit so the M2 acceptance line "FPS >= 60, no periodic hitch at the 1 s
//! replay flush" and the `--max-fps` cap have numbers instead of eyeballs.
//!
//! `frame_ms mean=<f> p95=<f> max=<f> max_at_flush=<f>` is the frame
//! period in milliseconds over the run (`Time<Real>::delta`, the first frame
//! skipped). `max_at_flush` is the worst frame among the frame that sent a
//! recorder flush (every [`FLUSH_EVERY_TICKS`], see
//! [`DriverStats::flushes_sent`]) and the frame after it, where a blocking
//! write would show up. `frames=<n> seconds=<s> fps=<f>` is the plain mean
//! rate for the `--max-fps` check. With the arrival proxy on,
//! `arrived=<n>/<total> by tick <t>` counts units within
//! [`ARRIVED_TILES`] of the scenario's goal: `n` at exit, `t` the tick the
//! count first reached `total` (or the exit tick when it never did).

use std::time::Duration;

use bevy::app::AppExit;
use bevy::prelude::*;
use sim::FxVec2;

use crate::sim_driver::{DriverStats, FLUSH_EVERY_TICKS, SimHandle};

/// A unit counts as arrived within this many tiles of its goal, the same
/// gate `sim-cli bench` uses (docs/design/pathing.md). A measurement
/// threshold for the proxy line, not a gameplay number.
pub const ARRIVED_TILES: i64 = 3;

/// `ARRIVED_TILES` squared, in `FxVec2::dist_sq_i64` scale.
const ARRIVED_DIST_SQ: i64 = (ARRIVED_TILES * ARRIVED_TILES) << 32;

/// Frames flagged after each flush: the flush frame itself and the next.
const FLUSH_WINDOW_FRAMES: u8 = 2;

/// Collected frame periods and flush attribution.
#[derive(Resource, Debug, Default)]
pub struct FrameStats {
    /// Frame periods in milliseconds, in order.
    pub samples_ms: Vec<f64>,
    /// Worst frame in a flush window.
    pub max_at_flush_ms: f64,
    /// `DriverStats::flushes_sent` last seen.
    flushes_seen: u32,
    /// Frames still to attribute to the latest flush.
    flush_window: u8,
}

/// Mean, 95th percentile and maximum of the samples (milliseconds).
pub fn summarize(samples: &[f64]) -> (f64, f64, f64) {
    if samples.is_empty() {
        return (0.0, 0.0, 0.0);
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
    let rank = ((sorted.len() as f64 * 0.95).ceil() as usize).clamp(1, sorted.len());
    (mean, sorted[rank - 1], sorted[sorted.len() - 1])
}

impl FrameStats {
    /// Record one frame period and attribute it to a flush window when due.
    /// `flushes_sent` is the driver's counter as of the frame that is
    /// running now; the period of that frame is the next sample, so a
    /// change opens the window after this one.
    pub fn record(&mut self, frame: Duration, flushes_sent: u32) {
        let ms = frame.as_secs_f64() * 1000.0;
        self.samples_ms.push(ms);
        if self.flush_window > 0 {
            self.flush_window -= 1;
            self.max_at_flush_ms = self.max_at_flush_ms.max(ms);
        }
        if flushes_sent != self.flushes_seen {
            self.flushes_seen = flushes_sent;
            self.flush_window = FLUSH_WINDOW_FRAMES;
        }
    }

    /// The `frame_ms ...` line.
    pub fn frame_line(&self) -> String {
        let (mean, p95, max) = summarize(&self.samples_ms);
        format!(
            "frame_ms mean={mean:.2} p95={p95:.2} max={max:.2} max_at_flush={:.2}",
            self.max_at_flush_ms
        )
    }

    /// The `frames=... fps=...` line.
    pub fn rate_line(&self) -> String {
        let seconds = self.samples_ms.iter().sum::<f64>() / 1000.0;
        let fps = if seconds > 0.0 {
            self.samples_ms.len() as f64 / seconds
        } else {
            0.0
        };
        format!(
            "frames={} seconds={seconds:.2} fps={fps:.1}",
            self.samples_ms.len()
        )
    }
}

/// Arrival tracking for a scenario whose units all head for one goal.
#[derive(Resource, Debug, Clone, PartialEq, Eq)]
pub struct ArrivalProxy {
    /// Where the scripted order sends every unit.
    pub goal: FxVec2,
    /// Units expected to arrive.
    pub total: u32,
    /// Units within [`ARRIVED_TILES`] of `goal` after the latest tick.
    pub arrived: u32,
    /// First tick at which `arrived == total`.
    pub all_arrived_at: Option<u32>,
}

impl ArrivalProxy {
    /// Track `total` units heading for `goal`.
    pub fn new(goal: FxVec2, total: u32) -> Self {
        Self {
            goal,
            total,
            arrived: 0,
            all_arrived_at: None,
        }
    }

    /// Count after a tick; `tick` is the completed tick.
    pub fn observe(&mut self, positions: impl Iterator<Item = FxVec2>, tick: u32) {
        let arrived = positions
            .filter(|pos| pos.dist_sq_i64(self.goal) <= ARRIVED_DIST_SQ)
            .count();
        self.arrived = u32::try_from(arrived).unwrap_or(u32::MAX);
        if self.arrived >= self.total && self.all_arrived_at.is_none() {
            self.all_arrived_at = Some(tick);
        }
    }

    /// The `arrived=...` line for an exit at `exit_tick`.
    pub fn line(&self, exit_tick: u32) -> String {
        format!(
            "arrived={}/{} by tick {}",
            self.arrived,
            self.total,
            self.all_arrived_at.unwrap_or(exit_tick)
        )
    }
}

/// Collects frame periods and prints the summary lines when the app exits.
pub struct FrameStatsPlugin {
    /// Also count arrivals at this goal for this many units.
    pub arrival: Option<(FxVec2, u32)>,
}

impl Plugin for FrameStatsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FrameStats>()
            .add_systems(Update, sample_frame)
            .add_systems(Last, print_on_exit);
        if let Some((goal, total)) = self.arrival {
            app.insert_resource(ArrivalProxy::new(goal, total))
                .add_systems(FixedPostUpdate, observe_arrivals);
        }
    }
}

fn sample_frame(time: Res<Time<Real>>, driver: Res<DriverStats>, mut stats: ResMut<FrameStats>) {
    // The first frame's delta is zero (the clock has no previous update).
    if time.delta() > Duration::ZERO {
        stats.record(time.delta(), driver.flushes_sent);
    }
}

fn observe_arrivals(sim: NonSend<SimHandle>, mut proxy: ResMut<ArrivalProxy>) {
    let view = sim.view();
    proxy.observe(view.units().map(|u| u.pos), sim.tick());
}

fn print_on_exit(
    mut exits: MessageReader<AppExit>,
    stats: Res<FrameStats>,
    driver: Res<DriverStats>,
    arrival: Option<Res<ArrivalProxy>>,
    sim: NonSend<SimHandle>,
) {
    if exits.is_empty() {
        return;
    }
    exits.clear();
    println!("{}", stats.frame_line());
    println!("{}", stats.rate_line());
    println!(
        "ticks={} dropped_ticks={} stalled_ticks={} flushes={} (every {FLUSH_EVERY_TICKS} ticks)",
        sim.tick(),
        driver.dropped_ticks,
        driver.stalled_ticks,
        driver.flushes_sent
    );
    if let Some(arrival) = arrival {
        println!("{}", arrival.line(sim.tick()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_is_mean_p95_max() {
        let (mean, p95, max) = summarize(&[1.0, 2.0, 3.0, 4.0, 100.0]);
        assert!((mean - 22.0).abs() < 1e-9);
        assert_eq!(p95, 100.0);
        assert_eq!(max, 100.0);
        let twenty: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(summarize(&twenty).1, 19.0, "p95 of 1..=20 is the 19th");
        assert_eq!(summarize(&[]), (0.0, 0.0, 0.0));
    }

    #[test]
    fn flush_window_covers_the_flush_frame_and_the_next() {
        let mut s = FrameStats::default();
        let f = |ms: u64| Duration::from_millis(ms);
        s.record(f(10), 0); // quiet frame
        s.record(f(10), 1); // the flush was sent in the frame measured NEXT
        s.record(f(30), 1); // flush frame
        s.record(f(25), 1); // frame after
        s.record(f(90), 1); // outside the window
        assert_eq!(s.max_at_flush_ms, 30.0);
        assert_eq!(s.samples_ms.len(), 5);
        assert_eq!(
            s.frame_line(),
            "frame_ms mean=33.00 p95=90.00 max=90.00 max_at_flush=30.00"
        );
        assert_eq!(s.rate_line(), "frames=5 seconds=0.17 fps=30.3");
    }

    #[test]
    fn arrival_proxy_counts_and_remembers_the_first_full_tick() {
        let goal = FxVec2::from_ints(100, 64);
        let mut a = ArrivalProxy::new(goal, 2);
        let far = FxVec2::from_ints(10, 64);
        let near = FxVec2::from_ints(102, 65);
        a.observe([far, near].into_iter(), 5);
        assert_eq!(a.arrived, 1);
        assert_eq!(a.all_arrived_at, None);
        assert_eq!(a.line(5), "arrived=1/2 by tick 5");
        a.observe([near, goal].into_iter(), 9);
        assert_eq!(a.all_arrived_at, Some(9));
        a.observe([far, far].into_iter(), 12);
        assert_eq!(a.line(12), "arrived=0/2 by tick 9");
    }
}
