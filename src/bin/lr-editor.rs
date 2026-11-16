//! Long Range map editor.
//!
//!   lr-editor                       interactive editor
//!   lr-editor --generate NAME CELLS SEED [PLAYERS] [BIOME]   write a generated map with settlements, trees and rocks to assets/maps/NAME
//!                                   (BIOME: temperate, desert, arctic, rocky; default from the seed)
//!   lr-editor --points NAME                          place control points on the saved map NAME (as a game would by itself)
//!   lr-editor --scatter NAME [SEED] [AMOUNT]          add trees and rocks to the saved map NAME (AMOUNT 1 = normal)
//!
//! Maps are stored as `assets/maps/<name>/map.json` plus a 16-bit `height.png` and one 8-bit
//! `layerN.png` mask per ground texture layer above the first.
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use longrange::data::{asset_root, DefId, Defs};
use longrange::map::*;
use longrange::view::models::{spawn_model, ModelCache};
use longrange::data::FeatureClass;
use longrange::view::flora::{flora_material, rebuild_flora, spawn_flora, FloraChunks, FloraView};
use longrange::view::terrain::{spawn_scene, update_chunks, GameScene, GroundSnap, MainCamera, TerrainChunk, WaterPlane};
use longrange::view::yaw_quat;
use longrange::surface::{Biome, BIOMES, KINDS};
use longrange::view::surface::{SurfacePlugin, SurfaceView};

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Raise,
    Lower,
    Smooth,
    Flatten,
    Deposit,
    Start,
    /// Control points of the control points mode (right mouse removes one).
    Point,
    Prop,
    Erase,
    /// Paint trees or rocks (right mouse removes them).
    Flora,
    /// Paint the selected ground texture layer (right mouse takes it away).
    Ground,
    /// Draw roads point by point (right mouse finishes, or deletes the road under the pointer).
    Road,
    /// Fill the hollow under the pointer with a lake (right mouse drains the lake there).
    Lake,
}

/// What the tree and rock brush paints.
#[derive(Clone, Copy, PartialEq)]
enum Paint {
    /// Any feature of the class, picked by its weight.
    Mixed(FeatureClass),
    /// One feature type (index into `rules.features`).
    One(usize),
}

#[derive(Resource)]
struct Editor {
    tool: Tool,
    radius: f32,
    strength: f32,
    /// Lake tool: water depth at the clicked spot.
    lake_depth: f32,
    cam_target: V2,
    cam_dist: f32,
    name: String,
    new_cells: usize,
    players: usize,
    seed: u64,
    start_index: usize,
    prop: DefId,
    prop_hostile: bool,
    prop_yaw: f32,
    dirty: bool,
    flatten_height: Option<f32>,
    status: String,
    maps: Vec<String>,
    paint: Paint,
    /// Brush spacing: 1 = crowns touching, higher = sparser.
    spacing: f32,
    rng: Rng,
    /// Ground layer the Ground tool paints.
    layer: usize,
    biome: Biome,
    road_kind: &'static str,
    road_width: f32,
    /// The road being drawn.
    drawing: Vec<V2>,
}

fn biome_arg(s: &str) -> Option<Biome> {
    BIOMES.iter().copied().find(|b| b.name().to_lowercase().starts_with(&s.to_lowercase()))
}

const SIZES: [usize; 4] = [128, 192, 256, 384];

