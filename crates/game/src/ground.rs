//! The flat ground: a 128 x 128 m grid of 1 m quads with flat normals and
//! per-tile vertex colours, plus a gizmo grid every 8 tiles. No heightmap
//! in v0.1. Tiles take their colour from the match's map: grass in three
//! close greens, forest floor, rock and water (`docs/ART_STYLE.md`).
//!
//! Territory (M3b) is drawn into the same vertex colours: an owned tile is
//! tinted [`TERRITORY_TINT`] toward its owner's team colour, and an owned
//! tile beside a tile of another owner (or neutral) is tinted
//! [`BORDER_TINT`], a 1-tile line on the inside of every border. Neutral
//! and tied tiles keep their ground colour. The colours are rebuilt only
//! when the field's owner grid changes, compared after every tick.

use std::f32::consts::FRAC_PI_2;

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use rules::{MapDef, Terrain};

use crate::palette;
use crate::sim_driver::{SimHandle, SimSystems, sim_stepped};

/// Ground edge length in tiles (and metres).
pub const GROUND_SIZE_TILES: u32 = 128;
/// Edge length of one tile in metres.
pub const TILE_SIZE: f32 = 1.0;
/// Gizmo grid spacing in tiles.
pub const GRID_SPACING_TILES: u32 = 8;
/// Half the ground edge in metres; the camera focus is clamped to this.
pub const HALF_EXTENT: f32 = GROUND_SIZE_TILES as f32 * TILE_SIZE * 0.5;
/// How far an owned tile is blended toward its team colour
/// (`docs/ART_STYLE.md`: about 12%).
pub const TERRITORY_TINT: f32 = 0.12;
/// How far an owned tile on the edge of its territory is blended toward
/// its team colour: the soft 1-tile border line.
pub const BORDER_TINT: f32 = 0.45;

/// Spawns the ground mesh and draws the grid.
pub struct GroundPlugin;

impl Plugin for GroundPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_ground, configure_gizmos))
            .add_systems(Update, draw_grid)
            .add_systems(
                FixedUpdate,
                sync_territory.after(SimSystems::Step).run_if(sim_stepped),
            );
    }
}

/// The ground mesh, each tile's untinted colour (row-major, sim tile
/// `(x, y)` at `y * 128 + x`), and the owner grid the vertex colours
/// currently show.
#[derive(Resource)]
pub struct GroundTiles {
    mesh: Handle<Mesh>,
    base: Vec<Color>,
    shown: Vec<u8>,
}

/// Marker for the ground entity.
#[derive(Component)]
pub struct Ground;

/// Deterministic tile colour choice. Not sim state: purely cosmetic, so it
/// may use floats and need not match across machines.
fn tile_palette_index(x: u32, z: u32) -> usize {
    let mut h = x.wrapping_mul(0x9E37_79B1) ^ z.wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    // Mostly the base green; each variant appears on 1/8 of tiles.
    match h & 7 {
        0..=5 => 0,
        6 => 1,
        _ => 2,
    }
}

/// The colour of tile `(x, z)`: its terrain from `map`, all grass without a
/// map. Grass varies between three close greens.
fn tile_color(map: Option<&MapDef>, x: u32, z: u32) -> Color {
    let terrain = map
        .filter(|m| x < u32::from(m.width) && z < u32::from(m.height))
        .map_or(Terrain::Grass, |m| {
            m.tile(
                u16::try_from(x).expect("map width fits u16"),
                u16::try_from(z).expect("map height fits u16"),
            )
        });
    match terrain {
        Terrain::Grass => palette::GRASS[tile_palette_index(x, z)],
        Terrain::Forest => palette::FOREST_FLOOR,
        Terrain::Mountain => palette::ROCK,
        Terrain::Water => palette::WATER,
    }
}

/// Each ground tile's untinted colour, row-major.
pub fn base_colors(map: Option<&MapDef>) -> Vec<Color> {
    let n = GROUND_SIZE_TILES;
    (0..n)
        .flat_map(|z| (0..n).map(move |x| (x, z)))
        .map(|(x, z)| tile_color(map, x, z))
        .collect()
}

