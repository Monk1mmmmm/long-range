//! The zone-based skirmish AI behind every AI type except `Simple`.
//!
//! The ideas come from three Supreme Commander: FA AI mods, rebuilt for this game's rules:
//! - **M28** (land zones): the map is cut into zones around groups of deposits; every zone keeps
//!   what both sides have there, when it was last seen and what it is worth. Units are handed to
//!   zones by need, armies travel zone by zone and fall back to a rally zone when outmatched.
//! - **M27** (overseer): one grand strategy at a time (rush, main, eco and tech, air, turtle,
//!   commander kill) picked from the threat balance, initial raiders, kiting with the longer gun,
//!   upgrade manager for extractors.
//! - **RNG** (intel manager): scouts keep every zone's intel fresh, production follows what the
//!   enemy was seen to field, expansions get their own defences, zones are scored by
//!   value / distance / enemy strength.
//!
//! The AI only uses what its team sees or has on radar, plus a memory of what it saw before
//! (`Brain::memory`, the counterpart of a player's ghosts and blips). Nothing here is part of the
//! simulation state: after a host migration a fresh brain rebuilds itself from the world.
use super::{find_spot_where, AiKind, Ctx, Mine, Tries};
use crate::data::{DefId, Defs, UnitDef, WeaponKind};
use crate::map::*;
use crate::sim::Command;
use crate::sim::*;
use std::collections::{BTreeMap, VecDeque};

// ---------------------------------------------------------------------------- unit roles

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    None,
    Engineer,
    Scout,
    /// Unarmed mobile radar.
    Radar,
    /// Direct-fire ground unit.
    Tank,
    /// Long range or indirect fire.
    Arty,
    Aa,
    Fighter,
    /// Aircraft that hit the ground (bombers, gunships).
    Bomber,
    /// Revives and reclaims wrecks.
    Scavenger,
    /// Air transport.
    Transport,
    /// Unarmed land transport (APC): ferries infantry to the army.
    Carrier,
    Other,
}

const ROLES: usize = 13;

/// What the AI needs to know about a unit type, worked out from its JSON definition.
#[derive(Clone, Copy, Debug)]
pub struct Info {
    pub role: Role,
    /// Strength against ground units, in mass.
    pub threat: f32,
    /// Strength against aircraft, in mass.
    pub aa: f32,
    /// Reach of its weapons against ground targets.
    pub range: f32,
    pub speed: f32,
    pub mass: f32,
    /// Foot soldier (takes cover, garrisons buildings, storms them if `assault` > 0).
    pub infantry: bool,
    pub assault: f32,
    /// How far it sees enemies for the guns behind it: binoculars, else its vision.
    pub eyes: f32,
    /// Fires only when it stands still (set-up weapons: mortars, guided missiles).
    pub hold: bool,
    /// Indirect fire (arcs over hills: may shell remembered buildings).
    pub indirect: bool,
    /// Dives at its target and is gone (suicide drone).
    pub suicide: bool,
    /// Builds sandbags and trenches.
    pub works: bool,
    /// Land transport capacity (APCs, infantry fighting vehicles).
    pub carry: u8,
    /// Land-weapon damage per second, and its share by damage type (for counters).
    pub dps: f32,
    pub dtypes: [f32; 8],
}

fn info(defs: &Defs, d: &UnitDef, commander: bool) -> Info {
    let dps = |land: bool| -> f32 { d.weapons.iter().filter(|w| if land { w.hits_land } else { w.hits_air }).map(|w| w.damage * w.salvo.max(1) as f32 / w.reload.max(0.1)).sum() };
    let reach = |land: bool| -> f32 { d.weapons.iter().filter(|w| if land { w.hits_land } else { w.hits_air }).map(|w| w.range).fold(0.0, f32::max) };
    let (land, air) = (dps(true), dps(false));
    let mass = d.cost.mass;
    // A commander costs a fortune but fights like a heavy tank.
    let worth = if commander { 700.0 } else { mass };
    let threat = if land > 0.0 { worth } else { 0.0 };
    let aa = if air > 0.0 { worth * (air / land.max(air)).min(1.0) } else { 0.0 };
    let indirect = d.weapons.iter().any(|w| w.hits_land && matches!(w.kind, WeaponKind::Arc | WeaponKind::HighArc));
    let mut dtypes = [0.0f32; 8];
    for w in d.weapons.iter().filter(|w| w.hits_land) {
        dtypes[w.dtype.min(7)] += w.damage * w.salvo.max(1) as f32 / w.reload.max(0.1) / land.max(0.01);
    }
    let works = d.builds.iter().any(|b| defs.get(*b).is_fieldwork());
    let carry = if !d.is_air() && !d.is_structure() { d.transport.as_ref().map_or(0, |t| t.capacity) } else { 0 };
    let role = if d.is_structure() {
        Role::None
    } else if d.is_engineer() {
        Role::Engineer
    } else if d.is_scavenger() {
        Role::Scavenger
    } else if d.transport.is_some() && d.is_air() {
        Role::Transport
    } else if carry > 0 {
        // Armed or not (an infantry fighting vehicle's machine gun is no tank gun): they carry.
        Role::Carrier
    } else if d.build.is_some() {
        Role::Other
    } else if !d.armed() {
        if d.has_radar() {
            Role::Radar
        } else if d.is_air() || d.vision >= 40.0 {
            Role::Scout
        } else {
            Role::Other
        }
    } else if d.is_air() {
        if land > 0.0 && land >= air {
            Role::Bomber
        } else {
            Role::Fighter
        }
    } else if air > 0.0 && air >= land && reach(false) > reach(true) {
        Role::Aa
    } else if mass <= 60.0 && d.mv.speed >= 5.0 && d.vision >= 40.0 {
        Role::Scout
    } else if land <= 0.0 {
        Role::Aa
    } else if indirect || reach(true) >= 110.0 {
        // (Direct guns up to 100 or so are tanks: the Bastion, the Robin, the AT trooper.)
        Role::Arty
    } else {
        Role::Tank
    };
    Info {
        role,
        threat,
        aa,
        range: reach(true),
        speed: d.mv.speed,
        mass,
        infantry: d.is_infantry(),
        assault: d.assault,
        eyes: d.binoculars.as_ref().map_or(0.0, |b| b.range).max(d.vision),
        hold: d.weapons.iter().any(|w| w.hits_land && w.hold > 0.0),
        indirect,
        suicide: d.weapons.iter().any(|w| w.suicide),
        works,
        carry,
        dps: land,
        dtypes,
    }
}

// ---------------------------------------------------------------------------- profiles

/// The grand strategy of the moment (M27). Only the Overseer type changes it during a game.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Strategy {
    #[default]
    Main,
    Rush,
    Eco,
    Air,
    Turtle,
    Kill,
}

/// The numbers one AI type plays by.
#[derive(Clone, Copy, Debug)]
pub struct Profile {
    /// The first factory goes up before anything else.
    fac_first: bool,
    /// One more land factory per this much mass income.
    fac_income: f32,
    max_facs: u32,
    /// Extractors owned before the first air factory.
    air_at: u32,
    /// Mass of ground-attack aircraft wanted, as a share of the land army.
    air: f32,
    eng: f32,
    eng_max: f32,
    /// How far towards the enemy engineers go for deposits (0 = home, 1 = enemy base).
    expand: f32,
    /// Factor on the income needed for upgrades (below 1 = earlier tech).
    tech: f32,
    mex_up_at: u32,
    scouts: u32,
    raiders: u32,
    /// Turrets per defended zone.
    defense: u32,
    radar_forward: bool,
    /// Share of the land army that should be long range.
    arty: f32,
    aa_base: f32,
    /// Bias towards the higher tiers when picking what to produce.
    tier_pow: f32,
    /// Own strength / enemy strength needed to attack a zone.
    attack: f32,
    /// Below this ratio in a fight the army falls back.
    retreat: f32,
    min_force: f32,
    /// Factories keep producing while spending is below this many times the mass income.
    spend: f32,
    /// Scavengers kept while there are wrecks to work on; infantry put into buildings near the front.
    scav: u32,
    garrison: u32,
    /// Share of the land army that should be infantry (spotters with binoculars, garrisons,
    /// storming parties, anti-tank and anti-air teams), and trench pieces dug at the rally point.
    infantry: f32,
    works: u32,
    /// Landings (0 = never, 1 = as often and as close to the enemy as it can): infantry flown or
    /// driven (amphibious carriers across rivers) to buildings near the enemy or ahead of the
    /// attack, to hold them until the army comes.
    landing: f32,
    /// Defends allied zones too.
    team: bool,
    /// Production follows what the enemy fields.
    adapt: bool,
}

const BASE: Profile = Profile {
    fac_first: false,
    fac_income: 7.0,
    max_facs: 4,
    air_at: 6,
    air: 0.2,
    eng: 2.0,
    eng_max: 9.0,
    expand: 0.55,
    tech: 1.0,
    mex_up_at: 4,
    scouts: 2,
    raiders: 0,
    defense: 1,
    radar_forward: false,
    arty: 0.3,
    aa_base: 0.05,
    tier_pow: 2.0,
    attack: 1.3,
    retreat: 0.7,
    min_force: 500.0,
    spend: 1.5,
    scav: 1,
    garrison: 4,
    infantry: 0.12,
    works: 6,
    landing: 0.4,
    team: false,
    adapt: false,
};

const RUSH: Profile = Profile { fac_first: true, fac_income: 4.0, max_facs: 6, air_at: 99, air: 0.0, eng: 1.0, eng_max: 5.0, tech: 2.5, mex_up_at: 8, scouts: 1, raiders: 3, defense: 0, arty: 0.15, tier_pow: 1.0, attack: 0.85, retreat: 0.45, min_force: 200.0, spend: 2.5, scav: 0, garrison: 3, infantry: 0.15, works: 0, landing: 1.0, ..BASE };
const TURTLE: Profile = Profile { expand: 0.45, tech: 0.65, scouts: 1, defense: 3, radar_forward: true, arty: 0.6, air: 0.25, attack: 2.0, retreat: 1.0, min_force: 1500.0, scav: 2, garrison: 8, infantry: 0.2, works: 12, landing: 0.15, ..BASE };

pub fn profile(kind: AiKind, s: Strategy) -> Profile {
    match kind {
        AiKind::Simple => BASE,
        AiKind::Zones => Profile { garrison: 6, infantry: 0.16, works: 8, scouts: 3, raiders: 2, radar_forward: true, attack: 1.4, retreat: 0.8, landing: 0.6, team: true, ..BASE },
        AiKind::Adaptive => Profile { scav: 3, expand: 0.72, eng: 3.0, eng_max: 12.0, raiders: 4, air: 0.3, attack: 1.15, min_force: 400.0, landing: 0.7, adapt: true, ..BASE },
        AiKind::Rusher => RUSH,
        AiKind::Turtle => TURTLE,
        AiKind::Overseer => match s {
            Strategy::Rush => RUSH,
            Strategy::Main => Profile { raiders: 2, ..BASE },
            Strategy::Eco => Profile { tech: 0.6, eng: 3.0, raiders: 2, attack: 1.6, min_force: 900.0, ..BASE },
            Strategy::Air => Profile { air: 0.9, air_at: 3, attack: 1.5, landing: 0.8, ..BASE },
            Strategy::Turtle => Profile { defense: 2, arty: 0.45, attack: 2.0, retreat: 1.0, min_force: 900.0, ..BASE },
            Strategy::Kill => Profile { attack: 0.0, retreat: 0.25, min_force: 0.0, landing: 0.0, ..BASE },
        },
    }
}

// ---------------------------------------------------------------------------- zones and memory

#[derive(Clone, Debug, Default)]
pub struct Zone {
    pub pos: V2,
    /// Indices into `map.deposits`.
    pub deposits: Vec<usize>,
    pub adj: Vec<usize>,
    /// By land from our start.
    pub reachable: bool,
    /// Zones with the same number are connected by land (0 = on blocked ground).
    pub comp: u16,
    /// The same for amphibious vehicles (they drive through shallow rivers), and whether an
    /// amphibious vehicle gets there from our start.
    pub acomp: u16,
    pub areach: bool,
    pub d_home: f32,
    /// 0 at home, 1 at the nearest enemy start.
    pub front: f32,
    /// Control point standing here.
    pub cp: Option<usize>,
    /// Player whose start position this is.
    pub base: Option<u8>,
    // Refreshed every update:
    /// Strength of our team's armed units here (mobile and turrets).
    pub ally: f32,
    /// Mass of our team's / my own buildings here.
    pub own: f32,
    pub mine: f32,
    pub turrets: u32,
    /// Known enemy ground strength: mobile, and turrets.
    pub enemy: f32,
    pub enemy_static: f32,
    pub enemy_aa: f32,
    pub enemy_air: f32,
    /// What destroying the enemy's things here is worth.
    pub prize: f32,
    /// Centre of the known enemy presence.
    pub enemy_pos: V2,
    /// Tick the zone centre was last in sight, last under our radar, last had enemies among our buildings.
    pub seen: u32,
    pub radar: u32,
    pub alert: u32,
    /// Enemy start positions: the army assumed to stand there while nobody is looking.
    pub guess: f32,
}

impl Zone {
    fn danger(&self) -> f32 {
        self.enemy + self.enemy_static
    }
    fn safe(&self) -> bool {
        self.danger() <= self.ally * 0.7 + 20.0
    }
}

/// An enemy unit as last seen or detected.
#[derive(Clone, Copy, Debug)]
pub struct Seen {
    pub def: DefId,
    pub pos: V2,
    pub tick: u32,
    pub structure: bool,
    pub air: bool,
    pub team: u8,
    pub hp: f32,
    /// Seen or on radar right now.
    pub known: bool,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
enum Phase {
    #[default]
    Gather,
    Attack(usize),
    Retreat(u32),
}

#[derive(Default)]
pub struct Brain {
    info: Vec<Info>,
    data_hash: u64,
    pub zones: Vec<Zone>,
    /// Zone of every deposit of the map.
    dep_zone: Vec<usize>,
    home: usize,
    /// Direction from our start towards the nearest enemy start still in the game.
    fwd: V2,
    /// Bit mask of the enemy players `front` and `fwd` were worked out for.
    foes: u16,
    pub memory: BTreeMap<u32, Seen>,
    pub strategy: Strategy,
    strategy_tick: u32,
    phase: Phase,
    /// Counts failed attacks: the next one waits for a bigger army.
    wave: u32,
    scout_target: BTreeMap<u32, usize>,
    raiders: Vec<u32>,
    raid_zone: Option<usize>,
    /// The attacking army, and reinforcements on their way to it. Everything else of the main
    /// force waits at the rally zone.
    body: Vec<u32>,
    joiners: Vec<u32>,
    /// Last tick the army was not just waiting at the rally zone, and how much of the usual
    /// superiority the attack under way was launched with (waiting makes impatient).
    busy: u32,
    nerve: f32,
    /// Units told to get into a transport, until that tick: nothing else orders them about.
    boarding: BTreeMap<u32, u32>,
    /// For the report: revive orders given, transport trips started, infantry in buildings, units abroad.
    stats: [u32; 4],
    /// Attack progress: closest the body got to its target and when, pushes made since.
    best: (f32, u32),
    pushes: u32,
    /// A zone the army could not get into, left alone until the tick.
    avoid: (usize, u32),
    /// The zone the last attack fell back from and how many attacks on it in a row did; after a
    /// fall-back no new attack starts before `calm`.
    lost: (usize, u32),
    calm: u32,
    note: String,
    /// Slowly decaying peaks of what the enemy was seen to have.
    enemy_peak: f32,
    enemy_air_peak: f32,
    kill: u32,
    /// What the enemy fields, slowly forgotten: mobile ground mass by armor class, the share of
    /// its ground damage by damage type, and the share of it that comes as missiles. Production
    /// counters these (`counter`).
    mix: Vec<f32>,
    emix: [f32; 8],
    missiles: f32,
    /// Where the attacking body is to stand next (it advances in bounds, everyone together),
    /// and since when.
    step: V2,
    step_tick: u32,
    /// For the report: focus-fire orders, storms ordered, field works ordered, ferry trips, and
    /// the field works standing at the rally point.
    acts: [u32; 4],
    dug: u32,
    /// Build spots that came to nothing, left alone for a while.
    tries: Tries,
    /// The landing under way, when the next may start, and buildings our infantry is to keep
    /// holding until a tick (landings, infantry that took shelter).
    landing: Option<Landing>,
    landing_next: u32,
    held: BTreeMap<u32, u32>,
    /// Rivers or lakes lie between us and the enemy (amphibious carriers are worth having).
    wet: bool,
    /// For the report: landings made, infantry sent into shelter, artillery hunts, landings by
    /// air / amphibious carrier / land carrier.
    ops: [u32; 6],
    /// Enemy guns being hunted down right now.
    hunted: Vec<u32>,
    /// Infantry kept back at the rally point for the next landing, and since when.
    party: Vec<u32>,
    party_since: u32,
}

/// Infantry carried to buildings near the enemy or ahead of the attack (an airborne or
/// amphibious landing): who is on it, what carries them, until when it counts as under way.
#[derive(Clone, Debug)]
struct Landing {
    troops: Vec<u32>,
    carriers: Vec<u32>,
    until: u32,
}

impl Brain {
    /// One line for the self-test's report.
    pub fn debug(&self) -> String {
        let held = self.zones.iter().filter(|z| z.mine > 0.0).count();
        let known: f32 = self.zones.iter().map(|z| z.danger()).sum();
        format!("{:?} {:?} wave {} zones {}/{} raiders {} enemy known {:.0} peak {:.0} air {:.0} | {}", self.strategy, self.phase, self.wave, held, self.zones.len(), self.raiders.len(), known, self.enemy_peak, self.enemy_air_peak, self.note) + &format!(" | revives {} trips {} housed {} abroad {} | focus {} storms {} works {} ({} stand) ferries {} | landings {} (air {} amphibious {} land {}) shelters {} hunts {}", self.stats[0], self.stats[1], self.stats[2], self.stats[3], self.acts[0], self.acts[1], self.acts[2], self.dug, self.acts[3], self.ops[0], self.ops[3], self.ops[4], self.ops[5], self.ops[1], self.ops[2])
    }
    pub fn zone_at(&self, p: V2) -> usize {
        let mut best = (f32::MAX, 0);
        for (i, z) in self.zones.iter().enumerate() {
            let d = (z.pos - p).len2();
            if d < best.0 {
                best = (d, i);
            }
        }
        best.1
    }

