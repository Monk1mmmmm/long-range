//! The simulation as ECS data: components, resources, commands and snapshots.
//! Systems live in `systems.rs`. The host runs schedule `SimTick` ten times a second;
//! clients never run it and instead apply `Snapshot`s to the same components.
use crate::data::{DefId, Defs, MoveKind};
use crate::map::*;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub mod systems;

pub const DT: f32 = 0.1;
/// Owner ids of the two non-player sides.
pub const NEUTRAL: u8 = 250;
pub const HOSTILE: u8 = 251;
/// Most players a game can have.
pub const MAX_PLAYERS: usize = 10;
pub const TEAM_NEUTRAL: u8 = 14;
pub const TEAM_HOSTILE: u8 = 15;

#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
pub struct SimTick;

// ---------------------------------------------------------------------------- orders & commands

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Debug)]
pub enum Order {
    Move(V2),
    AttackMove(V2),
    Patrol(V2),
    Attack(u32),
    /// Shell a spot; with a radius, shots are spread over the area.
    AttackGround { pos: V2, radius: f32 },
    Build { def: DefId, pos: V2 },
    /// Help build, repair or guard.
    Assist(u32),
    Reclaim(u32),
    ReclaimArea { pos: V2, radius: f32 },
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum Command {
    /// `orders` holds either one order for everybody or one order per unit.
    Orders { ids: Vec<u32>, orders: Vec<Order>, queue: bool },
    Stop { ids: Vec<u32> },
    Produce { factory: u32, def: DefId, count: i16 },
    Repeat { ids: Vec<u32>, on: bool },
    Upgrade { ids: Vec<u32> },
    SelfDestruct { ids: Vec<u32> },
    Resign,
}

#[derive(Resource, Default)]
pub struct CommandQueue(pub Vec<(u8, Command)>);

// ---------------------------------------------------------------------------- components

/// Identity of every unit and structure. `id` is the stable network id.
#[derive(Component, Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct Unit {
    pub id: u32,
    pub def: DefId,
    pub owner: u8,
}

/// Position on the ground plane, height, facing and velocity.
#[derive(Component, Clone, Copy, Serialize, Deserialize, Debug, Default)]
pub struct Body {
    pub pos: V2,
    pub y: f32,
    pub yaw: f32,
    pub vel: V2,
}
impl Body {
    pub fn p3(&self) -> Vec3 {
        self.pos.at(self.y)
    }
}

#[derive(Component, Clone, Copy, Serialize, Deserialize, Debug)]
pub struct Health(pub f32);

/// Present while a unit is still being built; value is progress 0..1.
#[derive(Component, Clone, Copy, Debug)]
pub struct UnderConstruction(pub f32);

/// Present while a structure upgrades to its `upgrade` definition.
#[derive(Component, Clone, Copy, Debug)]
pub struct Upgrading(pub f32);

#[derive(Component, Clone, Debug, Default)]
pub struct Orders(pub Vec<Order>);

#[derive(Clone, Copy, Serialize, Deserialize, Debug, Default)]
pub struct WeaponState {
    /// World yaw and pitch the weapon currently points at.
    pub yaw: f32,
    pub pitch: f32,
    /// Seconds until reloaded.
    pub cd: f32,
    /// How long the gun has been steady on target.
    pub aim: f32,
    pub burst: u8,
    pub burst_cd: f32,
    pub target: u32,
    /// Muzzle speed of the current firing solution (indirect weapons vary their charge).
    #[serde(skip)]
    pub speed: f32,
}

#[derive(Component, Clone, Debug, Default)]
pub struct Weapons {
    pub list: Vec<WeaponState>,
    /// Target requested by an attack order.
    pub focus: u32,
    /// Ground-attack point and scatter radius.
    pub ground: Option<(V2, f32)>,
}

/// Anything with build power. `work` is the unit or wreck being worked on.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Builder {
    pub work: u32,
}

#[derive(Component, Clone, Serialize, Deserialize, Debug, Default)]
pub struct Factory {
    pub queue: Vec<DefId>,
    pub repeat: bool,
    pub rally: Option<V2>,
    /// Unit currently under construction inside.
    pub building: u32,
}

