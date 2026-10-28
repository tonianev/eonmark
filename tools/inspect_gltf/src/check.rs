//! `check <data> <assets>`: every Scene in `visuals.ron` (its parts and
//! node filters resolved, placed with its scale, yaw and offset as the game
//! places it) against the art pass's budgets and import rules
//! (`docs/ART_STYLE.md`): file size, triangles per use, texture size,
//! indexed triangles with `POSITION`, `NORMAL` and `TEXCOORD_0` and one base
//! texture, no cameras, lights, skins or animations, ground contact, team
//! area against `team_swatches`, the reserved-rect rule and the 1.4x
//! silhouette rule within a rig group. It prints one `ok` line per Scene
//! with triangles, bytes, top, size and team share, plus a suggested
//! `scale_x100` for a building's footprint or a unit's look height.

use crate::bake::rig_key;
use crate::image::Rgba;
use crate::math::{Mat4, Vec3, triangle_area};
use crate::model::{Model, bounds};
use crate::uv::{Cover, Mask, Rect, face_rect};
use rules::visuals::{DecorAt, Part, Primitive, Visual, Visuals};
use rules::{BuildingKind, UnitKind};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Textures shipped by a pack that may exceed the 256-texel rule because
/// every model of the pack shares them (owner decision 5 of the art pass:
/// KayKit Hexagon's palette atlas is vendored as shipped).
pub const SHIPPED_ATLASES: &[&str] = &["hexagons_medieval.png"];

/// Lowest and highest allowed base of a placed model, metres.
pub const GROUND_Y: (f32, f32) = (-0.02, 0.05);
/// Largest x/z distance of a placed model's centre from its origin, metres.
pub const CENTRE_TOLERANCE: f32 = 0.10;
/// The same for terrain props, which are jittered anyway.
pub const PROP_CENTRE_TOLERANCE: f32 = 0.5;
/// Share of a footprint side a building model should fill.
pub const FOOTPRINT_FILL: f32 = 0.95;

/// The limits `check` applies (`docs/ART_STYLE.md`, "Budgets").
#[derive(Clone, Debug)]
pub struct Budgets {
    /// Bytes per file.
    pub file_bytes: u64,
    /// Texture side in texels (except [`SHIPPED_ATLASES`]).
    pub texture_side: u32,
    /// A unit above this many triangles prints a note.
    pub unit_triangles_target: usize,
    /// Unit triangle cap.
    pub unit_triangles: usize,
    /// Building triangle cap.
    pub building_triangles: usize,
    /// Construction-site and rubble cap.
    pub site_triangles: usize,
    /// Seat-flag cap.
    pub flag_triangles: usize,
    /// Cap for a prop that stands only on interior tiles.
    pub interior_prop_triangles: usize,
    /// Cap for a prop that may stand on edge tiles.
    pub edge_prop_triangles: usize,
    /// Least team share of a unit, permille.
    pub unit_team_permille: u32,
    /// Least team share of the Supply Wain (only its pennant), permille.
    pub wain_team_permille: u32,
    /// Least team share of a building, permille.
    pub building_team_permille: u32,
    /// Least team share of the Seat flag, permille.
    pub flag_team_permille: u32,
}

impl Default for Budgets {
    /// The caps set in art slice 2 from the measured sources
    /// (`inspect_gltf inspect` on every chosen file; `docs/ART_STYLE.md`,
    /// "Budgets"), each a little above the heaviest file it covers:
    ///
    /// - units: the KayKit Adventurers bodies with their kept items are
    ///   4,005 (hooded Rogue) to 5,325 (Barbarian with two axes) triangles,
    ///   and no lighter CC0 character is reachable, so the plan's
    ///   provisional 2,000 / 3,000 became 5,000 / 5,500; the Mangonel's
    ///   catapult nodes are 1,326 and the Wain's parts 668;
    /// - buildings: the castle is the heaviest at 5,659 (under the plan's
    ///   12,000, so it stays);
    /// - sites and rubble: `building_stage_C` 1,218;
    /// - the Seat flag: 48;
    /// - interior props: `trees_B_large` 1,540 (under the plan's 2,000, so
    ///   the large trees stay for interiors);
    /// - edge props: `tree_single_B` 220.
    ///
    /// The team floors are the plan's (units 200‰, the Wain 80‰, buildings
    /// 100‰); the Seat flag's 300‰ sits under its measured 457‰.
    fn default() -> Self {
        Budgets {
            file_bytes: 2 * 1024 * 1024,
            texture_side: 256,
            unit_triangles_target: 5_000,
            unit_triangles: 5_500,
            building_triangles: 6_000,
            site_triangles: 1_500,
            flag_triangles: 100,
            interior_prop_triangles: 1_600,
            edge_prop_triangles: 300,
            unit_team_permille: 200,
            wain_team_permille: 80,
            building_team_permille: 100,
            flag_team_permille: 300,
        }
    }
}