    /// Cuts the map into zones (M28's land zones): every start position and control point is
    /// one, deposits join the nearest zone close by or found their own, and open ground far
    /// from all of them gets empty zones so that it is scouted and travelled through.
    fn init(&mut self, cx: &Ctx, me: u8) {
        let (map, game, defs) = (cx.map, cx.game, cx.defs);
        let n = map.n;
        let p = &game.players[me as usize];
        self.data_hash = defs.hash;
        self.info = defs.units.iter().map(|d| info(defs, d, defs.factions.iter().any(|f| f.commander == Some(d.idx)))).collect();
        let cell_pos = |x: i32, y: i32| v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL);
        let free_at = |q: V2| -> V2 {
            let (x, y) = map.cell_of(map.clamp(q));
            nearest_free(&map.blocked, n, x, y).map_or(q, |(x, y)| cell_pos(x, y))
        };
        // Ground connected by land gets one number (M27's pathing groups, M28's plateaus).
        let mut comp = vec![0u16; n * n];
        let mut next = 0u16;
        let mut flood = |comp: &mut Vec<u16>, q: V2| -> u16 {
            let (sx, sy) = map.cell_of(q);
            if !map.inside(sx, sy) || map.blocked[sy as usize * n + sx as usize] != 0 {
                return 0;
            }
            if comp[sy as usize * n + sx as usize] != 0 {
                return comp[sy as usize * n + sx as usize];
            }
            next += 1;
            let mut todo = VecDeque::from([(sx, sy)]);
            comp[sy as usize * n + sx as usize] = next;
            while let Some((x, y)) = todo.pop_front() {
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (a, b) = (x + dx, y + dy);
                    if map.inside(a, b) {
                        let k = b as usize * n + a as usize;
                        if comp[k] == 0 && map.blocked[k] == 0 {
                            comp[k] = next;
                            todo.push_back((a, b));
                        }
                    }
                }
            }
            next
        };
        let home_comp = flood(&mut comp, free_at(p.start));
        // Amphibious vehicles also drive through shallow water.
        let mut acomp = vec![0u16; n * n];
        let mut anext = 0u16;
        let mut aflood = |acomp: &mut Vec<u16>, q: V2| -> u16 {
            let (sx, sy) = map.cell_of(q);
            let ok = |k: usize| passable_for(map.blocked[k], true);
            if !map.inside(sx, sy) || !ok(sy as usize * n + sx as usize) {
                return 0;
            }
            if acomp[sy as usize * n + sx as usize] != 0 {
                return acomp[sy as usize * n + sx as usize];
            }
            anext += 1;
            let mut todo = VecDeque::from([(sx, sy)]);
            acomp[sy as usize * n + sx as usize] = anext;
            while let Some((x, y)) = todo.pop_front() {
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    let (a, b) = (x + dx, y + dy);
                    if map.inside(a, b) {
                        let k = b as usize * n + a as usize;
                        if acomp[k] == 0 && ok(k) {
                            acomp[k] = anext;
                            todo.push_back((a, b));
                        }
                    }
                }
            }
            anext
        };
        let home_acomp = aflood(&mut acomp, free_at(p.start));
        // Water between us and an enemy start: a river to cross.
        self.wet = game.players.iter().filter(|o| hostile(p.team, o.team)).any(|o| {
            (1..60).any(|k| {
                let (x, y) = map.cell_of(p.start + (o.start - p.start) * (k as f32 / 60.0));
                map.inside(x, y) && map.blocked[y as usize * n + x as usize] == OCC_DEEP
            })
        });
        let mut zs: Vec<Zone> = vec![];
        let nearest = |zs: &[Zone], q: V2| zs.iter().enumerate().map(|(i, z)| (z.pos.dist(q), i)).min_by(|a, b| a.0.total_cmp(&b.0));
        for (k, pl) in game.players.iter().enumerate() {
            match nearest(&zs, pl.start) {
                Some((d, _)) if d < 12.0 => {}
                _ => zs.push(Zone { pos: pl.start, base: Some(k as u8), ..Default::default() }),
            }
        }
        for (k, cp) in game.points.iter().enumerate() {
            match nearest(&zs, cp.pos) {
                Some((d, i)) if d < 14.0 => zs[i].cp = zs[i].cp.or(Some(k)),
                _ => zs.push(Zone { pos: cp.pos, cp: Some(k), ..Default::default() }),
            }
        }
        let fixed = zs.len();
        for (k, d) in map.deposits.iter().enumerate() {
            match nearest(&zs, *d) {
                Some((dist, i)) if dist < 26.0 => zs[i].deposits.push(k),
                _ => zs.push(Zone { pos: *d, deposits: vec![k], ..Default::default() }),
            }
        }
        for z in zs.iter_mut().skip(fixed) {
            let sum = z.deposits.iter().fold(v2(0.0, 0.0), |a, k| a + map.deposits[*k]);
            z.pos = sum * (1.0 / z.deposits.len().max(1) as f32);
        }
        let step = 44.0;
        let mut y = step * 0.5;
        while y < map.world() {
            let mut x = step * 0.5;
            while x < map.world() {
                let q = v2(x, y);
                let (cx_, cy_) = map.cell_of(q);
                if map.inside(cx_, cy_) && map.blocked[cy_ as usize * n + cx_ as usize] == 0 && zs.iter().all(|z| z.pos.dist(q) > 40.0) {
                    zs.push(Zone { pos: q, ..Default::default() });
                }
                x += step;
            }
            y += step;
        }
        for z in zs.iter_mut() {
            z.pos = free_at(z.pos);
            z.comp = flood(&mut comp, z.pos);
            z.reachable = z.comp != 0 && z.comp == home_comp;
            z.acomp = aflood(&mut acomp, z.pos);
            z.areach = z.acomp != 0 && z.acomp == home_acomp;
            z.d_home = z.pos.dist(p.start);
            z.enemy_pos = z.pos;
        }
        // Neighbours: two zones with no third one between them (Gabriel graph).
        for i in 0..zs.len() {
            for j in i + 1..zs.len() {
                let mid = (zs[i].pos + zs[j].pos) * 0.5;
                let r2 = (zs[i].pos - zs[j].pos).len2() * 0.25;
                if r2 < 90.0 * 90.0 && !(0..zs.len()).any(|k| k != i && k != j && (zs[k].pos - mid).len2() < r2 * 0.98) {
                    zs[i].adj.push(j);
                    zs[j].adj.push(i);
                }
            }
        }
        self.zones = zs;
        self.dep_zone = map.deposits.iter().map(|d| self.zone_at(*d)).collect();
        self.home = self.zone_at(p.start);
        self.foes = u16::MAX;
        self.set_front(cx, me);
    }

    /// Where the front runs: every zone's `front` and the direction to the nearest enemy start,
    /// from the enemies still in the game. Redone when one is defeated, so that its half of the
    /// map becomes ours to expand into instead of staying "near the enemy" for ever.
    fn set_front(&mut self, cx: &Ctx, me: u8) {
        let game = cx.game;
        let p = &game.players[me as usize];
        let foes = game.players.iter().enumerate().filter(|(_, o)| o.alive && hostile(p.team, o.team)).fold(0u16, |a, (k, _)| a | 1 << k);
        if foes == self.foes {
            return;
        }
        self.foes = foes;
        let enemy_starts: Vec<V2> = game.players.iter().filter(|o| o.alive && hostile(p.team, o.team)).map(|o| o.start).collect();
        for z in self.zones.iter_mut() {
            let d_enemy = enemy_starts.iter().map(|s| s.dist(z.pos)).fold(f32::MAX, f32::min);
            z.front = if d_enemy == f32::MAX { 0.5 } else { z.d_home / (z.d_home + d_enemy).max(1.0) };
        }
        let target = enemy_starts.iter().min_by(|a, b| a.dist(p.start).total_cmp(&b.dist(p.start))).copied().unwrap_or(cx.map.center());
        self.fwd = (target - p.start).norm();
        if self.fwd.len2() < 0.5 {
            self.fwd = v2(1.0, 0.0);
        }
    }

    /// Updates the memory of enemy units and every zone's figures.
    fn refresh(&mut self, cx: &Ctx, me: u8, team: u8) {
        let (census, defs) = (cx.census, cx.defs);
        let tick = cx.game.tick;
        for c in &census.list {
            // (Sandbags and trenches are no targets.)
            if hostile(team, c.team) && c.known_to(team) && !c.hosted && !c.fieldwork && c.hp > 0.0 {
                self.memory.insert(c.id, Seen { def: c.def, pos: c.pos, tick, structure: c.structure, air: c.air, team: c.team, hp: c.hp / c.max_hp.max(1.0), known: true });
            }
        }
        // Buildings are remembered until they are gone, moving units for half a minute.
        self.memory.retain(|id, s| {
            s.known = s.tick == tick;
            census.get(*id).map_or(false, |c| c.hp > 0.0 && hostile(team, c.team)) && (s.structure || tick - s.tick < 300)
        });
        for z in self.zones.iter_mut() {
            z.ally = 0.0;
            z.own = 0.0;
            z.mine = 0.0;
            z.turrets = 0;
            z.enemy = 0.0;
            z.enemy_static = 0.0;
            z.enemy_aa = 0.0;
            z.enemy_air = 0.0;
            z.prize = 0.0;
        }
        for c in &census.list {
            if c.team != team || c.hosted {
                continue;
            }
            let d = defs.get(c.def);
            let inf = self.info[c.def as usize];
            let zi = self.zone_at(c.pos);
            let z = &mut self.zones[zi];
            if c.structure {
                z.own += inf.mass;
                if c.owner == me {
                    z.mine += inf.mass;
                    if inf.threat > 0.0 {
                        z.turrets += 1;
                    }
                }
            }
            if c.done && !c.air {
                z.ally += inf.threat;
            }
            if c.done && z.pos.dist(c.pos) < d.vision.max(c.radius + 8.0) {
                z.seen = tick;
            }
            let radar = d.radar_reach() * 0.85;
            if c.done && !c.paused && radar > 0.0 {
                for z in self.zones.iter_mut() {
                    if z.pos.dist(c.pos) < radar {
                        z.radar = tick;
                    }
                }
            }
        }
        let mut centre: Vec<(V2, f32)> = vec![(v2(0.0, 0.0), 0.0); self.zones.len()];
        let (mut total, mut air) = (0.0, 0.0);
        let mut mix = vec![0.0f32; defs.rules.armor_names.len().max(1)];
        let (mut emix, mut missiles, mut dmg) = ([0.0f32; 8], 0.0f32, 0.0f32);
        for s in self.memory.values().filter(|s| !s.air && s.team != TEAM_HOSTILE) {
            let d = defs.get(s.def);
            let inf = self.info[s.def as usize];
            if !s.structure {
                let k = d.armor_id.min(mix.len() - 1);
                mix[k] += inf.mass;
            }
            if inf.threat > 0.0 {
                for (k, share) in inf.dtypes.iter().enumerate() {
                    emix[k] += share * inf.mass;
                }
                dmg += inf.mass;
                if d.weapons.iter().any(|w| w.hits_land && w.kind == WeaponKind::Missile) {
                    missiles += inf.mass;
                }
            }
        }
        if self.mix.len() != mix.len() {
            self.mix = vec![0.0; mix.len()];
        }
        for (a, b) in self.mix.iter_mut().zip(&mix) {
            *a = (*a * 0.998).max(*b);
        }
        if dmg > 0.0 {
            for (a, b) in self.emix.iter_mut().zip(&emix) {
                *a = *a * 0.98 + 0.02 * b / dmg;
            }
            self.missiles = self.missiles * 0.98 + 0.02 * missiles / dmg;
        }
        for s in self.memory.values() {
            let inf = self.info[s.def as usize];
            let zi = self.zone_at(s.pos);
            let z = &mut self.zones[zi];
            let player = s.team != TEAM_HOSTILE;
            let mut weight = inf.mass.max(10.0);
            if s.structure {
                z.enemy_static += inf.threat;
                z.enemy_aa += inf.aa;
                if player {
                    z.prize += if inf.threat > 0.0 { inf.mass * 0.4 } else { inf.mass };
                }
            } else if s.air {
                z.enemy_air += inf.threat.max(inf.aa);
                air += inf.threat.max(inf.aa);
                weight = 0.0;
            } else {
                z.enemy += inf.threat;
                z.enemy_aa += inf.aa;
                total += inf.threat;
                if player {
                    z.prize += inf.threat * 0.3;
                }
            }
            centre[zi].0 = centre[zi].0 + s.pos * weight;
            centre[zi].1 += weight;
        }
        for (z, c) in self.zones.iter_mut().zip(centre) {
            z.enemy_pos = if c.1 > 0.0 { c.0 * (1.0 / c.1) } else { z.pos };
            // An enemy base is as strong as it was when last looked at; one never scouted is
            // assumed to hold an army that grows with the length of the game.
            let enemy_base = z.base.map_or(false, |o| cx.game.players[o as usize].alive && hostile(team, cx.game.players[o as usize].team));
            z.guess = if !enemy_base {
                0.0
            } else if tick - z.seen < 50 || z.enemy > 0.0 {
                z.danger()
            } else if z.seen == 0 {
                tick as f32 * 0.4
            } else {
                z.guess
            };
            if z.enemy > 0.0 && z.own > 0.0 {
                z.alert = tick;
            }
        }
        self.enemy_peak = (self.enemy_peak * 0.997).max(total);
        self.enemy_air_peak = (self.enemy_air_peak * 0.997).max(air);
    }

    /// What taking a zone is worth to the main army. An enemy start nobody has looked at yet is
    /// assumed to hold a base; control points nobody of ours holds count in that game mode.
    fn worth(&self, cx: &Ctx, team: u8, z: &Zone) -> f32 {
        let mut v = z.prize;
        if let Some(o) = z.base {
            let pl = &cx.game.players[o as usize];
            if pl.alive && hostile(team, pl.team) && z.seen == 0 {
                v += 600.0;
            }
        }
        if let Some(cp) = z.cp.and_then(|k| cx.game.points.get(k)) {
            if cp.owner != team {
                v += 500.0;
            }
        }
        v
    }

    /// How well a unit type fits against what the enemy fields (1 = no better or worse than
    /// any other): its damage against the enemy's armor classes (small arms do next to nothing
    /// to tanks), how much of the enemy's damage its own armor takes, and reactive armor while
    /// the enemy fights with missiles.
    fn counter(&self, defs: &Defs, d: &UnitDef) -> f32 {
        let inf = self.info[d.idx as usize];
        let total: f32 = self.mix.iter().sum();
        let hit = if total > 0.0 && inf.dps > 0.0 {
            self.mix.iter().enumerate().map(|(a, m)| m * inf.dtypes.iter().enumerate().map(|(t, s)| s * defs.armor_mult(t, a)).sum::<f32>()).sum::<f32>() / total
        } else {
            1.0
        };
        let e: f32 = self.emix.iter().sum();
        let taken = if e > 0.01 { self.emix.iter().enumerate().map(|(t, s)| s * defs.armor_mult(t, d.armor_id)).sum::<f32>() / e } else { 1.0 };
        let era = if d.reactive.is_some() { 1.0 + 1.5 * self.missiles } else { 1.0 };
        if off("counter") {
            return 1.0;
        }
        fm::powf(hit.clamp(0.05, 2.0), 1.5) / (0.4 + 0.6 * taken.clamp(0.1, 2.0)) * era
    }

    /// Where the army gathers and falls back to: our safe zone closest to the enemy, and the
    /// spot in it (a little on our side of its centre).
    fn rally(&self, map: &MapData, start: V2) -> (usize, V2) {
        let zi = (0..self.zones.len())
            .filter(|&i| {
                let z = &self.zones[i];
                i == self.home || (z.mine > 0.0 && z.reachable && z.safe() && z.front < 0.6)
            })
            .max_by(|&x, &y| self.zones[x].front.total_cmp(&self.zones[y].front))
            .unwrap_or(self.home);
        let z = self.zones[zi].pos;
        let q = map.clamp(z + (start - z).norm() * 6.0 + self.fwd * if zi == self.home { 16.0 } else { 0.0 });
        let (x, y) = map.cell_of(q);
        (zi, nearest_free(&map.blocked, map.n, x, y).map_or(q, |(x, y)| v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL)))
    }

    fn defence(&self, z: &Zone) -> f32 {
        (z.danger() + 0.5 * z.adj.iter().map(|a| self.zones[*a].enemy).sum::<f32>()).max(z.guess)
    }

    /// The zone the main army should take next (RNG's zone score: value over distance, scaled
    /// down by how strong the enemy is there), or nothing if every prize is too well defended.
    fn pick_target(&self, cx: &Ctx, team: u8, from: V2, force: f32, pf: &Profile) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        let mut any = false;
        for (i, z) in self.zones.iter().enumerate() {
            let v = self.worth(cx, team, z);
            if v <= 0.0 || !z.reachable || (self.avoid.0 == i && cx.game.tick < self.avoid.1) {
                continue;
            }
            any = true;
            let def = self.defence(z);
            if force < pf.attack * def {
                continue;
            }
            let score = v / (60.0 + from.dist(z.pos)) / (1.0 + def / force.max(1.0));
            if best.map_or(true, |b| score > b.0) {
                best = Some((score, i));
            }
        }
        if best.is_none() && !any {
            // Nothing known anywhere: sweep the zone we have not seen for the longest time.
            let tick = cx.game.tick;
            return self.zones.iter().enumerate().filter(|(i, z)| z.reachable && *i != self.home && z.front > 0.35).max_by_key(|(_, z)| tick - z.seen).map(|(i, _)| i);
        }
        best.map(|b| b.1)
    }

    /// Zones to travel through from `a` to `b`, keeping away from defended ones (M28).
    fn route(&self, a: usize, b: usize, force: f32) -> Vec<usize> {
        let n = self.zones.len();
        let mut dist = vec![f32::MAX; n];
        let mut prev = vec![usize::MAX; n];
        let mut done = vec![false; n];
        dist[a] = 0.0;
        loop {
            let Some(u) = (0..n).filter(|i| !done[*i] && dist[*i] < f32::MAX).min_by(|x, y| dist[*x].total_cmp(&dist[*y])) else { break };
            if u == b {
                break;
            }
            done[u] = true;
            for &v in &self.zones[u].adj {
                let z = &self.zones[v];
                if done[v] || (!z.reachable && v != b) {
                    continue;
                }
                let risk = if v == b { 0.0 } else { z.danger() / (force + 50.0) };
                let d = dist[u] + self.zones[u].pos.dist(z.pos) * (1.0 + risk * 2.0);
                if d < dist[v] {
                    dist[v] = d;
                    prev[v] = u;
                }
            }
        }
        if prev[b] == usize::MAX {
            return vec![a, b];
        }
        let mut path = vec![b];
        while *path.last().unwrap() != a {
            path.push(prev[*path.last().unwrap()]);
        }
        path.reverse();
        path
    }

    /// M27's strategic overseer: one grand strategy from the balance of forces.
    fn pick_strategy(&mut self, cx: &Ctx, me: u8, team: u8, mine: &[Mine]) {
        let tick = cx.game.tick;
        if tick < self.strategy_tick {
            return;
        }
        self.strategy_tick = tick + 100;
        let p = &cx.game.players[me as usize];
        let mut army = 0.0;
        let mut strike = 0.0;
        let mut air_fac = false;
        for m in mine.iter().filter(|m| !m.uc) {
            let inf = self.info[m.def as usize];
            match inf.role {
                Role::Tank | Role::Arty | Role::Aa => army += inf.threat,
                Role::Bomber => strike += inf.mass,
                _ => {}
            }
            let d = cx.defs.get(m.def);
            air_fac |= d.is_factory() && d.builds.iter().any(|b| cx.defs.get(*b).is_air());
        }
        let secs = tick / 10;
        let enemy_aa: f32 = self.zones.iter().map(|z| z.enemy_aa).sum();
        let near = self.zones.iter().filter(|z| z.base.map_or(false, |o| hostile(team, cx.game.players[o as usize].team) && cx.game.players[o as usize].alive)).map(|z| z.d_home).fold(f32::MAX, f32::min);
        let home = &self.zones[self.home];
        let home_safe = home.enemy < home.ally * 0.5 + 50.0;
        // The enemy commander in sight and hurt, or caught by our army: everything goes for it.
        self.kill = 0;
        for (id, s) in &self.memory {
            let commander = s.team != TEAM_HOSTILE && cx.defs.factions.iter().any(|f| f.commander == Some(s.def));
            if commander && s.known {
                let z = &self.zones[self.zone_at(s.pos)];
                if s.hp < 0.45 || z.ally > 2.5 * (self.defence(z) + 700.0) {
                    self.kill = *id;
                }
            }
        }
        self.strategy = if self.kill != 0 {
            Strategy::Kill
        } else if secs > 180 && self.enemy_peak > 1.6 * army + 200.0 {
            Strategy::Turtle
        } else if secs < 360 && near < cx.map.world() * 0.6 && self.wave < 2 {
            Strategy::Rush
        } else if secs > 360 && air_fac && enemy_aa < strike * 0.5 + 250.0 && self.enemy_air_peak < 300.0 && p.income_m > 12.0 {
            Strategy::Air
        } else if secs > 240 && home_safe && army > 1.5 * self.enemy_peak + 300.0 {
            Strategy::Eco
        } else {
            Strategy::Main
        };
    }
}