/// Scatters civilian settlements (and a hostile outpost in the middle) with 4-fold symmetry.
fn add_settlements(map: &mut MapData, defs: &Defs, seed: u64) {
    let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) | 1);
    for _ in 0..6 {
        rng.next();
    }
    let w = map.world();
    let c = map.center();
    let n = map.n;
    let mut occ = map.blocked.clone();
    let kk = map.starts.len().max(1);
    let step = std::f32::consts::TAU / kk as f32;
    let rot = |p: V2, k: usize| {
        let q = p - c;
        let (sn, cs) = (step * k as f32).sin_cos();
        v2(q.x * cs - q.y * sn, q.x * sn + q.y * cs) + c
    };
    let place = |map: &mut MapData, occ: &mut Vec<u8>, name: &str, pos: V2, owner: u8, yaw: f32| -> bool {
        let Some(d) = defs.id(name) else { return false };
        let f = defs.get(d).footprint.max(1);
        // All rotated copies must fit and stay apart; copies that land on the same spot (a building
        // in the very centre) are placed once.
        let mut spots: Vec<(V2, usize)> = vec![];
        for k in 0..kk {
            let s = longrange::sim::snap(defs, map, d, rot(pos, k));
            if !spots.iter().any(|(q, _)| q.dist(s) < 0.01) {
                spots.push((s, k));
            }
        }
        for (i, (a, _)) in spots.iter().enumerate() {
            if spots[..i].iter().any(|(b, _)| (a.x - b.x).abs() < (f + 1) as f32 * CELL && (a.y - b.y).abs() < (f + 1) as f32 * CELL) {
                return false;
            }
        }
        for (s, _) in &spots {
            let (x0, y0) = foot_origin(*s, f);
            for y in y0 - 1..y0 + f + 1 {
                for x in x0 - 1..x0 + f + 1 {
                    if !map.inside(x, y) || occ[y as usize * n + x as usize] != 0 {
                        return false;
                    }
                }
            }
            // Towns may surround a control point, but leave its middle open.
            if map.deposits.iter().any(|dp| dp.dist(*s) < f as f32 * CELL + 3.0) || map.starts.iter().any(|st| st.dist(*s) < 45.0) || map.points.iter().any(|cp| cp.dist(*s) < f as f32 * CELL + 6.0) {
                return false;
            }
        }
        for (s, k) in spots.iter() {
            let (x0, y0) = foot_origin(*s, f);
            for y in y0..y0 + f {
                for x in x0..x0 + f {
                    occ[y as usize * n + x as usize] = 2;
                }
            }
            map.props.push(Prop { def: name.into(), pos: *s, yaw: yaw + *k as f32 * step, owner });
        }
        true
    };
    // Unknown ids (a data preset without these buildings) are skipped by `place`.
    let homes = [
        "house", "house2", "house3", "house4", "house5", "house6", "house7", "house8", "house9", "row_house", "old_house2", "old_house3", "old_house4", "corner_house1",
        "corner_house2", "corner_house3", "corner_house4", "apartment", "apartment2", "residential_tower", "church", "grocery", "water_tower", "pylon",
    ];
    let industry = ["warehouse", "refinery", "fuel_tank", "containers", "containers2", "farm", "plant", "admin_building", "science_lab", "pylon", "comm_tower"];
    let towns = 2 + n / 160;
    let mut made = 0;
    let mut towns_at: Vec<V2> = vec![];
    let first_road = map.roads.len();
    let mut tries = 0;
    while made < towns && tries < 400 {
        tries += 1;
        let center = v2(rng.range(40.0, w - 40.0), rng.range(40.0, w - 40.0));
        let (cx, cy) = map.cell_of(center);
        if map.blocked[cy as usize * n + cx as usize] != 0 || map.starts.iter().any(|s| s.dist(center) < 70.0) {
            continue;
        }
        let list: &[&str] = if made % 2 == 0 { &homes } else { &industry };
        // Towns are dense: each building takes the free lot nearest the centre, so neighbours end
        // up one cell apart (`place` keeps that cell free) - a street about 1.5 tanks wide.
        // The jitter on the distance keeps the outline from being a perfect disc.
        let mut lots: Vec<(f32, V2)> = vec![];
        for dy in -15..=15 {
            for dx in -15..=15 {
                let off = v2(dx as f32 * CELL, dy as f32 * CELL);
                let d = off.len();
                if d <= 30.0 {
                    lots.push((d * rng.range(0.75, 1.25), off));
                }
            }
        }
        lots.sort_by(|a, b| a.0.total_cmp(&b.0));
        let size = 10 + rng.below(9);
        let mut placed = 0;
        let first = map.props.len();
        for _ in 0..size + 6 {
            if placed >= size {
                break;
            }
            let name = list[rng.below(list.len())];
            let yaw = rng.below(4) as f32 * std::f32::consts::FRAC_PI_2;
            if lots.iter().any(|(_, off)| place(map, &mut occ, name, center + *off, PROP_NEUTRAL, yaw)) {
                placed += 1;
            }
        }
        if placed >= 3 {
            made += 1;
            // Paved (homes) or gravel (industry) ground around the buildings and in the streets.
            let ground = if made % 2 == 1 { "paving" } else { "gravel" };
            if let Some(l) = map.layer_of(ground) {
                let spots: Vec<(V2, f32)> = map.props[first..].iter().map(|p| (p.pos, defs.id(&p.def).map_or(2, |d| defs.get(d).footprint) as f32 * CELL * 0.5 + 3.0)).collect();
                for (pos, r) in spots {
                    map.paint_mask(l, pos, r, 1.0, 1.0);
                }
            }
            for k in 0..kk {
                towns_at.push(rot(center, k));
            }
        }
    }
    // Asphalt roads: every town to its nearest neighbour (towns, starts), each pair once.
    let mut ends: Vec<V2> = towns_at.clone();
    ends.extend(map.starts.iter().copied());
    let mut pairs: Vec<(usize, usize)> = vec![];
    for (i, a) in towns_at.iter().enumerate() {
        let near = ends.iter().enumerate().filter(|(j, _)| *j != i).min_by(|x, y| x.1.dist(*a).total_cmp(&y.1.dist(*a)));
        if let Some((j, _)) = near {
            let p = (i.min(j), i.max(j));
            if !pairs.contains(&p) {
                pairs.push(p);
            }
        }
    }
    for (i, j) in pairs {
        let (a, b) = (ends[i], ends[j]);
        map.roads.push(longrange::surface::Road { kind: "asphalt".into(), width: 4.0, points: longrange::surface::winding(a, b, 4.0, seed ^ (i * 31 + j) as u64) });
    }
    // Hostile outpost guarding the centre.
    place(map, &mut occ, "comm_tower", c, PROP_NEUTRAL, 0.0);
    place(map, &mut occ, "high_command", c + v2(14.0, 10.0), PROP_NEUTRAL, 0.0);
    place(map, &mut occ, "bunker", c + v2(-20.0, -3.0), PROP_HOSTILE, 0.0);
    place(map, &mut occ, "bunker", c + v2(-10.0, -22.0), PROP_HOSTILE, 0.0);
    clear_under_props(map, defs);
    map.clear_features_on_roads(&defs.rules.features, first_road);
}

/// Removes trees and rocks under the buildings among the props (the game flattens them anyway).
fn clear_under_props(map: &mut MapData, defs: &Defs) {
    let foots: Vec<(V2, i32)> = map.props.iter().filter_map(|p| defs.id(&p.def).map(|d| (p.pos, defs.get(d).footprint))).filter(|f| f.1 > 0).collect();
    for (pos, f) in foots {
        map.clear_features_square(pos, f);
    }
}

/// Control points as a game would place them, kept off the map's buildings (a point under a house
/// could not be stood on).
fn auto_points(map: &mut MapData, defs: &Defs) -> Vec<V2> {
    map.rebuild_blocked();
    let n = map.n;
    for p in &map.props {
        let Some(d) = defs.id(&p.def) else { continue };
        let f = defs.get(d).footprint;
        if f <= 0 {
            continue;
        }
        let (x0, y0) = foot_origin(p.pos, f);
        for y in y0 - 1..y0 + f + 1 {
            for x in x0 - 1..x0 + f + 1 {
                if map.inside(x, y) {
                    map.blocked[y as usize * n + x as usize] = 1;
                }
            }
        }
    }
    let out = map.auto_points();
    map.rebuild_blocked();
    out
}

/// The feature types the brush can put down.
fn paint_kinds(defs: &Defs, paint: Paint) -> Vec<(usize, f32)> {
    match paint {
        Paint::One(k) => vec![(k, 1.0)],
        Paint::Mixed(c) => defs.rules.features.iter().enumerate().filter(|(_, f)| f.class == c && f.weight > 0.0).map(|(i, f)| (i, f.weight)).collect(),
    }
}

fn paint_class(defs: &Defs, paint: Paint) -> FeatureClass {
    match paint {
        Paint::One(k) => defs.rules.features.get(k).map_or(FeatureClass::Tree, |f| f.class),
        Paint::Mixed(c) => c,
    }
}