/// What a Scene is drawn for.
#[derive(Clone, Copy)]
enum Use<'a> {
    Unit(&'a UnitKind),
    Building(&'a BuildingKind),
    Site,
    Rubble,
    Flag,
    Prop(DecorAt),
}

struct SceneRef<'a> {
    path: &'a str,
    scale_x100: u32,
    y_offset_x100: i32,
    yaw_deg: i32,
    nodes: &'a [String],
    parts: &'a [Part],
    team: Option<&'a str>,
    fallback: Option<&'a Visual>,
}

fn scene_ref(v: &Visual) -> Option<SceneRef<'_>> {
    match v {
        Visual::Scene {
            path,
            scale_x100,
            y_offset_tiles_x100,
            yaw_deg,
            nodes,
            parts,
            team,
            fallback,
        } => Some(SceneRef {
            path,
            scale_x100: *scale_x100,
            y_offset_x100: *y_offset_tiles_x100,
            yaw_deg: *yaw_deg,
            nodes,
            parts,
            team: team.as_deref(),
            fallback: fallback.as_deref(),
        }),
        _ => None,
    }
}

/// The placed, merged model of one Scene.
struct Placed {
    tris: Vec<([Vec3; 3], [[f32; 2]; 3])>,
    texture: Option<(Rgba, String, u64)>,
    files: BTreeMap<PathBuf, u64>,
    rig: String,
    problems: Vec<String>,
}

/// `T(0, lift) * S(scale) * R(yaw)`: the game's placement of a Scene.
fn scene_matrix(s: &SceneRef) -> Mat4 {
    Mat4::translation([0.0, s.y_offset_x100 as f32 / 100.0, 0.0])
        .mul(&Mat4::scale(s.scale_x100 as f32 / 100.0))
        .mul(&Mat4::yaw_deg(s.yaw_deg))
}

/// `T(offset) * R(yaw) * S(scale)`: a part in its scene's model space.
fn part_matrix(p: &Part) -> Mat4 {
    let o = p.offset_x100;
    Mat4::translation([o.0 as f32 / 100.0, o.1 as f32 / 100.0, o.2 as f32 / 100.0])
        .mul(&Mat4::yaw_deg(p.yaw_deg))
        .mul(&Mat4::scale(p.scale_x100 as f32 / 100.0))
}