// ---------------------------------------------------------------------------- one update

/// `LR_AIOFF=bounds,crescent,...`: switches parts of the AI off, to measure what each is worth
/// (`--selftest` duels against `simple`). Read once.
fn off(what: &str) -> bool {
    static OFF: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    OFF.get_or_init(|| std::env::var("LR_AIOFF").map(|v| v.split(',').map(|x| x.trim().to_string()).collect()).unwrap_or_default()).iter().any(|x| x == what)
}

/// How far the attacking body moves in one bound.
const BOUND: f32 = 35.0;

pub fn run(b: &mut Brain, rng: &mut Rng, kind: AiKind, reclaim: bool, me: u8, cx: &Ctx, mine: &[Mine], out: &mut Vec<Command>) {
    let team = cx.game.players[me as usize].team;
    if b.zones.is_empty() || b.data_hash != cx.defs.hash || b.info.len() != cx.defs.units.len() {
        b.init(cx, me);
    }
    b.set_front(cx, me);
    b.refresh(cx, me, team);
    if kind == AiKind::Overseer {
        b.pick_strategy(cx, me, team, mine);
    }
    let mut pf = profile(kind, b.strategy);
    if off("infantry") {
        pf.infantry = 0.0;
    }
    if off("works") {
        pf.works = 0;
    }
    eco(b, rng, &pf, reclaim, me, cx, mine, out);
    army(b, &pf, me, cx, mine, out);
}

fn weighted<'a>(rng: &mut Rng, pool: &[&'a UnitDef], weight: impl Fn(&UnitDef) -> f32) -> Option<&'a UnitDef> {
    let sum: f32 = pool.iter().map(|d| weight(d)).sum();
    let mut roll = rng.f() * sum;
    for d in pool {
        roll -= weight(d);
        if roll <= 0.0 {
            return Some(d);
        }
    }
    pool.last().copied()
}