/// One stroke of the tree and rock brush: `tries` random spots in the circle get a feature where
/// there is room. Returns true if anything was placed.
fn paint_features(map: &mut MapData, defs: &Defs, rng: &mut Rng, paint: Paint, spacing: f32, center: V2, radius: f32, tries: usize) -> bool {
    let kinds = paint_kinds(defs, paint);
    if kinds.is_empty() {
        return false;
    }
    let total: f32 = kinds.iter().map(|k| k.1).sum();
    let mut placed = false;
    // What already stands near the brush: (position, radius).
    let kind_r: Vec<f32> = map.feature_kinds.iter().map(|k| defs.feature_id(k).map_or(1.0, |i| defs.rules.features[i as usize].radius())).collect();
    let mut near: Vec<(V2, f32)> = map.features.iter().filter(|f| f.pos.dist(center) < radius + 8.0).map(|f| (f.pos, kind_r.get(f.kind as usize).copied().unwrap_or(1.0) * f.scale)).collect();
    for _ in 0..tries {
        // Uniform in the disc.
        let r = radius * rng.f().sqrt();
        let p = center + V2::from_angle(rng.range(0.0, std::f32::consts::TAU)) * r;
        let mut t = rng.f() * total;
        let mut kind = kinds[0].0;
        for (k, wt) in &kinds {
            if t < *wt {
                kind = *k;
                break;
            }
            t -= wt;
        }
        let def = &defs.rules.features[kind];
        let scale = rng.range(0.8, 1.2);
        let fr = def.radius() * scale;
        let w = map.world();
        if p.x < 1.0 || p.y < 1.0 || p.x > w - 1.0 || p.y > w - 1.0 || map.h(p) < map.water_at(p) + 0.2 {
            continue;
        }
        if near.iter().any(|(q, qr)| q.dist(p) < (qr + fr) * 0.85 * spacing) {
            continue;
        }
        near.push((p, fr));
        let k = map.feature_kind(&def.id);
        map.features.push(Feature { kind: k, pos: p, yaw: rng.range(0.0, std::f32::consts::TAU), scale });
        placed = true;
    }
    placed
}

#[allow(clippy::too_many_arguments)]
fn rebuild(commands: &mut Commands, ed: &mut Editor, map: &mut MapData, defs: &Defs, old: &Query<Entity, With<GameScene>>, cache: &mut ModelCache, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>, flora: &mut FloraView) {
    for e in old.iter() {
        commands.entity(e).despawn();
    }
    map.rebuild_blocked();
    spawn_scene(commands, map, meshes, materials, images, false);
    flora.shown = vec![true; map.features.len()];
    flora.dirty.clear();
    flora.all_dirty = false;
    spawn_flora(commands, map, defs, flora, meshes);
    for p in &map.props {
        let Some(d) = defs.id(&p.def) else { continue };
        let (ci, rgb) = if p.owner == PROP_HOSTILE { (201, defs.rules.hostile_color) } else { (200, defs.rules.neutral_color) };
        let root = spawn_model(commands, cache, defs, d, ci, rgb, false, None, meshes, images, materials);
        let h = map.h(p.pos);
        commands.spawn((Transform::from_translation(p.pos.at(h)).with_rotation(yaw_quat(p.yaw)), Visibility::Visible, GroundSnap(0.0), GameScene)).add_child(root);
    }
    ed.dirty = false;
}

