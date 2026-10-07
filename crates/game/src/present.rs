//! Presentation of the sim (M2 decision 4): one entity per `UnitId`,
//! spawned and despawned by diffing the `SimView` after every tick, with a
//! per-unit [`Interp`] of the previous and current tick positions that the
//! render transform lerps by `Time<Fixed>::overstep_fraction()`. Yaw is
//! derived render-side from the unit's facing vector with `atan2` in f32.
//! Meshes come from `data/visuals.ron` (`rules::Visual`), one shared mesh
//! per kind and one material per `(kind, team)` so 200 units are a handful
//! of draw calls.
//!
//! Coordinates: one tile is one metre; sim `(x, y)` maps to world
//! `(x - HALF_EXTENT, y_up, y - HALF_EXTENT)` so the 128 x 128 ground mesh
//! built by `ground.rs` is centred on the origin. [`world_from_sim`] and
//! [`sim_from_world`] are the only two conversion points; orders and
//! selection use them too.
//!
//! Ownership (M2 contract): agent B owns this file. The pure helpers below
//! are final and tested; the two systems have their signatures and a
//! description of what they do, and B fills the bodies.
// M2-B: remove this allow once sync_lifecycle and sync_transforms use the
// helpers, Interp and UnitVisuals below.
#![allow(dead_code)]

use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

use bevy::prelude::*;
use bevy::transform::TransformSystems;
use rules::{Primitive, Rules, Visual};
use sim::{Fx, FxVec2, UnitId};

use crate::ground::HALF_EXTENT;
use crate::palette;
use crate::sim_driver::{SimHandle, SimSystems};

/// Height of a unit's pivot above the ground, in metres: capsules stand on
/// the ground, so the pivot is half the total height.
pub const UNIT_Y: f32 = 0.0;

/// Capsule length (cylinder part) as a multiple of the unit's radius.
pub const CAPSULE_LENGTH_PER_RADIUS: f32 = 1.4;

/// Marks a unit entity and names the sim unit it mirrors. Not `Reflect`:
/// `sim::UnitId` is engine-free and derives nothing from Bevy; the inspector
/// shows the id through `Name` instead.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnitRef(pub UnitId);

/// Previous and current tick poses of one unit, in world space. Updated by
/// [`sync_lifecycle`] after every tick; read by [`sync_transforms`] every
/// frame. On spawn `prev == curr` so a new unit does not slide in.
#[derive(Component, Debug, Clone, Copy, PartialEq, Reflect)]
#[reflect(Component)]
pub struct Interp {
    /// World position at the previous tick.
    pub prev: Vec3,
    /// World position at the current tick.
    pub curr: Vec3,
    /// Yaw (radians about +Y) at the previous tick.
    pub prev_yaw: f32,
    /// Yaw at the current tick.
    pub curr_yaw: f32,
}

impl Interp {
    /// Both poses at `pos`/`yaw` (a fresh spawn).
    pub fn at(pos: Vec3, yaw: f32) -> Self {
        Self {
            prev: pos,
            curr: pos,
            prev_yaw: yaw,
            curr_yaw: yaw,
        }
    }

    /// Shift `curr` into `prev` and set the new current pose.
    pub fn advance(&mut self, pos: Vec3, yaw: f32) {
        self.prev = self.curr;
        self.prev_yaw = self.curr_yaw;
        self.curr = pos;
        self.curr_yaw = yaw;
    }

    /// Interpolated position and yaw at `t` in `0..=1` (the overstep
    /// fraction). Yaw takes the short way round.
    pub fn sample(&self, t: f32) -> (Vec3, f32) {
        let t = t.clamp(0.0, 1.0);
        let pos = self.prev.lerp(self.curr, t);
        let mut dyaw = self.curr_yaw - self.prev_yaw;
        if dyaw > std::f32::consts::PI {
            dyaw -= std::f32::consts::TAU;
        } else if dyaw < -std::f32::consts::PI {
            dyaw += std::f32::consts::TAU;
        }
        (pos, self.prev_yaw + dyaw * t)
    }
}

/// The render-side mirror: which entity shows which sim unit. Rings, bars
/// and markers are keyed by `UnitId` elsewhere; this is the only map from
/// id to the unit's own entity.
#[derive(Resource, Default, Debug)]
pub struct UnitEntities(pub HashMap<UnitId, Entity>);

/// Shared mesh per unit kind and material per `(kind, team)`, created on
/// first use so draw calls stay low (one batch per combination).
#[derive(Resource, Default)]
pub struct UnitVisuals {
    meshes: HashMap<u16, Handle<Mesh>>,
    materials: HashMap<(u16, u8), Handle<StandardMaterial>>,
}

