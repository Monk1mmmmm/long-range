//! Long Range map editor.
//!
//!   lr-editor                       interactive editor
//!   lr-editor --generate NAME CELLS SEED [PLAYERS]   write a generated map with settlements to assets/maps/NAME
//!
//! Maps are stored as `assets/maps/<name>/map.json` plus a 16-bit `height.png`.
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy_egui::{egui, EguiContexts, EguiPrimaryContextPass};
use longrange::data::{asset_root, DefId, Defs};
use longrange::map::*;
use longrange::view::models::{spawn_model, ModelCache};
use longrange::view::terrain::{spawn_scene, GameScene, MainCamera};
use longrange::view::yaw_quat;

#[derive(Clone, Copy, PartialEq)]
enum Tool {
    Raise,
    Lower,
    Smooth,
    Flatten,
    Deposit,
    Start,
    Prop,
    Erase,
}

#[derive(Resource)]
struct Editor {
    tool: Tool,
    radius: f32,
    strength: f32,
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
    rebuild_timer: f32,
    flatten_height: Option<f32>,
    status: String,
    maps: Vec<String>,
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
        // All four rotated copies must fit.
        let spots: Vec<V2> = (0..kk).map(|k| longrange::sim::snap(defs, map, d, rot(pos, k))).collect();
        for s in &spots {
            let (x0, y0) = foot_origin(*s, f);
            for y in y0 - 1..y0 + f + 1 {
                for x in x0 - 1..x0 + f + 1 {
                    if !map.inside(x, y) || occ[y as usize * n + x as usize] != 0 {
                        return false;
                    }
                }
            }
            if map.deposits.iter().any(|dp| dp.dist(*s) < f as f32 * CELL + 3.0) || map.starts.iter().any(|st| st.dist(*s) < 45.0) {
                return false;
            }
        }
        for (k, s) in spots.iter().enumerate() {
            let (x0, y0) = foot_origin(*s, f);
            for y in y0..y0 + f {
                for x in x0..x0 + f {
                    occ[y as usize * n + x as usize] = 2;
                }
            }
            map.props.push(Prop { def: name.into(), pos: *s, yaw: yaw + k as f32 * step, owner });
        }
        true
    };
    let homes = ["house", "house", "row_house", "house", "apartment", "row_house", "water_tower", "pylon"];
    let industry = ["warehouse", "refinery", "fuel_tank", "fuel_tank", "pylon", "comm_tower"];
    let towns = 2 + n / 160;
    let mut made = 0;
    let mut tries = 0;
    while made < towns && tries < 400 {
        tries += 1;
        let center = v2(rng.range(30.0, w - 30.0), rng.range(30.0, w - 30.0));
        let (cx, cy) = map.cell_of(center);
        if map.blocked[cy as usize * n + cx as usize] != 0 || map.starts.iter().any(|s| s.dist(center) < 70.0) {
            continue;
        }
        let list: &[&str] = if made % 2 == 0 { &homes } else { &industry };
        let mut placed = 0;
        for _ in 0..40 {
            if placed >= 9 {
                break;
            }
            let name = list[rng.below(list.len())];
            let off = v2((rng.below(7) as f32 - 3.0) * 6.0, (rng.below(7) as f32 - 3.0) * 6.0);
            let yaw = rng.below(4) as f32 * std::f32::consts::FRAC_PI_2;
            if place(map, &mut occ, name, center + off, PROP_NEUTRAL, yaw) {
                placed += 1;
            }
        }
        if placed >= 3 {
            made += 1;
        }
    }
    // Hostile outpost guarding the centre.
    place(map, &mut occ, "comm_tower", c + v2(1.0, 1.0), PROP_NEUTRAL, 0.0);
    place(map, &mut occ, "bunker", c + v2(-14.0, -2.0), PROP_HOSTILE, 0.0);
    place(map, &mut occ, "bunker", c + v2(-8.0, -16.0), PROP_HOSTILE, 0.0);
}

