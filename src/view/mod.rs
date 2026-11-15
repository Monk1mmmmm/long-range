//! Presentation layer: reads the simulation components (identical on host and clients),
//! draws them in 3D and turns player input into `Command`s.
use crate::data::{DefId, Defs};
use crate::map::*;
use crate::sim::*;
use crate::sim::Command;
use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

pub mod cursor;
pub mod flora;
pub mod fx;
pub mod hud;
pub mod icons;
pub mod intel;
pub mod models;
pub mod sound;
pub mod terrain;

use models::*;
use terrain::*;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Mode {
    None,
    AttackMove,
    Patrol,
    GroundAttack,
    Reclaim,
    /// Explicit move / assist commands (FAF-style hotkeys).
    Move,
    Assist,
    Capture,
    /// Scavengers: click a wreck or drag a circle.
    Revive,
    /// Engineers: drag a circle; the wheel sets how far to raise or lower (0 = level).
    Terraform,
    /// Transports: click a unit or drag a circle. Other units: click a transport.
    Load,
    Unload,
    Place(DefId),
}

/// Local player's view of the game: camera, selection, input state.
#[derive(Resource)]
pub struct ViewState {
    pub me: u8,
    pub cam_target: V2,
    pub cam_dist: f32,
    pub cam_pos: Vec3,
    /// Camera heading around the vertical axis (radians) and extra tilt (degrees), set with Space + mouse.
    pub cam_yaw: f32,
    pub cam_tilt: f32,
    pub space_last: Option<Vec2>,
    pub space_moved: bool,
    pub vp: Mat4,
    pub inv: Mat4,
    pub sel: Vec<u32>,
    pub groups: Vec<Vec<u32>>,
    pub mode: Mode,
    pub box_start: Option<Vec2>,
    pub drag_start: Option<V2>,
    /// 0 = left button (mode order), 1 = right button.
    pub pathing: Option<u8>,
    pub path: Vec<V2>,
    pub last_click: (f64, DefId),
    pub last_group: (f64, usize),
    pub idle_cycle: usize,
    pub menu_open: bool,
    pub quit: bool,
    pub mmb_last: Option<Vec2>,
    pub last_tick: u32,
    pub tick_time: f64,
    pub settings_open: bool,
    pub show_help: bool,
    /// Hotkey preset: 0 = default, 1 = FAF alternative.
    pub preset: u8,
    /// Hotbuild cycling: (key, index, time, last definition).
    pub hot: (Option<KeyCode>, usize, f64, DefId),
    /// A run of a factory queue being dragged: (factory id, index of the run).
    pub queue_drag: Option<(u32, usize)>,
    /// Terraform mode: how far to raise (+) or lower (-) the ground; 0 levels it.
    pub terra_delta: f32,
    /// Selection of the previous frame, to notice when it changes (selection sounds).
    pub last_sel: Vec<u32>,
    /// The game is paused, by that player (set by the session).
    pub paused: Option<u8>,
    /// The local player asked to pause or resume this frame (read by the session).
    pub pause_toggle: bool,
}

/// Hotkey preset from the saved settings: 0 = default, 1 = FAF alternative.
pub fn load_preset() -> u8 {
    (crate::settings::Settings::load().hotkeys == "faf_alternative") as u8
}

impl ViewState {
    pub fn new(me: u8, start: V2) -> ViewState {
        ViewState {
            me,
            cam_target: start,
            cam_dist: 45.0,
            cam_pos: Vec3::ZERO,
            cam_yaw: 0.0,
            cam_tilt: 0.0,
            space_last: None,
            space_moved: false,
            vp: Mat4::IDENTITY,
            inv: Mat4::IDENTITY,
            sel: vec![],
            groups: vec![vec![]; 10],
            mode: Mode::None,
            box_start: None,
            drag_start: None,
            pathing: None,
            path: vec![],
            last_click: (0.0, DefId::MAX),
            last_group: (0.0, 99),
            idle_cycle: 0,
            menu_open: false,
            quit: false,
            mmb_last: None,
            last_tick: u32::MAX,
            tick_time: 0.0,
            settings_open: false,
            show_help: false,
            preset: load_preset(),
            hot: (None, 0, 0.0, DefId::MAX),
            queue_drag: None,
            terra_delta: 0.0,
            last_sel: vec![],
            paused: None,
            pause_toggle: false,
        }
    }
}

