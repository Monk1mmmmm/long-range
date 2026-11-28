//! Skirmish AIs (`AiKind`). They read the JSON definitions by role (extractor, generator, factory, ...)
//! rather than by unit name, so they keep working with modded rosters, and they play through
//! the same `Command` queue as human players.
use crate::data::{DefId, Defs, UnitDef};
use crate::map::*;
use crate::sim::*;
use crate::sim::Command;
use bevy::prelude::*;

pub mod brain;

/// The AI types a lobby slot can hold. `Simple` is the original build-and-wave AI; the others
/// run on the zone engine in `brain.rs` and differ in how they use it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum AiKind {
    #[default]
    Simple,
    /// M27 style: switches between grand strategies as the balance of forces changes.
    Overseer,
    /// M28 style: holds and takes the map zone by zone, helps its allies' zones.
    Zones,
    /// RNG style: expands wide, raids, builds what counters the enemy.
    Adaptive,
    Rusher,
    Turtle,
}

/// In the order the lobby cycles through them.
pub const AI_KINDS: [AiKind; 6] = [AiKind::Zones, AiKind::Overseer, AiKind::Adaptive, AiKind::Rusher, AiKind::Turtle, AiKind::Simple];

impl AiKind {
    pub fn id(self) -> u8 {
        self as u8
    }
    pub fn from_id(id: u8) -> AiKind {
        AI_KINDS.iter().copied().find(|k| k.id() == id).unwrap_or(AiKind::Simple)
    }
    /// Short name: player names, `LR_AI`.
    pub fn name(self) -> &'static str {
        match self {
            AiKind::Simple => "Simple",
            AiKind::Overseer => "Overseer",
            AiKind::Zones => "Zones",
            AiKind::Adaptive => "Adaptive",
            AiKind::Rusher => "Rusher",
            AiKind::Turtle => "Turtle",
        }
    }
    pub fn desc(self) -> &'static str {
        match self {
            AiKind::Simple => "the old AI: builds up at home and attacks in growing waves",
            AiKind::Overseer => "changes its strategy with the situation: rush, eco and tech, air, turtle, commander kill (after M27)",
            AiKind::Zones => "takes and holds the map zone by zone, scouts everything, helps allies (after M28)",
            AiKind::Adaptive => "expands wide, raids extractors, builds what counters your army (after RNG)",
            AiKind::Rusher => "many factories, cheap units, early and constant attacks",
            AiKind::Turtle => "turrets, radar and long range guns; techs up and attacks late",
        }
    }
    pub fn parse(s: &str) -> Option<AiKind> {
        AI_KINDS.iter().copied().find(|k| k.name().eq_ignore_ascii_case(s.trim()))
    }
    pub fn next(self) -> Option<AiKind> {
        let k = AI_KINDS.iter().position(|x| *x == self).unwrap_or(0);
        AI_KINDS.get(k + 1).copied()
    }
}

pub struct Ai {
    pub player: u8,
    pub kind: AiKind,
    /// Idle engineers reclaim trees and rocks (a lobby option).
    pub reclaim: bool,
    wave: u32,
    rng: Rng,
    brain: brain::Brain,
    tries: Tries,
}

impl Ai {
    pub fn new(player: u8, seed: u64) -> Ai {
        Ai::with_kind(player, seed, AiKind::Simple)
    }
    pub fn with_kind(player: u8, seed: u64, kind: AiKind) -> Ai {
        Ai { player, kind, reclaim: true, wave: 0, rng: Rng(seed.wrapping_mul(31).wrapping_add(player as u64 * 7919) | 1), brain: Default::default(), tries: Default::default() }
    }
    /// What the AI is up to (for the self-test's report).
    pub fn debug(&self) -> String {
        if self.kind == AiKind::Simple { format!("wave {}", self.wave) } else { self.brain.debug() }
    }
}

#[derive(Resource, Default)]
pub struct Ais(pub Vec<Ai>);

/// What the zone AI reads of the world.
pub struct Ctx<'a> {
    pub census: &'a Census,
    pub defs: &'a Defs,
    pub map: &'a MapData,
    pub nav: &'a Nav,
    pub game: &'a Game,
    pub features: &'a Features,
    pub wrecks: &'a WreckList,
}

/// One of the AI's own units, as the zone AI sees it.
pub struct Mine {
    pub id: u32,
    pub def: DefId,
    pub pos: V2,
    /// Its current order and how many it has.
    pub order: Option<Order>,
    pub orders: usize,
    /// Under construction, upgrading, inside a factory or transport.
    pub uc: bool,
    pub upg: bool,
    pub hosted: bool,
    /// Factories: length of the production queue.
    pub fq: Option<usize>,
    /// Hit points, 0..1.
    pub hp: f32,
    /// Its queued build orders.
    pub builds: Vec<(DefId, V2)>,
    /// Factories: where finished units go first (`Factory::rally_point`).
    pub rally: Option<V2>,
}

