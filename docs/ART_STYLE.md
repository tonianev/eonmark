# Art style

This document is the visual contract for Eonmark. It fixes the palette, material and lighting settings, the readability rule, the HUD look, the primitives-first approach and the rule for swapping in real models, the glTF import rules, the allowed asset sources and licences, and the screenshot checklist that every art PR is judged against. The look is calm, matte, flat-shaded low-poly 3D on a flat ground plane. The contract applies from M0 (the ground plane) through M7 (the look pass); the hex values are starting suggestions and are finalized at the M7 screenshot sign-off. Licensing rules are in [adr/0001-license.md](adr/0001-license.md).

## The look in one paragraph

A board game seen from a fixed-pitch camera. Flat colours, soft shadows, no shine, no glow. Units and buildings are simple silhouettes in team colour. Owned land is tinted a little and bounded by a soft line. The HUD is numbers and borders on matte panels. Nothing flashes, pulses or blooms. Readability at the far zoom beats detail at the near zoom.

## Palette

At most 12 base hues plus 2 team colours. Keep saturation low and value in the middle; nothing pure white, nothing pure black. These are suggestions in a matte range; the final values are measured at M7 and recorded here.

| Role | Suggested hex | Notes |
|---|---|---|
| Grass | `#7A9B5C` | Ground plane default |
| Forest floor | `#5C7A4A` | Forest tiles; tree clusters sit on it |
| Rock | `#8C8578` | Mountain tiles |
| Water | `#5B7C8F` | The impassable strip; flat, no animation |
| Soil and paths | `#A08C6A` | Build footprints |
| Timber | `#8B6B4A` | Lumber Yard, Supply Wain |
| Stone | `#B5AFA3` | Towns, Watchtower, Engine Yard |
| Thatch | `#B08D57` | Croft roofs |
| Ore | `#6E6A72` | Ore Pit, Mangonel frame |
| Grain gold | `#C9A961` | Grain resource icon and node |
| Lore ink | `#4F5B7A` | Scriptorium accent, Lore icon |
| Neutral and fog | `#3A3A3F` | Neutral territory, fog overlay colour, panel base |

| Team | Suggested hex | Notes |
|---|---|---|
| Blue | `#3F6FA8` | Player 1 default; match to the KayKit blue atlas swatch at M7 |
| Red | `#B04A3F` | Player 2 default; match to the KayKit red atlas swatch at M7 |

HUD accents are drawn from the same table plus two states:

| HUD role | Suggested hex |
|---|---|
| Text | `#ECE9E2` |
| Panel | `#26262A` at 92% opacity |
| Disabled | `#6B6B70` |
| Yield Cap amber | `#D29B3B` |
| Affordable or valid placement | `#6FA06A` |
| Unaffordable or invalid placement | `#B04A3F` |

## Material settings

One `StandardMaterial` per kind and team, shared by every instance of that kind.

| Property | Value |
|---|---|
| `perceptual_roughness` | 0.95 |
| `reflectance` | 0.2 |
| `metallic` | 0.0 |
| `emissive` | black; never used for gameplay signalling |
| Alpha | opaque, except HUD panels; territory is drawn inside the opaque ground material by `FieldExt`, not as a separate overlay |

## Lighting and post-processing

| Setting | Value |
|---|---|
| Lights | One `DirectionalLight` with soft cascaded shadows, plus `AmbientLight` |
| Tonemapping | `Tonemapping::TonyMcMapface` |
| Anti-aliasing | `Msaa::Sample4` |
| Bloom | none |
| Emissive glow | none |
| SSAO | optional, low strength only; off by default |
| Other post-processing | none |
| Present mode | `PresentMode::AutoVsync` (Mailbox is unsupported on Metal) |

## Readability at 40 m

The camera zooms between 15 m and 60 m at a pitch of about 55 degrees. The acceptance distance is 40 m: at that zoom, in a 1280x720 screenshot, every unit kind must be identifiable by silhouette and every team by colour. Shape carries identity, colour carries ownership. If a kind is only distinguishable up close, change its silhouette, not its texture.