fn place(s: &SceneRef, assets: &Path, budgets: &Budgets) -> Placed {
    let mut placed = Placed {
        tris: Vec::new(),
        texture: None,
        files: BTreeMap::new(),
        rig: String::new(),
        problems: Vec::new(),
    };
    let scene = scene_matrix(s);
    let mut sources: Vec<(&str, &[String], Mat4)> = vec![(s.path, s.nodes, Mat4::IDENTITY)];
    for p in s.parts {
        sources.push((&p.path, &p.nodes, part_matrix(p)));
    }
    for (i, (path, nodes, local)) in sources.into_iter().enumerate() {
        let file = assets.join(path);
        let model = match Model::load(&file) {
            Ok(m) => m,
            Err(e) => {
                placed.problems.push(e);
                continue;
            }
        };
        if i == 0 {
            placed.rig = rig_key(&model);
            if placed.rig.is_empty() {
                placed.rig = path.to_string();
            }
        }
        for (f, bytes) in &model.files {
            if *bytes > budgets.file_bytes {
                placed.problems.push(format!(
                    "{} is {bytes} bytes; the limit is {}",
                    f.display(),
                    budgets.file_bytes
                ));
            }
            placed.files.insert(f.clone(), *bytes);
        }
        if model.doc.cameras().count() > 0 {
            placed.problems.push(format!("{path} has cameras"));
        }
        if model
            .doc
            .extensions_used()
            .any(|e| e == "KHR_lights_punctual")
        {
            placed.problems.push(format!("{path} has lights"));
        }
        if model.doc.skins().count() > 0 || model.doc.animations().count() > 0 {
            placed.problems.push(format!(
                "{path} has skins or animations; bake it first (tools/assets/bake.ron)"
            ));
        }
        let prims = match model.prims(&model.rest_locals(), &|n| crate::glob::keeps(nodes, n)) {
            Ok(p) => p,
            Err(e) => {
                placed.problems.push(e);
                continue;
            }
        };
        if prims.is_empty() {
            placed
                .problems
                .push(format!("{path}: no mesh node matches nodes {nodes:?}"));
        }
        let m = scene.mul(&local);
        for p in &prims {
            let what = format!("{path} node {:?}", p.node_name);
            if !p.triangles_mode {
                placed
                    .problems
                    .push(format!("{what} is {}, not triangles", p.mode));
                continue;
            }
            if !p.indexed {
                placed.problems.push(format!("{what} is not indexed"));
            }
            if p.normals.is_none() {
                placed.problems.push(format!("{what} has no NORMAL"));
            }
            let Some(uvs) = &p.uvs else {
                placed.problems.push(format!("{what} has no TEXCOORD_0"));
                continue;
            };
            if p.tex_coord != 0 {
                placed.problems.push(format!(
                    "{what}'s base texture reads TEXCOORD_{}",
                    p.tex_coord
                ));
            }
            let Some(image) = p.image else {
                placed
                    .problems
                    .push(format!("{what} has no base-colour texture"));
                continue;
            };
            match model.image(image) {
                Ok(tex) => {
                    let hash = tex.content_hash();
                    match &placed.texture {
                        None => placed.texture = Some((tex, model.image_name(image), hash)),
                        Some((_, name, h)) if *h != hash => {
                            placed.problems.push(format!(
                                "{what} uses {} but the model already uses {name}; one base texture per model",
                                model.image_name(image)
                            ));
                        }
                        Some(_) => {}
                    }
                }
                Err(e) => placed.problems.push(e),
            }
            for t in 0..p.triangles() {
                let c = p.tri(t);
                placed.tris.push((
                    [
                        m.point(p.positions[c[0]]),
                        m.point(p.positions[c[1]]),
                        m.point(p.positions[c[2]]),
                    ],
                    [uvs[c[0]], uvs[c[1]], uvs[c[2]]],
                ));
            }
        }
    }
    placed
}

/// The nominal height of a unit's Primitive or Body look, metres.
fn look_height(v: &Visual, kind: &UnitKind) -> Option<f32> {
    let r = kind.radius_tiles_x100 as f32 / 100.0;
    match v {
        Visual::Primitive(Primitive::Capsule) => Some(3.4 * r),
        Visual::Primitive(_) => Some(2.0 * r),
        Visual::Body {
            height_tiles_x100,
            lying,
            ..
        } => Some(if *lying {
            2.0 * r
        } else {
            *height_tiles_x100 as f32 / 100.0
        }),
        Visual::Scene { .. } => None,
    }
}

/// The result of a check.
#[derive(Debug, Default)]
pub struct CheckReport {
    /// One line per Scene (`ok …` or `FAIL …`) and notes.
    pub lines: Vec<String>,
    /// Every problem found.
    pub problems: Vec<String>,
    /// Scenes checked.
    pub scenes: usize,
}

impl CheckReport {
    /// The printed report, ending in the summary line.
    pub fn text(&self) -> String {
        let mut out = self.lines.join("\n");
        if !out.is_empty() {
            out.push('\n');
        }
        if self.problems.is_empty() {
            out.push_str(&format!("check: ok {} scene(s)\n", self.scenes));
        } else {
            out.push_str(&format!(
                "check: FAIL {} problem(s) in {} scene(s)\n",
                self.problems.len(),
                self.scenes
            ));
        }
        out
    }
}

struct Measured<'a> {
    field: String,
    path: &'a str,
    usage: Use<'a>,
    team: Option<&'a str>,
    size: Vec3,
    rig: String,
    texture_hash: Option<u64>,
    faces: Vec<(Rect, f32)>,
}