impl UnitVisuals {
    /// The mesh for `kind`, built from its `Visual` and `radius_tiles_x100`.
    /// A `Visual::Scene` falls back to a capsule until M7.
    pub fn mesh(&mut self, kind: u16, rules: &Rules, meshes: &mut Assets<Mesh>) -> Handle<Mesh> {
        self.meshes
            .entry(kind)
            .or_insert_with(|| {
                let radius = rules
                    .unit_kind(kind)
                    .map_or(0.35, |k| k.radius_tiles_x100 as f32 / 100.0);
                let primitive = match rules.visual(kind) {
                    Some(Visual::Primitive(p)) => *p,
                    Some(Visual::Scene { .. }) | None => Primitive::Capsule,
                };
                meshes.add(primitive_mesh(primitive, radius))
            })
            .clone()
    }

    /// The matte team-coloured material for `(kind, team)`.
    pub fn material(
        &mut self,
        kind: u16,
        team: u8,
        materials: &mut Assets<StandardMaterial>,
    ) -> Handle<StandardMaterial> {
        self.materials
            .entry((kind, team))
            .or_insert_with(|| {
                materials.add(StandardMaterial {
                    base_color: palette::team_color(team),
                    perceptual_roughness: 0.9,
                    reflectance: 0.25,
                    metallic: 0.0,
                    ..default()
                })
            })
            .clone()
    }

    /// Number of distinct meshes and materials created so far (dev panel).
    pub fn counts(&self) -> (usize, usize) {
        (self.meshes.len(), self.materials.len())
    }
}

/// Fixed-point to f32: exact for the magnitudes a 128-tile map produces.
pub fn fx_to_f32(v: Fx) -> f32 {
    v.0.to_num::<f32>()
}

/// f32 to fixed-point (rounding to the nearest representable value).
pub fn fx_from_f32(v: f32) -> Fx {
    Fx(sim::fx::Raw::from_num(v))
}

/// Sim position (tiles, y along the map's rows) to world metres with the
/// ground centred on the origin and `y_up` as height.
pub fn world_from_sim(pos: FxVec2, y_up: f32) -> Vec3 {
    Vec3::new(
        fx_to_f32(pos.x) - HALF_EXTENT,
        y_up,
        fx_to_f32(pos.y) - HALF_EXTENT,
    )
}

/// World metres back to a sim position (height ignored). The inverse of
/// [`world_from_sim`]; clamped to the map so a click past the edge still
/// resolves to a tile.
pub fn sim_from_world(p: Vec3) -> FxVec2 {
    let max = HALF_EXTENT * 2.0 - 0.001;
    FxVec2::new(
        fx_from_f32((p.x + HALF_EXTENT).clamp(0.0, max)),
        fx_from_f32((p.z + HALF_EXTENT).clamp(0.0, max)),
    )
}

/// Yaw about +Y so that a mesh facing +X at rest points along `facing`
/// (sim x maps to world x, sim y to world z): `atan2(-z, x)`.
pub fn yaw_from_facing(facing: FxVec2) -> f32 {
    let x = fx_to_f32(facing.x);
    let z = fx_to_f32(facing.y);
    if x == 0.0 && z == 0.0 {
        0.0
    } else {
        (-z).atan2(x)
    }
}

/// The mesh for a primitive sized from the unit's collision radius: the
/// capsule and cylinder stand on the ground (their origin is lifted so the
/// bottom touches y = 0), the cuboid is a radius-wide box of twice the
/// radius in height, the sphere sits on the ground.
pub fn primitive_mesh(primitive: Primitive, radius: f32) -> Mesh {
    let lift = |mut mesh: Mesh, dy: f32| {
        mesh.translate_by(Vec3::Y * dy);
        mesh
    };
    match primitive {
        Primitive::Capsule => {
            let length = radius * CAPSULE_LENGTH_PER_RADIUS;
            lift(
                Mesh::from(Capsule3d::new(radius, length)),
                radius + length * 0.5,
            )
        }
        Primitive::Cuboid => lift(
            Mesh::from(Cuboid::new(radius * 2.0, radius * 2.0, radius * 2.0)),
            radius,
        ),
        Primitive::Cylinder => lift(Mesh::from(Cylinder::new(radius, radius * 2.0)), radius),
        Primitive::Sphere => lift(Mesh::from(Sphere::new(radius)), radius),
    }
}

/// Rotation that lays a gizmo circle (drawn in XY) flat on the ground.
pub fn flat_on_ground() -> Quat {
    Quat::from_rotation_x(-FRAC_PI_2)
}

/// Mirrors sim units into entities and interpolates their transforms.
pub struct PresentPlugin;

impl Plugin for PresentPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UnitEntities>()
            .init_resource::<UnitVisuals>()
            .register_type::<Interp>()
            .add_systems(FixedUpdate, sync_lifecycle.after(SimSystems::Step))
            .add_systems(
                PostUpdate,
                sync_transforms.before(TransformSystems::Propagate),
            );
    }
}

