//! The simulation as ECS data: components, resources, commands and snapshots.
//! Systems live in `systems.rs`. Every machine runs schedule `SimTick` for the same ticks with the
//! same commands (see `netplay.rs`); a `Snapshot` is the complete state, used to join a game, to
//! roll a client back to the last tick the host confirmed, and to repair a diverged client.
use crate::data::{DefId, Defs, MoveKind};
use crate::map::*;
use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use bevy::tasks::ParallelSlice;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;

pub mod systems;

pub const DT: f32 = 0.1;
/// Owner ids of the two non-player sides.
pub const NEUTRAL: u8 = 250;
pub const HOSTILE: u8 = 251;
/// The "player" of someone who only watches (lobby observer slots): owns nothing, gives no
/// commands, sees everything.
pub const OBSERVER: u8 = 249;
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
    /// Engineers: take over a neutral or enemy unit.
    Capture(u32),
    /// Scavengers: rebuild a wreck as a unit of their own side.
    Revive(u32),
    /// Scavengers: revive every wreck in the circle, nearest first.
    ReviveArea { pos: V2, radius: f32 },
    /// Engineers: bring the ground inside the circle to `height`.
    Terraform { pos: V2, radius: f32, height: f32 },
    /// For a transport: pick that unit up. For anything else: board that transport.
    Load(u32),
    /// Transports: pick up own units inside the circle until full.
    LoadArea { pos: V2, radius: f32 },
    /// Transports: set the cargo down spread over the circle.
    Unload { pos: V2, radius: f32 },
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum Command {
    /// `orders` holds either one order for everybody or one order per unit.
    Orders { ids: Vec<u32>, orders: Vec<Order>, queue: bool },
    Stop { ids: Vec<u32> },
    Produce { factory: u32, def: DefId, count: i16 },
    /// Moves `len` queue entries starting at `from` so that they begin at index `to` of what is left.
    QueueMove { factory: u32, from: u16, len: u16, to: u16 },
    /// Takes one entry out of a factory queue.
    Dequeue { factory: u32, index: u16 },
    Repeat { ids: Vec<u32>, on: bool },
    Upgrade { ids: Vec<u32> },
    SelfDestruct { ids: Vec<u32> },
    /// Pause or resume: builders stop working, extractors and radars switch off and stop drawing energy.
    Pause { ids: Vec<u32>, on: bool },
    Resign,
}

#[derive(Resource, Default)]
pub struct CommandQueue(pub Vec<(u8, Command)>);

/// Host only: the commands the last tick applied, AI orders included, for sending to the clients.
#[derive(Resource, Default)]
pub struct TurnLog(pub Vec<(u8, Command)>);

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
#[derive(Component, Clone, Copy, Serialize, Deserialize, Debug)]
pub struct UnderConstruction(pub f32);

/// A construction site made by reviving the wreck with this id: only scavengers work on it.
#[derive(Component, Clone, Copy, Serialize, Deserialize, Debug)]
pub struct Revival(pub u32);

/// Present on a unit somebody is capturing: who, and how far along (0..1).
#[derive(Component, Clone, Copy, Serialize, Deserialize, Debug)]
pub struct Capture {
    pub by: u8,
    pub progress: f32,
}

/// Present while a structure upgrades to its `upgrade` definition.
#[derive(Component, Clone, Copy, Serialize, Deserialize, Debug)]
pub struct Upgrading(pub f32);

#[derive(Component, Clone, Serialize, Deserialize, Debug, Default)]
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
    pub speed: f32,
    /// Bombs: sideways push given to every bomb of the current stick so that it lands on the
    /// target (the bomber is never lined up exactly).
    #[serde(default)]
    pub fix: V2,
}

#[derive(Component, Clone, Serialize, Deserialize, Debug, Default)]
pub struct Weapons {
    pub list: Vec<WeaponState>,
    /// Target requested by an attack order.
    pub focus: u32,
    /// Ground-attack point and scatter radius.
    pub ground: Option<(V2, f32)>,
}

/// Anything with build power. `work` is the unit or wreck being worked on.
#[derive(Component, Clone, Copy, Serialize, Deserialize, Debug, Default)]
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

/// The unit is inside the factory (while being built) or the transport with this id.
#[derive(Component, Clone, Copy, Serialize, Deserialize, Debug)]
pub struct Hosted(pub u32);

/// Team bitmasks (bit = team number, up to 14 player teams plus neutral and hostile):
/// who sees this unit, who has it on radar. Units with binoculars also keep where those look.
#[derive(Component, Clone, Copy, Serialize, Deserialize, Debug, Default)]
pub struct Intel {
    pub seen: u16,
    pub radar: u16,
    /// Direction of the binoculars (yaw, like `Body::yaw`).
    pub look: f32,
    /// The enemy the binoculars follow (0 = none: scanning).
    pub lock: u32,
    /// Units with a `spot_time`: how far each team has got with spotting it (`SPOTTED` = seen).
    #[serde(default)]
    pub spot: [u16; 16],
}
/// `Intel.spot` of a team that has made the unit out.
pub const SPOTTED: u16 = u16::MAX;

/// Switched off by the player: no construction work, no production, no upkeep, no radar.
#[derive(Component, Clone, Copy, Serialize, Deserialize, Debug)]
pub struct Paused;

#[derive(Component, Clone, Copy, Serialize, Deserialize, Debug)]
pub struct Structure;

/// Movement state.
#[derive(Component, Clone, Serialize, Deserialize, Default)]
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
    /// Infantry: the spot behind a tree, rock or wreck they are taking cover at (re-chosen every
    /// few ticks, cleared by a new order).
    #[serde(default)]
    pub cover: Option<V2>,
    /// Seconds left that the unit must stay put after firing a weapon with a `hold` (mortars).
    #[serde(default)]
    pub hold: f32,
}

/// A unit's wreck, or a piece of debris a building dropped (`piece` > 0).
#[derive(Component, Clone, Serialize, Deserialize, Debug, Default)]
pub struct Wreck {
    pub id: u32,
    /// The unit it is (a piece of): model, armor and how much mass each hit point is worth.
    pub def: DefId,
    pub mass: f32,
    pub pos: V2,
    pub y: f32,
    pub yaw: f32,
    /// Debris: size of the piece in world units (0 = a unit's whole wreck). Pieces are reclaimed
    /// like wrecks but never revived.
    pub piece: f32,
    /// Debris still falling (`vel` its velocity): not reclaimable or in `WreckList` until it lands.
    pub falling: bool,
    pub vel: V3,
}

impl Wreck {
    /// Footprint radius and height above the ground (what blocks sight, takes hits and gives cover).
    pub fn shape(defs: &Defs, def: DefId, piece: f32) -> (f32, f32) {
        if piece > 0.0 {
            return (piece * 0.5, piece * 0.45);
        }
        let d = defs.get(def);
        (d.radius() * 0.7, d.height() * if d.is_structure() { 0.35 } else { 0.8 })
    }
}