/// Engineers, factories and upgrades.
#[allow(clippy::too_many_arguments)]
fn eco(b: &mut Brain, rng: &mut Rng, pf: &Profile, reclaim: bool, me: u8, cx: &Ctx, mine: &[Mine], out: &mut Vec<Command>) {
    let (defs, map, game, census) = (cx.defs, cx.map, cx.game, cx.census);
    let p = &game.players[me as usize];
    let tick = game.tick;
    let start = p.start;
    let fwd = b.fwd;
    let side = fwd.perp();
    let commander = defs.factions[p.faction as usize].commander;
    let is_pump = |d: &UnitDef| d.needs_deposit;
    let is_power = |d: &UnitDef| d.is_structure() && d.eco.energy > 0.0;
    let is_fac = |d: &UnitDef, air: bool| d.is_factory() && d.builds.iter().any(|x| defs.get(*x).is_air() == air && !defs.get(*x).is_structure());
    let is_radar = |d: &UnitDef| d.is_structure() && d.has_radar() && !d.armed();

    // Build spots that came to nothing are left alone for a while.
    let quit = b.tries.update(defs, census, me, tick, |id, at| mine.binary_search_by_key(&id, |m| m.id).ok().map(|k| (mine[k].pos, mine[k].builds.iter().position(|q| q.1.dist(at) < 0.5))));
    if !quit.is_empty() {
        out.push(Command::Stop { ids: quit });
    }
    let tries = std::mem::take(&mut b.tries);
    let ok = |q: V2| off("tries") || tries.ok(q, tick);
    let mut noted: Vec<(DefId, V2, u32, V2)> = vec![];

    // --- what we have
    let mut count = vec![0u32; defs.units.len()];
    let mut claimed: Vec<V2> = census.list.iter().filter(|c| defs.get(c.def).needs_deposit).map(|c| c.pos).collect();
    let (mut pump_up, mut fac_up, mut radar_up) = (0u32, 0u32, 0u32);
    let mut zone_turrets: Vec<u32> = b.zones.iter().map(|z| z.turrets).collect();
    let mut role_mass = [0.0f32; ROLES];
    let mut army_n = 0u32;
    // Infantry in the army, and engineers that dig trenches.
    let (mut foot_mass, mut diggers, mut foot_n) = (0.0f32, 0u32, 0u32);
    // Energy income of generators being built or ordered.
    let mut coming_e = 0.0;
    for m in mine {
        let d = defs.get(m.def);
        count[m.def as usize] += 1;
        if m.uc {
            coming_e += d.eco.energy;
        }
        if m.upg {
            if d.needs_deposit {
                pump_up += 1;
            } else if d.is_factory() {
                fac_up += 1;
            } else {
                radar_up += 1;
            }
        }
        let inf = b.info[m.def as usize];
        role_mass[inf.role as usize] += inf.mass;
        if matches!(inf.role, Role::Tank | Role::Arty | Role::Aa) {
            army_n += 1;
            if inf.infantry {
                foot_mass += inf.mass;
                foot_n += 1;
            }
        }
        if inf.works {
            diggers += 1;
        }
        for (bd, pos) in &m.builds {
            // A site that already stands is counted as a unit.
            if mine.iter().any(|x| x.pos == *pos && x.def == *bd) {
                continue;
            }
            count[*bd as usize] += 1;
            coming_e += defs.get(*bd).eco.energy;
            if defs.get(*bd).needs_deposit {
                claimed.push(*pos);
            }
            if b.info[*bd as usize].threat > 0.0 {
                zone_turrets[b.zone_at(*pos)] += 1;
            }
        }
    }
    let total = |f: &dyn Fn(&UnitDef) -> bool| -> u32 { defs.units.iter().filter(|d| f(d)).map(|d| count[d.idx as usize]).sum() };
    let mexes = total(&is_pump);
    let power = total(&is_power);
    let land_facs = total(&|d| is_fac(d, false));
    let air_facs = total(&|d| is_fac(d, true));
    let engineers = total(&|d| d.is_engineer() && Some(d.idx) != commander);
    let radars = total(&is_radar);
    let aa_structs = total(&|d| d.is_structure() && b.info[d.idx as usize].aa > 0.0 && b.info[d.idx as usize].threat == 0.0);
    let stations = total(&|d| d.is_station());

    // --- state of the economy (M27's stall and float flags)
    let stall_m = p.mass < 40.0 && p.spend_m > p.income_m * 1.2;
    let float_m = p.mass > p.max_mass * 0.6;
    // Urgent: the bank is draining. Soft: income is thin for what we run; deposits come first then.
    // Generators already on the way count, or every engineer starts one in the same minute.
    let have_e = p.income_e + coming_e;
    let mut need_power = (p.energy < p.max_energy * 0.25 || p.power < 0.95 || (p.income_e < p.spend_e && p.energy < p.max_energy * 0.6)) && have_e < p.spend_e * 1.2 + 40.0;
    let mut more_power = have_e < p.spend_e * 1.1 + 20.0 || have_e < 40.0 + mexes as f32 * 6.0 + (land_facs + air_facs) as f32 * 40.0;
    // Mass discipline: in a stall everything is built slower in proportion, extractors included,
    // so factories and upgrades only add to the spending while the income carries it.
    let mut may_spend = (p.mass > 60.0 || p.spend_m < p.income_m * 1.3) && (p.power >= 0.9 || p.energy > p.max_energy * 0.3);
    // Factories may overspend a little further than buildings (a rusher a lot further).
    let may_produce = (p.mass > 60.0 || p.spend_m < p.income_m * pf.spend) && (p.power >= 0.8 || p.energy > p.max_energy * 0.3);
    let home_safe = b.zones[b.home].enemy < b.zones[b.home].ally * 0.5 + 50.0;
    // No way to the enemy by land: aircraft have to do the work.
    let island = !b.zones.iter().any(|z| z.reachable && z.base.map_or(false, |o| hostile(p.team, game.players[o as usize].team)));
    let want_land = if island { 1 } else { (1 + (p.income_m / pf.fac_income) as u32).min(pf.max_facs) + (float_m && p.income_m > 20.0) as u32 * 2 };
    let want_air = if island {
        (1 + (p.income_m / 15.0) as u32).min(3)
    } else if mexes >= pf.air_at || b.enemy_air_peak > 0.0 {
        1 + (pf.air >= 0.5 && p.income_m > 16.0) as u32
    } else {
        0
    };
    // With the land mass each one is on: an engineer flown to an island works there.
    let free_deps: Vec<(V2, u16)> = map
        .deposits
        .iter()
        .enumerate()
        .filter(|(k, d)| {
            let z = &b.zones[b.dep_zone[*k]];
            z.comp != 0 && z.safe() && (!z.reachable || z.front <= pf.expand || z.d_home < 45.0) && !claimed.iter().any(|c| c.dist(**d) < 1.0) && ok(**d)
        })
        .map(|(k, d)| (*d, b.zones[b.dep_zone[k]].comp))
        .collect();
    // The zone of ours closest to the enemy: where forward radars and turrets go.
    let front_zone = b.zones.iter().enumerate().filter(|(_, z)| z.mine > 0.0 && z.reachable).max_by(|x, y| x.1.front.total_cmp(&y.1.front)).map(|(i, _)| i);

    // --- the commander backs off when hurt or outgunned (M27's ACU manager)
    if let Some(com) = mine.iter().find(|m| Some(m.def) == commander && !m.hosted) {
        let mut near = 0.0;
        for s in b.memory.values().filter(|s| s.known && !s.air && s.pos.dist(com.pos) < 45.0) {
            near += b.info[s.def as usize].threat;
        }
        if (com.hp < 0.5 && near > 0.0 || near > 500.0) && com.pos.dist(start) > 25.0 && !matches!(com.order, Some(Order::Move(q)) if q.dist(start) < 6.0) {
            out.push(Command::Orders { ids: vec![com.id], orders: vec![Order::Move(start)], queue: false });
        }
    }

    // --- field works: a trench line across the rally point, sandbags at its ends and a bunker
    // behind it. Idle infantry there drop into the trench by themselves (cover), the garrison
    // code puts them in the bunker. Finished works belong to nobody, so only count what stands.
    let mut digging: Vec<u32> = vec![];
    let (rally_zone, rally) = b.rally(map, start);
    if pf.works > 0 && b.zones[rally_zone].safe() && tick > 1200 {
        let line = rally + fwd * 9.0;
        let half = pf.works as f32 * CELL * 0.5;
        let piece = |dig: bool| defs.units.iter().find(|d| d.work().map_or(false, |w| (w.1 > 0.0) == dig)).map(|d| d.idx);
        let near_line = |q: V2| (q - line).dot(side).abs() < half + 6.0 && (q - line).dot(fwd).abs() < 8.0;
        let standing = |k: DefId| census.list.iter().filter(|c| c.def == k && near_line(c.pos)).count() as u32 + mine.iter().flat_map(|m| m.builds.iter()).filter(|(d, q)| *d == k && near_line(*q)).count() as u32;
        b.dug = [piece(true), piece(false)].iter().flatten().map(|k| census.list.iter().filter(|c| c.def == *k && c.done && near_line(c.pos)).count() as u32).sum();
        for m in mine.iter().filter(|m| !m.uc && !m.hosted && m.orders == 0 && b.info[m.def as usize].works && Some(m.def) != commander) {
            let df = defs.get(m.def);
            let mut list: Vec<Order> = vec![];
            let add = |d: DefId, q: V2, list: &mut Vec<Order>| {
                let q = snap(defs, map, d, q);
                if df.builds.contains(&d) && can_place(defs, map, &cx.nav.occ, d, q) && ok(q) {
                    list.push(Order::Build { def: d, pos: q });
                }
            };
            if let Some(t) = piece(true).filter(|t| standing(*t) < pf.works * 2 / 3) {
                for k in 0..pf.works {
                    add(t, line + side * ((k as f32 - (pf.works as f32 - 1.0) * 0.5) * CELL), &mut list);
                }
            }
            if let Some(sb) = piece(false).filter(|t| standing(*t) < 2) {
                for e in [-1.0, 1.0] {
                    add(sb, line + side * (e * (half + CELL)) + fwd * 2.0, &mut list);
                }
            }
            let bunker = df.builds.iter().map(|d| defs.get(*d)).find(|d| d.is_garrison() && d.cost.mass < p.mass + p.income_m * 20.0);
            if let (Some(bk), true) = (bunker, p.income_m > 15.0) {
                let have = census.list.iter().any(|c| c.def == bk.idx && c.pos.dist(rally) < 25.0) || mine.iter().any(|m| m.builds.iter().any(|(d, _)| *d == bk.idx));
                if !have {
                    if let Some(q) = find_spot_where(rng, defs, map, &cx.nav.occ, bk.idx, rally - fwd * 3.0, ok) {
                        list.push(Order::Build { def: bk.idx, pos: q });
                    }
                }
            }
            if !list.is_empty() {
                for o in &list {
                    if let Order::Build { def, pos } = o {
                        noted.push((*def, *pos, m.id, m.pos));
                    }
                }
                b.acts[2] += list.len() as u32;
                out.push(Command::Orders { ids: vec![m.id], orders: list, queue: false });
                digging.push(m.id);
            }
        }
    }

    // --- engineers
    let mut claimed_now: Vec<V2> = vec![];
    let mut power_started = 0.0;
    let mut started_fac = false;
    // Engineers that are only helping or reclaiming count as free for real work.
    let filler = |m: &Mine| m.orders == 1 && matches!(m.order, Some(Order::Assist(_)) | Some(Order::ReclaimArea { .. }));
    for m in mine.iter().filter(|m| !m.uc && !m.hosted && (m.orders == 0 || filler(m)) && defs.get(m.def).is_engineer() && !digging.contains(&m.id)) {
        let df = defs.get(m.def);
        let is_com = Some(m.def) == commander;
        let opts: Vec<&UnitDef> = df.builds.iter().map(|d| defs.get(*d)).collect();
        let cheapest = |f: &dyn Fn(&UnitDef) -> bool| opts.iter().filter(|d| f(d)).min_by(|x, y| x.cost.mass.total_cmp(&y.cost.mass)).map(|d| d.idx);
        let afford = |d: &UnitDef| d.cost.mass <= 100.0 || d.cost.mass < p.mass + p.income_m * 60.0;
        let runs = |d: &UnitDef| d.eco.upkeep <= p.income_e * 0.2;
        let spot = |rng: &mut Rng, d: DefId, at: V2| find_spot_where(rng, defs, map, &cx.nav.occ, d, at, ok).map(|s| (d, s));
        let my_comp = b.zones[b.zone_at(m.pos)].comp;
        let abroad = my_comp != b.zones[b.home].comp;
        let near_dep = free_deps
            .iter()
            .filter(|d| d.1 == my_comp && !claimed_now.iter().any(|c| c.dist(d.0) < 1.0) && (!is_com || d.0.dist(start) < 40.0))
            .min_by(|x, y| x.0.dist(m.pos).total_cmp(&y.0.dist(m.pos)))
            .map(|d| d.0);
        let mut choice: Option<(DefId, V2)> = None;
        if abroad {
            // Flown over to an island: extractors there, nothing that needs the base.
            if let (Some(dep), Some(pump)) = (near_dep, cheapest(&is_pump)) {
                choice = Some((pump, dep));
                claimed_now.push(dep);
            }
        } else if land_facs == 0 && !started_fac && (pf.fac_first || (mexes >= 2 && power >= 1)) {
            choice = cheapest(&|d| is_fac(d, false)).and_then(|d| spot(rng, d, start + fwd * 9.0));
            started_fac = choice.is_some();
        } else if need_power || (more_power && (is_com || near_dep.is_none() || m.pos.dist(start) < 50.0)) {
            // The biggest generator we can sensibly afford.
            let best = opts.iter().filter(|d| is_power(d) && (d.cost.mass <= 100.0 || d.cost.mass < p.income_m * 45.0)).max_by(|x, y| x.eco.energy.total_cmp(&y.eco.energy)).map(|d| d.idx);
            choice = best.and_then(|d| spot(rng, d, start - fwd * 6.0));
            power_started += 1.0;
            need_power &= power_started < 1.0 + (p.spend_e - p.income_e) / 100.0;
            more_power = false;
        } else if let (Some(dep), Some(pump)) = (near_dep, cheapest(&is_pump)) {
            choice = Some((pump, dep));
            claimed_now.push(dep);
        } else if stall_m || !may_spend {
            // Out of mass: nothing new is started, the engineer helps or reclaims instead.
        } else if !started_fac && land_facs < want_land {
            choice = cheapest(&|d| is_fac(d, false)).and_then(|d| spot(rng, d, start + fwd * 9.0 + side * (land_facs as f32 * 6.0 - 6.0)));
            started_fac = choice.is_some();
        } else if !started_fac && air_facs < want_air {
            choice = cheapest(&|d| is_fac(d, true)).and_then(|d| spot(rng, d, start - fwd * 4.0 + side * 12.0));
            started_fac = choice.is_some();
        } else {
            // Radar first (tanks out-range their own sight), then defences where the enemy
            // showed up (RNG's structure requests), then anti-air once aircraft were seen.
            let radar = opts.iter().filter(|d| is_radar(d) && runs(d) && afford(d)).min_by(|x, y| x.cost.mass.total_cmp(&y.cost.mass)).map(|d| d.idx);
            let turret_zone = (0..b.zones.len())
                .filter(|&i| {
                    let z = &b.zones[i];
                    let wanted = (z.alert > 0 && tick - z.alert < 1800) || Some(i) == front_zone || (i == b.home && pf.defense >= 2);
                    z.mine > 0.0 && wanted && zone_turrets[i] < pf.defense + (i == b.home) as u32
                })
                .max_by_key(|&i| (b.zones[i].alert, i));
            let uncovered = front_zone.filter(|&i| pf.radar_forward && i != b.home && b.zones[i].radar != tick);
            if radars == 0 && tick > 600 && p.income_e >= 60.0 && radar.is_some() {
                choice = radar.and_then(|d| spot(rng, d, start + fwd * 7.0 - side * 8.0));
            } else if let (true, Some(zi), false) = (pf.defense > 0 && tick > 1200, turret_zone, is_com && turret_zone != Some(b.home)) {
                let pool: Vec<&UnitDef> = opts.iter().filter(|d| d.is_structure() && b.info[d.idx as usize].threat > 0.0 && afford(d) && runs(d)).copied().collect();
                let z = &b.zones[zi];
                let at = z.pos + if zi == b.home { fwd * 14.0 } else { (z.pos - start).norm() * 6.0 };
                if let Some(d) = weighted(rng, &pool, |d| d.max_range()) {
                    choice = spot(rng, d.idx, at);
                    if choice.is_some() {
                        zone_turrets[zi] += 1;
                    }
                }
            } else if b.enemy_air_peak > 0.0 && aa_structs < 1 + pf.defense {
                let aa = opts.iter().filter(|d| d.is_structure() && b.info[d.idx as usize].aa > 0.0 && b.info[d.idx as usize].threat == 0.0 && afford(d) && runs(d)).max_by(|x, y| x.cost.mass.total_cmp(&y.cost.mass));
                choice = aa.and_then(|d| spot(rng, d.idx, start + side * (aa_structs as f32 * 10.0 - 5.0)));
            } else if let (Some(zi), Some(d), false) = (uncovered, radar, is_com) {
                if radars < 3 {
                    choice = spot(rng, d, b.zones[zi].pos);
                }
            } else if float_m && stations < 2 {
                choice = opts.iter().find(|d| d.is_station() && afford(d)).and_then(|d| spot(rng, d.idx, start + fwd * 5.0));
            }
        }
        if let Some((d, pos)) = choice {
            count[d as usize] += 1;
            noted.push((d, pos, m.id, m.pos));
            out.push(Command::Orders { ids: vec![m.id], orders: vec![Order::Build { def: d, pos }], queue: false });
            continue;
        }
        if m.orders != 0 || abroad {
            continue;
        }
        // Nothing to build: reclaim wrecks in safe zones, rocks while the mass bank has room,
        // trees when energy runs low, else help an upgrade or a factory.
        let want_mass = p.mass < p.max_mass * 0.8;
        let want_energy = p.energy < p.max_energy * 0.5;
        let mut pile: Option<(f32, V2)> = None;
        if !is_com && want_mass {
            for w in cx.wrecks.0.iter().filter(|w| w.2 >= 25.0) {
                let d = w.1.dist(m.pos);
                if d < 70.0 && pile.map_or(true, |q| d < q.0) && b.zones[b.zone_at(w.1)].safe() {
                    pile = Some((d, w.1));
                }
            }
        }
        if !is_com && reclaim && pile.is_none() && (want_mass || want_energy) {
            cx.features.near(m.pos, 45.0, |_, f| {
                let Some(fd) = defs.rules.features.get(f.def as usize) else { return };
                let useful = (want_mass && fd.mass >= 5.0) || (want_energy && fd.energy >= 5.0);
                let d = f.pos.dist(m.pos);
                if useful && f.pos.dist(start) < map.world() * 0.35 && pile.map_or(true, |q| d < q.0) {
                    pile = Some((d, f.pos));
                }
            });
        }
        if let Some((_, at)) = pile {
            out.push(Command::Orders { ids: vec![m.id], orders: vec![Order::ReclaimArea { pos: at, radius: 10.0 }], queue: false });
        } else if !is_com || m.pos.dist(start) < 40.0 {
            let helped = mine.iter().filter(|x| x.id != m.id && (x.upg || (x.fq.is_some() && !x.uc))).min_by(|x, y| (x.pos.dist(m.pos) - x.upg as u8 as f32 * 200.0).total_cmp(&(y.pos.dist(m.pos) - y.upg as u8 as f32 * 200.0)));
            if let Some(f) = helped {
                out.push(Command::Orders { ids: vec![m.id], orders: vec![Order::Assist(f.id)], queue: false });
            }
        }
    }

    // --- factories: tier up when the economy allows, otherwise produce
    // More builders while mass piles up or the energy runs short.
    let home_comp = b.zones[b.home].comp;
    let far_deps = free_deps.iter().any(|d| d.1 != home_comp);
    // Whole wrecks lying in zones we hold: work for scavengers.
    let wreck_mass: f32 = cx.wrecks.0.iter().filter(|w| w.4 <= 0.0 && w.2 >= 40.0 && b.zones[b.zone_at(w.1)].safe() && b.zones[b.zone_at(w.1)].reachable).map(|w| w.2).sum();
    let want_eng = (pf.eng + mexes as f32 / 3.0 + free_deps.iter().filter(|d| d.1 == home_comp).count().min(6) as f32 * 0.5).min(pf.eng_max) as u32 + if float_m || p.power < 0.9 { 4 } else { 0 };
    let mut eng_count = engineers;
    let land_mass = role_mass[Role::Tank as usize] + role_mass[Role::Arty as usize] + role_mass[Role::Aa as usize];
    let enemy_static: f32 = b.zones.iter().map(|z| z.enemy_static).sum();
    let enemy_aa: f32 = b.zones.iter().map(|z| z.enemy_aa).sum();
    let budget = (p.mass + p.income_m * 120.0).max(80.0);
    let mut queued = [0.0f32; ROLES];
    let mut committed = 0.0;
    for m in mine.iter().filter(|m| m.fq == Some(0) && !m.uc && !m.upg) {
        let df = defs.get(m.def);
        if let Some(up) = df.upgrade_id {
            let t = df.tier as f32;
            // Cut off by water with no transport in reach of this factory's tier: tech up for one.
            let for_carrier = (island || far_deps) && is_fac(df, true) && !df.builds.iter().any(|x| b.info[*x as usize].role == Role::Transport) && defs.get(up).cost.mass < p.mass + p.income_m * 120.0;
            if fac_up == 0 && for_carrier {
                fac_up += 1;
                out.push(Command::Upgrade { ids: vec![m.id] });
                continue;
            }
            if fac_up == 0 && home_safe && may_spend && p.income_m > (9.0 * t + 4.0) * pf.tech && p.income_e > 220.0 * t * pf.tech.min(1.2) && defs.get(up).cost.mass < p.mass + p.income_m * 90.0 {
                fac_up += 1;
                out.push(Command::Upgrade { ids: vec![m.id] });
                continue;
            }
        }
        // Nothing whose upkeep or energy bill the income cannot carry (the big guns and bombers
        // cost more energy than a small economy makes in minutes).
        let opts: Vec<&UnitDef> = df.builds.iter().map(|d| defs.get(*d)).filter(|d| d.eco.upkeep <= p.income_e * 0.2 && (d.cost.energy <= p.income_e * 25.0 || d.cost.energy <= 3000.0)).collect();
        let of = |r: Role| -> Vec<&UnitDef> { opts.iter().filter(|d| b.info[d.idx as usize].role == r && d.cost.mass <= budget).copied().collect() };
        let have = |r: Role| role_mass[r as usize] + queued[r as usize];
        let n_of = |r: Role, air: bool| mine.iter().filter(|x| b.info[x.def as usize].role == r && defs.get(x.def).is_air() == air).count() as u32;
        // Higher tiers and longer guns, scaled by how well the type answers what the enemy fields.
        let tiered = |d: &UnitDef| fm::powf(d.tier as f32, pf.tier_pow) * (b.info[d.idx as usize].range / 40.0).clamp(0.5, 2.0) * b.counter(defs, d);
        let mut pick: Option<&UnitDef> = None;
        if is_fac(df, true) {
            let scouts = of(Role::Scout);
            let fighters = of(Role::Fighter);
            let bombers = of(Role::Bomber);
            let strike_ok = b.strategy == Strategy::Air || enemy_aa < have(Role::Bomber) * 1.5 + 400.0;
            let air_share = if island { 3.0 } else { pf.air };
            // Transports: to islands (engineers, the army) and for dropping raiders.
            let carriers = of(Role::Transport);
            let want_carriers = if island { 2 } else { (far_deps || pf.raiders >= 3 || (pf.landing >= 0.5 && foot_n >= 4)) as u32 };
            if !scouts.is_empty() && n_of(Role::Scout, true) < 1 + (pf.scouts > 2) as u32 {
                pick = scouts.first().copied();
            } else if !carriers.is_empty() && (may_spend || (far_deps && n_of(Role::Transport, true) == 0)) && n_of(Role::Transport, true) + (queued[Role::Transport as usize] > 0.0) as u32 * 9 < want_carriers {
                pick = carriers.iter().max_by_key(|d| d.transport.as_ref().map_or(0, |t| t.capacity)).copied();
            } else if !fighters.is_empty() && have(Role::Fighter) < b.enemy_air_peak {
                pick = weighted(rng, &fighters, tiered);
            } else if !bombers.is_empty() && may_spend && strike_ok && have(Role::Bomber) < air_share * (land_mass + 300.0) {
                pick = weighted(rng, &bombers, tiered);
            }
        } else {
            let eng = opts.iter().filter(|d| d.is_engineer()).max_by_key(|d| d.tier).copied();
            let scouts = of(Role::Scout);
            let radar = of(Role::Radar);
            if eng.is_some() && eng_count < want_eng {
                eng_count += 1;
                pick = eng;
            } else if !scouts.is_empty() && n_of(Role::Scout, false) + (queued[Role::Scout as usize] > 0.0) as u32 * 9 < pf.scouts {
                pick = scouts.first().copied();
            } else if may_spend && n_of(Role::Scavenger, false) + (queued[Role::Scavenger as usize] > 0.0) as u32 * 9 < pf.scav.min((wreck_mass / 400.0) as u32) {
                // Scavengers while wrecks worth reviving lie in our zones.
                pick = of(Role::Scavenger).first().copied();
            } else if let Some(d) = opts.iter().find(|d| b.info[d.idx as usize].works && d.cost.mass <= budget).filter(|_| pf.works > 0 && army_n >= 4 && diggers == 0 && may_spend) {
                // A field engineer for the trenches at the rally point.
                diggers += 1;
                pick = Some(*d);
            } else if let Some(d) = {
                // With a river in the way, the first carrier for landings is an amphibious one.
                let pool = of(Role::Carrier);
                let swim = b.wet && pf.landing > 0.0 && !mine.iter().any(|x| b.info[x.def as usize].role == Role::Carrier && defs.get(x.def).mv.amphibious);
                let ratio = |x: &&UnitDef| x.transport.as_ref().map_or(0, |t| t.capacity) as f32 / x.cost.mass.max(1.0) * if swim && x.mv.amphibious { 100.0 } else { 1.0 };
                let want = (foot_n / 6).min(3).max((pf.landing > 0.0 && foot_n >= 3) as u32 + (swim && foot_n >= 3) as u32);
                pool.iter().max_by(|x, y| ratio(x).total_cmp(&ratio(y))).copied().filter(|_| !off("carrier") && may_spend && n_of(Role::Carrier, false) + (queued[Role::Carrier as usize] > 0.0) as u32 * 9 < want)
            } {
                // Carriers to take the infantry up to the army, and on landings.
                pick = Some(d);
            } else if !radar.is_empty() && army_n >= 3 && n_of(Role::Radar, false) + (queued[Role::Radar as usize] > 0.0) as u32 * 9 < (1 + army_n / 12).min(3) {
                // Mobile radar goes with the army: its guns reach further than its eyes.
                pick = radar.iter().filter(|d| d.cost.mass < p.income_m * 40.0 + 150.0).max_by_key(|d| d.tier).copied().or(radar.first().copied());
            } else if (may_produce && committed < p.mass + p.income_m * 30.0) || (land_mass < 200.0 && !stall_m) {
                // (A first army even when spending runs ahead, but not in a stall: a rusher's
                // factory used to eat the starting mass before any extractor stood.)
                // The role the army is shortest of (RNG's production ratios); anti-air follows
                // the aircraft the enemy was seen with.
                let all = land_mass + queued[Role::Tank as usize] + queued[Role::Arty as usize] + queued[Role::Aa as usize] + 1.0;
                let aa = if b.enemy_air_peak > 0.0 { (b.enemy_air_peak / (all + b.enemy_air_peak)).clamp(0.08, 0.4) } else { pf.aa_base };
                let arty = pf.arty + if pf.adapt && enemy_static > b.enemy_peak { 0.25 } else { 0.0 };
                let mut wants = [(Role::Tank, 1.0 - aa - arty), (Role::Arty, arty), (Role::Aa, aa)];
                wants.sort_by(|x, y| (y.1 - have(y.0) / all).total_cmp(&(x.1 - have(x.0) / all)));
                // Infantry: its share of the army, whatever role each soldier plays in it.
                let foot: Vec<&UnitDef> = opts.iter().filter(|d| b.info[d.idx as usize].infantry && matches!(b.info[d.idx as usize].role, Role::Tank | Role::Arty | Role::Aa) && d.cost.mass <= budget).copied().collect();
                // As much less of it as it answers the enemy worse than the rest of the army does
                // (rifles do next to nothing to tanks).
                let mean = |v: &[&UnitDef]| v.iter().map(|d| b.counter(defs, d)).sum::<f32>() / v.len().max(1) as f32;
                let land_pool: Vec<&UnitDef> = opts.iter().filter(|d| matches!(b.info[d.idx as usize].role, Role::Tank | Role::Arty) && !b.info[d.idx as usize].infantry).copied().collect();
                let fit = if land_pool.is_empty() { 1.0 } else { (mean(&foot) / mean(&land_pool).max(0.01)).clamp(0.2, 1.5) };
                if !foot.is_empty() && foot_mass < pf.infantry * fit * all {
                    pick = weighted(rng, &foot, tiered);
                    if let Some(d) = pick {
                        foot_mass += d.cost.mass;
                    }
                }
                for (r, _) in wants {
                    let pool = of(r);
                    if pick.is_none() && !pool.is_empty() {
                        pick = weighted(rng, &pool, tiered);
                        break;
                    }
                }
            }
        }
        if let Some(d) = pick {
            queued[b.info[d.idx as usize].role as usize] += d.cost.mass;
            // One new unit per update while the bank is low.
            committed += d.cost.mass;
            may_spend &= committed < p.mass + p.income_m * 20.0;
            out.push(Command::Produce { factory: m.id, def: d.idx, count: 1 });
        }
    }

    // --- extractor upgrades (M27's upgrade manager): the safest one first, more at once as income grows
    let max_up = 1 + (p.income_m / 25.0) as u32;
    // Not while the enemy army is much bigger than ours: an upgrade costs the mass of a few tanks
    // and pays back in minutes, which a base under attack does not have.
    let outgunned = !off("outgunned") && b.enemy_peak > land_mass * 1.6 + 500.0;
    if pump_up < max_up && mexes >= pf.mex_up_at && p.income_e > 120.0 && (p.mass > 200.0 || p.spend_m < p.income_m * 1.1) && home_safe && !outgunned {
        let cand = mine
            .iter()
            .filter(|m| {
                let d = defs.get(m.def);
                !m.uc && !m.upg && d.needs_deposit && d.upgrade_id.map_or(false, |t| defs.get(t).cost.mass < p.mass + p.income_m * 100.0 && defs.get(t).eco.upkeep <= p.income_e * 0.25)
            })
            .min_by(|x, y| (defs.get(x.def).tier, x.pos.dist(start)).partial_cmp(&(defs.get(y.def).tier, y.pos.dist(start))).unwrap());
        if let Some(m) = cand {
            out.push(Command::Upgrade { ids: vec![m.id] });
        }
    }
    // --- radar upgrades, when the energy carries the higher upkeep
    if radar_up == 0 && may_spend && home_safe {
        let cand = mine.iter().find(|m| {
            let d = defs.get(m.def);
            !m.uc && !m.upg && is_radar(d) && d.upgrade_id.map_or(false, |t| defs.get(t).eco.upkeep <= p.income_e * 0.12 && defs.get(t).cost.mass < p.income_m * 40.0)
        });
        if let Some(m) = cand {
            out.push(Command::Upgrade { ids: vec![m.id] });
        }
    }
    let mut tries = tries;
    for (d, q, id, at) in noted {
        tries.note(d, q, id, at, tick);
    }
    b.tries = tries;
}

