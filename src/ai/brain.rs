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
use super::{find_spot, AiKind, Ctx, Mine};
use crate::data::{DefId, UnitDef, WeaponKind};
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
    Other,
}

const ROLES: usize = 12;

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
}

fn info(d: &UnitDef, commander: bool) -> Info {
    let dps = |land: bool| -> f32 { d.weapons.iter().filter(|w| if land { w.hits_land } else { w.hits_air }).map(|w| w.damage * w.salvo.max(1) as f32 / w.reload.max(0.1)).sum() };
    let reach = |land: bool| -> f32 { d.weapons.iter().filter(|w| if land { w.hits_land } else { w.hits_air }).map(|w| w.range).fold(0.0, f32::max) };
    let (land, air) = (dps(true), dps(false));
    let mass = d.cost.mass;
    // A commander costs a fortune but fights like a heavy tank.
    let worth = if commander { 700.0 } else { mass };
    let threat = if land > 0.0 { worth } else { 0.0 };
    let aa = if air > 0.0 { worth * (air / land.max(air)).min(1.0) } else { 0.0 };
    let indirect = d.weapons.iter().any(|w| w.hits_land && matches!(w.kind, WeaponKind::Arc | WeaponKind::HighArc));
    let role = if d.is_structure() {
        Role::None
    } else if d.is_engineer() {
        Role::Engineer
    } else if d.is_scavenger() {
        Role::Scavenger
    } else if d.transport.is_some() && d.is_air() {
        Role::Transport
    } else if d.build.is_some() || d.transport.is_some() {
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
    } else if indirect || reach(true) >= 85.0 {
        Role::Arty
    } else {
        Role::Tank
    };
    Info { role, threat, aa, range: reach(true), speed: d.mv.speed, mass }
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
    team: false,
    adapt: false,
};

const RUSH: Profile = Profile { fac_first: true, fac_income: 4.0, max_facs: 6, air_at: 99, air: 0.0, eng: 1.0, eng_max: 5.0, tech: 2.5, mex_up_at: 8, scouts: 1, raiders: 3, defense: 0, arty: 0.15, tier_pow: 1.0, attack: 0.85, retreat: 0.45, min_force: 200.0, spend: 2.5, scav: 0, garrison: 0, ..BASE };
const TURTLE: Profile = Profile { expand: 0.45, tech: 0.65, scouts: 1, defense: 3, radar_forward: true, arty: 0.6, air: 0.25, attack: 2.0, retreat: 1.0, min_force: 1500.0, scav: 2, garrison: 8, ..BASE };