#[allow(clippy::too_many_arguments)]
fn editor_ui(
    mut contexts: EguiContexts,
    mut ed: ResMut<Editor>,
    mut map: ResMut<MapData>,
    defs: Res<Defs>,
    mut commands: Commands,
    old: Query<Entity, With<GameScene>>,
    mut cache: ResMut<ModelCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut images: ResMut<Assets<Image>>,
    (keys, buttons): (Res<ButtonInput<KeyCode>>, Res<ButtonInput<MouseButton>>),
    mut wheel: MessageReader<MouseWheel>,
    time: Res<Time>,
    mut gizmos: Gizmos,
    mut camera: Query<&mut Transform, (With<MainCamera>, Without<WaterPlane>, Without<GroundSnap>)>,
    (chunks, mut water, mut snapped, mut fchunks, mut flora, mut surface): (
        Query<(&TerrainChunk, &Mesh3d)>,
        Query<&mut Transform, (With<WaterPlane>, Without<MainCamera>, Without<GroundSnap>)>,
        Query<(&mut Transform, &GroundSnap), (Without<MainCamera>, Without<WaterPlane>)>,
        FloraChunks,
        ResMut<FloraView>,
        Option<ResMut<SurfaceView>>,
    ),
) -> Result {
    let ctx = contexts.ctx_mut()?.clone();
    let ed = &mut *ed;
    let map = &mut *map;
    let dt = time.delta_secs().min(0.1);
    let screen = ctx.content_rect();
    let (sw, sh) = (screen.width().max(1.0), screen.height().max(1.0));
    let mouse = ctx.pointer_latest_pos().unwrap_or(egui::Pos2::new(sw * 0.5, sh * 0.5));
    let mut full_rebuild = false;

    egui::Window::new("Map editor").default_pos([10.0, 10.0]).default_width(250.0).show(&ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Name");
            ui.text_edit_singleline(&mut ed.name);
        });
        ui.horizontal(|ui| {
            if ui.button("Save").clicked() {
                let dir = asset_root().join("maps").join(ed.name.trim());
                map.name = ed.name.trim().to_string();
                ed.status = match map.save(&dir) {
                    Ok(()) => format!("saved to {}", dir.display()),
                    Err(e) => format!("save failed: {e}"),
                };
                ed.maps = MapData::list(&asset_root());
            }
            egui::ComboBox::from_id_salt("load").selected_text("Load...").show_ui(ui, |ui| {
                for m in ed.maps.clone() {
                    if ui.selectable_label(false, &m).clicked() {
                        match MapData::load(&asset_root().join("maps").join(&m)) {
                            Ok(loaded) => {
                                *map = loaded;
                                ed.name = m.clone();
                                ed.cam_target = map.center();
                                ed.status = format!("loaded {m}");
                                full_rebuild = true;
                            }
                            Err(e) => ed.status = e,
                        }
                    }
                }
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("New:");
            egui::ComboBox::from_id_salt("size").selected_text(format!("{} cells", ed.new_cells)).show_ui(ui, |ui| {
                for s in SIZES {
                    ui.selectable_value(&mut ed.new_cells, s, format!("{s} cells ({} units)", s as f32 * CELL));
                }
            });
        });
        ui.add(egui::Slider::new(&mut ed.players, 2..=longrange::sim::MAX_PLAYERS).text("players"));
        ui.horizontal(|ui| {
            ui.label("Seed");
            ui.add(egui::DragValue::new(&mut ed.seed));
        });
        ui.horizontal(|ui| {
            if ui.button("Generate").clicked() {
                *map = MapData::generate(ed.new_cells, ed.seed, ed.players);
                map.paint_ground(ed.biome);
                map.scatter_features(&defs.rules.features, ed.seed, 1.0);
                ed.cam_target = map.center();
                full_rebuild = true;
            }
            if ui.button("Flat").clicked() {
                let n = ed.new_cells;
                let w = n as f32 * CELL;
                *map = MapData { name: ed.name.clone(), n, heights: vec![3.0; (n + 1) * (n + 1)], water: 0.0, starts: (0..ed.players).map(|k| v2(w * 0.5, w * 0.5) + V2::from_angle(k as f32 / ed.players as f32 * 6.2832 - 2.356) * (w * 0.38)).collect(), ..Default::default() };
                map.paint_ground(ed.biome);
                ed.cam_target = map.center();
                full_rebuild = true;
            }
            if ui.button("+ Towns").clicked() {
                map.rebuild_blocked();
                add_settlements(map, &defs, ed.seed);
                full_rebuild = true;
            }
        });
        ui.separator();
        ui.label("Terrain");
        ui.horizontal_wrapped(|ui| {
            for (t, l) in [(Tool::Raise, "Raise"), (Tool::Lower, "Lower"), (Tool::Smooth, "Smooth"), (Tool::Flatten, "Flatten")] {
                ui.selectable_value(&mut ed.tool, t, l);
            }
        });
        ui.add(egui::Slider::new(&mut ed.radius, 2.0..=60.0).text("radius"));
        ui.add(egui::Slider::new(&mut ed.strength, 0.5..=20.0).text("strength"));
        if ui.add(egui::Slider::new(&mut map.water, -5.0..=10.0).text("water level")).changed() {
            // Move the existing water plane instead of rebuilding the scene.
            for mut t in &mut water {
                t.translation.y = map.water;
            }
        }
        // Lakes above that level (real places: a lake up in the hills).
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut ed.tool, Tool::Lake, "Lake").on_hover_text("Left click: fill the hollow under the pointer with water, as deep at that spot as the slider says. Right click: drain the lake there. Click a lake again after reshaping its shore.");
            ui.label(format!("{} on the map", map.lakes.len()));
            if !map.lakes.is_empty() && ui.button("Drain all").clicked() {
                map.clear_lakes();
                full_rebuild = true;
            }
        });
        if ed.tool == Tool::Lake {
            ui.add(egui::Slider::new(&mut ed.lake_depth, 0.3..=30.0).logarithmic(true).text("lake depth"));
        }
        ui.separator();
        ground_ui(ui, ed, map, &mut surface, &mut full_rebuild);
        ui.separator();
        ui.label("Trees and rocks");
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut ed.tool, Tool::Flora, "Brush");
            if ui.button("Scatter").on_hover_text("Adds forests and rock fields with the map's symmetry (uses the seed above)").clicked() {
                map.rebuild_blocked();
                map.scatter_features(&defs.rules.features, ed.seed, 1.0);
                full_rebuild = true;
            }
            if ui.button("Clear all").clicked() {
                map.features.clear();
                map.feature_kinds.clear();
                full_rebuild = true;
            }
        });
        if ed.tool == Tool::Flora {
            let label = |p: Paint| match p {
                Paint::Mixed(FeatureClass::Tree) => "Trees (mixed)".to_string(),
                Paint::Mixed(FeatureClass::Rock) => "Rocks (mixed)".to_string(),
                Paint::One(k) => defs.rules.features.get(k).map_or("?".into(), |f| {
                    let mut v = vec![];
                    if f.mass > 0.0 {
                        v.push(format!("{} mass", f.mass));
                    }
                    if f.energy > 0.0 {
                        v.push(format!("{} energy", f.energy));
                    }
                    format!("{} ({})", f.name, v.join(", "))
                }),
            };
            egui::ComboBox::from_id_salt("paint").selected_text(label(ed.paint)).show_ui(ui, |ui| {
                for p in [Paint::Mixed(FeatureClass::Tree), Paint::Mixed(FeatureClass::Rock)].into_iter().chain((0..defs.rules.features.len()).map(Paint::One)) {
                    ui.selectable_value(&mut ed.paint, p, label(p));
                }
            });
            ui.add(egui::Slider::new(&mut ed.spacing, 0.6..=4.0).text("spacing"));
            let mut detail = flora.detail;
            egui::ComboBox::from_id_salt("flora_detail").selected_text(format!("Detail: {}", longrange::settings::FLORA_DETAIL[detail as usize])).show_ui(ui, |ui| {
                for (k, l) in longrange::settings::FLORA_DETAIL.iter().enumerate() {
                    ui.selectable_value(&mut detail, k as u8, *l);
                }
            });
            if detail != flora.detail {
                // Same setting as in the game (settings.json).
                flora.set_detail(&defs, detail);
                let mut st = longrange::settings::Settings::load();
                st.flora_detail = detail;
                st.save();
            }
            ui.label(egui::RichText::new("Radius and strength above set the brush. Left mouse paints, right mouse removes this kind (trees or rocks).").weak());
        }
        ui.separator();
        ui.label("Objects");
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut ed.tool, Tool::Deposit, "Deposit");
            ui.selectable_value(&mut ed.tool, Tool::Start, "Start");
            ui.selectable_value(&mut ed.tool, Tool::Point, "Control point");
            ui.selectable_value(&mut ed.tool, Tool::Prop, "Building/unit");
            ui.selectable_value(&mut ed.tool, Tool::Erase, "Erase");
        });
        if ed.tool == Tool::Start {
            ui.add(egui::Slider::new(&mut ed.start_index, 0..=longrange::sim::MAX_PLAYERS - 1).text("start number (0 = first)"));
            ui.label(format!("{} start positions. Click the map to place or move one; a new number adds it.", map.starts.len()));
            if map.starts.len() > 2 && ui.button("Remove last start").clicked() {
                map.starts.pop();
            }
        }
        if ed.tool == Tool::Point {
            if map.points.is_empty() {
                ui.label("No control points: games on this map place them automatically.");
            } else {
                ui.label(format!("{} control points.", map.points.len()));
            }
            ui.label(egui::RichText::new("Left mouse places one, right mouse removes the one under the pointer.").weak());
            ui.horizontal(|ui| {
                if ui.button("Auto place").on_hover_text("Replace them with the ones a game would place by itself").clicked() {
                    map.points = auto_points(map, &defs);
                }
                if ui.button("Clear").clicked() {
                    map.points.clear();
                }
            });
        }
        if ed.tool == Tool::Prop {
            egui::ComboBox::from_id_salt("prop").selected_text(defs.get(ed.prop).name.clone()).height(360.0).show_ui(ui, |ui| {
                for f in &defs.factions {
                    ui.label(egui::RichText::new(&f.name).strong());
                    for d in &f.units {
                        ui.selectable_value(&mut ed.prop, *d, &defs.get(*d).name);
                    }
                }
            });
            ui.checkbox(&mut ed.prop_hostile, "hostile (otherwise neutral)");
            ui.add(egui::Slider::new(&mut ed.prop_yaw, 0.0..=6.28).text("facing"));
        }
        ui.separator();
        ui.label(format!("{} cells, {} deposits, {} starts, {} control points, {} objects, {} trees and rocks", map.n, map.deposits.len(), map.starts.len(), map.points.len(), map.props.len(), map.features.len()));
        ui.label(egui::RichText::new("Left mouse: use tool.  Arrows/WASD pan, wheel zoom.").weak());
        if !ed.status.is_empty() {
            ui.label(&ed.status);
        }
    });

    // --- camera
    let world = map.world();
    let pan = ed.cam_dist * 0.9 * dt;
    if !ctx.egui_wants_keyboard_input() {
        if keys.pressed(KeyCode::ArrowLeft) || keys.pressed(KeyCode::KeyA) {
            ed.cam_target.x -= pan;
        }
        if keys.pressed(KeyCode::ArrowRight) || keys.pressed(KeyCode::KeyD) {
            ed.cam_target.x += pan;
        }
        if keys.pressed(KeyCode::ArrowUp) || keys.pressed(KeyCode::KeyW) {
            ed.cam_target.y -= pan;
        }
        if keys.pressed(KeyCode::ArrowDown) || keys.pressed(KeyCode::KeyS) {
            ed.cam_target.y += pan;
        }
    }
    let over_ui = ctx.is_pointer_over_egui();
    let scroll: f32 = wheel.read().map(|w| w.y.signum()).sum();
    if scroll != 0.0 && !over_ui {
        ed.cam_dist = (ed.cam_dist * if scroll > 0.0 { 0.85 } else { 1.0 / 0.85 }).clamp(10.0, world * 1.5);
    }
    ed.cam_target = v2(ed.cam_target.x.clamp(0.0, world), ed.cam_target.y.clamp(0.0, world));
    let pitch = 62f32.to_radians();
    let target = ed.cam_target.at(map.surface(ed.cam_target));
    let cam_pos = target + Vec3::new(0.0, ed.cam_dist * pitch.sin(), ed.cam_dist * pitch.cos());
    if let Ok(mut t) = camera.single_mut() {
        *t = Transform::from_translation(cam_pos).looking_at(target, Vec3::Y);
    }
    let fov = longrange::view::hud::FOV;
    let inv = view_proj(fov, sw / sh, cam_pos, target).inverse();
    let far = inv.project_point3(Vec3::new(2.0 * mouse.x / sw - 1.0, 1.0 - 2.0 * mouse.y / sh, 0.5));
    let hit = map.raycast(cam_pos, far - cam_pos).map(|p| map.clamp(flat(p)));

    // --- markers
    let ring = |g: &mut Gizmos, map: &MapData, c: V2, r: f32, col: Color| {
        let n = 40;
        g.linestrip(
            (0..=n).map(|k| {
                let p = map.clamp(c + V2::from_angle(k as f32 / n as f32 * 6.2832) * r);
                p.at(map.surface(p) + 0.3)
            }),
            col,
        );
    };
    for (k, s) in map.starts.iter().enumerate() {
        let c = defs.rules.player_colors[k % defs.rules.player_colors.len()];
        let col = Color::srgb(c[0], c[1], c[2]);
        ring(&mut gizmos, map, *s, 4.0, col);
        gizmos.line(s.at(map.h(*s)), s.at(map.h(*s) + 14.0), col);
    }
    // Control points: a ring of the capture radius and a pole.
    let cp_col = Color::srgb(1.0, 0.95, 0.6);
    for c in &map.points {
        ring(&mut gizmos, map, *c, defs.rules.cp_radius, cp_col);
        gizmos.line(c.at(map.h(*c)), c.at(map.h(*c) + 10.0), cp_col);
    }

    // --- tools
    if let (Some(p), false) = (hit, over_ui) {
        let terrain_tool = matches!(ed.tool, Tool::Raise | Tool::Lower | Tool::Smooth | Tool::Flatten);
        let brush = terrain_tool || ed.tool == Tool::Flora || ed.tool == Tool::Ground;
        let ring_col = match ed.tool {
            Tool::Flora => Color::srgb(0.5, 1.0, 0.5),
            Tool::Ground => Color::srgb(1.0, 0.8, 0.4),
            _ => Color::WHITE,
        };
        ring(&mut gizmos, map, p, if brush { ed.radius } else if ed.tool == Tool::Road { ed.road_width * 0.5 } else { 1.5 }, ring_col);
        if ed.tool == Tool::Ground && (buttons.pressed(MouseButton::Left) || buttons.pressed(MouseButton::Right)) {
            let target = if buttons.pressed(MouseButton::Left) { 1.0 } else { 0.0 };
            if map.paint_mask(ed.layer, p, ed.radius, (ed.strength * dt * 0.4).min(1.0), target) {
                if let Some(s) = surface.as_mut() {
                    s.masks_dirty = true;
                }
            }
        }
        if ed.tool == Tool::Lake {
            if buttons.just_pressed(MouseButton::Left) {
                // Under an existing lake the pointer is on its surface: measure from the bed.
                let level = map.h(p) + ed.lake_depth;
                ed.status = match map.flood_lake(p, level) {
                    Ok(cells) => format!("Lake at level {level:.1}: {cells} cells"),
                    Err(e) => format!("No lake: {e}"),
                };
                full_rebuild = true;
            }
            if buttons.just_pressed(MouseButton::Right) && map.remove_lake(p) {
                ed.status = "Lake drained".into();
                full_rebuild = true;
            }
        }
        if ed.tool == Tool::Road {
            // The road being drawn, and where the next point would go.
            let line: Vec<Vec3> = ed.drawing.iter().chain(std::iter::once(&p)).map(|q| q.at(map.surface(*q) + 0.3)).collect();
            if line.len() > 1 {
                gizmos.linestrip(line, Color::srgb(1.0, 0.85, 0.3));
            }
            if buttons.just_pressed(MouseButton::Left) {
                ed.drawing.push(p);
            }
            if buttons.just_pressed(MouseButton::Right) {
                let changed = if ed.drawing.len() >= 2 {
                    map.roads.push(longrange::surface::Road { kind: ed.road_kind.into(), width: ed.road_width, points: std::mem::take(&mut ed.drawing) });
                    // Trees and rocks on the new road go.
                    let count = map.features.len();
                    map.clear_features_on_roads(&defs.rules.features, map.roads.len() - 1);
                    if map.features.len() != count {
                        flora.shown = vec![true; map.features.len()];
                        full_rebuild = true;
                    }
                    true
                } else {
                    ed.drawing.clear();
                    map.erase_roads(p, 1.0)
                };
                if let (true, Some(s)) = (changed, surface.as_mut()) {
                    s.roads_dirty = true;
                }
            }
        }
        if ed.tool == Tool::Flora {
            let mut changed = false;
            if buttons.pressed(MouseButton::Left) {
                // About `strength` x 4 tries per second per 100 square units of brush.
                let want = ed.strength * 4.0 * dt * (std::f32::consts::PI * ed.radius * ed.radius / 100.0);
                let tries = want.floor() as usize + (ed.rng.f() < want.fract()) as usize;
                changed |= paint_features(map, &defs, &mut ed.rng, ed.paint, ed.spacing, p, ed.radius, tries);
            } else if buttons.pressed(MouseButton::Right) {
                let class = paint_class(&defs, ed.paint);
                let before = map.features.len();
                let kinds: Vec<Option<FeatureClass>> = map.feature_kinds.iter().map(|k| defs.feature_id(k).map(|i| defs.rules.features[i as usize].class)).collect();
                map.features.retain(|f| f.pos.dist(p) > ed.radius || kinds.get(f.kind as usize).copied().flatten().map_or(false, |c| c != class));
                changed |= map.features.len() != before;
            }
            if changed {
                flora.shown = vec![true; map.features.len()];
                flora.mark_area(map, p, ed.radius + 4.0);
            }
        }
        if terrain_tool && buttons.pressed(MouseButton::Left) {
            if buttons.just_pressed(MouseButton::Left) {
                ed.flatten_height = Some(map.h(p));
            }
            let n = map.n;
            let (r, k) = (ed.radius, ed.strength * dt);
            let (x0, x1) = ((((p.x - r) / CELL).floor().max(0.0)) as usize, (((p.x + r) / CELL).ceil() as usize).min(n));
            let (y0, y1) = ((((p.y - r) / CELL).floor().max(0.0)) as usize, (((p.y + r) / CELL).ceil() as usize).min(n));
            let snapshot = map.heights.clone();
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let d = v2(x as f32 * CELL, y as f32 * CELL).dist(p);
                    if d > r {
                        continue;
                    }
                    let fall = (1.0 - d / r).powi(2) * (3.0 - 2.0 * (1.0 - d / r));
                    let i = y * (n + 1) + x;
                    match ed.tool {
                        Tool::Raise => map.heights[i] += k * fall,
                        Tool::Lower => map.heights[i] -= k * fall,
                        Tool::Flatten => {
                            let t = ed.flatten_height.unwrap_or(0.0);
                            map.heights[i] += (t - map.heights[i]) * (k * 0.5 * fall).min(1.0);
                        }
                        _ => {
                            let mut acc = 0.0;
                            let mut cnt = 0.0;
                            for (dx, dy) in [(0i32, 0i32), (1, 0), (-1, 0), (0, 1), (0, -1)] {
                                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                                if nx >= 0 && ny >= 0 && nx <= n as i32 && ny <= n as i32 {
                                    acc += snapshot[ny as usize * (n + 1) + nx as usize];
                                    cnt += 1.0;
                                }
                            }
                            map.heights[i] += (acc / cnt - map.heights[i]) * (k * 0.4 * fall).min(1.0);
                        }
                    }
                }
            }
            // Update only the touched terrain chunks, in place, and keep markers on the ground.
            // (Rebuilding the whole scene on every stroke made the map flicker.)
            update_chunks(map, &mut meshes, &chunks, p, ed.radius);
            flora.mark_area(map, p, ed.radius + 2.0);
            for (mut t, snap) in &mut snapped {
                if flat(t.translation).dist(p) < ed.radius + 8.0 {
                    t.translation.y = map.height(t.translation.x, t.translation.z) + snap.0;
                }
            }
        }
        if buttons.just_pressed(MouseButton::Left) {
            match ed.tool {
                Tool::Deposit => {
                    let (cx, cy) = map.cell_of(p);
                    let c = v2((cx as f32 + 0.5) * CELL, (cy as f32 + 0.5) * CELL);
                    if !map.deposits.iter().any(|d| d.dist(c) < 4.0) {
                        map.deposits.push(c);
                        full_rebuild = true;
                    }
                }
                Tool::Start => {
                    // A number past the end appends the next start (no gaps).
                    let k = ed.start_index.min(map.starts.len());
                    if k == map.starts.len() {
                        map.starts.push(p);
                    } else {
                        map.starts[k] = p;
                    }
                }
                Tool::Point => {
                    let (cx, cy) = map.cell_of(p);
                    let c = v2((cx as f32 + 0.5) * CELL, (cy as f32 + 0.5) * CELL);
                    if !map.points.iter().any(|q| q.dist(c) < 4.0) {
                        map.points.push(c);
                    }
                }
                Tool::Prop => {
                    let d = defs.get(ed.prop);
                    let pos = if d.is_structure() { longrange::sim::snap(&defs, map, ed.prop, p) } else { p };
                    map.props.push(Prop { def: d.id.clone(), pos, yaw: ed.prop_yaw, owner: if ed.prop_hostile { PROP_HOSTILE } else { PROP_NEUTRAL } });
                    if d.is_structure() {
                        map.clear_features_square(pos, d.footprint);
                    }
                    full_rebuild = true;
                }
                _ => {}
            }
        }
        if ed.tool == Tool::Point && buttons.just_pressed(MouseButton::Right) {
            // The nearest one within its capture radius.
            if let Some(i) = map.points.iter().enumerate().filter(|(_, q)| q.dist(p) < defs.rules.cp_radius.max(4.0)).min_by(|a, b| a.1.dist(p).total_cmp(&b.1.dist(p))).map(|(i, _)| i) {
                map.points.remove(i);
            }
        }
        // Erase works while the button is held, so a drag clears a path through trees and rocks.
        if ed.tool == Tool::Erase && buttons.pressed(MouseButton::Left) {
            let before = (map.deposits.len(), map.props.len());
            map.deposits.retain(|d| d.dist(p) > 2.5);
            map.props.retain(|o| o.pos.dist(p) > 2.5);
            full_rebuild |= before != (map.deposits.len(), map.props.len());
            // Trees and rocks count as hit when the cursor ring touches their crown or body.
            let kind_r: Vec<f32> = map.feature_kinds.iter().map(|k| defs.feature_id(k).map_or(1.0, |i| defs.rules.features[i as usize].radius())).collect();
            let count = map.features.len();
            map.features.retain(|f| f.pos.dist(p) > 2.5 + kind_r.get(f.kind as usize).copied().unwrap_or(1.0) * f.scale * 0.5);
            if map.features.len() != count {
                flora.shown = vec![true; map.features.len()];
                flora.mark_area(map, p, 2.5 + 8.0);
            }
        }
    }
    // Only structural changes (new map, load, objects, deposits) respawn the scene.
    if full_rebuild || ed.dirty {
        rebuild(&mut commands, ed, map, &defs, &old, &mut cache, &mut meshes, &mut materials, &mut images, &mut flora);
    } else {
        rebuild_flora(&mut commands, map, &defs, &mut flora, &mut fchunks, &mut meshes);
    }
    Ok(())
}