/// A free spot for building `d` near `center` (rings outwards), skipping the places `ok` turns
/// down (`Tries::ok`).
#[allow(clippy::too_many_arguments)]
pub(crate) fn find_spot_where(rng: &mut Rng, defs: &Defs, map: &MapData, occ: &[u8], d: DefId, center: V2, ok: impl Fn(V2) -> bool) -> Option<V2> {
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
            if can_place(defs, map, occ, d, p) && ok(p) {
                return Some(p);
            }
        }
    }
    None
}

/// Build orders the AI gave, and the spots where they came to nothing: the engineer got there and
/// could not build, died on the way or at the site, never arrived, or the site was destroyed
/// before it was finished. Such a spot is left alone for a while, twice as long after every
/// further failure, so the AI does not send engineer after engineer to the same place.
#[derive(Default)]
pub(crate) struct Tries {
    open: Vec<Try>,
    /// (where, radius, left alone until, failures)
    bad: Vec<(V2, f32, u32, u32)>,
}

struct Try {
    def: DefId,
    pos: V2,
    eng: u32,
    /// Where the engineer was last seen.
    at: V2,
    since: u32,
    /// The site stood once.
    started: bool,
}

impl Tries {
    /// How long a spot is left alone after its first failure (ticks).
    const BAN: u32 = 900;

    pub fn ok(&self, p: V2, tick: u32) -> bool {
        !self.bad.iter().any(|b| tick < b.2 && b.0.dist(p) < b.1)
    }

    /// A build order given.
    pub fn note(&mut self, def: DefId, pos: V2, eng: u32, at: V2, tick: u32) {
        if !self.open.iter().any(|o| o.def == def && o.pos.dist(pos) < 0.5) {
            self.open.push(Try { def, pos, eng, at, since: tick, started: false });
        }
    }

    fn fail(&mut self, p: V2, r: f32, tick: u32) {
        match self.bad.iter_mut().find(|b| b.0.dist(p) < b.1.max(r)) {
            Some(b) => {
                b.3 += 1;
                b.1 = b.1.max(r);
                b.2 = tick + (Self::BAN << (b.3 - 1).min(3));
            }
            None => self.bad.push((p, r, tick + Self::BAN, 1)),
        }
    }