/// The unit is still inside the factory with this id.
#[derive(Component, Clone, Copy, Debug)]
pub struct Hosted(pub u32);

/// Team bitmasks (bit = team number, up to 14 player teams plus neutral and hostile):
/// who sees this unit, who has it on radar.
#[derive(Component, Clone, Copy, Serialize, Deserialize, Debug, Default)]
pub struct Intel {
    pub seen: u16,
    pub radar: u16,
}

#[derive(Component)]
pub struct Structure;

/// Host-only movement state.
#[derive(Component, Default)]
pub struct Mover {
    pub goal: Option<V2>,
    pub path: Vec<V2>,
    pub path_goal: V2,
    pub speed: f32,
    pub reversing: bool,
    pub stuck: u16,
    /// Planes: fly straight for this long before turning back.
    pub extend: f32,
    /// Planes circle here when idle.
    pub anchor: V2,
}

#[derive(Component, Clone, Serialize, Deserialize, Debug)]
pub struct Wreck {
    pub id: u32,
    pub def: DefId,
    pub mass: f32,
    pub pos: V2,
    pub y: f32,
    pub yaw: f32,
}

pub const PROJ_SHELL: u8 = 0;
pub const PROJ_MISSILE: u8 = 1;
pub const PROJ_BOMB: u8 = 2;

#[derive(Component, Clone, Serialize, Deserialize, Debug)]
pub struct Projectile {
    pub id: u32,
    pub pos: V3,
    pub vel: V3,
    pub kind: u8,
    pub owner: u8,
    pub damage: f32,
    pub dtype: u8,
    pub radius: f32,
    pub life: f32,
    pub target: u32,
    pub turn: f32,
    pub hits_land: bool,
    pub hits_air: bool,
}

// ---------------------------------------------------------------------------- resources

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct Player {
    pub name: String,
    pub team: u8,
    pub color: u8,
    pub faction: u8,
    pub is_ai: bool,
    pub alive: bool,
    pub mass: f32,
    pub energy: f32,
    pub max_mass: f32,
    pub max_energy: f32,
    pub income_m: f32,
    pub income_e: f32,
    pub spend_m: f32,
    pub spend_e: f32,
    pub power_out: bool,
    pub start: V2,
}

#[derive(Resource, Clone, Serialize, Deserialize, Default)]
pub struct Game {
    pub tick: u32,
    pub players: Vec<Player>,
    pub winner: Option<u8>,
    pub next_id: u32,
}

impl Game {
    pub fn team_of(&self, owner: u8) -> u8 {
        match owner {
            NEUTRAL => TEAM_NEUTRAL,
            HOSTILE => TEAM_HOSTILE,
            o => self.players.get(o as usize).map_or(TEAM_NEUTRAL, |p| p.team),
        }
    }
    pub fn alloc(&mut self) -> u32 {
        self.next_id += 1;
        self.next_id
    }
}

/// Would units of team `a` shoot at team `b` on their own?
pub fn hostile(a: u8, b: u8) -> bool {
    a != b && a != TEAM_NEUTRAL && b != TEAM_NEUTRAL
}

pub const FX_BLAST: u8 = 0;
pub const FX_MUZZLE: u8 = 1;
pub const FX_DEATH: u8 = 2;

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
pub struct Fx {
    pub pos: V3,
    pub kind: u8,
    pub size: f32,
}

/// Visual events. `tick` holds this tick's events (for snapshots), `pending` is drained by the view.
#[derive(Resource, Default)]
pub struct FxLog {
    pub tick: Vec<Fx>,
    pub pending: Vec<Fx>,
}
impl FxLog {
    pub fn push(&mut self, pos: Vec3, kind: u8, size: f32) {
        let f = Fx { pos: pos.into(), kind, size };
        self.tick.push(f);
        self.pending.push(f);
    }
}

/// Occupancy grid for building and pathfinding: 0 free, 1 terrain, 2 structure.
#[derive(Resource, Default)]
pub struct Nav {
    pub occ: Vec<u8>,
    pub dirty: bool,
    pub scratch: PathScratch,
    pub paths_left: i32,
}

