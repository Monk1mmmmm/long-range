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
pub mod debris;
pub mod flora;
pub mod fx;
pub mod hud;
pub mod icons;
pub mod intel;
pub mod models;
pub mod sound;
pub mod surface;
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
    /// Network notice shown in the middle of the screen (the host left, a new host is taking over...).
    pub net_note: Option<String>,
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
            net_note: None,
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
                world.insert_resource(debris::DebrisAssets::new(&mut meshes, &mut materials));
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
    world.remove_resource::<debris::DebrisAssets>();
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
        if let Some(a) = Animator::new(cache.model(&defs, u.def, &mut meshes)) {
            commands.entity(e).insert(a);
        }
    }
    for (e, u, uc, children) in &existing {
        for c in children.iter() {
            if let Ok(r) = roots.get(c) {
                if r.def != u.def || r.building != uc {
                    commands.entity(c).despawn();
                    let (ci, rgb) = owner_color(&defs, &game, u.owner);
                    let root = spawn_model(&mut commands, &mut cache, &defs, u.def, ci, rgb, uc, None, &mut meshes, &mut images, &mut materials);
                    commands.entity(e).add_child(root);
                    match Animator::new(cache.model(&defs, u.def, &mut meshes)) {
                        Some(a) => commands.entity(e).insert(a),
                        None => commands.entity(e).remove::<Animator>(),
                    };
                }
            }
        }
    }
}