    /// Checks the open orders. `eng(id, spot)` says where an engineer is and where in its list
    /// of build orders that spot is (None: it is gone). Returns engineers to call off: their
    /// spot was given up on.
    pub fn update(&mut self, defs: &Defs, census: &Census, me: u8, tick: u32, eng: impl Fn(u32, V2) -> Option<(V2, Option<usize>)>) -> Vec<u32> {
        let mut stop = vec![];
        let mut failed: Vec<(V2, f32)> = vec![];
        self.open.retain_mut(|o| {
            // Ours (finished field works belong to nobody), or somebody else's.
            let mut site: Option<(bool, bool)> = None;
            census.near(o.pos, 1.0, |c| {
                if c.structure && c.pos.dist(o.pos) < 0.5 && c.hp > 0.0 {
                    site = Some((c.def == o.def && (c.owner == me || (c.done && c.owner == NEUTRAL)), c.done));
                }
            });
            let r = defs.get(o.def).footprint as f32 * CELL * 0.5 + 2.0;
            let e = eng(o.eng, o.pos).map(|(at, k)| {
                o.at = at;
                // Still waiting for its turn: the clock starts when the engineer sets out for it.
                if k.map_or(false, |k| k > 0) {
                    o.since = tick;
                }
                (at, k.is_some())
            });
            let near = o.at.dist(o.pos) < r + 10.0;
            match (site, e) {
                (Some((true, true)), _) => false,
                (Some((false, _)), _) => {
                    failed.push((o.pos, r));
                    false
                }
                (Some((true, false)), e) => {
                    o.started = true;
                    // Its engineer left it: the AI gives a new order if it wants it finished.
                    e.map_or(false, |e| e.1)
                }
                // Started and gone again: destroyed while being built, the place is under fire.
                (None, e) if o.started => {
                    failed.push((o.pos, r + 6.0));
                    if let Some((_, true)) = e {
                        stop.push(o.eng);
                    }
                    false
                }
                // Killed at the site or close to it.
                (None, None) => {
                    if near {
                        failed.push((o.pos, r + 6.0));
                    }
                    false
                }
                // Arrived and could not build there (an order replaced far away does not count).
                (None, Some((_, false))) => {
                    if near {
                        failed.push((o.pos, r));
                    }
                    false
                }
                (None, Some((_, true))) if tick > o.since + 1800 => {
                    // Three minutes and still not there: it cannot get through.
                    failed.push((o.pos, r));
                    stop.push(o.eng);
                    false
                }
                _ => true,
            }
        });
        for (p, r) in failed {
            self.fail(p, r, tick);
        }
        // Spots forgiven a while after their ban ran out.
        self.bad.retain(|b| tick < b.2 + 6000);
        stop
    }
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
    features: Res<Features>,
    wrecks: Res<WreckList>,
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
        if ai.kind != AiKind::Simple {
            let mut mine: Vec<Mine> = q
                .iter()
                .filter(|(u, ..)| u.owner == me)
                .map(|(u, b, orders, factory, uc, upg, hosted)| Mine {
                    id: u.id,
                    def: u.def,
                    pos: b.pos,
                    order: orders.0.first().copied(),
                    orders: orders.0.len(),
                    uc,
                    upg,
                    hosted,
                    fq: factory.map(|f| f.queue.len()),
                    hp: census.get(u.id).map_or(1.0, |c| c.hp / c.max_hp.max(1.0)),
                    builds: orders.0.iter().filter_map(|o| if let Order::Build { def, pos } = o { Some((*def, *pos)) } else { None }).collect(),
                    rally: factory.and_then(|f| f.rally_point()),
                })
                .collect();
            mine.sort_by_key(|m| m.id);
            // Units just out of a factory still on its rally orders: only land fighters keep them (a
            // transport assisting the factory may carry them); engineers, scavengers, transports and
            // aircraft are stopped, so the brain finds them idle as before factories had rally orders.
            let rallies: Vec<&Vec<Order>> = q.iter().filter(|(u, ..)| u.owner == me).filter_map(|(_, _, _, f, ..)| f.map(|f| &f.rally)).filter(|r| !r.is_empty()).collect();
            let mut halt: Vec<u32> = vec![];
            for (u, _, orders, ..) in q.iter().filter(|(u, _, o, ..)| u.owner == me && !o.0.is_empty()) {
                let d = defs.get(u.def);
                let fighter = !d.is_air() && d.build.is_none() && d.transport.is_none() && d.armed();
                if !fighter && rallies.iter().any(|r| **r == orders.0) {
                    halt.push(u.id);
                }
            }
            if !halt.is_empty() {
                halt.sort_unstable();
                for m in mine.iter_mut().filter(|m| halt.binary_search(&m.id).is_ok()) {
                    (m.order, m.orders) = (None, 0);
                }
                out.push(Command::Stop { ids: halt });
            }
            let cx = Ctx { census: &census, defs: &defs, map: &map, nav: &nav, game: &game, features: &features, wrecks: &wrecks };
            brain::run(&mut ai.brain, &mut ai.rng, ai.kind, ai.reclaim, me, &cx, &mine, &mut out);
            queue.0.extend(out.into_iter().map(|c| (me, c)));
            continue;
        }
        // In id order: the ECS's own order is not the same on every machine or run.
        let mut all: Vec<_> = q.iter().collect();
        all.sort_by_key(|x| x.0.id);
        let team = p.team;
        let start = p.start;
        let world = map.extent();
        let to_center = (map.center() - start).norm();
        let commander = defs.factions[p.faction as usize].commander;
        let is_pump = |d: &UnitDef| d.needs_deposit;
        let is_power = |d: &UnitDef| d.is_structure() && d.eco.energy > 0.0;
        let is_fac = |d: &UnitDef, cat: &str| d.is_factory() && d.build.as_ref().map_or(false, |b| b.categories.iter().any(|c| c == cat));