pub fn profile(kind: AiKind, s: Strategy) -> Profile {
    match kind {
        AiKind::Simple => BASE,
        AiKind::Zones => Profile { garrison: 6, scouts: 3, raiders: 2, radar_forward: true, attack: 1.4, retreat: 0.8, team: true, ..BASE },
        AiKind::Adaptive => Profile { scav: 3, expand: 0.72, eng: 3.0, eng_max: 12.0, raiders: 4, air: 0.3, attack: 1.15, min_force: 400.0, adapt: true, ..BASE },
        AiKind::Rusher => RUSH,
        AiKind::Turtle => TURTLE,
        AiKind::Overseer => match s {
            Strategy::Rush => RUSH,
            Strategy::Main => Profile { raiders: 2, ..BASE },
            Strategy::Eco => Profile { tech: 0.6, eng: 3.0, raiders: 2, attack: 1.6, min_force: 900.0, ..BASE },
            Strategy::Air => Profile { air: 0.9, air_at: 3, attack: 1.5, ..BASE },
            Strategy::Turtle => Profile { defense: 2, arty: 0.45, attack: 2.0, retreat: 1.0, min_force: 900.0, ..BASE },
            Strategy::Kill => Profile { attack: 0.0, retreat: 0.25, min_force: 0.0, ..BASE },
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
    regroup: i32,
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
}

impl Brain {
    /// One line for the self-test's report.
    pub fn debug(&self) -> String {
        let held = self.zones.iter().filter(|z| z.mine > 0.0).count();
        let known: f32 = self.zones.iter().map(|z| z.danger()).sum();
        format!("{:?} {:?} wave {} zones {}/{} raiders {} enemy known {:.0} peak {:.0} air {:.0} | {}", self.strategy, self.phase, self.wave, held, self.zones.len(), self.raiders.len(), known, self.enemy_peak, self.enemy_air_peak, self.note) + &format!(" | revives {} trips {} housed {} abroad {}", self.stats[0], self.stats[1], self.stats[2], self.stats[3])
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
        self.info = defs.units.iter().map(|d| info(d, defs.factions.iter().any(|f| f.commander == Some(d.idx)))).collect();
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
    let pf = profile(kind, b.strategy);
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

    // --- what we have
    let mut count = vec![0u32; defs.units.len()];
    let mut claimed: Vec<V2> = census.list.iter().filter(|c| defs.get(c.def).needs_deposit).map(|c| c.pos).collect();
    let (mut pump_up, mut fac_up, mut radar_up) = (0u32, 0u32, 0u32);
    let mut zone_turrets: Vec<u32> = b.zones.iter().map(|z| z.turrets).collect();
    let mut role_mass = [0.0f32; ROLES];
    let mut army_n = 0u32;
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
            z.comp != 0 && z.safe() && (!z.reachable || z.front <= pf.expand || z.d_home < 45.0) && !claimed.iter().any(|c| c.dist(**d) < 1.0)
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

    // --- engineers
    let mut claimed_now: Vec<V2> = vec![];
    let mut power_started = 0.0;
    let mut started_fac = false;
    // Engineers that are only helping or reclaiming count as free for real work.
    let filler = |m: &Mine| m.orders == 1 && matches!(m.order, Some(Order::Assist(_)) | Some(Order::ReclaimArea { .. }));
    for m in mine.iter().filter(|m| !m.uc && !m.hosted && (m.orders == 0 || filler(m)) && defs.get(m.def).is_engineer()) {
        let df = defs.get(m.def);
        let is_com = Some(m.def) == commander;
        let opts: Vec<&UnitDef> = df.builds.iter().map(|d| defs.get(*d)).collect();
        let cheapest = |f: &dyn Fn(&UnitDef) -> bool| opts.iter().filter(|d| f(d)).min_by(|x, y| x.cost.mass.total_cmp(&y.cost.mass)).map(|d| d.idx);
        let afford = |d: &UnitDef| d.cost.mass <= 100.0 || d.cost.mass < p.mass + p.income_m * 60.0;
        let runs = |d: &UnitDef| d.eco.upkeep <= p.income_e * 0.2;
        let spot = |rng: &mut Rng, d: DefId, at: V2| find_spot(rng, defs, map, &cx.nav.occ, d, at).map(|s| (d, s));
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
        let tiered = |d: &UnitDef| fm::powf(d.tier as f32, pf.tier_pow) * (b.info[d.idx as usize].range / 40.0).clamp(0.5, 2.0);
        let mut pick: Option<&UnitDef> = None;
        if is_fac(df, true) {
            let scouts = of(Role::Scout);
            let fighters = of(Role::Fighter);
            let bombers = of(Role::Bomber);
            let strike_ok = b.strategy == Strategy::Air || enemy_aa < have(Role::Bomber) * 1.5 + 400.0;
            let air_share = if island { 3.0 } else { pf.air };
            // Transports: to islands (engineers, the army) and for dropping raiders.
            let carriers = of(Role::Transport);
            let want_carriers = if island { 2 } else { (far_deps || pf.raiders >= 3) as u32 };
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
            } else if !radar.is_empty() && army_n >= 3 && n_of(Role::Radar, false) + (queued[Role::Radar as usize] > 0.0) as u32 * 9 < (1 + army_n / 12).min(3) {
                // Mobile radar goes with the army: its guns reach further than its eyes.
                pick = radar.iter().filter(|d| d.cost.mass < p.income_m * 40.0 + 150.0).max_by_key(|d| d.tier).copied().or(radar.first().copied());
            } else if (may_produce && committed < p.mass + p.income_m * 30.0) || land_mass < 200.0 {
                // The role the army is shortest of (RNG's production ratios); anti-air follows
                // the aircraft the enemy was seen with.
                let all = land_mass + queued[Role::Tank as usize] + queued[Role::Arty as usize] + queued[Role::Aa as usize] + 1.0;
                let aa = if b.enemy_air_peak > 0.0 { (b.enemy_air_peak / (all + b.enemy_air_peak)).clamp(0.08, 0.4) } else { pf.aa_base };
                let arty = pf.arty + if pf.adapt && enemy_static > b.enemy_peak { 0.25 } else { 0.0 };
                let mut wants = [(Role::Tank, 1.0 - aa - arty), (Role::Arty, arty), (Role::Aa, aa)];
                wants.sort_by(|x, y| (y.1 - have(y.0) / all).total_cmp(&(x.1 - have(x.0) / all)));
                for (r, _) in wants {
                    let pool = of(r);
                    if !pool.is_empty() {
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
    if pump_up < max_up && mexes >= pf.mex_up_at && p.income_e > 120.0 && (p.mass > 200.0 || p.spend_m < p.income_m * 1.1) && home_safe {
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
        if !arrived && !same(m.order, o) {
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
    let rally_zone = (0..b.zones.len())
        .filter(|&i| {
            let z = &b.zones[i];
            i == b.home || (z.mine > 0.0 && z.reachable && z.safe() && z.front < 0.6)
        })
        .max_by(|&x, &y| b.zones[x].front.total_cmp(&b.zones[y].front))
        .unwrap_or(b.home);
    let rally = {
        let z = b.zones[rally_zone].pos;
        free_at(z + (start - z).norm() * 6.0 + b.fwd * if rally_zone == b.home { 16.0 } else { 0.0 })
    };
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
    let near_front = |q: V2| q.dist(rally) < 45.0 || q.dist(start) < 45.0;
    let riders = |id: u32| census.list.iter().filter(|r| r.host == id && r.hp > 0.0).map(|r| defs.get(r.def).cargo as i32).sum::<i32>();
    let mut housed = census.list.iter().filter(|r| r.owner == me && r.hosted && census.get(r.host).map_or(false, |h| h.structure && defs.get(h.def).is_garrison())).count() as u32;
    housed += mine.iter().filter(|m| matches!(m.order, Some(Order::Load(t)) if census.get(t).map_or(false, |h| h.structure))).count() as u32;
    let mut houses: Vec<(u32, V2, i32)> = census
        .list
        .iter()
        .filter(|c| c.structure && c.done && c.hp > 0.0 && defs.get(c.def).is_garrison() && (c.team == team || c.owner == NEUTRAL) && near_front(c.pos) && b.zones[b.zone_at(c.pos)].safe())
        .map(|c| (c.id, c.pos, defs.get(c.def).transport.as_ref().map_or(0, |t| t.capacity as i32) - riders(c.id)))
        .collect();
    if matches!(b.phase, Phase::Gather) || pf.garrison >= 6 {
        let mut k = 0;
        while k < free.len() && housed < pf.garrison {
            let m = free[k];
            let d = defs.get(m.def);
            let house = if d.is_infantry() && d.cargo > 0 && !b.body.contains(&m.id) && !b.joiners.contains(&m.id) { houses.iter_mut().filter(|h| h.2 >= d.cargo as i32 && h.1.dist(m.pos) < 60.0).min_by(|x, y| x.1.dist(m.pos).total_cmp(&y.1.dist(m.pos))) } else { None };
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
        if !near_front(c.pos) && tick.saturating_sub(z.alert) > 1200 && tick % 100 < 10 {
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
                        b.regroup = 0;
                        b.best = (f32::MAX, tick);
                        b.pushes = 0;
                        b.body = main.iter().filter(|m| m.pos.dist(rally) < room).map(|m| m.id).collect();
                    }
                }
            }
            Phase::Attack(zi) => {
                let cur = b.zone_at(centre);
                let path = b.route(cur, zi, force);
                let dest = match path.get(1) {
                    Some(&next) if next != zi => b.zones[next].pos,
                    _ => b.zones[zi].enemy_pos,
                };
                // Stragglers catch up before the body moves on; a stuck unit does not hold it for ever.
                let together = within(&body, centre, 45.0) >= 0.6 * force;
                let push = b.pushes > 0 && tick < b.best.1 + 200;
                b.regroup = if together { b.regroup.min(0) } else { b.regroup + 1 };
                if b.regroup > 25 {
                    b.regroup = -30;
                }
                b.note += &format!(" to target {:.0} together {} foe {:.0} pushes {}", centre.dist(b.zones[zi].pos), together, enemy_near(centre, 70.0), b.pushes);
                if together || b.regroup < 0 {
                    // Short guns in front, long guns behind them.
                    let mut rows: Vec<&Mine> = body.clone();
                    rows.sort_by(|x, y| (b.info[x.def as usize].range, x.id).partial_cmp(&(b.info[y.def as usize].range, y.id)).unwrap());
                    let cols = (rows.len() as f32).sqrt().ceil().max(1.0) as usize * 2;
                    let ahead = (dest - centre).norm();
                    let across = ahead.perp();
                    for (k, m) in rows.iter().enumerate() {
                        let (c, r) = ((k % cols) as f32 - (cols.min(rows.len()) as f32 - 1.0) * 0.5, (k / cols) as f32);
                        let at = map.clamp(dest + across * (c * 3.0) - ahead * (r * 4.0));
                        go(&mut orders, m, if push && b.info[m.def as usize].role == Role::Tank { Order::Move(at) } else { Order::AttackMove(at) });
                    }
                } else {
                    for m in &body {
                        go(&mut orders, m, Order::AttackMove(centre));
                    }
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
                    go(&mut orders, m, to_rally(m));
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
        let aboard = census.list.iter().any(|r| r.host == t.id);
        if t.orders != 0 {
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

    // --- micro for everything on the ground that fights
    for m in &land {
        let inf = b.info[m.def as usize];
        if inf.range <= 0.0 {
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
        if inf.role == Role::Arty && m.pos.dist(centre) < 60.0 && !b.raiders.contains(&m.id) {
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
    let idle: Vec<&Mine> = live.iter().filter(|m| role(m) == Role::Bomber && m.orders == 0).copied().collect();
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