/// The colour of every tile with territory drawn in: `owners` is the
/// field's owner grid (`0` neutral, else player index + 1), `width` x
/// `height` row-major; tiles outside it are untinted. An owned tile whose
/// 4-neighbour inside the field has a different owner is a border tile.
pub fn territory_colors(base: &[Color], owners: &[u8], width: u32, height: u32) -> Vec<Color> {
    let n = GROUND_SIZE_TILES;
    let owner = |x: i64, z: i64| -> Option<u8> {
        if x < 0 || z < 0 || x >= i64::from(width) || z >= i64::from(height) {
            return None;
        }
        owners.get((z * i64::from(width) + x) as usize).copied()
    };
    (0..n)
        .flat_map(|z| (0..n).map(move |x| (x, z)))
        .map(|(x, z)| {
            let ground = base[(z * n + x) as usize];
            let (xi, zi) = (i64::from(x), i64::from(z));
            match owner(xi, zi) {
                Some(code) if code > 0 => {
                    let edge = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                        .iter()
                        .any(|(dx, dz)| owner(xi + dx, zi + dz).is_some_and(|c| c != code));
                    let team = palette::team_color(code - 1);
                    ground.mix(&team, if edge { BORDER_TINT } else { TERRITORY_TINT })
                }
                _ => ground,
            }
        })
        .collect()
}

/// Per-vertex colours for the ground mesh from per-tile colours (four
/// vertices per tile, in mesh order).
fn vertex_colors(tiles: &[Color]) -> Vec<[f32; 4]> {
    tiles
        .iter()
        .flat_map(|c| [c.to_linear().to_f32_array(); 4])
        .collect()
}

/// Build the ground mesh: four unique vertices per tile so normals and
/// colours are flat per tile. 65 536 vertices, 98 304 indices (u32). Tile
/// colours follow `map`'s terrain (sim tile `(x, y)` is world tile
/// `(x, z)`); without a map every tile is grass. The mesh keeps its CPU
/// copy (`MAIN_WORLD | RENDER_WORLD`) so the territory tint can rewrite its
/// colours.
pub fn build_ground_mesh(map: Option<&MapDef>) -> Mesh {
    let n = GROUND_SIZE_TILES;
    let tiles = (n * n) as usize;
    let mut positions: Vec<[f32; 3]> = Vec::with_capacity(tiles * 4);
    let mut normals: Vec<[f32; 3]> = Vec::with_capacity(tiles * 4);
    let mut indices: Vec<u32> = Vec::with_capacity(tiles * 6);

    for z in 0..n {
        for x in 0..n {
            let x0 = x as f32 * TILE_SIZE - HALF_EXTENT;
            let z0 = z as f32 * TILE_SIZE - HALF_EXTENT;
            let x1 = x0 + TILE_SIZE;
            let z1 = z0 + TILE_SIZE;
            let base = positions.len() as u32;
            positions.extend_from_slice(&[
                [x0, 0.0, z0],
                [x1, 0.0, z0],
                [x1, 0.0, z1],
                [x0, 0.0, z1],
            ]);
            normals.extend_from_slice(&[[0.0, 1.0, 0.0]; 4]);
            // Counter-clockwise seen from +Y.
            indices.extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }

    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, vertex_colors(&base_colors(map)))
    .with_inserted_indices(Indices::U32(indices))
}

fn spawn_ground(
    mut commands: Commands,
    sim: Option<NonSend<SimHandle>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let map = sim.as_ref().and_then(|s| s.rules().map(&s.setup().map));
    let mesh = meshes.add(build_ground_mesh(map));
    commands.insert_resource(GroundTiles {
        mesh: mesh.clone(),
        base: base_colors(map),
        shown: Vec::new(),
    });
    let material = materials.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.95,
        reflectance: 0.2,
        metallic: 0.0,
        ..default()
    });
    commands.spawn((
        Name::new("Ground"),
        Ground,
        Mesh3d(mesh),
        MeshMaterial3d(material),
        Transform::IDENTITY,
    ));
}

/// After every tick: when the territory field's owner grid differs from
/// the one the ground shows, rewrite the ground's vertex colours.
pub fn sync_territory(
    sim: NonSend<SimHandle>,
    tiles: Option<ResMut<GroundTiles>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let Some(mut tiles) = tiles else {
        return;
    };
    let view = sim.view();
    let field = view.territory();
    if field.owner_grid() == tiles.shown.as_slice() {
        return;
    }
    let colors = territory_colors(
        &tiles.base,
        field.owner_grid(),
        u32::from(field.width()),
        u32::from(field.height()),
    );
    if let Some(mut mesh) = meshes.get_mut(&tiles.mesh) {
        mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, vertex_colors(&colors));
    }
    tiles.shown = field.owner_grid().to_vec();
}

fn configure_gizmos(mut store: ResMut<GizmoConfigStore>) {
    let (config, _) = store.config_mut::<DefaultGizmoConfigGroup>();
    config.line.width = 1.0;
}