/// The Ground section of the editor window: layers, automatic painting, the paint and road tools.
fn ground_ui(ui: &mut egui::Ui, ed: &mut Editor, map: &mut MapData, surface: &mut Option<ResMut<SurfaceView>>, full_rebuild: &mut bool) {
    let kind_name = |id: &str| KINDS.iter().find(|k| k.id == id).map_or(id.to_string(), |k| k.name.to_string());
    let (mut layers_changed, mut masks_changed, mut roads_changed) = (false, false, false);
    ui.label("Ground");
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut ed.tool, Tool::Ground, "Paint");
        ui.selectable_value(&mut ed.tool, Tool::Road, "Road");
    });
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("biome").selected_text(ed.biome.name()).show_ui(ui, |ui| {
            for b in BIOMES {
                ui.selectable_value(&mut ed.biome, b, b.name());
            }
        });
        if ui.button("Auto paint").on_hover_text("Replaces all layers with the biome's, painted from heights and slopes (also used by Generate and Flat)").clicked() {
            map.paint_ground(ed.biome);
            ed.layer = ed.layer.min(map.layers.len().saturating_sub(1));
            layers_changed = true;
            masks_changed = true;
        }
    });
    // Layers, top first, as in an image editor.
    let count = map.layers.len();
    let mut action: Option<(usize, u8)> = None;
    for i in (0..count).rev() {
        ui.horizontal(|ui| {
            ui.radio_value(&mut ed.layer, i, format!("{i}"));
            let cur = map.layers[i].kind.clone();
            egui::ComboBox::from_id_salt(("layer_kind", i)).width(110.0).selected_text(kind_name(&cur)).show_ui(ui, |ui| {
                for k in KINDS {
                    if ui.selectable_label(cur == k.id, k.name).clicked() && cur != k.id {
                        map.layers[i].kind = k.id.into();
                        layers_changed = true;
                    }
                }
            });
            let mut sc = if map.layers[i].scale > 0.0 { map.layers[i].scale } else { KINDS.iter().find(|k| k.id == cur).map_or(8.0, |k| k.scale) };
            if ui.add(egui::DragValue::new(&mut sc).range(1.0..=64.0).speed(0.2)).on_hover_text("Tile size in world units").changed() {
                map.layers[i].scale = sc;
                layers_changed = true;
            }
            if i > 0 {
                if i + 1 < count && ui.small_button("^").on_hover_text("Move up").clicked() {
                    action = Some((i, 0));
                }
                if i > 1 && ui.small_button("v").on_hover_text("Move down").clicked() {
                    action = Some((i - 1, 0));
                }
                if ui.small_button("x").on_hover_text("Remove layer").clicked() {
                    action = Some((i, 1));
                }
            }
        });
    }
    match action {
        Some((i, 0)) => {
            map.swap_layers(i);
            if ed.layer == i {
                ed.layer = i + 1;
            } else if ed.layer == i + 1 {
                ed.layer = i;
            }
            layers_changed = true;
            masks_changed = true;
        }
        Some((i, _)) => {
            map.remove_layer(i);
            ed.layer = ed.layer.min(map.layers.len().saturating_sub(1));
            layers_changed = true;
            masks_changed = true;
        }
        None => {}
    }
    if map.layers.len() < longrange::surface::MAX_LAYERS && ui.button("+ Layer").clicked() {
        if let Some(i) = map.add_layer("dirt") {
            ed.layer = i;
        }
        layers_changed = true;
        masks_changed = true;
    }
    if ed.tool == Tool::Ground {
        ui.label(egui::RichText::new("Radius and strength above set the brush. Left mouse paints the selected layer, right mouse takes it away. On layer 0 it takes away every layer above.").weak());
    }
    if ed.tool == Tool::Road {
        egui::ComboBox::from_id_salt("road_kind").selected_text(kind_name(ed.road_kind)).show_ui(ui, |ui| {
            for k in KINDS {
                ui.selectable_value(&mut ed.road_kind, k.id, k.name);
            }
        });
        ui.add(egui::Slider::new(&mut ed.road_width, 1.0..=12.0).text("width"));
        ui.label(egui::RichText::new(format!("Left mouse adds a point, right mouse finishes the road (or deletes the road under the pointer). {} roads.", map.roads.len())).weak());
        if !map.roads.is_empty() && ui.button("Remove all roads").clicked() {
            map.roads.clear();
            roads_changed = true;
        }
    }
    if let Some(s) = surface.as_mut() {
        s.layers_dirty |= layers_changed;
        s.masks_dirty |= masks_changed;
        s.roads_dirty |= roads_changed;
    } else if layers_changed || masks_changed || roads_changed {
        *full_rebuild = true;
    }
}