/// Commands issued by the local player this frame; the session forwards them.
#[derive(Resource, Default)]
pub struct LocalCommands(pub Vec<Command>);

/// Render-side interpolation between the last two simulation ticks.
#[derive(Component, Clone, Copy)]
pub struct Interp {
    pub prev: (V2, f32, f32),
    pub cur: (V2, f32, f32),
    pub pos: V2,
    pub y: f32,
    pub yaw: f32,
}

pub fn owner_color(defs: &Defs, game: &Game, owner: u8) -> (u8, [f32; 3]) {
    match owner {
        NEUTRAL => (200, defs.rules.neutral_color),
        HOSTILE => (201, defs.rules.hostile_color),
        o => {
            let c = game.players.get(o as usize).map_or(0, |p| p.color);
            (c, defs.rules.player_colors[c as usize % defs.rules.player_colors.len()])
        }
    }
}

pub fn yaw_quat(yaw: f32) -> Quat {
    Quat::from_rotation_y(-yaw - FRAC_PI_2)
}

/// Starts the view for a game that is already set up in the world (map, game, units).
pub fn start_view(world: &mut World, me: u8, with_fog: bool) {
    let map = world.resource::<MapData>().clone();
    let start = world.resource::<Game>().players.get(me as usize).map_or(map.center(), |p| p.start);
    world.insert_resource(ViewState::new(me, start));
    world.insert_resource(LocalCommands::default());
    world.insert_resource(intel::IntelMemory::default());
    world.insert_resource(intel::FogGrid::new(map.n));
    world.insert_resource(hud::HudAssets::default());
    world.resource_scope(|world, mut meshes: Mut<Assets<Mesh>>| {
        world.resource_scope(|world, mut materials: Mut<Assets<StandardMaterial>>| {
            world.resource_scope(|world, mut images: Mut<Assets<Image>>| {
                let mut cache = ModelCache::default();
                cache.init(&mut materials);
                world.insert_resource(cache);
                world.insert_resource(fx::FxAssets::new(&mut meshes, &mut materials));
                let fog = spawn_scene(&mut world.commands(), &map, &mut meshes, &mut materials, &mut images, with_fog);
                // Trees and rocks: what still stands in the state the game is in.
                let shown: Vec<bool> = match world.get_resource::<crate::sim::Features>() {
                    Some(f) => (0..map.features.len()).map(|i| f.alive(i)).collect(),
                    None => vec![true; map.features.len()],
                };
                let version = world.get_resource::<crate::sim::Features>().map_or(0, |f| f.version);
                let defs = world.resource::<Defs>().clone();
                let detail = world.get_resource::<crate::settings::Settings>().map_or(1, |s| s.flora_detail);
                let mut flora = flora::FloraView::new(&defs, flora::flora_material(&mut materials, Some(fog)), shown, detail);
                flora.version = version;
                flora::spawn_flora(&mut world.commands(), &map, &defs, &flora, &mut meshes);
                world.insert_resource(flora);
            });
        });
    });
    world.flush();
}

/// Removes everything the view created.
pub fn stop_view(world: &mut World) {
    let doomed: Vec<Entity> = world.query_filtered::<Entity, With<GameScene>>().iter(world).collect();
    for e in doomed {
        if world.get_entity(e).is_ok() {
            world.despawn(e);
        }
    }
    world.remove_resource::<ViewState>();
    world.remove_resource::<ModelCache>();
    world.remove_resource::<flora::FloraView>();
    world.remove_resource::<hud::HudAssets>();
}

/// New tick or snapshot: shift interpolation targets.
fn tick_sync(mut view: ResMut<ViewState>, game: Res<Game>, time: Res<Time>, mut q: Query<(&Body, &mut Interp)>) {
    if game.tick == view.last_tick {
        return;
    }
    view.last_tick = game.tick;
    view.tick_time = time.elapsed_secs_f64();
    for (b, mut i) in &mut q {
        i.prev = i.cur;
        i.cur = (b.pos, b.y, b.yaw);
    }
}

