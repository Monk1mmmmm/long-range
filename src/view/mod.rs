//! Presentation layer: reads the simulation components (identical on host and clients),
//! draws them in 3D and turns player input into `Command`s.
use crate::data::{DefId, Defs};
use crate::map::*;
use crate::sim::*;
use crate::sim::Command;
use bevy::prelude::*;
use std::collections::HashMap;
use std::f32::consts::FRAC_PI_2;

pub mod bench;
pub mod cursor;
pub mod debris;
pub mod decimate;
pub mod flora;
pub mod grass;
pub mod fx;
pub mod hud;
pub mod icons;
pub mod intel;
pub mod marks;
pub mod models;
pub mod perf;
pub mod sky;
pub mod smoke;
pub mod sound;
pub mod surface;
pub mod terrain;
pub mod water;
pub mod weather;

use models::*;
use perf::vmark;
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
    /// Transports: click the drop point of a ferry route (shift: add points); the beacon is where they
    /// are. Other units: click a beacon to wait there for the ferry.
    Ferry,
    Place(DefId),
}

/// A land transport about to be queued with riders: the factory type it is for, the transport and
/// the riders picked so far.
#[derive(Clone, Debug, PartialEq)]
pub struct Loadout {
    pub factory: DefId,
    pub host: DefId,
    pub cargo: Vec<DefId>,
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
    /// 0 = left button (mode order), 1 = right button, 2 = right button drawing a field work line.
    pub pathing: Option<u8>,
    pub path: Vec<V2>,
    pub last_click: (f64, DefId),
    pub last_group: (f64, usize),
    pub idle_cycle: usize,
    pub idle_click: f64,
    /// Idle factories button: next one to pick, click time + type of the last click, type list open.
    pub fac_cycle: usize,
    pub fac_click: (f64, DefId),
    pub fac_open: bool,
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
    /// The loadout picker of a land transport, open above the factory panel.
    pub loadout: Option<Loadout>,
    /// The last loadout queued for each transport type, offered again when its picker opens.
    pub last_loadout: HashMap<DefId, Vec<DefId>>,
    /// Factory build menu: selected tab (`hud::BUILD_TABS`).
    pub build_tab: usize,
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
    /// Mass overflow warning: storage full while income exceeds spending
    /// (overflowing now, `Time::elapsed` of the last warning sound).
    pub mass_overflow: (bool, f64),
    /// Height of the screen in egui points, from the last HUD pass (model level of detail).
    pub screen_h: f32,
    /// Game tick of the last automatic garrisoning pass (`hud::auto_garrison`).
    pub auto_garrison_tick: u32,
    /// Playing back a replay: the controls the HUD shows (`hud::replay_bar`), shared with the session.
    pub replay: Option<ReplayBar>,
    /// Game speed chosen by the host (a game of AIs only); read by the session.
    pub host_speed: f32,
    /// The session allows the speed control this frame (host, every player an AI).
    pub host_speed_ui: bool,
}

