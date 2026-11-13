//! A simple skirmish AI. It plays through the same `Command` interface as humans.
use crate::defs::*;
use crate::map::*;
use crate::sim::*;

pub struct Ai {
    pub player: u8,
    wave: u32,
    rng: Rng,
}

impl Ai {
    pub fn new(player: u8, seed: u64) -> Ai {
        Ai { player, wave: 0, rng: Rng(seed.wrapping_mul(31).wrapping_add(player as u64 * 7919) | 1) }
    }

    fn find_spot(&mut self, sim: &Sim, d: DefId, center: V2) -> Option<V2> {
        let step = (def(d).foot + 1) as f32 * CELL;
        for ring in 2..14i32 {
            let mut cands = vec![];
            for j in -ring..=ring {
                for i in -ring..=ring {
                    if i.abs() == ring || j.abs() == ring {
                        cands.push((i, j));
                    }
                }
            }
            for _ in 0..cands.len() {
                let k = self.rng.below(cands.len());
                let (i, j) = cands.swap_remove(k);
                let p = sim.snap(d, center + v2(i as f32 * step, j as f32 * step));
                if sim.can_place(d, p) {
                    return Some(p);
                }
            }
        }
        None
    }

    pub fn think(&mut self, sim: &Sim) -> Vec<Command> {
        let me = self.player;
        let mut out = vec![];
        let p = &sim.s.players[me as usize];
        if !p.alive {
            return out;
        }
        let team = p.team;
        let start = p.start;
        let world = sim.map.world();
        let to_center = (v2(world * 0.5, world * 0.5) - start).norm();
        let mut count = [0u32; DEFS.len()];
        let mut claimed: Vec<V2> = vec![];
        for u in &sim.s.units {
            if u.d().needs_deposit {
                claimed.push(u.pos);
            }
            if u.owner != me {
                continue;
            }
            count[u.def as usize] += 1;
            for o in &u.orders {
                if let Order::Build { def: d, pos } = o {
                    count[*d as usize] += 1;
                    if def(*d).needs_deposit {
                        claimed.push(*pos);
                    }
                }
            }
        }
        let mexes = count[PUMP as usize] + count[PUMP2 as usize];
        let mut need_power = p.energy < p.max_energy * 0.3
            || p.income_e
                < 30.0
                    + mexes as f32 * 8.0
                    + (count[LAND_FACTORY as usize] + count[AIR_FACTORY as usize]) as f32 * 60.0
                    + count[PUMP2 as usize] as f32 * 30.0;

        // --- builders
        for u in &sim.s.units {
            if u.owner != me || !u.done() || !u.d().is_builder() || !u.orders.is_empty() {
                continue;
            }
            let is_com = u.def == COMMANDER;
            let mut choice: Option<(DefId, V2)> = None;
            let near_dep = sim
                .map
                .deposits
                .iter()
                .filter(|d| !claimed.iter().any(|c| c.dist(**d) < 1.0))
                .filter(|d| d.dist(start) < if is_com { 60.0 } else { world * 0.45 + self.wave as f32 * 20.0 })
                .min_by(|a, b| a.dist(u.pos).partial_cmp(&b.dist(u.pos)).unwrap());
            if count[LAND_FACTORY as usize] == 0 && mexes >= 2 && count[WINDMILL as usize] >= 2 {
                choice = self.find_spot(sim, LAND_FACTORY, start + to_center * 14.0).map(|s| (LAND_FACTORY, s));
            } else if need_power {
                let d = if !is_com && p.income_m > 8.0 && self.rng.f() < 0.5 { SOLAR } else { WINDMILL };
                choice = self.find_spot(sim, d, start - to_center * 10.0).map(|s| (d, s));
                need_power = false;
            } else if let Some(dep) = near_dep {
                choice = Some((PUMP, *dep));
                claimed.push(*dep);
            } else if count[LAND_FACTORY as usize] < (1 + mexes / 6).min(3) {
                choice = self.find_spot(sim, LAND_FACTORY, start + to_center * 14.0).map(|s| (LAND_FACTORY, s));
            } else if count[AIR_FACTORY as usize] == 0 && mexes >= 6 {
                choice = self.find_spot(sim, AIR_FACTORY, start).map(|s| (AIR_FACTORY, s));
            } else if p.mass > p.max_mass * 0.6 {
                let opts = [MG_TURRET, GAUSS_TURRET, AA_TURRET, RADAR_TOWER, E_STORAGE, GAUSS_TURRET, M_STORAGE];
                let d = opts[self.rng.below(opts.len())];
                if count[d as usize] < 5 && !(d == RADAR_TOWER && count[d as usize] > 0) {
                    choice = self.find_spot(sim, d, start + to_center * 34.0).map(|s| (d, s));
                }
            }
            match choice {
                Some((d, pos)) => {
                    count[d as usize] += 1;
                    out.push(Command::Orders { ids: vec![u.id], orders: vec![Order::Build { def: d, pos }], queue: false });
                }
                None => {
                    // Nothing to build: help a factory.
                    if let Some(f) = sim.s.units.iter().find(|f| f.owner == me && f.d().kind == Kind::Factory && f.done()) {
                        if !is_com {
                            out.push(Command::Orders { ids: vec![u.id], orders: vec![Order::Assist(f.id)], queue: false });
                        }
                    }
                }
            }
        }

        // --- factories
        let late = sim.s.tick > 4200;
        for f in &sim.s.units {
            if f.owner != me || !f.done() || f.d().kind != Kind::Factory || !f.queue.is_empty() {
                continue;
            }
            let d = if f.def == LAND_FACTORY {
                if count[ENGINEER as usize] < (2 + mexes / 3).min(7) {
                    count[ENGINEER as usize] += 1;
                    ENGINEER
                } else if late {
                    let o = [OVERTAKER, CRONY, CRONY, CRONY, MORTAR, MORTAR, ROBIN, BOHDANA, FLAK, DRAGOON, TROOPER, BUGGY];
                    o[self.rng.below(o.len())]
                } else {
                    let o = [TROOPER, TROOPER, BUGGY, OVERTAKER, OVERTAKER, DRAGOON, MORTAR];
                    o[self.rng.below(o.len())]
                }
            } else {
                let o = [INTERCEPTOR, BOMBER, BOMBER, GUNSHIP, GUNSHIP, AIR_SCOUT];
                o[self.rng.below(o.len())]
            };
            out.push(Command::Produce { factory: f.id, def: d, count: 1 });
        }

        // --- economy upgrades
        if p.mass > 450.0 && mexes >= 4 && p.income_e > 150.0 {
            if let Some(m) = sim.s.units.iter().find(|u| u.owner == me && u.def == PUMP && u.done() && u.upg == 0.0) {
                if !sim.s.units.iter().any(|u| u.owner == me && u.upg > 0.0) {
                    out.push(Command::Upgrade { ids: vec![m.id] });
                }
            }
        }

        // --- army
        let army: Vec<&Unit> = sim
            .s
            .units
            .iter()
            .filter(|u| {
                u.owner == me && u.done() && u.host == 0 && u.orders.is_empty() && !u.d().is_structure() && !u.d().is_builder()
            })
            .collect();
        if army.is_empty() {
            return out;
        }
        // Defend the base against anything we know about.
        if let Some(j) = sim.nearest_enemy(start, team, 90.0, 0.0, true, false) {
            let ids: Vec<u32> = army.iter().map(|u| u.id).collect();
            out.push(Command::Orders { ids, orders: vec![Order::AttackMove(sim.s.units[j].pos)], queue: false });
            return out;
        }
        let need = (5 + self.wave * 3).min(28) as usize;
        if army.len() >= need && sim.s.tick % 100 < 10 {
            let target = sim
                .s
                .units
                .iter()
                .filter(|u| u.def == COMMANDER && sim.team_of(u.owner) != team)
                .min_by(|a, b| a.pos.dist(start).partial_cmp(&b.pos.dist(start)).unwrap())
                .map(|u| u.pos);
            if let Some(t) = target {
                self.wave += 1;
                // Keep the group loose: BAR-style spread around the destination.
                let n = army.len();
                let cols = (n as f32).sqrt().ceil() as usize;
                let side = (t - start).norm().perp();
                let fwd = (t - start).norm();
                let pts: Vec<V2> = (0..n)
                    .map(|k| {
                        let (c, r) = ((k % cols) as f32 - cols as f32 * 0.5, (k / cols) as f32);
                        sim.map.clamp(t + side * (c * 6.0) - fwd * (r * 6.0 + 20.0))
                    })
                    .collect();
                let pos: Vec<V2> = army.iter().map(|u| u.pos).collect();
                let asg = assign(&pos, &pts);
                out.push(Command::Orders {
                    ids: army.iter().map(|u| u.id).collect(),
                    orders: asg.iter().map(|k| Order::AttackMove(pts[*k])).collect(),
                    queue: false,
                });
            }
        }
        out
    }
}