/// After every tick: for each unit in `sim.view().units()`, update its
/// [`Interp`] (`advance` to the new pose) or spawn a new entity when the id
/// is not in [`UnitEntities`] (`Mesh3d`, `MeshMaterial3d`, `Transform`,
/// `UnitRef`, `Interp::at`, `Pickable::default()`, `Name`); despawn every
/// entity whose id is no longer in the view and drop it from the map. Runs
/// in `FixedUpdate` after [`SimSystems::Step`] so it sees every tick even
/// when several run in one frame.
///
/// M2-B: body to implement.
pub fn sync_lifecycle(
    _sim: NonSend<SimHandle>,
    _entities: ResMut<UnitEntities>,
    _visuals: ResMut<UnitVisuals>,
    _meshes: ResMut<Assets<Mesh>>,
    _materials: ResMut<Assets<StandardMaterial>>,
    _interps: Query<&mut Interp>,
    _commands: Commands,
) {
    // M2-B: diff ids, spawn/despawn, advance Interp; see the doc comment.
}

/// Every frame: `transform.translation, yaw = interp.sample(fixed.overstep_fraction())`,
/// `transform.rotation = Quat::from_rotation_y(yaw)`. Runs in `PostUpdate`
/// before transform propagation so the frame renders the interpolated pose.
///
/// M2-B: body to implement.
pub fn sync_transforms(
    _fixed: Res<Time<Fixed>>,
    _units: Query<(&Interp, &mut Transform), With<UnitRef>>,
) {
    // M2-B: lerp by overstep fraction; see the doc comment.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sim_world_round_trip_is_centred_on_the_origin() {
        let centre = FxVec2::from_ints(64, 64);
        assert_eq!(world_from_sim(centre, 0.0), Vec3::ZERO);
        let west = world_from_sim(sim::scenarios::WEST, 0.0);
        assert!(west.x < 0.0 && west.z == 0.0);
        let back = sim_from_world(west);
        assert_eq!(back, sim::scenarios::WEST);
        // Half-tile positions survive the round trip.
        let p = FxVec2::new(Fx::from_ratio(51, 2), Fx::from_ratio(7, 4));
        assert_eq!(sim_from_world(world_from_sim(p, 3.0)), p);
        // Clicks past the edge clamp into the map.
        let far = sim_from_world(Vec3::new(1e4, 0.0, -1e4));
        assert!(far.x < Fx::from_int(128) && far.y >= Fx::ZERO);
    }

    #[test]
    fn yaw_follows_the_facing_vector() {
        let eps = 1e-5;
        assert!(
            (yaw_from_facing(FxVec2::from_ints(1, 0))).abs() < eps,
            "+x is yaw 0"
        );
        assert!(
            (yaw_from_facing(FxVec2::from_ints(0, 1)) + FRAC_PI_2).abs() < eps,
            "+sim y (world +z) is -90 deg"
        );
        assert!((yaw_from_facing(FxVec2::from_ints(0, -1)) - FRAC_PI_2).abs() < eps);
        assert_eq!(yaw_from_facing(FxVec2::ZERO), 0.0);
        // Rotating a +X vector by the yaw gives the world-space direction.
        let yaw = yaw_from_facing(FxVec2::from_ints(1, 1));
        let dir = Quat::from_rotation_y(yaw) * Vec3::X;
        assert!((dir - Vec3::new(1.0, 0.0, 1.0).normalize()).length() < 1e-5);
    }

    #[test]
    fn interp_samples_between_poses_and_takes_the_short_way_round() {
        let mut i = Interp::at(Vec3::ZERO, 0.0);
        i.advance(Vec3::X * 2.0, 3.0);
        let (p, yaw) = i.sample(0.5);
        assert_eq!(p, Vec3::X);
        assert!((yaw - 1.5).abs() < 1e-6);
        let mut j = Interp::at(Vec3::ZERO, 3.0);
        j.advance(Vec3::ZERO, -3.0);
        let (_, yaw) = j.sample(0.5);
        // From 3.0 to -3.0 is 0.28 rad through pi, not 6 rad through zero.
        assert!(yaw.abs() > 3.0, "{yaw}");
        assert_eq!(i.sample(2.0).0, Vec3::X * 2.0, "clamped");
    }

    #[test]
    fn primitive_meshes_stand_on_the_ground() {
        use bevy::camera::primitives::MeshAabb;
        for p in [
            Primitive::Capsule,
            Primitive::Cuboid,
            Primitive::Cylinder,
            Primitive::Sphere,
        ] {
            let mesh = primitive_mesh(p, 0.35);
            let aabb = mesh.compute_aabb().expect("positions");
            let min_y = aabb.min().y;
            assert!(min_y.abs() < 1e-4, "{p:?} bottom at {min_y}");
            assert!(aabb.max().y > 0.3, "{p:?}");
        }
    }

    #[test]
    fn visuals_are_shared_per_kind_and_team() {
        let rules =
            Rules::load(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data"))
                .unwrap();
        let mut meshes = Assets::<Mesh>::default();
        let mut materials = Assets::<StandardMaterial>::default();
        let mut v = UnitVisuals::default();
        let a = v.mesh(0, &rules, &mut meshes);
        let b = v.mesh(0, &rules, &mut meshes);
        assert_eq!(a, b);
        let m0 = v.material(0, 0, &mut materials);
        let m1 = v.material(0, 1, &mut materials);
        assert_ne!(m0, m1);
        assert_eq!(v.material(0, 0, &mut materials), m0);
        assert_eq!(v.counts(), (1, 2));
    }
}