/// Replay controls. The session fills in where the playback is; the HUD sets the `want_*` requests.
#[derive(Clone, Default, Debug)]
pub struct ReplayBar {
    pub tick: u32,
    pub first: u32,
    pub end: u32,
    pub speed: f32,
    pub paused: bool,
    /// Jumping to another tick (simulating there without effects).
    pub seeking: bool,
    /// First tick whose state differs from the recording.
    pub desync: Option<u32>,
    /// The time line being dragged: the tick under the handle.
    pub drag: Option<u32>,
    pub want_seek: Option<u32>,
    pub want_speed: Option<f32>,
    /// See the game as this player (`sim::OBSERVER`: everything).
    pub want_vision: Option<u8>,
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
            idle_click: -10.0,
            fac_cycle: 0,
            fac_click: (-10.0, DefId::MAX),
            fac_open: false,
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
            loadout: None,
            last_loadout: HashMap::new(),
            build_tab: 0,
            terra_delta: 0.0,
            last_sel: vec![],
            paused: None,
            pause_toggle: false,
            net_note: None,
            mass_overflow: (false, -100.0),
            screen_h: 720.0,
            replay: None,
            auto_garrison_tick: 0,
            host_speed: 1.0,
            host_speed_ui: false,
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
    world.insert_resource(marks::Marks::default());
    world.insert_resource(intel::IntelMemory::default());
    world.insert_resource(intel::FogGrid::new(map.dims()));
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
                let mut flora = flora::FloraView::new(&defs, &mut materials, Some(fog), shown, detail);
                flora.version = version;
                // Trees already felled lie on the ground from the start.
                if let Some(f) = world.get_resource::<crate::sim::Features>() {
                    flora.fall = f.list.iter().map(|s| s.fall).collect();
                }
                flora::spawn_flora(&mut world.commands(), &map, &defs, &mut flora, &mut meshes);
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

/// A building on a slope: the lowest ground under its footprint lies `depth` below its base.
#[derive(Component)]
struct Foundation {
    depth: f32,
}

/// Shows the skirt of a new building model on a slope, stretched from the model's base down to the ground
/// (`models::Skirt`): its walls run on down as they are, the model itself is not stretched.
fn foundation_fit(mut skirts: Query<(&ChildOf, &mut Transform, &mut Visibility), Added<models::Skirt>>, roots: Query<(&ChildOf, &Transform), Without<models::Skirt>>, found: Query<&Foundation>) {
    for (c, mut t, mut vis) in &mut skirts {
        let Ok((unit, rt)) = roots.get(c.parent()) else { continue };
        if let Ok(f) = found.get(unit.parent()) {
            t.scale.y = f.depth / rt.scale.y.max(1e-3);
            *vis = Visibility::Inherited;
        }
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
    map: Res<MapData>,
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
        cache.variant = u.id;
        let root = spawn_model(&mut commands, &mut cache, &defs, u.def, ci, rgb, uc, None, &mut meshes, &mut images, &mut materials);
        let s = (b.pos, b.y, b.yaw);
        commands
            .entity(e)
            .insert((Interp { prev: s, cur: s, pos: b.pos, y: b.y, yaw: b.yaw }, Transform::from_translation(b.p3()).with_rotation(yaw_quat(b.yaw)), Visibility::Hidden))
            .add_child(root);
        // On a slope the low side of a building would hang in the air: its walls are carried down to the lowest
        // ground under the footprint (`foundation_fit`).
        let d = defs.get(u.def);
        if d.is_structure() && d.footprint > 0 && !d.is_fieldwork() && d.height() > 0.0 {
            let f = d.footprint;
            let (x0, y0) = foot_origin(b.pos, f);
            let mut lo = f32::MAX;
            for y in y0..=y0 + f {
                for x in x0..=x0 + f {
                    lo = lo.min(map.vertex(x.clamp(0, map.nx as i32) as usize, y.clamp(0, map.ny as i32) as usize));
                }
            }
            let depth = (b.y - lo + 0.1).min(d.height() * 2.0);
            if depth > 0.15 {
                commands.entity(e).insert(Foundation { depth });
            }
        }
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
                    cache.variant = u.id;
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
                AnimKind::Rolloff | AnimKind::GateE | AnimKind::GateW | AnimKind::GateS | AnimKind::GateN => {
                    // Out, held while the finished unit leaves, then back. A land factory's door opens
                    // only on the side the unit leaves by.
                    let building = factory.map_or(0, |f| if f.carrier != 0 { f.carrier } else { f.building });
                    let side = crate::sim::systems::exit_side(i.cur.0, factory.and_then(|f| f.rally));
                    let mine = match s.anim.kind {
                        AnimKind::GateE => side.x > 0.5,
                        AnimKind::GateW => side.x < -0.5,
                        AnimKind::GateS => side.y > 0.5,
                        AnimKind::GateN => side.y < -0.5,
                        _ => true,
                    };
                    if s.mark != 0 && building != s.mark && s.seq < 0.0 && mine {
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

/// Strategic icons replace a model from the icon zoom (`Settings::icon_dist`) on, and wherever
/// the model is smaller than this on screen (diameter in pixels).
pub const SPECK_PX: f32 = 5.0;

// **Model LOD** (setting `model_lod`, default on): unit and ghost models are hidden wherever HUD draws the
// strategic icon instead (`view::as_icon`: icon zoom, or under `SPECK_PX` on screen; `view::model_lod`). Neutral
// buildings keep models while `neutral_icons` is off. Wrecks have no icons and always keep theirs.
/// Whether the HUD shows a unit as its strategic icon rather than its model; `rpx` is the unit's
/// radius on screen (pixels).
pub fn as_icon(view: &ViewState, settings: &crate::settings::Settings, rpx: f32) -> bool {
    view.cam_dist >= settings.icon_dist.max(1.0) || rpx * 2.0 < SPECK_PX
}

/// Screen radius (egui points) of a unit of type `d` standing at `at`, with the camera of the last
/// HUD pass, the way the HUD measures it. 0 behind the camera.
pub fn screen_radius(view: &ViewState, d: &crate::data::UnitDef, at: Vec3) -> f32 {
    let c = view.vp * (at + Vec3::Y * d.height() * 0.5).extend(1.0);
    if c.w <= 0.05 {
        return 0.0;
    }
    d.radius() * view.screen_h / (2.0 * (hud::FOV * 0.5).tan() * c.w)
}

/// Level of detail for unit and ghost models (setting `model_lod`): no model where the HUD draws
/// an icon instead (the icon zoom, or a speck on screen). Neutral buildings keep theirs when the setting hides
/// their icons.
fn model_lod(view: &ViewState, settings: &crate::settings::Settings, d: &crate::data::UnitDef, owner: u8, at: Vec3) -> bool {
    let no_icon = !settings.neutral_icons && owner == NEUTRAL && d.is_structure();
    settings.model_lod && !no_icon && as_icon(view, settings, screen_radius(view, d, at))
}

/// Interpolated transforms, terrain tilt, visibility by fog of war and level of detail.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn unit_transforms(
    view: Res<ViewState>,
    time: Res<Time>,
    game: Res<Game>,
    defs: Res<Defs>,
    map: Res<MapData>,
    settings: Res<crate::settings::Settings>,
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
        let visible = !hosted && (reveal || intel.seen & (1 << team) != 0) && !model_lod(&view, &settings, d, u.owner, t.translation);
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

// **Contact memory** (client side, `intel.rs`): radar blips of mobile units fade over `radar_fade_secs`;
// buildings once detected stay as ghosts until spot is seen again and found empty. A ghost of building
// that was seen keeps its model (as last seen: type, owner, facing, construction height) out of sight
// (`view::ghost_visuals`), also after it was destroyed unseen: then, with preset switch
// `rules.ghosts_show_destroyed` (default on), its model is drawn dark (`ModelCache::dim_mat`) and icon faded. Graphics
// setting "Remembered buildings: Show models in fog" (`Settings::ghost_models`, default on) turns these models off:
// out of sight only icons remain. Right-clicking a
// ghost issues ground fire at its position.
#[allow(clippy::too_many_arguments)]
fn ghost_visuals(
    mut commands: Commands,
    mem: Res<intel::IntelMemory>,
    defs: Res<Defs>,
    game: Res<Game>,
    settings: Res<crate::settings::Settings>,
    view: Res<ViewState>,
    mut q: Query<(Entity, &GhostVisual, &Transform, &mut Visibility)>,
    mut cache: ResMut<ModelCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut last: Local<(bool, (Mat4, f32, f32), Option<crate::settings::Settings>)>,
) {
    // The memory changes once a tick and the level of detail when the camera moves: frames in
    // between have nothing to do (on town maps there are thousands of remembered buildings).
    let cam = (view.vp, view.screen_h, view.cam_dist);
    let moved = cam != last.1;
    // Compared by value: the settings resource is written to (and so marked changed) by more than edits.
    let new_settings = last.2.as_ref() != Some(&*settings);
    let dirty = mem.is_changed() || new_settings || last.0;
    if !dirty && !moved {
        return;
    }
    last.1 = cam;
    if new_settings {
        last.2 = Some(settings.clone());
    }
    // Graphics setting: without ghost models every one of them is despawned and none made.
    let want = |g: &intel::Ghost| settings.ghost_models && g.structure && !g.seen && g.def.is_some();
    let dim = |g: &intel::Ghost| g.gone && defs.rules.ghosts_show_destroyed;
    let mut shown = std::collections::HashSet::new();
    let lod_vis = |def: DefId, owner: u8, at: Vec3| if model_lod(&view, &settings, defs.get(def), owner, at) { Visibility::Hidden } else { Visibility::Visible };
    for (e, gv, t, mut vis) in &mut q {
        let keep = !dirty || mem.ghosts.get(&gv.id).is_some_and(|g| want(g) && g.def == Some(gv.def) && g.owner == gv.owner && g.built == gv.built && g.yaw == gv.yaw && dim(g) == gv.dim);
        if keep {
            shown.insert(gv.id);
            let v = lod_vis(gv.def, gv.owner, t.translation);
            if *vis != v {
                *vis = v;
            }
        } else {
            commands.entity(e).despawn();
        }
    }
    if !dirty {
        return;
    }
    let mut fresh: Vec<&intel::Ghost> = mem.ghosts.values().filter(|g| want(g) && !shown.contains(&g.id)).collect();
    fresh.sort_by_key(|g| g.id);
    // More than a frame's worth to make: come back next frame.
    last.0 = fresh.len() > 12;
    for g in fresh.into_iter().take(12) {
        let def = g.def.unwrap();
        let d = defs.get(def);
        let (ci, rgb) = owner_color(&defs, &game, g.owner);
        cache.variant = g.id;
        let root = spawn_model_dim(&mut commands, &mut cache, &defs, def, ci, rgb, g.built.is_some(), None, dim(g), &mut meshes, &mut images, &mut materials);
        let scale = match g.built {
            Some(p) if d.is_structure() => Vec3::new(1.0, 0.25 + 0.75 * p.clamp(0.0, 1.0), 1.0),
            _ => Vec3::ONE,
        };
        let t = Transform::from_translation(g.pos.at(g.y)).with_rotation(yaw_quat(g.yaw)).with_scale(scale);
        commands
            .spawn((GhostVisual { id: g.id, def, owner: g.owner, built: g.built, yaw: g.yaw, dim: dim(g) }, t, lod_vis(def, g.owner, t.translation), GameScene))
            .add_child(root);
    }
}

/// A whole wreck's resting orientation: on the ground's slope, collapsed slightly askew.
fn wreck_rot(map: &MapData, w: &Wreck) -> Quat {
    Quat::from_rotation_arc(Vec3::Y, map.normal(w.pos.x, w.pos.y)) * yaw_quat(w.yaw) * Quat::from_rotation_z(((w.id % 7) as f32 - 3.0) * 0.04)
}

#[allow(clippy::too_many_arguments)]
fn wreck_visuals(
    mut commands: Commands,
    fresh: Query<(Entity, &Wreck), Without<WreckVisual>>,
    mut placed: Query<(&Wreck, &mut Transform), With<WreckVisual>>,
    time: Res<Time>,
    defs: Res<Defs>,
    map: Res<MapData>,
    mut cache: ResMut<ModelCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    settings: Res<crate::settings::Settings>,
) {
    // (Pieces of debris are drawn by `debris::debris_visuals`.)
    for (e, w) in fresh.iter().filter(|(_, w)| w.piece <= 0.0).take(10) {
        let d = defs.get(w.def);
        let mat = if d.model.mesh.is_some() {
            cache.wreck_mat(&defs, w.def, &mut images, &mut materials)
        } else {
            cache.flat_mat(&crate::data::ColorSpec::Rgb([0.2, 0.18, 0.17]), 255, [0.2; 3], &mut materials)
        };
        // High building detail: a ruin with its top broken off and holes burnt through, not a sunk wreck.
        let ruin = settings.building_detail >= 2 && d.is_structure() && d.model.mesh.is_some();
        let root = if ruin {
            let rubble = spawn_rubble(&mut commands, &mut cache, d.size, w.id, &mut meshes, &mut materials);
            commands.entity(e).add_children(&rubble);
            spawn_ruin(&mut commands, &mut cache, &defs, w.def, w.id, &mut meshes, &mut images, &mut materials)
        } else {
            spawn_model(&mut commands, &mut cache, &defs, w.def, 255, [0.3, 0.28, 0.26], false, Some(mat), &mut meshes, &mut images, &mut materials)
        };
        // Collapsed and slightly askew.
        let squash = if ruin { 1.0 } else if d.is_structure() { 0.35 } else { 0.8 };
        let rot = wreck_rot(&map, w);
        commands
            .entity(e)
            .insert((WreckVisual, Transform::from_translation(w.pos.at(w.y)).with_rotation(rot).with_scale(Vec3::new(1.0, squash, 1.0)), Visibility::Visible))
            .add_child(root);
    }
    // Blasts shove wrecks about (`sim::systems::push_wrecks`): follow the slide smoothly, jump
    // when a rollback or a jump in a replay moved one far.
    let k = 1.0 - (-14.0 * time.delta_secs()).exp();
    for (w, mut t) in &mut placed {
        if w.piece > 0.0 {
            continue;
        }
        let want = w.pos.at(w.y);
        let d = t.translation.distance(want);
        if d < 1e-3 {
            continue;
        }
        t.translation = if d > 4.0 { want } else { t.translation.lerp(want, k) };
        t.rotation = wreck_rot(&map, w);
    }
}

/// Camera distances (`ViewState::cam_dist`) below which glass and bare metal show their reflections
/// (ultra building detail), and above which they stop again: only a close look shows them.
const GLOSS_NEAR: f32 = 38.0;
const GLOSS_FAR: f32 = 46.0;

/// Swaps glass and metal between their glossy and plain materials as the camera comes close or goes.
fn gloss_sync(settings: Res<crate::settings::Settings>, view: Res<ViewState>, mut cache: ResMut<ModelCache>, mut parts: Query<&mut MeshMaterial3d<StandardMaterial>>) {
    let close = settings.building_detail >= 3 && if cache.close { view.cam_dist < GLOSS_FAR } else { view.cam_dist < GLOSS_NEAR };
    if cache.close != close {
        cache.set_close(close, &mut parts);
    }
}

/// Tells the model cache which building detail is set (`Settings::building_detail`).
fn model_detail_sync(settings: Res<crate::settings::Settings>, map: Option<Res<MapData>>, mut cache: ResMut<ModelCache>) {
    if cache.detail != settings.building_detail {
        cache.detail = settings.building_detail;
    }
    let low = settings.building_detail < 2;
    if cache.low_poly != low {
        cache.low_poly = low;
    }
    if let Some(map) = map {
        cache.set_theme(&map.theme);
    }
}

/// Depth-of-field blur of what is far beyond the point looked at (`Settings::distance_blur`): the focus sits a
/// little in front of the camera's target, so the far ground and the horizon soften while the action stays sharp.
/// Off when the slider is 0 or the camera is far up (strategic zoom).
fn blur_sync(mut commands: Commands, settings: Res<crate::settings::Settings>, view: Res<ViewState>, cams: Query<(Entity, Has<bevy::post_process::dof::DepthOfField>), With<MainCamera>>) {
    use bevy::post_process::dof::{DepthOfField, DepthOfFieldMode};
    // OpenGL cannot compile Bevy's depth-of-field shader (it loads from a depth texture): never add the component.
    let s = if settings.uses_gl() { 0.0 } else { settings.distance_blur.clamp(0.0, 1.0) };
    for (e, has) in &cams {
        if s <= 0.0 || view.cam_dist > 500.0 {
            if has {
                commands.entity(e).remove::<DepthOfField>();
            }
            continue;
        }
        let focus = (view.cam_dist * 0.85).max(5.0);
        commands.entity(e).insert(DepthOfField {
            mode: DepthOfFieldMode::Gaussian,
            focal_distance: focus,
            // Blur grows with the slider; the f-number scales with the distance so the effect looks the same at any zoom.
            aperture_f_stops: 1.25 / (s * focus),
            max_depth: focus * 8.0,
            ..default()
        });
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
        app.init_resource::<LocalCommands>().init_resource::<marks::Marks>().init_resource::<sound::UiSounds>();
        app.add_plugins(surface::SurfacePlugin);
        app.add_plugins(grass::GrassPlugin);
        app.add_plugins(smoke::SmokePlugin);
        app.add_plugins(water::WaterPlugin);
        app.add_systems(Startup, sound::load);
        app.add_systems(
            Update,
            // `perf::vmark::<I>` between the systems times them while a `perf::FrameProfile` exists
            // (LR_PROFILE=1, --bench). Inner tuples need their own chain().
            (
                (vmark::<0>, tick_sync, vmark::<1>, terrain::terrain_sync, vmark::<2>, terrain::map_edge_sync, vmark::<3>, flora::flora_sync, grass::grass_sync, model_detail_sync, models::model_lod_swap, gloss_sync, vmark::<4>, intel::update_intel, vmark::<5>, unit_visuals, foundation_fit, vmark::<6>).chain(),
                (wreck_visuals, vmark::<7>, ghost_visuals, vmark::<8>, debris::debris_visuals, vmark::<9>, unit_transforms, vmark::<10>, drive_anims, vmark::<11>, animate_parts, vmark::<12>).chain(),
                (fx::projectile_visuals, vmark::<13>, fx::spawn_fx, vmark::<14>, fx::update_fx, smoke::update_billows, vmark::<15>, light_follow, blur_sync, vmark::<16>).chain(),
                (weather::lay_tracks, weather::weather_sync).chain(),
            )
                .chain()
                .run_if(in_game),
        );
        app.add_systems(Update, sound::play_ui);
        app.add_systems(Update, sky::sky_sync.run_if(in_game));
        app.add_systems(bevy_egui::EguiPrimaryContextPass, hud::ui_system.run_if(in_game));
        app.init_resource::<cursor::CursorWant>().init_resource::<cursor::CursorImages>();
        app.add_systems(Startup, cursor::take_over_from_egui);
        app.add_systems(PostUpdate, cursor::apply_cursor.after(bevy_egui::EguiPostUpdateSet::ProcessOutput));
    }
}