/// Plays the units' model animations (the mod's .sca files) from what the simulation shows, the way
/// SupCom's unit scripts play them (`AnimKind` says which does what). View only.
#[allow(clippy::type_complexity)]
fn drive_anims(
    time: Res<Time>,
    defs: Res<Defs>,
    game: Res<Game>,
    map: Res<MapData>,
    mut q: Query<(
        &Unit,
        &Interp,
        &mut Animator,
        Option<&Upgrading>,
        Option<&Builder>,
        Option<&Factory>,
        Option<&Weapons>,
        Has<Paused>,
        Has<UnderConstruction>,
    )>,
) {
    let dt = time.delta_secs();
    for (u, i, mut a, up, builder, factory, weapons, paused, uc) in &mut q {
        let d = defs.get(u.def);
        let top = d.mv.speed.max(0.1);
        let step = i.cur.0 - i.prev.0;
        let forward = step.dot(V2::from_angle(i.yaw)) / DT;
        let moving = step.len() / DT > top * 0.15;
        let working = builder.is_some_and(|b| b.work != 0) && !paused && !uc;
        // Toward the end while `on`, back to the start otherwise; shown while not at the start.
        let toward = |s: &mut AnimSlot, on: bool, back: f32| {
            let dur = s.anim.duration();
            s.time = if on { (s.time + dt).min(dur) } else { (s.time - dt * back).max(0.0) };
            s.weight = if s.time > 0.0 { 1.0 } else { 0.0 };
        };
        for s in a.slots.iter_mut() {
            let dur = s.anim.duration();
            match s.anim.kind {
                AnimKind::Walk => {
                    // At full speed the cycle plays at the mod blueprint's rate (baked into the GLB), unless the
                    // feet would then cover more ground than the unit (a big model such as the commander):
                    // then slower, to keep them planted. Reversing plays it backwards.
                    let frac = (forward / top).clamp(-1.5, 1.5);
                    let full = if s.stride > 0.0 { (top * dur / s.stride).min(1.0) } else { 1.0 };
                    if dur > 0.0 {
                        s.time = (s.time + dt * frac * full).rem_euclid(dur);
                    }
                    let target = if frac.abs() > 0.05 { 1.0 } else { 0.0 };
                    s.weight += (target - s.weight).clamp(-dt * 4.0, dt * 6.0);
                }
                AnimKind::Upgrade => {
                    s.time = up.map_or(0.0, |p| p.0.clamp(0.0, 1.0) * dur);
                    s.weight = if up.is_some() { 1.0 } else { 0.0 };
                }
                AnimKind::Work => {
                    // Mass extractors pump at the share of energy their owner gets; paused they freeze.
                    if !paused && !uc && up.is_none() && dur > 0.0 {
                        let power = game.players.get(u.owner as usize).map_or(1.0, |p| p.power.clamp(0.0, 1.0));
                        s.time = (s.time + dt * power).rem_euclid(dur);
                    }
                    s.weight = if uc { 0.0 } else { 1.0 };
                }
                AnimKind::Open => {
                    // Engineers stand folded (first frame) and unfold to build.
                    toward(s, working, 1.0);
                    s.weight = if uc { 0.0 } else { 1.0 };
                }
                AnimKind::Doors => {
                    // Opens to the middle and holds there while working; when done it plays on (at 1.5 / 2 of
                    // the opening rate, as SupCom does) to the end, which is the closed start pose again.
                    let half = dur * 0.5;
                    if working && s.time <= half {
                        s.time = (s.time + dt).min(half);
                    } else if s.time > 0.0 {
                        s.time += dt * 0.75;
                        if s.time >= dur {
                            s.time = 0.0;
                        }
                    }
                    s.weight = if s.time > 0.0 { 1.0 } else { 0.0 };
                }
                AnimKind::Unpack => {
                    let target = weapons.is_some_and(|w| w.list.iter().any(|st| st.target != 0));
                    toward(s, target && !moving, 1.0);
                }
                AnimKind::Land => toward(s, moving, 1.0),
                AnimKind::Water => toward(s, map.h(i.cur.0) < map.water_at(i.cur.0), 1.0),
                AnimKind::Rolloff => {
                    // Out, held while the finished unit leaves, then back.
                    let building = factory.map_or(0, |f| f.building);
                    if s.mark != 0 && building != s.mark && s.seq < 0.0 {
                        s.seq = 0.0;
                    }
                    s.mark = building;
                    const HOLD: f32 = 1.0;
                    if s.seq >= 0.0 {
                        s.seq += dt;
                        s.time = if s.seq < dur { s.seq } else if s.seq < dur + HOLD { dur } else { (2.0 * dur + HOLD - s.seq).max(0.0) };
                        if s.seq >= 2.0 * dur + HOLD {
                            s.seq = -1.0;
                        }
                    }
                    s.weight = if s.time > 0.0 { 1.0 } else { 0.0 };
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

/// The unit entity a model part belongs to (the parent of its model root).
fn unit_of(part: Entity, parents: &Query<&ChildOf>, roots: &Query<(), With<ModelRoot>>) -> Option<Entity> {
    let mut cur = part;
    for _ in 0..12 {
        let p = parents.get(cur).ok()?;
        if roots.get(cur).is_ok() {
            return Some(p.parent());
        }
        cur = p.parent();
    }
    None
}

/// Model animations, turrets, barrels and spinning parts. Aim turns a part on top of its animated pose
/// (the trooper's torso and gun arm are both), as SupCom layers aim manipulators over animations.
#[allow(clippy::type_complexity)]
fn animate_parts(
    time: Res<Time>,
    units: Query<(&Interp, Option<&Weapons>, Option<&Animator>)>,
    parents: Query<&ChildOf>,
    roots: Query<(), With<ModelRoot>>,
    mut aims: Query<(Entity, &AimPart, Option<&AnimPart>, &mut Transform), Without<SpinPart>>,
    mut animated: Query<(Entity, &AnimPart, &mut Transform), (Without<AimPart>, Without<SpinPart>)>,
    mut spins: Query<(&SpinPart, &mut Transform), Without<AimPart>>,
) {
    for (e, part, mut t) in &mut animated {
        let animator = unit_of(e, &parents, &roots).and_then(|u| units.get(u).ok()).and_then(|(_, _, a)| a);
        let want = part.pose(animator);
        if *t != want {
            *t = want;
        }
    }
    for (e, aim, anim_part, mut t) in &mut aims {
        let Some((interp, Some(w), animator)) = unit_of(e, &parents, &roots).and_then(|u| units.get(u).ok()) else { continue };
        let Some(st) = w.list.get(aim.weapon) else { continue };
        let q = if aim.pitch { Quat::from_rotation_x(st.pitch.clamp(-0.3, 1.4)) } else { Quat::from_rotation_y(-wrap(st.yaw - interp.yaw)) };
        let base = anim_part.map_or(aim.rest, |p| p.pose(animator));
        *t = base * Transform::from_rotation(q);
    }
    let secs = time.elapsed_secs();
    for (s, mut t) in &mut spins {
        *t = s.rest * Transform::from_rotation(Quat::from_axis_angle(s.axis, (secs * s.speed.to_radians()) % std::f32::consts::TAU));
    }
}

#[derive(Component)]
struct WreckVisual;

/// The remembered look of an enemy building that is out of sight (SupCom style): its model as last seen,
/// kept where it stood until the spot is seen again (`intel::IntelMemory`, also after it was destroyed).
#[derive(Component)]
struct GhostVisual {
    id: u32,
    def: DefId,
    owner: u8,
    built: Option<f32>,
    yaw: f32,
    dim: bool,
}

#[allow(clippy::too_many_arguments)]
fn ghost_visuals(
    mut commands: Commands,
    mem: Res<intel::IntelMemory>,
    defs: Res<Defs>,
    game: Res<Game>,
    settings: Res<crate::settings::Settings>,
    q: Query<(Entity, &GhostVisual)>,
    mut cache: ResMut<ModelCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Graphics setting: without ghost models every one of them is despawned and none made.
    let want = |g: &intel::Ghost| settings.ghost_models && g.structure && !g.seen && g.def.is_some();
    let dim = |g: &intel::Ghost| g.gone && defs.rules.ghosts_show_destroyed;
    let mut shown = std::collections::HashSet::new();
    for (e, gv) in &q {
        let keep = mem.ghosts.get(&gv.id).is_some_and(|g| want(g) && g.def == Some(gv.def) && g.owner == gv.owner && g.built == gv.built && g.yaw == gv.yaw && dim(g) == gv.dim);
        if keep {
            shown.insert(gv.id);
        } else {
            commands.entity(e).despawn();
        }
    }
    let mut fresh: Vec<&intel::Ghost> = mem.ghosts.values().filter(|g| want(g) && !shown.contains(&g.id)).collect();
    fresh.sort_by_key(|g| g.id);
    for g in fresh.into_iter().take(12) {
        let def = g.def.unwrap();
        let d = defs.get(def);
        let (ci, rgb) = owner_color(&defs, &game, g.owner);
        let root = spawn_model_dim(&mut commands, &mut cache, &defs, def, ci, rgb, g.built.is_some(), None, dim(g), &mut meshes, &mut images, &mut materials);
        let scale = match g.built {
            Some(p) if d.is_structure() => Vec3::new(1.0, 0.25 + 0.75 * p.clamp(0.0, 1.0), 1.0),
            _ => Vec3::ONE,
        };
        let t = Transform::from_translation(g.pos.at(g.y)).with_rotation(yaw_quat(g.yaw)).with_scale(scale);
        commands
            .spawn((GhostVisual { id: g.id, def, owner: g.owner, built: g.built, yaw: g.yaw, dim: dim(g) }, t, Visibility::Visible, GameScene))
            .add_child(root);
    }
}

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
    // (Pieces of debris are drawn by `debris::debris_visuals`.)
    for (e, w) in fresh.iter().filter(|(_, w)| w.piece <= 0.0).take(10) {
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
        app.add_plugins(surface::SurfacePlugin);
        app.add_systems(Startup, sound::load);
        app.add_systems(
            Update,
            (tick_sync, terrain::terrain_sync, terrain::map_edge_sync, flora::flora_sync, intel::update_intel, unit_visuals, wreck_visuals, ghost_visuals, debris::debris_visuals, unit_transforms, drive_anims, animate_parts, fx::projectile_visuals, fx::spawn_fx, fx::update_fx, light_follow)
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