        let mut count = vec![0u32; defs.units.len()];
        let mut claimed: Vec<V2> = vec![];
        let mut upgrading_any = false;
        for (u, b, orders, _, _, upg, _) in all.iter().copied() {
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

        // Build spots that came to nothing are left alone for a while.
        let tick = game.tick;
        let quit = ai.tries.update(&defs, &census, me, tick, |id, at| {
            all.binary_search_by_key(&id, |x| x.0.id).ok().map(|k| (all[k].1.pos, all[k].2 .0.iter().filter(|o| matches!(o, Order::Build { .. })).position(|o| matches!(o, Order::Build { pos, .. } if pos.dist(at) < 0.5))))
        });
        if !quit.is_empty() {
            out.push(Command::Stop { ids: quit });
        }

        // --- builders
        for (u, b, orders, _, uc, _, hosted) in all.iter().copied() {
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
                .filter(|d| !claimed.iter().any(|c| c.dist(**d) < 1.0) && ai.tries.ok(**d, tick))
                .filter(|d| d.dist(start) < if is_com { 34.0 } else { world * 0.45 + ai.wave as f32 * 12.0 })
                .min_by(|a, c| a.dist(b.pos).partial_cmp(&c.dist(b.pos)).unwrap());
            let spot = |ai: &mut Ai, d: DefId, at: V2| {
                let Ai { rng, tries, .. } = ai;
                find_spot_where(rng, &defs, &map, &nav.occ, d, at, |q| tries.ok(q, tick)).map(|s| (d, s))
            };
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
                    // Higher radars cost a lot to run: only when the income carries them.
                    .filter(|d| d.eco.upkeep <= p.income_e * 0.2)
                    .filter(|d| count[d.idx as usize] < if d.armed() { 4 } else { 2 })
                    .collect();
                if !extras.is_empty() {
                    let d = extras[ai.rng.below(extras.len())];
                    let at = if d.armed() || d.has_radar() { start + to_center * 20.0 } else { start - to_center * 4.0 };
                    choice = spot(ai, d.idx, at);
                }
            }
            match choice {
                Some((d, pos)) => {
                    count[d as usize] += 1;
                    ai.tries.note(d, pos, u.id, b.pos, tick);
                    out.push(Command::Orders { ids: vec![u.id], orders: vec![Order::Build { def: d, pos }], queue: false });
                }
                None => {
                    // Nothing to build: reclaim rocks while the mass bank has room, trees when
                    // energy runs low (nearby and on our side of the map), else help a factory.
                    let want_mass = p.mass < p.max_mass * 0.8;
                    let want_energy = p.energy < p.max_energy * 0.5;
                    let mut pile: Option<(f32, V2)> = None;
                    if !is_com && ai.reclaim && (want_mass || want_energy) {
                        features.near(b.pos, 45.0, |_, f| {
                            let Some(fd) = defs.rules.features.get(f.def as usize) else { return };
                            let useful = (want_mass && fd.mass >= 5.0) || (want_energy && fd.energy >= 5.0);
                            let d = f.pos.dist(b.pos);
                            if useful && f.pos.dist(start) < world * 0.35 && pile.map_or(true, |p| d < p.0) {
                                pile = Some((d, f.pos));
                            }
                        });
                    }
                    if let Some((_, at)) = pile {
                        out.push(Command::Orders { ids: vec![u.id], orders: vec![Order::ReclaimArea { pos: at, radius: 10.0 }], queue: false });
                    } else if !is_com {
                        if let Some(f) = census.list.iter().find(|c| c.owner == me && c.done && defs.get(c.def).is_factory()) {
                            out.push(Command::Orders { ids: vec![u.id], orders: vec![Order::Assist(f.id)], queue: false });
                        }
                    }
                }
            }
        }

        // --- factories: tier up when the economy allows, otherwise produce
        let mut eng_count = engineers;
        for (u, _, _, factory, uc, upg, _) in all.iter().copied() {
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
                let pool: Vec<&&UnitDef> = opts.iter().filter(|d| !d.is_engineer() && !d.is_scavenger() && d.transport.is_none() && d.cost.mass <= budget.max(80.0) && d.eco.upkeep <= p.income_e * 0.2).collect();
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
            if let Some((u, ..)) = all.iter().copied().find(|(u, _, _, _, uc, upg, _)| {
                let d = defs.get(u.def);
                u.owner == me && !uc && !upg && d.needs_deposit && d.upgrade_id.map_or(false, |t| defs.get(t).cost.mass < p.mass + p.income_m * 100.0)
            }) {
                out.push(Command::Upgrade { ids: vec![u.id] });
            }
        }

        // --- army
        let army: Vec<(u32, V2)> = all
            .iter()
            .copied()
            .filter(|(u, _, orders, _, uc, _, hosted)| {
                let d = defs.get(u.def);
                u.owner == me && !uc && !hosted && orders.0.is_empty() && !d.is_structure() && d.build.is_none() && d.transport.is_none()
            })
            .map(|(u, b, ..)| (u.id, b.pos))
            .collect();
        if !army.is_empty() {
            // Control points mode: the nearest point we do not hold, once a few units are ready.
            // (Idle units standing on a point they are taking get the same order again and stay.)
            let point = game.points.iter().filter(|cp| cp.owner != team).min_by(|a, b| a.pos.dist(start).partial_cmp(&b.pos.dist(start)).unwrap()).map(|cp| cp.pos);
            if let Some(t) = census.nearest_enemy(start, team, 70.0, 0.0, true, false) {
                out.push(Command::Orders { ids: army.iter().map(|a| a.0).collect(), orders: vec![Order::AttackMove(t.pos)], queue: false });
            } else if let (Some(cp), true) = (point, army.len() >= 3 && army.len() < (5 + ai.wave * 3).min(28) as usize && game.tick % 50 < 10) {
                let land: Vec<u32> = army.iter().filter(|a| census.get(a.0).map_or(false, |c| !c.air && c.pos.dist(cp) > 5.0)).map(|a| a.0).collect();
                if !land.is_empty() {
                    out.push(Command::Orders { ids: land, orders: vec![Order::AttackMove(cp)], queue: false });
                }
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