/// Check every Scene of `visuals` against the files under `assets`.
pub fn check_visuals(
    visuals: &Visuals,
    units: &[UnitKind],
    buildings: &[BuildingKind],
    assets: &Path,
    budgets: &Budgets,
) -> CheckReport {
    let mut entries: Vec<(String, Use, &Visual)> = Vec::new();
    for (id, v) in &visuals.units {
        if let Some(kind) = units.iter().find(|k| &k.id == id) {
            entries.push((format!("units[{id:?}]"), Use::Unit(kind), v));
        }
    }
    for (id, v) in &visuals.buildings {
        if let Some(kind) = buildings.iter().find(|k| &k.id == id) {
            entries.push((format!("buildings[{id:?}]"), Use::Building(kind), v));
        }
    }
    for (i, v) in visuals.sites.iter().enumerate() {
        entries.push((format!("sites[{i}]"), Use::Site, v));
    }
    if let Some(r) = &visuals.rubble {
        entries.push(("rubble.visual".into(), Use::Rubble, &r.visual));
    }
    if let Some(f) = &visuals.seat_flag {
        entries.push(("seat_flag".into(), Use::Flag, f));
    }
    for (key, decor) in &visuals.terrain {
        for (i, look) in decor.looks.iter().enumerate() {
            entries.push((
                format!("terrain[{key:?}].looks[{i}].visual"),
                Use::Prop(look.at),
                &look.visual,
            ));
        }
    }

    let mut report = CheckReport::default();
    let mut measured: Vec<Measured> = Vec::new();
    let mut all_files: BTreeSet<PathBuf> = BTreeSet::new();
    for (field, usage, visual) in entries {
        let Some(s) = scene_ref(visual) else { continue };
        report.scenes += 1;
        let placed = place(&s, assets, budgets);
        let mut problems = placed.problems.clone();
        let mut notes = Vec::new();
        all_files.extend(placed.files.keys().cloned());
        let tris = placed.tris.len();
        let cap = match usage {
            Use::Unit(_) => budgets.unit_triangles,
            Use::Building(_) => budgets.building_triangles,
            Use::Site | Use::Rubble => budgets.site_triangles,
            Use::Flag => budgets.flag_triangles,
            Use::Prop(DecorAt::Interior) => budgets.interior_prop_triangles,
            Use::Prop(_) => budgets.edge_prop_triangles,
        };
        if tris > cap {
            problems.push(format!("{tris} triangles; the cap for this use is {cap}"));
        } else if matches!(usage, Use::Unit(_)) && tris > budgets.unit_triangles_target {
            notes.push(format!(
                "{tris} triangles is over the unit target {}",
                budgets.unit_triangles_target
            ));
        }
        let (lo, hi) =
            bounds(placed.tris.iter().flat_map(|(p, _)| p.iter())).unwrap_or(([0.0; 3], [0.0; 3]));
        let size = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
        if tris > 0 {
            if lo[1] < GROUND_Y.0 || lo[1] > GROUND_Y.1 {
                problems.push(format!(
                    "its base is at y={:.3} m, outside [{}, {}]; set y_offset_tiles_x100",
                    lo[1], GROUND_Y.0, GROUND_Y.1
                ));
            }
            let tol = if matches!(usage, Use::Prop(_)) {
                PROP_CENTRE_TOLERANCE
            } else {
                CENTRE_TOLERANCE
            };
            let (cx, cz) = ((lo[0] + hi[0]) / 2.0, (lo[2] + hi[2]) / 2.0);
            if cx.abs() > tol || cz.abs() > tol {
                problems.push(format!(
                    "its x/z centre is ({cx:.3}, {cz:.3}) m, more than {tol} m from the origin"
                ));
            }
        }
        let mut texture_desc = "none".to_string();
        let mut faces = Vec::new();
        let mut team_permille = None;
        if let Some((tex, name, _)) = &placed.texture {
            texture_desc = format!("{}x{}", tex.width, tex.height);
            let base = Path::new(name)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            if tex.width.max(tex.height) > budgets.texture_side {
                if SHIPPED_ATLASES.contains(&base.as_str()) {
                    notes.push(format!("{name} is {texture_desc}, over {} texels, shipped as is (owner decision 5)", budgets.texture_side));
                } else {
                    problems.push(format!(
                        "{name} is {texture_desc}; the limit is {} texels a side",
                        budgets.texture_side
                    ));
                }
            }
            for (p, uv) in &placed.tris {
                faces.push((
                    face_rect(*uv, tex.width, tex.height),
                    triangle_area(p[0], p[1], p[2]),
                ));
            }
            if let Some(key) = s.team {
                match visuals.team_swatches.get(key) {
                    None => problems.push(format!("team {key:?} is not a key of team_swatches")),
                    Some(swatch) => {
                        let rects: Vec<Rect> = swatch
                            .rects
                            .iter()
                            .map(|&(x, y, w, h)| Rect { x, y, w, h })
                            .collect();
                        if let Some(r) = rects
                            .iter()
                            .find(|r| r.x + r.w > tex.width || r.y + r.h > tex.height)
                        {
                            problems.push(format!("team_swatches[{key:?}] rect {r} lies outside the {texture_desc} texture"));
                        }
                        let mask = Mask::from_rects(tex.width, tex.height, &rects);
                        let (mut team_area, mut area, mut straddles) = (0.0f64, 0.0f64, 0);
                        for (r, a) in &faces {
                            area += f64::from(*a);
                            match mask.cover(r) {
                                Cover::Inside => team_area += f64::from(*a),
                                Cover::Straddles => straddles += 1,
                                Cover::Outside => {}
                            }
                        }
                        let permille = if area > 0.0 {
                            (1000.0 * team_area / area).round() as u32
                        } else {
                            0
                        };
                        team_permille = Some(permille);
                        let floor = match usage {
                            Use::Unit(k) if k.id == "supply_wain" => budgets.wain_team_permille,
                            Use::Unit(_) => budgets.unit_team_permille,
                            Use::Building(_) => budgets.building_team_permille,
                            Use::Flag => budgets.flag_team_permille,
                            _ => 0,
                        };
                        if permille < floor {
                            problems
                                .push(format!("team area {permille}‰ is under the {floor}‰ floor"));
                        }
                        if straddles > 0 {
                            problems.push(format!(
                                "{straddles} face(s) straddle a team rect edge, so the repaint would split them (reserved-rect rule)"
                            ));
                        }
                    }
                }
            }
        }
        let mut suggest = String::new();
        let unscale = 100.0 / s.scale_x100.max(1) as f32;
        match usage {
            Use::Building(k) => {
                let side = k.width_tiles.min(k.height_tiles) as f32 * FOOTPRINT_FILL;
                let extent = size[0].max(size[2]) * unscale;
                if extent > 0.0 {
                    suggest = format!(
                        " suggest scale_x100={} ({side:.2} m footprint)",
                        (100.0 * side / extent).round()
                    );
                }
            }
            Use::Site | Use::Rubble => {
                let extent = size[0].max(size[2]) * unscale;
                if extent > 0.0 {
                    suggest = format!(
                        " suggest scale_x100={} ({FOOTPRINT_FILL} m per footprint tile)",
                        (100.0 * FOOTPRINT_FILL / extent).round()
                    );
                }
            }
            Use::Unit(k) => {
                if let Some(h) = s.fallback.and_then(|f| look_height(f, k))
                    && size[1] > 0.0
                {
                    suggest = format!(
                        " suggest scale_x100={} (fallback height {h:.2} m)",
                        (100.0 * h / (size[1] * unscale)).round()
                    );
                }
            }
            _ => {}
        }
        let bytes: u64 = placed.files.values().sum();
        let team_desc = team_permille.map_or("-".to_string(), |p| format!("{p}‰"));
        let measures = format!(
            "tris={tris} bytes={bytes} top={:.2} size={:.2}x{:.2}x{:.2} team={team_desc} texture={texture_desc}{suggest}",
            hi[1], size[0], size[1], size[2]
        );
        if problems.is_empty() {
            report
                .lines
                .push(format!("ok {field} {} {measures}", s.path));
        } else {
            for p in &problems {
                report.lines.push(format!("FAIL {field} {}: {p}", s.path));
                report.problems.push(format!("{field}: {p}"));
            }
            if tris > 0 {
                report.lines.push(format!("  measured {field}: {measures}"));
            }
        }
        for n in notes {
            report.lines.push(format!("  note {field}: {n}"));
        }
        measured.push(Measured {
            field,
            path: s.path,
            usage,
            team: s.team,
            size,
            rig: placed.rig,
            texture_hash: placed.texture.as_ref().map(|t| t.2),
            faces,
        });
    }

    // Reserved rects: a neutral model must not sample the team rects of a
    // swatch that team models on the same texture use, or it would show a
    // team-looking colour.
    let mut team_rects: BTreeMap<u64, Vec<(String, Rect)>> = BTreeMap::new();
    for m in &measured {
        if let (Some(key), Some(hash)) = (m.team, m.texture_hash)
            && let Some(swatch) = visuals.team_swatches.get(key)
        {
            let list = team_rects.entry(hash).or_default();
            for &(x, y, w, h) in &swatch.rects {
                let r = Rect { x, y, w, h };
                if !list.iter().any(|(k, q)| k == key && *q == r) {
                    list.push((key.to_string(), r));
                }
            }
        }
    }
    for m in &measured {
        let (None, Some(hash)) = (m.team, m.texture_hash) else {
            continue;
        };
        let Some(rects) = team_rects.get(&hash) else {
            continue;
        };
        let hits = m
            .faces
            .iter()
            .filter(|(r, _)| rects.iter().any(|(_, q)| q.intersects(r)))
            .count();
        if hits > 0 {
            let msg = format!(
                "{hits} face(s) of this neutral model sample team rects of the same texture (reserved-rect rule)"
            );
            report
                .lines
                .push(format!("FAIL {} {}: {msg}", m.field, m.path));
            report.problems.push(format!("{}: {msg}", m.field));
        }
    }

    // The 1.4x silhouette rule over placed unit bounds within a rig group.
    for (i, a) in measured.iter().enumerate() {
        for b in &measured[i + 1..] {
            let (Use::Unit(_), Use::Unit(_)) = (a.usage, b.usage) else {
                continue;
            };
            if a.rig != b.rig {
                continue;
            }
            let apart = (0..3).any(|c| {
                let (lo, hi) = (a.size[c].min(b.size[c]), a.size[c].max(b.size[c]));
                hi >= lo * 1.4
            });
            if !apart {
                let msg = format!(
                    "same rig as {} and less than 1.4x apart on every axis ({:.2}x{:.2}x{:.2} vs {:.2}x{:.2}x{:.2} m); each kind needs its own silhouette",
                    a.field, b.size[0], b.size[1], b.size[2], a.size[0], a.size[1], a.size[2]
                );
                report
                    .lines
                    .push(format!("FAIL {} {}: {msg}", b.field, b.path));
                report.problems.push(format!("{}: {msg}", b.field));
            }
        }
    }
    report
        .lines
        .push(format!("files: {} distinct", all_files.len()));
    report
}

