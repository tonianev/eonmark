//! The Eonmark simulation: deterministic, fixed-tick, fixed-point, engine-free.
//!
//! Invariants (see docs/DETERMINISM.md):
//! - Only [`Sim::step`] mutates state.
//! - No floats, no `HashMap`, no wall clock, no engine types in this crate
//!   (`clippy.toml` bans them; CI greps `cargo tree` for bevy/glam/wgpu/winit).
//! - Every container iterates in a total order; ids are monotonic and never reused.
//! - A command issued during tick `N` applies at tick `N + cmd_delay`.
//! - The scripted AI runs inside `step` through [`AiController`].
//!
//! Module map:
//!
//! | Module | Contents |
//! |--------|----------|
//! | [`fx`] | `Fx` 32.32 fixed point, `FxVec2`, `dist_sq_i64` |
//! | [`ids`] | `PlayerId`, `UnitId`, `BuildingId`, kind ids, `Tile`, `IdGen` |
//! | [`command`] | `Command`, `PlayerCommand`, `sort_commands` |
//! | [`state`] | `Sim`, `MatchSetup`, `State`, `Unit`, `MoveOrder`, `SimEvent`, `SubHashes`, snapshot/restore |
//! | [`map`] | `Map`: cost grid, generation, components, neighbours, BFS, spiral |
//! | [`pathing`] | budgeted resumable A*, request queue, transparent cache |
//! | [`movement`] | `SpatialGrid`, movement step, group targets |
//! | [`replay`] | `.eonreplay` writer, reader, `verify` |
//! | [`scenarios`] | scripted M1 command streams shared by tests, benches, `record` and fixtures |
//! | [`ai_hook`] | `AiController`, `SimView` |
//! | [`hash`] | xxh3 over postcard |
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod ai_hook;
pub mod command;
pub mod fx;
pub mod hash;
pub mod ids;
pub mod map;
pub mod movement;
pub mod pathing;
pub mod replay;
pub mod scenarios;
pub mod state;

pub use ai_hook::{AiController, SimView};
pub use command::{Command, PlayerCommand, sort_commands};
pub use fx::{Fx, FxVec2};
pub use ids::{
    BuildingId, BuildingKindId, IdGen, PlayerId, TargetId, TechId, Tile, UnitId, UnitKindId,
};
pub use map::Map;
pub use movement::SpatialGrid;
pub use pathing::{AStarSearch, PathRequest, Pathing, SearchStatus};
pub use replay::{ReplayError, ReplayReader, ReplayWriter, VerifyOutcome, verify};
pub use rules::Rules;
pub use state::{
    Building, MatchSetup, MoveOrder, Player, PlayerSlot, REPATH_INTERVAL_TICKS, RejectReason,
    SIM_VERSION, Sim, SimEvent, SnapshotError, SubHashes, Unit,
};