fn setup(mut commands: Commands, mut ed: ResMut<Editor>, mut materials: ResMut<Assets<StandardMaterial>>, mut cache: ResMut<ModelCache>, defs: Res<Defs>) {
    cache.init(&mut materials);
    commands.insert_resource(FloraView::new(&defs, flora_material(&mut materials, None), vec![], longrange::settings::Settings::load().flora_detail));
    commands.spawn((Camera3d::default(), Msaa::Off, Transform::from_xyz(0.0, 50.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y), MainCamera));
    ed.dirty = true;
}

fn main() {
    let defs = match Defs::load(&asset_root()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Cannot load game data: {e}");
            std::process::exit(1);
        }
    };
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--generate") {
        let name = args.get(i + 1).cloned().unwrap_or_else(|| "generated".into());
        let cells: usize = args.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(256);
        let seed: u64 = args.get(i + 3).and_then(|s| s.parse().ok()).unwrap_or(1);
        let players: usize = args.get(i + 4).and_then(|s| s.parse().ok()).unwrap_or(4);
        let mut map = MapData::generate(cells, seed, players);
        map.name = name.clone();
        if let Some(b) = args.get(i + 5).and_then(|s| biome_arg(s)) {
            map.paint_ground(b);
        }
        map.scatter_features(&defs.rules.features, seed, 1.0);
        add_settlements(&mut map, &defs, seed);
        let dir = asset_root().join("maps").join(&name);
        match map.save(&dir) {
            Ok(()) => println!("wrote {} ({} cells, {} deposits, {} objects, {} trees and rocks)", dir.display(), map.n, map.deposits.len(), map.props.len(), map.features.len()),
            Err(e) => eprintln!("failed: {e}"),
        }
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--points") {
        let name = args.get(i + 1).cloned().unwrap_or_default();
        let dir = asset_root().join("maps").join(&name);
        let mut map = match MapData::load(&dir) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("cannot load map '{name}': {e}");
                std::process::exit(1);
            }
        };
        if !map.points.is_empty() {
            eprintln!("{} already has {} control points (move them in the editor)", dir.display(), map.points.len());
            std::process::exit(1);
        }
        let points = auto_points(&mut map, &defs);
        // Only the points go into map.json: saving the whole map would also write what it reads
        // with defaults (ground layers of a map saved without them).
        let file = dir.join("map.json");
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        let list = serde_json::to_string_pretty(&points).unwrap_or_default().replace('\n', "\n  ");
        let block = format!("\n  \"points\": {list},");
        let at = text.find("\n  \"props\":").or_else(|| text.find("\n  \"seed\":"));
        let Some(at) = at else {
            eprintln!("{}: no props or seed entry to put the points before", file.display());
            std::process::exit(1);
        };
        let out = format!("{}{}{}", &text[..at], block, &text[at..]);
        match MapData::load(&dir).and_then(|_| std::fs::write(&file, &out).map_err(|e| e.to_string())).and_then(|_| MapData::load(&dir)) {
            Ok(m) if m.points.len() == points.len() => println!("{}: {} control points placed", dir.display(), points.len()),
            Ok(_) => eprintln!("{}: the points did not read back", file.display()),
            Err(e) => eprintln!("failed: {e}"),
        }
        return;
    }
    if let Some(i) = args.iter().position(|a| a == "--scatter") {
        let name = args.get(i + 1).cloned().unwrap_or_default();
        let dir = asset_root().join("maps").join(&name);
        let mut map = match MapData::load(&dir) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("cannot load map '{name}': {e}");
                std::process::exit(1);
            }
        };
        let seed: u64 = args.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(map.seed.max(1));
        let amount: f32 = args.get(i + 3).and_then(|s| s.parse().ok()).unwrap_or(1.0);
        let before = map.features.len();
        map.scatter_features(&defs.rules.features, seed, amount);
        clear_under_props(&mut map, &defs);
        match map.save(&dir) {
            Ok(()) => println!("{}: {} trees and rocks added, {} in all", dir.display(), map.features.len() as i64 - before as i64, map.features.len()),
            Err(e) => eprintln!("failed: {e}"),
        }
        return;
    }
    let mut map = MapData::generate(192, 1, 4);
    map.scatter_features(&defs.rules.features, 1, 1.0);
    let first_prop = defs.id("house").unwrap_or(0);
    let mut app = App::new();
    app
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "Long Range - map editor".into(), resolution: bevy::window::WindowResolution::new(1280, 720), ..default() }),
            ..default()
        }).set(longrange::settings::render_plugin(&longrange::settings::Settings::load())).set(longrange::settings::log_plugin()))
        .add_plugins(bevy_egui::EguiPlugin::default())
        .add_plugins(SurfacePlugin)
        .insert_resource(ClearColor(Color::srgb(0.06, 0.07, 0.09)))
        .insert_resource(Editor {
            tool: Tool::Raise,
            radius: 14.0,
            strength: 6.0,
            lake_depth: 2.0,
            cam_target: map.center(),
            cam_dist: 140.0,
            name: "my_map".into(),
            new_cells: 192,
            players: 4,
            seed: 1,
            start_index: 0,
            prop: first_prop,
            prop_hostile: false,
            prop_yaw: 0.0,
            dirty: true,
            flatten_height: None,
            status: String::new(),
            maps: MapData::list(&asset_root()),
            paint: Paint::Mixed(FeatureClass::Tree),
            spacing: 1.0,
            rng: Rng(0x5EED_F00D),
            layer: 1,
            biome: Biome::Temperate,
            road_kind: "dirt_road",
            road_width: 3.5,
            drawing: vec![],
        })
        .insert_resource(map)
        .insert_resource(defs)
        .init_resource::<ModelCache>()
        .add_systems(Startup, setup)
        .add_systems(EguiPrimaryContextPass, editor_ui)
        .add_systems(Update, longrange::view::terrain::map_edge_sync);
    longrange::settings::tolerate_render_errors(&mut app);
    longrange::settings::add_window_icon(&mut app);
    longrange::settings::gl_window_display(&mut app);
    app.run();
}