Primitive silhouettes (M2 to M7):

| Kind | Primitive | Notes |
|---|---|---|
| Units | One shape per kind (table below) | Since M4b each kind has its own silhouette and height, low-poly and flat-shaded; M2 drew every unit as a smooth `Capsule` |
| Buildings | KayKit Hexagon models since art slice 3 (the table under "Building looks"); `Primitive(Cuboid)`, the M3b team-coloured box over the footprint, is every building's fallback | Team colour on roofs and trim, plus a team banner beside each building |
| Town | KayKit castle; a Seat flies the large Seat flag on its keep | The Seat flag is 1.8 m tall, at least 1.4 times any banner (the check holds it) |
| Construction site | KayKit construction stages A, B and C, one per third of the build, neutral | Scaled by the footprint side; the health bar shows the progress |
| Rubble | KayKit `building_destroyed`, neutral, for 30 s | Where a building or site was destroyed; gone early when a new building takes the footprint |
| Terrain props | KayKit trees on forest, mountains and rocks on mountain, lilies and plants on water (`terrain` in `visuals.ron`) | Never on grass; a look only where its footprint stays on its own terrain; nothing over 1.6 m on a tile whose north neighbour is passable, so props never hide units |
| Selection ring | Flat annulus mesh, unlit (retained gizmo lines until the art pass) | Separate entities keyed by id, never children of moving units; one shared mesh and material for every ring, so any number of rings is one draw call |
| Rally marker, move marker | Retained gizmo lines | Same rule |
| Attrition indicator (M5a) | Flat 4-segment annulus mesh, unlit (retained gizmo lines until the art pass) | Same rule; a steady Yield Cap amber (#D29B3B) diamond under a unit bleeding on enemy land, one shared mesh and material for every diamond; nothing pulses |
| Supply ring (M5a) | Retained gizmo lines | Same rule; a Timber ring around a selected Supply Wain; nothing pulses |
| Annexation timer (M5a) | Immediate gizmo lines, 3 px | A #3A3A3F track circle just outside a contested Town and the attacker's team-colour arc filling clockwise from north |
| Health bar | Flat quad entity | Same rule; unlit, a team-colour fill on a #3A3A3F track, no green-to-red, shown only while the target is damaged or selected (M4b, `health_bars.rs`) |

Unit looks (M4b, `data/visuals.ron`), one silhouette per kind. Width is the kind's collision diameter from `units.ron`; each mesh has 10 facets around (a 6-latitude capsule, an icosphere of 80 faces) and flat face normals, at most 600 vertices, one mesh per kind and one material per kind and team:

| Kind | Look | Silhouette |
|---|---|---|
| Yeoman | `Primitive(Capsule)` | Upright pill, 0.7 m wide, 1.19 m tall |
| Scribe | `Primitive(Cylinder)` | Drum, 0.7 x 0.7 m |
| Skirmisher | `Body(shape: Cone, height_tiles_x100: 150)` | Spike, 1.5 m |
| Shieldbearer | `Body(shape: Cuboid, height_tiles_x100: 130)` | Block, 0.8 x 1.3 x 0.8 m |
| Bowman | `Body(shape: Cylinder, height_tiles_x100: 190)` | Tall post, 1.9 m |
| Outrider | `Body(shape: Capsule, height_tiles_x100: 180, lying: true)` | Pill lying along its facing, 1.8 m long (twice its collision diameter; see the crowd check below), 0.9 m tall |
| Mangonel | `Primitive(Sphere)` | Ball, 1.0 m |
| Supply Wain | `Body(shape: Cuboid, height_tiles_x100: 60)` | Low box, 1.0 x 0.6 x 1.0 m |

Two kinds share a shape only when they differ by at least 1.4 times on some axis (the Scribe's drum and the Bowman's post, the Shieldbearer's block and the Wain's low box, the Yeoman's upright pill and the Outrider's lying one); `present::tests::every_unit_kind_has_its_own_silhouette` checks it. The 40 m view is `--scenario unit_looks`: one unit of each kind per team in two rows 15 m apart, framed at 40 m unless `--camera` is given (`target/ci/eonmark --background --scenario unit_looks --exit-after-seconds 40 --screenshot-at 100:looks.png`; CI's render smoke test renders it on every PR). The owner chose single-shape silhouettes over multi-part looks on 2026-10-22 (ROADMAP M4b, Notes, decision 11); multi-part looks wait for the art pass. The looks themselves are signed off on the 40 m screenshot, and that sign-off is still pending: the owner's run on the Mac, with the P3 and sRGB check and the checklist below.

The sign-off also looks at a crowd, because the lineup stands every unit alone 4 m from the next and cannot show how the looks pack in a fight: `target/ci/eonmark --background --scenario enemy_outpost --camera 100,32,40 --exit-after-seconds 60 --screenshot-at 250:crowd.png` (`just render-smoke out enemy_outpost 100,32,40 250` under Xvfb) frames the 200 vs 200 prologue at 40 m mid-fight. Every look is exactly its kind's collision diameter across except the Outrider's, whose lying pill is 1.8 m long against a 0.9 m diameter. Melee range is measured edge to edge (0.25 m for the Outrider) and the sim keeps units apart only by a soft push, so two Outriders holding on each other stand at most 1.15 m apart, centre to centre, while each pill reaches 0.9 m ahead of its centre: head-on they overlap by at least 0.65 m, and in a packed melee the pills visibly pass through each other. Shortening the pill is a data-only change, `height_tiles_x100` of `"outrider"` in `data/visuals.ron` (120 keeps a lying pill, cuts the head-on overlap to 0.05 m and still differs from the Yeoman's upright pill by 1.7 times along its length, and `data-check`'s 1.4x silhouette check (`rules::Visuals::validate`; at M4b it was `present::tests::every_unit_kind_has_its_own_silhouette`, which since the art pass measures the frozen M4b looks) passes with it, but reads closer to the Mangonel's 1.0 m ball); since `rules_version` 25 `visuals.ron` is not in `rules_hash` ([ADR 0003](adr/0003-visuals-outside-rules-hash.md)), so it needs no `rules_version` bump and no `just rerecord` (at M4b it needed both). The owner decides that trade at the sign-off.

Motion is procedural (bob, swing, scale pulse), not animation clips, in v0.1.

## Territory and fog

| Element | Value |
|---|---|
| Owned-land tint | about 12% team colour over the ground |
| Border | a 1-tile soft isoline where ownership changes |
| Neutral or tied tiles | no tint, except within half a tile of owned land, where the owner's tint fades from about half of the 12% at the edge to nothing at the tile centre, under the border line |
| Fog, explored | ground and buildings darkened to 0.45 |
| Fog, unexplored | 0.0 (hidden) |
| Supply coverage | a thin Timber (#8B6B4A) ring of radius 14.5 m around a selected Supply Wain (M5a, `markers.rs`); the subtle hatching from the same field texture's A channel waits for M7 with fog (M5a owner decision 10) |

The ground's material is the `FieldExt` extended material (`crates/game/src/field.rs`, shader `field.wgsl` embedded in the binary) reading an RGBA8 field texture with one texel per tile, 128x128 on the current maps (R owner, G strength, B fog, A supply). Each owner's membership is interpolated between tile centres with smoothstep weights; the tint follows it and the border is the isoline where it crosses one half, fading to nothing at the tile centres on either side, so the line follows the buildable tiles with rounded corners. M3b drew a vertex-coloured version first and replaced it within the milestone. The shader is mandatory by M7 because fog rides the same texture.

## HUD

`bevy_ui` only in shipped builds. Inter (variable weight) for all text. Kenney UI Pack 9-slice panels through `NodeImageMode::Sliced`. Matte panels, thin borders, no drop shadows, no glow. The top resource bar shows current stockpile and per-30-s rate per resource and turns the readout amber while production is being discarded at the Yield Cap. Disabled command-card buttons are greyed with a tooltip, not hidden. Layout-only root nodes are `Pickable::IGNORE` so empty HUD space passes clicks to the world. `bevy_egui` appears only behind the `dev` feature.

## P3 and sRGB check

wgpu on a wide-gamut display can render colours more saturated than authored. Every palette or art change is judged twice: once on the development Mac's P3 display, once as an sRGB screenshot opened in Preview with the display profile set to sRGB. Both must look matte. If a colour reads as neon in either, lower its saturation until both agree.

## Primitives first, models by data

Every unit kind is drawn through one `Visual` value from `data/visuals.ron`, keyed by its `units.ron` id, and since the art pass every building kind too, keyed by its `buildings.ron` id (`Primitive(Cuboid)` is the M3b box `crates/game/src/buildings.rs` draws; slice 1 mapped all eight kinds to it, slice 3 maps them to KayKit models, "Building looks" below):

```ron
"yeoman": Primitive(Capsule),
"skirmisher": Body(shape: Cone, height_tiles_x100: 150),
"outrider": Body(shape: Capsule, height_tiles_x100: 180, lying: true),
"mangonel": Scene(path: "models/mangonel.glb", scale_x100: 100, y_offset_tiles_x100: 0, yaw_deg: 0, fallback: Some(Primitive(Sphere))),
```

`Visual::Primitive(shape)` draws a team-coloured primitive sized from the kind's radius (`Capsule`, `Cuboid`, `Cylinder`, `Sphere` or `Cone`). `Visual::Body(shape, height_tiles_x100, lying)` (M4b) draws the same primitive two radii wide with its own height in hundredths of a metre (20 to 400; never a `Sphere`); `lying: true` lays a `Capsule` or `Cylinder` along the unit's facing, that long and two radii tall. `Visual::Scene(path, scale_x100, y_offset_tiles_x100, yaw_deg)` loads a glTF scene; the scale and offset are in hundredths and the yaw in whole degrees, so the file stays float-free. Swapping a cube for a model is a RON edit, never a Rust change, and since `rules_version` 25 it needs no `rules_version` bump or re-record either: `data-check` validates `visuals.ron`, but `rules_hash` leaves it out ([ADR 0003](adr/0003-visuals-outside-rules-hash.md)). M0 to M6 ship primitives only; M7 maps every kind to CC0 models and keeps the primitive look as the fallback. Anything drawn above a unit (the M4b health bar) takes its height from `present::look_top_y`, computed from the `Visual`, so the art pass changes that one function. Since art slice 3, `crates/game/src/models.rs` loads every Scene the game draws as a model once, merges each into one placed mesh, and holds the sim's first tick until every model is ready or has failed (the load gate in `sim_driver::match_running`), so nothing appears as a box and then swaps; a building whose model fails draws the box with one `error!` (the render smoke fails on it). Units still draw their `Primitive` or `Body` looks until slice 4, and a unit `Scene` draws its `fallback`, else `Primitive(Capsule)` (`present::FALLBACK_VISUAL`). Bars and attack markers over a building take its top from `buildings::building_top_y` (the model's top, a site's stage times its scale, or the box). `data-check` holds the 1.4x silhouette rule over the live file's `Primitive` and `Body` looks (a Scene by its fallback), while the `present.rs` look tests measure the M4b looks frozen as `M4B_LOOKS`, so a look edit never fails a Rust test.

## glTF import rules

These replace the Blender-export rules of M0; they describe the files the art pass vendors and bakes, and `inspect_gltf check` enforces every row it can measure.

| Rule | Value |
|---|---|
| Format | glTF 2.0: a pack's `.gltf` + `.bin` + one PNG as shipped, or a baked `.glb` (art slice 4) |
| Up and units | +Y up, 1 unit = 1 m; any other size is set by `scale_x100`; `convert_coordinates` stays off |
| Facing | units face +X at rest after `yaw_deg`; buildings, sites and rubble put their front toward +Z, toward the camera; props have no facing. KayKit Hexagon buildings already face +Z at yaw 0 (their doors, the mine's entrance and the forge face +Z; screenshots at four yaws, art slice 3), so every building uses `yaw_deg: 0`. The KayKit flag's cloth faces ±X, so the banner and the Seat flag turn it with `yaw_deg: -90` to face the camera |
| Origin | not required in the file. The placed model's bounds must have their base within -0.02..0.05 m and their x/z centre within 0.10 m of the origin (0.5 m for props), set with `y_offset_tiles_x100`, `x_offset_tiles_x100` and `z_offset_tiles_x100` (art slice 3), applied after the yaw and the scale |
| Contents | indexed triangles with `POSITION`, `NORMAL` and `TEXCOORD_0` on every kept primitive; one base-colour texture per model; no cameras or lights; skins and animations only in bake inputs |
| Material | the file's material gives only its base colour and texture; the game builds `StandardMaterial { perceptual_roughness: 0.95, reflectance: 0.2, metallic: 0 }`, no emissive |
| Sampler | nearest, forced with `override_sampler` |
| Texture | one palette or gradient texture at most 256 px on a side, except a pack's shared palette atlas, which ships as is (KayKit Hexagon's 1024 px `hexagons_medieval.png`, art-pass owner decision 5); baked characters are downsampled only when every face keeps its colour |
| Naming | the pack's own names under `models/<pack>/...`; baked files `models/kaykit_adventurers/<kind>.glb` |
| Inspection | `cargo run -p inspect_gltf --release -- inspect <file>` prints scenes, the node tree with transforms, meshes and primitives, images, skins, clips, rest bounds and a T-pose hint; `swatches`, `palette`, `bake` and `check` are described below |

Node globs in a Scene's `nodes` match a node's own name only (`rules::visuals::glob_match`, shared by the game's merge and the tool), so keeping a node keeps its mesh, not its children.

## Building looks (art slice 3)

Each building kind is one line of `data/visuals.ron`: a KayKit Hexagon model (blue variant, recoloured at run time), scaled to fill 0.95 of its footprint side (`inspect_gltf check` prints the suggestion), lifted 1 cm, centred where the file is off-centre, at yaw 0, with the owner's `kaykit_hexagon` swatch and a team banner beside it. Team shares are measured by `inspect_gltf check` on the model plus its banner at its data scale (owner decision 12 of the art pass, 2026-10-28):

| Kind | Model | Scale | Offsets (cm) | Placed size (m) | Banner (height) | Team share: model / with banner |
|---|---|---|---|---|---|---|
| Town | `building_castle_blue` | 126 | - | 2.49 x 5.01 x 2.84 | 1.25 m at the front-right; the Seat flag 1.80 m on the keep | 67‰ / 74‰, held for the owner |
| Croft | `building_home_A_blue` | 223 | - | 1.77 x 2.07 x 1.90 | 0.69 m | 258‰ / 263‰ |
| Lumber Yard | `building_lumbermill_blue` | 139 | z -19 | 1.90 x 2.37 x 1.65 | 1.53 m | 64‰ / 102‰ |
| Ore Pit | `building_mine_blue` | 99 | - | 1.59 x 1.13 x 1.90 | 0.97 m | 5‰ / 25‰, held for the owner |
| Scriptorium | `building_church_blue` | 247 | - | 2.54 x 4.06 x 2.85 | 0.83 m | 280‰ / 283‰ |
| Muster Hall | `building_barracks_blue` | 182 | z -12 | 2.62 x 2.98 x 2.85 | 2.16 m | 84‰ / 102‰ |
| Watchtower | `building_tower_A_blue` | 82 | - | 0.81 x 1.80 x 0.95 | 0.83 m on its roof | 81‰ / 109‰ |
| Engine Yard | `building_blacksmith_blue` | 221 | x -3, z -14 | 2.85 x 2.18 x 2.75 | 0.97 m | 91‰ / 102‰ |

The banner is the KayKit flag (48 triangles, 457‰ team area), one shared mesh with one material per team (the same team material as the buildings on the atlas), so all banners add about two draw bins; banners and the Seat flag cast no shadow. Its `banner_at` mount places the flag's centre from the footprint centre on the ground, and its pole stands 0.11 m x scale to the left of that point. A banner may not stand taller than its building, and the Seat flag must stand at least 1.4 times taller than the Town's banner (both are `check` rules). The Ore Pit cannot reach 100‰ with a banner no taller than the mine (1.13 m gives 31‰), and the Town would need a banner about 2.8 m tall with a cloth as wide as the castle; both keep a readable banner and are listed in `inspect_gltf::check::PENDING_TEAM_FLOOR`, which `check` prints as `PENDING` lines until the owner decides (ROADMAP "Art pass (pre-stream)", Notes). A site carries no banner (the stages are neutral scaffolding; its builders show whose it is), and rubble never does.

Sites draw the KayKit construction stages A, B and C (neutral, scaled 91, 84 and 82 so one tile of footprint is 0.95 m, then scaled by the footprint side), rubble `building_destroyed` (scale 62, 30 s). Terrain props: forest interiors take the four KayKit tree patches at 130% (1.2-1.7 m), forest edges the single trees (1.2-1.3 m); mountain interiors the three mountains at 110% (1.6-2.0 m) and, two rings deep in a range, mountains B and C at 190% (2.9-3.4 m); mountain edges single rocks; water 6% lilies and plants, without shadows.

### Swapping a building's look (walkthrough)

Swapping a look is one line of `data/visuals.ron` and no Rust. Here the Croft goes back to the M3b box:

```bash
cp data/visuals.ron /tmp/visuals.ron.bak
sed -i 's|^    "croft": Scene(.*|    "croft": Primitive(Cuboid),|' data/visuals.ron
cargo run -q -p sim-cli -- data-check data      # OK ... visuals=units:8 buildings:8 terrain:3 models=<n>
cargo nextest run -p game --cargo-profile ci    # green: no test reads the live building looks
git diff --stat                                 # data/visuals.ron only
just verify                                     # every fixture OK: visuals.ron is not in rules_hash
cp /tmp/visuals.ron.bak data/visuals.ron
```

The Croft's line carries its banner mount, so the box (which is team-coloured all over) has none. The swap back to a model is the same edit the other way; `inspect_gltf check` then prints the new model's measurements. The literal walkthrough of the ROADMAP's M7 line (the Yeoman back to `Primitive(Capsule)`) follows with the unit models in art slice 4.

## Allowed asset sources

| Source | Licence | Use |
|---|---|---|
| Kenney (UI Pack, Game Icons, Nature Kit, and other kits) | CC0-1.0 | UI 9-slice, icons, trees, props |
| KayKit (Medieval Builder, Medieval Hexagon, Adventurers) | CC0-1.0 | Buildings with team colours, units |
| Freesound, filtered to CC0 | CC0-1.0 | About 10 OGG clips (M7) |
| Inter | OFL-1.1 | The one UI font; licence text vendored beside the `.ttf` |
| Any other source | CC0-1.0, CC-BY-4.0 or OFL-1.1 only | Each with an `assets/ATTRIBUTION.md` row |

Excluded: Quaternius (proprietary licence since 2026-08-28), CC-BY-SA, CC-BY-NC, GPL art, Sampling+, and any material from the inspiration's fan wiki.

Each vendored file gets an `ATTRIBUTION.md` row: asset, author, source URL, licence, modifications, download date. In-house files are marked `original: yes` and are CC0. `scripts/check_assets.sh` fails CI if a non-allowlisted file lacks a row, exceeds 2 MB, or the total exceeds 300 MB, and `scripts/check_assets.sh --self-test` proves that an unattributed file fails with its path. No Git LFS.

### Where the art pass's files come from

The pre-stream art pass ([ROADMAP.md](ROADMAP.md), "Art pass (pre-stream)") vendors KayKit Medieval Hexagon 1.0 (buildings, construction stages, rubble, flag, cart props and nature) and Inter 4.1, and bakes its units from KayKit Adventurers 1.0. Everything is fetched from `raw.githubusercontent.com` (the `KayKit-Game-Assets` organisation, which the owner confirmed as the source to use, and `rsms/inter` at tag `v4.1`), the only host that serves these files to the agent's VM: github.com pages, its git and API hosts, kenney.nl, itch.io and opengameart.org refuse, and KayKit Medieval Builder and Forest Nature are itch-only (404 on raw). So:

- **Pins are sha256, not commits.** `tools/assets/sources.tsv` lists every remote file with its URL, sha256, licence, author and per-file GitHub page; `just fetch-assets` (`scripts/fetch_assets.sh`, maintainer-only, never in CI) fetches and checks the licence texts first (KayKit's `LICENSE.txt` must name `publicdomain/zero/1.0`, Inter's must be the SIL Open Font License 1.1, or it stops), then every other file into a download cache, verifies each pin and copies the vendored ones into `assets/` byte for byte. The vendored copies are the source of truth; a second run changes nothing. Offline, `scripts/check_assets.sh` (CI's check job and `just ci`) hashes every vendored file against its pin, so a file whose `ATTRIBUTION.md` row says Modifications `none` stays byte for byte as shipped: an edit fails CI naming the file until its row and its Modifications text record it. `.editorconfig` turns off whitespace trimming under `assets/`, since KayKit's licence texts end lines in tabs.
- **Tool-only inputs stay in the cache.** The red KayKit variants exist only for `inspect_gltf swatches`, and the 3.6 MB rigged character files only for `inspect_gltf bake` (`just art-bake`, `tools/assets/bake.ron`); neither is committed.
- **Licence texts are vendored with their own rows** (`models/kaykit_hexagon/LICENSE.txt`, `models/kaykit_adventurers/LICENSE.txt`, `fonts/OFL-Inter.txt`), and `inspect_gltf`'s `attribution_rows_match_the_sources` test holds `ATTRIBUTION.md` to `sources.tsv` and `bake.ron`.
- **Kenney is deferred to M7** (UI Pack panels, Game Icons): its site is unreachable from the agent's VM, so the owner fetches it on the Mac then. The KenneyNL starter kits on raw GitHub are not used: no per-asset CC0 statement has been read there.

### Budgets (measured in art slice 2)

`inspect_gltf check data assets` (CI's check job) holds every Scene in `data/visuals.ron` to these limits. The file, texture and triangle limits were set from `inspect_gltf inspect` on every chosen source, each a little above the heaviest file it covers. The team-area floors were not measured: they are the plan's (its section 3.4); since art slice 3 a building's share counts its banner (below the table). Each `ok` line also prints `facing=`, the horizontal side with the most surface and its share (a hint for the rest-facing rule: units face +X after `yaw_deg`, buildings, sites and rubble +Z toward the camera; props print `-`); a share near 25% means no side dominates. It does not find a door: on the KayKit buildings it reads 26-29% on whichever side is largest (for example `+X(29%)` for the Croft, whose door faces +Z), so the yaws were set from screenshots and the 40 m screenshots decide.

| Limit | Value | Heaviest measured |
|---|---|---|
| File | 2 MB | castle `.bin` 317,732 B |
| Texture side | 256 px, except the shipped Hexagon atlas | `hexagons_medieval.png` 1024 x 1024 (owner decision 5); characters embed 1024 x 1024 |
| Unit triangles | 5,500 (a note above 5,000) | Adventurers body plus kept items 4,005 (hooded Rogue) to 5,325 (Barbarian with two axes); Mangonel catapult nodes 1,326; Wain parts 668 |
| Building triangles | 6,000 | castle 5,659 (under the plan's 12,000, so it stays) |
| Site and rubble triangles | 1,500 | `building_stage_C` 1,218 |
| Seat flag and banner triangles | 100 | `flag_blue` 48 |
| Interior prop triangles | 1,600 | `trees_B_large` 1,540 (the large trees stay) |
| Edge prop triangles | 300 | `tree_single_B` 220 |
| Team area | units 200‰, the Wain 80‰, buildings 100‰ (model plus banner), the Seat flag and the banner 300‰ | Hexagon buildings 5‰ (mine) to 280‰ (church); flag 457‰ |

The team area is the share of a model's surface whose faces sample its `team_swatches` rects. KayKit Hexagon's team texels are one rect of the shared atlas, `(17, 787, 91, 207)`, found by `inspect_gltf swatches` over the blue and red variants of every vendored building, the catapult tower and the flag; no non-team face samples it, and no neutral model (stages, rubble, nature, cart props) does either.

**The chosen models against the team floors** (a trial `check` with the building mapping of the art-pass plan, every building `team: Some("kaykit_hexagon")`, slice 2):

| Use | Model | Team area | Floor |
|---|---|---|---|
| town | `building_castle_blue` | 67‰ | 100‰, under |
| lumber_yard | `building_lumbermill_blue` | 64‰ | 100‰, under |
| ore_pit | `building_mine_blue` | 5‰ | 100‰, under |
| muster_hall | `building_barracks_blue` | 84‰ | 100‰, under |
| watchtower | `building_tower_A_blue` | 81‰ | 100‰, under |
| engine_yard | `building_blacksmith_blue` | 91‰ | 100‰, under |
| croft | `building_home_A_blue` | 258‰ | 100‰, passes |
| scriptorium | `building_church_blue` | 280‰ | 100‰, passes |
| mangonel (unit) | `building_tower_catapult_blue`, catapult nodes | about 25‰ | 200‰, under |
| Seat flag | `flag_blue` | 457‰ | 300‰, passes |

The rect cannot be widened: it is already every texel that differs between KayKit's blue and red variants, so these buildings simply carry little team surface. The owner chose a team banner on every team building and on the Mangonel (owner decision 12 of the art pass, 2026-10-28; a base disc, lower floors and other models were declined), with the floor measured on model plus banner: the "Building looks" table above has the result, four of the six reach the floor with a banner no taller than their building, and the Town and the Ore Pit are held for the owner. The Mangonel's banner comes with the unit models in slice 4.

`inspect_gltf palette <file> [--grid 16]` prints the base texture's cell grid: each used cell's texel rect, mean colour and the nodes whose faces sample it. It is how a character's `team_swatches` rects are picked (art slice 4), since the Adventurers characters ship in one colour and have no red variant for `swatches` to diff.

## Screenshot sign-off checklist

Attach one 1280x720 screenshot at 40 m to every PR that adds or changes art, and tick each line.

| Check | Yes |
|---|---|
| Every unit kind on screen is identifiable by silhouette alone | |
| Team colour is the only strong hue on units and buildings | |
| No surface shows specular highlight, glow or bloom | |
| Owned land reads as a soft tint with a visible but not hard border | |
| HUD panels are matte and text is legible at 100% scale | |
| The same screenshot looks matte on P3 and as an sRGB export | |
| Total hue count on screen, excluding team colours, is at most 12 | |
| New files have `ATTRIBUTION.md` rows and `scripts/check_assets.sh` passes | |
| `visuals.ron` changed, Rust did not (for a model swap) | |
| Draw-call counter in the dev panel is under 500 with the test scene | |

The last row had no counter to read through M4b: Bevy 0.19.1 keeps its batch counts `pub(crate)` and wgpu has no draw counters, so it was marked n/a in the PR with the expected counts instead (at 200 vs 200 about 75 to 100 batches, [design/ui.md](design/ui.md#health-bars-rings-and-markers)), and an Xcode Metal frame capture on the Mac stood in for it. The art pass's counter (`--draw-stats`, the dev panel) reads it: `art_bench` with every building, site, banner and terrain prop as a model peaks at 111 draws under lavapipe (art slice 3).

## Milestones

| Milestone | Visual deliverable |
|---|---|
| M0 | Flat matte ground plane, directional light, camera stub |
| M2 | Team-coloured primitives, gizmo rings and markers |
| M3b | Territory tint and border; build ghost in green or red |
| M4b | Five distinct unit silhouettes; health bars |
| M5a | Attrition indicator, supply ring, annexation timer |
| M7 | Fog through the field shader, minimap, Inter, Kenney panels, CC0 glTF models, audio; palette finalized here |