fn same(cur: Option<Order>, o: Order) -> bool {
    match (cur, o) {
        (Some(Order::Move(a)), Order::Move(b)) | (Some(Order::AttackMove(a)), Order::AttackMove(b)) => a.dist(b) < 5.0,
        (Some(Order::Attack(a)), Order::Attack(b)) => a == b,
        (Some(Order::AttackGround { pos: a, .. }), Order::AttackGround { pos: b, .. }) => a.dist(b) < 2.0,
        _ => false,
    }
}

/// Scouts, raiders, zone defence, the main army and its micro.
fn army(b: &mut Brain, pf: &Profile, me: u8, cx: &Ctx, mine: &[Mine], out: &mut Vec<Command>) {
    let (defs, map, game, census) = (cx.defs, cx.map, cx.game, cx.census);
    let p = &game.players[me as usize];
    let (team, tick, start) = (p.team, game.tick, p.start);
    let n = map.n;
    let free_at = |q: V2| -> V2 {
        let (x, y) = map.cell_of(map.clamp(q));
        nearest_free(&map.blocked, n, x, y).map_or(q, |(x, y)| v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL))
    };
    let mut orders: BTreeMap<u32, Order> = BTreeMap::new();
    let mut then: Vec<(u32, Order)> = vec![];
    // An order is only sent when it changes something.
    let go = |orders: &mut BTreeMap<u32, Order>, m: &Mine, o: Order| {
        let arrived = match o {
            Order::Move(q) | Order::AttackMove(q) => m.orders == 0 && m.pos.dist(q) < if defs.get(m.def).is_air() { 22.0 } else { 8.0 },
            _ => false,
        };
        // A unit on its way into a building, a trench or a carrier (or storming one) keeps at it;
        // the march or the defence would otherwise call it back every second.
        let loading = matches!(m.order, Some(Order::Load(_))) && matches!(o, Order::Move(_) | Order::AttackMove(_));
        if !arrived && !loading && !same(m.order, o) {
            orders.insert(m.id, o);
        }
    };
    let role = |m: &Mine| b.info[m.def as usize].role;
    let live: Vec<&Mine> = mine.iter().filter(|m| !m.uc && !m.hosted && !defs.get(m.def).is_structure()).collect();
    b.scout_target.retain(|id, _| census.get(*id).is_some());
    b.raiders.retain(|id| census.get(*id).is_some());

    // --- scouts keep every zone's intel fresh, the stalest and most forward first (RNG, M28)
    let mut taken: Vec<usize> = vec![];
    for m in live.iter().filter(|m| role(m) == Role::Scout) {
        let air = defs.get(m.def).is_air();
        let cur = b.scout_target.get(&m.id).copied().filter(|z| tick - b.zones[*z].seen > 100 && !taken.contains(z));
        let zi = cur.or_else(|| {
            (0..b.zones.len())
                .filter(|&i| i != b.home && (air || b.zones[i].reachable) && !taken.contains(&i))
                .max_by(|&x, &y| {
                    let s = |i: usize| (tick - b.zones[i].seen).min(6000) as f32 * (0.4 + b.zones[i].front) - m.pos.dist(b.zones[i].pos) * 2.0;
                    s(x).total_cmp(&s(y))
                })
        });
        if let Some(zi) = zi {
            taken.push(zi);
            b.scout_target.insert(m.id, zi);
            go(&mut orders, m, Order::Move(b.zones[zi].pos));
        }
    }

    // --- the land army
    b.boarding.retain(|id, until| tick < *until && census.get(*id).map_or(false, |c| !c.hosted));
    let home_comp = b.zones[b.home].comp;
    let fighters_on_foot: Vec<&Mine> = live.iter().filter(|m| matches!(role(m), Role::Tank | Role::Arty | Role::Aa) && !b.boarding.contains_key(&m.id)).copied().collect();
    // Units set down on another land mass fight there on their own (below).
    let (land, abroad): (Vec<&Mine>, Vec<&Mine>) = fighters_on_foot.iter().copied().partition(|m| b.zones[b.zone_at(m.pos)].comp == home_comp);
    let threat = |m: &Mine| b.info[m.def as usize].threat.max(b.info[m.def as usize].aa * 0.3);
    // Where the army falls back to and gathers: our safe zone closest to the enemy.
    let (_, rally) = b.rally(map, start);
    // Known enemy ground strength around a point: units in sight and remembered turrets that
    // reach it. Long-range batteries further off than the units looked for are no part of this
    // fight: falling back would not take the army out of their reach (a heavy battery covers half
    // the map, rally zone included), so counting them made attacks turn back as soon as they began.
    let enemy_near = |q: V2, r: f32| -> f32 {
        b.memory
            .values()
            .filter(|s| !s.air && (s.known || s.structure))
            .filter(|s| s.pos.dist(q) < if s.structure { (b.info[s.def as usize].range + 15.0).min(r + 15.0) } else { r })
            .map(|s| b.info[s.def as usize].threat)
            .sum()
    };

    // Raiders (M27's initial raiders, M28's raider zones): a few fast units go for lightly
    // held extractor zones and run from anything stronger.
    if (b.raiders.len() as u32) < pf.raiders {
        for m in &land {
            let inf = b.info[m.def as usize];
            if (b.raiders.len() as u32) < pf.raiders && inf.role == Role::Tank && inf.speed >= 4.4 && inf.mass <= 450.0 && !b.raiders.contains(&m.id) {
                b.raiders.push(m.id);
            }
        }
    }
    let raid: Vec<&Mine> = land.iter().filter(|m| b.raiders.contains(&m.id)).copied().collect();
    if raid.len() as u32 >= pf.raiders.min(2) && !raid.is_empty() {
        let force: f32 = raid.iter().map(|m| threat(m)).sum();
        let centre = raid.iter().fold(v2(0.0, 0.0), |a, m| a + m.pos) * (1.0 / raid.len() as f32);
        let lure = |z: &Zone| z.prize > 0.0 || (!z.deposits.is_empty() && z.front > 0.55 && z.mine == 0.0 && tick - z.seen > 600);
        let ok = |z: &Zone| z.reachable && lure(z) && z.danger() <= force * 0.6;
        if b.raid_zone.map_or(true, |i| !ok(&b.zones[i])) {
            b.raid_zone = (0..b.zones.len()).filter(|&i| ok(&b.zones[i])).max_by(|&x, &y| {
                let s = |i: usize| (b.zones[i].prize.max(100.0)) / (40.0 + centre.dist(b.zones[i].pos));
                s(x).total_cmp(&s(y))
            });
        }
        let flee = enemy_near(centre, 45.0) > force * 1.2;
        for m in &raid {
            match (flee, b.raid_zone) {
                (false, Some(zi)) => go(&mut orders, m, Order::AttackMove(b.zones[zi].enemy_pos)),
                _ => go(&mut orders, m, Order::Move(rally)),
            }
        }
    }

    let attacking = matches!(b.phase, Phase::Attack(_));
    // Zone defence (M28): every zone of ours with enemies in it gets the nearest units, enough
    // to outmatch what was seen there. Control points nobody of ours holds get a detachment.
    let mut free: Vec<&Mine> = land.iter().filter(|m| !b.raiders.contains(&m.id) || raid.len() < pf.raiders.min(2) as usize).copied().collect();
    let all_force: f32 = free.iter().map(|m| threat(m)).sum();
    let mut needs: Vec<(f32, f32, V2)> = vec![];
    for (i, z) in b.zones.iter().enumerate() {
        let ours = if pf.team { z.own } else { z.mine };
        if z.enemy > 0.0 && ours > 0.0 {
            needs.push((ours + if i == b.home { 1.0e6 } else { 0.0 }, z.enemy * 1.25 + 40.0, z.enemy_pos));
        }
    }
    let mut cps: Vec<&Zone> = b.zones.iter().filter(|z| z.reachable && z.cp.and_then(|k| game.points.get(k)).map_or(false, |cp| cp.owner != team) && z.danger() <= all_force * 0.5).collect();
    cps.sort_by(|x, y| x.d_home.total_cmp(&y.d_home));
    for z in cps.iter().take(2) {
        let cp = game.points[z.cp.unwrap()].pos;
        needs.push((1000.0 - z.d_home, (z.danger() * 1.3).max(150.0), cp));
    }
    // Counter-battery: enemy artillery found shelling our zones, the rally point or the army
    // (radar picks up its shells) is run down by the nearest units: mortars and howitzers are
    // helpless up close (minimum range, slow traverse) and cannot fire on the move.
    let mut hunt_need = 0.0;
    let mut guns: Vec<(u32, Seen)> = b.memory.iter().filter(|(_, s)| s.known && !s.air && !s.structure && b.info[s.def as usize].indirect).map(|(id, s)| (*id, *s)).collect();
    guns.sort_by(|x, y| (x.1.pos.dist(rally), x.0).partial_cmp(&(y.1.pos.dist(rally), y.0)).unwrap());
    let mut hunted = vec![];
    for (gid, s) in guns {
        let reach = b.info[s.def as usize].range + 20.0;
        let threatens = s.pos.dist(rally) < reach || (attacking && s.pos.dist(b.step) < reach) || b.zones.iter().any(|z| z.own > 0.0 && z.pos.dist(s.pos) < reach);
        // Covered by a whole army: that is the main force's job, not a detachment's.
        let guard = enemy_near(s.pos, 35.0) - b.info[s.def as usize].threat;
        let need = b.info[s.def as usize].threat * 1.5 + guard * 1.3 + 60.0;
        if threatens && hunt_need + need < all_force * 0.4 && !needs.iter().any(|n| n.2.dist(s.pos) < 20.0) {
            hunt_need += need;
            b.ops[2] += !b.hunted.contains(&gid) as u32;
            hunted.push(gid);
            needs.push((2.0e5 - s.pos.dist(rally), need, s.pos));
        }
    }
    b.hunted = hunted;
    needs.sort_by(|x, y| y.0.total_cmp(&x.0));
    for (_, need, at) in needs {
        let mut got = 0.0;
        while got < need && !free.is_empty() {
            let k = (0..free.len()).min_by(|&x, &y| free[x].pos.dist(at).total_cmp(&free[y].pos.dist(at))).unwrap();
            let m = free.swap_remove(k);
            got += threat(m);
            go(&mut orders, m, Order::AttackMove(at));
        }
    }

    // --- the main force: gather at the rally zone, attack a zone it can take, travel zone by
    // zone as one body, fall back when the fight goes badly (M27 platoons, M28 zone moves).
    // Units built during an attack gather too and follow in groups, so the body is not
    // strung out along the road.
    // Garrisons: infantry waiting at the rally zone or at home moves into buildings there and
    // fights from the roof; buildings left behind by the front are emptied again.
    // Towns usually stand between the bases, far from both: buildings near the rally point and
    // home take the waiting infantry, buildings the attack passes take the body's own (a roof
    // sees further, so they spot for the guns behind).
    // Slots taken in every transport and building, counted once.
    let mut aboard: BTreeMap<u32, i32> = BTreeMap::new();
    for r in census.list.iter().filter(|r| r.hosted && r.hp > 0.0) {
        *aboard.entry(r.host).or_default() += defs.get(r.def).cargo as i32;
    }
    let riders = |id: u32| aboard.get(&id).copied().unwrap_or(0);
    // Buildings our infantry may walk into, with the room left in them.
    let mut refuges: Vec<(u32, V2, i32)> = census
        .list
        .iter()
        .filter(|c| c.structure && c.done && c.hp > 0.0 && defs.get(c.def).is_garrison() && (c.team == team || c.owner == NEUTRAL))
        .map(|c| (c.id, c.pos, defs.get(c.def).transport.as_ref().map_or(0, |t| t.capacity as i32) - riders(c.id)))
        .filter(|h| h.2 > 0)
        .collect();
    b.held.retain(|id, until| tick < *until && census.get(*id).map_or(false, |c| c.hp > 0.0));
    // Enemy artillery known to reach a point (radar finds guns by their shells).
    let shelled = |q: V2| -> f32 { b.memory.values().filter(|s| s.known && !s.air && !s.structure && b.info[s.def as usize].indirect && s.pos.dist(q) < b.info[s.def as usize].range).map(|s| b.info[s.def as usize].threat).sum() };

    // --- shelter: infantry caught by a stronger enemy, hurt with enemies about, or under
    // artillery fire, gets into the nearest building it can and fights from the roof (out of
    // reach of the guns, seeing further) instead of dying in the open.
    {
        let mut k = 0;
        while k < free.len() {
            let m = free[k];
            let d = defs.get(m.def);
            if !d.is_infantry() || d.cargo == 0 || matches!(m.order, Some(Order::Load(_))) {
                k += 1;
                continue;
            }
            let foe = enemy_near(m.pos, 40.0) + shelled(m.pos) * 0.6;
            let ours: f32 = land.iter().filter(|o| o.pos.dist(m.pos) < 30.0).map(|o| threat(o)).sum();
            let danger = foe > 0.0 && (foe > ours * 1.5 + 30.0 || m.hp < 0.5);
            let house = if danger { refuges.iter_mut().filter(|h| h.2 >= d.cargo as i32 && h.1.dist(m.pos) < 25.0).min_by(|x, y| x.1.dist(m.pos).total_cmp(&y.1.dist(m.pos))) } else { None };
            match house {
                Some(h) => {
                    h.2 -= d.cargo as i32;
                    orders.insert(m.id, Order::Load(h.0));
                    b.held.insert(h.0, tick + 600);
                    b.ops[1] += 1;
                    free.swap_remove(k);
                }
                None => k += 1,
            }
        }
    }

    // --- landings (Market Garden, but done properly): infantry carried by air transports, or by
    // land carriers (amphibious ones straight across rivers, round the defended bridges), to a
    // group of buildings near the enemy or ahead of the attack. Set down beside them, it walks
    // in and holds them: spotting for the guns from the roofs, raiding what stands near, a
    // foothold for the army to come up to.
    if let Some(l) = &b.landing {
        let left = l.troops.iter().filter(|id| census.get(**id).map_or(false, |c| c.hp > 0.0)).count();
        if left == 0 || tick > l.until {
            b.landing = None;
        }
    }
    // The landing party: as many foot soldiers as the transports we have can lift (a handful),
    // kept back from the attacks at the rally point. Released if no landing comes of it.
    let lift: i32 = live.iter().filter(|t| matches!(role(t), Role::Transport | Role::Carrier)).map(|t| defs.get(t.def).transport.as_ref().map_or(0, |c| c.capacity as i32)).sum();
    b.party.retain(|id| free.iter().any(|m| m.id == *id && !matches!(m.order, Some(Order::Load(_)))));
    let want_party = if pf.landing > 0.0 && b.landing.is_none() && tick > 1200 && !off("landing") { lift.min((6.0 + 6.0 * pf.landing) as i32) } else { 0 };
    if want_party == 0 || tick > b.party_since + 3600 {
        if !b.party.is_empty() && want_party > 0 {
            b.landing_next = tick + 1800;
        }
        b.party.clear();
    }
    if b.party.is_empty() {
        b.party_since = tick;
    }
    if want_party > 0 && tick >= b.landing_next.saturating_sub(600) {
        let mut have: i32 = b.party.iter().filter_map(|id| census.get(*id)).map(|c| defs.get(c.def).cargo as i32).sum();
        let mut cands: Vec<&Mine> = free.iter().filter(|m| defs.get(m.def).is_infantry() && defs.get(m.def).cargo > 0 && !b.party.contains(&m.id) && !matches!(m.order, Some(Order::Load(_)))).copied().collect();
        cands.sort_by(|x, y| (x.pos.dist(rally), x.id).partial_cmp(&(y.pos.dist(rally), y.id)).unwrap());
        for m in cands {
            let c = defs.get(m.def).cargo as i32;
            if have + c <= want_party {
                have += c;
                b.party.push(m.id);
            }
        }
    }
    if b.landing.is_none() && pf.landing > 0.0 && tick >= b.landing_next && tick > 1800 && !off("landing") {
        let idle = |t: &&&Mine| t.orders == 0 && riders(t.id) == 0 && !orders.contains_key(&t.id);
        let air: Vec<&Mine> = live.iter().filter(|t| role(t) == Role::Transport).filter(idle).copied().collect();
        let apcs: Vec<&Mine> = live.iter().filter(|t| role(t) == Role::Carrier && t.pos.dist(rally) < 45.0).filter(idle).copied().collect();
        let swim: Vec<&Mine> = apcs.iter().filter(|t| defs.get(t.def).mv.amphibious).copied().collect();
        let troops: Vec<&Mine> = free.iter().filter(|m| b.party.contains(&m.id) && m.pos.dist(rally) < 55.0 && !matches!(m.order, Some(Order::Load(_)))).copied().collect();
        let cap_of = |v: &[&Mine]| v.iter().map(|t| defs.get(t.def).transport.as_ref().map_or(0, |c| c.capacity as i32)).sum::<i32>();
        // (kind, transports): flying, swimming, driving.
        let mut ways: Vec<(u8, Vec<&Mine>)> = vec![];
        if !air.is_empty() {
            ways.push((0, air.clone()));
        }
        if !swim.is_empty() {
            ways.push((1, swim.clone()));
        }
        if !apcs.is_empty() {
            ways.push((2, apcs.clone()));
        }
        let target_zone = if let Phase::Attack(zi) = b.phase { Some(zi) } else { None };
        // Enemy guns and anti-air covering a straight run from `a` to `q`.
        let along = |a: V2, q: V2, aa: bool| -> f32 {
            b.memory
                .values()
                .filter(|s| !s.air && (s.known || s.structure))
                .filter(|s| {
                    let inf = b.info[s.def as usize];
                    let r = if aa { if inf.aa > 0.0 { inf.range.max(60.0) + 10.0 } else { -1.0 } } else if inf.threat > 0.0 { inf.range + 5.0 } else { -1.0 };
                    let ab = q - a;
                    let t = ((s.pos - a).dot(ab) / ab.len2().max(1.0)).clamp(0.0, 1.0);
                    (a + ab * t).dist(s.pos) < r
                })
                .map(|s| if aa { b.info[s.def as usize].aa } else { b.info[s.def as usize].threat })
                .sum()
        };
        let crosses_water = |a: V2, q: V2| (1..40).any(|k| {
            let (x, y) = map.cell_of(a + (q - a) * (k as f32 / 40.0));
            map.inside(x, y) && map.blocked[y as usize * n + x as usize] == OCC_DEEP
        });
        let force: f32 = troops.iter().map(|m| threat(m)).sum();
        let mut best: Option<(f32, usize, V2, Vec<usize>)> = None;
        for (wi, (way, ts)) in ways.iter().enumerate() {
            let cap = cap_of(ts).min(troops.len() as i32);
            if cap < 3 {
                continue;
            }
            let from = ts.iter().fold(v2(0.0, 0.0), |a, t| a + t.pos) * (1.0 / ts.len() as f32);
            for h in refuges.iter() {
                let z = &b.zones[b.zone_at(h.1)];
                if h.1.dist(rally) < 50.0 || z.front < 0.3 || z.front > 0.75 + 0.3 * pf.landing || b.held.contains_key(&h.0) {
                    continue;
                }
                let ok_way = match way {
                    0 => z.comp != 0,
                    1 => z.areach,
                    _ => z.reachable,
                };
                if !ok_way {
                    continue;
                }
                let group: Vec<usize> = (0..refuges.len()).filter(|&j| refuges[j].1.dist(h.1) < 16.0 && !b.held.contains_key(&refuges[j].0)).collect();
                let room: i32 = group.iter().map(|&j| refuges[j].2).sum();
                let take = room.min(cap);
                if take < 3 {
                    continue;
                }
                let danger = enemy_near(h.1, 45.0);
                if danger > force * (0.6 + 2.0 * pf.landing) + 60.0 * pf.landing {
                    continue;
                }
                if *way == 0 && (z.enemy_aa > 60.0 * pf.landing || along(rally, h.1, true) > 60.0 * pf.landing) {
                    continue;
                }
                // Worth: what of the enemy's stands near (to watch and to raid), ahead of the
                // attack, forward, and for an amphibious crossing a way round the bridges.
                let prize: f32 = b.memory.values().filter(|s| s.structure && s.pos.dist(h.1) < 70.0).map(|s| b.info[s.def as usize].mass).sum();
                let mut score = take as f32 * (1.0 + prize / 500.0).min(3.0) * (0.5 + z.front);
                if let Some(zi) = target_zone {
                    let tz = b.zones[zi].pos;
                    if h.1.dist(tz) < 70.0 || (h.1.dist(tz) < b.step.dist(tz) && h.1.dist(b.step) < 70.0) {
                        score *= 2.0;
                    }
                }
                if *way == 1 {
                    if !z.reachable {
                        score *= 1.8;
                    } else if crosses_water(from, h.1) {
                        score *= 1.3;
                    }
                }
                if *way != 0 {
                    score /= 1.0 + along(from, h.1, false) / 300.0;
                }
                score /= 60.0 + from.dist(h.1) * 0.5;
                score /= 1.0 + danger / 200.0;
                if best.as_ref().map_or(true, |x| score > x.0) {
                    best = Some((score, wi, h.1, group));
                }
            }
        }
        if let Some((_, wi, at, group)) = best {
            let ts = &ways[wi].1;
            let mut rooms: Vec<(u32, i32)> = group.iter().map(|&j| (refuges[j].0, refuges[j].2)).collect();
            let from = ts.iter().fold(v2(0.0, 0.0), |a, t| a + t.pos) * (1.0 / ts.len() as f32);
            let drop = free_at(at + (from - at).norm() * 7.0);
            let mut pool: Vec<&Mine> = troops.clone();
            pool.sort_by(|x, y| (x.pos.dist(from), x.id).partial_cmp(&(y.pos.dist(from), y.id)).unwrap());
            let (mut sent, mut carriers, mut houses) = (vec![], vec![], vec![]);
            for t in ts.iter() {
                let mut room = defs.get(t.def).transport.as_ref().map_or(0, |c| c.capacity as i32);
                let mut load = vec![];
                pool.retain(|m| {
                    let c = defs.get(m.def).cargo as i32;
                    let Some(h) = rooms.iter_mut().find(|h| h.1 >= c) else { return true };
                    if c > room {
                        return true;
                    }
                    room -= c;
                    h.1 -= c;
                    load.push((m.id, h.0));
                    false
                });
                if load.len() < 2 {
                    continue;
                }
                orders.insert(t.id, Order::Load(load[0].0));
                for (id, _) in load.iter().skip(1) {
                    then.push((t.id, Order::Load(*id)));
                }
                then.push((t.id, Order::Unload { pos: drop, radius: 6.0 }));
                then.push((t.id, Order::Move(rally)));
                carriers.push(t.id);
                for (id, h) in load {
                    orders.insert(id, Order::Load(t.id));
                    then.push((id, Order::Load(h)));
                    b.boarding.insert(id, tick + 1500);
                    sent.push(id);
                    if !houses.contains(&h) {
                        houses.push(h);
                    }
                }
            }
            if !sent.is_empty() {
                for h in &houses {
                    b.held.insert(*h, tick + 4200);
                }
                free.retain(|m| !sent.contains(&m.id));
                b.party.retain(|id| !sent.contains(id));
                b.ops[0] += 1;
                b.ops[3 + ways[wi].0 as usize] += 1;
                b.note += &format!(" landing {} by {} to {:.0},{:.0}", sent.len(), ["air", "amphibious", "land"][ways[wi].0 as usize], at.x, at.y);
                b.landing = Some(Landing { troops: sent, carriers, until: tick + 1500 });
                b.landing_next = tick + (2400.0 / (0.5 + pf.landing)) as u32;
            }
        } else {
            // Nothing worth a landing now: look again in a while.
            b.landing_next = tick + 300;
        }
    }

    // The party waits at the rally point, out of the attacks and the garrisons.
    for m in free.iter().filter(|m| b.party.contains(&m.id)) {
        if !matches!(orders.get(&m.id), Some(Order::Load(_))) && m.pos.dist(rally) > 20.0 {
            go(&mut orders, m, Order::Move(free_at(rally - b.fwd * 6.0)));
        }
    }
    free.retain(|m| !b.party.contains(&m.id));
    let op_carriers: Vec<u32> = b.landing.as_ref().map_or(vec![], |l| l.carriers.clone());
    let on_op = |id: u32| op_carriers.contains(&id);
    let near_front = |q: V2| q.dist(rally) < 70.0 || q.dist(start) < 50.0;
    let step = b.step;
    let on_the_way = |q: V2| attacking && q.dist(step) < 45.0;
    let mut housed = census.list.iter().filter(|r| r.owner == me && r.hosted && census.get(r.host).map_or(false, |h| h.structure && defs.get(h.def).is_garrison())).count() as u32;
    housed += mine.iter().filter(|m| matches!(m.order, Some(Order::Load(t)) if census.get(t).map_or(false, |h| h.structure))).count() as u32;
    let mut houses: Vec<(u32, V2, i32)> = census
        .list
        .iter()
        .filter(|c| c.structure && c.done && c.hp > 0.0 && defs.get(c.def).is_garrison() && (c.team == team || c.owner == NEUTRAL) && (near_front(c.pos) || on_the_way(c.pos)) && (on_the_way(c.pos) || b.zones[b.zone_at(c.pos)].safe()))
        .map(|c| (c.id, c.pos, defs.get(c.def).transport.as_ref().map_or(0, |t| t.capacity as i32) - riders(c.id)))
        .collect();
    {
        let waiting = matches!(b.phase, Phase::Gather) || pf.garrison >= 6;
        let mut k = 0;
        while k < free.len() && housed < pf.garrison * 2 {
            let m = free[k];
            let d = defs.get(m.def);
            let in_body = b.body.contains(&m.id);
            // Spotters with binoculars on a roof see furthest of all.
            let fits = |h: &&mut (u32, V2, i32)| h.2 >= d.cargo as i32 && if in_body { on_the_way(h.1) && h.1.dist(m.pos) < 25.0 } else { waiting && housed < pf.garrison && near_front(h.1) && h.1.dist(m.pos) < 60.0 };
            let house = if d.is_infantry() && d.cargo > 0 && !b.joiners.contains(&m.id) { houses.iter_mut().filter(|h| fits(h)).min_by(|x, y| x.1.dist(m.pos).total_cmp(&y.1.dist(m.pos))) } else { None };
            match house {
                Some(h) => {
                    h.2 -= d.cargo as i32;
                    housed += 1;
                    orders.insert(m.id, Order::Load(h.0));
                    free.swap_remove(k);
                }
                None => k += 1,
            }
        }
    }
    for c in census.list.iter().filter(|c| c.owner == me && c.structure && defs.get(c.def).is_garrison() && riders(c.id) > 0) {
        let z = &b.zones[b.zone_at(c.pos)];
        let keep = b.held.contains_key(&c.id) || enemy_near(c.pos, 45.0) > 0.0 || shelled(c.pos) > 0.0;
        if !keep && !near_front(c.pos) && !(attacking && c.pos.dist(step) < 70.0) && tick.saturating_sub(z.alert) > 1200 && tick % 100 < 10 {
            out.push(Command::Orders { ids: vec![c.id], orders: vec![Order::Unload { pos: c.pos + (rally - c.pos).norm() * 6.0, radius: 3.0 }], queue: false });
        }
    }
    b.stats[2] = housed;
    b.stats[3] = abroad.len() as u32;
    // Units already walking to a building keep doing so.
    free.retain(|m| !matches!(m.order, Some(Order::Load(_))));

    let main = free;
    b.body.retain(|id| main.iter().any(|m| m.id == *id));
    b.joiners.retain(|id| main.iter().any(|m| m.id == *id));
    let total: f32 = main.iter().map(|m| threat(m)).sum();
    let min_force = pf.min_force * (1.0 + 0.4 * b.wave as f32).min(4.0);
    let sum = |v: &[&Mine]| v.iter().map(|m| threat(m)).sum::<f32>();
    let within = |v: &[&Mine], at: V2, r: f32| v.iter().filter(|m| m.pos.dist(at) < r).map(|m| threat(m)).sum::<f32>();
    let mid = |v: &[&Mine]| free_at(v.iter().fold(v2(0.0, 0.0), |a, m| a + m.pos * threat(m).max(1.0)) * (1.0 / v.iter().map(|m| threat(m).max(1.0)).sum::<f32>().max(1.0)));
    // Units of the main force away from the rally zone: those that stand still without firing
    // are held by something they cannot hit (no line of fire) and would wait on attack-move for
    // ever, so they walk back without stopping; those shelling something far off are busy.
    let still = |m: &Mine| census.get(m.id).map_or(false, |c| c.vel.len() < 0.05);
    let busy = |m: &Mine| census.get(m.id).map_or(false, |c| c.firing);
    let to_rally = |m: &Mine| {
        let back = matches!(m.order, Some(Order::Move(q)) if q.dist(rally) < 5.0);
        if back || (m.pos.dist(rally) > 60.0 && still(m) && !busy(m)) { Order::Move(rally) } else { Order::AttackMove(rally) }
    };
    let mut centre = rally;
    if b.strategy == Strategy::Kill && !main.is_empty() {
        // M27's commander kill: everything on the enemy commander.
        let o = match b.memory.get(&b.kill) {
            Some(s) if s.known => Order::Attack(b.kill),
            Some(s) => Order::AttackMove(s.pos),
            None => Order::AttackMove(rally),
        };
        for m in &main {
            go(&mut orders, m, o);
        }
        centre = mid(&main);
        b.phase = Phase::Gather;
        b.body.clear();
        b.joiners.clear();
    } else if !main.is_empty() {
        if matches!(b.phase, Phase::Gather) {
            b.body.clear();
            b.joiners.clear();
        }
        let body: Vec<&Mine> = main.iter().filter(|m| b.body.contains(&m.id)).copied().collect();
        let joiners: Vec<&Mine> = main.iter().filter(|m| b.joiners.contains(&m.id)).copied().collect();
        let reserve: Vec<&Mine> = main.iter().filter(|m| !b.body.contains(&m.id) && !b.joiners.contains(&m.id)).copied().collect();
        let force = sum(&body);
        if !body.is_empty() {
            centre = mid(&body);
        }
        if let Phase::Attack(zi) = b.phase {
            let z = &b.zones[zi];
            let local = within(&body, centre, 45.0);
            let foe = enemy_near(centre, 70.0);
            let cleared = tick - z.seen < 100 && b.worth(cx, team, z) <= 0.0 && z.enemy <= 0.0;
            if body.is_empty() {
                b.phase = Phase::Gather;
                b.wave += 1;
            } else if (foe > 0.0 && local < pf.retreat * foe) || force < pf.retreat * b.nerve * b.defence(z) {
                b.phase = Phase::Retreat(tick + 400);
                b.wave += 1;
                // Thrown back from the same zone again and again: try somewhere else for a while.
                b.lost = (zi, if b.lost.0 == zi { b.lost.1 + 1 } else { 1 });
                if b.lost.1 >= 3 {
                    b.avoid = (zi, tick + 3000);
                    b.lost = (zi, 0);
                }
            } else if cleared || !z.reachable {
                if cleared {
                    b.lost = (zi, 0);
                }
                b.phase = match b.pick_target(cx, team, centre, force, &Profile { attack: pf.attack * b.nerve, ..*pf }) {
                    Some(t) => Phase::Attack(t),
                    None => Phase::Gather,
                };
                b.best = (f32::MAX, tick);
                b.pushes = 0;
            } else {
                // No progress for a minute (a stand-off without a line of fire, a cliff in the
                // way): drive in for a while; after three tries leave that zone alone.
                let d = centre.dist(z.enemy_pos) + z.danger() * 0.02;
                if d < b.best.0 - 8.0 {
                    b.best = (d, tick);
                } else if tick > b.best.1 + 600 {
                    b.best = (d, tick);
                    b.pushes += 1;
                    if b.pushes > 3 {
                        b.avoid = (zi, tick + 3000);
                        b.phase = Phase::Retreat(tick + 300);
                        b.wave += 1;
                    }
                }
            }
        }
        b.note = format!("main {:.0} body {:.0} joiners {:.0}", total, force, sum(&joiners));
        match b.phase {
            Phase::Gather => {
                for m in &main {
                    go(&mut orders, m, to_rally(m));
                }
                // Stalemates end: ten minutes of waiting halve the superiority asked for.
                let nerve = 1.0 - 0.5 * (tick.saturating_sub(b.busy) as f32 / 6000.0).min(1.0);
                let eager = Profile { attack: pf.attack * nerve, ..*pf };
                // What has arrived decides, not the share of the army: units keep coming.
                let room = 40.0 + (main.len() as f32).sqrt() * 2.0;
                let ready = within(&main, rally, room);
                // Units in a fight far away (artillery shelling a base) do not hold the next attack up.
                let away: f32 = main.iter().filter(|m| m.pos.dist(rally) > room + 20.0 && still(m) && busy(m)).map(|m| threat(m)).sum();
                b.note += &format!(" at rally {:.0} need {:.0} nerve {:.2}", ready, min_force, nerve);
                if ready >= min_force && ready >= 0.4 * (total - away) && tick >= b.calm {
                    if let Some(t) = b.pick_target(cx, team, rally, ready, &eager) {
                        b.nerve = nerve;
                        b.phase = Phase::Attack(t);
                        b.best = (f32::MAX, tick);
                        b.pushes = 0;
                        // Everything near the rally point goes: the first bound waits for it.
                        b.body = main.iter().filter(|m| m.pos.dist(rally) < (room * 2.0).max(80.0)).map(|m| m.id).collect();
                        b.step = rally;
                        b.step_tick = tick;
                    }
                }
            }
            Phase::Attack(zi) => {
                let path = b.route(b.zone_at(b.step), zi, force);
                let dest = match path.get(1) {
                    Some(&next) if next != zi => b.zones[next].pos,
                    _ => b.zones[zi].enemy_pos,
                };
                // Bounds: the body moves from step to step, and the next step is only taken once
                // most of it stands at this one (units busy shooting at something far off do not
                // count), so fast units wait for slow ones and the army arrives as one.
                let marching: Vec<&Mine> = body.iter().filter(|m| !busy(m)).copied().collect();
                // Where the march is: the plain mean of the marching units, artillery left out (it
                // stays behind on purpose; weighted by worth one heavy gun would drag it back).
                let front: Vec<&Mine> = marching.iter().filter(|m| !b.info[m.def as usize].indirect).copied().collect();
                let mc = if front.is_empty() { centre } else { free_at(front.iter().fold(v2(0.0, 0.0), |a, m| a + m.pos) * (1.0 / front.len() as f32)) };
                // The enemy units the body is up against: in sight or on radar within the reach of
                // most of its guns (not of the one heavy battery with it), and enough of them to
                // matter: a lone scout does not stop the march, the guns take it on the way.
                let mut ranges: Vec<f32> = body.iter().map(|m| b.info[m.def as usize].range).collect();
                ranges.sort_by(|x, y| x.total_cmp(y));
                let reach = ranges.get(ranges.len() * 3 / 4).copied().unwrap_or(40.0).clamp(30.0, 90.0);
                let foes: Vec<(u32, Seen)> = b.memory.iter().filter(|(_, s)| s.known && !s.air && !s.structure && b.info[s.def as usize].threat > 0.0 && s.pos.dist(mc) < reach + 15.0).map(|(id, s)| (*id, *s)).collect();
                let foe_force: f32 = foes.iter().map(|(_, s)| b.info[s.def as usize].threat).sum();
                let fighting = foe_force >= 0.1 * force || foes.iter().any(|(_, s)| s.pos.dist(mc) < 45.0);
                let foes = if fighting && !off("crescent") { foes } else { vec![] };
                let r = 14.0 + (body.len() as f32).sqrt() * 2.5;
                let at_step = marching.iter().filter(|m| m.pos.dist(b.step) < r).count() as f32 >= 0.6 * marching.len() as f32;
                let left = b.step.dist(dest);
                if off("bounds") {
                    b.step = dest;
                } else if foes.is_empty() && left > 3.0 && (at_step || tick > b.step_tick + 300) {
                    // After a timeout (someone stuck) the next bound starts from where the body is,
                    // so the step never runs away from it.
                    let from = if at_step { b.step } else { mc };
                    b.step = free_at(from + (dest - from).norm() * from.dist(dest).min(BOUND));
                    b.step_tick = tick;
                }
                b.note += &format!(" to target {:.0} step {:.0} at step {} foes {} pushes {}", mc.dist(b.zones[zi].pos), mc.dist(b.step), at_step, foes.len(), b.pushes);
                let push = b.pushes > 0 && tick < b.best.1 + 200;
                if std::env::var("LR_AIDEBUG").is_ok() && me == 0 && tick % 300 == 0 {
                    let mut h = String::new();
                    for m in &body {
                        let o = match m.order { Some(Order::Move(_)) => "M", Some(Order::AttackMove(_)) => "A", Some(Order::Attack(_)) => "T", Some(Order::Load(_)) => "L", None => "-", _ => "?" };
                        h += &format!(" {}:{:.0}/{:.0}{}{}", defs.get(m.def).id, m.pos.dist(b.step), m.pos.dist(mc), o, if busy(m) { "*" } else { "" });
                    }
                    eprintln!("t={} step->dest {:.0} body:{}", tick / 10, b.step.dist(dest), h);
                }
                if foes.is_empty() {
                    // On the march: eyes in front (binoculars see further than the guns reach),
                    // then the short guns, the long ones behind them.
                    let ahead = if (dest - b.step).len2() > 1.0 { (dest - b.step).norm() } else { b.fwd };
                    let across = ahead.perp();
                    let eyes = |m: &Mine| b.info[m.def as usize].eyes >= b.info[m.def as usize].range * 1.3 + 30.0;
                    let mut rows: Vec<&Mine> = body.iter().filter(|m| !eyes(m)).copied().collect();
                    rows.sort_by(|x, y| (b.info[x.def as usize].range, x.id).partial_cmp(&(b.info[y.def as usize].range, y.id)).unwrap());
                    let cols = (rows.len() as f32).sqrt().ceil().max(1.0) as usize * 2;
                    for (k, m) in rows.iter().enumerate() {
                        let (c, r) = ((k % cols) as f32 - (cols.min(rows.len()) as f32 - 1.0) * 0.5, (k / cols) as f32);
                        let at = map.clamp(b.step + across * (c * 3.0) - ahead * (r * 4.0));
                        go(&mut orders, m, if push && b.info[m.def as usize].role == Role::Tank && !b.info[m.def as usize].infantry { Order::Move(at) } else { Order::AttackMove(at) });
                    }
                    let scouts: Vec<&Mine> = body.iter().filter(|m| eyes(m)).copied().collect();
                    for (k, m) in scouts.iter().enumerate() {
                        let c = k as f32 - (scouts.len() as f32 - 1.0) * 0.5;
                        go(&mut orders, m, Order::AttackMove(free_at(b.step + ahead * 16.0 + across * (c * 8.0))));
                    }
                } else {
                    // In the fight: a crescent round the enemy, every gun at its own range, the
                    // wings on the enemy's flanks (side armour is thinner than the front), and the
                    // direct-fire guns that reach concentrate on one target at a time.
                    let (mut fc, mut w) = (v2(0.0, 0.0), 0.0);
                    for (_, s) in &foes {
                        let t = b.info[s.def as usize].threat.max(10.0);
                        fc = fc + s.pos * t;
                        w += t;
                    }
                    let fc = fc * (1.0 / w.max(1.0));
                    let to_us = if (centre - fc).len2() > 1.0 { (centre - fc).norm() } else { b.fwd * -1.0 };
                    let mut lineup: Vec<&Mine> = body.clone();
                    lineup.sort_by(|x, y| ((x.pos - fc).dot(to_us.perp()), x.id).partial_cmp(&((y.pos - fc).dot(to_us.perp()), y.id)).unwrap());
                    let n = lineup.len();
                    let span = (0.15 * n as f32).min(1.6);
                    let (cs, sn) = (|a: f32| fm::cos(a), |a: f32| fm::sin(a));
                    for (k, m) in lineup.iter().enumerate() {
                        let inf = b.info[m.def as usize];
                        let frac = if n > 1 { k as f32 / (n - 1) as f32 - 0.5 } else { 0.0 };
                        let a = frac * span;
                        let dir = v2(to_us.x * cs(a) - to_us.y * sn(a), to_us.x * sn(a) + to_us.y * cs(a));
                        let slot = free_at(fc + dir * (inf.range * 0.8).clamp(10.0, 100.0));
                        // Guns that fire on the move drive round to their place; set-up weapons
                        // and infantry (which goes into cover) stop at the first target in reach.
                        let flank = !inf.hold && !inf.infantry && inf.role == Role::Tank && m.pos.dist(slot) > 14.0 && m.pos.dist(fc) > inf.range * 0.5;
                        let o = if flank { Order::Move(slot) } else { Order::AttackMove(slot) };
                        // Slots drift with the enemy: an order to near the same place stands.
                        let settled = matches!(m.order, Some(Order::AttackMove(q)) | Some(Order::Move(q)) if q.dist(slot) < if inf.infantry || inf.hold { 12.0 } else { 8.0 });
                        if !settled {
                            go(&mut orders, m, o);
                        }
                    }
                    // Focus fire, greedy: the target the guns in reach kill fastest for its worth,
                    // only as many guns on it as kill it in a few seconds, then the next.
                    let mut gunners: Vec<&Mine> = body.iter().filter(|m| {
                        let inf = b.info[m.def as usize];
                        !inf.infantry && !inf.hold && !inf.indirect && inf.dps > 0.0 && !matches!(orders.get(&m.id), Some(Order::Move(_)))
                    }).copied().collect();
                    let mut focused = 0;
                    for _ in 0..if off("focus") { 0 } else { 3 } {
                        let dmg = |m: &Mine, s: &Seen| {
                            let inf = b.info[m.def as usize];
                            let armor = defs.get(s.def).armor_id;
                            inf.dps * inf.dtypes.iter().enumerate().map(|(t, sh)| sh * defs.armor_mult(t, armor)).sum::<f32>()
                        };
                        let in_reach = |m: &Mine, s: &Seen| s.pos.dist(m.pos) < b.info[m.def as usize].range * 0.92;
                        let best = foes.iter().filter_map(|(id, s)| {
                            let hp = census.get(*id).map_or(0.0, |c| c.hp);
                            let fire: f32 = gunners.iter().filter(|m| in_reach(m, s)).map(|m| dmg(m, s)).sum();
                            (hp > 0.0 && fire > 0.0).then(|| (b.info[s.def as usize].threat.max(20.0) * fire / hp, *id, *s, hp))
                        }).max_by(|x, y| x.0.total_cmp(&y.0));
                        let Some((_, id, s, hp)) = best else { break };
                        let mut fire = 0.0;
                        gunners.sort_by(|x, y| (x.pos.dist(s.pos), x.id).partial_cmp(&(y.pos.dist(s.pos), y.id)).unwrap());
                        gunners.retain(|m| {
                            if fire < hp * 0.35 && in_reach(m, &s) {
                                fire += dmg(m, &s);
                                focused += !same(m.order, Order::Attack(id)) as u32;
                                if same(m.order, Order::Attack(id)) {
                                    orders.remove(&m.id);
                                } else {
                                    orders.insert(m.id, Order::Attack(id));
                                }
                                false
                            } else {
                                true
                            }
                        });
                    }
                    b.acts[0] += focused;
                }
                // Reinforcements: gather, set out as a group, merge on arrival.
                for m in &joiners {
                    if m.pos.dist(centre) < 40.0 {
                        b.body.push(m.id);
                    } else {
                        go(&mut orders, m, Order::AttackMove(centre));
                    }
                }
                b.joiners.retain(|id| !b.body.contains(id));
                let ready: Vec<&&Mine> = reserve.iter().filter(|m| m.pos.dist(rally) < 40.0).collect();
                let ready_force: f32 = ready.iter().map(|m| threat(m)).sum();
                if ready_force >= (force * 0.25).max(min_force * 0.5).min(3000.0) {
                    b.joiners.extend(ready.iter().map(|m| m.id));
                }
                for m in &reserve {
                    // Set down by a carrier next to the body, or caught up with it: part of it now.
                    if m.pos.dist(centre) < 30.0 {
                        b.body.push(m.id);
                    } else {
                        go(&mut orders, m, to_rally(m));
                    }
                }
            }
            Phase::Retreat(until) => {
                for m in &main {
                    go(&mut orders, m, Order::Move(rally));
                }
                if tick > until || within(&main, rally, 50.0) >= 0.5 * total {
                    // A breather before the next attack, so that a fall-back is not followed by
                    // the same attack in the next second.
                    b.phase = Phase::Gather;
                    b.calm = tick + 200;
                }
            }
        }
    } else {
        b.phase = Phase::Gather;
    }

    if !matches!(b.phase, Phase::Gather) {
        b.busy = tick;
    }

    // --- units on another land mass (flown there): take what they can, else hold together
    if !abroad.is_empty() {
        let force: f32 = abroad.iter().map(|m| threat(m)).sum();
        let at = abroad.iter().fold(v2(0.0, 0.0), |a, m| a + m.pos) * (1.0 / abroad.len() as f32);
        let comp = b.zones[b.zone_at(at)].comp;
        let target = (0..b.zones.len()).filter(|&i| b.zones[i].comp == comp && b.worth(cx, team, &b.zones[i]) > 0.0 && force >= pf.attack * 0.8 * b.defence(&b.zones[i])).max_by(|&x, &y| {
            let s = |i: usize| b.worth(cx, team, &b.zones[i]) / (60.0 + at.dist(b.zones[i].pos));
            s(x).total_cmp(&s(y))
        });
        for m in &abroad {
            go(&mut orders, m, Order::AttackMove(target.map_or(free_at(at), |i| b.zones[i].enemy_pos)));
        }
    }

    // --- scavengers revive the wrecks lying in our zones (the dearest first), reclaim debris
    // when there is nothing to revive, and wait at the rally zone otherwise
    for m in live.iter().filter(|m| role(m) == Role::Scavenger && m.orders == 0) {
        let ok = |q: V2| b.zones[b.zone_at(q)].safe() && b.zones[b.zone_at(q)].comp == home_comp;
        let wreck = cx.wrecks.0.iter().filter(|w| w.4 <= 0.0 && w.2 >= 40.0 && ok(w.1)).max_by(|x, y| (x.2 / (40.0 + x.1.dist(m.pos))).total_cmp(&(y.2 / (40.0 + y.1.dist(m.pos)))));
        let scrap = cx.wrecks.0.iter().filter(|w| w.2 >= 10.0 && w.1.dist(m.pos) < 80.0 && ok(w.1)).min_by(|x, y| x.1.dist(m.pos).total_cmp(&y.1.dist(m.pos)));
        match (wreck, scrap) {
            (Some(w), _) => {
                // What cannot come back (a plot built over) is taken apart afterwards.
                orders.insert(m.id, Order::ReviveArea { pos: w.1, radius: 18.0 });
                then.push((m.id, Order::ReclaimArea { pos: w.1, radius: 18.0 }));
                b.stats[0] += 1;
            }
            (None, Some(w)) if p.mass < p.max_mass * 0.8 => {
                orders.insert(m.id, Order::ReclaimArea { pos: w.1, radius: 12.0 });
            }
            _ => go(&mut orders, m, Order::Move(rally)),
        }
    }

    // --- air transports (M27's and RNG's transport logic, reduced to three jobs): an engineer
    // to an island with free deposits, the army to an enemy it cannot walk to, raiders to a
    // far-away raid zone. Passengers walk to the transport, it sets them down and flies home.
    let carriers: Vec<&Mine> = live.iter().filter(|m| role(m) == Role::Transport).copied().collect();
    let mut used: Vec<u32> = vec![];
    for t in &carriers {
        let aboard = riders(t.id) > 0;
        if t.orders != 0 || on_op(t.id) || orders.contains_key(&t.id) {
            continue;
        }
        if aboard {
            // Something went wrong on the way: bring them home.
            orders.insert(t.id, Order::Unload { pos: rally, radius: 8.0 });
            continue;
        }
        let cap = defs.get(t.def).transport.as_ref().map_or(0, |c| c.capacity as i32);
        let fits = |list: &[&Mine], used: &[u32]| -> Vec<u32> {
            let mut room = cap;
            let mut v = vec![];
            for m in list {
                let c = defs.get(m.def).cargo as i32;
                if c > 0 && c <= room && !used.contains(&m.id) && !defs.get(m.def).is_air() {
                    room -= c;
                    v.push(m.id);
                }
            }
            v
        };
        let quiet = |z: &Zone| z.enemy_aa < 100.0 && z.enemy_air <= 0.0;
        let mut job: Option<(Vec<u32>, V2)> = None;
        // 1. An engineer for a land mass with free deposits and none of ours there yet.
        let builders: Vec<&Mine> = mine.iter().filter(|m| defs.get(m.def).is_engineer() && !m.uc && !m.hosted).collect();
        let island = (0..b.zones.len()).find(|&i| {
            let z = &b.zones[i];
            let free = z.deposits.iter().any(|k| !census.list.iter().any(|c| c.structure && c.pos.dist(map.deposits[*k]) < 1.0));
            z.comp != 0 && z.comp != home_comp && free && z.safe() && quiet(z) && !builders.iter().any(|e| b.zones[b.zone_at(e.pos)].comp == z.comp) && !b.boarding.keys().any(|id| builders.iter().any(|e| e.id == *id))
        });
        let commander = defs.factions[p.faction as usize].commander;
        if let Some(zi) = island {
            let eng = builders.iter().filter(|e| Some(e.def) != commander && defs.get(e.def).cargo as i32 <= cap && defs.get(e.def).cargo > 0 && b.zones[b.zone_at(e.pos)].comp == home_comp && !used.contains(&e.id)).min_by(|x, y| x.pos.dist(t.pos).total_cmp(&y.pos.dist(t.pos)));
            if let Some(e) = eng {
                job = Some((vec![e.id], b.zones[zi].pos));
            }
        }
        // 2. The army to an enemy across the water, set down in the calmest zone next to it.
        if job.is_none() {
            let spare: Vec<&Mine> = land.iter().filter(|m| !b.body.contains(&m.id) && !b.joiners.contains(&m.id) && !b.raiders.contains(&m.id) && m.pos.dist(rally) < 60.0).copied().collect();
            let ready: f32 = spare.iter().map(|m| threat(m)).sum();
            let target = (0..b.zones.len()).filter(|&i| b.zones[i].comp != 0 && b.zones[i].comp != home_comp && b.worth(cx, team, &b.zones[i]) > 0.0).max_by(|&x, &y| {
                let s = |i: usize| b.worth(cx, team, &b.zones[i]) / (100.0 + b.zones[i].d_home) / (1.0 + b.zones[i].enemy_aa / 200.0);
                s(x).total_cmp(&s(y))
            });
            if let (Some(ti), true) = (target, ready >= pf.min_force * 0.5) {
                let c = b.zones[ti].comp;
                let drop = (0..b.zones.len()).filter(|&i| b.zones[i].comp == c && quiet(&b.zones[i]) && b.zones[i].pos.dist(b.zones[ti].pos) < 150.0).min_by(|&x, &y| {
                    let s = |i: usize| b.zones[i].danger() * 10.0 + b.zones[i].pos.dist(b.zones[ti].pos) - b.zones[i].ally * 5.0;
                    s(x).total_cmp(&s(y))
                });
                let who = fits(&spare, &used);
                if let (Some(di), false) = (drop, who.is_empty()) {
                    job = Some((who, b.zones[di].pos));
                }
            }
        }
        // 3. Raiders to a raid zone that is a long drive away.
        if job.is_none() {
            if let Some(zi) = b.raid_zone.filter(|&i| quiet(&b.zones[i]) && b.zones[i].danger() <= 0.0) {
                let far: Vec<&Mine> = land.iter().filter(|m| b.raiders.contains(&m.id) && m.pos.dist(b.zones[zi].pos) > 140.0 && m.pos.dist(t.pos) < 120.0).copied().collect();
                let who = fits(&far, &used);
                if who.len() >= 2 {
                    let z = b.zones[zi].pos;
                    job = Some((who, free_at(z + (start - z).norm() * 18.0)));
                }
            }
        }
        match job {
            Some((who, to)) => {
                orders.insert(t.id, Order::Load(who[0]));
                for id in who.iter().skip(1) {
                    then.push((t.id, Order::Load(*id)));
                }
                then.push((t.id, Order::Unload { pos: to, radius: 8.0 }));
                then.push((t.id, Order::Move(rally)));
                b.stats[1] += 1;
                for id in who {
                    orders.insert(id, Order::Load(t.id));
                    b.boarding.insert(id, tick + 450);
                    used.push(id);
                }
            }
            None => go(&mut orders, t, Order::Move(rally)),
        }
    }

    // --- storming: infantry that fights at close quarters takes enemy-held buildings and trench
    // pieces near it when it clearly outnumbers who holds them (an Attack order on them becomes a
    // storm for such infantry, and once they are empty they walk in)
    let mut stormers: Vec<&Mine> = land.iter().filter(|m| b.info[m.def as usize].assault > 0.0 && !b.boarding.contains_key(&m.id) && !matches!(m.order, Some(Order::Load(_))) && !matches!(orders.get(&m.id), Some(Order::Load(_)))).copied().collect();
    let odds = defs.rules.garrison_defense.max(1.0) * 1.5;
    let mut storms = 0;
    if !stormers.is_empty() {
        // What each enemy strongpoint near our infantry holds: a building with riders, or a trench
        // piece with enemy infantry in it (attacking one of them storms the piece).
        let mut points: Vec<(u32, V2, f32)> = vec![];
        for (id, s) in b.memory.iter().filter(|(_, s)| s.structure && s.known && defs.get(s.def).is_garrison()) {
            let held: f32 = census.list.iter().filter(|r| r.host == *id && r.hp > 0.0 && hostile(team, r.team)).map(|r| defs.get(r.def).assault.max(1.0)).sum();
            if held > 0.0 {
                points.push((*id, s.pos, held));
            }
        }
        let trenches: Vec<&Cen> = census.list.iter().filter(|c| c.fieldwork && c.done && c.hp > 0.0 && defs.get(c.def).work().map_or(false, |w| w.1 > 0.0)).collect();
        for (id, s) in b.memory.iter().filter(|(_, s)| s.known && !s.structure && b.info[s.def as usize].infantry) {
            if let Some(t) = trenches.iter().find(|t| t.pos.dist(s.pos) < CELL * 0.75) {
                if !points.iter().any(|p| p.1.dist(t.pos) < 0.5) {
                    let held: f32 = b.memory.values().filter(|o| o.known && !o.structure && b.info[o.def as usize].infantry && o.pos.dist(t.pos) < CELL * 0.75).map(|o| b.info[o.def as usize].assault.max(1.0)).sum();
                    points.push((*id, t.pos, held));
                }
            }
        }
        for (id, at, held) in points {
            let near: Vec<&Mine> = stormers.iter().filter(|m| m.pos.dist(at) < 45.0).copied().collect();
            let ours: f32 = near.iter().map(|m| b.info[m.def as usize].assault).sum();
            if ours > held * odds {
                for m in &near {
                    storms += !same(m.order, Order::Attack(id)) as u32;
                    orders.insert(m.id, Order::Attack(id));
                }
                stormers.retain(|m| !near.iter().any(|n| n.id == m.id));
            }
        }
    }
    b.acts[1] += storms;

    // --- land carriers (APCs): infantry waiting at the rally point rides up to the attacking body
    // and is set down behind it; empty carriers wait at the rally point
    let fwd_drop = if attacking && !b.body.is_empty() { Some(free_at(centre + (rally - centre).norm() * 10.0)).filter(|q| q.dist(rally) > 70.0) } else { None };
    for t in live.iter().filter(|m| role(m) == Role::Carrier) {
        let aboard = riders(t.id) > 0;
        if t.orders != 0 || on_op(t.id) || orders.contains_key(&t.id) {
            continue;
        }
        if aboard {
            orders.insert(t.id, Order::Unload { pos: fwd_drop.unwrap_or(rally), radius: 6.0 });
            then.push((t.id, Order::Move(rally)));
            continue;
        }
        let cap = b.info[t.def as usize].carry as i32;
        let mut who: Vec<u32> = vec![];
        if let (Some(_), true) = (fwd_drop, t.pos.dist(rally) < 30.0) {
            let mut room = cap;
            let mut walkers: Vec<&Mine> = land.iter().filter(|m| b.info[m.def as usize].infantry && m.pos.dist(rally) < 40.0 && !b.body.contains(&m.id) && !b.boarding.contains_key(&m.id) && !matches!(orders.get(&m.id), Some(Order::Load(_))) && !matches!(m.order, Some(Order::Load(_)))).copied().collect();
            walkers.sort_by(|x, y| (x.pos.dist(t.pos), x.id).partial_cmp(&(y.pos.dist(t.pos), y.id)).unwrap());
            for m in walkers {
                let c = defs.get(m.def).cargo as i32;
                if c > 0 && c <= room {
                    room -= c;
                    who.push(m.id);
                }
            }
        }
        if who.len() >= 2 {
            orders.insert(t.id, Order::Load(who[0]));
            for id in who.iter().skip(1) {
                then.push((t.id, Order::Load(*id)));
            }
            then.push((t.id, Order::Unload { pos: fwd_drop.unwrap(), radius: 6.0 }));
            then.push((t.id, Order::Move(rally)));
            b.acts[3] += 1;
            for id in who {
                orders.insert(id, Order::Load(t.id));
                b.boarding.insert(id, tick + 450);
            }
        } else {
            go(&mut orders, t, Order::Move(rally));
        }
    }

    // --- micro for everything on the ground that fights
    for m in &land {
        let inf = b.info[m.def as usize];
        if inf.range <= 0.0 || matches!(m.order, Some(Order::Load(_))) || matches!(orders.get(&m.id), Some(Order::Load(_))) {
            continue;
        }
        // Kiting (M27, M28's skirmishers): the longer gun keeps the shorter one at a distance.
        let mut close: Option<(f32, &Seen)> = None;
        let mut target_known = false;
        for s in b.memory.values().filter(|s| s.known && !s.air) {
            let d = s.pos.dist(m.pos);
            target_known |= d < inf.range;
            if !s.structure && b.info[s.def as usize].threat > 0.0 && d < inf.range && close.map_or(true, |c| d < c.0) {
                close = Some((d, s));
            }
        }
        if let Some((d, s)) = close {
            let reach = b.info[s.def as usize].range;
            if inf.range >= 40.0 && reach + 8.0 < inf.range && d < reach + 8.0 {
                let back = map.clamp(m.pos + (m.pos - s.pos).norm() * 14.0);
                let resume = orders.get(&m.id).copied().or(m.order).filter(|o| matches!(o, Order::AttackMove(_))).unwrap_or(Order::AttackMove(m.pos));
                orders.insert(m.id, Order::Move(back));
                then.push((m.id, resume));
                continue;
            }
        }
        // Long guns shell buildings the team remembers but no longer sees (a player's ground
        // fire at ghosts), as long as nothing visible is in reach.
        if inf.role == Role::Arty && inf.indirect && m.pos.dist(centre) < 60.0 && !b.raiders.contains(&m.id) {
            let ghost = |q: V2| b.memory.values().any(|s| s.structure && !s.known && s.pos.dist(q) < 1.0);
            let shelling = matches!(m.order, Some(Order::AttackGround { pos, .. }) if ghost(pos) && pos.dist(m.pos) < inf.range);
            let marching = matches!(orders.get(&m.id), Some(Order::AttackMove(q)) | Some(Order::Move(q)) if q.dist(m.pos) > 25.0);
            if target_known || marching || matches!(b.phase, Phase::Retreat(_)) {
                continue;
            }
            if shelling {
                orders.remove(&m.id);
            } else if let Some(s) = b.memory.values().filter(|s| s.structure && !s.known && s.pos.dist(m.pos) < inf.range * 0.95 && s.pos.dist(m.pos) > 12.0).min_by(|x, y| x.pos.dist(m.pos).total_cmp(&y.pos.dist(m.pos))) {
                orders.insert(m.id, Order::AttackGround { pos: s.pos, radius: 1.5 });
            }
        }
    }

    // --- mobile radars stay just behind the army so that it sees what it can reach
    let anchor = free_at(centre + (start - centre).norm() * 12.0);
    for m in live.iter().filter(|m| role(m) == Role::Radar) {
        go(&mut orders, m, Order::Move(anchor));
    }

    // --- aircraft: fighters go where enemy aircraft were seen, strike aircraft hit the richest
    // zone with the least anti-air in groups and come home (M27's air overseer, much reduced)
    let fighters: Vec<&Mine> = live.iter().filter(|m| role(m) == Role::Fighter).copied().collect();
    let air_zone = (0..b.zones.len()).filter(|&i| b.zones[i].enemy_air > 0.0 && b.zones[i].enemy_aa < 400.0).min_by(|&x, &y| b.zones[x].d_home.total_cmp(&b.zones[y].d_home));
    for m in &fighters {
        match air_zone {
            Some(zi) => go(&mut orders, m, Order::AttackMove(b.zones[zi].enemy_pos)),
            None if m.orders == 0 && m.pos.dist(rally) > 60.0 => go(&mut orders, m, Order::Move(rally)),
            None => {}
        }
    }
    // Suicide drones: each known enemy vehicle near our army or our zones gets as many as it takes
    // to kill it, the dearest first; drones that have a target keep it.
    let mut drones: Vec<&Mine> = live.iter().filter(|m| role(m) == Role::Bomber && b.info[m.def as usize].suicide && m.orders == 0).copied().collect();
    if !drones.is_empty() {
        let mut prey: Vec<(u32, Seen)> = b.memory.iter().filter(|(_, s)| s.known && !s.air && !s.structure && b.info[s.def as usize].threat > 0.0 && (s.pos.dist(centre) < 120.0 || b.zones[b.zone_at(s.pos)].own > 0.0) && b.zones[b.zone_at(s.pos)].enemy_aa < 150.0).map(|(id, s)| (*id, *s)).collect();
        prey.sort_by(|x, y| (b.info[y.1.def as usize].mass, x.0).partial_cmp(&(b.info[x.1.def as usize].mass, y.0)).unwrap());
        for (id, s) in prey {
            if drones.is_empty() {
                break;
            }
            let hp = census.get(id).map_or(0.0, |c| c.hp);
            let armor = defs.get(s.def).armor_id;
            let blow = |m: &Mine| defs.get(m.def).weapons.iter().filter(|w| w.suicide).map(|w| w.damage * defs.armor_mult(w.dtype, armor)).sum::<f32>().max(1.0);
            let mut dealt: f32 = live.iter().filter(|m| b.info[m.def as usize].suicide && m.order == Some(Order::Attack(id))).map(|m| blow(m)).sum();
            while dealt < hp && !drones.is_empty() {
                let k = (0..drones.len()).min_by(|&x, &y| drones[x].pos.dist(s.pos).total_cmp(&drones[y].pos.dist(s.pos))).unwrap();
                let m = drones.swap_remove(k);
                dealt += blow(m);
                orders.insert(m.id, Order::Attack(id));
            }
        }
        for m in drones.iter().filter(|m| m.pos.dist(rally) > 30.0) {
            go(&mut orders, m, Order::Move(rally));
        }
    }
    let idle: Vec<&Mine> = live.iter().filter(|m| role(m) == Role::Bomber && m.orders == 0 && !b.info[m.def as usize].suicide).copied().collect();
    let strike: f32 = idle.iter().map(|m| b.info[m.def as usize].mass).sum();
    if b.strategy == Strategy::Kill && b.memory.contains_key(&b.kill) {
        for m in live.iter().filter(|m| role(m) == Role::Bomber) {
            go(&mut orders, m, Order::Attack(b.kill));
        }
    } else if idle.len() >= 3 || (idle.len() >= 1 && tick % 600 < 10) {
        let target = (0..b.zones.len()).filter(|&i| (b.zones[i].prize > 0.0 || b.zones[i].enemy > 0.0) && b.zones[i].enemy_aa < strike * 0.8 + 150.0).max_by(|&x, &y| {
            let s = |i: usize| (b.zones[i].prize + b.zones[i].enemy) / (1.0 + b.zones[i].enemy_aa / 100.0) / (100.0 + b.zones[i].d_home);
            s(x).total_cmp(&s(y))
        });
        if let Some(zi) = target {
            for m in &idle {
                orders.insert(m.id, Order::AttackMove(b.zones[zi].enemy_pos));
                then.push((m.id, Order::Move(rally)));
            }
        }
    }

    if !orders.is_empty() {
        out.push(Command::Orders { ids: orders.keys().copied().collect(), orders: orders.values().copied().collect(), queue: false });
    }
    for (id, o) in then {
        if orders.contains_key(&id) {
            out.push(Command::Orders { ids: vec![id], orders: vec![o], queue: true });
        }
    }
}