/// Mass of its wreck value a building has already dropped as debris (taken off its wreck and
/// off what reclaiming it gives).
#[derive(Component, Clone, Copy, Serialize, Deserialize, Debug)]
pub struct Shed(pub f32);

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
    /// Where it was fired from (on the ground plane): cover right next to the shooter does not
    /// stop its own shots (`rules.cover_grace`).
    #[serde(default)]
    pub from: V2,
    /// Sniper round: kills a rider of the garrison it hits instead of damaging the building.
    #[serde(default)]
    pub snipe: bool,
    /// The unit that fired it: never hit by its own shot or blast (nor the building it fires from).
    #[serde(default)]
    pub shooter: u32,
}

// ---------------------------------------------------------------------------- resources

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct Player {
    pub name: String,
    pub team: u8,
    pub color: u8,
    pub faction: u8,
    pub is_ai: bool,
    /// AI players: `ai::AiKind::id` (a new host restarts the AIs as what they were).
    #[serde(default)]
    pub ai_kind: u8,
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
    /// Energy upkeep asked for by running extractors, radars...
    #[serde(default)]
    pub upkeep: f32,
    /// Share of the energy demand that was met last tick (1 = no stall).
    #[serde(default)]
    pub power: f32,
    pub start: V2,
}

#[derive(Resource, Clone, Serialize, Deserialize, Default)]
pub struct Game {
    pub tick: u32,
    pub players: Vec<Player>,
    pub winner: Option<u8>,
    pub next_id: u32,
    /// `MODE_ANNIHILATION` or `MODE_POINTS`.
    #[serde(default)]
    pub mode: u8,
    /// Control points mode: the positions to hold and the points every team has left (by team).
    #[serde(default)]
    pub points: Vec<ControlPoint>,
    #[serde(default)]
    pub scores: Vec<f32>,
    /// Points every team started with (the full length of the HUD bars).
    #[serde(default)]
    pub cp_start: f32,
    /// Lobby option: every player knows the map's neutral and hostile buildings from the start
    /// (the view puts them into its contact memory; the simulation's intel is unchanged).
    #[serde(default)]
    pub reveal_props: bool,
}

pub const MODE_ANNIHILATION: u8 = 0;
pub const MODE_POINTS: u8 = 1;
/// `ControlPoint::owner` of a point nobody holds.
pub const NOBODY: u8 = 255;

/// A position teams fight over in control points mode.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct ControlPoint {
    pub pos: V2,
    /// Team holding it, or `NOBODY`.
    pub owner: u8,
    /// Team currently taking it and how far along it is (0..1).
    pub by: u8,
    pub progress: f32,
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
/// A piece of debris hit the ground (dust; drawn at the high effects level only).
pub const FX_DEBRIS: u8 = 3;

#[derive(Clone, Copy, Serialize, Deserialize, Debug)]
pub struct Fx {
    pub pos: V3,
    pub kind: u8,
    pub size: f32,
    /// Muzzle flashes: which sound the shot makes (index into `SOUNDS`).
    pub snd: u8,
}

/// Sound names. A weapon picks one with `"sound"` in its JSON; without it the weapon kind decides.
/// The last two belong to explosions and dying units.
pub const SOUNDS: [&str; 7] = ["cannon", "mg", "artillery", "missile", "bomb", "blast", "death"];

/// Visual events. `tick` holds this tick's events, `pending` is drained by the view.
#[derive(Resource, Default)]
pub struct FxLog {
    pub tick: Vec<Fx>,
    pub pending: Vec<Fx>,
}
impl FxLog {
    pub fn push(&mut self, pos: Vec3, kind: u8, size: f32) {
        self.push_snd(pos, kind, size, 0);
    }
    /// `snd`: index into `SOUNDS` for the shot's sound.
    pub fn push_snd(&mut self, pos: Vec3, kind: u8, size: f32, snd: u8) {
        let f = Fx { pos: pos.into(), kind, size, snd };
        self.tick.push(f);
        self.pending.push(f);
    }
}