impl Nav {
    pub fn stamp(&mut self, map: &MapData, pos: V2, foot: i32, value: u8) {
        let (x0, y0) = foot_origin(pos, foot);
        for y in y0..y0 + foot {
            for x in x0..x0 + foot {
                if map.inside(x, y) {
                    self.occ[y as usize * map.n + x as usize] = value;
                }
            }
        }
    }
    pub fn at(&self, map: &MapData, p: V2) -> u8 {
        let (cx, cy) = map.cell_of(p);
        if map.inside(cx, cy) {
            self.occ[cy as usize * map.n + cx as usize]
        } else {
            1
        }
    }
}

/// Snaps a build position to the grid (and to the nearest deposit for extractors).
pub fn snap(defs: &Defs, map: &MapData, d: DefId, p: V2) -> V2 {
    let df = defs.get(d);
    if df.needs_deposit {
        let mut best = (f32::MAX, p);
        for dep in &map.deposits {
            let dd = dep.dist(p);
            if dd < best.0 {
                best = (dd, *dep);
            }
        }
        if best.0 < 8.0 {
            return best.1;
        }
    }
    if df.footprint % 2 == 1 {
        v2(((p.x / CELL).floor() + 0.5) * CELL, ((p.y / CELL).floor() + 0.5) * CELL)
    } else {
        v2((p.x / CELL).round() * CELL, (p.y / CELL).round() * CELL)
    }
}

pub fn can_place(defs: &Defs, map: &MapData, occ: &[u8], d: DefId, p: V2) -> bool {
    let df = defs.get(d);
    let f = df.footprint;
    if f <= 0 || occ.is_empty() {
        return false;
    }
    let (x0, y0) = foot_origin(p, f);
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for y in y0..y0 + f {
        for x in x0..x0 + f {
            if !map.inside(x, y) || occ[y as usize * map.n + x as usize] != 0 {
                return false;
            }
        }
    }
    for y in y0..=y0 + f {
        for x in x0..=x0 + f {
            let h = map.vertex(x.max(0) as usize, y.max(0) as usize);
            lo = lo.min(h);
            hi = hi.max(h);
        }
    }
    // Big buildings need reasonably level ground.
    if hi - lo > 1.2 + f as f32 * 0.5 {
        return false;
    }
    let half = f as f32 * CELL * 0.5;
    let on_dep = map.deposits.iter().any(|dp| (dp.x - p.x).abs() < half && (dp.y - p.y).abs() < half);
    if df.needs_deposit {
        map.deposits.iter().any(|dp| dp.dist(p) < 0.5)
    } else {
        !on_dep
    }
}

/// Per-tick read-only view of every unit, so systems can look at other units
/// without fighting over component borrows.
#[derive(Clone, Copy, Debug)]
pub struct Cen {
    pub e: Entity,
    pub id: u32,
    pub def: DefId,
    pub owner: u8,
    pub team: u8,
    pub pos: V2,
    pub y: f32,
    pub yaw: f32,
    pub vel: V2,
    pub hp: f32,
    pub max_hp: f32,
    pub done: bool,
    pub hosted: bool,
    pub air: bool,
    pub structure: bool,
    pub radius: f32,
    pub height: f32,
    pub seen: u16,
    pub radar: u16,
    pub work: u32,
    pub building: u32,
    pub upgrading: bool,
}
impl Cen {
    pub fn p3(&self) -> Vec3 {
        self.pos.at(self.y)
    }
    pub fn center(&self) -> Vec3 {
        self.pos.at(self.y + self.height * 0.5)
    }
    pub fn known_to(&self, team: u8) -> bool {
        (self.seen | self.radar) & (1 << team) != 0
    }
}