fn rebuild(commands: &mut Commands, ed: &mut Editor, map: &mut MapData, defs: &Defs, old: &Query<Entity, With<GameScene>>, cache: &mut ModelCache, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>) {
    for e in old.iter() {
        commands.entity(e).despawn();
    }
    map.rebuild_blocked();
    spawn_scene(commands, map, meshes, materials, images, false);
    for p in &map.props {
        let Some(d) = defs.id(&p.def) else { continue };
        let (ci, rgb) = if p.owner == PROP_HOSTILE { (201, defs.rules.hostile_color) } else { (200, defs.rules.neutral_color) };
        let root = spawn_model(commands, cache, defs, d, ci, rgb, false, None, meshes, images, materials);
        let h = map.h(p.pos);
        commands.spawn((Transform::from_translation(p.pos.at(h)).with_rotation(yaw_quat(p.yaw)), Visibility::Visible, GameScene)).add_child(root);
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
    keys: Res<ButtonInput<KeyCode>>,
    buttons: Res<ButtonInput<MouseButton>>,
    mut wheel: MessageReader<MouseWheel>,
    time: Res<Time>,
    mut gizmos: Gizmos,
    mut camera: Query<&mut Transform, With<MainCamera>>,
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
                ed.cam_target = map.center();
                full_rebuild = true;
            }
            if ui.button("Flat").clicked() {
                let n = ed.new_cells;
                let w = n as f32 * CELL;
                *map = MapData { name: ed.name.clone(), n, heights: vec![3.0; (n + 1) * (n + 1)], water: 0.0, starts: (0..ed.players).map(|k| v2(w * 0.5, w * 0.5) + V2::from_angle(k as f32 / ed.players as f32 * 6.2832 - 2.356) * (w * 0.38)).collect(), ..Default::default() };
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
            ed.dirty = true;
        }
        ui.separator();
        ui.label("Objects");
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut ed.tool, Tool::Deposit, "Deposit");
            ui.selectable_value(&mut ed.tool, Tool::Start, "Start");
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
        ui.label(format!("{} cells, {} deposits, {} starts, {} objects", map.n, map.deposits.len(), map.starts.len(), map.props.len()));
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
    let target = ed.cam_target.at(map.h(ed.cam_target).max(map.water));
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
                p.at(map.h(p).max(map.water) + 0.3)
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

    // --- tools
    if let (Some(p), false) = (hit, over_ui) {
        let terrain_tool = matches!(ed.tool, Tool::Raise | Tool::Lower | Tool::Smooth | Tool::Flatten);
        ring(&mut gizmos, map, p, if terrain_tool { ed.radius } else { 1.5 }, Color::WHITE);
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
            ed.dirty = true;
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
                Tool::Prop => {
                    let d = defs.get(ed.prop);
                    let pos = if d.is_structure() { longrange::sim::snap(&defs, map, ed.prop, p) } else { p };
                    map.props.push(Prop { def: d.id.clone(), pos, yaw: ed.prop_yaw, owner: if ed.prop_hostile { PROP_HOSTILE } else { PROP_NEUTRAL } });
                    full_rebuild = true;
                }
                Tool::Erase => {
                    let before = (map.deposits.len(), map.props.len());
                    map.deposits.retain(|d| d.dist(p) > 2.5);
                    map.props.retain(|o| o.pos.dist(p) > 2.5);
                    full_rebuild |= before != (map.deposits.len(), map.props.len());
                }
                _ => {}
            }
        }
    }
    // Terrain edits rebuild the meshes a few times per second while painting.
    ed.rebuild_timer -= dt;
    if full_rebuild || (ed.dirty && (ed.rebuild_timer <= 0.0 || !buttons.pressed(MouseButton::Left))) {
        ed.rebuild_timer = 0.2;
        rebuild(&mut commands, ed, map, &defs, &old, &mut cache, &mut meshes, &mut materials, &mut images);
    }
    Ok(())
}

fn setup(mut commands: Commands, mut ed: ResMut<Editor>, mut materials: ResMut<Assets<StandardMaterial>>, mut cache: ResMut<ModelCache>) {
    cache.init(&mut materials);
    commands.spawn((Camera3d::default(), longrange::view::camera_aa(), Transform::from_xyz(0.0, 50.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y), MainCamera));
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
        add_settlements(&mut map, &defs, seed);
        let dir = asset_root().join("maps").join(&name);
        match map.save(&dir) {
            Ok(()) => println!("wrote {} ({} cells, {} deposits, {} objects)", dir.display(), map.n, map.deposits.len(), map.props.len()),
            Err(e) => eprintln!("failed: {e}"),
        }
        return;
    }
    let map = MapData::generate(192, 1, 4);
    let first_prop = defs.id("house").unwrap_or(0);
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window { title: "Long Range - map editor".into(), resolution: bevy::window::WindowResolution::new(1280, 720), ..default() }),
            ..default()
        }).set(longrange::view::render_plugin()))
        .add_plugins(bevy_egui::EguiPlugin::default())
        .insert_resource(ClearColor(Color::srgb(0.06, 0.07, 0.09)))
        .insert_resource(Editor {
            tool: Tool::Raise,
            radius: 14.0,
            strength: 6.0,
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
            rebuild_timer: 0.0,
            flatten_height: None,
            status: String::new(),
            maps: MapData::list(&asset_root()),
        })
        .insert_resource(map)
        .insert_resource(defs)
        .init_resource::<ModelCache>()
        .add_systems(Startup, setup)
        .add_systems(EguiPrimaryContextPass, editor_ui)
        .run();
}