/// Gives new units a model, and re-skins units whose type or build state changed.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn unit_visuals(
    mut commands: Commands,
    fresh: Query<(Entity, &Unit, &Body, Has<UnderConstruction>), Without<Interp>>,
    existing: Query<(Entity, &Unit, Has<UnderConstruction>, &Children), With<Interp>>,
    roots: Query<&ModelRoot>,
    defs: Res<Defs>,
    game: Res<Game>,
    mut cache: ResMut<ModelCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut budget = 12;
    for (e, u, b, uc) in &fresh {
        if budget == 0 {
            break;
        }
        budget -= 1;
        let (ci, rgb) = owner_color(&defs, &game, u.owner);
        let root = spawn_model(&mut commands, &mut cache, &defs, u.def, ci, rgb, uc, None, &mut meshes, &mut images, &mut materials);
        let s = (b.pos, b.y, b.yaw);
        commands
            .entity(e)
            .insert((Interp { prev: s, cur: s, pos: b.pos, y: b.y, yaw: b.yaw }, Transform::from_translation(b.p3()).with_rotation(yaw_quat(b.yaw)), Visibility::Hidden))
            .add_child(root);
    }
    for (e, u, uc, children) in &existing {
        for c in children.iter() {
            if let Ok(r) = roots.get(c) {
                if r.def != u.def || r.building != uc {
                    commands.entity(c).despawn();
                    let (ci, rgb) = owner_color(&defs, &game, u.owner);
                    let root = spawn_model(&mut commands, &mut cache, &defs, u.def, ci, rgb, uc, None, &mut meshes, &mut images, &mut materials);
                    commands.entity(e).add_child(root);
                }
            }
        }
    }
}

/// Interpolated transforms, terrain tilt, visibility by fog of war.
#[allow(clippy::type_complexity)]
fn unit_transforms(
    view: Res<ViewState>,
    time: Res<Time>,
    game: Res<Game>,
    defs: Res<Defs>,
    map: Res<MapData>,
    mut q: Query<(&Unit, &Intel, &mut Interp, &mut Transform, &mut Visibility, Has<Hosted>, Option<&UnderConstruction>)>,
) {
    let alpha = (((time.elapsed_secs_f64() - view.tick_time) / DT as f64) as f32).clamp(0.0, 1.0);
    let team = game.team_of(view.me);
    let reveal = intel::reveal(&game, view.me);
    for (u, intel, mut i, mut t, mut vis, hosted, uc) in &mut q {
        let d = defs.get(u.def);
        let pos = i.prev.0.lerp(i.cur.0, alpha);
        let y = i.prev.1 + (i.cur.1 - i.prev.1) * alpha;
        let yaw = i.prev.2 + wrap(i.cur.2 - i.prev.2) * alpha;
        i.pos = pos;
        i.y = y;
        i.yaw = yaw;
        let mut rot = yaw_quat(yaw);
        if d.is_plane() {
            // Bank into turns.
            let rate = wrap(i.cur.2 - i.prev.2) / DT;
            rot *= Quat::from_rotation_z((-rate * 0.5).clamp(-0.7, 0.7));
        } else if !d.is_structure() && !d.is_air() {
            rot = Quat::from_rotation_arc(Vec3::Y, map.normal(pos.x, pos.y)) * rot;
        }
        t.translation = pos.at(y);
        t.rotation = rot;
        // Buildings rise out of the ground while under construction.
        t.scale = match uc {
            Some(p) if d.is_structure() => Vec3::new(1.0, 0.25 + 0.75 * p.0.clamp(0.0, 1.0), 1.0),
            _ => Vec3::ONE,
        };
        let visible = !hosted && (reveal || intel.seen & (1 << team) != 0);
        let want = if visible { Visibility::Visible } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }
}

/// Turrets, barrels and spinning parts.
fn animate_parts(
    time: Res<Time>,
    units: Query<(&Interp, Option<&Weapons>)>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<ModelRoot>>,
    mut aims: Query<(Entity, &AimPart, &mut Transform), Without<SpinPart>>,
    mut spins: Query<(&SpinPart, &mut Transform), Without<AimPart>>,
) {
    for (e, aim, mut t) in &mut aims {
        // Walk up to the unit entity (the parent of the model root).
        let mut cur = e;
        let mut unit = None;
        for _ in 0..12 {
            let Ok(p) = parents.get(cur) else { break };
            if roots.get(cur).is_ok() {
                unit = Some(p.parent());
                break;
            }
            cur = p.parent();
        }
        let Some((interp, Some(w))) = unit.and_then(|u| units.get(u).ok()) else { continue };
        let Some(st) = w.list.get(aim.weapon) else { continue };
        let q = if aim.pitch { Quat::from_rotation_x(st.pitch.clamp(-0.3, 1.4)) } else { Quat::from_rotation_y(-wrap(st.yaw - interp.yaw)) };
        *t = aim.rest * Transform::from_rotation(q);
    }
    let secs = time.elapsed_secs();
    for (s, mut t) in &mut spins {
        *t = s.rest * Transform::from_rotation(Quat::from_axis_angle(s.axis, (secs * s.speed.to_radians()) % std::f32::consts::TAU));
    }
}

