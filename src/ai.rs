//! Skirmish AI. It reads the JSON definitions by role (extractor, generator, factory, ...)
//! rather than by unit name, so it keeps working with modded rosters, and it plays through
//! the same `Command` queue as human players.
use crate::data::{DefId, Defs, UnitDef};
use crate::map::*;
use crate::sim::*;
use crate::sim::Command;
use bevy::prelude::*;

pub struct Ai {
    pub player: u8,
    wave: u32,
    rng: Rng,
}

impl Ai {
    pub fn new(player: u8, seed: u64) -> Ai {
        Ai { player, wave: 0, rng: Rng(seed.wrapping_mul(31).wrapping_add(player as u64 * 7919) | 1) }
    }
}

#[derive(Resource, Default)]
pub struct Ais(pub Vec<Ai>);

fn find_spot(rng: &mut Rng, defs: &Defs, map: &MapData, occ: &[u8], d: DefId, center: V2) -> Option<V2> {
    let step = (defs.get(d).footprint + 1) as f32 * CELL;
    for ring in 1..14i32 {
        let mut cands = vec![];
        for j in -ring..=ring {
            for i in -ring..=ring {
                if i.abs() == ring || j.abs() == ring {
                    cands.push((i, j));
                }
            }
        }
        for _ in 0..cands.len() {
            let k = rng.below(cands.len());
            let (i, j) = cands.swap_remove(k);
            let p = snap(defs, map, d, center + v2(i as f32 * step, j as f32 * step));
            if can_place(defs, map, occ, d, p) {
                return Some(p);
            }
        }
    }
    None
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn ai_system(
    ais: Option<ResMut<Ais>>,
    q: Query<(&Unit, &Body, &Orders, Option<&Factory>, Has<UnderConstruction>, Has<Upgrading>, Has<Hosted>)>,
    census: Res<Census>,
    defs: Res<Defs>,
    map: Res<MapData>,
    nav: Res<Nav>,
    game: Res<Game>,
    mut queue: ResMut<CommandQueue>,
) {
    let Some(mut ais) = ais else { return };
    for ai in ais.0.iter_mut() {
        if (game.tick + ai.player as u32 * 3) % 10 != 0 {
            continue;
        }
        let me = ai.player;
        let Some(p) = game.players.get(me as usize) else { continue };
        if !p.alive {
            continue;
        }
        let mut out: Vec<Command> = vec![];
        let team = p.team;
        let start = p.start;
        let world = map.world();
        let to_center = (map.center() - start).norm();
        let commander = defs.factions[p.faction as usize].commander;
        let is_pump = |d: &UnitDef| d.needs_deposit;
        let is_power = |d: &UnitDef| d.is_structure() && d.eco.energy > 0.0;
        let is_fac = |d: &UnitDef, cat: &str| d.is_factory() && d.build.as_ref().map_or(false, |b| b.categories.iter().any(|c| c == cat));

        let mut count = vec![0u32; defs.units.len()];
        let mut claimed: Vec<V2> = vec![];
        let mut upgrading_any = false;
        for (u, b, orders, _, _, upg, _) in &q {
            if defs.get(u.def).needs_deposit {
                claimed.push(b.pos);
            }
            if u.owner != me {
                continue;
            }
            count[u.def as usize] += 1;
            upgrading_any |= upg;
            for o in &orders.0 {
                if let Order::Build { def: d, pos } = o {
                    count[*d as usize] += 1;
                    if defs.get(*d).needs_deposit {
                        claimed.push(*pos);
                    }
                }
            }
        }
        let total = |f: &dyn Fn(&UnitDef) -> bool| -> u32 { defs.units.iter().filter(|d| f(d)).map(|d| count[d.idx as usize]).sum() };
        let mexes = total(&is_pump);
        let power = total(&is_power);
        let land_facs = total(&|d| is_fac(d, "land"));
        let air_facs = total(&|d| is_fac(d, "air"));
        let engineers = total(&|d| d.is_engineer() && Some(d.idx) != commander);
        // Build generators when the bank is draining, not just because spending is high.
        let mut need_power = p.energy < p.max_energy * 0.25
            || (p.income_e - p.spend_e < 10.0 && p.energy < p.max_energy * 0.9)
            || p.income_e < 40.0 + mexes as f32 * 6.0 + (land_facs + air_facs) as f32 * 40.0;

        // --- builders
        for (u, b, orders, _, uc, _, hosted) in &q {
            let df = defs.get(u.def);
            if u.owner != me || uc || hosted || !df.is_engineer() || !orders.0.is_empty() {
                continue;
            }
            let is_com = Some(u.def) == commander;
            let opts: Vec<&UnitDef> = df.builds.iter().map(|d| defs.get(*d)).collect();
            let cheapest = |f: &dyn Fn(&UnitDef) -> bool| opts.iter().filter(|d| f(d)).min_by(|a, b| a.cost.mass.partial_cmp(&b.cost.mass).unwrap()).map(|d| d.idx);
            let mut choice: Option<(DefId, V2)> = None;
            let near_dep = map
                .deposits
                .iter()
                .filter(|d| !claimed.iter().any(|c| c.dist(**d) < 1.0))
                .filter(|d| d.dist(start) < if is_com { 34.0 } else { world * 0.45 + ai.wave as f32 * 12.0 })
                .min_by(|a, c| a.dist(b.pos).partial_cmp(&c.dist(b.pos)).unwrap());
            let spot = |ai: &mut Ai, d: DefId, at: V2| find_spot(&mut ai.rng, &defs, &map, &nav.occ, d, at).map(|s| (d, s));
            if land_facs == 0 && mexes >= 2 && power >= 2 {
                choice = cheapest(&|d| is_fac(d, "land")).and_then(|d| spot(ai, d, start + to_center * 9.0));
            } else if need_power {
                // The biggest generator we can sensibly afford.
                let best = opts
                    .iter()
                    .filter(|d| is_power(d) && (d.cost.mass <= 100.0 || d.cost.mass < p.income_m * 45.0))
                    .max_by(|a, b| a.eco.energy.partial_cmp(&b.eco.energy).unwrap())
                    .map(|d| d.idx);
                choice = best.and_then(|d| spot(ai, d, start - to_center * 6.0));
                need_power = false;
            } else if let (Some(dep), Some(pump)) = (near_dep, cheapest(&is_pump)) {
                choice = Some((pump, *dep));
                claimed.push(*dep);
            } else if land_facs < (1 + mexes / 6).min(3) {
                choice = cheapest(&|d| is_fac(d, "land")).and_then(|d| spot(ai, d, start + to_center * 9.0));
            } else if air_facs == 0 && mexes >= 6 {
                choice = cheapest(&|d| is_fac(d, "air")).and_then(|d| spot(ai, d, start));
            } else if p.mass > p.max_mass * 0.5 {
                let extras: Vec<&&UnitDef> = opts
                    .iter()
                    .filter(|d| !d.is_factory() && !is_pump(d) && !is_power(d) && d.cost.mass < p.mass + p.income_m * 60.0)
                    .filter(|d| count[d.idx as usize] < if d.armed() { 4 } else { 2 })
                    .collect();
                if !extras.is_empty() {
                    let d = extras[ai.rng.below(extras.len())];
                    let at = if d.armed() || d.radar > 0.0 { start + to_center * 20.0 } else { start - to_center * 4.0 };
                    choice = spot(ai, d.idx, at);
                }
            }
            match choice {
                Some((d, pos)) => {
                    count[d as usize] += 1;
                    out.push(Command::Orders { ids: vec![u.id], orders: vec![Order::Build { def: d, pos }], queue: false });
                }
                None => {
                    if !is_com {
                        if let Some(f) = census.list.iter().find(|c| c.owner == me && c.done && defs.get(c.def).is_factory()) {
                            out.push(Command::Orders { ids: vec![u.id], orders: vec![Order::Assist(f.id)], queue: false });
                        }
                    }
                }
            }
        }

        // --- factories: tier up when the economy allows, otherwise produce
        let mut eng_count = engineers;
        for (u, _, _, factory, uc, upg, _) in &q {
            let (Some(f), false, false) = (factory, uc, upg) else { continue };
            let df = defs.get(u.def);
            if u.owner != me || !f.queue.is_empty() {
                continue;
            }
            if let Some(up) = df.upgrade_id {
                let t = df.tier as f32;
                if !upgrading_any && p.income_m > 9.0 * t + 4.0 && p.income_e > 220.0 * t && defs.get(up).cost.mass < p.mass + p.income_m * 90.0 {
                    upgrading_any = true;
                    out.push(Command::Upgrade { ids: vec![u.id] });
                    continue;
                }
            }
            let opts: Vec<&UnitDef> = df.builds.iter().map(|d| defs.get(*d)).collect();
            let eng = opts.iter().filter(|d| d.is_engineer()).max_by_key(|d| d.tier);
            let pick = if eng.is_some() && eng_count < (2 + mexes / 3).min(8) {
                eng_count += 1;
                eng.map(|d| d.idx)
            } else {
                // Combat units, biased toward the higher tiers we can pay for.
                let budget = p.mass + p.income_m * 120.0;
                let pool: Vec<&&UnitDef> = opts.iter().filter(|d| !d.is_engineer() && d.cost.mass <= budget.max(80.0)).collect();
                let weight = |d: &UnitDef| (d.tier as f32).powi(2) * if d.armed() { 3.0 } else { 0.6 };
                let sum: f32 = pool.iter().map(|d| weight(d)).sum();
                let mut roll = ai.rng.f() * sum;
                let mut chosen = pool.first().map(|d| d.idx);
                for d in &pool {
                    roll -= weight(d);
                    if roll <= 0.0 {
                        chosen = Some(d.idx);
                        break;
                    }
                }
                chosen
            };
            if let Some(d) = pick {
                out.push(Command::Produce { factory: u.id, def: d, count: 1 });
            }
        }

        // --- extractor upgrades
        if !upgrading_any && p.mass > 400.0 && mexes >= 4 && p.income_e > 150.0 {
            if let Some((u, ..)) = q.iter().find(|(u, _, _, _, uc, upg, _)| {
                let d = defs.get(u.def);
                u.owner == me && !uc && !upg && d.needs_deposit && d.upgrade_id.map_or(false, |t| defs.get(t).cost.mass < p.mass + p.income_m * 100.0)
            }) {
                out.push(Command::Upgrade { ids: vec![u.id] });
            }
        }

        // --- army
        let army: Vec<(u32, V2)> = q
            .iter()
            .filter(|(u, _, orders, _, uc, _, hosted)| {
                let d = defs.get(u.def);
                u.owner == me && !uc && !hosted && orders.0.is_empty() && !d.is_structure() && !d.is_engineer()
            })
            .map(|(u, b, ..)| (u.id, b.pos))
            .collect();
        if !army.is_empty() {
            if let Some(t) = census.nearest_enemy(start, team, 70.0, 0.0, true, false) {
                out.push(Command::Orders { ids: army.iter().map(|a| a.0).collect(), orders: vec![Order::AttackMove(t.pos)], queue: false });
            } else if army.len() >= (5 + ai.wave * 3).min(28) as usize && game.tick % 100 < 10 {
                let target = census
                    .list
                    .iter()
                    .filter(|c| hostile(team, c.team) && c.team != TEAM_HOSTILE && game.players.get(c.owner as usize).map_or(false, |pl| defs.factions[pl.faction as usize].commander == Some(c.def)))
                    .min_by(|a, b| a.pos.dist(start).partial_cmp(&b.pos.dist(start)).unwrap())
                    .map(|c| c.pos);
                if let Some(t) = target {
                    ai.wave += 1;
                    let n = army.len();
                    let cols = (n as f32).sqrt().ceil() as usize;
                    let fwd = (t - start).norm();
                    let side = fwd.perp();
                    let pts: Vec<V2> = (0..n)
                        .map(|k| {
                            let (c, r) = ((k % cols) as f32 - cols as f32 * 0.5, (k / cols) as f32);
                            map.clamp(t + side * (c * 3.0) - fwd * (r * 3.0 + 12.0))
                        })
                        .collect();
                    let pos: Vec<V2> = army.iter().map(|a| a.1).collect();
                    let asg = assign(&pos, &pts);
                    out.push(Command::Orders { ids: army.iter().map(|a| a.0).collect(), orders: asg.iter().map(|k| Order::AttackMove(pts[*k])).collect(), queue: false });
                }
            }
        }
        queue.0.extend(out.into_iter().map(|c| (me, c)));
    }
}

/// Assigns each unit position to one target point so that the total squared travel distance
/// is small and paths do not cross (greedy start, then pairwise swaps).
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