#[derive(Resource, Default)]
pub struct Census {
    pub list: Vec<Cen>,
    pub index: HashMap<u32, usize>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

const GRID: f32 = 16.0;

impl Census {
    pub fn clear(&mut self) {
        self.list.clear();
        self.index.clear();
        self.grid.clear();
    }
    pub fn push(&mut self, c: Cen) {
        let i = self.list.len();
        self.index.insert(c.id, i);
        self.grid.entry(((c.pos.x / GRID) as i32, (c.pos.y / GRID) as i32)).or_default().push(i as u32);
        self.list.push(c);
    }
    pub fn get(&self, id: u32) -> Option<&Cen> {
        if id == 0 {
            return None;
        }
        self.index.get(&id).map(|i| &self.list[*i])
    }
    /// Calls `f` for every unit whose centre is within `r` of `p` (2D).
    pub fn near(&self, p: V2, r: f32, mut f: impl FnMut(&Cen)) {
        if r > 80.0 {
            let r2 = r * r;
            for c in &self.list {
                if (c.pos - p).len2() <= r2 {
                    f(c);
                }
            }
            return;
        }
        let (x0, x1) = (((p.x - r) / GRID) as i32, ((p.x + r) / GRID) as i32);
        let (y0, y1) = (((p.y - r) / GRID) as i32, ((p.y + r) / GRID) as i32);
        let r2 = r * r;
        for gy in y0..=y1 {
            for gx in x0..=x1 {
                if let Some(v) = self.grid.get(&(gx, gy)) {
                    for i in v {
                        let c = &self.list[*i as usize];
                        if (c.pos - p).len2() <= r2 {
                            f(c);
                        }
                    }
                }
            }
        }
    }
    pub fn valid_target(&self, c: &Cen, team: u8, land: bool, air: bool) -> bool {
        c.hp > 0.0 && !c.hosted && hostile(team, c.team) && c.known_to(team) && if c.air { air } else { land }
    }
    pub fn nearest_enemy(&self, pos: V2, team: u8, range: f32, min_range: f32, land: bool, air: bool) -> Option<&Cen> {
        let mut best: Option<(f32, usize)> = None;
        let m2 = min_range * min_range;
        self.near(pos, range, |c| {
            let d2 = (c.pos - pos).len2();
            if d2 >= m2 && best.map_or(true, |b| d2 < b.0) && self.valid_target(c, team, land, air) {
                best = Some((d2, self.index[&c.id]));
            }
        });
        best.map(|b| &self.list[b.1])
    }
}

/// Wrecks at the start of the tick, for reclaim orders.
#[derive(Resource, Default)]
pub struct WreckList(pub Vec<(u32, V2, f32)>);

/// Effects on other entities are queued and applied by dedicated systems.
#[derive(Resource, Default)]
pub struct Effects {
    /// (unit id, raw damage, damage type)
    pub damage: Vec<(u32, f32, usize)>,
    /// (wreck id, amount, player)
    pub reclaim: Vec<(u32, f32, u8)>,
    /// Sites created this tick: (def, position).
    pub sites: Vec<(DefId, V2, u32)>,
}

#[derive(Resource)]
pub struct SimRng(pub Rng);

// ---------------------------------------------------------------------------- spawning

pub struct PlayerSetup {
    pub name: String,
    pub team: u8,
    pub is_ai: bool,
    pub slot: usize,
    pub faction: u8,
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_unit(commands: &mut Commands, defs: &Defs, game: &mut Game, map: &MapData, def: DefId, owner: u8, pos: V2, yaw: f32, complete: bool, hosted: u32) -> u32 {
    let d = defs.get(def);
    let id = game.alloc();
    let team = game.team_of(owner);
    let y = map.h(pos) + d.mv.altitude;
    let mut e = commands.spawn((
        Unit { id, def, owner },
        Body { pos, y, yaw, vel: v2(0.0, 0.0) },
        Health(if complete { d.hp } else { d.hp * 0.05 }),
        Orders::default(),
        Intel { seen: 1 << team, radar: 1 << team },
    ));
    if !complete {
        e.insert(UnderConstruction(0.0));
    }
    if d.armed() {
        e.insert(Weapons { list: d.weapons.iter().map(|_| WeaponState { yaw, ..default() }).collect(), ..default() });
    }
    if d.build.is_some() {
        e.insert(Builder::default());
    }
    if d.is_factory() {
        e.insert(Factory::default());
    }
    if hosted != 0 {
        e.insert(Hosted(hosted));
    }
    if d.mv.kind == MoveKind::None {
        e.insert(Structure);
    } else {
        e.insert(Mover { anchor: pos, path_goal: pos, ..default() });
    }
    id
}

/// Sets up a fresh game world: resources, commanders and map props.
pub fn setup_game(world: &mut World, map: MapData, setup: &[PlayerSetup]) {
    clear_game(world);
    let defs = world.resource::<Defs>().clone();
    let mut game = Game::default();
    let mut nav = Nav { occ: map.blocked.clone(), ..default() };
    let center = map.center();
    let mut spawns: Vec<(DefId, u8, V2, f32)> = vec![];
    for (i, p) in setup.iter().enumerate() {
        let start = map.starts[p.slot % map.starts.len().max(1)];
        game.players.push(Player {
            name: p.name.clone(),
            team: p.team,
            color: p.slot as u8,
            faction: p.faction,
            is_ai: p.is_ai,
            alive: true,
            mass: defs.rules.start_mass,
            energy: defs.rules.start_energy,
            max_mass: defs.rules.start_mass,
            max_energy: defs.rules.start_energy,
            start,
            ..default()
        });
        if let Some(c) = defs.factions[p.faction as usize].commander {
            spawns.push((c, i as u8, start, (center - start).angle()));
        }
    }
    for prop in &map.props {
        if let Some(d) = defs.id(&prop.def) {
            let owner = if prop.owner == PROP_HOSTILE { HOSTILE } else { NEUTRAL };
            let pos = if defs.get(d).is_structure() { snap(&defs, &map, d, prop.pos) } else { prop.pos };
            if defs.get(d).is_structure() {
                nav.stamp(&map, pos, defs.get(d).footprint, 2);
            }
            spawns.push((d, owner, pos, prop.yaw));
        }
    }
    let seed = map.seed;
    {
        let mut commands = world.commands();
        for (d, owner, pos, yaw) in spawns {
            spawn_unit(&mut commands, &defs, &mut game, &map, d, owner, pos, yaw, true, 0);
        }
    }
    world.flush();
    world.insert_resource(game);
    world.insert_resource(nav);
    world.insert_resource(map);
    world.insert_resource(SimRng(Rng(seed ^ 0xABCDEF123 | 1)));
    world.insert_resource(Census::default());
    world.insert_resource(WreckList::default());
    world.insert_resource(Effects::default());
    world.insert_resource(FxLog::default());
    world.insert_resource(CommandQueue::default());
}

/// Removes every simulation entity (used when leaving a game).
pub fn clear_game(world: &mut World) {
    let mut doomed: Vec<Entity> = world.query_filtered::<Entity, Or<(With<Unit>, With<Wreck>, With<Projectile>)>>().iter(world).collect();
    for e in doomed.drain(..) {
        world.despawn(e);
    }
}

// ---------------------------------------------------------------------------- snapshots

#[derive(Clone, Serialize, Deserialize)]
pub struct UnitSnap {
    pub unit: Unit,
    pub body: Body,
    pub hp: f32,
    pub built: Option<f32>,
    pub upg: Option<f32>,
    pub orders: Vec<Order>,
    pub weapons: Vec<WeaponState>,
    pub work: Option<u32>,
    pub factory: Option<Factory>,
    pub hosted: Option<u32>,
    pub intel: Intel,
}

/// Complete game state sent to clients every tick.
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Snapshot {
    pub game: Game,
    pub units: Vec<UnitSnap>,
    pub projectiles: Vec<Projectile>,
    pub wrecks: Vec<Wreck>,
    pub fx: Vec<Fx>,
}

pub fn make_snapshot(world: &mut World) -> Snapshot {
    let mut s = Snapshot { game: world.resource::<Game>().clone(), fx: world.resource::<FxLog>().tick.clone(), ..default() };
    let mut q = world.query::<(
        &Unit,
        &Body,
        &Health,
        Option<&UnderConstruction>,
        Option<&Upgrading>,
        &Orders,
        Option<&Weapons>,
        Option<&Builder>,
        Option<&Factory>,
        Option<&Hosted>,
        &Intel,
    )>();
    for (u, b, h, uc, up, o, w, bl, f, ho, i) in q.iter(world) {
        s.units.push(UnitSnap {
            unit: *u,
            body: *b,
            hp: h.0,
            built: uc.map(|x| x.0),
            upg: up.map(|x| x.0),
            orders: o.0.clone(),
            weapons: w.map(|w| w.list.clone()).unwrap_or_default(),
            work: bl.map(|b| b.work),
            factory: f.cloned(),
            hosted: ho.map(|h| h.0),
            intel: *i,
        });
    }
    let mut q = world.query::<&Projectile>();
    s.projectiles = q.iter(world).cloned().collect();
    let mut q = world.query::<&Wreck>();
    s.wrecks = q.iter(world).cloned().collect();
    s
}

/// Maps network ids to entities on a client.
#[derive(Resource, Default)]
pub struct Replicas {
    pub units: HashMap<u32, Entity>,
    pub projectiles: HashMap<u32, Entity>,
    pub wrecks: HashMap<u32, Entity>,
}

/// Client side: makes the ECS world match a snapshot.
pub fn apply_snapshot(world: &mut World, s: Snapshot) {
    let mut rep = world.remove_resource::<Replicas>().unwrap_or_default();
    let defs = world.resource::<Defs>().clone();
    let mut alive = std::collections::HashSet::new();
    for u in s.units {
        alive.insert(u.unit.id);
        let e = match rep.units.get(&u.unit.id).copied().filter(|e| world.get_entity(*e).is_ok()) {
            Some(e) => e,
            None => {
                let e = world.spawn_empty().id();
                if defs.get(u.unit.def).is_structure() {
                    world.entity_mut(e).insert(Structure);
                }
                rep.units.insert(u.unit.id, e);
                e
            }
        };
        let mut em = world.entity_mut(e);
        em.insert((u.unit, u.body, Health(u.hp), Orders(u.orders), u.intel));
        match u.built {
            Some(p) => {
                em.insert(UnderConstruction(p));
            }
            None => {
                em.remove::<UnderConstruction>();
            }
        }
        match u.upg {
            Some(p) => {
                em.insert(Upgrading(p));
            }
            None => {
                em.remove::<Upgrading>();
            }
        }
        match u.hosted {
            Some(h) => {
                em.insert(Hosted(h));
            }
            None => {
                em.remove::<Hosted>();
            }
        }
        if !u.weapons.is_empty() {
            em.insert(Weapons { list: u.weapons, ..default() });
        }
        if let Some(w) = u.work {
            em.insert(Builder { work: w });
        }
        if let Some(f) = u.factory {
            em.insert(f);
        }
    }
    rep.units.retain(|id, e| {
        if alive.contains(id) {
            true
        } else {
            if world.get_entity(*e).is_ok() {
                world.despawn(*e);
            }
            false
        }
    });
    let mut alive = std::collections::HashSet::new();
    for p in s.projectiles {
        alive.insert(p.id);
        match rep.projectiles.get(&p.id).copied().filter(|e| world.get_entity(*e).is_ok()) {
            Some(e) => {
                world.entity_mut(e).insert(p);
            }
            None => {
                let id = p.id;
                rep.projectiles.insert(id, world.spawn(p).id());
            }
        }
    }
    rep.projectiles.retain(|id, e| {
        if alive.contains(id) {
            true
        } else {
            if world.get_entity(*e).is_ok() {
                world.despawn(*e);
            }
            false
        }
    });
    let mut alive = std::collections::HashSet::new();
    for w in s.wrecks {
        alive.insert(w.id);
        match rep.wrecks.get(&w.id).copied().filter(|e| world.get_entity(*e).is_ok()) {
            Some(e) => {
                world.entity_mut(e).insert(w);
            }
            None => {
                let id = w.id;
                rep.wrecks.insert(id, world.spawn(w).id());
            }
        }
    }
    rep.wrecks.retain(|id, e| {
        if alive.contains(id) {
            true
        } else {
            if world.get_entity(*e).is_ok() {
                world.despawn(*e);
            }
            false
        }
    });
    world.insert_resource(rep);
    world.insert_resource(s.game);
    if let Some(mut log) = world.get_resource_mut::<FxLog>() {
        log.pending.extend(s.fx);
    }
}

/// Registers the simulation schedule and its systems.
pub struct SimPlugin;
impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.init_schedule(SimTick);
        app.init_resource::<CommandQueue>().init_resource::<FxLog>().init_resource::<Replicas>();
        systems::register(app);
    }
}