#[derive(Component)]
struct WreckVisual;

#[allow(clippy::too_many_arguments)]
fn wreck_visuals(
    mut commands: Commands,
    fresh: Query<(Entity, &Wreck), Without<WreckVisual>>,
    defs: Res<Defs>,
    map: Res<MapData>,
    mut cache: ResMut<ModelCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (e, w) in fresh.iter().take(10) {
        let d = defs.get(w.def);
        let mat = if d.model.mesh.is_some() {
            cache.wreck_mat(&defs, w.def, &mut images, &mut materials)
        } else {
            cache.flat_mat(&crate::data::ColorSpec::Rgb([0.2, 0.18, 0.17]), 255, [0.2; 3], &mut materials)
        };
        let root = spawn_model(&mut commands, &mut cache, &defs, w.def, 255, [0.3, 0.28, 0.26], false, Some(mat), &mut meshes, &mut images, &mut materials);
        // Collapsed and slightly askew.
        let squash = if d.is_structure() { 0.35 } else { 0.8 };
        let rot = Quat::from_rotation_arc(Vec3::Y, map.normal(w.pos.x, w.pos.y)) * yaw_quat(w.yaw) * Quat::from_rotation_z(((w.id % 7) as f32 - 3.0) * 0.04);
        commands
            .entity(e)
            .insert((WreckVisual, Transform::from_translation(w.pos.at(w.y)).with_rotation(rot).with_scale(Vec3::new(1.0, squash, 1.0)), Visibility::Visible))
            .add_child(root);
    }
}

/// Moves the sun's shadow range with the zoom level and applies the shadow toggle.
fn light_follow(view: Res<ViewState>, settings: Res<crate::settings::Settings>, mut q: Query<(&mut DirectionalLight, &mut bevy::light::CascadeShadowConfig), With<Sun>>) {
    for (mut light, mut cfg) in &mut q {
        let on = settings.shadows > 0 && view.cam_dist < 420.0;
        if light.shadow_maps_enabled != on {
            light.shadow_maps_enabled = on;
        }
        let max = (view.cam_dist * 2.6).clamp(60.0, 1200.0);
        let cur = cfg.bounds.last().copied().unwrap_or(0.0);
        if (cur - max).abs() > max * 0.15 {
            *cfg = bevy::light::CascadeShadowConfigBuilder { num_cascades: 3, maximum_distance: max, first_cascade_far_bound: max * 0.18, ..default() }.build();
        }
    }
}

/// Keeps placement of per-def data out of systems that only need it occasionally.
#[derive(Resource, Default)]
pub struct NameCache(pub HashMap<DefId, String>);

pub fn in_game(view: Option<Res<ViewState>>, game: Option<Res<Game>>, map: Option<Res<MapData>>) -> bool {
    view.is_some() && game.is_some() && map.is_some()
}

pub struct ViewPlugin;
impl Plugin for ViewPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LocalCommands>().init_resource::<sound::UiSounds>();
        app.add_systems(Startup, sound::load);
        app.add_systems(
            Update,
            (tick_sync, terrain::terrain_sync, terrain::map_edge_sync, flora::flora_sync, intel::update_intel, unit_visuals, wreck_visuals, unit_transforms, animate_parts, fx::projectile_visuals, fx::spawn_fx, fx::update_fx, light_follow)
                .chain()
                .run_if(in_game),
        );
        app.add_systems(Update, sound::play_ui);
        app.add_systems(bevy_egui::EguiPrimaryContextPass, hud::ui_system.run_if(in_game));
        app.init_resource::<cursor::CursorWant>().init_resource::<cursor::CursorImages>();
        app.add_systems(Startup, cursor::take_over_from_egui);
        app.add_systems(PostUpdate, cursor::apply_cursor.after(bevy_egui::EguiPostUpdateSet::ProcessOutput));
    }
}
