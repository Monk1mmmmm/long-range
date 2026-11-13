//! Authoritative game simulation: flow economy, orders, movement, combat, vision.
//! Runs at 10 ticks per second on the host; clients only receive `State` snapshots.
use crate::defs::*;
use crate::map::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DT: f32 = 0.1;
pub const BUILD_RANGE: f32 = 8.0;

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Debug)]
pub enum Order {
    Move(V2),
    AttackMove(V2),
    Patrol(V2),
    Attack(u32),
    Build { def: DefId, pos: V2 },
    Assist(u32),
    Reclaim(u32),
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub enum Command {
    /// `orders` holds either one order for everybody or one order per unit.
    Orders { ids: Vec<u32>, orders: Vec<Order>, queue: bool },
    Stop { ids: Vec<u32> },
    Produce { factory: u32, def: DefId, count: i16 },
    Repeat { ids: Vec<u32>, on: bool },
    Upgrade { ids: Vec<u32> },
    Resign,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Unit {
    pub id: u32,
    pub def: DefId,
    pub owner: u8,
    pub pos: V2,
    pub heading: f32,
    pub aim: f32,
    pub hp: f32,
    /// Construction progress 0..1.
    pub built: f32,
    /// Upgrade progress, 0 when not upgrading.
    pub upg: f32,
    pub orders: Vec<Order>,
    pub target: u32,
    pub cd: [f32; 2],
    pub queue: Vec<DefId>,
    pub repeat: bool,
    pub rally: Option<V2>,
    /// Factory: id of the unit under construction.
    pub building: u32,
    /// Id of the factory this unit is still inside of (0 = free).
    pub host: u32,
    /// Id of the unit or wreck this builder is working on.
    pub work: u32,
    /// Team bitmasks: who sees this unit / has it on radar.
    pub seen: u8,
    pub radar: u8,
    pub anchor: V2,
    #[serde(skip)]
    pub path: Vec<V2>,
    #[serde(skip)]
    pub path_goal: V2,
    #[serde(skip)]
    pub goal: Option<V2>,
    #[serde(skip)]
    pub stuck: u16,
    #[serde(skip)]
    pub vel: V2,
    #[serde(skip)]
    pub extend: f32,
}

impl Unit {
    pub fn d(&self) -> &'static UnitDef {
        def(self.def)
    }
    pub fn done(&self) -> bool {
        self.built >= 1.0
    }
    pub fn radius(&self) -> f32 {
        let d = self.d();
        if d.foot > 0 {
            d.foot as f32 * CELL * 0.5
        } else {
            d.radius
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Wreck {
    pub id: u32,
    pub pos: V2,
    pub mass: f32,
    pub def: DefId,
    pub heading: f32,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Proj {
    pub pos: V2,
    pub vel: V2,
    pub z0: f32,
    pub z1: f32,
    pub arc: f32,
    pub life: f32,
    pub total: f32,
    pub kind: u8,
    pub dmg: f32,
    pub dtype: Dmg,
    pub aoe: f32,
    pub owner: u8,
    pub target: u32,
    pub air: bool,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Fx {
    pub pos: V2,
    pub r: f32,
    pub h: f32,
}

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Player {
    pub name: String,
    pub team: u8,
    pub color: u8,
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

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct State {
    pub tick: u32,
    pub units: Vec<Unit>,
    pub wrecks: Vec<Wreck>,
    pub projs: Vec<Proj>,
    pub players: Vec<Player>,
    pub fx: Vec<Fx>,
    pub winner: Option<u8>,
    pub next_id: u32,
}

pub struct Sim {
    pub map: Map,
    pub s: State,
    /// Blocked cells: 1 = terrain, 2 = structure.
    pub occ: Vec<u8>,
    pub idx: HashMap<u32, usize>,
    occ_dirty: bool,
    rng: Rng,
    scratch: PathScratch,
    paths_left: i32,
}

pub struct PlayerSetup {
    pub name: String,
    pub team: u8,
    pub is_ai: bool,
    pub slot: usize,
}

impl Sim {
    pub fn new(map: Map, setup: &[PlayerSetup]) -> Sim {
        let mut sim = Sim::view(map);
        for (i, p) in setup.iter().enumerate() {
            let start = sim.map.starts[p.slot % sim.map.starts.len()];
            sim.s.players.push(Player {
                name: p.name.clone(),
                team: p.team,
                color: p.slot as u8,
                is_ai: p.is_ai,
                alive: true,
                mass: 650.0,
                energy: 4000.0,
                max_mass: 650.0,
                max_energy: 4000.0,
                start,
                ..Default::default()
            });
            let id = sim.spawn(COMMANDER, i as u8, start, 1.0);
            let center = v2(sim.map.world() * 0.5, sim.map.world() * 0.5);
            if let Some(j) = sim.find(id) {
                sim.s.units[j].heading = (center - start).angle();
                sim.s.units[j].aim = sim.s.units[j].heading;
            }
        }
        sim.rebuild_occ();
        sim
    }

    /// A passive container used by clients to hold received snapshots.
    pub fn view(map: Map) -> Sim {
        let seed = map.seed;
        let mut sim = Sim {
            map,
            s: State { next_id: 1, ..Default::default() },
            occ: vec![],
            idx: HashMap::new(),
            occ_dirty: true,
            rng: Rng(seed ^ 0xABCDEF123 | 1),
            scratch: PathScratch::default(),
            paths_left: 0,
        };
        sim.rebuild_occ();
        sim
    }

    pub fn set_state(&mut self, s: State) {
        self.s = s;
        self.reindex();
        self.rebuild_occ();
    }

    pub fn reindex(&mut self) {
        self.idx.clear();
        for (i, u) in self.s.units.iter().enumerate() {
            self.idx.insert(u.id, i);
        }
    }

    pub fn find(&self, id: u32) -> Option<usize> {
        if id == 0 {
            return None;
        }
        self.idx.get(&id).copied()
    }
    pub fn unit(&self, id: u32) -> Option<&Unit> {
        self.find(id).map(|i| &self.s.units[i])
    }
    pub fn team_of(&self, owner: u8) -> u8 {
        self.s.players[owner as usize].team
    }

    pub fn rebuild_occ(&mut self) {
        let n = self.map.n;
        self.occ.clear();
        self.occ.extend(self.map.terrain.iter().map(|t| if *t == T_GROUND { 0u8 } else { 1u8 }));
        for u in &self.s.units {
            let f = u.d().foot;
            if f > 0 {
                let (x0, y0) = foot_origin(u.pos, f);
                for y in y0..y0 + f {
                    for x in x0..x0 + f {
                        if x >= 0 && y >= 0 && (x as usize) < n && (y as usize) < n {
                            self.occ[y as usize * n + x as usize] = 2;
                        }
                    }
                }
            }
        }
        self.occ_dirty = false;
    }

    pub fn occ_at(&self, p: V2) -> u8 {
        let (cx, cy) = self.map.cell_of(p);
        if self.map.inside(cx, cy) {
            self.occ[cy as usize * self.map.n + cx as usize]
        } else {
            1
        }
    }

    /// Snaps a build position to the grid (and to a deposit for extractors).
    pub fn snap(&self, d: DefId, p: V2) -> V2 {
        let df = def(d);
        if df.needs_deposit {
            let mut best = (1e9, p);
            for dep in &self.map.deposits {
                let dd = dep.dist(p);
                if dd < best.0 {
                    best = (dd, *dep);
                }
            }
            if best.0 < 12.0 {
                return best.1;
            }
        }
        if df.foot % 2 == 1 {
            v2(((p.x / CELL).floor() + 0.5) * CELL, ((p.y / CELL).floor() + 0.5) * CELL)
        } else {
            v2((p.x / CELL).round() * CELL, (p.y / CELL).round() * CELL)
        }
    }

    pub fn can_place(&self, d: DefId, p: V2) -> bool {
        let df = def(d);
        let f = df.foot;
        if f <= 0 {
            return false;
        }
        let (x0, y0) = foot_origin(p, f);
        for y in y0..y0 + f {
            for x in x0..x0 + f {
                if !self.map.inside(x, y) || self.occ[y as usize * self.map.n + x as usize] != 0 {
                    return false;
                }
            }
        }
        let half = f as f32 * CELL * 0.5;
        let on_dep = self.map.deposits.iter().any(|dp| (dp.x - p.x).abs() < half && (dp.y - p.y).abs() < half);
        if df.needs_deposit {
            self.map.deposits.iter().any(|dp| dp.dist(p) < 0.5)
        } else {
            !on_dep
        }
    }

    fn spawn(&mut self, d: DefId, owner: u8, pos: V2, built: f32) -> u32 {
        let id = self.s.next_id;
        self.s.next_id += 1;
        let df = def(d);
        let team = self.team_of(owner);
        self.s.units.push(Unit {
            id,
            def: d,
            owner,
            pos,
            heading: 1.5708,
            aim: 1.5708,
            hp: if built >= 1.0 { df.hp } else { df.hp * 0.05 },
            built,
            upg: 0.0,
            orders: vec![],
            target: 0,
            cd: [0.0; 2],
            queue: vec![],
            repeat: false,
            rally: None,
            building: 0,
            host: 0,
            work: 0,
            seen: 1 << team,
            radar: 1 << team,
            anchor: pos,
            path: vec![],
            path_goal: pos,
            goal: None,
            stuck: 0,
            vel: v2(0.0, 0.0),
            extend: 0.0,
        });
        self.idx.insert(id, self.s.units.len() - 1);
        if df.foot > 0 {
            self.rebuild_occ();
        }
        id
    }

    // ------------------------------------------------------------------ commands

    pub fn apply(&mut self, pl: u8, c: Command) {
        if (pl as usize) >= self.s.players.len() || !self.s.players[pl as usize].alive {
            return;
        }
        match c {
            Command::Orders { ids, orders, queue } => {
                if orders.is_empty() {
                    return;
                }
                for (k, id) in ids.iter().enumerate() {
                    let Some(i) = self.find(*id) else { continue };
                    if self.s.units[i].owner != pl {
                        continue;
                    }
                    let mut o = if orders.len() == ids.len() { orders[k] } else { orders[0] };
                    let df = self.s.units[i].d();
                    match &mut o {
                        Order::Move(p) | Order::AttackMove(p) | Order::Patrol(p) => *p = self.map.clamp(*p),
                        Order::Build { def: bd, pos } => {
                            if !df.builds.contains(bd) || (*bd as usize) >= DEFS.len() || def(*bd).foot == 0 {
                                continue;
                            }
                            *pos = self.snap(*bd, *pos);
                        }
                        _ => {}
                    }
                    let u = &mut self.s.units[i];
                    if df.kind == Kind::Factory {
                        if let Order::Move(p) | Order::AttackMove(p) | Order::Patrol(p) = o {
                            u.rally = Some(p);
                        }
                        continue;
                    }
                    if !queue {
                        u.orders.clear();
                        u.path.clear();
                        u.stuck = 0;
                    }
                    if u.orders.len() >= 64 {
                        continue;
                    }
                    if let Order::Patrol(_) = o {
                        let has = u.orders.iter().any(|x| matches!(x, Order::Patrol(_)));
                        u.orders.push(o);
                        if !has {
                            u.orders.push(Order::Patrol(u.pos));
                        }
                    } else {
                        u.orders.push(o);
                    }
                }
            }
            Command::Stop { ids } => {
                for id in ids {
                    let Some(i) = self.find(id) else { continue };
                    if self.s.units[i].owner != pl {
                        continue;
                    }
                    let u = &mut self.s.units[i];
                    u.orders.clear();
                    u.path.clear();
                    u.target = 0;
                    u.upg = 0.0;
                    u.queue.clear();
                    let b = u.building;
                    u.building = 0;
                    if let Some(j) = self.find(b) {
                        self.s.units[j].hp = -1.0;
                        self.s.units[j].built = 0.0;
                    }
                }
            }
            Command::Produce { factory, def: d, count } => {
                let Some(i) = self.find(factory) else { return };
                let u = &self.s.units[i];
                if u.owner != pl || u.d().kind != Kind::Factory || !u.d().builds.contains(&d) {
                    return;
                }
                if count > 0 {
                    for _ in 0..count.min(50) {
                        if self.s.units[i].queue.len() < 100 {
                            self.s.units[i].queue.push(d);
                        }
                    }
                } else {
                    for _ in 0..(-count).min(50) {
                        let q = &self.s.units[i].queue;
                        let Some(k) = q.iter().rposition(|x| *x == d) else { break };
                        if k == 0 && self.s.units[i].building != 0 {
                            let b = self.s.units[i].building;
                            self.s.units[i].building = 0;
                            if let Some(j) = self.find(b) {
                                self.s.units[j].hp = -1.0;
                                self.s.units[j].built = 0.0;
                            }
                        }
                        self.s.units[i].queue.remove(k);
                    }
                }
            }
            Command::Repeat { ids, on } => {
                for id in ids {
                    if let Some(i) = self.find(id) {
                        if self.s.units[i].owner == pl {
                            self.s.units[i].repeat = on;
                        }
                    }
                }
            }
            Command::Upgrade { ids } => {
                for id in ids {
                    if let Some(i) = self.find(id) {
                        let u = &mut self.s.units[i];
                        if u.owner == pl && u.d().upgrade.is_some() && u.upg == 0.0 && u.done() {
                            u.upg = 0.0001;
                        }
                    }
                }
            }
            Command::Resign => {
                for u in self.s.units.iter_mut() {
                    if u.owner == pl && u.def == COMMANDER {
                        u.hp = -1.0;
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------------ tick

    pub fn tick(&mut self) {
        self.s.tick += 1;
        self.s.fx.clear();
        self.reindex();
        if self.occ_dirty {
            self.rebuild_occ();
        }
        self.paths_left = 24;
        self.visibility();
        self.income();
        for i in 0..self.s.units.len() {
            self.think(i);
        }
        self.build_pass();
        for i in 0..self.s.units.len() {
            self.locomote(i);
        }
        self.separate();
        for i in 0..self.s.units.len() {
            self.fire(i);
        }
        self.projectiles();
        self.deaths();
    }

    fn visibility(&mut self) {
        let mut viewers: Vec<(V2, f32, f32, u8)> = vec![];
        for u in &self.s.units {
            if u.done() && u.host == 0 {
                let p = &self.s.players[u.owner as usize];
                let d = u.d();
                viewers.push((u.pos, d.vision * d.vision, if p.power_out { 0.0 } else { d.radar * d.radar }, p.team));
            }
        }
        let teams: Vec<u8> = self.s.players.iter().map(|p| p.team).collect();
        for u in self.s.units.iter_mut() {
            let own = 1u8 << teams[u.owner as usize];
            let (mut seen, mut radar) = (own, own);
            for v in &viewers {
                let bit = 1u8 << v.3;
                if (seen & bit != 0) && (radar & bit != 0) {
                    continue;
                }
                let d2 = (v.0 - u.pos).len2();
                if d2 < v.1 {
                    seen |= bit;
                }
                if d2 < v.2 {
                    radar |= bit;
                }
            }
            u.seen = seen;
            u.radar = radar;
        }
    }

    fn income(&mut self) {
        for p in self.s.players.iter_mut() {
            p.max_mass = 0.0;
            p.max_energy = 0.0;
            p.income_m = 0.0;
            p.income_e = 0.0;
            p.spend_m = 0.0;
            p.spend_e = 0.0;
        }
        for u in &self.s.units {
            if !u.done() {
                continue;
            }
            let d = u.d();
            let p = &mut self.s.players[u.owner as usize];
            p.max_mass += d.store_m;
            p.max_energy += d.store_e;
            p.income_m += d.prod_m * if p.power_out && d.upkeep_e > 0.0 { 0.25 } else { 1.0 };
            p.income_e += d.prod_e;
            p.spend_e += d.upkeep_e;
        }
        for p in self.s.players.iter_mut() {
            p.mass += p.income_m * DT;
            p.energy += (p.income_e - p.spend_e) * DT;
        }
    }

    fn known(&self, j: usize, team: u8) -> bool {
        let u = &self.s.units[j];
        (u.seen | u.radar) & (1 << team) != 0
    }

    fn valid_target(&self, j: usize, team: u8, land: bool, air: bool) -> bool {
        let u = &self.s.units[j];
        if u.hp <= 0.0 || u.host != 0 || self.team_of(u.owner) == team || !self.known(j, team) {
            return false;
        }
        if u.d().is_air() {
            air
        } else {
            land
        }
    }

    pub fn nearest_enemy(&self, pos: V2, team: u8, range: f32, min_range: f32, land: bool, air: bool) -> Option<usize> {
        let mut best: Option<(f32, usize)> = None;
        let (r2, m2) = (range * range, min_range * min_range);
        for (j, u) in self.s.units.iter().enumerate() {
            let d2 = (u.pos - pos).len2();
            if d2 > r2 || d2 < m2 || best.map_or(false, |b| d2 >= b.0) {
                continue;
            }
            if self.valid_target(j, team, land, air) {
                best = Some((d2, j));
            }
        }
        best.map(|b| b.1)
    }

    fn think(&mut self, i: usize) {
        self.s.units[i].work = 0;
        self.s.units[i].goal = None;
        let u = &self.s.units[i];
        if !u.done() || u.hp <= 0.0 {
            return;
        }
        let df = u.d();
        if df.kind == Kind::Factory {
            self.think_factory(i);
            return;
        }
        let (pos, owner, id) = (u.pos, u.owner, u.id);
        let team = self.team_of(owner);
        let is_struct = df.is_structure();
        let plane = df.layer == Layer::Plane;
        let arrive = if plane { 12.0 } else { 1.2 };
        let front = u.orders.first().copied();
        let mut goal: Option<V2> = None;
        let mut pop = false;
        let mut work = 0u32;
        let mut target = 0u32;
        let w0 = df.weapons.first();
        match front {
            None => {
                if plane {
                    if let Some(w) = w0 {
                        if w.air {
                            if let Some(j) = self.nearest_enemy(pos, team, 80.0, 0.0, false, true) {
                                goal = Some(self.s.units[j].pos);
                            }
                        }
                    }
                }
            }
            Some(Order::Move(p)) => {
                if is_struct || pos.dist(p) <= arrive {
                    pop = true;
                    if plane {
                        self.s.units[i].anchor = p;
                    }
                } else {
                    goal = Some(p);
                }
            }
            Some(Order::AttackMove(p)) | Some(Order::Patrol(p)) => {
                let mut engaged = false;
                if let Some(w) = w0 {
                    if plane {
                        if let Some(j) = self.nearest_enemy(pos, team, 70.0, 0.0, w.land, w.air) {
                            goal = Some(self.s.units[j].pos);
                            engaged = true;
                        }
                    } else if self.nearest_enemy(pos, team, w.range * 0.95, w.min_range, w.land, w.air).is_some() {
                        engaged = true;
                    }
                }
                if is_struct {
                    pop = true;
                } else if !engaged {
                    if pos.dist(p) <= arrive.max(2.5) {
                        pop = true;
                        if plane {
                            self.s.units[i].anchor = p;
                        }
                    } else {
                        goal = Some(p);
                    }
                }
            }
            Some(Order::Attack(tid)) => {
                let mut ok = false;
                if let Some(j) = self.find(tid) {
                    let t = &self.s.units[j];
                    let t_air = t.d().is_air();
                    let range = df
                        .weapons
                        .iter()
                        .filter(|w| if t_air { w.air } else { w.land })
                        .map(|w| w.range)
                        .fold(0.0, f32::max);
                    if range > 0.0 && t.hp > 0.0 && self.known(j, team) && self.team_of(t.owner) != team {
                        ok = true;
                        target = tid;
                        if plane || (!is_struct && pos.dist(t.pos) > range * 0.9) {
                            goal = Some(t.pos);
                        }
                    }
                }
                pop = !ok;
            }
            Some(Order::Build { def: bd, pos: bp }) => {
                if !df.is_builder() || !df.builds.contains(&bd) {
                    pop = true;
                } else {
                    let reach = BUILD_RANGE + def(bd).foot as f32 * CELL * 0.5;
                    let site = self.s.units.iter().position(|s| {
                        s.pos == bp && s.d().foot > 0 && s.hp > 0.0 && self.team_of(s.owner) == team
                    });
                    match site {
                        Some(j) => {
                            let s = &self.s.units[j];
                            if s.done() || s.def != bd {
                                pop = true;
                            } else if pos.dist(bp) > reach {
                                goal = Some(bp);
                            } else {
                                work = s.id;
                            }
                        }
                        None => {
                            if pos.dist(bp) > reach {
                                goal = Some(bp);
                            } else if self.can_place(bd, bp) {
                                work = self.spawn(bd, owner, bp, 0.0);
                            } else {
                                pop = true;
                            }
                        }
                    }
                }
            }
            Some(Order::Assist(tid)) => match self.find(tid) {
                Some(j) if self.s.units[j].hp > 0.0 && tid != id => {
                    let t = &self.s.units[j];
                    let reach = BUILD_RANGE + t.radius();
                    if df.is_builder() {
                        let job = if !t.done() || t.upg > 0.0 {
                            t.id
                        } else if t.building != 0 {
                            t.building
                        } else if t.work != 0 && t.work != id {
                            t.work
                        } else if t.hp < t.d().hp {
                            t.id
                        } else {
                            0
                        };
                        if pos.dist(t.pos) > reach {
                            goal = Some(t.pos);
                        } else {
                            work = job;
                        }
                        if job == 0 && !matches!(t.d().kind, Kind::Factory | Kind::Engineer | Kind::Commander) {
                            pop = true;
                        }
                    } else if is_struct {
                        pop = true;
                    } else if pos.dist(t.pos) > reach + 4.0 {
                        goal = Some(t.pos);
                    }
                }
                _ => pop = true,
            },
            Some(Order::Reclaim(wid)) => {
                let w = self.s.wrecks.iter().position(|w| w.id == wid);
                match w {
                    Some(k) if df.is_builder() => {
                        let wp = self.s.wrecks[k].pos;
                        if pos.dist(wp) > BUILD_RANGE {
                            goal = Some(wp);
                        } else {
                            let amt = (df.build_rate * 2.0 * DT).min(self.s.wrecks[k].mass);
                            self.s.wrecks[k].mass -= amt;
                            self.s.players[owner as usize].mass += amt;
                            work = wid;
                            if self.s.wrecks[k].mass <= 0.01 {
                                self.s.wrecks.remove(k);
                                pop = true;
                            }
                        }
                    }
                    _ => pop = true,
                }
            }
        }
        let u = &mut self.s.units[i];
        u.work = work;
        u.goal = goal;
        u.target = target;
        if pop && !u.orders.is_empty() {
            let o = u.orders.remove(0);
            if let Order::Patrol(_) = o {
                u.orders.push(o);
            }
            u.stuck = 0;
        }
    }

    fn think_factory(&mut self, i: usize) {
        let (fid, fpos, owner, building, foot) = {
            let f = &self.s.units[i];
            (f.id, f.pos, f.owner, f.building, f.d().foot)
        };
        if building != 0 {
            match self.find(building) {
                None => self.s.units[i].building = 0,
                Some(j) => {
                    if self.s.units[j].done() {
                        let mut exit = fpos + v2(0.0, foot as f32 * CELL * 0.5 + 3.0);
                        let air = self.s.units[j].d().is_air();
                        if !air && self.occ_at(exit) != 0 {
                            let (cx, cy) = self.map.cell_of(exit);
                            if let Some((x, y)) = nearest_free(&self.occ, self.map.n, cx, cy) {
                                exit = v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL);
                            }
                        }
                        let rally = self.s.units[i].rally.unwrap_or(exit + v2(0.0, 10.0));
                        let c = &mut self.s.units[j];
                        c.host = 0;
                        if !air {
                            c.pos = exit;
                        }
                        c.anchor = rally;
                        c.orders = vec![Order::Move(rally)];
                        let f = &mut self.s.units[i];
                        f.building = 0;
                        if !f.queue.is_empty() {
                            let d = f.queue.remove(0);
                            if f.repeat {
                                f.queue.push(d);
                            }
                        }
                    } else {
                        self.s.units[i].work = building;
                    }
                }
            }
        }
        if self.s.units[i].building == 0 {
            if let Some(d) = self.s.units[i].queue.first().copied() {
                let id = self.spawn(d, owner, fpos, 0.0);
                let j = self.s.units.len() - 1;
                self.s.units[j].host = fid;
                self.s.units[i].building = id;
                self.s.units[i].work = id;
            }
        }
    }

    fn build_pass(&mut self) {
        // (builder index, target index, build rate, kind: 0 build, 1 upgrade, 2 repair)
        let mut jobs: Vec<(usize, usize, f32, u8)> = vec![];
        for (i, u) in self.s.units.iter().enumerate() {
            if !u.done() || u.hp <= 0.0 {
                continue;
            }
            if u.upg > 0.0 {
                jobs.push((i, i, u.d().build_rate.max(5.0), 1));
            }
            if u.work == 0 {
                continue;
            }
            let Some(j) = self.find(u.work) else { continue };
            let t = &self.s.units[j];
            let kind = if !t.done() {
                0
            } else if t.upg > 0.0 {
                1
            } else if t.hp < t.d().hp {
                2
            } else {
                continue;
            };
            jobs.push((i, j, u.d().build_rate, kind));
        }
        let np = self.s.players.len();
        let mut dm = vec![0.0f32; np];
        let mut de = vec![0.0f32; np];
        let cost = |units: &Vec<Unit>, j: usize, kind: u8| -> (f32, f32, f32) {
            let d = if kind == 1 { def(units[j].d().upgrade.unwrap_or(units[j].def)) } else { units[j].d() };
            (d.mass, d.energy, d.build_time)
        };
        for &(i, j, rate, kind) in &jobs {
            if kind == 2 {
                continue;
            }
            let (m, e, bt) = cost(&self.s.units, j, kind);
            let o = self.s.units[i].owner as usize;
            dm[o] += m * rate / bt;
            de[o] += e * rate / bt;
        }
        let mut ratio = vec![1.0f32; np];
        for p in 0..np {
            let pl = &self.s.players[p];
            if dm[p] > 0.0 {
                ratio[p] = ratio[p].min(pl.mass.max(0.0) / (dm[p] * DT));
            }
            if de[p] > 0.0 {
                ratio[p] = ratio[p].min(pl.energy.max(0.0) / (de[p] * DT));
            }
            ratio[p] = ratio[p].clamp(0.0, 1.0);
        }
        for &(i, j, rate, kind) in &jobs {
            let o = self.s.units[i].owner as usize;
            if kind == 2 {
                let t = &mut self.s.units[j];
                t.hp = (t.hp + rate * 3.0 * DT).min(t.d().hp);
                continue;
            }
            let (m, e, bt) = cost(&self.s.units, j, kind);
            let t = &mut self.s.units[j];
            let cur = if kind == 0 { t.built } else { t.upg };
            let prog = (rate * DT * ratio[o] / bt).min(1.0 - cur).max(0.0);
            let pl = &mut self.s.players[o];
            pl.mass -= m * prog;
            pl.energy -= e * prog;
            pl.spend_m += m * prog / DT;
            pl.spend_e += e * prog / DT;
            if kind == 0 {
                t.built += prog;
                t.hp = (t.hp + prog * t.d().hp).min(t.d().hp);
                if t.built >= 0.9999 {
                    t.built = 1.0;
                }
            } else {
                t.upg += prog;
                if t.upg >= 0.9999 {
                    let frac = t.hp / t.d().hp;
                    t.def = t.d().upgrade.unwrap_or(t.def);
                    t.hp = frac * t.d().hp;
                    t.upg = 0.0;
                }
            }
        }
        for (p, pl) in self.s.players.iter_mut().enumerate() {
            // Show requested spending while stalling so the player sees the deficit.
            if ratio[p] < 1.0 {
                pl.spend_m = dm[p];
                pl.spend_e = de[p] + (pl.spend_e - de[p] * ratio[p]).max(0.0);
            }
            pl.power_out = pl.energy <= 0.0 && pl.income_e < pl.spend_e;
            pl.mass = pl.mass.clamp(0.0, pl.max_mass.max(1.0));
            pl.energy = pl.energy.clamp(0.0, pl.max_energy.max(1.0));
        }
    }

    fn locomote(&mut self, i: usize) {
        let u = &self.s.units[i];
        let df = u.d();
        if !u.done() || df.is_structure() || u.host != 0 || u.hp <= 0.0 {
            return;
        }
        let old = u.pos;
        let world = self.map.world();
        match df.layer {
            Layer::Plane => {
                let u = &mut self.s.units[i];
                let mut desired = u.heading;
                if u.extend > 0.0 {
                    u.extend -= DT;
                } else if let Some(g) = u.goal {
                    desired = (g - u.pos).angle();
                    let off = wrap(desired - u.heading).abs();
                    if u.pos.dist(g) < 11.0 && off > 1.0 {
                        u.extend = 2.5;
                        desired = u.heading;
                    }
                } else if u.pos.dist(u.anchor) > 14.0 {
                    desired = (u.anchor - u.pos).angle();
                } else {
                    desired = u.heading + 1.0;
                }
                // Stay over the map.
                if u.pos.x < 4.0 || u.pos.y < 4.0 || u.pos.x > world - 4.0 || u.pos.y > world - 4.0 {
                    desired = (v2(world * 0.5, world * 0.5) - u.pos).angle();
                    u.extend = 0.0;
                }
                u.heading = turn_toward(u.heading, desired, df.turn * DT);
                u.aim = u.heading;
                u.pos = u.pos + V2::from_angle(u.heading) * (df.speed * DT);
            }
            Layer::Hover => {
                let u = &mut self.s.units[i];
                if let Some(g) = u.goal {
                    let to = g - u.pos;
                    let dist = to.len();
                    if dist > 0.5 {
                        u.heading = turn_toward(u.heading, to.angle(), df.turn * DT);
                        u.pos = u.pos + to.norm() * (df.speed * DT).min(dist);
                    }
                }
            }
            Layer::Land => {
                let Some(g) = u.goal else {
                    let u = &mut self.s.units[i];
                    u.path.clear();
                    u.stuck = 0;
                    u.vel = v2(0.0, 0.0);
                    return;
                };
                if u.path.is_empty() || u.path_goal.dist(g) > 5.0 {
                    if self.paths_left <= 0 {
                        return;
                    }
                    self.paths_left -= 1;
                    let path = find_path(&self.occ, self.map.n, u.pos, g, &mut self.scratch);
                    let u = &mut self.s.units[i];
                    u.path = path;
                    u.path_goal = g;
                }
                let n = self.map.n as i32;
                let occ = &self.occ;
                let blocked = |p: V2| -> bool {
                    let (cx, cy) = ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32);
                    cx < 0 || cy < 0 || cx >= n || cy >= n || occ[(cy * n + cx) as usize] != 0
                };
                let u = &mut self.s.units[i];
                while u.path.len() > 1 && u.pos.dist(*u.path.last().unwrap()) < 1.5 {
                    u.path.pop();
                }
                let wp = *u.path.last().unwrap();
                let to = wp - u.pos;
                let dist = to.len();
                if dist < 0.05 {
                    return;
                }
                let desired = to.angle();
                u.heading = turn_toward(u.heading, desired, df.turn * DT);
                let diff = wrap(desired - u.heading).abs();
                let sp = if diff < 0.5 {
                    df.speed
                } else if diff < 1.2 {
                    df.speed * 0.4
                } else {
                    0.0
                };
                let step = (sp * DT).min(dist);
                let dir = V2::from_angle(u.heading);
                let np = u.pos + dir * step;
                let inside = blocked(u.pos);
                if inside || !blocked(np) {
                    u.pos = np;
                } else if !blocked(v2(np.x, u.pos.y)) {
                    u.pos = v2(np.x, u.pos.y);
                } else if !blocked(v2(u.pos.x, np.y)) {
                    u.pos = v2(u.pos.x, np.y);
                }
                if sp > 0.0 && u.pos.dist(old) < step * 0.3 {
                    u.stuck += 1;
                    if u.stuck % 25 == 24 {
                        u.path.clear();
                    }
                    if u.stuck > 90 {
                        u.stuck = 0;
                        if !u.orders.is_empty() {
                            u.orders.remove(0);
                        }
                    }
                } else if u.stuck > 0 {
                    u.stuck -= 1;
                }
            }
        }
        let u = &mut self.s.units[i];
        u.pos = v2(u.pos.x.clamp(1.0, world - 1.0), u.pos.y.clamp(1.0, world - 1.0));
        u.vel = (u.pos - old) * (1.0 / DT);
    }

    /// Pushes overlapping mobile units apart.
    fn separate(&mut self) {
        const G: f32 = 6.0;
        let mut grid: HashMap<(i32, i32, bool), Vec<usize>> = HashMap::new();
        for (i, u) in self.s.units.iter().enumerate() {
            let d = u.d();
            if d.is_structure() || d.layer == Layer::Plane || u.host != 0 || !u.done() {
                continue;
            }
            grid.entry(((u.pos.x / G) as i32, (u.pos.y / G) as i32, d.is_air())).or_default().push(i);
        }
        let mut push: Vec<(usize, V2)> = vec![];
        for (&(gx, gy, air), list) in &grid {
            for &i in list {
                let a = &self.s.units[i];
                let mut acc = v2(0.0, 0.0);
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let Some(other) = grid.get(&(gx + dx, gy + dy, air)) else { continue };
                        for &j in other {
                            if i == j {
                                continue;
                            }
                            let b = &self.s.units[j];
                            let d = a.pos - b.pos;
                            let min = a.d().radius + b.d().radius + 0.3;
                            let l = d.len();
                            if l < min {
                                let dir = if l > 0.01 { d * (1.0 / l) } else { V2::from_angle((i as f32) * 2.4) };
                                // Idle units yield more readily than units that hold a goal.
                                let w = if a.goal.is_some() && b.goal.is_none() { 0.2 } else { 0.5 };
                                acc = acc + dir * ((min - l) * w);
                            }
                        }
                    }
                }
                if acc.len2() > 0.0 {
                    push.push((i, acc));
                }
            }
        }
        for (i, p) in push {
            let p = if p.len() > 1.0 { p.norm() } else { p };
            let np = self.s.units[i].pos + p;
            if self.s.units[i].d().is_air() || self.occ_at(np) == 0 {
                self.s.units[i].pos = self.map.clamp(np);
            }
        }
    }

    fn fire(&mut self, i: usize) {
        let u = &self.s.units[i];
        let df = u.d();
        if df.weapons.is_empty() || !u.done() || u.host != 0 || u.hp <= 0.0 {
            return;
        }
        let team = self.team_of(u.owner);
        let plane = df.layer == Layer::Plane;
        let (pos, heading, owner, pref) = (u.pos, u.heading, u.owner, u.target);
        for (wi, w) in df.weapons.iter().enumerate().take(2) {
            self.s.units[i].cd[wi] -= DT;
            if self.s.units[i].cd[wi] > 0.0 {
                continue;
            }
            let mut tj = None;
            if let Some(j) = self.find(pref) {
                let d = self.s.units[j].pos.dist(pos);
                if self.valid_target(j, team, w.land, w.air) && d <= w.range && d >= w.min_range {
                    tj = Some(j);
                }
            }
            if tj.is_none() {
                tj = self.nearest_enemy(pos, team, w.range, w.min_range, w.land, w.air);
            }
            let Some(j) = tj else {
                let u = &mut self.s.units[i];
                u.cd[wi] = 0.3 + (u.id % 4) as f32 * 0.05;
                if wi == 0 && !plane {
                    u.aim = turn_toward(u.aim, u.heading, 0.5);
                }
                continue;
            };
            let (tp, tv, tid, t_air, t_alt) = {
                let t = &self.s.units[j];
                (t.pos, t.vel, t.id, t.d().is_air(), t.d().alt)
            };
            let desired = (tp - pos).angle();
            if plane {
                if wrap(desired - heading).abs() > 0.6 {
                    continue;
                }
            } else if wi == 0 {
                let u = &mut self.s.units[i];
                u.aim = turn_toward(u.aim, desired, 4.0 * DT);
                if wrap(desired - u.aim).abs() > 0.15 {
                    continue;
                }
            }
            let dist = tp.dist(pos);
            let z0 = df.alt + 1.2;
            let z1 = t_alt + 0.8;
            let mut p = Proj {
                pos,
                vel: v2(0.0, 0.0),
                z0,
                z1,
                arc: 0.0,
                life: 1.0,
                total: 1.0,
                kind: w.pk as u8,
                dmg: w.dmg,
                dtype: w.dtype,
                aoe: w.aoe,
                owner,
                target: tid,
                air: t_air,
            };
            match w.pk {
                PK::Direct => {
                    let mut t = dist / w.speed;
                    let mut ap = tp;
                    for _ in 0..2 {
                        ap = tp + tv * t;
                        t = ap.dist(pos) / w.speed;
                    }
                    p.vel = (ap - pos).norm() * w.speed;
                    p.life = w.range * 1.2 / w.speed;
                }
                PK::Arc => {
                    let a = self.rng.range(0.0, 6.283);
                    let r = self.rng.f().sqrt() * w.scatter * dist;
                    let t = (dist / w.speed).max(0.6);
                    let dest = tp + tv * (t * 0.5) + V2::from_angle(a) * r;
                    p.vel = (dest - pos) * (1.0 / t);
                    p.life = t;
                    p.arc = dist * 0.22;
                    p.z1 = 0.0;
                    p.air = false;
                }
                PK::Homing => {
                    p.vel = (tp - pos).norm() * w.speed;
                    p.life = w.range * 2.5 / w.speed;
                }
                PK::Bomb => {
                    let t = 1.3;
                    let dest = tp + tv * t;
                    p.vel = (dest - pos) * (1.0 / t);
                    p.life = t;
                    p.z0 = df.alt;
                    p.z1 = 0.0;
                    p.air = false;
                    self.s.units[i].extend = 3.5;
                }
            }
            p.total = p.life;
            self.s.projs.push(p);
            self.s.units[i].cd[wi] = w.reload;
        }
    }

    fn projectiles(&mut self) {
        let mut projs = std::mem::take(&mut self.s.projs);
        let mut hits: Vec<(V2, usize, Option<usize>)> = vec![]; // (where, proj index, direct victim)
        let mut keep = vec![true; projs.len()];
        for (k, p) in projs.iter_mut().enumerate() {
            let team = self.team_of(p.owner);
            p.life -= DT;
            if p.kind == PK::Direct as u8 {
                let a = p.pos;
                let b = p.pos + p.vel * DT;
                let ab = b - a;
                let mut best: Option<(f32, usize)> = None;
                for (j, u) in self.s.units.iter().enumerate() {
                    if u.hp <= 0.0 || u.host != 0 || u.d().is_air() != p.air || self.team_of(u.owner) == team {
                        continue;
                    }
                    let t = ((u.pos - a).dot(ab) / ab.len2().max(1e-6)).clamp(0.0, 1.0);
                    let c = a + ab * t;
                    if c.dist(u.pos) < u.radius().min(5.0) + 0.4 && best.map_or(true, |x| t < x.0) {
                        best = Some((t, j));
                    }
                }
                if let Some((t, j)) = best {
                    hits.push((a + ab * t, k, Some(j)));
                    keep[k] = false;
                } else {
                    p.pos = b;
                    if p.life <= 0.0 {
                        keep[k] = false;
                        if p.aoe > 0.0 {
                            hits.push((b, k, None));
                        }
                    }
                }
            } else if p.kind == PK::Homing as u8 {
                let speed = p.vel.len();
                let mut hit = None;
                if let Some(j) = self.find(p.target) {
                    let t = &self.s.units[j];
                    let to = t.pos - p.pos;
                    p.vel = to.norm() * speed;
                    if to.len() < speed * DT + t.radius().min(4.0) {
                        hit = Some(j);
                    }
                }
                if let Some(j) = hit {
                    hits.push((self.s.units[j].pos, k, Some(j)));
                    keep[k] = false;
                } else {
                    p.pos = p.pos + p.vel * DT;
                    if p.life <= 0.0 {
                        keep[k] = false;
                        hits.push((p.pos, k, None));
                    }
                }
            } else {
                p.pos = p.pos + p.vel * DT;
                if p.life <= 0.0 {
                    keep[k] = false;
                    hits.push((p.pos, k, None));
                }
            }
        }
        for (at, k, victim) in hits {
            let p = &projs[k];
            let team = self.team_of(p.owner);
            self.s.fx.push(Fx { pos: at, r: p.aoe.max(0.6 + p.dmg.sqrt() * 0.08), h: if p.air { 12.0 } else { 0.5 } });
            if p.aoe > 0.0 {
                for u in self.s.units.iter_mut() {
                    if u.hp <= 0.0 || u.host != 0 || u.d().is_air() != p.air {
                        continue;
                    }
                    if self.s.players[u.owner as usize].team == team {
                        continue;
                    }
                    let d = (u.pos.dist(at) - u.radius().min(5.0) * 0.6).max(0.0);
                    if d < p.aoe {
                        let fall = 1.0 - 0.6 * d / p.aoe;
                        u.hp -= p.dmg * fall * armor_mult(p.dtype, u.d().armor);
                    }
                }
            } else if let Some(j) = victim {
                let u = &mut self.s.units[j];
                u.hp -= p.dmg * armor_mult(p.dtype, u.d().armor);
            }
        }
        let mut k = 0;
        projs.retain(|_| {
            k += 1;
            keep[k - 1]
        });
        self.s.projs = projs;
    }

    fn deaths(&mut self) {
        // Units of defeated players blow up over a few ticks; units inside a dead factory vanish.
        let tick = self.s.tick;
        let alive: Vec<bool> = self.s.players.iter().map(|p| p.alive).collect();
        let ids: std::collections::HashSet<u32> = self.s.units.iter().filter(|u| u.hp > 0.0).map(|u| u.id).collect();
        for u in self.s.units.iter_mut() {
            if !alive[u.owner as usize] && (u.id + tick) % 6 == 0 {
                u.hp = -1.0;
            }
            if u.host != 0 && !ids.contains(&u.host) {
                u.hp = -1.0;
                u.built = 0.0;
            }
        }
        if !self.s.units.iter().any(|u| u.hp <= 0.0) {
            self.check_winner();
            return;
        }
        let mut blasts: Vec<(V2, u8)> = vec![];
        let mut structure_died = false;
        for u in &self.s.units {
            if u.hp > 0.0 {
                continue;
            }
            let d = u.d();
            if u.host != 0 || (u.built <= 0.0 && d.foot == 0) {
                continue; // silently cancelled factory job
            }
            structure_died |= d.foot > 0;
            self.s.fx.push(Fx { pos: u.pos, r: u.radius() * 2.0 + 1.0, h: d.alt + 1.0 });
            if u.def == COMMANDER {
                self.s.fx.push(Fx { pos: u.pos, r: 22.0, h: 2.0 });
                blasts.push((u.pos, u.owner));
            } else if !d.is_air() && u.done() && d.mass >= 20.0 {
                self.s.wrecks.push(Wreck { id: 0, pos: u.pos, mass: d.mass * 0.6, def: u.def, heading: u.heading });
            }
        }
        for w in self.s.wrecks.iter_mut() {
            if w.id == 0 {
                w.id = self.s.next_id;
                self.s.next_id += 1;
            }
        }
        if self.s.wrecks.len() > 300 {
            let extra = self.s.wrecks.len() - 300;
            self.s.wrecks.drain(0..extra);
        }
        for (pos, owner) in blasts {
            self.s.players[owner as usize].alive = false;
            for u in self.s.units.iter_mut() {
                if u.owner != owner && u.pos.dist(pos) < 20.0 && u.hp > 0.0 {
                    u.hp -= 1500.0 * if u.def == COMMANDER { 0.2 } else { 1.0 };
                }
            }
        }
        self.s.units.retain(|u| u.hp > 0.0);
        self.reindex();
        if structure_died {
            self.rebuild_occ();
        }
        self.check_winner();
    }

    fn check_winner(&mut self) {
        if self.s.winner.is_some() || self.s.players.len() < 2 {
            return;
        }
        let mut teams: Vec<u8> = self.s.players.iter().filter(|p| p.alive).map(|p| p.team).collect();
        teams.dedup();
        teams.sort();
        teams.dedup();
        if teams.len() == 1 {
            self.s.winner = Some(teams[0]);
        } else if teams.is_empty() {
            self.s.winner = Some(255);
        }
    }
}

/// Top-left cell of a footprint of `f` cells centred on `p`.
pub fn foot_origin(p: V2, f: i32) -> (i32, i32) {
    let half = f as f32 * CELL * 0.5;
    (((p.x - half + 0.01) / CELL).floor() as i32, ((p.y - half + 0.01) / CELL).floor() as i32)
}

/// Assigns each unit position to one target point so that the total squared
/// travel distance is small and paths do not cross (greedy + pairwise swaps).
pub fn assign(units: &[V2], points: &[V2]) -> Vec<usize> {
    let n = units.len().min(points.len());
    let mut taken = vec![false; points.len()];
    let mut out = vec![0usize; units.len()];
    for (i, u) in units.iter().enumerate() {
        let mut best = (f32::MAX, i.min(points.len().saturating_sub(1)));
        for (k, p) in points.iter().enumerate() {
            if !taken[k] {
                let d = (*p - *u).len2();
                if d < best.0 {
                    best = (d, k);
                }
            }
        }
        if i < n {
            taken[best.1] = true;
        }
        out[i] = best.1;
    }
    if n <= 400 {
        for _ in 0..8 {
            let mut changed = false;
            for a in 0..n {
                for b in a + 1..n {
                    let cur = (points[out[a]] - units[a]).len2() + (points[out[b]] - units[b]).len2();
                    let swp = (points[out[b]] - units[a]).len2() + (points[out[a]] - units[b]).len2();
                    if swp + 0.001 < cur {
                        out.swap(a, b);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
    }
    out
}