/// Occupancy grid for building and pathfinding: 0 free, 1 terrain, 2 structure,
/// 3 field work (`map::OCC_WORK`: taken for building, open to traffic).
#[derive(Resource, Default)]
pub struct Nav {
    pub occ: Vec<u8>,
    pub dirty: bool,
    /// Path searches left this tick (`locomotion` plans them).
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
    /// Marks the cells of a building of type `d` standing at `pos`.
    pub fn stamp_def(&mut self, map: &MapData, d: &crate::data::UnitDef, pos: V2) {
        self.stamp(map, pos, d.footprint, if d.is_fieldwork() { crate::map::OCC_WORK } else { 2 });
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
    // Big buildings need reasonably level ground. (Extractors go on their deposit whatever shells
    // have done to it.)
    if !df.needs_deposit && hi - lo > 1.2 + f as f32 * 0.5 {
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

/// Can this unit ride in a transport?
pub fn carriable(d: &crate::data::UnitDef) -> bool {
    !d.is_structure() && !d.is_air() && d.cargo > 0 && d.transport.is_none()
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
    /// Id of the factory or transport the unit is inside (0 = none).
    pub host: u32,
    pub air: bool,
    pub structure: bool,
    pub radius: f32,
    pub height: f32,
    /// The cylinder shots and blasts hit (`UnitDef::hit_radius` / `hit_height`).
    pub hit_r: f32,
    pub hit_h: f32,
    pub seen: u16,
    pub radar: u16,
    pub work: u32,
    pub building: u32,
    pub upgrading: bool,
    pub paused: bool,
    /// A weapon is reloading: it has fired a moment ago.
    pub firing: bool,
    /// Wreck id this unit is being revived from (0 = not a revival site, or finished).
    pub revival: u32,
    /// Sandbags, a trench: cover, not a target (`UnitDef::is_fieldwork`).
    pub fieldwork: bool,
}
impl Cen {
    /// An unfinished revival site: scavengers build it, engineers do not.
    pub fn reviving(&self) -> bool {
        self.revival != 0 && !self.done
    }
    pub fn p3(&self) -> Vec3 {
        self.pos.at(self.y)
    }
    pub fn center(&self) -> Vec3 {
        self.pos.at(self.y + self.hit_h * 0.5)
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
        c.hp > 0.0 && !c.hosted && !c.fieldwork && hostile(team, c.team) && c.known_to(team) && if c.air { air } else { land }
    }
    /// Like `nearest_enemy`, but an enemy for which `preferred` holds beats any other, however far.
    pub fn pick_enemy(&self, pos: V2, team: u8, range: f32, min_range: f32, land: bool, air: bool, preferred: impl Fn(&Cen) -> bool) -> Option<&Cen> {
        let mut best: Option<(bool, f32, usize)> = None;
        let m2 = min_range * min_range;
        self.near(pos, range, |c| {
            let d2 = (c.pos - pos).len2();
            if d2 >= m2 && self.valid_target(c, team, land, air) {
                let p = preferred(c);
                if best.map_or(true, |b| (p && !b.0) || (p == b.0 && d2 < b.1)) {
                    best = Some((p, d2, self.index[&c.id]));
                }
            }
        });
        best.map(|b| &self.list[b.2])
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

/// Wrecks and landed debris at the start of the tick, for reclaim orders:
/// (id, position, mass, def, piece size; 0 = a whole wreck).
#[derive(Resource, Default)]
pub struct WreckList(pub Vec<(u32, V2, f32, DefId, f32)>);

/// Effects on other entities are queued and applied by dedicated systems.
#[derive(Resource, Default)]
pub struct Effects {
    /// (unit id, raw damage, damage type)
    pub damage: Vec<(u32, f32, usize)>,
    /// (wreck id, amount, player)
    pub reclaim: Vec<(u32, f32, u8)>,
    /// Sites created this tick: (def, position).
    pub sites: Vec<(DefId, V2, u32)>,
    /// Capture work done this tick: (target id, build power x time, player).
    pub capture: Vec<(u32, f32, u8)>,
    /// Wrecks turned back into units this tick: (wreck id, new unit id).
    pub revived: Vec<(u32, u32)>,
    /// Reclaim work on live buildings this tick: (unit id, mass, player).
    pub reclaim_unit: Vec<(u32, f32, u8)>,
    /// Buildings reclaimed down to nothing this tick: they leave no wreck.
    pub reclaimed: Vec<u32>,
    /// Shell holes to dig this tick: (centre, radius, depth).
    pub craters: Vec<(V2, f32, f32)>,
    /// Terraforming work this tick: (centre, radius, wanted height, volume, player).
    pub terraform: Vec<(V2, f32, f32, f32, u8)>,
    /// Units taken aboard a transport this tick: (unit, transport).
    pub loaded: Vec<(u32, u32)>,
    /// Infantry at the door of an enemy-held building this tick, fighting for it: (unit, building).
    pub assault: Vec<(u32, u32)>,
    /// Buildings being stormed this tick (their defenders do not fire out).
    pub stormed: Vec<u32>,
    /// Reclaim work on trees and rocks this tick: (feature id, mass-equivalent amount, player).
    pub reclaim_feature: Vec<(u32, f32, u8)>,
}

// ---------------------------------------------------------------------------- trees and rocks

/// Ids of map features (trees, rocks) start here: feature `i` of the map is `FEATURE_ID0 + i`.
/// Unit and wreck ids count up from 1 and never get this far.
pub const FEATURE_ID0: u32 = 0xF000_0000;

pub fn is_feature(id: u32) -> bool {
    id >= FEATURE_ID0
}

/// One tree or rock of the map.
#[derive(Clone, Copy, Debug)]
pub struct FeatureState {
    /// Index into `rules.features`; `u16::MAX` for a type this data set does not know.
    pub def: u16,
    pub pos: V2,
    /// Share of its value still there (1 = untouched, 0 = gone).
    pub left: f32,
    /// Height above the ground: it blocks sight up to there.
    pub height: f32,
    /// Footprint radius on the ground.
    pub radius: f32,
    /// Hit points when untouched; damage takes `left` down by its share of them.
    pub hp: f32,
    /// Map cell it stands in.
    pub cell: u32,
}

/// Per map cell, how high above the ground sight is blocked: standing trees and rocks plus wrecks
/// (a wreck blocks up to the height of its collapsed model). Radar is not blocked by these.
/// Empty when the rules switch that off (`sight_blocked_by_reclaimables`).
pub fn sight_block<'a>(features: &'a Features, wrecks: &WreckList, defs: &Defs, map: &MapData) -> std::borrow::Cow<'a, [f32]> {
    if !defs.rules.sight_blocked_by_reclaimables {
        return std::borrow::Cow::Borrowed(&[]);
    }
    if wrecks.0.is_empty() || features.sight.len() != map.n * map.n {
        return std::borrow::Cow::Borrowed(&features.sight[..]);
    }
    let mut v = features.sight.clone();
    for w in &wrecks.0 {
        // Small pieces of debris do not block sight.
        if w.4 > 0.0 && w.4 < defs.rules.debris.sight_size {
            continue;
        }
        let h = Wreck::shape(defs, w.3, w.4).1;
        let (cx, cy) = map.cell_of(w.1);
        if map.inside(cx, cy) {
            let c = cy as usize * map.n + cx as usize;
            v[c] = v[c].max(h);
        }
    }
    std::borrow::Cow::Owned(v)
}

/// Size of a cell of the features' spatial grid, in world units.
const FEATURE_GRID: f32 = 16.0;

/// The map's reclaimable trees and rocks. The map lists them; this keeps what reclaiming and
/// building took of them as the difference to the map (`changed`), which is what a snapshot holds.
#[derive(Resource, Default)]
pub struct Features {
    pub list: Vec<FeatureState>,
    /// Touched features: index -> share left.
    pub changed: BTreeMap<u32, f32>,
    /// Counts up whenever a feature disappears or comes back (the view redraws then).
    pub version: u64,
    /// Per map cell: height of the tallest standing feature (blocks sight, see `sight_block`).
    pub sight: Vec<f32>,
    grid: Vec<Vec<u32>>,
    gn: usize,
}

impl Features {
    pub fn new(map: &MapData, defs: &Defs) -> Features {
        let kinds: Vec<u16> = map.feature_kinds.iter().map(|k| defs.feature_id(k).unwrap_or(u16::MAX)).collect();
        let gn = (map.world() / FEATURE_GRID).ceil().max(1.0) as usize;
        let mut f = Features { gn, grid: vec![vec![]; gn * gn], sight: vec![0.0; map.n * map.n], ..default() };
        let last = map.n.saturating_sub(1) as i32;
        for (i, m) in map.features.iter().enumerate() {
            let def = kinds.get(m.kind as usize).copied().unwrap_or(u16::MAX);
            let fd = defs.rules.features.get(def as usize);
            let height = fd.map_or(0.0, |d| d.height() * m.scale);
            let radius = fd.map_or(0.0, |d| d.radius() * m.scale);
            let hp = fd.map_or(1.0, |d| (d.max_hp() * m.scale).max(1.0));
            let (cx, cy) = map.cell_of(m.pos);
            let cell = (cy.clamp(0, last) as usize * map.n + cx.clamp(0, last) as usize) as u32;
            f.list.push(FeatureState { def, pos: m.pos, left: if def == u16::MAX { 0.0 } else { 1.0 }, height, radius, hp, cell });
            let c = f.cell(m.pos);
            f.grid[c].push(i as u32);
            if def != u16::MAX {
                if let Some(s) = f.sight.get_mut(cell as usize) {
                    *s = s.max(height);
                }
            }
        }
        f
    }
    /// Recomputes the sight blocker of one map cell from the features standing in it.
    fn resight(&mut self, cell: u32, pos: V2) {
        let mut h = 0.0f32;
        for &i in &self.grid[self.cell(pos)] {
            let s = &self.list[i as usize];
            if s.cell == cell && s.left > 0.0 {
                h = h.max(s.height);
            }
        }
        if let Some(v) = self.sight.get_mut(cell as usize) {
            *v = h;
        }
    }
    fn cell(&self, p: V2) -> usize {
        let x = ((p.x / FEATURE_GRID).floor().max(0.0) as usize).min(self.gn - 1);
        let y = ((p.y / FEATURE_GRID).floor().max(0.0) as usize).min(self.gn - 1);
        y * self.gn + x
    }
    pub fn alive(&self, idx: usize) -> bool {
        self.list.get(idx).map_or(false, |f| f.left > 0.0)
    }
    /// A standing feature by id.
    pub fn get(&self, id: u32) -> Option<&FeatureState> {
        if !is_feature(id) {
            return None;
        }
        self.list.get((id - FEATURE_ID0) as usize).filter(|f| f.left > 0.0)
    }
    pub fn set_left(&mut self, idx: usize, left: f32) {
        let Some(f) = self.list.get_mut(idx) else { return };
        if f.def == u16::MAX {
            return;
        }
        let left = if left <= 1e-4 { 0.0 } else { left.min(1.0) };
        let flip = (f.left > 0.0) != (left > 0.0);
        f.left = left;
        let (cell, pos) = (f.cell, f.pos);
        if flip {
            self.version += 1;
            self.resight(cell, pos);
        }
        if left >= 1.0 {
            self.changed.remove(&(idx as u32));
        } else {
            self.changed.insert(idx as u32, left);
        }
    }
    /// Calls `f(index, feature)` for every standing feature within `r` of `p`, in index order
    /// per grid cell and cells in a fixed order (deterministic).
    pub fn near(&self, p: V2, r: f32, mut f: impl FnMut(usize, &FeatureState)) {
        if self.gn == 0 {
            return;
        }
        let g = FEATURE_GRID;
        let (x0, x1) = (((p.x - r) / g).floor().max(0.0) as usize, (((p.x + r) / g).floor().max(0.0) as usize).min(self.gn - 1));
        let (y0, y1) = (((p.y - r) / g).floor().max(0.0) as usize, (((p.y + r) / g).floor().max(0.0) as usize).min(self.gn - 1));
        for y in y0..=y1 {
            for x in x0..=x1 {
                for &i in &self.grid[y * self.gn + x] {
                    let s = &self.list[i as usize];
                    if s.left > 0.0 && s.pos.dist(p) <= r {
                        f(i as usize, s);
                    }
                }
            }
        }
    }
    /// The standing feature inside the circle that is nearest to `from`: (id, position).
    pub fn nearest(&self, c: V2, radius: f32, from: V2) -> Option<(u32, V2)> {
        let mut best: Option<(f32, usize)> = None;
        self.near(c, radius, |i, s| {
            let d = s.pos.dist(from);
            if best.map_or(true, |b| d < b.0 || (d == b.0 && i < b.1)) {
                best = Some((d, i));
            }
        });
        best.map(|b| (FEATURE_ID0 + b.1 as u32, self.list[b.1].pos))
    }
    /// Removes what stands inside a square of `foot` cells around `pos` (a building goes up there).
    pub fn clear_square(&mut self, pos: V2, foot: i32) {
        let half = foot as f32 * CELL * 0.5 + 0.3;
        let mut hit = vec![];
        self.near(pos, half * 1.5, |i, s| {
            if (s.pos.x - pos.x).abs() <= half && (s.pos.y - pos.y).abs() <= half {
                hit.push(i);
            }
        });
        for i in hit {
            self.set_left(i, 0.0);
        }
    }
    /// The touched features in index order, for a snapshot.
    pub fn save(&self) -> Vec<(u32, f32)> {
        self.changed.iter().map(|(k, v)| (*k, *v)).collect()
    }
    /// Makes the features match a saved list.
    pub fn load(&mut self, to: &[(u32, f32)]) {
        let stale: Vec<u32> = self.changed.keys().copied().filter(|k| to.binary_search_by_key(k, |e| e.0).is_err()).collect();
        for k in stale {
            self.set_left(k as usize, 1.0);
        }
        for (k, v) in to {
            self.set_left(*k as usize, *v);
        }
    }
}

// ---------------------------------------------------------------------------- changing terrain

/// The terrain as changed during the game (shell craters, terraforming), kept as the difference
/// to the map everybody started with. `MapData::heights` always holds the current heights; this
/// resource remembers which vertices differ so a state can be saved, restored and hashed cheaply.
#[derive(Resource, Default)]
pub struct Ground {
    /// Heights of the map as it was loaded.
    pub base: Vec<f32>,
    /// Changed vertices: index into `MapData::heights` -> current height.
    pub changed: BTreeMap<u32, f32>,
    /// Order-independent fingerprint of `changed`.
    pub hash: u64,
    /// World positions whose terrain changed since the view last looked (drained by the view).
    pub touched: Vec<V2>,
    /// Too many changes to list (or nobody is looking): the view redraws all of the terrain.
    pub touched_all: bool,
    saved: Option<(u64, Arc<Vec<(u32, f32)>>)>,
}

fn ground_mix(idx: u32, h: f32) -> u64 {
    let mut x = (idx as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15) ^ (h.to_bits() as u64).wrapping_mul(0xC2B2AE3D27D4EB4F);
    x ^= x >> 29;
    x = x.wrapping_mul(0xBF58476D1CE4E5B9);
    x ^ (x >> 32)
}

impl Ground {
    pub fn new(map: &MapData) -> Ground {
        Ground { base: map.heights.clone(), ..default() }
    }

    /// Sets one vertex and keeps the passability of the cells around it up to date.
    pub fn set(&mut self, map: &mut MapData, idx: usize, h: f32) {
        if idx >= map.heights.len() || map.heights[idx] == h {
            return;
        }
        let key = idx as u32;
        if let Some(old) = self.changed.get(&key) {
            self.hash ^= ground_mix(key, *old);
        }
        if self.base.get(idx).copied() == Some(h) {
            self.changed.remove(&key);
        } else {
            self.changed.insert(key, h);
            self.hash ^= ground_mix(key, h);
        }
        map.heights[idx] = h;
        let (x, y) = (idx % (map.n + 1), idx / (map.n + 1));
        map.reblock_around(x, y);
        let p = v2(x as f32 * CELL, y as f32 * CELL);
        if self.touched.len() >= 2048 {
            self.touched.clear();
            self.touched_all = true;
        }
        if !self.touched_all && self.touched.last().map_or(true, |l| l.dist(p) > 6.0) {
            self.touched.push(p);
        }
    }

    /// The changed vertices in index order, for a snapshot. Shared until the terrain changes again.
    pub fn save(&mut self) -> Arc<Vec<(u32, f32)>> {
        match &self.saved {
            Some((h, v)) if *h == self.hash && v.len() == self.changed.len() => v.clone(),
            _ => {
                let v = Arc::new(self.changed.iter().map(|(k, h)| (*k, *h)).collect::<Vec<_>>());
                self.saved = Some((self.hash, v.clone()));
                v
            }
        }
    }

    /// Makes the terrain match a saved list of changed vertices.
    pub fn load(&mut self, map: &mut MapData, to: &Arc<Vec<(u32, f32)>>) {
        if let Some((h, v)) = &self.saved {
            if *h == self.hash && Arc::ptr_eq(v, to) {
                return;
            }
        }
        // Vertices changed here but not in the saved state go back to the original map.
        let stale: Vec<u32> = self.changed.keys().copied().filter(|k| to.binary_search_by_key(k, |e| e.0).is_err()).collect();
        for k in stale {
            let h = self.base[k as usize];
            self.set(map, k as usize, h);
        }
        for (k, h) in to.iter() {
            self.set(map, *k as usize, *h);
        }
        self.saved = Some((self.hash, to.clone()));
    }
}

/// True for vertices that must keep their height: the map border (it joins the land beyond the
/// map) and the corners of cells a building stands on.
fn ground_locked(map: &MapData, occ: &[u8], x: usize, y: usize) -> bool {
    let n = map.n;
    if x == 0 || y == 0 || x >= n || y >= n {
        return true;
    }
    for (cx, cy) in [(x - 1, y - 1), (x, y - 1), (x - 1, y), (x, y)] {
        if occ.get(cy * n + cx).copied() == Some(2) {
            return true;
        }
    }
    false
}

/// The vertices inside a circle, as (index, distance from the centre), in index order.
fn ground_circle(map: &MapData, occ: &[u8], pos: V2, radius: f32) -> Vec<(usize, f32)> {
    let n = map.n as i32;
    let (x0, x1) = (((pos.x - radius) / CELL).floor() as i32, ((pos.x + radius) / CELL).ceil() as i32);
    let (y0, y1) = (((pos.y - radius) / CELL).floor() as i32, ((pos.y + radius) / CELL).ceil() as i32);
    let mut out = vec![];
    for y in y0.max(0)..=y1.min(n) {
        for x in x0.max(0)..=x1.min(n) {
            let d = v2(x as f32 * CELL, y as f32 * CELL).dist(pos);
            if d <= radius && !ground_locked(map, occ, x as usize, y as usize) {
                out.push((y as usize * (map.n + 1) + x as usize, d));
            }
        }
    }
    out
}

/// Digs a bowl-shaped hole, never deeper than `max_depth` below the original ground.
///
/// Craters dent the ground but never close it to traffic: a vertex is not lowered further than
/// `CRATER_STEP` below its highest neighbour (the passability limit is 1.7 across a cell, see
/// `MapData::cell_blocked`), and dry land is not dug down to the water. Heavy shelling therefore
/// makes holes wider rather than steeper.
pub fn dig_crater(map: &mut MapData, ground: &mut Ground, occ: &[u8], pos: V2, radius: f32, depth: f32, max_depth: f32) {
    const CRATER_STEP: f32 = 1.4;
    // At least one vertex has to lie inside, whatever the size of the blast.
    let radius = radius.max(CELL * 1.3);
    let side = map.n + 1;
    for (i, d) in ground_circle(map, occ, pos, radius) {
        let t = d / radius;
        let k = (1.0 - t * t) * (1.0 - t * t);
        let h = map.heights[i];
        let mut floor = ground.base[i] - max_depth;
        // Locked vertices are never inside the circle, so all eight neighbours exist.
        let (x, y) = (i % side, i / side);
        let water = map.water_vertex(x, y);
        if ground.base[i] > water + 0.1 {
            floor = floor.max(water + 0.1);
        }
        for (nx, ny) in [(x - 1, y - 1), (x, y - 1), (x + 1, y - 1), (x - 1, y), (x + 1, y), (x - 1, y + 1), (x, y + 1), (x + 1, y + 1)] {
            floor = floor.max(map.heights[ny * side + nx] - CRATER_STEP);
        }
        let new = (h - depth * k).max(floor).min(h);
        ground.set(map, i, new);
    }
}

/// Digs a trench piece: the four corners of the cell at `pos` go down to `depth` below the
/// original ground (never up, so shell holes stay). Like craters it keeps every cell passable,
/// stays above the water and leaves the corners of other buildings' cells alone.
pub fn dig_trench(map: &mut MapData, ground: &mut Ground, occ: &[u8], pos: V2, depth: f32) {
    const STEP: f32 = 1.4;
    let side = map.n + 1;
    let (cx, cy) = map.cell_of(pos);
    if !map.inside(cx, cy) {
        return;
    }
    for (x, y) in [(cx, cy), (cx + 1, cy), (cx, cy + 1), (cx + 1, cy + 1)] {
        let (x, y) = (x as usize, y as usize);
        if ground_locked(map, occ, x, y) {
            continue;
        }
        let i = y * side + x;
        let h = map.heights[i];
        let mut floor = ground.base[i] - depth;
        let water = map.water_vertex(x, y);
        if ground.base[i] > water + 0.1 {
            floor = floor.max(water + 0.1);
        }
        for (nx, ny) in [(x - 1, y - 1), (x, y - 1), (x + 1, y - 1), (x - 1, y), (x + 1, y), (x - 1, y + 1), (x, y + 1), (x + 1, y + 1)] {
            floor = floor.max(map.heights[ny * side + nx] - STEP);
        }
        if floor < h {
            ground.set(map, i, floor);
        }
    }
}

/// Where a terraforming order wants each vertex of its circle: (index, wanted height).
/// Heights stay within `limit` of the original ground.
pub fn terraform_targets(map: &MapData, ground: &Ground, occ: &[u8], pos: V2, radius: f32, height: f32, limit: f32) -> Vec<(usize, f32)> {
    ground_circle(map, occ, pos, radius.max(CELL * 0.8)).into_iter().map(|(i, _)| (i, height.clamp(ground.base[i] - limit, ground.base[i] + limit))).collect()
}

/// Height difference still to be moved by a terraforming order (sum over its vertices).
pub fn terraform_left(map: &MapData, ground: &Ground, occ: &[u8], pos: V2, radius: f32, height: f32, limit: f32) -> f32 {
    terraform_targets(map, ground, occ, pos, radius, height, limit).iter().map(|(i, t)| (t - map.heights[*i]).abs()).sum()
}

/// Switches a freshly set up game to control points mode; every team starts with `start` points.
pub fn enable_control_points(world: &mut World, start: f32) {
    let start = start.max(1.0);
    let spots = world.resource::<MapData>().control_points();
    let mut game = world.resource_mut::<Game>();
    game.mode = MODE_POINTS;
    game.points = spots.into_iter().map(|pos| ControlPoint { pos, owner: NOBODY, by: NOBODY, progress: 0.0 }).collect();
    game.scores = vec![start; 16];
    game.cp_start = start;
}

#[derive(Resource)]
pub struct SimRng(pub Rng);

// ---------------------------------------------------------------------------- parallel work

/// Maps `items` to results on Bevy's compute threads, results in the order of `items`.
/// `min_chunk`: fewest items worth handing to a thread (about 1 for work of a millisecond, like a
/// path search; tens for small per-unit work, which otherwise costs more to share out than to do).
///
/// Only for work where each result depends on its own item and on shared data nobody writes
/// meanwhile (no `SimRng`, id allocation, budgets or queued effects inside `f`): then the outcome is
/// bit for bit the one a plain loop gives, on any number of threads, and the simulation stays
/// deterministic. Apply the results in a sequential loop afterwards. Short lists run inline.
pub fn par_map<T: Sync, R: Send + 'static>(items: &[T], min_chunk: usize, f: impl Fn(&T) -> R + Send + Sync) -> Vec<R> {
    let min_chunk = min_chunk.max(1);
    let pool = compute_pool();
    if items.len() < min_chunk * 2 || pool.thread_num() < 2 {
        return items.iter().map(f).collect();
    }
    // A few chunks per thread, so that a chunk of expensive items does not hold up the rest.
    let chunk = (items.len() / (pool.thread_num() * 4)).max(min_chunk);
    let parts = items.par_chunk_map(pool, chunk, |_, c| c.iter().map(&f).collect::<Vec<R>>());
    let mut out = Vec::with_capacity(items.len());
    for p in parts {
        out.extend(p);
    }
    out
}

/// Bevy's compute task pool (the game's `TaskPoolPlugin`, or the schedule executor, sets it up).
pub fn compute_pool() -> &'static bevy::tasks::TaskPool {
    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::default)
}