/// Load the rules under `data` and check `visuals.ron` against `assets`.
pub fn check_dirs(data: &Path, assets: &Path) -> Result<CheckReport, String> {
    let rules = rules::Rules::load(data).map_err(|e| e.to_string())?;
    Ok(check_visuals(
        &rules.visuals,
        &rules.units,
        &rules.buildings,
        assets,
        &Budgets::default(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{TempDir, box_gltf, repo_root};

    #[test]
    fn check_flags_budgets_ground_contact_team_area_and_silhouettes() {
        let dir = TempDir::new("check");
        let assets = dir.path().join("assets");
        std::fs::create_dir_all(assets.join("m")).unwrap();
        // Boxes: size, base y, and whether the four sides sample the team
        // texels (4..8, 0..4 of the test atlas) or a neutral one.
        box_gltf(&assets.join("m"), "unit", [0.5, 1.5, 0.5], 0.0, true);
        box_gltf(&assets.join("m"), "floating", [0.5, 1.5, 0.5], 0.5, true);
        box_gltf(&assets.join("m"), "house", [2.0, 1.5, 2.0], 0.0, true);
        box_gltf(&assets.join("m"), "rock", [0.5, 0.4, 0.5], 0.0, false);
        let rules = rules::Rules::load(repo_root().join("data")).unwrap();
        let ron = |units: &str, buildings: &str| {
            format!(
                r#"(units: {{ {units} }}, buildings: {{ {buildings} }},
                   team_swatches: {{ "t": (rects: [(4, 0, 4, 4)], desaturate_permille: 0) }},
                   terrain: {{ "mountain": (interior_permille: 1000, edge_permille: 0, jitter_x100: 0, scale_jitter_pct: 0,
                      looks: [(visual: Scene(path: "m/rock.gltf", scale_x100: 100, y_offset_tiles_x100: 0, yaw_deg: 0), at: Edge, weight: 1)]) }})"#
            )
        };
        let unit = |id: &str, file: &str, scale: u32| {
            format!(
                r#""{id}": Scene(path: "m/{file}.gltf", scale_x100: {scale}, y_offset_tiles_x100: 0, yaw_deg: 0, team: Some("t"), fallback: Some(Primitive(Capsule))),"#
            )
        };
        let house = r#""croft": Scene(path: "m/house.gltf", scale_x100: 100, y_offset_tiles_x100: 0, yaw_deg: 0, team: Some("t")),"#;
        let run = |text: String, budgets: &Budgets| {
            let v: Visuals = ron::from_str(&text).unwrap();
            check_visuals(&v, &rules.units, &rules.buildings, &assets, budgets)
        };
        let budgets = Budgets::default();

        // Two kinds on one rig 1.5x apart in height, a house and a rock: ok.
        let good = run(
            ron(
                &(unit("yeoman", "unit", 100) + &unit("scribe", "unit", 150)),
                house,
            ),
            &budgets,
        );
        assert!(good.problems.is_empty(), "{}", good.text());
        assert_eq!(good.scenes, 4);
        let text = good.text();
        assert!(
            text.contains("ok units[\"yeoman\"] m/unit.gltf tris=12"),
            "{text}"
        );
        // The four sides sample the team rect, the top and bottom do not:
        // 3.0 of 3.5 m² on the unit, 12 of 20 m² on the house.
        assert!(text.contains("team=857‰"), "{text}");
        assert!(text.contains("team=600‰"), "{text}");
        // The 2 m house fills FOOTPRINT_FILL of the Croft's footprint side,
        // read from the rules, so a data-only footprint change does not
        // fail this test.
        let croft = rules.buildings.iter().find(|k| k.id == "croft").unwrap();
        let side = croft.width_tiles.min(croft.height_tiles) as f32 * FOOTPRINT_FILL;
        let want = format!(
            "suggest scale_x100={} ({side:.2} m footprint)",
            (100.0 * side / 2.0).round()
        );
        assert!(text.contains(&want), "{want}\n{text}");
        // The yeoman's capsule fallback is 3.4 of its radii tall.
        let yeoman = rules.units.iter().find(|k| k.id == "yeoman").unwrap();
        let height = 3.4 * yeoman.radius_tiles_x100 as f32 / 100.0;
        let want = format!("(fallback height {height:.2} m)");
        assert!(text.contains(&want), "{want}\n{text}");
        assert!(text.ends_with("check: ok 4 scene(s)\n"), "{text}");

        // Same rig, same size: the silhouette rule fires.
        let clash = run(
            ron(
                &(unit("yeoman", "unit", 100) + &unit("scribe", "unit", 110)),
                house,
            ),
            &budgets,
        );
        assert!(
            clash.text().contains("FAIL units[\"yeoman\"]")
                || clash.text().contains("FAIL units[\"scribe\"]"),
            "{}",
            clash.text()
        );
        assert!(
            clash.text().contains("less than 1.4x apart"),
            "{}",
            clash.text()
        );

        // Floating off the ground.
        let float = run(ron(&unit("yeoman", "floating", 100), house), &budgets);
        assert!(
            float.text().contains("its base is at y=0.500 m"),
            "{}",
            float.text()
        );

        // Over a triangle cap, and under the team floor.
        let tight = Budgets {
            edge_prop_triangles: 6,
            building_team_permille: 700,
            ..Budgets::default()
        };
        let over = run(ron(&unit("yeoman", "unit", 100), house), &tight);
        let text = over.text();
        assert!(
            text.contains("12 triangles; the cap for this use is 6"),
            "{text}"
        );
        assert!(
            text.contains("team area 600‰ is under the 700‰ floor"),
            "{text}"
        );
        assert!(text.contains("check: FAIL 2 problem(s)"), "{text}");

        // A rect that cuts faces in half breaks the reserved-rect rule.
        let split = run(
            ron(&unit("yeoman", "unit", 100), house).replace("(4, 0, 4, 4)", "(4, 0, 2, 4)"),
            &budgets,
        );
        assert!(
            split.text().contains("straddle a team rect edge"),
            "{}",
            split.text()
        );

        // A neutral model sampling team rects of its texture fails too.
        box_gltf(&assets.join("m"), "rock", [0.5, 0.4, 0.5], 0.0, true);
        let reserved = run(ron(&unit("yeoman", "unit", 100), house), &budgets);
        assert!(
            reserved
                .text()
                .contains("this neutral model sample team rects"),
            "{}",
            reserved.text()
        );

        // A missing file is a problem, not a panic.
        let missing = run(ron(&unit("yeoman", "nope", 100), ""), &budgets);
        assert!(
            missing
                .text()
                .contains("FAIL units[\"yeoman\"] m/nope.gltf"),
            "{}",
            missing.text()
        );
    }
}