fn draw_grid(mut gizmos: Gizmos) {
    let cells = GROUND_SIZE_TILES / GRID_SPACING_TILES;
    let spacing = GRID_SPACING_TILES as f32 * TILE_SIZE;
    // `grid` draws in the XY plane; rotate it onto XZ and lift it 2 cm to
    // avoid z-fighting with the ground.
    gizmos.grid(
        Isometry3d::new(Vec3::Y * 0.02, Quat::from_rotation_x(-FRAC_PI_2)),
        UVec2::splat(cells),
        Vec2::splat(spacing),
        palette::GRID_LINE,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_has_four_vertices_and_six_indices_per_tile() {
        let mesh = build_ground_mesh(None);
        let tiles = (GROUND_SIZE_TILES * GROUND_SIZE_TILES) as usize;
        assert_eq!(mesh.count_vertices(), tiles * 4);
        let Some(Indices::U32(indices)) = mesh.indices() else {
            panic!("expected u32 indices");
        };
        assert_eq!(indices.len(), tiles * 6);
    }

    #[test]
    fn tiles_take_their_terrain_colour_from_the_map() {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let rules = rules::Rules::load(&data).expect("data loads");
        let map = rules.map("plains_1v1").expect("default map");
        let mut seen = [false; 4];
        for z in 0..u32::from(map.height) {
            for x in 0..u32::from(map.width) {
                let terrain = map.tile(x as u16, z as u16);
                let want = match terrain {
                    Terrain::Grass => None,
                    Terrain::Forest => Some(palette::FOREST_FLOOR),
                    Terrain::Mountain => Some(palette::ROCK),
                    Terrain::Water => Some(palette::WATER),
                };
                seen[terrain as usize] = true;
                let got = tile_color(Some(map), x, z);
                match want {
                    Some(c) => assert_eq!(got, c, "({x}, {z})"),
                    None => assert!(palette::GRASS.contains(&got), "({x}, {z})"),
                }
            }
        }
        assert_eq!(seen, [true; 4], "plains_1v1 has every terrain");
    }

    #[test]
    fn owned_tiles_are_tinted_and_their_edge_is_a_stronger_line() {
        let base = base_colors(None);
        let n = GROUND_SIZE_TILES as usize;
        // Player 0 owns a 5x5 block at (10..15, 20..25); player 1 owns the
        // tile right of it; everything else is neutral.
        let mut owners = vec![0u8; n * n];
        for z in 20..25 {
            for x in 10..15 {
                owners[z * n + x] = 1;
            }
        }
        owners[22 * n + 15] = 2;
        let colors = territory_colors(&base, &owners, n as u32, n as u32);
        let at = |x: usize, z: usize| colors[z * n + x];
        let blue = palette::team_color(0);
        assert_eq!(
            at(12, 22),
            base[22 * n + 12].mix(&blue, TERRITORY_TINT),
            "interior"
        );
        assert_eq!(
            at(10, 22),
            base[22 * n + 10].mix(&blue, BORDER_TINT),
            "edge by neutral"
        );
        assert_eq!(
            at(14, 22),
            base[22 * n + 14].mix(&blue, BORDER_TINT),
            "edge by player 1"
        );
        let red = palette::team_color(1);
        assert_eq!(
            at(15, 22),
            base[22 * n + 15].mix(&red, BORDER_TINT),
            "a lone tile is all edge"
        );
        assert_eq!(at(9, 22), base[22 * n + 9], "neutral is untinted");
        assert_eq!(at(60, 60), base[60 * n + 60]);
    }

    #[test]
    fn a_field_smaller_than_the_ground_tints_only_its_tiles() {
        let base = base_colors(None);
        let owners = vec![1u8; 4 * 4];
        let colors = territory_colors(&base, &owners, 4, 4);
        let n = GROUND_SIZE_TILES as usize;
        assert_ne!(colors[0], base[0]);
        assert_eq!(colors[5 * n + 5], base[5 * n + 5]);
        // The map edge is not a border: tile (0, 0) has no outside neighbour.
        let blue = palette::team_color(0);
        assert_eq!(colors[n + 1], base[n + 1].mix(&blue, TERRITORY_TINT));
    }

    #[test]
    fn palette_index_is_in_range_and_stable() {
        for z in 0..GROUND_SIZE_TILES {
            for x in 0..GROUND_SIZE_TILES {
                let i = tile_palette_index(x, z);
                assert!(i < palette::GRASS.len());
                assert_eq!(i, tile_palette_index(x, z));
            }
        }
    }
}