/// Headless tests: makes the compute pool before anything else does, `LR_THREADS=n` threads
/// (`LR_THREADS=1` runs `par_map` inline: the state hashes must not change).
pub fn init_headless_pool() {
    bevy::tasks::ComputeTaskPool::get_or_init(|| {
        let mut b = bevy::tasks::TaskPoolBuilder::new().thread_name("sim compute".into());
        if let Some(n) = std::env::var("LR_THREADS").ok().and_then(|s| s.parse::<usize>().ok()) {
            b = b.num_threads(n.max(1));
        }
        b.build()
    });
}

// ---------------------------------------------------------------------------- spawning

#[derive(Clone)]
pub struct PlayerSetup {
    pub name: String,
    pub team: u8,
    pub is_ai: bool,
    pub slot: usize,
    pub faction: u8,
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_unit(commands: &mut Commands, defs: &Defs, game: &mut Game, map: &MapData, def: DefId, owner: u8, pos: V2, yaw: f32, complete: bool, hosted: u32) -> u32 {
    spawn_unit_built(commands, defs, game, map, def, owner, pos, yaw, if complete { None } else { Some(0.0) }, hosted)
}

/// `built`: `None` for a finished unit, otherwise how far its construction already is (0..1).
#[allow(clippy::too_many_arguments)]
pub fn spawn_unit_built(commands: &mut Commands, defs: &Defs, game: &mut Game, map: &MapData, def: DefId, owner: u8, pos: V2, yaw: f32, built: Option<f32>, hosted: u32) -> u32 {
    spawn_site(commands, defs, game, map, def, owner, pos, yaw, built, hosted, 0)
}

/// `spawn_unit_built`, for a construction site revived from wreck `revival` (0 = none).
#[allow(clippy::too_many_arguments)]
pub fn spawn_site(commands: &mut Commands, defs: &Defs, game: &mut Game, map: &MapData, def: DefId, owner: u8, pos: V2, yaw: f32, built: Option<f32>, hosted: u32, revival: u32) -> u32 {
    let d = defs.get(def);
    let id = game.alloc();
    let team = game.team_of(owner);
    let y = map.h(pos) + d.mv.altitude;
    let hp = d.hp * built.map_or(1.0, |b| 0.05 + 0.95 * b.clamp(0.0, 1.0));
    let weapons = d.armed().then(|| Weapons { list: d.weapons.iter().map(|_| WeaponState { yaw, ..default() }).collect(), ..default() });
    let (builder, factory, structure) = (d.build.is_some(), d.is_factory(), d.mv.kind == MoveKind::None);
    // Every field of the state is spelt out (no `..default()`): a component added to `unit_state!`
    // does not compile until it is given its starting value here.
    let state = UnitSnap {
        unit: Unit { id, def, owner },
        body: Body { pos, y, yaw, vel: v2(0.0, 0.0) },
        health: Health(hp),
        orders: Orders::default(),
        intel: Intel { seen: 1 << team, radar: 1 << team, look: yaw, lock: 0, spot: [0; 16] },
        built: built.map(UnderConstruction),
        upgrading: None,
        capture: None,
        weapons,
        builder: builder.then(Builder::default),
        factory: factory.then(Factory::default),
        hosted: (hosted != 0).then_some(Hosted(hosted)),
        paused: None,
        structure: structure.then_some(Structure),
        mover: (!structure).then(|| Mover { anchor: pos, path_goal: pos, ..default() }),
        shed: None,
        revival: (revival != 0).then_some(Revival(revival)),
    };
    commands.queue(move |world: &mut World| {
        let e = claim(world, KIND_UNIT, id);
        state.write(&mut world.entity_mut(e));
    });
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
            power: 1.0,
            start,
            ..default()
        });
        if let Some(c) = defs.factions[p.faction as usize].commander {
            spawns.push((c, i as u8, start, (center - start).angle()));
        }
    }
    let mut features = Features::new(&map, &defs);
    for prop in &map.props {
        if let Some(d) = defs.id(&prop.def) {
            let owner = if prop.owner == PROP_HOSTILE { HOSTILE } else { NEUTRAL };
            let pos = if defs.get(d).is_structure() { snap(&defs, &map, d, prop.pos) } else { prop.pos };
            if defs.get(d).is_structure() {
                nav.stamp_def(&map, defs.get(d), pos);
                features.clear_square(pos, defs.get(d).footprint);
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
    world.insert_resource(Ground::new(&map));
    world.insert_resource(features);
    world.insert_resource(map);
    world.insert_resource(SimRng(Rng(seed ^ 0xABCDEF123 | 1)));
    world.insert_resource(Census::default());
    world.insert_resource(WreckList::default());
    world.insert_resource(Effects::default());
    world.insert_resource(FxLog::default());
    world.insert_resource(CommandQueue::default());
    world.insert_resource(Limbo::default());
}

/// Removes every simulation entity (used when leaving a game).
pub fn clear_game(world: &mut World) {
    let mut doomed: Vec<Entity> = world.query_filtered::<Entity, Or<(With<Unit>, With<Wreck>, With<Projectile>)>>().iter(world).collect();
    for e in doomed.drain(..) {
        world.despawn(e);
    }
}

// ---------------------------------------------------------------------------- state save / restore

/// Declares the simulation components of a unit, once: the snapshot record `UnitSnap` (one field
/// per component) and how it is read from an entity, written to one and stripped off one. Snapshots,
/// rollback, joining, the state hash and `spawn_site` all go through these, so a new unit
/// component added to the list is saved and restored everywhere with nothing else to keep in step
/// (and `spawn_site` fails to compile until it sets it).
///
/// `required`: every unit has these. `optional`: present or not (`Option` in the record); also the
/// marker and state components such as `UnderConstruction` or `Paused`. The order is the order of
/// the serialized record (part of the network format and the state hash).
macro_rules! unit_state {
    (required { $($(#[$rm:meta])* $req:ident: $rty:ty),* $(,)? } optional { $($(#[$om:meta])* $opt:ident: $oty:ty),* $(,)? }) => {
        /// One unit's complete simulation state, as kept in a `Snapshot`. Made by `unit_state!`.
        #[derive(Clone, Serialize, Deserialize)]
        pub struct UnitSnap {
            $($(#[$rm])* pub $req: $rty,)*
            $($(#[$om])* pub $opt: Option<$oty>,)*
        }

        impl UnitSnap {
            /// The state of a unit entity (`None` if it lacks a required component).
            pub fn read(e: &EntityRef) -> Option<UnitSnap> {
                Some(UnitSnap {
                    $($req: e.get::<$rty>()?.clone(),)*
                    $($opt: e.get::<$oty>().cloned(),)*
                })
            }

            /// Makes the entity hold exactly this state: sets every component it lists and removes
            /// the optional ones it does not have. Other components (the view's) stay.
            pub fn write(&self, em: &mut EntityWorldMut) {
                em.insert(($(self.$req.clone(),)*));
                $(put(em, self.$opt.clone());)*
            }

            /// Takes every simulation component of a unit off the entity (it goes to `Limbo`).
            pub fn strip(em: &mut EntityWorldMut) {
                $(em.remove::<$rty>();)*
                $(em.remove::<$oty>();)*
            }
        }
    };
}

unit_state! {
    required {
        unit: Unit,
        body: Body,
        health: Health,
        orders: Orders,
        intel: Intel,
    }
    optional {
        built: UnderConstruction,
        upgrading: Upgrading,
        capture: Capture,
        weapons: Weapons,
        builder: Builder,
        factory: Factory,
        hosted: Hosted,
        paused: Paused,
        structure: Structure,
        mover: Mover,
        /// Mass dropped as debris.
        shed: Shed,
        /// Revival site: the wreck it came from.
        revival: Revival,
    }
}

/// The complete simulation state between two ticks, in id order. It is what a joining or
/// diverged client receives, what a client rolls back to, and what the state hash is taken of.
#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Snapshot {
    pub game: Game,
    pub rng: u64,
    /// Damage queued for the next tick (death blasts).
    pub damage: Vec<(u32, f32, usize)>,
    pub units: Vec<UnitSnap>,
    pub projectiles: Vec<Projectile>,
    pub wrecks: Vec<Wreck>,
    /// Terrain vertices that differ from the map the game started on: (index, height).
    pub ground: Arc<Vec<(u32, f32)>>,
    /// Trees and rocks partly or completely reclaimed: (feature index, share left).
    pub features: Vec<(u32, f32)>,
}

pub fn make_snapshot(world: &mut World) -> Snapshot {
    let mut s = Snapshot { game: world.resource::<Game>().clone(), rng: world.get_resource::<SimRng>().map_or(1, |r| r.0 .0), damage: world.get_resource::<Effects>().map(|e| e.damage.clone()).unwrap_or_default(), ..default() };
    let mut q = world.query_filtered::<EntityRef, With<Unit>>();
    s.units = q.iter(world).filter_map(|e| UnitSnap::read(&e)).collect();
    s.units.sort_by_key(|u| u.unit.id);
    let mut q = world.query::<&Projectile>();
    s.projectiles = q.iter(world).cloned().collect();
    s.projectiles.sort_by_key(|p| p.id);
    let mut q = world.query::<&Wreck>();
    s.wrecks = q.iter(world).cloned().collect();
    s.wrecks.sort_by_key(|w| w.id);
    if let Some(mut g) = world.get_resource_mut::<Ground>() {
        s.ground = g.save();
    }
    if let Some(f) = world.get_resource::<Features>() {
        s.features = f.save();
    }
    s
}

/// Fingerprint of a state; machines that simulated the same ticks get the same value.
pub fn state_hash(s: &Snapshot) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for b in bincode::serialize(s).unwrap_or_default() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    h
}

/// Entities that exist in the world but not in the state a client just rolled back to. They are
/// kept (without their simulation components) while the client re-simulates: a unit or shell that
/// is created again with the same id takes its old entity back, so its model does not flicker.
#[derive(Resource, Default)]
pub struct Limbo {
    pub units: HashMap<u32, Entity>,
    pub projectiles: HashMap<u32, Entity>,
    pub wrecks: HashMap<u32, Entity>,
}

const KIND_UNIT: u8 = 0;
const KIND_PROJECTILE: u8 = 1;
const KIND_WRECK: u8 = 2;

/// The entity a new simulation object should use: its old one from `Limbo`, or a fresh one.
fn claim(world: &mut World, kind: u8, id: u32) -> Entity {
    let old = world.get_resource_mut::<Limbo>().and_then(|mut l| match kind {
        KIND_UNIT => l.units.remove(&id),
        KIND_PROJECTILE => l.projectiles.remove(&id),
        _ => l.wrecks.remove(&id),
    });
    match old.filter(|e| world.get_entity(*e).is_ok()) {
        Some(e) => e,
        None => world.spawn_empty().id(),
    }
}

pub fn spawn_projectile(commands: &mut Commands, p: Projectile) {
    commands.queue(move |world: &mut World| {
        let e = claim(world, KIND_PROJECTILE, p.id);
        world.entity_mut(e).insert(p);
    });
}

pub fn spawn_wreck(commands: &mut Commands, w: Wreck) {
    commands.queue(move |world: &mut World| {
        let e = claim(world, KIND_WRECK, w.id);
        world.entity_mut(e).insert(w);
    });
}

/// Despawns what a re-simulation did not create again.
pub fn flush_limbo(world: &mut World) {
    let Some(mut limbo) = world.get_resource_mut::<Limbo>() else { return };
    let limbo = &mut *limbo;
    let doomed: Vec<Entity> = limbo.units.drain().chain(limbo.projectiles.drain()).chain(limbo.wrecks.drain()).map(|(_, e)| e).collect();
    for e in doomed {
        if world.get_entity(e).is_ok() {
            world.despawn(e);
        }
    }
}

fn put<C: Component>(em: &mut EntityWorldMut, c: Option<C>) {
    match c {
        Some(c) => {
            em.insert(c);
        }
        None => {
            em.remove::<C>();
        }
    }
}

/// Makes the world match a saved state. Existing entities are updated in place (they keep their
/// models), missing ones are created, and surplus ones go to `Limbo` until `flush_limbo`.
pub fn restore(world: &mut World, s: &Snapshot) {
    let mut limbo = world.remove_resource::<Limbo>().unwrap_or_default();
    let mut q = world.query::<(Entity, &Unit)>();
    let mut have: HashMap<u32, Entity> = q.iter(world).map(|(e, u)| (u.id, e)).collect();
    for u in &s.units {
        let e = have.remove(&u.unit.id).or_else(|| limbo.units.remove(&u.unit.id)).filter(|e| world.get_entity(*e).is_ok()).unwrap_or_else(|| world.spawn_empty().id());
        u.write(&mut world.entity_mut(e));
    }
    for (id, e) in have {
        UnitSnap::strip(&mut world.entity_mut(e));
        limbo.units.insert(id, e);
    }
    let mut q = world.query::<(Entity, &Projectile)>();
    let mut have: HashMap<u32, Entity> = q.iter(world).map(|(e, p)| (p.id, e)).collect();
    for p in &s.projectiles {
        let e = have.remove(&p.id).or_else(|| limbo.projectiles.remove(&p.id)).filter(|e| world.get_entity(*e).is_ok()).unwrap_or_else(|| world.spawn_empty().id());
        world.entity_mut(e).insert(p.clone());
    }
    for (id, e) in have {
        world.entity_mut(e).remove::<Projectile>();
        limbo.projectiles.insert(id, e);
    }
    let mut q = world.query::<(Entity, &Wreck)>();
    let mut have: HashMap<u32, Entity> = q.iter(world).map(|(e, w)| (w.id, e)).collect();
    for w in &s.wrecks {
        let e = have.remove(&w.id).or_else(|| limbo.wrecks.remove(&w.id)).filter(|e| world.get_entity(*e).is_ok()).unwrap_or_else(|| world.spawn_empty().id());
        world.entity_mut(e).insert(w.clone());
    }
    for (id, e) in have {
        world.entity_mut(e).remove::<Wreck>();
        limbo.wrecks.insert(id, e);
    }
    world.insert_resource(limbo);
    // Terrain: undo the craters and earthworks the saved state does not have, redo the ones it has.
    world.resource_scope(|world, mut map: Mut<MapData>| {
        if let Some(mut g) = world.get_resource_mut::<Ground>() {
            g.load(&mut map, &s.ground);
        }
    });
    if let Some(mut f) = world.get_resource_mut::<Features>() {
        f.load(&s.features);
    }
    world.insert_resource(s.game.clone());
    world.insert_resource(SimRng(Rng(s.rng)));
    world.insert_resource(Effects { damage: s.damage.clone(), ..default() });
    world.resource_mut::<CommandQueue>().0.clear();
    // The occupancy grid is rebuilt from the structures at the start of the next tick.
    world.resource_mut::<Nav>().dirty = true;
}

/// Client side: sets up the simulation for `map` and loads the host's state into it.
pub fn join_game(world: &mut World, mut map: MapData, s: &Snapshot) {
    clear_game(world);
    map.rebuild_blocked();
    world.insert_resource(Nav { occ: map.blocked.clone(), ..default() });
    world.insert_resource(Ground::new(&map));
    let defs = world.resource::<Defs>().clone();
    world.insert_resource(Features::new(&map, &defs));
    world.insert_resource(map);
    world.insert_resource(Census::default());
    world.insert_resource(WreckList::default());
    world.insert_resource(FxLog::default());
    world.insert_resource(CommandQueue::default());
    world.insert_resource(Limbo::default());
    restore(world, s);
    flush_limbo(world);
}

/// Registers the simulation schedule and its systems.
pub struct SimPlugin;
impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.init_schedule(SimTick);
        // The tick's systems run strictly one after another (`systems::register` chains them all), so
        // the multi-threaded executor has nothing to overlap and only adds the cost of handing every
        // system to a worker thread: that doubled the tick time on a 20-thread machine. Work inside
        // a system that can be shared out goes through `par_map`.
        app.edit_schedule(SimTick, |s| {
            s.set_executor(bevy::ecs::schedule::SingleThreadedExecutor::new());
        });
        app.init_resource::<CommandQueue>().init_resource::<FxLog>().init_resource::<Limbo>().init_resource::<Features>().init_resource::<WreckList>();
        systems::register(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit with every optional component of `unit_state!` present (and none of them default).
    fn full_unit() -> UnitSnap {
        let p = v2(10.0, 20.0);
        UnitSnap {
            unit: Unit { id: 7, def: 3, owner: 1 },
            body: Body { pos: p, y: 1.5, yaw: 0.3, vel: v2(0.1, 0.2) },
            health: Health(55.0),
            orders: Orders(vec![Order::Move(p)]),
            intel: Intel { seen: 3, radar: 5, look: 0.7, lock: 9, spot: [7; 16] },
            built: Some(UnderConstruction(0.4)),
            upgrading: Some(Upgrading(0.2)),
            capture: Some(Capture { by: 2, progress: 0.5 }),
            weapons: Some(Weapons { focus: 4, ..default() }),
            builder: Some(Builder { work: 11 }),
            factory: Some(Factory { queue: vec![1, 2], repeat: true, rally: Some(p), building: 12 }),
            hosted: Some(Hosted(13)),
            paused: Some(Paused),
            structure: Some(Structure),
            mover: Some(Mover { goal: Some(p), anchor: p, ..default() }),
            shed: Some(Shed(8.0)),
            revival: Some(Revival(14)),
        }
    }

    fn bytes(s: &UnitSnap) -> Vec<u8> {
        bincode::serialize(s).unwrap()
    }

    #[test]
    fn unit_state_round_trip() {
        let mut world = World::new();
        let e = world.spawn_empty().id();
        let full = full_unit();
        full.write(&mut world.entity_mut(e));
        let back = UnitSnap::read(&world.entity(e).into()).expect("a unit");
        assert_eq!(bytes(&back), bytes(&full), "a component was lost between write and read");

        // Writing a state without the optional components takes them off again.
        let bare = UnitSnap {
            built: None,
            upgrading: None,
            capture: None,
            weapons: None,
            builder: None,
            factory: None,
            hosted: None,
            paused: None,
            structure: None,
            mover: None,
            shed: None,
            revival: None,
            ..full_unit()
        };
        bare.write(&mut world.entity_mut(e));
        let back = UnitSnap::read(&world.entity(e).into()).expect("a unit");
        assert_eq!(bytes(&back), bytes(&bare), "an optional component stayed behind");

        // Stripping leaves an entity that is no unit at all (what `Limbo` keeps).
        full.write(&mut world.entity_mut(e));
        UnitSnap::strip(&mut world.entity_mut(e));
        assert!(UnitSnap::read(&world.entity(e).into()).is_none());
        assert_eq!(world.entity(e).archetype().component_count(), 0, "a simulation component was not stripped");
    }
}
