//! Simulation systems, run in order once per tick on every machine in the game.
//!
//! The simulation must be deterministic: every query loop that allocates ids, uses the random
//! generator, spends a shared budget or queues effects iterates in unit-id order (the ECS's own
//! order depends on each machine's entity layout), and angles go through `map::fm` (same results
//! on every platform, unlike the system maths library).
use super::*;
use crate::data::{Defs, MoveKind, WeaponDef, WeaponKind};
use std::f32::consts::PI;

pub fn register(app: &mut App) {
    // `mark::<I>` between the systems times them when a `SimProfile` is present (LR_PROFILE=1).
    app.add_systems(
        SimTick,
        (
            // Inner tuples need their own chain(): the outer one only orders the groups.
            (mark::<0>, begin_tick, mark::<1>, census_build, mark::<2>, crate::ai::ai_system, mark::<3>, apply_commands, mark::<4>, visibility, mark::<5>, income, mark::<6>).chain(),
            (think, mark::<7>, assaults, mark::<8>, factories, mark::<9>, build_progress, mark::<10>, locomotion, mark::<11>, separation, mark::<12>, carry, mark::<13>, garrisons, mark::<14>).chain(),
            (weapons, mark::<15>, projectiles, mark::<16>, rubble_motion, mark::<17>, apply_reclaim, mark::<18>, apply_capture, mark::<19>, deaths, mark::<20>, apply_ground, mark::<21>, control_points, mark::<22>, victory, mark::<23>).chain(),
        )
            .chain(),
    );
}

/// The tick's systems in the order `register` runs them (names for `SimProfile`).
pub const SYSTEM_NAMES: [&str; 23] = [
    "begin_tick", "census_build", "ai_system", "apply_commands", "visibility", "income", "think", "assaults", "factories", "build_progress", "locomotion", "separation", "carry", "garrisons", "weapons", "projectiles", "rubble_motion", "apply_reclaim", "apply_capture", "deaths", "apply_ground", "control_points", "victory",
];

/// Wall time each tick system took, summed over the ticks run while it is present (`LR_PROFILE=1`
/// in the headless tests), and the slowest single tick of each. Measurement only: no system reads
/// it, it is not part of the state.
#[derive(Resource)]
pub struct SimProfile {
    last: std::time::Instant,
    start: std::time::Instant,
    pub total: [std::time::Duration; SYSTEM_NAMES.len()],
    pub worst: [std::time::Duration; SYSTEM_NAMES.len()],
    /// Slowest whole tick.
    pub worst_tick: std::time::Duration,
    pub ticks: u32,
}

impl Default for SimProfile {
    fn default() -> Self {
        let now = std::time::Instant::now();
        SimProfile { last: now, start: now, total: Default::default(), worst: Default::default(), worst_tick: Default::default(), ticks: 0 }
    }
}

impl SimProfile {
    /// The systems that took longest, with their share of the tick, the time per tick and their
    /// slowest tick.
    pub fn report(&self) -> String {
        let sum: f64 = self.total.iter().map(|d| d.as_secs_f64()).sum::<f64>().max(1e-9);
        let ticks = self.ticks.max(1) as f64;
        let mut rows: Vec<(usize, f64)> = self.total.iter().map(|d| d.as_secs_f64()).enumerate().collect();
        rows.sort_by(|a, b| b.1.total_cmp(&a.1));
        let mut out = format!("tick systems over {} ticks: {:.3} ms per tick, slowest tick {:.3} ms\n", self.ticks, sum * 1000.0 / ticks, self.worst_tick.as_secs_f64() * 1000.0);
        for (i, t) in rows {
            out += &format!("  {:<15} {:6.1}%  {:8.3} ms/tick  slowest {:7.3} ms\n", SYSTEM_NAMES[i], t * 100.0 / sum, t * 1000.0 / ticks, self.worst[i].as_secs_f64() * 1000.0);
        }
        out
    }

    /// One line: time per tick, slowest tick and the `n` systems that took longest (ms per tick).
    pub fn short(&self, n: usize) -> String {
        let ticks = self.ticks.max(1) as f64;
        let sum: f64 = self.total.iter().map(|d| d.as_secs_f64()).sum();
        let mut rows: Vec<(usize, f64)> = self.total.iter().map(|d| d.as_secs_f64()).enumerate().collect();
        rows.sort_by(|a, b| b.1.total_cmp(&a.1));
        let top: Vec<String> = rows.iter().take(n).map(|(i, t)| format!("{} {:.2}", SYSTEM_NAMES[*i], t * 1000.0 / ticks)).collect();
        format!("{:.2} ms/tick (slowest {:.1}): {}", sum * 1000.0 / ticks, self.worst_tick.as_secs_f64() * 1000.0, top.join(", "))
    }

    /// Adds `o` into this one (totals, slowest ticks).
    pub fn add(&mut self, o: &SimProfile) {
        for i in 0..SYSTEM_NAMES.len() {
            self.total[i] += o.total[i];
            self.worst[i] = self.worst[i].max(o.worst[i]);
        }
        self.worst_tick = self.worst_tick.max(o.worst_tick);
        self.ticks += o.ticks;
    }
}

fn mark<const I: usize>(p: Option<ResMut<SimProfile>>) {
    let Some(mut p) = p else { return };
    let now = std::time::Instant::now();
    if I == 0 {
        p.ticks += 1;
        p.start = now;
    } else {
        let d = now - p.last;
        p.total[I - 1] += d;
        p.worst[I - 1] = p.worst[I - 1].max(d);
        if I == SYSTEM_NAMES.len() {
            let t = now - p.start;
            p.worst_tick = p.worst_tick.max(t);
        }
    }
    p.last = now;
}

// **Reclaim**: `Order::Reclaim(id)` takes wreck id or id of standing building (ids never collide);
// building gives `wreck_mass_fraction` of its cost in step with hit points and leaves no wreck.
// `ReclaimArea` collects wrecks, debris, trees, rocks, not buildings. Scavengers reclaim
// every reclaimable and revive whole wrecks (`Revive(wreck)` / `ReviveArea`; debris cannot come back). A revived wreck
// becomes construction site with `Revival(wreck id)` component (snapshot `revival`, `Cen::reviving`): only
// scavengers build it (`apply_commands`, `think`'s Assist, stations and `build_progress` keep engineers off it).
// Scavengers take `Assist`: on revival site, or on scavenger/engineer to revive or reclaim what it works on
// (any assister reclaims tree/rock/wreck assisted unit reclaims). On `ReclaimArea` / `ReviveArea` target
// side already works on (or own unfinished revival in circle) counts `systems::TEAMWORK` (12) nearer,
// so group works together; `Revive` whose wreck teammate already revived helps build that site. A revived unit keeps `Revival` when finished; its wreck, or that of revival site killed unfinished,
// cannot be revived for `rules.revive_cooldown` (60 s, `Wreck.revive_at`, `sim::revivable`), so scavengers do not bring same
// unit back over and over where it keeps dying (`ReclaimArea` still takes it). While a
// reclaim or revive order is being given, HUD shows what circle (or thing under pointer) yields or costs.
/// How much farther (world units) a worker on an area reclaim or revive order goes to help a
/// teammate with its target than to the nearest one of its own.
const TEAMWORK: f32 = 12.0;

fn begin_tick(
    mut game: ResMut<Game>,
    mut nav: ResMut<Nav>,
    mut fx: ResMut<FxLog>,
    mut effects: ResMut<Effects>,
    map: Res<MapData>,
    defs: Res<Defs>,
    structures: Query<(&Unit, &Body), With<Structure>>,
    wrecks: Query<&Wreck>,
) {
    game.tick += 1;
    fx.tick.clear();
    effects.sites.clear();
    effects.revived.clear();
    effects.loaded.clear();
    nav.paths_left = 30;
    if nav.dirty || nav.occ.len() != map.blocked.len() {
        nav.occ = map.blocked.clone();
        for (u, b) in &structures {
            nav.stamp_def(&map, defs.get(u.def), b.pos);
        }
        nav.dirty = false;
        nav.rubble.clear();
    }
    let cells = if game.rubble_blocks { rubble_cells(&defs, &map, &wrecks) } else { vec![] };
    if cells != nav.rubble {
        let nav = &mut *nav;
        for &c in &nav.rubble {
            if nav.occ[c as usize] == crate::map::OCC_RUBBLE {
                nav.occ[c as usize] = 0;
            }
        }
        nav.rubble.clear();
        for c in cells {
            // Only on open ground: buildings, steep and deep cells keep their own value.
            if nav.occ[c as usize] == 0 {
                nav.occ[c as usize] = crate::map::OCC_RUBBLE;
                nav.rubble.push(c);
            }
        }
    }
}

// **Rubble blocks paths** (lobby option, `Game.rubble_blocks`; ticked by default from `rules.rubble_blocks_paths`,
// on in built-in data, off in Relaxed; lobby resets it when data preset changes): `begin_tick` marks the
// free cells under unit wrecks (cells their `Wreck::shape` radius covers, at least their own), building ruins
// (footprint) and debris pieces of at least `debris.sight_size` as `map::OCC_RUBBLE` in `Nav.occ` (`rubble_cells`,
// `Nav.rubble` remembers them; derived, not state), so land units path round them (infantry too) until reclaimed or
// shoved away. `can_place` treats them as free: buildings may go on rubble. `--combattest` "Rubble blocks paths" line.
/// The cells rubble lies on (sorted, each once): the footprint of a building's ruin, the cells a
/// unit's wreck covers (at least its own), the cell of a piece of debris big enough to block sight.
fn rubble_cells(defs: &Defs, map: &MapData, wrecks: &Query<&Wreck>) -> Vec<u32> {
    let mut out: Vec<u32> = vec![];
    for w in wrecks {
        if w.falling || w.mass <= 0.0 || (w.piece > 0.0 && w.piece < defs.rules.debris.sight_size) {
            continue;
        }
        let d = defs.get(w.def);
        if w.piece <= 0.0 && d.is_structure() && d.footprint > 0 {
            let (x0, y0) = foot_origin(w.pos, d.footprint);
            for y in y0..y0 + d.footprint {
                for x in x0..x0 + d.footprint {
                    if map.inside(x, y) {
                        out.push(map.idx(x, y) as u32);
                    }
                }
            }
            continue;
        }
        let (r, _) = Wreck::shape(defs, w.def, w.piece);
        let (cx, cy) = map.cell_of(w.pos);
        let k = (r / CELL).ceil() as i32;
        for y in cy - k..=cy + k {
            for x in cx - k..=cx + k {
                let centre = v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL);
                if map.inside(x, y) && ((x, y) == (cx, cy) || centre.dist(w.pos) < r) {
                    out.push(map.idx(x, y) as u32);
                }
            }
        }
    }
    out.sort_unstable();
    out.dedup();
    out
}

#[allow(clippy::type_complexity)]
fn census_build(
    mut census: ResMut<Census>,
    mut wrecks: ResMut<WreckList>,
    game: Res<Game>,
    defs: Res<Defs>,
    q: Query<(Entity, &Unit, &Body, &Health, &Intel, Has<UnderConstruction>, Option<&Hosted>, Option<&Builder>, Option<&Factory>, Has<Upgrading>, Has<Paused>, Option<&Revival>, Option<&Weapons>, &Orders)>,
    wq: Query<&Wreck>,
) {
    census.clear();
    for (e, u, b, h, i, uc, host, builder, factory, upgrading, paused, rev, weapons, orders) in q.iter().sort_by_key::<&Unit, _>(|u| u.id) {
        let d = defs.get(u.def);
        let (ferry_wait, beacon) = ferry_state(d, &orders.0);
        census.push(Cen {
            e,
            id: u.id,
            def: u.def,
            owner: u.owner,
            team: game.team_of(u.owner),
            pos: b.pos,
            y: b.y,
            yaw: b.yaw,
            vel: b.vel,
            hp: h.0,
            max_hp: d.hp,
            done: !uc,
            hosted: host.is_some(),
            host: host.map_or(0, |h| h.0),
            air: d.is_air(),
            structure: d.is_structure(),
            radius: d.radius(),
            height: d.height(),
            hit_w: d.hit_box()[0] * 0.5,
            hit_h: d.hit_box()[1],
            hit_l: d.hit_box()[2] * 0.5,
            hit_r: d.hit_box()[0].max(d.hit_box()[2]) * 0.5,
            face: V2::from_angle(b.yaw),
            seen: i.seen,
            radar: i.radar,
            work: builder.map_or(0, |b| b.work),
            building: factory.map_or(0, |f| f.building),
            upgrading,
            paused,
            firing: weapons.map_or(false, |w| w.list.iter().any(|x| x.cd > 0.0 || x.burst > 0)),
            revival: if uc { rev.map_or(0, |r| r.0) } else { 0 },
            fieldwork: d.is_fieldwork(),
            ferry_wait,
            beacon,
            engineer: !d.is_structure() && (d.is_engineer() || d.is_scavenger()),
        });
    }
    wrecks.0.clear();
    // Falling debris cannot be reclaimed, hit or hidden behind until it lands.
    wrecks.0.extend(wq.iter().filter(|w| !w.falling).map(|w| (w.id, w.pos, w.mass, w.def, w.piece, w.revive_at)));
    wrecks.0.sort_by_key(|w| w.0);
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn apply_commands(
    mut queue: ResMut<CommandQueue>,
    log: Option<ResMut<TurnLog>>,
    mut census: ResMut<Census>,
    defs: Res<Defs>,
    map: Res<MapData>,
    game: Res<Game>,
    mut effects: ResMut<Effects>,
    nav: Res<Nav>,
    mut commands: Commands,
    mut q: Query<(&Unit, &Body, &mut Orders, Option<&mut Factory>, Option<&mut Mover>, Option<&mut Weapons>, Has<Upgrading>, Has<UnderConstruction>)>,
) {
    if let Some(mut log) = log {
        log.0 = queue.0.clone();
    }
    let touched = !queue.0.is_empty();
    for (pl, c) in queue.0.drain(..) {
        if !game.players.get(pl as usize).map_or(false, |p| p.alive) {
            continue;
        }
        let owned = |id: u32| census.get(id).filter(|c| c.owner == pl).map(|c| c.e);
        match c {
            Command::Orders { ids, orders, queue: append } => {
                if orders.is_empty() {
                    continue;
                }
                for (k, id) in ids.iter().enumerate() {
                    let Some(e) = owned(*id) else { continue };
                    let hosted = census.get(*id).filter(|c| c.hosted && c.done);
                    let Ok((u, b, mut ord, factory, mut mover, _, _, _)) = q.get_mut(e) else { continue };
                    let df = defs.get(u.def);
                    let mut o = if orders.len() == ids.len() { orders[k] } else { orders[0] };
                    // Units riding in a transport take no orders. Infantry in a building take move orders:
                    // they step out of the side facing where they go first.
                    let mut from = b.pos;
                    if let Some(c) = hosted {
                        let (Order::Move(p) | Order::AttackMove(p) | Order::Patrol(p)) = o else { continue };
                        let Some(h) = census.get(c.host).filter(|h| h.structure && defs.get(h.def).is_garrison()) else { continue };
                        let p = map.clamp(p);
                        let riders: Vec<u32> = census.list.iter().filter(|t| t.host == h.id).map(|t| t.id).collect();
                        let k = riders.iter().position(|r| *r == c.id).unwrap_or(0);
                        from = garrison_door(&map, &nav, h.pos, defs.get(h.def).radius(), p, k, riders.len());
                        commands.entity(e).remove::<Hosted>().insert(Body { pos: from, y: map.h(from), yaw: (p - from).angle(), vel: v2(0.0, 0.0) });
                        if let Some(m) = mover.as_mut() {
                            m.anchor = p;
                            m.path_goal = from;
                        }
                    }
                    // Infantry told to attack a building enemy infantry hold storm it, like a right-click
                    // does: shooting at its walls would only wear the building down.
                    // The same for a trench piece enemy infantry hold, or the enemy infantry in it: they
                    // charge it and fight it out at close quarters, where the trench gives no cover.
                    if let Order::Attack(t) = o {
                        let team = game.team_of(pl);
                        if census.get(t).is_some_and(|t| storms(&defs, &census, df, t, team)) {
                            o = Order::Load(t);
                        } else if let Some(piece) = census.get(t).and_then(|t| trench_to_storm(&defs, &census, df, t, team)) {
                            o = Order::Load(piece);
                        }
                    }
                    match &mut o {
                        Order::Move(p) | Order::AttackMove(p) | Order::Patrol(p) => *p = map.clamp(*p),
                        Order::AttackGround { pos, .. } | Order::ReclaimArea { pos, .. } => *pos = map.clamp(*pos),
                        Order::ReviveArea { pos, radius } => {
                            if !df.is_scavenger() || !radius.is_finite() {
                                continue;
                            }
                            *pos = map.clamp(*pos);
                        }
                        Order::Terraform { pos, radius, height } => {
                            if !df.is_engineer() || !defs.rules.terraforming || !radius.is_finite() || !height.is_finite() {
                                continue;
                            }
                            *pos = map.clamp(*pos);
                            *radius = radius.clamp(1.0, 24.0);
                        }
                        Order::LoadArea { pos, radius } | Order::Unload { pos, radius, .. } => {
                            if df.transport.is_none() || !radius.is_finite() {
                                continue;
                            }
                            *pos = map.clamp(*pos);
                            *radius = radius.clamp(0.0, 60.0);
                        }
                        Order::Load(_) if df.transport.is_none() && !carriable(df) => continue,
                        Order::Ferry { pos, pick } => {
                            if !(ferries(df) || (*pick && carriable(df))) {
                                continue;
                            }
                            *pos = map.clamp(*pos);
                        }
                        Order::Build { def: bd, pos } => {
                            if !df.builds.contains(bd) || !df.is_engineer() {
                                continue;
                            }
                            *pos = snap(&defs, &map, *bd, *pos);
                        }
                        Order::Capture(_) if !df.is_engineer() && !(game.infantry_capture && df.is_infantry()) => continue,
                        Order::Revive(_) if !df.is_scavenger() => continue,
                        Order::Attack(_) if df.is_scavenger() => continue,
                        // Only scavengers build what is being revived.
                        Order::Assist(t) if !df.is_scavenger() && census.get(*t).map_or(false, |t| t.reviving()) => continue,
                        _ => {}
                    }
                    if let Some(mut f) = factory {
                        if let Order::Move(p) | Order::AttackMove(p) | Order::Patrol(p) = o {
                            f.rally = Some(p);
                        }
                        continue;
                    }
                    if !append {
                        ord.0.clear();
                        if let Some(mut m) = mover {
                            m.path.clear();
                            m.stuck = 0;
                            m.cover = None;
                            m.pace = 0.0;
                            m.pace_accel = 0.0;
                            // A new order cuts a weapon's set-up/recoil hold short: the unit obeys at once.
                            m.hold = 0.0;
                        }
                    }
                    if ord.0.len() >= 96 {
                        continue;
                    }
                    if let Order::Patrol(_) = o {
                        let has = ord.0.iter().any(|x| matches!(x, Order::Patrol(_)));
                        ord.0.push(o);
                        if !has {
                            ord.0.push(Order::Patrol(from));
                        }
                    } else if let (Order::Ferry { pick: false, .. }, true) = (o, ferries(df)) {
                        // A further point of a ferry route goes in after its drop point, which is the
                        // entry before the beacon on the loop; a route without a beacon gets one where
                        // the transport is.
                        match ord.0.iter().position(|x| matches!(x, Order::Ferry { pick: true, .. })) {
                            Some(0) => ord.0.push(o),
                            Some(i) => ord.0.insert(i, o),
                            None => {
                                ord.0.push(Order::Ferry { pos: from, pick: true });
                                ord.0.push(o);
                            }
                        }
                    } else {
                        ord.0.push(o);
                    }
                }
            }
            Command::Stop { ids } => {
                for id in ids {
                    let Some(e) = owned(id) else { continue };
                    let Ok((_, b, mut ord, factory, mover, weapons, upgrading, _)) = q.get_mut(e) else { continue };
                    ord.0.clear();
                    if let Some(mut m) = mover {
                        m.path.clear();
                        m.pace = 0.0;
                        m.pace_accel = 0.0;
                        // Infantry look for cover around where they were stopped.
                        m.anchor = b.pos;
                        m.cover = None;
                    }
                    if let Some(mut w) = weapons {
                        w.focus = 0;
                        w.ground = None;
                    }
                    if upgrading {
                        commands.entity(e).remove::<Upgrading>();
                    }
                    if let Some(mut f) = factory {
                        f.queue.clear();
                        if let Some(p) = census.get(f.building) {
                            commands.entity(p.e).despawn();
                        }
                        f.building = 0;
                        // A transport being filled leaves with the riders it already has.
                        f.cargo.clear();
                    }
                }
            }
            Command::Produce { factory, def: d, count } => {
                let Some(e) = owned(factory) else { continue };
                let Ok((u, _, _, Some(mut f), _, _, _, _)) = q.get_mut(e) else { continue };
                if !defs.get(u.def).builds.contains(&d) {
                    continue;
                }
                if count > 0 {
                    for _ in 0..count.min(50) {
                        if f.queue.len() < 100 {
                            f.queue.push(Job::plain(d));
                        }
                    }
                } else {
                    for _ in 0..(-count).min(50) {
                        let Some(k) = f.queue.iter().rposition(|x| x.def == d) else { break };
                        if k == 0 && f.front_in_works() {
                            if let Some(p) = census.get(f.building) {
                                commands.entity(p.e).despawn();
                            }
                            f.building = 0;
                        }
                        f.queue.remove(k);
                    }
                }
            }
            Command::ProduceLoaded { factory, def: d, cargo, count } => {
                let Some(e) = owned(factory) else { continue };
                let Ok((u, _, _, Some(mut f), _, _, _, _)) = q.get_mut(e) else { continue };
                if !loadout_ok(&defs, u.def, d, &cargo) {
                    continue;
                }
                for _ in 0..count.clamp(0, 50) {
                    if f.queue.len() < 100 {
                        f.queue.push(Job { def: d, cargo: cargo.clone() });
                    }
                }
            }
            Command::QueueMove { factory, from, len, to } => {
                let Some(e) = owned(factory) else { continue };
                let Ok((_, _, _, Some(mut f), _, _, _, _)) = q.get_mut(e) else { continue };
                let (from, len) = (from as usize, len as usize);
                if len == 0 || from + len > f.queue.len() {
                    continue;
                }
                let first = f.queue.first().cloned();
                let run: Vec<Job> = f.queue.drain(from..from + len).collect();
                let to = (to as usize).min(f.queue.len());
                for (k, d) in run.into_iter().enumerate() {
                    f.queue.insert(to + k, d);
                }
                // Something else is at the front now: the unit in the works is scrapped.
                if f.queue.first() != first.as_ref() && f.front_in_works() {
                    if let Some(p) = census.get(f.building) {
                        commands.entity(p.e).despawn();
                    }
                    f.building = 0;
                }
            }
            Command::Dequeue { factory, index } => {
                let Some(e) = owned(factory) else { continue };
                let Ok((_, _, _, Some(mut f), _, _, _, _)) = q.get_mut(e) else { continue };
                let index = index as usize;
                if index >= f.queue.len() {
                    continue;
                }
                if index == 0 && f.front_in_works() {
                    if let Some(p) = census.get(f.building) {
                        commands.entity(p.e).despawn();
                    }
                    f.building = 0;
                }
                f.queue.remove(index);
            }
            Command::Repeat { ids, on } => {
                for id in ids {
                    if let Some(e) = owned(id) {
                        if let Ok((_, _, _, Some(mut f), _, _, _, _)) = q.get_mut(e) {
                            f.repeat = on;
                        }
                    }
                }
            }
            Command::Upgrade { ids } => {
                for id in ids {
                    if let Some(e) = owned(id) {
                        if let Ok((u, _, _, _, _, _, upgrading, uc)) = q.get_mut(e) {
                            if defs.get(u.def).upgrade_id.is_some() && !upgrading && !uc {
                                commands.entity(e).insert(Upgrading(0.0));
                            }
                        }
                    }
                }
            }
            Command::Pause { ids, on } => {
                for id in ids {
                    if let Some(e) = owned(id) {
                        if on {
                            commands.entity(e).insert(Paused);
                        } else {
                            commands.entity(e).remove::<Paused>();
                        }
                    }
                }
            }
            Command::SelfDestruct { ids } => {
                for id in ids {
                    if owned(id).is_some() {
                        effects.damage.push((id, 1.0e9, 0));
                    }
                }
            }
            Command::Pace { ids } => {
                // Ids are owned, mobile, out of transports and no planes (they cannot slow down).
                let group: Vec<(Entity, f32, f32)> = ids
                    .iter()
                    .filter_map(|id| census.get(*id).filter(|c| c.owner == pl && !c.hosted))
                    .filter_map(|c| {
                        let mv = &defs.get(c.def).mv;
                        (matches!(mv.kind, MoveKind::Land | MoveKind::Hover) && mv.speed > 0.0).then_some((c.e, mv.speed, mv.accel))
                    })
                    .collect();
                if group.len() < 2 {
                    continue;
                }
                let slow = group.iter().map(|g| g.1).fold(f32::MAX, f32::min);
                let accel = group.iter().map(|g| g.2).fold(f32::MAX, f32::min);
                for (e, ..) in group {
                    if let Ok((_, _, _, _, Some(mut m), _, _, _)) = q.get_mut(e) {
                        m.pace = slow;
                        m.pace_accel = accel;
                    }
                }
            }
            Command::Resign => {
                let com = defs.factions[game.players[pl as usize].faction as usize].commander;
                for c in &census.list {
                    if c.owner == pl && Some(c.def) == com {
                        effects.damage.push((c.id, 1.0e9, 0));
                    }
                }
            }
        }
    }
    // Ferry routes and waiting units as the new orders have them: a unit sent to a beacon in the
    // same tick as its transport must not find it unserved and give up.
    if touched {
        for i in 0..census.list.len() {
            if let Ok((u, _, ord, ..)) = q.get(census.list[i].e) {
                let (wait, beacon) = ferry_state(defs.get(u.def), &ord.0);
                census.list[i].ferry_wait = wait;
                census.list[i].beacon = beacon;
            }
        }
    }
}

/// A unit's ferry fields in the census (`Cen::ferry_wait`, `Cen::beacon`) from its orders.
fn ferry_state(d: &crate::data::UnitDef, orders: &[Order]) -> (Option<V2>, Option<V2>) {
    if ferries(d) {
        (None, orders.iter().find_map(|o| if let Order::Ferry { pos, pick: true } = o { Some(*pos) } else { None }))
    } else if let Some(Order::Ferry { pos, .. }) = orders.first() {
        (Some(*pos), None)
    } else {
        (None, None)
    }
}

// **Counter-battery radar** (`rules.counter_battery`): in `visibility`, artillery shell in flight (`PROJ_SHELL` whose
// shooter has `arc`/`high_arc` land weapon) within hostile team's level-1 radar range (no line of sight: shells fly
// high) sets shooter's `radar` bit for that team that tick, so it shows as radar contact while its shells fly and
// long guns (and AI) can answer. No new state: worked out from projectiles each tick. `--combattest` infantry
// "Counter-battery" line. Answer to mortar spam: Pikes out-range tanks (realistic: 120 mm mortar reaches
// 7 km), but every volley gives them away.
// **Spotting delay** (unit JSON `spot_time` / `spot_time_moving`, seconds; 0 = seen at once, default: tanks,
// buildings; three troopers: 5 / 0.5): unit with either is only `seen` by team once that team has had it in
// view (range + line of sight) that long. `Intel.spot[team]` (snapshotted) counts up to `sim::SPOTTED`
// in `visibility`: at moving rate while it moves or weapon is reloading (`Cen::firing`), at hiding rate
// while still; out of view it drains over `rules.spot_forget` (3 s). Within `rules.near_sight` seen at
// once. Once spotted it stays seen while in view. `--combattest` "Spotting" line (with `LR_TEST=binoculars`).
// **Visibility cost** (`systems::visibility`, costliest tick system on town maps): viewers found through grid
// (`VIEW_CELL`/`VIEW_NEAR`, longer-reaching ones checked by every unit); radar rays between two ends that both stand still
// are kept in `RadarLosCache` (exact ends + `Ground.hash` must match, two generations swapped every 300 ticks; not state:
// hit is what `map.los` would say); `map.los_top` skips ray samples above `Ground.top` (highest vertex per 8x8-cell
// block, only ever raised by `Ground::set`); who sees neutral building is redone every `NEUTRAL_EVERY` (5) ticks,
// staggered by id. `Census` index/grid use bevy's (foldhash) `HashMap`: lookups only, never iterated.
/// Who sees whom. Both sight and radar need a clear line over the terrain; radar looks from higher
/// up and reaches further, sight tells what the contact is.
#[allow(clippy::too_many_arguments)]
#[allow(clippy::too_many_arguments)]
fn visibility(mut census: ResMut<Census>, game: Res<Game>, defs: Res<Defs>, map: Res<MapData>, features: Res<Features>, wrecks: Res<WreckList>, shells: Query<&Projectile>, mut q: Query<&mut Intel>, (mut los_cache, ground): (ResMut<RadarLosCache>, Option<Res<Ground>>)) {
    // Trees, rocks, wrecks and buildings block sight like hills do (not radar); the rules can switch each off.
    let standing = census.list.iter().filter(|c| c.structure && c.done && !c.hosted).map(|c| (c.id, c.def, c.pos));
    let block = sight_block(&features, &wrecks, standing, &defs, &map);
    let (hills_sight, hills_radar) = (defs.rules.sight_blocked_by_terrain, defs.rules.radar_blocked_by_terrain);
    let near2 = defs.rules.near_sight * defs.rules.near_sight;
    // Weather shortens vision, binoculars and radar, and makes units slower to spot.
    let wx = defs.rules.weather_of(game.weather);
    struct Viewer {
        pos: V2,
        eye: Vec3,
        look: Vec3,
        vis2: f32,
        /// Squared radar range against stealth levels 0, 1 and 2.
        radar2: [f32; 3],
        team: u8,
        /// Binoculars: squared range (0 = none), direction, squared cosine of half the cone.
        bin2: f32,
        dir: V2,
        cos2: f32,
        /// The building it looks out of (itself, or the garrison it sits in): it does not block its view.
        from: u32,
        id: u32,
        /// Standing still: its radar lines of sight are worth keeping (`RadarLosCache`).
        still: bool,
        /// Squared distance beyond which it neither sees nor finds anything.
        reach2: f32,
    }
    let mut viewers = vec![];
    for c in &census.list {
        if !c.done || c.team == TEAM_NEUTRAL {
            continue;
        }
        // Infantry inside a building look out from its roof, and further; nobody sees out of a transport.
        let roof = c.hosted;
        let top = if c.hosted {
            match census.get(c.host).filter(|h| defs.get(h.def).is_garrison()) {
                Some(h) => h.height,
                None => continue,
            }
        } else {
            c.height.max(1.0)
        };
        let d = defs.get(c.def);
        let gain = if roof { census.get(c.host).map_or(1.0, |h| defs.get(h.def).roof_vision(&defs.rules)) } else { 1.0 };
        let vision = d.vision * gain * wx.vision;
        let (mut bin2, mut dir, mut cos2) = (0.0, v2(1.0, 0.0), 1.0);
        if let Some(bd) = d.binoculars.as_ref().filter(|b| b.range > 0.0) {
            let range = bd.range * gain * wx.binoculars;
            let (look, lock) = q.get(c.e).map_or((c.yaw, 0), |i| (i.look, i.lock));
            let (look, lock) = aim_binoculars(&census, c, bd, range, look, lock, roof, game.tick);
            if let Ok(mut i) = q.get_mut(c.e) {
                i.look = look;
                i.lock = lock;
            }
            let half = fm::cos(bd.angle.clamp(1.0, 170.0).to_radians() * 0.5);
            (bin2, dir, cos2) = (range * range, V2::from_angle(look), half * half);
        }
        let power_out = game.players.get(c.owner as usize).map_or(false, |p| p.power_out);
        let radar_on = !(power_out || c.paused) && d.has_radar();
        if vision <= 0.0 && bin2 <= 0.0 && !radar_on {
            continue;
        }
        viewers.push(Viewer {
            pos: c.pos,
            eye: c.pos.at(c.y + top + defs.rules.radar_los_height),
            look: c.pos.at(c.y + top + defs.rules.vision_los_height),
            vis2: vision * vision,
            radar2: if radar_on { [0u8, 1, 2].map(|s| {
                let r = d.radar_vs(s) * wx.radar;
                r * r
            }) } else { [0.0; 3] },
            team: c.team,
            bin2,
            dir,
            cos2,
            from: if c.hosted { c.host } else { c.id },
            id: c.id,
            still: c.vel.len2() < 1e-8,
            reach2: 0.0,
        });
    }
    // Counter-battery radar: an artillery shell in flight inside a team's radar range gives away
    // the gun that fired it (the shell flies high: no line of sight needed). Bit masks are or-ed,
    // so the order of the shells does not matter.
    let mut traced: std::collections::BTreeMap<u32, u16> = Default::default();
    if defs.rules.counter_battery {
        for p in &shells {
            if p.kind != PROJ_SHELL || p.shooter == 0 {
                continue;
            }
            let Some(gun) = census.get(p.shooter) else { continue };
            if !defs.get(gun.def).weapons.iter().any(|w| w.hits_land && matches!(w.kind, WeaponKind::Arc | WeaponKind::HighArc)) {
                continue;
            }
            let at = v2(p.pos.x, p.pos.z);
            let mut mask = 0u16;
            for v in viewers.iter().filter(|v| v.radar2[0] > 0.0 && hostile(v.team, gun.team)) {
                if (v.pos - at).len2() < v.radar2[0] {
                    mask |= 1 << v.team;
                }
            }
            if mask != 0 {
                *traced.entry(p.shooter).or_default() |= mask;
            }
        }
    }
    // Viewers that reach no further than `VIEW_NEAR` are found through a grid of `VIEW_CELL`
    // cells around each unit, the others are looked at by every unit. The order viewers are looked
    // at in only decides which checks are skipped once a team's bits are set, never the result.
    for v in viewers.iter_mut() {
        v.reach2 = v.vis2.max(v.bin2).max(v.radar2[0]).max(v.radar2[1]).max(v.radar2[2]);
    }
    let side = ((map.world_x() / VIEW_CELL).ceil() as i32 + 1).max(1);
    let side_y = ((map.world_z() / VIEW_CELL).ceil() as i32 + 1).max(1);
    let cell = |p: V2| (((p.x / VIEW_CELL).floor() as i32).clamp(0, side - 1), ((p.y / VIEW_CELL).floor() as i32).clamp(0, side_y - 1));
    let mut grid: Vec<Vec<u32>> = vec![vec![]; (side * side_y) as usize];
    let mut far: Vec<u32> = vec![];
    for (k, v) in viewers.iter().enumerate() {
        if v.reach2 <= VIEW_NEAR * VIEW_NEAR {
            let (x, y) = cell(v.pos);
            grid[(y * side + x) as usize].push(k as u32);
        } else {
            far.push(k as u32);
        }
    }
    let span = (VIEW_NEAR / VIEW_CELL).ceil() as i32;
    // Radar sight lines between two ends that stay put are kept (see `RadarLosCache`). Every
    // `RadarLosCache::KEEP` ticks the current generation becomes the old one and the old one is
    // dropped: pairs still looked at move on, the others go, and no tick redoes them all at once.
    if game.tick % RadarLosCache::KEEP == 0 {
        los_cache.old = std::mem::take(&mut los_cache.cur);
    }
    let top: &[f32] = ground.as_ref().map_or(&[], |g| &g.top[..]);
    let ground = ground.as_ref().map_or(0, |g| g.hash);
    let (cache, old) = (&los_cache.cur, &los_cache.old);
    // Who sees each unit depends only on the viewers and the terrain, so the units are worked out
    // in parallel (the sight lines are most of the tick) and written back in order.
    let map = &*map;
    let found = par_map(&census.list, 32, |c| {
        let mut fresh: Vec<((u32, u32), RadarLos)> = vec![];
        // Neutral buildings (towns: thousands of them) never move, shoot or get auto-targeted: who
        // sees them is worked out every `NEUTRAL_EVERY` ticks, spread over the ticks by id.
        if c.team == TEAM_NEUTRAL && c.structure && (game.tick + c.id) % NEUTRAL_EVERY != 0 {
            return (c.seen, c.radar, 0, fresh);
        }
        let own = 1u16 << c.team;
        let (mut seen, mut radar, mut near) = (own, own, own);
        let head = c.pos.at(c.y + c.height + 0.5);
        // Stealth level 3 never shows on radar.
        let d = defs.get(c.def);
        let stealth = d.stealth.min(3) as usize;
        // Units that take time to spot: `seen` is "in view" here, `near` those that see it at once.
        let slow = d.spot_time > 0.0 || d.spot_time_moving > 0.0;
        let (cx, cy) = cell(c.pos);
        let rows = (cy - span).max(0)..=(cy + span).min(side_y - 1);
        let nearby = rows.flat_map(|y| ((cx - span).max(0)..=(cx + span).min(side - 1)).map(move |x| (y * side + x) as usize));
        for k in nearby.flat_map(|i| grid[i].iter()).chain(far.iter()) {
            let v = &viewers[*k as usize];
            let bit = 1u16 << v.team;
            if seen & bit != 0 && radar & bit != 0 && (!slow || near & bit != 0) {
                continue;
            }
            let d2 = (v.pos - c.pos).len2();
            // Anything this close is seen regardless (no hiding behind the viewer's own bump).
            // Within vision all round, or further inside the binoculars' cone.
            let in_view = d2 < v.vis2
                || (d2 < v.bin2 && {
                    let along = (c.pos - v.pos).dot(v.dir);
                    along > 0.0 && along * along >= v.cos2 * d2
                });
            if in_view && d2 < near2 {
                seen |= bit;
                near |= bit;
            } else if seen & bit == 0 && in_view && map.sight_line(v.look, head, &block, hills_sight, [v.from, c.id]) {
                seen |= bit;
            }
            if radar & bit == 0 && stealth < 3 && d2 < v.radar2[stealth] && (!hills_radar || {
                // Only pairs that both stand still are ever kept.
                if v.still && c.vel.len2() < 1e-8 {
                    let key = (v.id, c.id);
                    let ok = |e: &&RadarLos| e.eye == v.eye && e.head == head && e.ground == ground;
                    match cache.get(&key).filter(ok) {
                        Some(e) => e.clear,
                        None if let Some(e) = old.get(&key).filter(ok) => {
                            fresh.push((key, RadarLos { eye: v.eye, head, ground, clear: e.clear }));
                            e.clear
                        }
                        None => {
                            let clear = map.los_top(v.eye, head, top);
                            fresh.push((key, RadarLos { eye: v.eye, head, ground, clear }));
                            clear
                        }
                    }
                } else {
                    map.los_top(v.eye, head, top)
                }
            }) {
                radar |= bit;
            }
        }
        (seen, radar, near, fresh)
    });
    let step = |secs: f32| if secs > 0.0 { ((SPOTTED as f32 * DT / secs) as u32).clamp(1, SPOTTED as u32) as u16 } else { SPOTTED };
    let forget = step(defs.rules.spot_forget);
    for (c, (mut seen, mut radar, near, fresh)) in census.list.iter_mut().zip(found) {
        los_cache.cur.extend(fresh);
        let Ok(mut i) = q.get_mut(c.e) else { continue };
        radar |= traced.get(&c.id).copied().unwrap_or(0);
        // Spotting delay: a team that has the unit in view gets on with making it out, quickly
        // while it moves or fires, slowly while it keeps still; out of view the progress drains.
        let d = defs.get(c.def);
        if d.spot_time > 0.0 || d.spot_time_moving > 0.0 {
            let exposed = c.firing || c.vel.len2() > 0.04;
            let gain = step(if exposed { d.spot_time_moving } else { d.spot_time } * wx.spotting);
            for t in 0..16 {
                let bit = 1u16 << t;
                if t == c.team as usize {
                    continue;
                }
                let s = &mut i.spot[t];
                *s = if near & bit != 0 {
                    SPOTTED
                } else if seen & bit != 0 {
                    s.saturating_add(gain)
                } else {
                    s.saturating_sub(forget)
                };
                if *s < SPOTTED {
                    seen &= !bit;
                }
            }
        }
        c.seen = seen;
        c.radar = radar;
        i.seen = seen;
        i.radar = radar;
    }
}

/// Ticks between two looks at who sees a neutral building (`visibility`).
const NEUTRAL_EVERY: u32 = 5;

/// Grid cell (world units) and reach of the viewers `visibility` finds by position.
const VIEW_CELL: f32 = 48.0;
const VIEW_NEAR: f32 = 150.0;

/// A radar line of sight over the terrain between `eye` and `head` on the ground of fingerprint `ground`.
pub struct RadarLos {
    eye: Vec3,
    head: Vec3,
    ground: u64,
    clear: bool,
}

/// Radar lines of sight of viewer/target pairs that both stand still (radars, buildings, parked
/// units): `map.los` gives the same answer for the same ends over the same ground, so it is asked
/// once (on town maps most rays went to neutral buildings, the same every tick). A hit needs the
/// exact same ends and terrain fingerprint, so the cache never changes a result: it is not part of
/// the state, and rollbacks, joins and replays need not know of it. Only looked up by key.
#[derive(Resource, Default)]
pub struct RadarLosCache {
    cur: bevy::platform::collections::HashMap<(u32, u32), RadarLos>,
    old: bevy::platform::collections::HashMap<(u32, u32), RadarLos>,
}

impl RadarLosCache {
    pub const KEEP: u32 = 300;
}

// **Binoculars** (`binoculars: {range, angle, turn, scan}` in unit JSON; Trooper and Sniper Trooper: 140, 20 deg):
// narrow view cone on top of round `vision`, same line-of-sight rules. `systems::aim_binoculars` (inside
// `visibility`) keeps direction and target in `Intel.look` / `Intel.lock` (snapshotted): follows the
// locked enemy while in range and known to team, else takes best known mobile enemy in range
// (radar-only contacts first, ground before air, nearest), else scans to and fro +-`scan` around body
// facing at half turn rate (all round from garrison roof). Fog view: `FogGrid.cone` (`covered_cone`);
// HUD draws cone of selected units (brighter while locked). `--combattest` checks scan, lock, tracking
// (alone: `LR_TEST=binoculars`).
/// Where a unit's binoculars look this tick, and what they follow. They keep following their
/// enemy while it stays in range and known to the team; else they take the best known enemy in
/// range (radar contacts nobody has identified first, ground before air, then the nearest);
/// with nothing to look at they scan to and fro around the way the unit faces, all round from a
/// building's roof. Buildings are not followed (they stay remembered once seen).
#[allow(clippy::too_many_arguments)]
fn aim_binoculars(census: &Census, c: &Cen, b: &crate::data::BinocularsDef, range: f32, look: f32, lock: u32, roof: bool, tick: u32) -> (f32, u32) {
    let r2 = range * range;
    let ok = |t: &Cen| t.hp > 0.0 && !t.hosted && !t.structure && t.done && hostile(c.team, t.team) && t.known_to(c.team) && (t.pos - c.pos).len2() <= r2;
    let mut target = census.get(lock).filter(|t| ok(t));
    if target.is_none() {
        let bit = 1u16 << c.team;
        let mut best: Option<(bool, bool, f32, u32)> = None;
        census.near(c.pos, range, |t| {
            if ok(t) {
                let key = (t.seen & bit != 0, t.air, (t.pos - c.pos).len2(), t.id);
                if best.map_or(true, |k| key < k) {
                    best = Some(key);
                }
            }
        });
        target = best.and_then(|k| census.get(k.3));
    }
    let step = b.turn.to_radians() * DT;
    if let Some(t) = target {
        return (turn_toward(look, (t.pos - c.pos).angle(), step), t.id);
    }
    let sweep = step * 0.5;
    if roof || b.scan >= 180.0 {
        return (wrap(look + if c.id % 2 == 0 { sweep } else { -sweep }), 0);
    }
    // A triangle wave between -scan and +scan at the sweep speed, a different phase per unit.
    let scan = b.scan.max(0.0).to_radians();
    let period = ((4.0 * scan / sweep.max(1e-4)).ceil() as u32).max(1);
    let p = ((tick + c.id.wrapping_mul(7)) % period) as f32 / period as f32;
    let tri = if p < 0.5 { 4.0 * p - 1.0 } else { 3.0 - 4.0 * p };
    (turn_toward(look, wrap(c.yaw + scan * tri), step), 0)
}

fn income(census: Res<Census>, mut game: ResMut<Game>, defs: Res<Defs>) {
    for p in game.players.iter_mut() {
        p.max_mass = 0.0;
        p.max_energy = 0.0;
        p.income_m = 0.0;
        p.income_e = 0.0;
        p.spend_m = 0.0;
        p.spend_e = 0.0;
        p.upkeep = 0.0;
    }
    let stall = defs.rules.power_stall_pump_factor;
    for c in &census.list {
        if !c.done || c.hosted {
            continue;
        }
        let Some(p) = game.players.get_mut(c.owner as usize) else { continue };
        let e = &defs.get(c.def).eco;
        p.max_mass += e.store_mass;
        p.max_energy += e.store_energy;
        if c.paused {
            continue;
        }
        // Extractors slow down with the share of energy they actually got last tick.
        p.income_m += e.mass * if e.upkeep > 0.0 { p.power.max(stall) } else { 1.0 };
        p.income_e += e.energy;
        p.upkeep += e.upkeep;
    }
    // Upkeep is not taken here: it competes with construction for energy in `build_progress`,
    // so a stalled economy slows everything down proportionally instead of locking up.
    for p in game.players.iter_mut() {
        p.mass += p.income_m * DT;
        p.energy += p.income_e * DT;
    }
}

/// Turns the front order of every unit into a movement goal, a work target and a weapon focus.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn think(
    mut q: Query<(&Unit, &Body, &mut Orders, Option<&mut Mover>, Option<&mut Builder>, Option<&mut Weapons>), (Without<UnderConstruction>, Without<Hosted>)>,
    census: Res<Census>,
    wrecks: Res<WreckList>,
    defs: Res<Defs>,
    map: Res<MapData>,
    mut nav: ResMut<Nav>,
    mut game: ResMut<Game>,
    mut effects: ResMut<Effects>,
    terrain: Res<Ground>,
    mut features: ResMut<Features>,
    mut commands: Commands,
) {
    // Free cargo slots of a transport, counting what was loaded earlier in this tick.
    let room = |effects: &Effects, tid: u32, cap: u8| -> i32 {
        let aboard: i32 = census.list.iter().filter(|c| c.host == tid).map(|c| defs.get(c.def).cargo as i32).sum();
        let coming: i32 = effects.loaded.iter().filter(|l| l.1 == tid).filter_map(|l| census.get(l.0)).map(|c| defs.get(c.def).cargo as i32).sum();
        cap as i32 - aboard - coming
    };
    for (u, body, mut orders, mut mover, mut builder, mut weapons) in q.iter_mut().sort_by_key::<&Unit, _>(|u| u.id) {
        let df = defs.get(u.def);
        if df.is_factory() {
            continue;
        }
        // Taken aboard a transport earlier in this tick.
        if effects.loaded.iter().any(|l| l.0 == u.id) {
            continue;
        }
        // Still falling from an airdrop: orders wait for the landing.
        if mover.as_ref().is_some_and(|m| m.drop > 0.0) {
            continue;
        }
        let (pos, id, owner) = (body.pos, u.id, u.owner);
        let team = game.team_of(owner);
        let is_struct = df.is_structure();
        let plane = df.is_plane();
        let arrive = if plane { 9.0 } else { 0.8 };
        // A transport plane cannot stop to load: it scoops up what it flies close over.
        let scoop = if plane { PLANE_SCOOP } else { 0.0 };
        let mut goal: Option<V2> = None;
        let mut pop = false;
        // Popped orders that go round again (ferry routes; patrols always do), and an order to do first.
        let mut rotate = false;
        let mut push_front: Option<Order> = None;
        let mut work = 0u32;
        let mut new_site: Option<V2> = None;
        let mut focus = 0u32;
        let mut ground: Option<(V2, f32)> = None;
        let w0 = df.weapons.first();
        // What the unit's guns can hit between them (a fighter-bomber has one of each).
        let (any_land, any_air) = (df.weapons.iter().any(|w| w.hits_land), df.weapons.iter().any(|w| w.hits_air));
        let air_range = df.weapons.iter().filter(|w| w.hits_air).map(|w| w.range).fold(0.0, f32::max);
        let reach = |target_r: f32| df.build_range().max(2.0) + target_r + df.radius();
        // Every gun with a target has the ground, a building or a friend in its line of fire
        // (direct fire only, as last tick's `weapons` found): standing here shoots nothing, so
        // the unit closes in instead.
        let no_shot = |tid: Option<u32>| {
            weapons.as_ref().is_some_and(|ws| {
                let aimed = ws.list.iter().filter(|s| s.target != 0 && tid.map_or(true, |t| s.target == t));
                aimed.clone().next().is_some() && aimed.into_iter().all(|s| s.blocked)
            })
        };
        // Infantry: a spot behind a tree, rock or wreck near `anchor`, re-chosen every half second.
        let infantry = df.is_infantry() && !is_struct && !plane;
        // No shot from here (see `no_shot`): a spot nearby with a clear line to the targets, to
        // move to and aim again from. Kept in `Mover.cover` (infantry use that for real cover),
        // re-searched every second.
        let reposition = |mover: &mut Option<Mut<Mover>>| -> Option<V2> {
            if is_struct || plane || infantry || !no_shot(None) {
                return None;
            }
            let ws = weapons.as_deref()?;
            let m = mover.as_mut()?;
            let keep = m.cover.filter(|c| c.dist(pos) > 0.5 && c.dist(pos) < 30.0);
            if keep.is_none() || (game.tick + id) % 10 == 0 {
                m.cover = firing_spot(&defs, &census, &features, &wrecks, &map, &nav, id, team, df, pos, ws, keep, game.weather);
            }
            m.cover.filter(|c| c.dist(pos) > 0.5)
        };
        let mut in_cover = false;
        let cover = |mover: &mut Option<Mut<Mover>>, anchor: Option<V2>, search: f32| -> Option<V2> {
            let m = mover.as_mut()?;
            let anchor = anchor.unwrap_or(m.anchor);
            if (game.tick + id) % 5 == 0 || m.cover.map_or(false, |c| c.dist(anchor) > search + 1.0) {
                m.cover = cover_spot(&defs, &census, &features, &wrecks, &map, &nav, id, team, df, pos, anchor, search);
            }
            m.cover.filter(|c| c.dist(pos) > 0.3)
        };

        match orders.0.first().copied() {
            None => {
                // Idle armed planes fight what is around them and follow it (SupCom style): they take
                // what is within reach plus a margin (bombers ground targets first, so after the
                // ordered target dies they go on with the army). Ground units stay put.
                if plane && !df.weapons.is_empty() {
                    let ground_first = df.weapons.iter().any(|w| w.kind == WeaponKind::Bomb);
                    let land_t = || if any_land { census.nearest_enemy(pos, team, df.max_range() + 45.0, 0.0, true, false) } else { None };
                    let air_t = || if any_air { census.nearest_enemy(pos, team, air_range + 45.0, 0.0, false, true) } else { None };
                    let t = if ground_first { land_t().or_else(air_t) } else { air_t().or_else(land_t) };
                    if let Some(t) = t {
                        goal = Some(t.pos);
                    }
                }
                // Ground units with a target but no line of fire to it (a rock, a wreck, a hill or a
                // friend in the way) step aside to a nearby spot that has one (`firing_spot`).
                if !plane && goal.is_none() {
                    goal = reposition(&mut mover);
                    // (The spot stays in `Mover.cover` until it is reached.)
                    in_cover |= goal.is_some();
                }
                // Engineering stations pick their own work.
                if df.is_station() {
                    let r = df.build_range();
                    let mut best: Option<(f32, u32)> = None;
                    census.near(pos, r + 6.0, |c| {
                        if c.team != team || c.id == id || c.hosted || c.hp <= 0.0 || pos.dist(c.pos) > r + c.radius {
                            return;
                        }
                        let job = if c.reviving() {
                            return;
                        } else if !c.done || c.upgrading {
                            c.id
                        } else if c.building != 0 {
                            c.building
                        } else if c.hp < c.max_hp {
                            c.id
                        } else {
                            return;
                        };
                        // Prefer construction over repairs, then the closest.
                        let score = pos.dist(c.pos) + if job == c.id && c.done && !c.upgrading { 1000.0 } else { 0.0 };
                        if best.map_or(true, |b| score < b.0) {
                            best = Some((score, job));
                        }
                    });
                    work = best.map_or(0, |b| b.1);
                }
                if infantry {
                    // Standing near a building it may take, infantry take it without being told.
                    let auto = if game.infantry_capture { seize_target(&census, &defs, team, pos) } else { None };
                    if let Some(t) = auto {
                        if pos.dist(t.pos) > t.radius + df.radius() + 1.5 {
                            goal = Some(t.pos);
                        } else {
                            effects.seize.push((t.id, owner));
                            work = t.id;
                        }
                    } else {
                        in_cover = true;
                        goal = cover(&mut mover, None, defs.rules.cover_search);
                    }
                }
            }
            Some(Order::Move(p)) => {
                // Infantry passing a building they may take stop and take it, then walk on.
                let auto = if infantry && game.infantry_capture && pos.dist(p) > arrive { seize_target(&census, &defs, team, pos) } else { None };
                if let Some(t) = auto {
                    if pos.dist(t.pos) > t.radius + df.radius() + 1.5 {
                        goal = Some(t.pos);
                    } else {
                        effects.seize.push((t.id, owner));
                        work = t.id;
                    }
                } else if is_struct || pos.dist(p) <= arrive {
                    pop = true;
                    if let Some(m) = mover.as_mut() {
                        m.anchor = p;
                    }
                } else {
                    goal = Some(p);
                }
            }
            Some(Order::AttackMove(p)) | Some(Order::Patrol(p)) => {
                let mut engaged = false;
                let mut shift: Option<V2> = None;
                if let Some(w) = w0 {
                    if plane {
                        // Bombers (fighter-bombers too) go for ground targets first, fighters for
                        // aircraft; either takes the other kind when there is nothing else.
                        let ground_first = df.weapons.iter().any(|w| w.kind == WeaponKind::Bomb);
                        let land_t = || if any_land { census.nearest_enemy(pos, team, df.max_range() + 45.0, 0.0, true, false) } else { None };
                        let air_t = || if any_air { census.nearest_enemy(pos, team, air_range + 45.0, 0.0, false, true) } else { None };
                        let t = if ground_first { land_t().or_else(air_t) } else { air_t().or_else(land_t) };
                        if let Some(t) = t {
                            goal = Some(t.pos);
                            engaged = true;
                        }
                    } else if census.nearest_enemy(pos, team, w.range * 0.95, w.min_range, w.hits_land, w.hits_air).is_some() {
                        if !no_shot(None) {
                            // Vehicles with a machine gun (also a tank's secondary one) shoot on the move:
                            // they never stop for a fight.
                            engaged = infantry || !df.weapons.iter().any(|w| w.is_machine_gun());
                        } else {
                            // Enemy in range but nothing to shoot with: step to where the guns have a line.
                            shift = reposition(&mut mover);
                        }
                    }
                }
                // A building infantry can take is not a fight: they walk up to it instead of
                // stopping to shoot it down. Anything else in range is.
                if engaged && infantry && game.infantry_capture {
                    if let Some(w) = w0 {
                        let mut foe = false;
                        census.near(pos, w.range * 0.95, |c| {
                            let takeable = c.structure && can_seize(&census, &defs, team, c) && !guarded(&census, &defs, c);
                            foe |= !takeable && census.valid_target(c, team, w.hits_land, w.hits_air);
                        });
                        engaged = foe;
                    }
                }
                if is_struct {
                    pop = true;
                } else if engaged && infantry {
                    // Fighting on the way: into cover close to where the fight found them.
                    in_cover = true;
                    goal = cover(&mut mover, Some(pos), defs.rules.cover_search * 0.6);
                } else if let Some(s) = shift {
                    goal = Some(s);
                    in_cover = true;
                } else if !engaged {
                    // Passing a building it may take, infantry take it on the way.
                    let auto = if infantry && game.infantry_capture { seize_target(&census, &defs, team, pos) } else { None };
                    if let Some(t) = auto {
                        if pos.dist(t.pos) > t.radius + df.radius() + 1.5 {
                            goal = Some(t.pos);
                        } else {
                            effects.seize.push((t.id, owner));
                            work = t.id;
                        }
                    } else if pos.dist(p) <= arrive.max(1.5) {
                        pop = true;
                        if let Some(m) = mover.as_mut() {
                            m.anchor = p;
                        }
                    } else {
                        goal = Some(p);
                    }
                }
            }
            Some(Order::Attack(tid)) => {
                pop = true;
                if let Some(t) = census.get(tid) {
                    let range = df.weapons.iter().filter(|w| if t.air { w.hits_air } else { w.hits_land }).map(|w| w.range).fold(0.0, f32::max);
                    if range > 0.0 && t.hp > 0.0 && !t.hosted && t.known_to(team) && t.team != team {
                        pop = false;
                        focus = tid;
                        if plane || (!is_struct && pos.dist(t.pos) > range * 0.9) {
                            goal = Some(t.pos);
                        } else if !is_struct && no_shot(Some(tid)) {
                            // No line to it from here: another angle if one is near, else closer in,
                            // but not right up to it (half the range is as close as that goes).
                            let spot = reposition(&mut mover);
                            in_cover |= spot.is_some();
                            goal = spot.or_else(|| (pos.dist(t.pos) > range * 0.5).then_some(t.pos));
                        }
                    }
                }
            }
            Some(Order::AttackGround { pos: gp, radius }) => {
                let range = df.weapons.iter().filter(|w| w.hits_land).map(|w| w.range).fold(0.0, f32::max);
                // A bombing run is one stick; shelling goes on for a few volleys, more for a wide circle.
                let bomber = df.weapons.iter().any(|w| w.kind == WeaponKind::Bomb);
                let need = if bomber { 1 } else { 3 + radius.round() as u16 };
                let same = weapons.as_ref().is_some_and(|w| w.ground.is_some_and(|(p, _)| p == gp));
                let done = same && weapons.as_ref().is_some_and(|w| w.ground_shots >= need);
                if range <= 0.0 || done {
                    pop = true;
                } else {
                    ground = Some((gp, radius));
                    if plane || (!is_struct && pos.dist(gp) > range * 0.92) {
                        goal = Some(gp);
                    }
                }
            }
            Some(Order::Build { def: bd, pos: bp }) => {
                if !df.is_engineer() || !df.builds.contains(&bd) {
                    pop = true;
                } else {
                    let r = reach(defs.get(bd).radius());
                    let mut site: Option<Cen> = None;
                    census.near(bp, 1.0, |c| {
                        if c.structure && c.pos == bp && c.hp > 0.0 {
                            site = Some(*c);
                        }
                    });
                    let pending = effects.sites.iter().find(|s| s.1 == bp).copied();
                    // The site was started (by this unit's order) and has been destroyed since.
                    let lost = builder.as_ref().map_or(false, |b| b.site == Some(bp));
                    match (site, pending) {
                        (Some(s), _) => {
                            if s.done || s.def != bd || s.team != team {
                                pop = true;
                            } else {
                                new_site = Some(bp);
                                if pos.dist(bp) > r {
                                    goal = Some(bp);
                                } else {
                                    work = s.id;
                                }
                            }
                        }
                        (None, Some(p)) => {
                            new_site = Some(bp);
                            work = if p.0 == bd { p.2 } else { 0 };
                        }
                        // Destroyed while under construction: skip it and go on with the queue.
                        (None, None) if lost => pop = true,
                        (None, None) => {
                            if pos.dist(bp) > r {
                                goal = Some(bp);
                            } else if can_place(&defs, &map, &nav.occ, bd, bp) {
                                let sid = spawn_unit(&mut commands, &defs, &mut game, &map, bd, owner, bp, PI * 0.5, false, 0);
                                nav.stamp_def(&map, defs.get(bd), bp);
                                // Trees and rocks on the plot are flattened.
                                features.clear_square(bp, defs.get(bd).footprint);
                                effects.sites.push((bd, bp, sid));
                                new_site = Some(bp);
                                work = sid;
                            } else {
                                pop = true;
                            }
                        }
                    }
                }
            }
            Some(Order::Assist(tid)) => match census.get(tid) {
                Some(t) if t.hp > 0.0 && tid != id && t.team == team => {
                    let r = reach(t.radius);
                    let td = defs.get(t.def);
                    if df.can_assist() || df.is_scavenger() {
                        // What the assisted unit is working on (or needs itself).
                        let job = if !t.done || t.upgrading {
                            t.id
                        } else if t.building != 0 {
                            t.building
                        } else if t.work != 0 && t.work != id {
                            t.work
                        } else if t.hp < t.max_hp {
                            t.id
                        } else {
                            0
                        };
                        // A tree, rock or wreck it is reclaiming: reclaim it too.
                        let reclaim = match job {
                            0 => None,
                            j if is_feature(j) => features.get(j).map(|f| (f.pos, defs.rules.features.get(f.def as usize).map_or(0.5, |d| d.radius()))),
                            j => wrecks.0.iter().find(|w| w.0 == j).map(|w| (w.1, 0.5)),
                        };
                        // Scavengers only build what is being revived, everybody else anything but that.
                        let job = match (reclaim, census.get(job)) {
                            (Some(_), _) => job,
                            (None, Some(j)) if if j.reviving() { df.is_scavenger() } else { df.can_assist() } => job,
                            _ => 0,
                        };
                        if let Some((at, jr)) = reclaim.filter(|_| job != 0) {
                            if pos.dist(at) > reach(jr) {
                                if is_struct {
                                    pop = true;
                                } else {
                                    goal = Some(at);
                                }
                            } else {
                                let amount = df.build_rate() * defs.rules.reclaim_rate * DT;
                                if is_feature(job) {
                                    effects.reclaim_feature.push((job, amount, owner));
                                } else {
                                    effects.reclaim.push((job, amount, owner));
                                }
                                work = job;
                            }
                        } else if pos.dist(t.pos) > r {
                            if is_struct {
                                pop = true;
                            } else {
                                goal = Some(t.pos);
                            }
                        } else {
                            work = job;
                        }
                        // With nothing to do, keep following a constructor (or a factory) that may
                        // find something; a scavenger only follows those that reclaim, and not when
                        // more orders wait (the assist a revive put in front of its area order).
                        let guard = if df.is_scavenger() { (td.is_engineer() || td.is_scavenger()) && orders.0.len() == 1 } else { td.is_factory() || td.can_assist() || td.is_scavenger() };
                        if job == 0 && !guard {
                            pop = true;
                        }
                    } else if is_struct {
                        pop = true;
                    } else if pos.dist(t.pos) > r + 3.0 {
                        goal = Some(t.pos);
                    }
                }
                _ => pop = true,
            },
            Some(Order::Reclaim(fid)) if is_feature(fid) => match features.get(fid) {
                // Trees and rocks: engineers and scavengers.
                Some(f) if df.can_assist() || df.is_scavenger() => {
                    let fr = defs.rules.features.get(f.def as usize).map_or(0.5, |d| d.radius());
                    if pos.dist(f.pos) > reach(fr) {
                        if is_struct {
                            pop = true;
                        } else {
                            goal = Some(f.pos);
                        }
                    } else {
                        effects.reclaim_feature.push((fid, df.build_rate() * defs.rules.reclaim_rate * DT, owner));
                        work = fid;
                    }
                }
                _ => pop = true,
            },
            Some(Order::Reclaim(wid)) => match wrecks.0.iter().find(|w| w.0 == wid) {
                Some(w) if df.can_assist() || df.is_scavenger() => {
                    if pos.dist(w.1) > reach(0.5) {
                        if is_struct {
                            pop = true;
                        } else {
                            goal = Some(w.1);
                        }
                    } else {
                        effects.reclaim.push((wid, df.build_rate() * defs.rules.reclaim_rate * DT, owner));
                        work = wid;
                    }
                }
                Some(_) => pop = true,
                // Not a wreck: a standing building, taken apart for its mass (own or enemy).
                None => match census.get(wid) {
                    Some(t) if df.can_assist() && t.structure && t.hp > 0.0 && !t.hosted && (t.owner == owner || (t.team != team && t.known_to(team))) => {
                        if pos.dist(t.pos) > reach(t.radius) {
                            if is_struct {
                                pop = true;
                            } else {
                                goal = Some(t.pos);
                            }
                        } else {
                            effects.reclaim_unit.push((wid, df.build_rate() * defs.rules.reclaim_rate * DT, owner));
                            work = wid;
                        }
                    }
                    _ => pop = true,
                },
            },
            // Infantry capture (lobby option): walk up to it and take it while nobody guards it.
            Some(Order::Capture(tid)) if !df.is_engineer() => match census.get(tid) {
                Some(t) if infantry && game.infantry_capture && can_seize(&census, &defs, team, t) => {
                    if pos.dist(t.pos) > t.radius + df.radius() + 1.5 {
                        goal = Some(t.pos);
                    } else if !guarded(&census, &defs, t) {
                        effects.seize.push((tid, owner));
                        work = tid;
                    }
                }
                _ => pop = true,
            },
            Some(Order::Capture(tid)) => match census.get(tid) {
                Some(t) if df.is_engineer() && t.hp > 0.0 && t.done && !t.hosted && !t.air && t.team != team && t.known_to(team) && !defs.factions.iter().any(|f| f.commander == Some(t.def)) && !defs.get(t.def).is_garrison() => {
                    if pos.dist(t.pos) > reach(t.radius) {
                        goal = Some(t.pos);
                    } else {
                        effects.capture.push((tid, df.build_rate() * DT, owner));
                        work = tid;
                    }
                }
                _ => pop = true,
            },
            Some(Order::Revive(wid)) => match wrecks.0.iter().find(|w| w.0 == wid && revivable(w, game.tick)).copied() {
                Some(w) if df.is_scavenger() => {
                    let wd = defs.get(w.3);
                    if let Some(done) = effects.revived.iter().find(|r| r.0 == wid) {
                        // Another scavenger started it this very tick: help with it.
                        orders.0[0] = Order::Assist(done.1);
                    } else if pos.dist(w.1) > reach(wd.radius()) {
                        goal = Some(w.1);
                    } else if wd.is_structure() && !can_place(&defs, &map, &nav.occ, w.3, w.1) {
                        pop = true;
                    } else {
                        let sid = revive(&mut commands, &defs, &mut game, &map, &mut nav, &mut effects, w, owner);
                        orders.0[0] = Order::Assist(sid);
                        work = sid;
                    }
                }
                // Somebody of the side revived it already: help build it.
                None if df.is_scavenger() => match census.list.iter().find(|c| c.revival == wid && c.reviving() && c.owner == owner && c.hp > 0.0) {
                    Some(c) => orders.0[0] = Order::Assist(c.id),
                    None => pop = true,
                },
                _ => pop = true,
            },
            Some(Order::ReviveArea { pos: c, radius }) => {
                // The nearest wreck in the circle that can come back (a building needs its plot free).
                let best = wrecks
                    .0
                    .iter()
                    .filter(|w| revivable(w, game.tick) && w.1.dist(c) <= radius && w.2 > 0.0 && !effects.revived.iter().any(|r| r.0 == w.0))
                    .filter(|w| !defs.get(w.3).is_structure() || can_place(&defs, &map, &nav.occ, w.3, w.1))
                    .min_by(|a, b| a.1.dist(pos).partial_cmp(&b.1.dist(pos)).unwrap_or(std::cmp::Ordering::Equal))
                    .copied();
                // An unfinished revival of ours in the circle: worth a detour of up to `TEAMWORK`
                // to help with it rather than starting another one.
                let mut site: Option<(f32, u32)> = None;
                census.near(c, radius, |t| {
                    if t.owner == owner && t.reviving() && t.hp > 0.0 && !t.hosted {
                        let d = pos.dist(t.pos) - TEAMWORK;
                        if site.map_or(true, |s| d < s.0 || (d == s.0 && t.id < s.1)) {
                            site = Some((d, t.id));
                        }
                    }
                });
                let site = site.filter(|s| best.map_or(true, |w| s.0 < w.1.dist(pos))).and_then(|s| census.get(s.1));
                match (site, best) {
                    (Some(t), _) if df.is_scavenger() => {
                        if pos.dist(t.pos) > reach(t.radius) {
                            goal = Some(t.pos);
                        } else {
                            orders.0.insert(0, Order::Assist(t.id));
                            work = t.id;
                        }
                    }
                    (None, Some(w)) if df.is_scavenger() => {
                        if pos.dist(w.1) > reach(defs.get(w.3).radius()) {
                            goal = Some(w.1);
                        } else {
                            // Finish this one, then come back to the area order for the next.
                            let sid = revive(&mut commands, &defs, &mut game, &map, &mut nav, &mut effects, w, owner);
                            orders.0.insert(0, Order::Assist(sid));
                            work = sid;
                        }
                    }
                    _ => pop = true,
                }
            }
            Some(Order::Terraform { pos: c, radius, height }) => {
                let limit = defs.rules.terraform_max;
                if !df.is_engineer() || !defs.rules.terraforming || terraform_left(&map, &terrain, &nav.occ, c, radius, height, limit) < 0.05 {
                    pop = true;
                } else if pos.dist(c) > df.build_range().max(2.0) + radius + df.radius() {
                    goal = Some(c);
                } else if !census.get(id).map_or(false, |c| c.paused) {
                    effects.terraform.push((c, radius, height, df.build_rate() * defs.rules.terraform_rate * DT, owner));
                }
            }
            Some(Order::Load(tid)) => {
                pop = true;
                let near = |t: &Cen| pos.dist(t.pos) <= 2.5 + t.radius + df.radius() + scoop;
                if is_struct {
                    // An occupiable building does not fetch anybody: infantry walk in.
                } else if let Some(tr) = &df.transport {
                    // A transport fetching one unit.
                    if let Some(t) = census.get(tid).filter(|t| t.owner == owner && t.done && !t.hosted && t.hp > 0.0 && fits(df, defs.get(t.def))) {
                        let free = room(&effects, id, tr.capacity) >= defs.get(t.def).cargo as i32 && !effects.loaded.iter().any(|l| l.0 == tid);
                        if free && !near(t) {
                            goal = Some(t.pos);
                            pop = false;
                        } else if free {
                            commands.entity(t.e).insert(Hosted(id));
                            effects.loaded.push((tid, id));
                        }
                    }
                } else if let Some(t) = census.get(tid).filter(|t| df.is_infantry() && df.assault > 0.0 && carriable(df) && is_trench(&defs, t)) {
                    // A trench piece: storm the enemy infantry in it (`assaults` fights it out at close
                    // quarters), then step into it unless one of ours already stands there.
                    if !trench_holders(&census, &defs, t, team).is_empty() {
                        pop = false;
                        if near(t) {
                            effects.assault.push((id, tid));
                        } else {
                            goal = Some(t.pos);
                        }
                    } else if !in_cell(pos, t.pos) {
                        let mut taken = false;
                        census.near(t.pos, CELL, |o| {
                            if o.id != id && o.team == team && !o.air && !o.hosted && !o.structure && in_cell(o.pos, t.pos) {
                                taken = true;
                            }
                        });
                        if !taken {
                            pop = false;
                            goal = Some(t.pos);
                        }
                    }
                } else if let Some(t) = census.get(tid).filter(|t| t.done && !t.hosted && t.hp > 0.0 && carriable(df) && df.assault > 0.0 && defs.get(t.def).is_garrison() && census.list.iter().any(|r| r.host == t.id && r.done && r.hp > 0.0 && hostile(team, r.team))) {
                    // An enemy-held building: storm it (`assaults` fights it out at the door), then walk in.
                    pop = false;
                    if near(t) {
                        effects.assault.push((id, tid));
                    } else {
                        goal = Some(t.pos);
                    }
                } else if let Some(t) = census.get(tid).filter(|t| t.done && !t.hosted && t.hp > 0.0 && carriable(df) && may_board(&defs, &census, df, t, team)) {
                    // A unit walking up to a transport (or infantry to a building) and getting in.
                    if let Some(tr) = &defs.get(t.def).transport {
                        if room(&effects, tid, tr.capacity) >= df.cargo as i32 {
                            if near(t) {
                                if let Some(me) = census.get(id) {
                                    commands.entity(me.e).insert(Hosted(tid));
                                    effects.loaded.push((id, tid));
                                }
                            } else {
                                goal = Some(t.pos);
                                pop = false;
                            }
                        }
                    }
                }
            }
            Some(Order::Ferry { pos: c, pick }) if ferries(df) => {
                let aboard = census.list.iter().any(|t| t.host == id) || effects.loaded.iter().any(|l| l.1 == id);
                // The next route point round the loop: when it is the beacon, this is the drop point.
                let next_pick = orders.0.iter().skip(1).find_map(|o| if let Order::Ferry { pick, .. } = o { Some(*pick) } else { None });
                match (pick, next_pick) {
                    // What is left of a route that lost its beacon or all its other points: done.
                    (true, None) => pop = true,
                    (false, None) => {
                        pop = true;
                        if aboard {
                            push_front = Some(Order::Unload { pos: c, radius: FERRY_DROP, occupy: false });
                        }
                    }
                    (true, Some(_)) => {
                        let free = room(&effects, id, df.transport.as_ref().map_or(0, |t| t.capacity));
                        if pos.dist(c) > FERRY_PICK + 4.0 {
                            goal = Some(c);
                        } else {
                            // The nearest unit waiting at this beacon that fits; leave with what is aboard
                            // once nobody who fits is waiting or on the way.
                            let mut best: Option<(f32, u32)> = None;
                            let mut coming = false;
                            census.near(c, FERRY_COMING, |t| {
                                let td = defs.get(t.def);
                                if t.owner != owner || !t.done || t.hosted || t.hp <= 0.0 || !fits(df, td) || td.cargo as i32 > free || !t.ferry_wait.is_some_and(|w| w.dist(c) <= FERRY_SAME) || effects.loaded.iter().any(|l| l.0 == t.id) {
                                    return;
                                }
                                if t.pos.dist(c) > FERRY_PICK {
                                    coming = true;
                                    return;
                                }
                                let d = pos.dist(t.pos);
                                if best.map_or(true, |b| d < b.0 || (d == b.0 && t.id < b.1)) {
                                    best = Some((d, t.id));
                                }
                            });
                            if let Some(t) = best.and_then(|b| census.get(b.1)) {
                                if pos.dist(t.pos) <= 2.5 + t.radius + df.radius() + scoop {
                                    commands.entity(t.e).insert(Hosted(id));
                                    effects.loaded.push((t.id, id));
                                } else {
                                    goal = Some(t.pos);
                                }
                            } else if aboard && !coming {
                                pop = true;
                                rotate = true;
                            } else if pos.dist(c) > arrive {
                                goal = Some(c);
                            }
                        }
                    }
                    // A waypoint.
                    (false, Some(false)) => {
                        if pos.dist(c) <= arrive.max(3.0) {
                            pop = true;
                            rotate = true;
                        } else {
                            goal = Some(c);
                        }
                    }
                    // The drop point: everybody out (the Unload order flies there first), then back.
                    (false, Some(true)) => {
                        pop = true;
                        rotate = true;
                        if aboard {
                            push_front = Some(Order::Unload { pos: c, radius: FERRY_DROP, occupy: false });
                        }
                    }
                }
            }
            Some(Order::Ferry { pos: c, .. }) => {
                // Waiting for a ferry at its beacon; given up when no transport of ours serves it any more.
                let served = census.list.iter().any(|t| t.owner == owner && t.beacon.is_some_and(|b| b.dist(c) <= FERRY_SAME));
                if !served || is_struct {
                    pop = true;
                } else if pos.dist(c) > 3.0 {
                    goal = Some(c);
                }
            }
            Some(Order::LoadArea { pos: c, radius }) => {
                pop = true;
                if let Some(tr) = df.transport.as_ref().filter(|_| !is_struct) {
                    let free = room(&effects, id, tr.capacity);
                    let mut best: Option<(f32, u32)> = None;
                    census.near(c, radius.max(1.0), |t| {
                        if t.owner != owner || !t.done || t.hosted || t.hp <= 0.0 || t.id == id || !fits(df, defs.get(t.def)) || defs.get(t.def).cargo as i32 > free {
                            return;
                        }
                        if effects.loaded.iter().any(|l| l.0 == t.id) {
                            return;
                        }
                        let d = pos.dist(t.pos);
                        if best.map_or(true, |b| d < b.0 || (d == b.0 && t.id < b.1)) {
                            best = Some((d, t.id));
                        }
                    });
                    if let Some(t) = best.and_then(|b| census.get(b.1)) {
                        pop = false;
                        if pos.dist(t.pos) <= 2.5 + t.radius + df.radius() + scoop {
                            commands.entity(t.e).insert(Hosted(id));
                            effects.loaded.push((t.id, id));
                        } else {
                            goal = Some(t.pos);
                        }
                    }
                }
            }
            Some(Order::Unload { pos: c, radius, occupy }) => {
                // A ground transport cannot stop on a cliff or a building: the nearest free spot will do.
                let mut c = c;
                if !df.is_air() && nav.at(&map, c) != 0 {
                    let (cx, cy) = map.cell_of(c);
                    if let Some((x, y)) = nearest_free(&nav.occ, map.dims(), cx, cy) {
                        c = v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL);
                    }
                }
                let cargo: Vec<&Cen> = census.list.iter().filter(|t| t.host == id).collect();
                if df.transport.is_none() || cargo.is_empty() {
                    pop = true;
                } else if is_struct {
                    // Infantry leave an occupied building on the side facing the spot and walk there.
                    let n = cargo.len() as f32;
                    for (k, t) in cargo.iter().enumerate() {
                        let dest = set_down(&map, &nav, c + V2::from_angle(k as f32 * 2.4) * (radius.max(1.5) * ((k as f32 + 0.5) / n).sqrt()));
                        let door = garrison_door(&map, &nav, pos, df.radius(), c, k, cargo.len());
                        commands.entity(t.e).remove::<Hosted>().insert((
                            Body { pos: door, y: map.h(door), yaw: (dest - door).angle(), vel: v2(0.0, 0.0) },
                            Orders(vec![Order::Move(dest)]),
                            Mover { anchor: dest, path_goal: door, ..default() },
                        ));
                    }
                    pop = true;
                } else if !plane && pos.dist(c) > arrive.max(1.5) {
                    goal = Some(c);
                } else {
                    let r = radius.max(1.5);
                    // Who was set down this tick, and where they will stand.
                    let mut released: Vec<(&Cen, V2)> = vec![];
                    if plane {
                        // An airdrop on the go: the plane flies straight through the circle and lets each rider
                        // go as it passes over his spot; he falls (`Mover.drop`, mostly for show) and lands
                        // `DROP_SECS` later. What is left when it flies out of the circle comes round again.
                        let dir = V2::from_angle(body.yaw);
                        let rel = pos - c;
                        if rel.len() > r + AIRDROP_MARGIN {
                            goal = Some(c);
                        } else {
                            goal = Some(pos + dir * 30.0);
                            let along = rel.x * dir.x + rel.y * dir.y;
                            for t in cargo.iter() {
                                let spot = drop_spot(t.id, c, r);
                                if along < (spot.x - c.x) * dir.x + (spot.y - c.y) * dir.y {
                                    continue;
                                }
                                let to = set_down(&map, &nav, spot);
                                let from = Body { pos, y: body.y, yaw: body.yaw, vel: v2(0.0, 0.0) };
                                let mover = Mover { anchor: to, path_goal: to, drop: DROP_SECS, drop_from: pos, drop_to: to, drop_h: (body.y - map.h(to)).max(1.0), ..default() };
                                commands.entity(t.e).try_remove::<Hosted>().try_insert((from, mover));
                                released.push((t, to));
                            }
                        }
                    } else {
                        // Everybody out, spread over the circle (a spiral from the middle).
                        let n = cargo.len() as f32;
                        for (k, t) in cargo.iter().enumerate() {
                            let p = set_down(&map, &nav, c + V2::from_angle(k as f32 * 2.4) * (r * ((k as f32 + 0.5) / n).sqrt()));
                            // Their own orders stay: what they were told to do before or while riding.
                            commands.entity(t.e).try_remove::<Hosted>().try_insert((Body { pos: p, y: map.h(p), yaw: body.yaw, vel: v2(0.0, 0.0) }, Mover { anchor: p, path_goal: p, ..default() }));
                            released.push((t, p));
                        }
                    }
                    // A dragged circle: infantry go on into the free buildings in it (the nearest to where they
                    // are set down, while there is room); the rest just stand spread out.
                    if occupy && !released.is_empty() {
                        let mut room_left: Vec<(u32, V2, i32)> = vec![];
                        for b in census.list.iter().filter(|b| b.structure && b.done && !b.hosted && b.hp > 0.0 && b.host != id && defs.get(b.def).is_garrison() && c.dist(b.pos) <= radius + b.radius) {
                            let Some(tr) = &defs.get(b.def).transport else { continue };
                            let used: i32 = census.list.iter().filter(|r| r.host == b.id && r.hp > 0.0).map(|r| defs.get(r.def).cargo as i32).sum();
                            room_left.push((b.id, b.pos, tr.capacity as i32 - used));
                        }
                        room_left.sort_by_key(|r| r.0);
                        for &(t, p) in &released {
                            let td = defs.get(t.def);
                            if !td.is_infantry() || !carriable(td) {
                                continue;
                            }
                            let best = room_left.iter_mut().filter(|r| r.2 >= td.cargo as i32 && census.get(r.0).is_some_and(|b| may_board(&defs, &census, td, b, team))).min_by(|a, b| a.1.dist(p).total_cmp(&b.1.dist(p)).then(a.0.cmp(&b.0)));
                            if let Some(r) = best {
                                r.2 -= td.cargo as i32;
                                // Their queued orders give way; the others stay where they stand.
                                commands.entity(t.e).try_insert(Orders(vec![Order::Load(r.0)]));
                            }
                        }
                    }
                    pop = released.len() == cargo.len();
                }
            }
            Some(Order::ReclaimArea { pos: c, radius }) => {
                // The nearest wreck, tree or rock in the circle; one the side is already reclaiming
                // counts `TEAMWORK` nearer, so a group works on the same things together.
                let mut busy: Vec<u32> = census.list.iter().filter(|u| u.team == team && u.id != id && u.work != 0).map(|u| u.work).collect();
                busy.sort_unstable();
                busy.dedup();
                let score = |tid: u32, tp: V2| pos.dist(tp) - if busy.binary_search(&tid).is_ok() { TEAMWORK } else { 0.0 };
                let feature_r = |fid: u32| features.get(fid).and_then(|f| defs.rules.features.get(f.def as usize)).map_or(0.5, |d| d.radius());
                let mut cands: Vec<(u32, V2, f32)> = vec![];
                cands.extend(
                    wrecks
                        .0
                        .iter()
                        .filter(|w| w.1.dist(c) <= radius && w.2 > 0.0)
                        .min_by(|a, b| score(a.0, a.1).partial_cmp(&score(b.0, b.1)).unwrap_or(std::cmp::Ordering::Equal))
                        .map(|w| (w.0, w.1, 0.5)),
                );
                if df.can_assist() || df.is_scavenger() {
                    cands.extend(features.nearest(c, radius, pos).map(|(fid, fp)| (fid, fp, feature_r(fid))));
                    for &fid in busy.iter().filter(|f| is_feature(**f)) {
                        if let Some(f) = features.get(fid).filter(|f| f.pos.dist(c) <= radius) {
                            cands.push((fid, f.pos, feature_r(fid)));
                        }
                    }
                }
                let best = cands.into_iter().min_by(|a, b| score(a.0, a.1).partial_cmp(&score(b.0, b.1)).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
                match best {
                    Some((tid, tp, tr)) if df.can_assist() || df.is_scavenger() => {
                        if pos.dist(tp) > reach(tr) {
                            if is_struct {
                                pop = true;
                            } else {
                                goal = Some(tp);
                            }
                        } else {
                            let amount = df.build_rate() * defs.rules.reclaim_rate * DT;
                            if is_feature(tid) {
                                effects.reclaim_feature.push((tid, amount, owner));
                            } else {
                                effects.reclaim.push((tid, amount, owner));
                            }
                            work = tid;
                        }
                    }
                    _ => pop = true,
                }
            }
        }
        if let Some(b) = builder.as_mut() {
            b.work = work;
            b.site = new_site;
        }
        if let Some(m) = mover.as_mut() {
            m.goal = goal;
            if !in_cover {
                m.cover = None;
            }
        }
        if let Some(w) = weapons.as_mut() {
            w.focus = focus;
            if w.ground.map(|g| g.0) != ground.map(|g| g.0) {
                w.ground_shots = 0;
            }
            w.ground = ground;
        }
        if pop && !orders.0.is_empty() {
            let o = orders.0.remove(0);
            if rotate || matches!(o, Order::Patrol(_)) {
                orders.0.push(o);
            }
            if let Some(m) = mover.as_mut() {
                m.stuck = 0;
            }
        }
        if let Some(o) = push_front {
            orders.0.insert(0, o);
        }
    }
}

/// Ferry routes: units within `FERRY_PICK` of the beacon are fetched, a transport waits for those
/// heading there from within `FERRY_COMING`; beacons closer than `FERRY_SAME` are the same one.
/// Riders are set down over a circle of `FERRY_DROP`.
const FERRY_PICK: f32 = 12.0;
const FERRY_COMING: f32 = 40.0;
const FERRY_SAME: f32 = 4.0;
const FERRY_DROP: f32 = 6.0;
/// Airdrops: how long a rider falls (visual, but he is helpless meanwhile), and how far outside the
/// circle a transport plane is already lined up to start letting go.
pub const DROP_SECS: f32 = 2.5;
const AIRDROP_MARGIN: f32 = 4.0;
/// A transport plane picks units up within this much more than a hovering transport does.
const PLANE_SCOOP: f32 = 5.0;

// **Airdrops** (Swift, `MoveKind::Plane` with `transport` block; `systems.rs`, `Order::Unload`): transport plane cannot stop, so
// never lands. *Unloading*: it flies at circle (click is circle of 3), and once within `AIRDROP_MARGIN` of it flies straight on
// through, letting each rider go when it passes over his spot (`drop_spot(id, c, r)`: golden-ratio angle and radius from id alone, so
// spread does not depend on who is already out); riders left when it flies out of circle are dropped on next pass. A released rider gets
// `Mover.drop` (`DROP_SECS` 2.5, snapshotted with Mover): `locomotion` glides it from plane to its spot, `drop_h` above ground at
// first (`h (1 - t^2)`), and `think` / `weapons` leave it alone until it lands - for gameplay fall is mostly visual, but he is helpless for
// 2.5 s. *Loading*: it scoops up what it flies within `PLANE_SCOOP` (5) more than hovering transport would (`Load`, `LoadArea`, ferry beacons: it
// circles round beacon). `Unload.occupy` (set by HUD for dragged circle, also for APCs and Stork): infantry set down get
// `Order::Load(building)` for nearest free garrisonable building in circle (capacity and `may_board` counted), rest stay spread out.
// `--combattest` "Swift airdrop" line (scoops 8 troopers, drops them over circle with house in it, never slows down, they land 2-4 s later).
/// Where rider `id` of an airdrop over the circle (`c`, `r`) lands: a spread that depends on the id
/// alone (golden-ratio angle and radius), so every tick agrees however many are already out.
fn drop_spot(id: u32, c: V2, r: f32) -> V2 {
    let k = id as f32;
    let f = k * 0.618034;
    c + V2::from_angle(k * 2.4) * (r * (f - f.floor()).sqrt())
}

/// Can unit type `df` of `team` get into `t`? Transports take their own side; infantry also
/// walk into an empty (neutral) building or one their side already holds. A map building whose
/// last rider died this tick still has the old owner until `garrisons` runs: it counts as empty.
fn may_board(defs: &Defs, census: &Census, df: &crate::data::UnitDef, t: &Cen, team: u8) -> bool {
    let td = defs.get(t.def);
    if td.is_garrison() {
        // Every garrison (houses, bunkers, field bunkers, watch towers) belongs to whoever holds
        // it: an empty one is open to anybody but the hostile side's.
        let empty = || !census.list.iter().any(|r| r.host == t.id && r.hp > 0.0);
        df.is_infantry() && (t.team == team || t.owner == NEUTRAL || (t.owner != HOSTILE && empty()))
    } else {
        t.team == team && (td.transport.is_none() || fits(td, df))
    }
}

/// Can a unit of type `df` (on `team`) storm `t`: a finished building it could garrison, held by
/// enemy infantry? Only infantry with an `assault` value do.
fn storms(defs: &Defs, census: &Census, df: &crate::data::UnitDef, t: &Cen, team: u8) -> bool {
    let td = defs.get(t.def);
    df.is_infantry() && df.assault > 0.0 && carriable(df) && td.is_garrison() && t.done && t.hp > 0.0 && hostile(team, t.team) && census.list.iter().any(|r| r.host == t.id && r.done && r.hp > 0.0 && hostile(team, r.team))
}

/// A finished, standing trench piece (a field work dug into the ground).
fn is_trench(defs: &Defs, t: &Cen) -> bool {
    t.fieldwork && t.done && t.hp > 0.0 && defs.get(t.def).work().is_some_and(|w| w.1 > 0.0)
}

/// Ids of the infantry of teams hostile to `team` standing in trench piece `piece`, in id order.
fn trench_holders(census: &Census, defs: &Defs, piece: &Cen, team: u8) -> Vec<u32> {
    let mut ids = vec![];
    census.near(piece.pos, CELL, |c| {
        if c.hp > 0.0 && hostile(team, c.team) && trench_of(census, defs, c).is_some_and(|t| t.0 == piece.id) {
            ids.push(c.id);
        }
    });
    ids.sort_unstable();
    ids
}

// **Trenches are stormed same way**: attack order by such infantry on enemy infantry standing in trench piece, or on
// piece they hold, becomes `Load(piece)` (`systems::trench_to_storm`); `think` runs them to its edge and queues them in
// `Effects.assault`, `assaults` takes piece's holders (`trench_holders`) as defenders (trench cover does not
// soften close-quarters damage), holders do not fire out while stormed, and once clear one attacker steps in.
// Before this trench could only be shot out: 6 riflemen on attack-move lost to 4 in trench. `--combattest` fieldworks
// "Trench fight": 6 storming riflemen take trench 4 hold, even fight goes to defenders.
/// The trench piece a unit of type `df` (on `team`) told to attack `t` storms instead: `t` itself if
/// it is a piece enemy infantry hold, or the piece the enemy infantry `t` stands in. Only infantry
/// with an `assault` value storm.
fn trench_to_storm(defs: &Defs, census: &Census, df: &crate::data::UnitDef, t: &Cen, team: u8) -> Option<u32> {
    if !df.is_infantry() || df.assault <= 0.0 || !carriable(df) {
        return None;
    }
    if is_trench(defs, t) {
        return (!trench_holders(census, defs, t, team).is_empty()).then_some(t.id);
    }
    if !hostile(team, t.team) {
        return None;
    }
    trench_of(census, defs, t).map(|p| p.0)
}

/// The nearest spot a land unit can stand on (off buildings and cliffs).
/// Where rider `k` of `n` steps out of a building at `pos` (radius `r`): on the side facing `toward`.
fn garrison_door(map: &MapData, nav: &Nav, pos: V2, r: f32, toward: V2, k: usize, n: usize) -> V2 {
    let face = (toward - pos).angle();
    set_down(map, nav, pos + V2::from_angle(face + (k as f32 - n as f32 * 0.5) * 0.3) * (r + 1.2))
}

fn set_down(map: &MapData, nav: &Nav, p: V2) -> V2 {
    let p = map.clamp(p);
    if nav.at(map, p) == 0 {
        return p;
    }
    let (cx, cy) = map.cell_of(p);
    nearest_free(&nav.occ, map.dims(), cx, cy).map_or(p, |(x, y)| v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL))
}

/// Infantry cover: where to stand so that a tree, rock or wreck is between the unit and the
/// nearest known armed enemy; within `search` of `anchor`, not taken by another unit of the side,
/// preferring spots close to the unit and to the anchor. None when there is no threat or no cover.
#[allow(clippy::too_many_arguments)]
/// A nearby spot from which every direct-fire gun of the unit that has no shot now (a building,
/// a hill or a friend in the way) would have a clear line to its target and be in range: the unit
/// moves there and lays its guns again instead of driving straight at the target. Rings of
/// growing radius round the unit (the nearest that works: a step aside, not a drive up to the
/// target), the spot picked last time first (`keep`), so a unit does not dither between two spots.
#[allow(clippy::too_many_arguments)]
fn firing_spot(defs: &Defs, census: &Census, features: &Features, wrecks: &WreckList, map: &MapData, nav: &Nav, id: u32, team: u8, df: &crate::data::UnitDef, pos: V2, ws: &Weapons, keep: Option<V2>, weather: u8) -> Option<V2> {
    let lane = Lane {
        census,
        map,
        team,
        friendly_fire: defs.rules.friendly_fire,
        grace: defs.rules.cover_grace,
        shooter: id,
        host: 0,
        g: defs.rules.gravity,
        spread: defs.rules.weather_of(weather).spread,
        features,
        wrecks,
        defs,
    };
    let jobs: Vec<(&WeaponDef, &Cen)> = df
        .weapons
        .iter()
        .zip(ws.list.iter())
        .filter(|(w, s)| w.kind == WeaponKind::Direct && s.target != 0 && s.blocked)
        .filter_map(|(w, s)| census.get(s.target).map(|c| (w, c)))
        .collect();
    if jobs.is_empty() {
        return None;
    }
    let h = df.height() * 0.8;
    let mut cands: Vec<V2> = keep.into_iter().collect();
    for (ri, r) in [3.0f32, 6.0, 10.0, 15.0, 20.0, 26.0].into_iter().enumerate() {
        let n = 8 + ri * 4;
        for k in 0..n {
            cands.push(pos + V2::from_angle(k as f32 / n as f32 * std::f32::consts::TAU + ri as f32 * 0.4) * r);
        }
    }
    cands.into_iter().find(|&spot| {
        if map.clamp(spot) != spot || nav.at(map, spot) != 0 || map.h(spot) < map.water_at(spot) + 0.4 {
            return false;
        }
        let origin = spot.at(map.h(spot) + h);
        jobs.iter().all(|(w, c)| {
            let d = c.pos.dist(spot);
            d <= w.range * 0.95 && d >= w.min_range && lane.aim(w, origin, c).is_some()
        })
    })
}

// **Infantry** (`"categories": ["infantry"]`, `UnitDef::is_infantry`: Trooper, AT/SAM Trooper): idle or
// engaged on attack-move/patrol, `think` sends them to `cover_spot`: behind nearest tree/rock/wreck (at
// least 0.7 tall) on far side from closest known armed enemy within `cover_threat_range`, within
// `cover_search` of `Mover.anchor` (0.6x of it around themselves when fighting). Re-chosen every 5 ticks,
// kept in `Mover.cover`; new order or Stop clears it (Stop also moves anchor to where they stand).
// Field works are cover for every side (finished ones belong to `NEUTRAL`, `build_progress`): anybody's sandbags soak shots from anybody (`soak_works`, except within
// `cover_grace` of shooter), infantry hide behind any sandbags; any trench shelters whoever stands in it.
// Hovering transport or garrison in sight shows slots taken (`hud_map.slots_taken`).
// Right-clicking trench piece no enemy infantry hold (`hud::Frame::trench_spots`) spreads selected infantry
// over whole joined trench, one per piece (`hud::spread`: evenly when fewer men than pieces, pieces our other
// men hold left to them); rest of selection moves to click.
fn cover_spot(defs: &Defs, census: &Census, features: &Features, wrecks: &WreckList, map: &MapData, nav: &Nav, id: u32, team: u8, df: &crate::data::UnitDef, pos: V2, anchor: V2, search: f32) -> Option<V2> {
    let rules = &defs.rules;
    let mut threat: Option<(f32, V2)> = None;
    census.near(pos, rules.cover_threat_range, |c| {
        if c.air || c.hp <= 0.0 || !hostile(team, c.team) || !c.known_to(team) {
            return;
        }
        // Infantry inside a building shoot from it.
        if c.hosted && !census.get(c.host).map_or(false, |h| h.structure) {
            return;
        }
        if !defs.get(c.def).weapons.iter().any(|w| w.hits_land) {
            return;
        }
        let d = c.pos.dist(pos);
        if threat.map_or(true, |t| d < t.0) {
            threat = Some((d, c.pos));
        }
    });
    let me = df.radius();
    // A trench piece near by is the best place there is, whichever way the enemy comes from
    // (and with no enemy about, too): one soldier per piece, the nearest free one.
    let mut trench: Option<(f32, V2)> = None;
    census.near(anchor, search + 1.0, |c| {
        if !c.fieldwork || !c.done || c.hp <= 0.0 || defs.get(c.def).work().map_or(true, |w| w.1 <= 0.0) || c.pos.dist(anchor) > search {
            return;
        }
        let mut taken = false;
        census.near(c.pos, 2.0, |o| {
            if o.id != id && !o.air && !o.hosted && !o.structure && in_cell(o.pos, c.pos) {
                taken = true;
            }
        });
        let score = c.pos.dist(pos) + c.pos.dist(anchor) * 0.5;
        if !taken && trench.map_or(true, |b| score < b.0) {
            trench = Some((score, c.pos));
        }
    });
    if let Some(t) = trench {
        return Some(t.1);
    }
    let (_, enemy) = threat?;
    // Too low to hide behind (bushes, small stones).
    let low = (df.height() * 0.45).max(0.7);
    let mut best: Option<(f32, V2)> = None;
    let mut consider = |at: V2, r: f32, h: f32| {
        if h < low || at.dist(enemy) < r + 1.0 {
            return;
        }
        let spot = at + (at - enemy).norm() * (r * 0.8 + me + 0.2);
        if spot.dist(anchor) > search || nav.at(map, spot) != 0 {
            return;
        }
        let mut taken = false;
        census.near(spot, 2.0, |o| {
            if o.id != id && o.team == team && !o.air && !o.hosted && o.pos.dist(spot) < me + o.radius * 0.8 {
                taken = true;
            }
        });
        if taken {
            return;
        }
        let score = spot.dist(pos) + spot.dist(anchor) * 0.5;
        if best.map_or(true, |b| score < b.0) {
            best = Some((score, spot));
        }
    };
    features.near(anchor, search + 3.0, |_, s| {
        if rules.features.get(s.def as usize).map_or(false, |d| d.cover() > 0.05) {
            consider(s.pos, s.radius, s.height);
        }
    });
    // Sandbags (anybody's: they shelter every side).
    census.near(anchor, search + 3.0, |c| {
        if c.fieldwork && c.done && c.hp > 0.0 && defs.get(c.def).work().map_or(false, |w| w.0 > 0.05 && w.1 <= 0.0) {
            consider(c.pos, c.radius, c.height);
        }
    });
    if rules.wreck_cover > 0.05 {
        for &(_, wp, mass, wd, piece, _) in &wrecks.0 {
            // Debris too small to hide behind does not count.
            if mass > 0.0 && wp.dist(anchor) <= search + 3.0 && (piece <= 0.0 || piece >= rules.debris.sight_size) {
                let (r, top) = Wreck::shape(defs, wd, piece);
                consider(wp, r, top);
            }
        }
    }
    best.map(|b| b.1)
}

/// Is `p` on the cell whose centre is `cell`?
fn in_cell(p: V2, cell: V2) -> bool {
    (p.x - cell.x).abs() <= CELL * 0.5 && (p.y - cell.y).abs() <= CELL * 0.5
}

/// The trench piece a unit stands in, if it is infantry on foot: (trench id, cover share, centre).
fn trench_of(census: &Census, defs: &Defs, c: &Cen) -> Option<(u32, f32, V2)> {
    if c.air || c.structure || c.hosted || !defs.get(c.def).is_infantry() {
        return None;
    }
    let mut best: Option<(u32, f32, V2)> = None;
    census.near(c.pos, CELL, |t| {
        if t.fieldwork && t.done && t.hp > 0.0 && in_cell(c.pos, t.pos) && best.map_or(true, |b| t.id < b.0) {
            if let Some((cover, _)) = defs.get(t.def).work().filter(|w| w.1 > 0.0) {
                best = Some((t.id, cover, t.pos));
            }
        }
    });
    best
}

// **Storming**: `Order::Load` on building enemy infantry hold (right-click with infantry selected; `hud::Frame::stormers`)
// sends infantry with `assault` > 0 (unit JSON: close-quarters dps; Trooper 10, AT 3, SAM 2.5) to its door (`Order::Attack` on such building
// becomes that `Load` for them in `apply_commands`, `systems::storms`; other units still shoot it), where `think`
// queues them in `Effects.assault` and `systems::assaults` trades damage: attackers' summed assault -> lowest-id
// defender, defenders' summed assault x `rules.garrison_defense` (1.25) -> lowest-id attacker. Defenders of stormed
// building (`Effects.stormed`) do not fire out. When nobody is left inside, `may_board` lets them in (map building
// whose riders all died counts as empty before `garrisons` makes it neutral); emptied `HOSTILE` building keeps
// owner and cannot be entered (with infantry capture on, armed one is captured instead). Riders die inside at hp <= 0 (no wreck);
// death blasts skip hosted units. `--combattest` "Storm" lines (1 trooper beats 1 AT inside, even trooper fight goes to defender).
/// The fight for an occupied building or trench piece: infantry at its door or edge (queued by
/// `think` in `Effects.assault`) and the enemy infantry inside trade close-quarters damage (a
/// trench's cover does not soften it), `assault` per second each (the
/// defenders `rules.garrison_defense` times theirs). Each side's damage goes to the other side's
/// lowest-id unit, so it falls one at a time. When nobody is left inside the attackers walk in
/// (`may_board`). Attackers of several teams at one building each fight the defenders on their own.
fn assaults(census: Res<Census>, defs: Res<Defs>, mut effects: ResMut<Effects>) {
    // (building, attacking team) -> attackers, in id order.
    let mut fights: BTreeMap<(u32, u8), Vec<&Cen>> = BTreeMap::new();
    effects.stormed.clear();
    let mut list = std::mem::take(&mut effects.assault);
    list.sort_unstable();
    list.dedup();
    for (a, b) in list {
        if let Some(c) = census.get(a).filter(|c| c.hp > 0.0) {
            fights.entry((b, c.team)).or_default().push(c);
        }
    }
    for ((bid, team), attackers) in fights {
        // Defenders: the enemy infantry inside a building, or standing in a trench piece.
        let inside: Vec<&Cen> = match census.get(bid).filter(|t| is_trench(&defs, t)) {
            Some(piece) => trench_holders(&census, &defs, piece, team).into_iter().filter_map(|id| census.get(id)).collect(),
            None => census.list.iter().filter(|r| r.host == bid && r.done && r.hp > 0.0 && hostile(team, r.team)).collect(),
        };
        let (Some(a0), Some(d0)) = (attackers.first(), inside.first()) else { continue };
        let hit: f32 = attackers.iter().map(|c| defs.get(c.def).assault).sum::<f32>() * DT;
        let back: f32 = inside.iter().map(|c| defs.get(c.def).assault).sum::<f32>() * DT * defs.rules.garrison_defense;
        effects.stormed.push(bid);
        if hit > 0.0 {
            effects.damage.push((d0.id, hit, 0));
        }
        if back > 0.0 {
            effects.damage.push((a0.id, back, 0));
        }
    }
}

// **Garrisons**: structure with `transport` block (`UnitDef::is_garrison`; most civilian houses, civic and
// industrial buildings, bunker) takes infantry only (`Order::Load(building)`, right-click neutral or own
// building). Riders keep `Hosted`, can't be targeted, fire from roof (`weapons` takes hosted riders of a
// garrison, origin at building top) and see from it (`visibility`, `intel.rs`), vision and binocular
// range x `rules.garrison_vision` (1.3). Every hit on building also costs each rider
// `rules.garrison_damage` (0.04) x raw damage, their armor applied (`deaths`). `systems::garrisons`
// gives garrison to owner of its lowest-id rider and back to `NEUTRAL` when empty - map buildings, field
// bunkers and watch towers alike (requested: act like map's buildings, entered when empty, stormed when
// held); `HOSTILE` ones keep owner, and with infantry capture on so do armed ones (map's bunker).
// `Unload` from building sets them down at its side and walks them to spot. Move-type orders (move,
// attack-move, patrol) given to riders take them out of side facing goal first
// (`apply_commands`, `garrison_door`); HUD sends selected own building's move orders to its riders
// (`hud::Frame::garrison_riders`), so right-clicking ground with occupied house selected empties it there. When building dies,
// `deaths` throws riders out with `rules.garrison_eject_damage` x max hp damage (none if reclaimed);
// riders of mobile transport still die with it.
/// Occupied buildings (the map's and field bunkers / watch towers alike) belong to the side inside
/// (they can be shot at and the side sees from them); once empty they are neutral again. The
/// hostile side's keep their owner, and so, with infantry capture on, do armed ones (bunkers).
fn garrisons(census: Res<Census>, effects: Res<Effects>, defs: Res<Defs>, game: Res<Game>, mut q: Query<(&mut Unit, &mut Orders), With<Structure>>) {
    // Host -> owner of its lowest-id rider (the census is in id order).
    let mut inside: BTreeMap<u32, u8> = BTreeMap::new();
    for c in &census.list {
        if c.hosted && c.done && c.hp > 0.0 {
            inside.entry(c.host).or_insert(c.owner);
        }
    }
    for (rider, host) in &effects.loaded {
        if let Some(r) = census.get(*rider) {
            inside.entry(*host).or_insert(r.owner);
        }
    }
    for c in &census.list {
        let d = defs.get(c.def);
        // A site still being built has no riders yet: it stays its builder's, or the builders
        // (who only work on their own team's sites) would be shut out of it and it would hang.
        if !d.is_garrison() || !c.done || c.hp <= 0.0 || c.owner == HOSTILE {
            continue;
        }
        let want = inside.get(&c.id).copied().unwrap_or(NEUTRAL);
        // With infantry capture on, an armed one (a bunker) is a turret too: taken over, or
        // left by its riders, it keeps the side that has it.
        if want != c.owner && !(want == NEUTRAL && d.armed() && game.infantry_capture) {
            if let Ok((mut u, mut o)) = q.get_mut(c.e) {
                u.owner = want;
                o.0.clear();
            }
        }
    }
}

/// Turns a wreck into a construction site of `owner` that already holds the wreck's mass;
/// finishing it costs the rest, like any building work. Returns the new unit's id.
#[allow(clippy::too_many_arguments)]
fn revive(commands: &mut Commands, defs: &Defs, game: &mut Game, map: &MapData, nav: &mut Nav, effects: &mut Effects, w: (u32, V2, f32, DefId, f32, u32), owner: u8) -> u32 {
    let wd = defs.get(w.3);
    let built = (w.2 / wd.cost.mass.max(1.0)).clamp(0.05, 0.95);
    let sid = spawn_site(commands, defs, game, map, w.3, owner, w.1, PI * 0.5, Some(built), 0, w.0);
    if wd.is_structure() {
        nav.stamp(map, w.1, wd.footprint, 2);
    }
    effects.reclaim.push((w.0, 1.0e9, u8::MAX));
    effects.revived.push((w.0, sid));
    sid
}

/// The side of a factory at `pos` that land units leave by: the one facing the rally point (unit vector
/// along an axis); `+z` without a rally point. The view opens the door on the same side.
pub fn exit_side(pos: V2, rally: Option<V2>) -> V2 {
    let Some(r) = rally else { return v2(0.0, 1.0) };
    let (dx, dz) = (r.x - pos.x, r.y - pos.y);
    if dx.abs() > dz.abs() {
        v2(if dx > 0.0 { 1.0 } else { -1.0 }, 0.0)
    } else if dz < 0.0 {
        v2(0.0, -1.0)
    } else {
        v2(0.0, 1.0)
    }
}

#[allow(clippy::type_complexity)]
fn factories(
    mut q: Query<(&Unit, &Body, &mut Factory, &mut Builder, Has<Upgrading>), Without<UnderConstruction>>,
    census: Res<Census>,
    defs: Res<Defs>,
    map: Res<MapData>,
    nav: Res<Nav>,
    mut game: ResMut<Game>,
    mut commands: Commands,
) {
    for (u, body, mut f, mut builder, upgrading) in q.iter_mut().sort_by_key::<&Unit, _>(|u| u.id) {
        let df = defs.get(u.def);
        builder.work = 0;
        let rally0 = f.rally;
        // Sends a finished unit out of the factory to the rally point.
        let release = |p: &Cen, commands: &mut Commands| {
            let pd = defs.get(p.def);
            // Land units leave by the side facing the rally point; aircraft stay as they were.
            let side = if pd.is_air() { v2(0.0, 1.0) } else { exit_side(body.pos, rally0) };
            let yaw = if side.x > 0.5 { 0.0 } else if side.x < -0.5 { PI } else if side.y > 0.0 { PI * 0.5 } else { PI * 1.5 };
            let mut exit = body.pos + side * (df.footprint as f32 * CELL * 0.5 + 1.5);
            if !pd.is_air() && nav.at(&map, exit) != 0 {
                let (cx, cy) = map.cell_of(exit);
                if let Some((x, y)) = nearest_free(&nav.occ, map.dims(), cx, cy) {
                    exit = v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL);
                }
            }
            let mut rally = rally0.unwrap_or(exit + side * 6.0);
            // The default rally point can fall on a building next to the factory (an
            // extractor): units then pushed against it forever and the AI never gave
            // them new orders. Move it to the nearest free cell like the exit.
            if rally0.is_none() && !pd.is_air() && nav.at(&map, rally) != 0 {
                let (cx, cy) = map.cell_of(rally);
                if let Some((x, y)) = nearest_free(&nav.occ, map.dims(), cx, cy) {
                    rally = v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL);
                }
            }
            let pos = if pd.is_air() { body.pos } else { exit };
            commands.entity(p.e).remove::<Hosted>().insert((
                Body { pos, y: map.h(pos) + pd.mv.altitude, yaw: if pd.is_air() { PI * 0.5 } else { yaw }, vel: v2(0.0, 0.0) },
                Orders(vec![Order::Move(rally)]),
                Mover { anchor: rally, path_goal: pos, ..default() },
            ));
        };
        if f.building != 0 {
            match census.get(f.building) {
                None => f.building = 0,
                Some(p) if p.done => {
                    f.building = 0;
                    if f.carrier != 0 && p.id != f.carrier {
                        // A rider for the transport being filled: in it goes.
                        commands.entity(p.e).insert(Hosted(f.carrier));
                        if !f.cargo.is_empty() {
                            f.cargo.remove(0);
                        }
                    } else {
                        let job = (!f.queue.is_empty()).then(|| f.queue.remove(0));
                        if let (Some(j), true) = (&job, f.repeat) {
                            f.queue.push(j.clone());
                        }
                        match job.filter(|j| j.def == p.def && !j.cargo.is_empty()) {
                            // A transport with a loadout stays inside until its riders are built.
                            Some(j) => {
                                f.carrier = p.id;
                                f.cargo = j.cargo;
                            }
                            None => release(p, &mut commands),
                        }
                    }
                }
                Some(_) => builder.work = f.building,
            }
        }
        if f.carrier != 0 && f.building == 0 && !upgrading {
            match census.get(f.carrier) {
                None => {
                    f.carrier = 0;
                    f.cargo.clear();
                }
                Some(c) => {
                    let next = f.cargo.first().copied().filter(|d| df.builds.contains(d) && fits(defs.get(c.def), defs.get(*d)));
                    if let Some(d) = next {
                        let id = spawn_unit(&mut commands, &defs, &mut game, &map, d, u.owner, body.pos, PI * 0.5, false, u.id);
                        f.building = id;
                        builder.work = id;
                    } else {
                        release(c, &mut commands);
                        f.carrier = 0;
                        f.cargo.clear();
                    }
                }
            }
        }
        if f.building == 0 && f.carrier == 0 && !upgrading {
            if let Some(d) = f.queue.first().map(|j| j.def) {
                if df.builds.contains(&d) {
                    let id = spawn_unit(&mut commands, &defs, &mut game, &map, d, u.owner, body.pos, PI * 0.5, false, u.id);
                    f.building = id;
                    builder.work = id;
                } else {
                    f.queue.remove(0);
                }
            }
        }
    }
}

// **Energy stall**: upkeep and construction share energy proportionally (`build_progress`, `Player.power`);
// pumps produce at that fraction, radars cut out below 0.5. `Paused` (Tab / `Command::Pause`) switches a
// unit's build work, production, upkeep and radar off. An earlier version subtracted upkeep first, which
// dead-locked base whose upkeep exceeded income.
/// Flow economy: every builder requests resources for its job; if a player cannot pay,
/// all of that player's jobs slow down together.
#[allow(clippy::type_complexity)]
fn build_progress(
    mut q: Query<(Entity, &mut Unit, &mut Health, Option<&Builder>, Option<&mut UnderConstruction>, Option<&mut Upgrading>)>,
    census: Res<Census>,
    defs: Res<Defs>,
    effects: Res<Effects>,
    mut game: ResMut<Game>,
    mut reactive: Query<&mut Reactive>,
    mut commands: Commands,
) {
    // Units that heal by themselves (`regen`): finished and still alive.
    for (_, u, mut h, _, uc, _) in q.iter_mut() {
        let d = defs.get(u.def);
        if d.regen > 0.0 && uc.is_none() && h.0 > 0.0 && h.0 < d.hp {
            h.0 = (h.0 + d.regen * DT).min(d.hp);
        }
    }
    // (payer, target entity, build rate)
    let mut jobs: Vec<(u8, Entity, f32)> = vec![];
    for (e, u, _, builder, uc, up) in q.iter().sort_by_key::<&Unit, _>(|u| u.id) {
        if uc.is_some() || census.get(u.id).map_or(false, |c| c.paused) {
            continue;
        }
        let d = defs.get(u.def);
        if up.is_some() {
            jobs.push((u.owner, e, d.build_rate().max(5.0)));
        }
        if let Some(b) = builder {
            // A building this player is taking apart is not repaired at the same time.
            if effects.reclaim_unit.iter().any(|r| r.0 == b.work && r.2 == u.owner) {
                continue;
            }
            if let Some(t) = census.get(b.work) {
                // Only scavengers build what is being revived. `work` is also the unit being
                // captured: nobody builds, upgrades or repairs for another team.
                if t.e != e && t.team == game.team_of(u.owner) && (!t.reviving() || d.is_scavenger()) {
                    jobs.push((u.owner, t.e, d.build_rate()));
                }
            }
        }
    }
    // What does this target currently need? 0 = build, 1 = upgrade, 2 = repair.
    let kind_of = |u: &Unit, hp: f32, uc: bool, up: bool| -> Option<(u8, f32, f32, f32)> {
        let d = defs.get(u.def);
        if uc {
            Some((0, d.cost.mass, d.cost.energy, d.cost.time))
        } else if up {
            let t = defs.get(d.upgrade_id?);
            Some((1, t.cost.mass, t.cost.energy, t.cost.time))
        } else if hp < d.hp {
            Some((2, 0.0, 0.0, 1.0))
        } else {
            None
        }
    };
    let np = game.players.len();
    let (mut dm, mut de) = (vec![0.0f32; np], vec![0.0f32; np]);
    for &(payer, te, rate) in &jobs {
        let Ok((_, u, h, _, uc, up)) = q.get(te) else { continue };
        if let (Some((_, m, e, t)), true) = (kind_of(u, h.0, uc.is_some(), up.is_some()), (payer as usize) < np) {
            dm[payer as usize] += m * rate / t;
            de[payer as usize] += e * rate / t;
        }
    }
    let mut ratio = vec![1.0f32; np];
    let mut power = vec![1.0f32; np];
    for p in 0..np {
        let pl = &game.players[p];
        if dm[p] > 0.0 {
            ratio[p] = ratio[p].min(pl.mass.max(0.0) / (dm[p] * DT));
        }
        // Energy is shared between construction and upkeep.
        let want_e = de[p] + pl.upkeep;
        if want_e > 0.0 {
            power[p] = (pl.energy.max(0.0) / (want_e * DT)).clamp(0.0, 1.0);
            ratio[p] = ratio[p].min(power[p]);
        }
        ratio[p] = ratio[p].clamp(0.0, 1.0);
    }
    for &(payer, te, rate) in &jobs {
        let Ok((_, mut u, mut h, _, uc, up)) = q.get_mut(te) else { continue };
        // The state is re-read for every contribution, so a job that completed earlier in
        // this loop (e.g. an assisted upgrade) is never advanced twice.
        let Some((kind, m, e, t)) = kind_of(&u, h.0, uc.is_some(), up.is_some()) else { continue };
        let max_hp = defs.get(u.def).hp;
        if kind == 2 {
            h.0 = (h.0 + rate * defs.rules.repair_rate * DT).min(max_hp);
            // Repaired to full: the reactive armor is replaced too.
            if let (true, Some(era), Ok(mut left)) = (h.0 >= max_hp, defs.get(u.def).reactive, reactive.get_mut(te)) {
                left.0 = era.charges;
            }
            continue;
        }
        let r = ratio.get(payer as usize).copied().unwrap_or(1.0);
        let cur = if kind == 0 { uc.as_ref().map_or(1.0, |x| x.0) } else { up.as_ref().map_or(1.0, |x| x.0) };
        let prog = (rate * DT * r / t).min(1.0 - cur).max(0.0);
        if let Some(pl) = game.players.get_mut(payer as usize) {
            pl.mass -= m * prog;
            pl.energy -= e * prog;
            pl.spend_m += m * prog / DT;
            pl.spend_e += e * prog / DT;
        }
        if kind == 0 {
            let mut uc = uc.unwrap();
            uc.0 += prog;
            h.0 = (h.0 + prog * max_hp).min(max_hp);
            if uc.0 >= 0.9999 {
                // `Revival` stays: its wreck, should it die, cannot be revived again at once.
                commands.entity(te).remove::<UnderConstruction>();
                // Marked complete right away so later contributions in this tick see it.
                uc.0 = 2.0;
                // Field works (sandbags, trenches) belong to nobody once built: cover for every side.
                if defs.get(u.def).is_fieldwork() {
                    u.owner = NEUTRAL;
                }
            }
        } else {
            let mut up = up.unwrap();
            up.0 += prog;
            if up.0 >= 0.9999 {
                if let Some(to) = defs.get(u.def).upgrade_id {
                    let frac = h.0 / max_hp;
                    u.def = to;
                    h.0 = frac * defs.get(to).hp;
                }
                commands.entity(te).remove::<Upgrading>();
                up.0 = -1000.0;
            }
        }
    }
    for (p, pl) in game.players.iter_mut().enumerate() {
        pl.energy -= pl.upkeep * power[p] * DT;
        pl.spend_e += pl.upkeep * power[p];
        // Show what was asked for, so a deficit is visible. Each resource only where it is the
        // bottleneck: with energy to spare and mass short, construction slows and the energy
        // really spent is less than asked, so the stored energy does not fall by the demand.
        if ratio[p] < 1.0 {
            pl.spend_m = dm[p];
        }
        if power[p] < 1.0 {
            pl.spend_e = de[p] + pl.upkeep;
        }
        pl.power = power[p];
        // Radars drop out when less than half of the energy demand is met.
        pl.power_out = power[p] < 0.5;
        pl.mass = pl.mass.clamp(0.0, pl.max_mass.max(1.0));
        pl.energy = pl.energy.clamp(0.0, pl.max_energy.max(1.0));
    }
}

// **Movement**: land units accelerate/brake and choose between driving forward and reversing by comparing
// turn time + travel time for both (`systems.rs::locomotion`).
// Orders given with Ctrl held (`hud`: any non-build order, not Ctrl+key shortcut) are followed by `Command::Pace`:
// land and hover units in it get `Mover.pace` / `pace_accel` = slowest one's speed and acceleration, cleared by a
// new order, Stop or running out of orders. `--combattest` (`LR_TEST=apc`) "Pace" line.
#[allow(clippy::type_complexity)]
fn locomotion(
    mut q: Query<(&Unit, &mut Body, &mut Mover, &mut Orders), (Without<UnderConstruction>, Without<Hosted>)>,
    defs: Res<Defs>,
    map: Res<MapData>,
    mut nav: ResMut<Nav>,
    game: Res<Game>,
) {
    let (world, world_z) = (map.world_x(), map.world_z());
    // Mud and snow slow land units down.
    let wx_speed = defs.rules.weather_of(game.weather).speed;
    let n = map.nx as i32;
    let needs_path = |df: &crate::data::UnitDef, m: &Mover| -> Option<V2> {
        let g = m.goal?;
        (df.mv.kind == MoveKind::Land && m.hold <= 0.0 && (m.path.is_empty() || m.path_goal.dist(g) > 3.0)).then_some(g)
    };
    // Path searches (A*, the slowest thing a tick can do) are planned first: the first `paths_left`
    // land units in id order that need a new path, exactly those the loop below would serve. They
    // only read the occupancy grid, which nothing changes in here, so they run in parallel.
    let mut wanted: Vec<(u32, V2, V2, bool)> = vec![];
    for (u, body, m, _) in q.iter().sort_by_key::<&Unit, _>(|u| u.id) {
        if wanted.len() as i32 >= nav.paths_left {
            break;
        }
        let df = defs.get(u.def);
        if let Some(g) = needs_path(df, m) {
            wanted.push((u.id, body.pos, g, df.mv.amphibious));
        }
    }
    nav.paths_left -= wanted.len() as i32;
    let occ = &nav.occ;
    let found = par_map(&wanted, 1, |&(_, from, to, amph)| {
        thread_local!(static SCRATCH: std::cell::RefCell<PathScratch> = Default::default());
        SCRATCH.with(|sc| find_path(occ, map.dims(), from, to, amph, &mut sc.borrow_mut()))
    });
    let mut paths: Vec<(u32, Vec<V2>)> = wanted.iter().map(|w| w.0).zip(found).collect();
    paths.reverse();
    for (u, mut body, mut m, mut orders) in q.iter_mut().sort_by_key::<&Unit, _>(|u| u.id) {
        let df = defs.get(u.def);
        let mv = &df.mv;
        let old = body.pos;
        let turn = mv.turn.to_radians();
        if orders.0.is_empty() {
            // Moving in step ends with the orders it was given for.
            m.pace = 0.0;
            m.pace_accel = 0.0;
        }
        if m.drop > 0.0 {
            // An airdrop fall: glides down onto its spot and does nothing else until it lands.
            m.drop = (m.drop - DT).max(0.0);
            let t = 1.0 - m.drop / DROP_SECS;
            body.pos = m.drop_from + (m.drop_to - m.drop_from) * t;
            let ground = if mv.amphibious { map.surface(body.pos) } else { map.h(body.pos) };
            body.y = ground + m.drop_h * (1.0 - t * t);
            m.speed = 0.0;
            m.path.clear();
            body.vel = v2(0.0, 0.0);
            continue;
        }
        if m.hold > 0.0 && mv.kind != MoveKind::Plane {
            // Set up to fire: stays where it is, hull and all, until the hold runs out.
            m.hold = (m.hold - DT).max(0.0);
            m.speed = 0.0;
            body.vel = v2(0.0, 0.0);
            continue;
        }
        match mv.kind {
            MoveKind::None => continue,
            MoveKind::Plane => {
                let mut desired = body.yaw;
                if m.extend > 0.0 {
                    m.extend -= DT;
                } else if let Some(g) = m.goal {
                    desired = (g - body.pos).angle();
                    let off = wrap(desired - body.yaw).abs();
                    let turn_radius = mv.speed / turn.max(0.1);
                    // Bombers need a straight run-in at least as long as their bombs carry
                    // forward: with the target behind them they fly on until there is room.
                    let throw = if df.weapons.iter().any(|w| w.kind == WeaponKind::Bomb) { mv.speed * (2.0 * mv.altitude.max(1.0) / defs.rules.gravity).sqrt() } else { 0.0 };
                    if throw > 0.0 && off > 1.0 && body.pos.dist(g) < throw + turn_radius * 0.6 {
                        desired = body.yaw;
                    } else if body.pos.dist(g) < turn_radius * 1.1 && off > 1.0 {
                        // Too close to turn onto it: fly out and come back round.
                        m.extend = 2.0 / turn.max(0.1);
                        desired = body.yaw;
                    }
                } else if body.pos.dist(m.anchor) > mv.speed / turn.max(0.1) * 1.5 {
                    desired = (m.anchor - body.pos).angle();
                } else {
                    desired = body.yaw + 1.0;
                }
                if body.pos.x < 3.0 || body.pos.y < 3.0 || body.pos.x > world - 3.0 || body.pos.y > world_z - 3.0 {
                    desired = (map.center() - body.pos).angle();
                    m.extend = 0.0;
                }
                body.yaw = turn_toward(body.yaw, desired, turn * DT);
                body.pos = body.pos + V2::from_angle(body.yaw) * (mv.speed * DT);
                let want = map.surface(body.pos) + mv.altitude;
                body.y += (want - body.y).clamp(-5.0 * DT, 5.0 * DT);
            }
            MoveKind::Hover => {
                let mut want_speed = 0.0;
                if let Some(g) = m.goal {
                    let to = g - body.pos;
                    let dist = to.len();
                    if dist > 0.4 {
                        body.yaw = turn_toward(body.yaw, to.angle(), turn * DT);
                        let top = if m.pace > 0.0 { mv.speed.min(m.pace) } else { mv.speed };
                        want_speed = top.min((2.0 * mv.accel * dist).sqrt() + 0.5);
                        let accel = if m.pace_accel > 0.0 { mv.accel.min(m.pace_accel) } else { mv.accel };
                        m.speed += (want_speed - m.speed).clamp(-mv.accel * 2.0 * DT, accel * DT);
                        body.pos = body.pos + to.norm() * (m.speed * DT).min(dist);
                    }
                }
                if want_speed == 0.0 {
                    m.speed = 0.0;
                }
                let want = map.surface(body.pos) + mv.altitude;
                body.y += (want - body.y).clamp(-4.0 * DT, 4.0 * DT);
            }
            MoveKind::Land => {
                let Some(g) = m.goal else {
                    m.path.clear();
                    m.stuck = 0;
                    m.speed = 0.0;
                    body.vel = v2(0.0, 0.0);
                    body.y = if mv.amphibious { map.surface(body.pos) } else { map.h(body.pos) };
                    continue;
                };
                if m.path.is_empty() || m.path_goal.dist(g) > 3.0 {
                    // Planned above in the same id order; no path left in this tick's budget: wait.
                    if paths.last().map_or(true, |p| p.0 != u.id) {
                        continue;
                    }
                    m.path = paths.pop().unwrap().1;
                    m.path_goal = g;
                }
                let occ = &nav.occ;
                let blocked = |p: V2| -> bool {
                    let (cx, cy) = ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32);
                    !map.inside(cx, cy) || !crate::map::passable_for(occ[(cy * n + cx) as usize], mv.amphibious)
                };
                while m.path.len() > 1 && body.pos.dist(*m.path.last().unwrap()) < 1.0 {
                    m.path.pop();
                }
                let wp = *m.path.last().unwrap();
                let to = wp - body.pos;
                let dist = to.len();
                if dist < 0.03 {
                    m.speed = 0.0;
                    continue;
                }
                // Distance left along the whole path, for braking and the reverse decision.
                let mut remaining = dist;
                for w in m.path.windows(2) {
                    remaining += w[0].dist(w[1]);
                }
                let desired = to.angle();
                let diff_f = wrap(desired - body.yaw).abs();
                let diff_r = PI - diff_f;
                // Reverse when backing up costs less time than swinging the hull around.
                if mv.reverse > 0.1 {
                    let cost_f = diff_f / turn.max(0.05) + remaining / mv.speed.max(0.1);
                    let cost_r = diff_r / turn.max(0.05) + remaining / mv.reverse;
                    if m.reversing {
                        if cost_f * 1.2 < cost_r {
                            m.reversing = false;
                        }
                    } else if cost_r * 1.2 < cost_f {
                        m.reversing = true;
                    }
                } else {
                    m.reversing = false;
                }
                let face = if m.reversing { wrap(desired + PI) } else { desired };
                body.yaw = turn_toward(body.yaw, face, turn * DT);
                let off = wrap(face - body.yaw).abs();
                let mut top = if m.reversing { mv.reverse } else { mv.speed } * wx_speed;
                if m.pace > 0.0 {
                    top = top.min(m.pace * wx_speed);
                }
                let mut want = if off < 0.35 {
                    top
                } else if off < 1.0 {
                    top * 0.4
                } else {
                    0.0
                };
                let brake = mv.accel * 2.5;
                want = want.min((2.0 * brake * remaining).sqrt() + 0.4);
                let accel = if m.pace_accel > 0.0 { mv.accel.min(m.pace_accel) } else { mv.accel };
                m.speed += (want - m.speed).clamp(-brake * DT, accel * DT);
                let step = (m.speed * DT).min(dist).max(0.0);
                let dir = V2::from_angle(body.yaw) * if m.reversing { -1.0 } else { 1.0 };
                let np = body.pos + dir * step;
                let inside = blocked(body.pos);
                if inside || !blocked(np) {
                    body.pos = np;
                } else if !blocked(v2(np.x, body.pos.y)) {
                    body.pos = v2(np.x, body.pos.y);
                } else if !blocked(v2(body.pos.x, np.y)) {
                    body.pos = v2(body.pos.x, np.y);
                } else {
                    m.speed = 0.0;
                }
                if want > 0.3 && body.pos.dist(old) < top * DT * 0.15 && m.speed > top * 0.5 {
                    m.stuck += 1;
                    if m.stuck % 25 == 24 {
                        m.path.clear();
                    }
                    if m.stuck > 100 {
                        m.stuck = 0;
                        if !orders.0.is_empty() {
                            orders.0.remove(0);
                        }
                    }
                } else if m.stuck > 0 {
                    m.stuck -= 1;
                }
                // Amphibious units float on the water, others stand on the ground.
                body.y = if mv.amphibious { map.surface(body.pos) } else { map.h(body.pos) };
            }
        }
        body.pos = v2(body.pos.x.clamp(0.5, world - 0.5), body.pos.y.clamp(0.5, world_z - 0.5));
        body.vel = (body.pos - old) * (1.0 / DT);
    }
    debug_assert!(paths.is_empty(), "locomotion planned paths it did not use");
}

/// Pushes overlapping mobile units apart.
#[allow(clippy::type_complexity)]
fn separation(mut q: Query<(Entity, &Unit, &mut Body, &Mover), (Without<UnderConstruction>, Without<Hosted>)>, defs: Res<Defs>, map: Res<MapData>, nav: Res<Nav>) {
    const G: f32 = 3.0;
    struct It {
        e: Entity,
        pos: V2,
        r: f32,
        air: bool,
        amph: bool,
        moving: bool,
    }
    let mut items: Vec<It> = vec![];
    let mut grid: HashMap<(i32, i32, bool), Vec<usize>> = HashMap::new();
    for (e, u, b, m) in q.iter().sort_by_key::<&Unit, _>(|u| u.id) {
        let d = defs.get(u.def);
        if d.is_plane() {
            continue;
        }
        grid.entry(((b.pos.x / G) as i32, (b.pos.y / G) as i32, d.is_air())).or_default().push(items.len());
        items.push(It { e, pos: b.pos, r: d.radius(), air: d.is_air(), amph: d.mv.amphibious, moving: m.goal.is_some() });
    }
    for (i, a) in items.iter().enumerate() {
        let mut acc = v2(0.0, 0.0);
        let (gx, gy) = ((a.pos.x / G) as i32, (a.pos.y / G) as i32);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let Some(list) = grid.get(&(gx + dx, gy + dy, a.air)) else { continue };
                for &j in list {
                    if i == j {
                        continue;
                    }
                    let b = &items[j];
                    let d = a.pos - b.pos;
                    let min = a.r + b.r + 0.15;
                    let l = d.len();
                    if l < min {
                        let dir = if l > 0.01 { d * (1.0 / l) } else { V2::from_angle(i as f32 * 2.4) };
                        let w = if a.moving && !b.moving { 0.2 } else { 0.5 };
                        acc = acc + dir * ((min - l) * w);
                    }
                }
            }
        }
        if acc.len2() > 0.0 {
            let push = if acc.len() > 0.5 { acc.norm() * 0.5 } else { acc };
            let np = map.clamp(a.pos + push);
            if a.air || nav.at(&map, np) == 0 || (a.amph && nav.at(&map, np) == crate::map::OCC_DEEP) {
                if let Ok((_, _, mut body, _)) = q.get_mut(a.e) {
                    body.pos = np;
                    if !a.air {
                        body.y = if a.amph { map.surface(np) } else { map.h(np) };
                    }
                }
            }
        }
    }
}

// **Ferry** (SupCom, `Order::Ferry { pos, pick }`, HUD key F): mobile transport's route is loop of `Ferry` orders
// rotated like patrol: `pick` one is beacon (where transports stood when ordered), one before it on
// loop drop point (set down there with inserted `Unload`), any between are waypoints (Shift adds points
// after drop point, `apply_commands` inserts them before beacon). Other units given `Ferry` (right-click a
// beacon, or F + click) wait at beacon (`Cen::ferry_wait`); transports there load them nearest first and leave
// when full or when nobody who fits is waiting within `FERRY_PICK` or still coming within `FERRY_COMING`. A waiting
// unit gives up when no own transport has that beacon (`Cen::beacon`; `apply_commands` refreshes both fields after
// new orders). `--combattest` "Ferry" line: 6 tanks across in 3 trips, then on to their queued goal.
/// Units riding in a transport (or waiting inside a factory) travel with it. They keep the rest of
/// their orders for when they are set down; only an order to board the transport they are in is done.
fn carry(census: Res<Census>, mut riders: Query<(&Hosted, &mut Body, &mut Orders)>, hosts: Query<&Body, Without<Hosted>>) {
    for (h, mut b, mut o) in &mut riders {
        if o.0.contains(&Order::Load(h.0)) {
            o.0.retain(|o| *o != Order::Load(h.0));
        }
        if let Some(Order::Ferry { .. }) = o.0.first() {
            o.0.remove(0);
        }
        if let Some(host) = census.get(h.0).and_then(|c| hosts.get(c.e).ok()) {
            b.pos = host.pos;
            b.y = host.y;
            b.vel = host.vel;
        }
    }
}

/// Launch solution for a ballistic shot: (pitch, flight time, muzzle speed).
/// `boost` lets indirect weapons raise their charge to reach the target.
pub fn ballistic(d: f32, h: f32, v: f32, g: f32, high: bool, boost: bool) -> Option<(f32, f32, f32)> {
    let d = d.max(0.3);
    let mut v = v.max(1.0);
    let mut disc = (v * v) * (v * v) - g * (g * d * d + 2.0 * h * v * v);
    if disc < 0.0 {
        if !boost {
            return None;
        }
        v = (g * (h + (h * h + d * d).sqrt())).max(1.0).sqrt() * 1.01;
        disc = ((v * v) * (v * v) - g * (g * d * d + 2.0 * h * v * v)).max(0.0);
    }
    let root = disc.sqrt();
    let tan = (v * v + if high { root } else { -root }) / (g * d);
    let pitch = fm::atan(tan);
    Some((pitch, d / (v * fm::cos(pitch)).max(0.01), v))
}

fn dir_from(yaw: f32, pitch: f32) -> Vec3 {
    let (cp, sp) = (fm::cos(pitch), fm::sin(pitch));
    Vec3::new(fm::cos(yaw) * cp, sp, fm::sin(yaw) * cp)
}

struct Solution {
    yaw: f32,
    pitch: f32,
    speed: f32,
    point: Vec3,
}

fn solve(w: &WeaponDef, origin: Vec3, target: Vec3, tvel: V2, g: f32) -> Solution {
    let mut point = target;
    let mut best = Solution { yaw: 0.0, pitch: 0.0, speed: w.speed, point };
    for _ in 0..3 {
        let to = point - origin;
        let d = Vec2::new(to.x, to.z).length();
        let yaw = fm::atan2(to.z, to.x);
        let (pitch, t, speed) = match w.kind {
            WeaponKind::Direct => ballistic(d, to.y, w.speed, g, false, false).unwrap_or((fm::atan2(to.y, d.max(0.1)), d / w.speed.max(1.0), w.speed)),
            WeaponKind::Arc => ballistic(d, to.y, w.speed, g, false, true).unwrap(),
            // Mortars and howitzers pick a charge for a steep (about 65 degree) arc instead of
            // always lobbing at full power, which would keep shells in the air for ages.
            WeaponKind::HighArc => ballistic(d, to.y, (g * d / 0.766).sqrt().clamp(8.0, w.speed * 1.5), g, true, true).unwrap(),
            // A suicide drone dives straight at it.
            WeaponKind::Missile if w.suicide => (fm::atan2(to.y, d.max(0.1)), d / w.speed.max(1.0), w.speed),
            WeaponKind::Missile => (if w.launch_pitch > 0.0 { w.launch_pitch.to_radians() } else { 0.35 }, d / w.speed.max(1.0), w.speed),
            WeaponKind::Bomb => (0.0, 0.0, 0.0),
        };
        best = Solution { yaw, pitch, speed, point };
        // Lead moving targets (indirect fire only half-leads: shells take too long to be sure).
        let lead = match w.kind {
            WeaponKind::Direct => 1.0,
            WeaponKind::Missile | WeaponKind::Bomb => 0.0,
            _ => 0.5,
        };
        point = target + (tvel * (t * lead)).at(0.0);
    }
    best
}

// **Line of fire** (direct fire, `systems::Lane`): gun follows shell's real arc (with spread allowance)
// and fires only if it reaches target before ground, water, building other than target, own
// (with friendly fire) or neutral unit, or reclaimable with at least `COVER_BLOCK` (0.25) cover (trees, rocks,
// wrecks, debris; bushes do not count; nothing within `cover_grace` of shooter). A hull-down target is aimed at
// higher (`WeaponState.lift`, up to 0.6 of its height); with no clear shot (`WeaponState.blocked`) gun takes
// another enemy in clear (not ordered one), and ground unit whose guns are all blocked - idle, attack-move or
// Attack - steps aside to nearest spot with clear line (`firing_spot`: rings up to 26 round
// it, kept in `Mover.cover` until reached); on Attack with no such spot it closes in, but not nearer than half its
// range (requested: try to shoot round boulder, move little, not drive up to target). `fire` adds half tick of gravity so shells fly the
// arc they were laid for. `--combattest` "Line of fire" lines.
/// Who fires a direct-fire shot, for working out what it would run into on the way.
struct Lane<'a> {
    census: &'a Census,
    map: &'a MapData,
    team: u8,
    friendly_fire: bool,
    grace: f32,
    /// The shooter and the garrison it fires from: shots never hit those.
    shooter: u32,
    host: u32,
    g: f32,
    /// Weather's multiplier of every weapon's spread.
    spread: f32,
    /// Trees, rocks and wrecks: shells fly through them but lose damage, so guns aim round them.
    features: &'a Features,
    wrecks: &'a WreckList,
    defs: &'a Defs,
}

/// Least cover share for a tree, rock or wreck to count as in the way of a shot (bushes do not).
const COVER_BLOCK: f32 = 0.25;

/// A cylinder a shell must stay out of: (centre, radius, bottom, top).
type Obstacle = (V2, f32, f32, f32);

impl Lane<'_> {
    /// What a shell from `from` to `to` would hit instead of `target` (as `projectiles` decides
    /// hits): buildings, and own (with friendly fire) or neutral units. Own units right beside the
    /// shooter are flown past, as in `projectiles`. An enemy unit in the way is no reason to hold
    /// fire: the shot hits an enemy either way.
    fn obstacles(&self, w: &WeaponDef, from: Vec3, to: V2, target: u32) -> Vec<Obstacle> {
        let from2 = flat(from);
        let mut list = vec![];
        self.census.near((from2 + to) * 0.5, from2.dist(to) * 0.5 + 8.0, |c| {
            if c.id == target || c.id == self.shooter || c.id == self.host || c.hp <= 0.0 || c.hosted || c.fieldwork {
                return;
            }
            if if c.air { !w.hits_air } else { !w.hits_land } {
                return;
            }
            let own = c.team == self.team;
            if own && (!self.friendly_fire || c.pos.dist(from2) - c.hit_r < self.grace) {
                return;
            }
            if own || c.structure || !hostile(self.team, c.team) {
                list.push((c.pos, c.hit_r, c.y - HIT_BELOW, c.y + c.hit_h));
            }
        });
        // Reclaimables (trees, rocks, wrecks, debris) soak a share of the damage: no aim through them.
        let (mid, reach) = ((from2 + to) * 0.5, from2.dist(to) * 0.5 + 4.0);
        let rules = &self.defs.rules;
        let mut soft = |pos: V2, r: f32, top: f32| {
            if pos.dist(from2) - r >= self.grace {
                let ground = self.map.h(pos);
                list.push((pos, r, ground - 0.3, ground + top + 0.2));
            }
        };
        self.features.near(mid, reach, |_, s| {
            if rules.features.get(s.def as usize).map_or(0.0, |d| d.cover()) >= COVER_BLOCK {
                soft(s.pos, s.radius, s.height);
            }
        });
        if rules.wreck_cover >= COVER_BLOCK {
            let r2 = reach * reach;
            for &(_, pos, mass, def, piece, _) in &self.wrecks.0 {
                if mass > 0.0 && (pos - mid).len2() <= r2 {
                    let (r, top) = Wreck::shape(self.defs, def, piece);
                    soft(pos, r, top);
                }
            }
        }
        list
    }

    /// Whether a shell laid with `sol` (pitch lowered by `dip`) gets into the target's cylinder
    /// (radius `r` around `sol.point`) before it touches the ground or water or runs into one of
    /// `obst` (widened by `side`, the sideways spread per unit of distance). Follows the real
    /// arc, which `fire` makes exact under the tick integration.
    fn clears(&self, origin: Vec3, sol: &Solution, r: f32, dip: f32, side: f32, obst: &[Obstacle]) -> bool {
        let to = sol.point - origin;
        let end = Vec2::new(to.x, to.z).length() - r;
        if end <= 0.5 {
            return true;
        }
        let pitch = sol.pitch - dip;
        let (cp, sp) = (fm::cos(pitch).max(0.01), fm::sin(pitch));
        let dir = V2::from_angle(sol.yaw);
        let vh = sol.speed.max(1.0) * cp;
        let (tan, k) = (sp / cp, self.g / (2.0 * vh * vh));
        let y = |s: f32| origin.y + s * tan - k * s * s;
        let steps = (end / 0.8).ceil() as i32;
        for i in 1..=steps {
            let s = end * i as f32 / steps as f32;
            let (x, z) = (origin.x + dir.x * s, origin.z + dir.y * s);
            if y(s) <= self.map.height(x, z).max(self.map.water_xz(x, z) - 0.5) {
                return false;
            }
        }
        let o = flat(origin);
        obst.iter().all(|&(c, cr, lo, hi)| {
            let rel = c - o;
            let along = rel.dot(dir);
            let r = cr + along.max(0.0) * side;
            let across = rel.dot(dir.perp()).abs();
            if across >= r || along + r <= 0.5 || along - r >= end {
                return true;
            }
            // The stretch of the flight above the cylinder's footprint; the arc's height there
            // (one end, the other, and its top if that lies between) must miss [lo, hi].
            let half = (r * r - across * across).sqrt();
            let (s0, s1) = ((along - half).max(0.5), (along + half).min(end));
            if s0 > s1 {
                return true;
            }
            let apex = (tan / (2.0 * k)).clamp(s0, s1);
            let ys = [y(s0), y(s1), y(apex)];
            let (min, max) = (ys.iter().copied().fold(f32::MAX, f32::min), ys.iter().copied().fold(f32::MIN, f32::max));
            max < lo || min > hi
        })
    }

    /// Direct fire: the height above the target's centre to aim at so the shell reaches the target
    /// rather than the ground, a building or a friend in front of it, even when spread throws it a
    /// little off. A target partly behind a crest is aimed at higher up; `None` when no part of it
    /// can be hit.
    fn aim(&self, w: &WeaponDef, origin: Vec3, c: &Cen) -> Option<f32> {
        let spread = w.spread.to_radians() * self.spread;
        let obst = self.obstacles(w, origin, c.pos + c.vel * (c.pos.dist(flat(origin)) / w.speed.max(1.0)), c.id);
        // `Rng::bell` throws up to +-1 x spread in yaw and +-0.6 x spread in pitch: allow for 80%
        // of that (nearly every shot), with no last resort that ignores the spread.
        let (dip, side) = (spread * 0.6 * 0.8, spread * 0.8);
        for lift in [0.0, 0.2, 0.4, 0.6] {
            let lift = lift * c.hit_h;
            let sol = solve(w, origin, c.center() + Vec3::Y * lift, c.vel, self.g);
            if self.clears(origin, &sol, c.hit_w.min(c.hit_l) * 0.5, dip, side, &obst) {
                return Some(lift);
            }
        }
        None
    }

    /// Direct fire target choice: preferred class first, then nearest, but an enemy the gun can
    /// hit without shooting into the ground, a building or a friend beats one it cannot. Returns
    /// the aim height too; with `skip` (the current target) only a target in the clear.
    fn pick(&self, w: &WeaponDef, origin: Vec3, pos: V2, skip: u32, preferred: impl Fn(&Cen) -> bool) -> Option<(&Cen, Option<f32>)> {
        let m2 = w.min_range * w.min_range;
        let mut list: Vec<(bool, bool, f32, u32)> = vec![];
        let mut eng = false;
        self.census.near(pos, w.range, |c| {
            let d2 = (c.pos - pos).len2();
            if c.id != skip && d2 >= m2 && self.census.valid_target(c, self.team, w.hits_land, w.hits_air) {
                eng |= c.engineer;
                list.push((!preferred(c), c.structure, d2, c.id));
            }
        });
        // While an engineer is in reach, buildings come after every other enemy.
        if !eng {
            list.iter_mut().for_each(|e| e.1 = false);
        }
        list.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.total_cmp(&b.2)).then(a.3.cmp(&b.3)));
        for &(_, _, _, id) in list.iter().take(4) {
            let c = self.census.get(id)?;
            if let Some(lift) = self.aim(w, origin, c) {
                return Some((c, Some(lift)));
            }
        }
        list.first().and_then(|e| self.census.get(e.3)).filter(|_| skip == 0).map(|c| (c, None))
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn weapons(
    mut q: Query<(&Unit, &Body, &mut Weapons, Option<&mut Mover>, Option<&Hosted>), Without<UnderConstruction>>,
    census: Res<Census>,
    defs: Res<Defs>,
    map: Res<MapData>,
    mut game: ResMut<Game>,
    mut rng: ResMut<SimRng>,
    mut fx: ResMut<FxLog>,
    mut effects: ResMut<Effects>,
    mut commands: Commands,
    (features, wreck_list): (Res<Features>, Res<WreckList>),
) {
    let g = defs.rules.gravity;
    let tick = game.tick;
    // Rain, wind and snow scatter the shots.
    let wx_spread = defs.rules.weather_of(game.weather).spread;
    for (u, body, mut ws, mut mover, hosted) in q.iter_mut().sort_by_key::<&Unit, _>(|u| u.id) {
        let mut ground_volleys = 0u16;
        // Still falling from an airdrop.
        if mover.as_ref().is_some_and(|m| m.drop > 0.0) {
            continue;
        }
        // Infantry fighting off a storm of their building are too busy to shoot out of it.
        if hosted.is_some_and(|h| effects.stormed.contains(&h.0)) {
            continue;
        }
        if !effects.stormed.is_empty() && hosted.is_none() && census.get(u.id).and_then(|c| trench_of(&census, &defs, c)).is_some_and(|t| effects.stormed.contains(&t.0)) {
            continue;
        }
        let df = defs.get(u.def);
        // Spent rounds come back one at a time for a flow of mass (not while paused). Mass is
        // taken in id order, so in a stall the lower ids rearm first.
        if !census.get(u.id).is_some_and(|c| c.paused) {
            for (w, st) in df.weapons.iter().zip(ws.list.iter_mut()) {
                if st.fired == 0 || w.ammo == 0 || w.rearm <= 0.0 {
                    continue;
                }
                let step = DT / w.rearm;
                let Some(pl) = game.players.get_mut(u.owner as usize) else { break };
                let want = w.rearm_mass * step;
                let r = if want > 0.0 { (pl.mass.max(0.0) / want).min(1.0) } else { 1.0 };
                pl.mass -= want * r;
                pl.spend_m += want * r / DT;
                st.rearm += step * r;
                if st.rearm >= 0.9999 {
                    st.rearm = 0.0;
                    st.fired -= 1;
                }
            }
        }
        let team = game.team_of(u.owner);
        let mut origin = body.pos.at(body.y + df.height() * 0.8);
        if let Some(h) = hosted {
            // Only infantry inside a building fire (from its windows and roof); transport riders do not.
            let Some(host) = census.get(h.0).filter(|c| c.done && defs.get(c.def).is_garrison()) else { continue };
            let spot = host.pos + V2::from_angle(u.id as f32 * 2.4) * (host.radius * 0.45);
            origin = spot.at(host.y + host.height * 0.85 + 0.2);
        }
        let plane = df.is_plane();
        let (focus, ground) = (ws.focus, ws.ground);
        let lane = Lane {
            census: &census,
            map: &map,
            team,
            friendly_fire: defs.rules.friendly_fire,
            grace: defs.rules.cover_grace,
            shooter: u.id,
            host: hosted.map_or(0, |h| h.0),
            g,
            spread: wx_spread,
            features: &features,
            wrecks: &wreck_list,
            defs: &defs,
        };
        for (wi, w) in df.weapons.iter().enumerate() {
            let Some(st) = ws.list.get_mut(wi) else { continue };
            st.cd = (st.cd - DT).max(0.0);
            let bomb = w.kind == WeaponKind::Bomb;
            let direct = w.kind == WeaponKind::Direct;
            // Bombs carry forward as far as the bomber flies while they fall, so a bomber has to
            // pick its target up well before it is overhead, whatever the JSON range says.
            let reach = if bomb { w.range.max(df.mv.speed * (2.0 * df.mv.altitude.max(1.0) / g).sqrt() + 12.0) } else { w.range };
            let drop = |tp: Vec3, tv: V2| bomb_drop(w, body, df.mv.speed, tp, tv, g);

            // --- target selection
            let in_envelope = |c: &Cen| -> bool {
                let d = c.pos.dist(body.pos);
                d <= reach && d >= w.min_range && c.hp > 0.0 && !c.hosted && if c.air { w.hits_air } else { w.hits_land }
            };
            let mut target: Option<&Cen> = None;
            let mut gpoint: Option<Vec3> = None;
            let mut checked = false;
            // Target priority (weapon `prefer`): the class this weapon goes for first.
            let preferred = |c: &Cen| -> bool {
                match w.prefer.as_str() {
                    "armored" => defs.get(c.def).is_armored(),
                    "infantry" => defs.get(c.def).is_infantry(),
                    _ => false,
                }
            };
            let prioritised = matches!(w.prefer.as_str(), "armored" | "infantry");
            if let Some((gp, radius)) = ground.filter(|_| w.hits_land) {
                let d = gp.dist(body.pos);
                if d <= w.range + radius && d >= w.min_range {
                    // Area fire covers the circle evenly: shot `j` of this weapon aims at point `j` of
                    // a low-discrepancy (R2) sequence mapped onto the disc by equal area, so shots fill
                    // the gaps the earlier ones left instead of clumping. The point stays the same
                    // through a reload (salvo, aiming); each unit and weapon starts further along
                    // the sequence, so several guns on one circle share it out. Points out of reach
                    // (beyond range or inside the minimum range) are skipped.
                    let shot = tick as u64 / ((w.reload * 10.0) as u64).max(1);
                    let base = shot + u.id as u64 * 97 + wi as u64 * 41;
                    let at = |j: u64| -> V2 {
                        let j = j as f64;
                        let x = (0.5 + j * 0.754_877_666_246_692_7).fract() as f32;
                        let y = (0.5 + j * 0.569_840_290_998_053_3).fract() as f32;
                        map.clamp(gp + V2::from_angle(y * std::f32::consts::TAU) * (x.sqrt() * radius))
                    };
                    let reachable = |p: V2| {
                        let d = p.dist(body.pos);
                        d <= w.range && d >= w.min_range
                    };
                    let p = if radius > 0.0 { (0..8).map(|k| at(base + k)).find(|p| reachable(*p)).unwrap_or_else(|| at(base)) } else { gp };
                    gpoint = Some(p.at(map.surface(p)));
                }
            }
            if gpoint.is_none() {
                if let Some(c) = census.get(focus).filter(|c| in_envelope(c) && c.team != team && c.known_to(team)) {
                    target = Some(c);
                } else if let Some(c) = census.get(st.target).filter(|c| in_envelope(c) && census.valid_target(c, team, w.hits_land, w.hits_air)) {
                    // A weapon with a priority drops a target of another class to look for a preferred one.
                    // So does one shooting a building while an enemy engineer is in reach.
                    let leave = c.structure
                        && (tick + u.id) % 5 == 0
                        && census.engineer_near(body.pos, team, reach, w.min_range, w.hits_land, w.hits_air);
                    if (tick + u.id) % 20 != 0 && !(prioritised && !preferred(c)) && !leave {
                        target = Some(c);
                    }
                }
                if bomb && census.get(focus).is_none() {
                    // Without an attack order a bomber drops on whatever it is lined up with:
                    // the target ahead whose stick would land closest.
                    // Buildings only while no enemy engineer is in reach.
                    let mut best = (f32::MAX, 0u32);
                    let eng = census.engineer_near(body.pos, team, reach, 0.0, w.hits_land, w.hits_air);
                    census.near(body.pos, reach, |c| {
                        if !census.valid_target(c, team, w.hits_land, w.hits_air) || (eng && c.structure) {
                            return;
                        }
                        let d = drop(c.center(), c.vel);
                        if d.along > -d.tol {
                            let score = d.cross.abs() * 3.0 + d.along.max(0.0);
                            if score < best.0 {
                                best = (score, c.id);
                            }
                        }
                    });
                    target = census.get(best.1);
                } else if target.is_none() && (st.target != 0 || (tick + u.id) % 3 == 0) {
                    target = if direct {
                        let pick = lane.pick(w, origin, body.pos, 0, &preferred);
                        if let Some((_, lift)) = pick {
                            checked = true;
                            st.lift = lift.unwrap_or(0.0);
                            st.blocked = lift.is_none();
                        }
                        pick.map(|p| p.0)
                    } else {
                        census.pick_enemy(body.pos, team, w.range, w.min_range, w.hits_land, w.hits_air, preferred)
                    };
                }
            }
            // A sniper rifle ignores buildings with nobody of the enemy inside.
            if w.snipe && target.is_some_and(|c| defs.get(c.def).is_garrison() && !census.list.iter().any(|r| r.host == c.id && r.done && r.hp > 0.0 && hostile(team, r.team))) {
                target = None;
            }
            // Direct fire: find where on the target a shell gets to before the ground (checked
            // every third tick and for a new target); with no part of it in the clear, take
            // another enemy that is - unless this one was ordered.
            match target.filter(|_| direct && gpoint.is_none()) {
                Some(c) => {
                    if !checked && (c.id != st.target || (tick + u.id + wi as u32) % 3 == 0) {
                        let lift = lane.aim(w, origin, c);
                        st.lift = lift.unwrap_or(0.0);
                        st.blocked = lift.is_none();
                        if st.blocked && c.id != focus {
                            if let Some((alt, Some(lift))) = lane.pick(w, origin, body.pos, c.id, &preferred) {
                                target = Some(alt);
                                st.lift = lift;
                                st.blocked = false;
                            }
                        }
                    }
                }
                None => {
                    st.lift = 0.0;
                    st.blocked = false;
                }
            }
            st.target = target.map_or(0, |c| c.id);

            // --- out of ammunition: silent until a round is rearmed
            if st.ammo_left(w) == Some(0) {
                st.target = 0;
                st.burst = 0;
                st.aim = 0.0;
                continue;
            }

            // --- bursts keep firing along the barrel (held back while the line of fire is gone)
            if st.burst > 0 {
                st.burst_cd -= DT;
                if st.burst_cd <= 0.0 {
                    st.burst -= 1;
                    st.burst_cd += w.salvo_delay;
                    // Look again: the unit may have moved since the lane was last checked.
                    let clear = !st.blocked && target.filter(|_| direct && gpoint.is_none()).map_or(true, |c| lane.aim(w, origin, c).is_some());
                    if !clear {
                        st.blocked = true;
                    } else {
                        fire(&mut commands, &mut game, &mut rng.0, &mut fx, w, u, body, origin, st, g, wx_spread);
                    }
                }
            }

            let lift = st.lift;
            let aim_at = gpoint.map(|p| (p, v2(0.0, 0.0))).or(target.map(|c| (c.center() + Vec3::Y * lift, c.vel)));
            let Some((tp, tv)) = aim_at else {
                // Nothing to shoot: swing back to the front.
                if let Some(t) = &w.turret {
                    st.yaw = turn_toward(st.yaw, body.yaw, t.yaw_speed.to_radians() * DT * 0.5);
                    st.pitch = turn_toward(st.pitch, 0.0, t.pitch_speed.to_radians() * DT);
                } else {
                    st.yaw = body.yaw;
                    st.pitch = 0.0;
                }
                st.aim = 0.0;
                continue;
            };

            // --- bombs: release when the fall will carry the middle of the stick onto the target
            // (SupCom style: the bomber runs over the target, the bombs are nudged onto it,
            // the bomber flies on and comes round for the next pass)
            if bomb {
                st.yaw = body.yaw;
                let d = drop(tp, tv);
                if st.cd <= 0.0 && st.burst == 0 && d.along <= d.speed * DT * 0.5 && d.along > -d.tol && d.cross.abs() < d.tol {
                    st.cd = w.reload;
                    st.burst = w.salvo.min(st.ammo_left(w).map_or(u8::MAX, |l| l.min(255) as u8)).saturating_sub(1);
                    st.burst_cd = w.salvo_delay;
                    let fix = d.miss * (1.0 / d.fall.max(0.3));
                    st.fix = if fix.len() > 6.0 { fix.norm() * 6.0 } else { fix };
                    ground_volleys |= gpoint.is_some() as u16;
                    fire(&mut commands, &mut game, &mut rng.0, &mut fx, w, u, body, origin, st, g, wx_spread);
                    if let Some(m) = mover.as_mut() {
                        // Fly on past the target before turning, so the next run starts lined up.
                        m.extend = m.extend.max(2.5 / df.mv.turn.to_radians().max(0.1) + d.half);
                    }
                }
                continue;
            }

            // --- lay the gun
            let sol = solve(w, origin, tp, tv, g);
            st.speed = sol.speed;
            let aligned = if let Some(t) = &w.turret {
                st.yaw = turn_toward(st.yaw, sol.yaw, t.yaw_speed.to_radians() * DT);
                st.pitch = turn_toward(st.pitch, sol.pitch, t.pitch_speed.to_radians() * DT);
                wrap(sol.yaw - st.yaw).abs() < 0.02 && (sol.pitch - st.pitch).abs() < 0.03
            } else {
                // Fixed guns: the unit has to point at the target; a small gimbal does the rest.
                st.yaw = body.yaw;
                let ok = wrap(sol.yaw - body.yaw).abs() < if plane { 0.3 } else { 0.12 };
                if ok {
                    st.yaw = sol.yaw;
                    st.pitch = sol.pitch;
                }
                ok
            };
            // Weapons with a hold fire only from a standstill, when the unit is not trying to go anywhere,
            // and only start aiming there: the aim time is the set-up after it stops.
            // A rider inside a building is settled by definition: `locomotion` skips hosted units, so
            // the speed and goal it had when it walked in would never clear.
            let unsettled = w.hold > 0.0 && hosted.is_none() && mover.as_ref().map_or(false, |m| m.goal.is_some() || m.speed > 0.05);
            if aligned && !unsettled {
                st.aim += DT;
            } else {
                st.aim = 0.0;
            }
            if st.cd > 0.0 || st.burst > 0 || !aligned || unsettled || st.aim < w.aim_time {
                continue;
            }
            // Direct fire needs a clear line; otherwise hold fire rather than shoot the hill.
            if direct && if gpoint.is_some() { !map.los(origin, sol.point + Vec3::Y * 0.3) } else { st.blocked } {
                continue;
            }
            // The check above can be two ticks old and the unit may have moved or something stepped
            // in front: look once more right before the shot, and lay again if the aim height changed.
            if let Some(c) = target.filter(|_| direct && gpoint.is_none()) {
                match lane.aim(w, origin, c) {
                    None => {
                        st.blocked = true;
                        continue;
                    }
                    Some(l) if (l - st.lift).abs() > 1e-4 => {
                        st.lift = l;
                        continue;
                    }
                    _ => {}
                }
            }
            st.cd = w.reload;
            st.burst = w.salvo.min(st.ammo_left(w).map_or(u8::MAX, |l| l.min(255) as u8)).saturating_sub(1);
            st.burst_cd = w.salvo_delay;
            // Aircraft (gunships) and machine guns never stand still for a shot or a burst.
            // (Nor does a rider in a building: nothing would run the hold down while it is inside.)
            if let Some(m) = mover.as_mut().filter(|_| !df.is_air() && !w.is_machine_gun() && hosted.is_none()) {
                m.hold = m.hold.max(w.hold + w.salvo_delay * st.burst as f32);
            }
            ground_volleys |= gpoint.is_some() as u16;
            fire(&mut commands, &mut game, &mut rng.0, &mut fx, w, u, body, origin, st, g, wx_spread);
            if w.suicide {
                // The drone is the warhead: it is gone, quietly (no wreck, no death blast).
                effects.damage.push((u.id, 1.0e9, 0));
                effects.reclaimed.push(u.id);
                break;
            }
        }
        ws.ground_shots = ws.ground_shots.saturating_add(ground_volleys);
    }
}

// **Armor angle** (`rules.armor_front/side/rear`, 0.6 / 1.0 / 1.5): projectile hitting armored unit (category
// `armored`, not structures) is scaled by where it comes from against hull's facing, blended linearly by angle,
// and by how flat it flies (`systems::armor_angle`: squared cosine of descent angle, so steep mortar rounds
// count angle little, one falling straight down not at all). Applied to `p.damage` at hit, before blast.
// `--combattest` infantry "Armor angle": Crony on Overtaker does 481 head on, 801 side, 1199 from behind.
/// The share of a hit an armored unit takes for the way it comes in (`rules.armor_front/side/rear`):
/// head on, from the side or from behind, blended linearly by the angle between the shot and the
/// hull's facing. The steeper the shot falls the less that counts (it lands on top): a mortar
/// round coming down at 60 degrees gets a quarter of it, one falling straight down none.
fn armor_angle(defs: &Defs, c: &Cen, vel: Vec3) -> f32 {
    if c.structure || !defs.get(c.def).is_armored() {
        return 1.0;
    }
    let r = &defs.rules;
    let (h2, s2) = (vel.x * vel.x + vel.z * vel.z, vel.length_squared());
    if h2 < 1e-9 {
        return 1.0;
    }
    // From where the shot comes, against the way the hull faces: 1 head on, -1 from behind.
    let from = v2(-vel.x, -vel.z) * (1.0 / h2.sqrt());
    let cos = from.dot(V2::from_angle(c.yaw)).clamp(-1.0, 1.0);
    let m = if cos >= 0.0 { r.armor_side + (r.armor_front - r.armor_side) * cos } else { r.armor_side + (r.armor_rear - r.armor_side) * -cos };
    // Squared cosine of the descent angle: 1 for a flat shot, 0 for one falling straight down.
    let flat = h2 / s2;
    1.0 + (m - 1.0) * flat
}

/// Where a bomb stick released now would land, relative to the target.
struct Drop {
    /// Target point minus the impact point of the stick's middle bomb.
    miss: V2,
    /// `miss` along the flight path (positive = the target is still ahead of the impact) and across it.
    along: f32,
    cross: f32,
    /// Seconds a bomb falls, half the time the stick takes to drop, the bomber's speed.
    fall: f32,
    half: f32,
    speed: f32,
    /// How far off the bombs may be pushed onto the target.
    tol: f32,
}

// **Bombers** (SupCom style, `systems.rs::bomb_drop`): bomb weapon's reach is at least forward
// throw (speed x fall time) + 12, whatever its JSON `range`. stick is released so its middle
// bomb lands on target (moving targets led) and every bomb gets same small sideways push
// (`WeaponState.fix`) onto it; without attack order bomber drops on target it is best lined up
// with. Locomotion keeps bomber flying on until it has run-in as long as its throw, then it comes
// round. A bomber must detect targets beyond its throw, hence radar on bombers. Planes on
// attack-move go for ground targets first when they carry bombs (fighter-bomber), else aircraft first.
// Missiles lead their target; `launch_pitch` (deg) makes SAM launch near-vertically.
fn bomb_drop(w: &WeaponDef, body: &Body, cruise: f32, tp: Vec3, tv: V2, g: f32) -> Drop {
    let vel = if body.vel.len() > 0.5 { body.vel } else { V2::from_angle(body.yaw) * cruise };
    let speed = vel.len().max(0.1);
    let dir = vel * (1.0 / speed);
    let half = (w.salvo.max(1) - 1) as f32 * 0.5 * w.salvo_delay;
    // Fall time under the tick integration (velocity first, then position).
    let h = (body.y - 0.3 - tp.y).max(0.3);
    let fall = (2.0 * h / g + DT * DT * 0.25).sqrt() - DT * 0.5;
    let aim = flat(tp) + tv * (half + fall);
    let impact = body.pos + vel * (half + fall);
    let miss = aim - impact;
    Drop { miss, along: miss.dot(dir), cross: miss.dot(dir.perp()), fall, half, speed, tol: 3.5 + w.radius * 0.5 }
}

// **Suicide drones** (weapon `suicide: true`, with `ammo: 1`; T2 Dove): when weapon fires, unit itself is
// launched at target as homing missile (straight dive from where it flies, `solve`/`fire`) and removed
// quietly (`Effects.reclaimed`: no wreck, no death blast). Dove: 70 mass, 40 hp, speed 5.5, altitude 6, 180
// Explosive / radius 4 warhead, range 14; AI treats it as bomber. `--combattest` air "Doves" lines: 4 kill an
// Overtaker unopposed; with radar tower MBFAG downs 4 of 6, T1 AA turret all 6.
#[allow(clippy::too_many_arguments)]
fn fire(commands: &mut Commands, game: &mut Game, rng: &mut Rng, fx: &mut FxLog, w: &WeaponDef, u: &Unit, body: &Body, origin: Vec3, st: &mut WeaponState, g: f32, wx_spread: f32) {
    st.fired = st.fired.saturating_add(1);
    let spread = w.spread.to_radians() * wx_spread;
    let (vel, kind, pos) = match w.kind {
        WeaponKind::Bomb => ((body.vel + st.fix).at(0.0), PROJ_BOMB, body.pos.at(body.y - 0.3)),
        WeaponKind::Missile => {
            if w.suicide {
                let d = dir_from(st.yaw, st.pitch);
                (d * w.speed, PROJ_MISSILE, body.pos.at(body.y))
            } else {
                let d = dir_from(st.yaw, st.pitch.max(0.3));
                (d * w.speed, PROJ_MISSILE, origin + d * 0.5)
            }
        }
        _ => {
            let speed = if st.speed > 0.0 { st.speed } else { w.speed };
            let d = dir_from(st.yaw + rng.bell() * spread, st.pitch + rng.bell() * spread * 0.6);
            // `projectiles` adds gravity to the velocity before moving, which drops a shell
            // g*DT*t/2 below the arc it was laid for; half a tick of gravity up front cancels that.
            (d * speed + Vec3::Y * (g * DT * 0.5), PROJ_SHELL, origin + d * 0.4)
        }
    };
    // The sound of the shot: named in the weapon's JSON, otherwise by the kind of weapon.
    let sound = SOUNDS.iter().position(|s| *s == w.sound).unwrap_or(match w.kind {
        WeaponKind::Missile => 3,
        WeaponKind::Bomb => 4,
        WeaponKind::Arc | WeaponKind::HighArc => 2,
        WeaponKind::Direct if w.reload < 1.2 => 1,
        WeaponKind::Direct => 0,
    });
    fx.push_snd(pos, FX_MUZZLE, (w.damage.sqrt() * 0.05).clamp(0.12, 1.2), sound as u8);
    let id = game.alloc();
    let shot = Projectile {
        id,
        pos: pos.into(),
        vel: vel.into(),
        kind,
        owner: u.owner,
        damage: w.damage,
        dtype: w.dtype as u8,
        radius: w.radius,
        life: match w.kind {
            WeaponKind::Direct => w.range * 1.6 / w.speed.max(1.0) + 0.5,
            WeaponKind::Missile => w.range * 3.0 / w.speed.max(1.0),
            _ => 60.0,
        },
        target: st.target,
        turn: w.turn_rate,
        hits_land: w.hits_land,
        hits_air: w.hits_air,
        from: flat(origin),
        snipe: w.snipe,
        shooter: u.id,
        splash: w.splash_share,
    };
    spawn_projectile(commands, shot);
}

// **Ballistics**: every shot is projectile entity with 3D position/velocity under gravity
// (`rules.json: gravity`). Weapon kinds: `direct` (needs clear line of fire, leads target), `arc`,
// `high_arc` (indirect, raise charge to reach), `missile` (homing, limited turn rate), `bomb`.
// Hits resolve against terrain and unit hit boxes, so shots miss, fall short or hit hills. Hit box
// (`hit_box: [w, h, l]` in unit JSON, default `size`) turns with unit and stands on its base (reaching
// `sim::HIT_BELOW` under it for slopes); numbers are model's bounds without gun barrels
// (`python3 tools/model_bounds.py` prints them next to JSON; troopers, commander, watch tower and Stork set by
// hand; aircraft at least 0.6 in every direction, on request, so guns still hit them). Shells: `Cen::hit_seg`; blasts and missile fuzes: `Cen::hit_dist`; line-of-fire obstacles use circle
// `Cen::hit_r` around box.
// Guns have turret traverse speed, `aim_time` (must hold on target) and `spread`; salvos fire bursts.
/// Moves projectiles under gravity and resolves hits against terrain and unit volumes.
#[allow(clippy::too_many_arguments)]
fn projectiles(
    mut q: Query<(Entity, &mut Projectile)>,
    mut wq: Query<(Entity, &mut Wreck)>,
    census: Res<Census>,
    defs: Res<Defs>,
    map: Res<MapData>,
    game: Res<Game>,
    mut effects: ResMut<Effects>,
    mut fx: ResMut<FxLog>,
    (mut features, wreck_list): (ResMut<Features>, Res<WreckList>),
    mut reactive: Query<&mut Reactive>,
    mut commands: Commands,
) {
    let mut pushes: Vec<(u32, Vec3)> = vec![];
    let g = defs.rules.gravity;
    let ff = defs.rules.friendly_fire;
    let grace = defs.rules.cover_grace;
    for (e, mut p) in q.iter_mut().sort_by_key::<&Projectile, _>(|p| p.id) {
        let team = game.team_of(p.owner);
        // With friendly fire a shot never hits the unit that fired it, nor the garrison it fires from
        // (without, the own team is skipped anyway).
        let shooter = if ff { p.shooter } else { 0 };
        let host = if shooter != 0 { census.get(shooter).map_or(0, |s| s.host) } else { 0 };
        let own = move |id: u32| shooter != 0 && (id == shooter || id == host);
        p.life -= DT;
        let a = p.pos.v();
        let mut vel = p.vel.v();
        let mut hit: Option<(Vec3, u32)> = None;
        if p.kind == PROJ_MISSILE {
            let speed = vel.length().max(1.0);
            if let Some(t) = census.get(p.target).filter(|t| t.hp > 0.0 && !t.hosted) {
                // Lead pursuit: steer for where the target will be when the missile gets there.
                let tgo = (t.center() - a).length() / speed;
                let want = (t.center() + t.vel.at(0.0) * tgo - a).normalize_or_zero();
                let cur = vel / speed;
                let ang = fm::acos(cur.dot(want).clamp(-1.0, 1.0));
                let max = p.turn.to_radians() * DT;
                let nd = if ang <= max || ang < 1e-4 { want } else { cur.lerp(want, max / ang).normalize_or_zero() };
                vel = nd * speed;
                if t.hit_dist(a) < speed * DT {
                    hit = Some((t.center(), t.id));
                }
            }
        } else {
            vel.y -= g * DT;
        }
        let b = a + vel * DT;
        if hit.is_none() {
            let seg = b - a;
            let seg2 = Vec2::new(seg.x, seg.z);
            let mut best: Option<(f32, u32)> = None;
            let mid = flat((a + b) * 0.5);
            census.near(mid, seg2.length() * 0.5 + 8.0, |c| {
                if c.hp <= 0.0 || c.hosted || (c.team == team && !ff) || own(c.id) {
                    return;
                }
                // Friendly fire: shots fly out past own units right beside the shooter, as past cover.
                if c.team == team && c.pos.dist(p.from) - c.hit_r < grace {
                    return;
                }
                // Sandbags and trenches stop nothing (they soak, below), unless they are what was aimed at.
                if c.fieldwork && c.id != p.target {
                    return;
                }
                if if c.air { !p.hits_air } else { !p.hits_land } {
                    return;
                }
                if let Some(t) = c.hit_seg(a, b).filter(|t| best.map_or(true, |x| *t < x.0)) {
                    best = Some((t, c.id));
                }
            });
            // Terrain along the segment.
            let steps = (seg.length() / 0.8).ceil().max(1.0) as i32;
            let mut ground_t: Option<f32> = None;
            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                let s = a + seg * t;
                if s.y <= map.height(s.x, s.z).max(map.water_xz(s.x, s.z) - 0.5) {
                    ground_t = Some(t);
                    break;
                }
            }
            match (best, ground_t) {
                (Some((t, id)), gt) if gt.map_or(true, |g| t <= g) => hit = Some((a + seg * t, id)),
                (_, Some(t)) => hit = Some((a + seg * t, 0)),
                _ => {}
            }
        }
        // Cover: trees, rocks and wrecks the shot flies through on the way soak up part of it.
        soak_cover(&mut p, a, hit.map_or(b, |h| h.0), hit.is_some(), &map, &defs, &mut features, &wreck_list, &mut pushes);
        soak_works(&mut p, a, hit.map_or(b, |h| h.0), hit, &map, &defs, &census, &mut effects);
        let Some((at, victim)) = hit else {
            if p.life <= 0.0 || b.x < -20.0 || b.z < -20.0 || b.x > map.world_x() + 20.0 || b.z > map.world_z() + 20.0 {
                if p.radius > 0.0 && p.life <= 0.0 {
                    fx.push(b, FX_BLAST, p.radius);
                }
                commands.entity(e).despawn();
            } else {
                p.pos = b.into();
                p.vel = vel.into();
            }
            continue;
        };
        commands.entity(e).despawn();
        fx.push(at, FX_BLAST, p.radius.max(0.25 + p.damage.sqrt() * 0.05));
        // Shells that land on the ground leave a hole.
        let rules = &defs.rules;
        if rules.craters_on() && p.damage >= rules.crater_min_damage && (victim == 0 || p.radius > 0.0) && at.y - map.height(at.x, at.z) < 1.5 && at.y > map.water_xz(at.x, at.z) - 0.5 {
            let depth = rules.crater_scale * p.damage.sqrt() * if p.radius > 0.0 { 1.0 } else { 0.5 };
            effects.craters.push((flat(at), p.radius * 0.5, depth));
        }
        // Armored units are tougher head on than from the side or behind (the blast of the shell
        // hurts whatever is around with what got through).
        if let Some(c) = census.get(victim) {
            p.damage *= armor_angle(&defs, c, vel);
        }
        // The unit hit takes it all (below); the blast hurts the others around with `splash` of it,
        // armored ones less again.
        if p.radius > 0.0 && p.splash > 0.0 {
            census.near(flat(at), p.radius + 6.0, |c| {
                if c.id == victim || c.hp <= 0.0 || c.hosted || (c.team == team && !ff) || (c.air && !p.hits_air) || own(c.id) {
                    return;
                }
                let d = c.hit_dist(at);
                if d < p.radius {
                    // Infantry in a trench are sheltered from a blast that is not in there with them.
                    let open = trench_of(&census, &defs, c).filter(|t| !in_cell(flat(at), t.2)).map_or(1.0, |t| 1.0 - t.1);
                    let share = p.splash * splash_armor(&defs, c) * blast_share(d, p.radius) * open;
                    effects.damage.push((c.id, p.damage * share, p.dtype as usize));
                }
            });
        }
        if victim != 0 && p.snipe {
            // Through the window: the lowest-id enemy rider takes it, the building nothing.
            let rider = census.get(victim).filter(|c| defs.get(c.def).is_garrison()).and_then(|_| {
                census.list.iter().filter(|r| r.host == victim && r.done && r.hp > 0.0 && hostile(team, r.team)).min_by_key(|r| r.id)
            });
            match rider {
                Some(r) => effects.damage.push((r.id, p.damage, p.dtype as usize)),
                None => {
                    // Not a garrison (a unit in the open): an ordinary hit.
                    if !census.get(victim).is_some_and(|c| defs.get(c.def).is_garrison()) {
                        effects.damage.push((victim, p.damage, p.dtype as usize));
                    }
                }
            }
        } else if victim != 0 {
            let mut dmg = p.damage;
            // A missile on reactive armor: a charge goes off and takes most of it.
            if p.kind == PROJ_MISSILE {
                if let Some((c, era)) = census.get(victim).and_then(|c| defs.get(c.def).reactive.map(|r| (c, r))) {
                    if let Ok(mut left) = reactive.get_mut(c.e) {
                        if left.0 > 0 {
                            left.0 -= 1;
                            dmg *= 1.0 - era.block;
                            fx.push(at, FX_BLAST, 0.6);
                        }
                    }
                }
            }
            effects.damage.push((victim, dmg, p.dtype as usize));
        }
        // Trees, rocks and wrecks never stop a shot and are never destroyed by one, but what lands on
        // them or blasts next to them fells trees, shatters rocks and shoves wrecks and debris about.
        if p.radius > 0.0 || victim == 0 {
            hurt_reclaimables(at, p.radius, p.damage, &map, &defs, &mut features, &wreck_list, &mut pushes);
        }
    }
    push_wrecks(&pushes, &mut wq, &defs);
}

// **Cover** (`systems::soak_cover`, called in `projectiles` for every flight segment): each tree, rock or wreck
// shot passes through (closest approach to its axis inside this tick's segment, so each counts once) takes
// its share of damage: `features[].cover` (default by shape: pine 0.3, broadleaf 0.35, bush 0.15, rock
// 0.65) or `rules.wreck_cover`; shot flies on with rest. Cover within `rules.cover_grace` of shooter
// (`Projectile.from`) does not count, so units fire out from behind their own. Cover is never used up.
/// A shot flying from `a` to `b` this tick passes trees, rocks and wrecks: each one whose axis it
/// passes closest to within this stretch (so every one counts once over the flight) and that it
/// goes through soaks up its `cover` share of the shot's damage, which the shot loses. Cover is
/// never used up: a hard enough hit fells a tree (shatters a rock), and wrecks and debris get a
/// shove along the shot (`pushes`, see `push_wrecks`). Cover within `rules.cover_grace` of the
/// shooter is left out. `end`: the shot stops at `b` (hits something).
#[allow(clippy::too_many_arguments)]
fn soak_cover(p: &mut Projectile, a: Vec3, b: Vec3, end: bool, map: &MapData, defs: &Defs, features: &mut Features, wrecks: &WreckList, pushes: &mut Vec<(u32, Vec3)>) {
    let rules = &defs.rules;
    let seg = b - a;
    let seg2 = Vec2::new(seg.x, seg.z);
    let len2 = seg2.length_squared();
    if p.damage <= 0.0 || len2 < 1e-6 {
        return;
    }
    // (position along the stretch, feature index or wreck id, is a feature, cover share)
    let mut cross: Vec<(f32, u32, bool, f32)> = vec![];
    let through = |pos: V2, r: f32, top: f32| -> Option<f32> {
        if pos.dist(p.from) - r < rules.cover_grace {
            return None;
        }
        let t = Vec2::new(pos.x - a.x, pos.y - a.z).dot(seg2) / len2;
        if t < 0.0 || if end { t > 1.0 } else { t >= 1.0 } {
            return None;
        }
        let at = a + seg * t;
        let ground = map.h(pos);
        (Vec2::new(at.x - pos.x, at.z - pos.y).length() < r && at.y > ground - 0.3 && at.y < ground + top + 0.2).then_some(t)
    };
    features.near(flat((a + b) * 0.5), seg2.length() * 0.5 + 4.0, |i, s| {
        let cover = rules.features.get(s.def as usize).map_or(0.0, |d| d.cover());
        if cover > 0.0 {
            if let Some(t) = through(s.pos, s.radius, s.height) {
                cross.push((t, i as u32, true, cover));
            }
        }
    });
    if rules.wreck_cover > 0.0 {
        let (lo, hi) = (flat(a), flat(b));
        let (x0, x1, z0, z1) = (lo.x.min(hi.x) - 4.0, lo.x.max(hi.x) + 4.0, lo.y.min(hi.y) - 4.0, lo.y.max(hi.y) + 4.0);
        for &(id, pos, mass, def, piece, _) in &wrecks.0 {
            if mass <= 0.0 || pos.x < x0 || pos.x > x1 || pos.y < z0 || pos.y > z1 {
                continue;
            }
            let (r, top) = Wreck::shape(defs, def, piece);
            if let Some(t) = through(pos, r, top) {
                cross.push((t, id, false, rules.wreck_cover));
            }
        }
    }
    if cross.is_empty() {
        return;
    }
    cross.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.2.cmp(&y.2)).then(x.1.cmp(&y.1)));
    let along = Vec3::new(seg2.x, 0.0, seg2.y) / len2.sqrt();
    for (_, key, is_feature, cover) in cross {
        let soak = p.damage * cover.clamp(0.0, 1.0);
        p.damage -= soak;
        if is_feature {
            if let Some(f) = features.list.get(key as usize).copied() {
                // A tree shot through hard enough falls the way the shot flew (a rock shatters).
                if fellable(defs, &f) && soak / f.hp >= rules.tree_fall {
                    features.fell(key as usize, fall_dir(v2(seg2.x, seg2.y), key as usize), f.left);
                }
            }
        } else {
            pushes.push((key, along * soak));
        }
    }
}

/// Built cover (field works) in the way of a shot flying from `a` to `b` this tick. Sandbags it
/// flies through take their `cover` share, like trees do (`soak_cover`), whoever built them: they
/// shelter every side. A shot that hits infantry standing
/// in a trench loses the trench's share to the trench. Works within `rules.cover_grace` of the
/// shooter do not count. What a work soaks up is damage to it, so cover is shot to pieces in time.
#[allow(clippy::too_many_arguments)]
fn soak_works(p: &mut Projectile, a: Vec3, b: Vec3, hit: Option<(Vec3, u32)>, map: &MapData, defs: &Defs, census: &Census, effects: &mut Effects) {
    let grace = defs.rules.cover_grace;
    let seg = b - a;
    let seg2 = Vec2::new(seg.x, seg.z);
    let len2 = seg2.length_squared();
    if p.damage <= 0.0 || len2 < 1e-6 {
        return;
    }
    // (position along the stretch, work id, cover share)
    let mut cross: Vec<(f32, u32, f32)> = vec![];
    census.near(flat((a + b) * 0.5), seg2.length() * 0.5 + 4.0, |c| {
        if !c.fieldwork || !c.done || c.hp <= 0.0 || c.id == p.target || c.pos.dist(p.from) - c.radius < grace {
            return;
        }
        let Some((cover, _)) = defs.get(c.def).work().filter(|w| w.0 > 0.0 && w.1 <= 0.0) else { return };
        let t = Vec2::new(c.pos.x - a.x, c.pos.y - a.z).dot(seg2) / len2;
        if t < 0.0 || if hit.is_some() { t > 1.0 } else { t >= 1.0 } {
            return;
        }
        let at = a + seg * t;
        let ground = map.h(c.pos);
        if Vec2::new(at.x - c.pos.x, at.z - c.pos.y).length() < c.radius && at.y > ground - 0.3 && at.y < ground + c.height + 0.2 {
            cross.push((t, c.id, cover));
        }
    });
    if let Some((id, cover, pos)) = hit.and_then(|h| census.get(h.1)).and_then(|v| trench_of(census, defs, v)) {
        if pos.dist(p.from) - CELL * 0.5 >= grace {
            cross.push((2.0, id, cover));
        }
    }
    cross.sort_by(|x, y| x.0.total_cmp(&y.0).then(x.1.cmp(&y.1)));
    for (_, id, cover) in cross {
        let soak = p.damage * cover;
        p.damage -= soak;
        effects.damage.push((id, soak, p.dtype as usize));
    }
}

/// Share of splash an armored unit takes (`rules.splash_armored`); 1 for anything else.
fn splash_armor(defs: &Defs, c: &Cen) -> f32 {
    if !c.structure && defs.get(c.def).is_armored() { defs.rules.splash_armored } else { 1.0 }
}

/// Share of a blast's damage at distance `d` from its centre: all of it at the centre, fading in a
/// straight line to nothing at the edge.
pub fn blast_share(d: f32, radius: f32) -> f32 {
    (1.0 - d / radius).clamp(0.0, 1.0)
}

/// A standing tree (not a bush) that a hit can throw down, or a rock it can shatter into rubble
/// (`rules.tree_fall`).
fn fellable(defs: &Defs, f: &FeatureState) -> bool {
    f.fall == 0 && f.left > 0.0 && defs.rules.tree_fall > 0.0 && defs.rules.features.get(f.def as usize).is_some_and(|d| d.shape != crate::data::FeatureShape::Bush)
}

/// Direction a tree falls: `dir`, or one picked from its index when that is too short (a blast right on it).
fn fall_dir(dir: V2, idx: usize) -> V2 {
    if dir.len2() > 1e-4 {
        return dir;
    }
    let a = (idx as f32 * 0.618034).fract() * std::f32::consts::TAU;
    v2(fm::cos(a), fm::sin(a))
}

// **Reclaimables** (user's word for trees, rocks and wrecks together): **weapons never destroy them** (requested;
// only reclaiming, building site on tree/rock and wreck caps remove them). Shells do not stop at them, but a
// shell flying through one loses its `cover` share of damage to it (cover, below); shell landing on one or a
// blast (radius, death blasts; `systems::hurt_reclaimables`) fells trees and shatters rocks (below) and shoves wrecks
// and debris (`push_wrecks`: impulse = damage away from blast, or along shot for cover passes; speed +=
// `rules.rubble_push` (0.25) x impulse / sqrt(mass still in it); piece shoved at least `RUBBLE_THROW` (2.5) fast is
// thrown (falling again, up to 10), others slide (whole unit wrecks up to 2); building wrecks never move).
// `rubble_motion` (was `debris_fall`) moves them: falling pieces as before (keeping 0.3 of ground speed on
// landing), sliding ones with friction 0.8 g and slope's pull, stopped by building's footprint. `Wreck.vel` and
// `falling` are snapshotted. View: wrecks follow slide (`wreck_visuals`), debris rolls (`debris_visuals`).
// Unfinished buildings are ordinary targets (5% hp at placement, hp grows with progress).
/// Weapon damage to the reclaimables around an impact at `at`: a shell (`radius` 0) acts on what
/// it lands on, a blast on everything within its radius, with the same fall-off as for units.
/// Reclaimables are never destroyed by weapons (only reclaimed): a hit of `rules.tree_fall` of a
/// tree's or rock's hit points fells the tree or shatters the rock, which keeps its worth; wrecks
/// and debris get a shove away from the blast (`pushes`: wreck id, impulse; see `push_wrecks`).
#[allow(clippy::too_many_arguments)]
pub fn hurt_reclaimables(at: Vec3, radius: f32, damage: f32, map: &MapData, defs: &Defs, features: &mut Features, wrecks: &WreckList, pushes: &mut Vec<(u32, Vec3)>) {
    let reach = radius.max(0.3);
    let centre = flat(at);
    let hurt = |horiz: f32, ground: f32, top: f32| -> Option<f32> {
        let vert = (at.y - top).max(0.0) + (ground - 0.5 - at.y).max(0.0);
        let d = (horiz * horiz + vert * vert).sqrt();
        (d < reach).then(|| if radius > 0.0 { damage * blast_share(d, radius) } else { damage })
    };
    // (index, thrown down: away from the blast)
    let mut fell: Vec<(usize, V2)> = vec![];
    features.near(centre, reach + 6.0, |i, s| {
        let ground = map.h(s.pos);
        if radius > 0.0 && fellable(defs, s) {
            if let Some(dmg) = hurt((s.pos.dist(centre) - s.radius).max(0.0), ground, ground + s.height) {
                if dmg / s.hp >= defs.rules.tree_fall {
                    fell.push((i, fall_dir(s.pos - centre, i)));
                }
            }
        }
    });
    for (i, dir) in fell {
        let left = features.list[i].left;
        features.fell(i, dir, left);
    }
    for &(id, pos, mass, def, piece, _) in &wrecks.0 {
        let (r, h) = Wreck::shape(defs, def, piece);
        let horiz = (pos.dist(centre) - r).max(0.0);
        if mass <= 0.0 || horiz >= reach {
            continue;
        }
        let ground = map.h(pos);
        // (As high as it blocks sight, see `sight_block`.)
        let top = ground + h;
        if let Some(dmg) = hurt(horiz, ground, top) {
            let away = pos - centre;
            // Right on it: thrown a way picked from its id.
            let dir = if away.len2() > 1e-4 { away * (1.0 / away.len2().sqrt()) } else { fall_dir(V2::default(), id as usize) };
            pushes.push((id, Vec3::new(dir.x, 0.0, dir.y) * dmg));
        }
    }
}

/// Least speed a shove must give rubble to move it at all, and below which sliding rubble stops.
const RUBBLE_MIN_SPEED: f32 = 0.3;
/// A piece of debris shoved at least this fast flies (and falls again); slower ones slide.
const RUBBLE_THROW: f32 = 2.5;
/// Fastest a shove throws a piece of debris, and slides a whole wreck.
const RUBBLE_MAX_PIECE: f32 = 10.0;
const RUBBLE_MAX_WRECK: f32 = 2.0;
/// Sliding friction (times gravity), and the share of its ground speed a thrown piece keeps when it lands.
const RUBBLE_FRICTION: f32 = 0.8;
const RUBBLE_LAND_KEEP: f32 = 0.3;

/// Applies the shoves blasts and shots gave wrecks and debris this tick: each (wreck id, impulse
/// = damage along its direction) adds `rules.rubble_push` x impulse / sqrt(mass) to its ground
/// speed, the mass being what it is still worth (a wreck half taken apart is lighter; the square
/// root so that a tank hulk still budges under a direct hit while small pieces fly). Pieces shoved
/// hard enough fly off and fall again; the rest slide (`rubble_motion`). Wrecks of buildings,
/// pieces still in the air and tiny shoves do nothing.
fn push_wrecks(pushes: &[(u32, Vec3)], q: &mut Query<(Entity, &mut Wreck)>, defs: &Defs) {
    if pushes.is_empty() || defs.rules.rubble_push <= 0.0 {
        return;
    }
    let mut sum: Vec<(u32, Vec3)> = vec![];
    for &(id, j) in pushes {
        match sum.iter_mut().find(|s| s.0 == id) {
            Some(s) => s.1 += j,
            None => sum.push((id, j)),
        }
    }
    for (_, mut w) in q.iter_mut().sort_by_key::<&Wreck, _>(|w| w.id) {
        let Some(&(_, j)) = sum.iter().find(|s| s.0 == w.id) else { continue };
        let piece = w.piece > 0.0;
        if w.mass <= 0.0 || w.falling || (!piece && defs.get(w.def).is_structure()) {
            continue;
        }
        let dv = j * (defs.rules.rubble_push / w.mass.max(defs.rules.debris.min_mass.max(0.5)).sqrt());
        let mut v = w.vel.v();
        v.x += dv.x;
        v.z += dv.z;
        let speed = (v.x * v.x + v.z * v.z).sqrt();
        if speed < RUBBLE_MIN_SPEED {
            continue;
        }
        let max = if piece { RUBBLE_MAX_PIECE } else { RUBBLE_MAX_WRECK };
        if speed > max {
            v.x *= max / speed;
            v.z *= max / speed;
        }
        if piece && speed >= RUBBLE_THROW {
            // Thrown up and away, a little less far than it goes up.
            let s = speed.min(max);
            v.x *= 0.8;
            v.z *= 0.8;
            v.y = s * 0.6;
            w.falling = true;
        } else {
            v.y = 0.0;
        }
        w.vel = v.into();
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_reclaim(
    mut q: Query<(Entity, &mut Wreck)>,
    mut units: Query<(&Unit, &mut Health, Option<&Shed>)>,
    census: Res<Census>,
    defs: Res<Defs>,
    mut effects: ResMut<Effects>,
    mut game: ResMut<Game>,
    mut features: ResMut<Features>,
    mut commands: Commands,
) {
    // Trees and rocks: the work done takes its share of both their mass and their energy.
    let feature_jobs: Vec<(u32, f32, u8)> = effects.reclaim_feature.drain(..).collect();
    for (fid, amount, player) in feature_jobs {
        let Some(f) = features.get(fid).copied() else { continue };
        let Some(fd) = defs.rules.features.get(f.def as usize) else { continue };
        let take = (amount / fd.work(&defs.rules).max(1e-3)).min(f.left);
        if let Some(p) = game.players.get_mut(player as usize) {
            p.mass = (p.mass + fd.mass * take).min(p.max_mass.max(1.0));
            p.energy = (p.energy + fd.energy * take).min(p.max_energy.max(1.0));
        }
        features.set_left((fid - FEATURE_ID0) as usize, f.left - take);
    }
    // Standing buildings: every hit point taken is worth its share of the wreck value.
    let unit_jobs: Vec<(u32, f32, u8)> = effects.reclaim_unit.drain(..).collect();
    for (id, amount, player) in unit_jobs {
        let Some((u, mut h, shed)) = census.get(id).and_then(|c| units.get_mut(c.e).ok()) else { continue };
        if h.0 <= 0.0 {
            continue;
        }
        let d = defs.get(u.def);
        // What it already dropped as debris is not in it any more.
        let worth = (d.cost.mass * defs.rules.wreck_mass_fraction - shed.map_or(0.0, |s| s.0)).max(0.0);
        let per_hp = (worth / d.hp.max(1.0)).max(1e-4);
        let hp = (amount / per_hp).min(h.0);
        h.0 -= hp;
        if let Some(p) = game.players.get_mut(player as usize) {
            p.mass = (p.mass + hp * per_hp).min(p.max_mass.max(1.0));
        }
        if h.0 <= 0.001 {
            h.0 = 0.0;
            effects.reclaimed.push(id);
        }
    }
    if effects.reclaim.is_empty() {
        return;
    }
    let jobs: Vec<(u32, f32, u8)> = effects.reclaim.drain(..).collect();
    for (e, mut w) in q.iter_mut().sort_by_key::<&Wreck, _>(|w| w.id) {
        for (id, amount, player) in &jobs {
            if *id == w.id && w.mass > 0.0 {
                let take = amount.min(w.mass);
                w.mass -= take;
                if let Some(p) = game.players.get_mut(*player as usize) {
                    p.mass = (p.mass + take).min(p.max_mass.max(1.0));
                }
            }
        }
        if w.mass <= 0.01 {
            commands.entity(e).despawn();
        }
    }
}

/// `t` is a building infantry of `team` may take over right now: finished, standing, known, not
/// theirs and `seizable` (never factories or field works; of the garrisons only bunkers and other
/// military ones, and only once nobody is inside: a held one is stormed). Guarded ones can be walked
/// up to and waited at, but not taken.
fn can_seize(census: &Census, defs: &Defs, team: u8, t: &Cen) -> bool {
    t.hp > 0.0
        && t.done
        && !t.hosted
        && t.team != team
        && t.known_to(team)
        && seizable(defs.get(t.def))
        && !(defs.get(t.def).is_garrison() && census.list.iter().any(|r| r.host == t.id && r.hp > 0.0))
}

/// The nearest building within `infantry_capture_reach` that infantry of `team` standing at `pos`
/// take over by themselves (lowest id among equals).
fn seize_target(census: &Census, defs: &Defs, team: u8, pos: V2) -> Option<Cen> {
    let r = defs.rules.infantry_capture_reach;
    let mut best: Option<(f32, Cen)> = None;
    census.near(pos, r + 8.0, |t| {
        if !t.structure || !can_seize(census, defs, team, t) || guarded(census, defs, t) {
            return;
        }
        let d = pos.dist(t.pos);
        if d > r + t.radius {
            return;
        }
        if best.as_ref().map_or(true, |b| d < b.0 || (d == b.0 && t.id < b.1.id)) {
            best = Some((d, *t));
        }
    });
    best.map(|b| b.1)
}

/// An armed, finished mobile unit of `t`'s side stands within `infantry_capture_guard` of it:
/// infantry cannot capture it.
fn guarded(census: &Census, defs: &Defs, t: &Cen) -> bool {
    let r = defs.rules.infantry_capture_guard;
    let mut any = false;
    census.near(t.pos, r + t.radius, |c| {
        if !any && c.team == t.team && !c.structure && !c.hosted && c.done && c.hp > 0.0 && c.pos.dist(t.pos) <= r + t.radius && defs.get(c.def).armed() {
            any = true;
        }
    });
    any
}

// **Infantry capture** (lobby option "Infantry capture buildings", `Game.infantry_capture`, on by default (requested; headless tests: Game default off, `--selftest` on); Cossacks
// style): infantry may take `Order::Capture` (right-click enemy or neutral building, or Capture button) on any
// building `sim::seizable` (every turret; not factories or field works; of garrisons only bunkers and other
// military ones - armed, or built by side - and only once empty, `systems::can_seize`: held ones are stormed). At building it queues
// `Effects.seize`; `apply_capture` adds `DT / rules.infantry_capture_secs` (20) per man, free of energy, but nothing
// while armed finished mobile unit of building's side is within `rules.infantry_capture_guard` (15,
// `systems::guarded`). With infantry and other units selected, infantry capture and rest attack. Zone AI
// sends up to 4 infantry (6 at turret) within 40 at known unguarded enemy building nobody holds (report: `captures`). **No order needed**: idle infantry, and infantry
// on Move, AttackMove or Patrol (not engaged), take nearest unguarded `seizable` building within
// `rules.infantry_capture_reach` (12; `systems::seize_target`, in `think`) - walk up and capture, then go on
// with order. Houses, factories and field works (trenches) are never taken this way (own behaviour: entered, stormed, cover). `--combattest` infantry "Infantry capture" lines (ordered, idle, moving,
// attack-moving, garrison/factory refused).
/// Capture work queued by engineers (and infantry, `Effects.seize`) this tick. A unit nobody
/// works on slowly shakes it off.
#[allow(clippy::type_complexity)]
fn apply_capture(
    mut q: Query<(Entity, &mut Unit, &mut Orders, Option<&mut Capture>, Option<&mut Factory>, Option<&mut Weapons>, Option<&mut Mover>)>,
    mut effects: ResMut<Effects>,
    census: Res<Census>,
    defs: Res<Defs>,
    mut game: ResMut<Game>,
    mut commands: Commands,
) {
    let jobs: Vec<(u32, f32, u8)> = effects.capture.drain(..).collect();
    let seize: Vec<(u32, u8)> = effects.seize.drain(..).collect();
    for (e, mut u, mut orders, cap, factory, weapons, mover) in q.iter_mut().sort_by_key::<&Unit, _>(|u| u.id) {
        let Some(by) = jobs.iter().find(|j| j.0 == u.id).map(|j| j.2).or_else(|| seize.iter().find(|j| j.0 == u.id).map(|j| j.1)) else {
            if let Some(mut c) = cap {
                c.progress -= DT * 0.1;
                if c.progress <= 0.0 {
                    commands.entity(e).remove::<Capture>();
                }
            }
            continue;
        };
        // The first capturer's side does the capturing; others of that player add to it.
        let work: f32 = jobs.iter().filter(|j| j.0 == u.id && j.2 == by).map(|j| j.1).sum();
        let d = defs.get(u.def);
        let step = work / (d.cost.time.max(1.0) * defs.rules.capture_time);
        let energy = d.cost.energy * defs.rules.capture_energy * step;
        let Some(pl) = game.players.get_mut(by as usize) else { continue };
        // Infantry work is free (an engineer short of energy still lets them go on).
        let men = seize.iter().filter(|j| j.0 == u.id && j.1 == by).count() as f32;
        let mut step = step;
        if pl.energy < energy {
            step = 0.0;
        } else {
            pl.energy -= energy;
        }
        step += men * DT / defs.rules.infantry_capture_secs.max(0.1);
        if step <= 0.0 {
            continue;
        }
        let progress = cap.as_ref().filter(|c| c.by == by).map_or(0.0, |c| c.progress) + step;
        if progress < 1.0 {
            match cap {
                Some(mut c) => *c = Capture { by, progress },
                None => {
                    commands.entity(e).insert(Capture { by, progress });
                }
            }
            continue;
        }
        // Taken over: it forgets what it was doing.
        u.owner = by;
        orders.0.clear();
        commands.entity(e).remove::<Capture>();
        if let Some(mut f) = factory {
            // (Riders already inside a transport being filled go with it: their host is gone.)
            for id in [f.building, f.carrier] {
                if let Some(p) = census.get(id) {
                    commands.entity(p.e).despawn();
                }
            }
            *f = Factory::default();
        }
        if let Some(mut w) = weapons {
            w.focus = 0;
            w.ground = None;
            w.list.iter_mut().for_each(|s| s.target = 0);
        }
        if let Some(mut m) = mover {
            m.goal = None;
            m.path.clear();
        }
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn deaths(
    mut q: Query<(Entity, &Unit, &Body, &mut Health, Option<&UnderConstruction>, Option<&Hosted>, Option<&Shed>, Has<Revival>)>,
    mut wrecks: Query<(Entity, &mut Wreck)>,
    (mut features, wreck_list): (ResMut<Features>, Res<WreckList>),
    census: Res<Census>,
    defs: Res<Defs>,
    map: Res<MapData>,
    mut game: ResMut<Game>,
    mut effects: ResMut<Effects>,
    mut nav: ResMut<Nav>,
    mut fx: ResMut<FxLog>,
    mut rng: ResMut<SimRng>,
    mut commands: Commands,
) {
    let dr = &defs.rules.debris;
    let drops = dr.enabled && defs.rules.wrecks;
    // Mass buildings dropped as debris this tick, on top of their `Shed`: (unit id, entity, mass).
    let mut shed_now: Vec<(u32, Entity, f32)> = vec![];
    let mut new_pieces = 0;
    // Infantry inside a building take a small share of every hit on it: (rider, raw damage, type).
    let mut riders_hit: Vec<(u32, f32, usize)> = vec![];
    for (id, dmg, dtype) in effects.damage.drain(..) {
        let Some(c) = census.get(id) else { continue };
        let Ok((e, u, b, mut h, uc, hosted, shed, _)) = q.get_mut(c.e) else { continue };
        let d = defs.get(u.def);
        let real = dmg * defs.armor_mult(dtype, d.armor_id);
        let standing = h.0 > 0.0;
        h.0 -= real;
        if standing && d.is_garrison() && defs.rules.garrison_damage > 0.0 {
            for r in census.list.iter().filter(|r| r.host == id && r.done) {
                riders_hit.push((r.id, dmg * defs.rules.garrison_damage, dtype));
            }
        }
        // A hard hit on a finished building breaks pieces off it: their mass leaves its wreck value.
        if drops && standing && uc.is_none() && hosted.is_none() && d.is_structure() && d.wreck && !d.is_air() && real >= dr.min_hit * d.hp {
            let value = d.cost.mass * defs.rules.wreck_mass_fraction;
            let done = shed.map_or(0.0, |s| s.0) + shed_now.iter().filter(|s| s.0 == id).map(|s| s.2).sum::<f32>();
            let share = (real / d.hp.max(1.0)).min(1.0);
            let mass = (share * value * dr.hit_share).min(value * dr.max_shed - done);
            let n = ((mass / dr.min_mass.max(0.01)) as u32).min(dr.hit_pieces.max(1)).min(1 + (share / 0.04) as u32);
            if n > 0 {
                new_pieces += drop_debris(&mut commands, &mut game, &mut rng.0, d, u.def, b, mass, n, false);
                match shed_now.iter_mut().find(|s| s.0 == id) {
                    Some(s) => s.2 += mass,
                    None => shed_now.push((id, e, mass)),
                }
            }
        }
    }
    for (id, dmg, dtype) in riders_hit {
        let Some(c) = census.get(id) else { continue };
        if let Ok((_, u, _, mut h, ..)) = q.get_mut(c.e) {
            h.0 -= dmg * defs.armor_mult(dtype, defs.get(u.def).armor_id);
        }
    }
    // (Queued before the despawns below: a building destroyed this tick gets it first, harmlessly.)
    for &(_, e, mass) in &shed_now {
        let old = q.get(e).ok().and_then(|x| x.6.copied()).map_or(0.0, |s| s.0);
        commands.entity(e).insert(Shed(old + mass));
    }
    let tick = game.tick;
    let mut blasts: Vec<(V2, f32, f32, f32, u8)> = vec![];
    let mut new_wrecks = 0;
    let reclaimed = std::mem::take(&mut effects.reclaimed);
    for (e, u, b, mut h, uc, hosted, shed, revived) in q.iter_mut().sort_by_key::<&Unit, _>(|u| u.id) {
        // A revived unit (or revival site) leaves a wreck that must cool down before it is revived again.
        let revive_at = if revived { tick + (defs.rules.revive_cooldown / DT).round() as u32 } else { 0 };
        let d = defs.get(u.def);
        let player_dead = game.players.get(u.owner as usize).map_or(false, |p| !p.alive);
        if player_dead && (u.id + tick) % 6 == 0 {
            h.0 = -1.0;
        }
        if let Some(host) = hosted {
            // Riders die with their transport (a commander inside takes its side with it).
            // Killed inside (the fight for a building, self-destruct): gone without a wreck.
            if census.get(host.0).is_none() || h.0 <= 0.0 {
                commands.entity(e).despawn();
                if game.players.get(u.owner as usize).map_or(false, |p| defs.factions[p.faction as usize].commander == Some(u.def)) {
                    game.players[u.owner as usize].alive = false;
                }
            }
            continue;
        }
        if h.0 > 0.0 {
            continue;
        }
        commands.entity(e).despawn();
        if d.is_structure() {
            nav.dirty = true;
        }
        let carrier = d.transport.is_some() && d.mv.kind == MoveKind::Land;
        if d.is_garrison() || carrier {
            // Infantry inside are thrown out of the ruin, hurt (unless it was taken apart quietly);
            // infantry riding an armoured carrier bail out of the wreck the same way (anything else
            // in it - vehicles, a commander - dies with it).
            let inside: Vec<&Cen> = census.list.iter().filter(|r| r.host == u.id && r.done && (!carrier || defs.get(r.def).is_infantry())).collect();
            for (k, r) in inside.iter().enumerate() {
                let p = set_down(&map, &nav, b.pos + V2::from_angle(k as f32 * 2.4 + u.id as f32) * (d.radius() + 1.0));
                commands.entity(r.e).try_remove::<Hosted>().try_insert((Body { pos: p, y: map.h(p), yaw: (p - b.pos).angle(), vel: v2(0.0, 0.0) }, Orders::default(), Mover { anchor: p, path_goal: p, ..default() }));
                if !reclaimed.contains(&u.id) {
                    effects.damage.push((r.id, r.max_hp * defs.rules.garrison_eject_damage, 0));
                }
            }
        }
        // Taken apart by an engineer: nothing is left, and nothing blows up.
        if reclaimed.contains(&u.id) {
            continue;
        }
        fx.push(b.pos.at(b.y + d.height() * 0.5), FX_DEATH, d.radius() * 1.6 + 0.6);
        // A wreck holds a share of the mass that actually went into the unit.
        let invested = d.cost.mass * uc.map_or(1.0, |x| x.0.clamp(0.0, 1.0));
        if uc.is_some() {
            if d.wreck && defs.rules.wrecks && !d.is_air() && invested * defs.rules.wreck_mass_fraction >= 5.0 {
                let id = game.alloc();
                spawn_wreck(&mut commands, Wreck { id, def: u.def, mass: invested * defs.rules.wreck_mass_fraction, pos: b.pos, y: b.y, yaw: b.yaw, revive_at, ..default() });
                new_wrecks += 1;
            }
            continue;
        }
        if let Some(blast) = &d.death {
            blasts.push((b.pos, b.y, blast.damage, blast.radius, u.owner));
            fx.push(b.pos.at(b.y + 1.0), FX_BLAST, blast.radius);
        }
        let is_commander = game.players.get(u.owner as usize).map_or(false, |p| defs.factions[p.faction as usize].commander == Some(u.def));
        if is_commander {
            game.players[u.owner as usize].alive = false;
        } else if d.wreck && defs.rules.wrecks && !d.is_air() {
            // What its debris already took is not in the wreck; a destroyed building spills more around it.
            let shed = shed.map_or(0.0, |s| s.0) + shed_now.iter().filter(|s| s.0 == u.id).map(|s| s.2).sum::<f32>();
            let mut mass = (d.cost.mass * defs.rules.wreck_mass_fraction - shed).max(0.0);
            if drops && d.is_structure() {
                let burst = mass * dr.death_share;
                let n = ((burst / dr.min_mass.max(0.01)) as u32).min(dr.death_pieces);
                if n > 0 {
                    new_pieces += drop_debris(&mut commands, &mut game, &mut rng.0, d, u.def, b, burst, n, true);
                    mass -= burst;
                }
            }
            if mass >= 5.0 {
                let id = game.alloc();
                spawn_wreck(&mut commands, Wreck { id, def: u.def, mass, pos: b.pos, y: b.y, yaw: b.yaw, revive_at, ..default() });
                new_wrecks += 1;
            }
        }
    }
    let mut pushes: Vec<(u32, Vec3)> = vec![];
    let blast_type = 3.min(defs.rules.damage_types.len() - 1);
    for (pos, y, damage, radius, owner) in blasts {
        hurt_reclaimables(pos.at(y + 0.5), radius, damage, &map, &defs, &mut features, &wreck_list, &mut pushes);
        let team = game.team_of(owner);
        if defs.rules.craters_on() && y - map.h(pos) < 3.0 {
            effects.craters.push((pos, radius * 0.4, defs.rules.crater_scale * damage.sqrt()));
        }
        census.near(pos, radius + 4.0, |c| {
            let d = c.hit_dist(pos.at(y));
            if d < radius && c.hp > 0.0 && !c.hosted && (c.team != team || defs.rules.friendly_fire || owner >= NEUTRAL) {
                effects.damage.push((c.id, damage * blast_share(d, radius) * splash_armor(&defs, c), blast_type));
            }
        });
    }
    push_wrecks(&pushes, &mut wrecks, &defs);
    // Keep the number of wrecks and of pieces of debris bounded (each on its own): drop the oldest.
    let (mut whole, mut pieces) = (new_wrecks, new_pieces);
    for (_, w) in &wrecks {
        if w.piece > 0.0 {
            pieces += 1;
        } else {
            whole += 1;
        }
    }
    if whole > defs.rules.max_wrecks || pieces > dr.max {
        let mut all: Vec<(u32, Entity, bool)> = wrecks.iter().map(|(e, w)| (w.id, e, w.piece > 0.0)).collect();
        all.sort_by_key(|x| x.0);
        let (mut cut_whole, mut cut_pieces) = (whole.saturating_sub(defs.rules.max_wrecks), pieces.saturating_sub(dr.max));
        for (_, e, piece) in all {
            let cut = if piece { &mut cut_pieces } else { &mut cut_whole };
            if *cut > 0 {
                *cut -= 1;
                commands.entity(e).despawn();
            }
        }
    }
}

/// Drops `n` pieces of debris holding `mass` in all from building `d` standing at `b`: off its
/// walls, falling close by (a hard hit), or spilling out all round (`burst`: it was destroyed).
/// They fall in `rubble_motion`. Returns how many it made.
#[allow(clippy::too_many_arguments)]
fn drop_debris(commands: &mut Commands, game: &mut Game, rng: &mut Rng, d: &crate::data::UnitDef, def: DefId, b: &Body, mass: f32, n: u32, burst: bool) -> usize {
    let weights: Vec<f32> = (0..n).map(|_| rng.range(0.6, 1.4)).collect();
    let sum: f32 = weights.iter().sum::<f32>().max(1e-3);
    let (r, h) = (d.radius(), d.height());
    for w in weights {
        let m = mass * w / sum;
        let dir = V2::from_angle(rng.range(-PI, PI));
        // From the wall of the (square, unrotated) footprint in that direction.
        let edge = r / dir.x.abs().max(dir.y.abs()).max(0.5);
        let start = b.pos + dir * (edge * rng.range(0.85, 1.0));
        let y = b.y + h * if burst { rng.range(0.3, 0.8) } else { rng.range(0.5, 0.95) };
        // A hit: pieces break off and drop almost straight down. Destroyed: they spill out further.
        let (out, up) = if burst { (rng.range(1.0, 2.6), rng.range(0.6, 1.8)) } else { (rng.range(0.3, 1.1), rng.range(0.0, 0.6)) };
        let id = game.alloc();
        let yaw = rng.range(-PI, PI);
        // Bigger for heavier pieces and bigger buildings.
        let piece = (0.2 + 0.1 * m.sqrt() + 0.07 * r).min(2.5);
        spawn_wreck(commands, Wreck { id, def, mass: m, pos: start, y, yaw, piece, falling: true, vel: Vec3::new(dir.x * out, up, dir.y * out).into(), revive_at: 0 });
    }
    n as usize
}

/// Rubble on the move. Falling debris: gravity until it reaches the ground, where it hurts the
/// units it lands on, keeps a little of its speed and then lies as a reclaimable piece. Shoved
/// wrecks and landed pieces (`push_wrecks`) slide on the ground: friction slows them, slopes
/// speed them up or down, a building's footprint stops them.
fn rubble_motion(mut q: Query<&mut Wreck>, census: Res<Census>, defs: Res<Defs>, map: Res<MapData>, nav: Res<Nav>, mut effects: ResMut<Effects>, mut fx: ResMut<FxLog>) {
    let g = defs.rules.gravity;
    let dr = &defs.rules.debris;
    for mut w in q.iter_mut().sort_by_key::<&Wreck, _>(|w| w.id) {
        if !w.falling {
            let v = w.vel.v();
            if v.x == 0.0 && v.z == 0.0 {
                continue;
            }
            let mut v = v2(v.x, v.z);
            // Downhill pull, then friction against the motion.
            let (e, p) = (0.5, w.pos);
            let grad = v2(map.h(p + v2(e, 0.0)) - map.h(p - v2(e, 0.0)), map.h(p + v2(0.0, e)) - map.h(p - v2(0.0, e))) * (1.0 / (2.0 * e));
            v = v - grad * (g * DT);
            let speed = v.len2().sqrt();
            let slow = RUBBLE_FRICTION * g * DT;
            if speed <= slow || speed < RUBBLE_MIN_SPEED * 0.5 {
                w.vel = V3::default();
                continue;
            }
            v = v * ((speed - slow) / speed);
            let next = map.clamp(p + v * DT);
            // Runs into a building (one already standing on it may move off).
            if nav.at(&map, next) == 2 && nav.at(&map, p) != 2 {
                w.vel = V3::default();
                continue;
            }
            w.pos = next;
            w.y = map.h(next);
            w.vel = Vec3::new(v.x, 0.0, v.y).into();
            continue;
        }
        let mut v = w.vel.v();
        v.y -= g * DT;
        let pos = map.clamp(w.pos + v2(v.x * DT, v.z * DT));
        let y = w.y + v.y * DT;
        let ground = map.h(pos);
        w.pos = pos;
        if y > ground {
            w.y = y;
            w.vel = v.into();
            continue;
        }
        w.y = ground;
        w.falling = false;
        let keep = v2(v.x, v.z) * RUBBLE_LAND_KEEP;
        w.vel = if keep.len2() >= RUBBLE_MIN_SPEED * RUBBLE_MIN_SPEED { Vec3::new(keep.x, 0.0, keep.y).into() } else { V3::default() };
        fx.push(pos.at(ground + 0.1), FX_DEBRIS, w.piece);
        let dmg = (w.mass * dr.land_damage).min(dr.land_damage_max);
        if dmg > 0.0 {
            // Units only: a piece landing next to a building falls past its walls.
            let r = w.piece * 0.5;
            census.near(pos, r + 6.0, |c| {
                if c.hp > 0.0 && !c.hosted && !c.air && !c.structure && c.pos.dist(pos) < c.radius + r {
                    effects.damage.push((c.id, dmg, 0));
                }
            });
        }
    }
}

/// Applies this tick's craters and terraforming to the terrain.
#[allow(clippy::type_complexity)]
fn apply_ground(
    mut effects: ResMut<Effects>,
    mut map: ResMut<MapData>,
    mut ground: ResMut<Ground>,
    mut nav: ResMut<Nav>,
    mut game: ResMut<Game>,
    defs: Res<Defs>,
    census: Res<Census>,
    mut works: Query<(&Unit, &mut Body, &Health), With<Structure>>,
) {
    let rules = &defs.rules;
    let (map, ground) = (&mut *map, &mut *ground);
    // Trenches are dug as they are built (the depth follows the site's hit points), and field
    // works sit on ground that shells may move: they follow it.
    let mut dug = false;
    if census.list.iter().any(|c| c.fieldwork) {
        for (u, mut b, h) in works.iter_mut().sort_by_key::<&Unit, _>(|u| u.id) {
            let d = defs.get(u.def);
            let Some((_, dig)) = d.work() else { continue };
            if dig > 0.0 && h.0 > 0.0 {
                let before = ground.hash;
                let built = if census.get(u.id).map_or(false, |c| c.done) { 1.0 } else { (h.0 / d.hp.max(1.0)).min(1.0) };
                dig_trench(map, ground, &nav.occ, b.pos, dig * built);
                dug |= ground.hash != before;
            }
            let y = map.h(b.pos);
            if b.y != y {
                b.y = y;
            }
        }
    }
    if effects.craters.is_empty() && effects.terraform.is_empty() {
        nav.dirty |= dug;
        return;
    }
    for (pos, radius, depth) in effects.craters.drain(..) {
        dig_crater(map, ground, &nav.occ, pos, radius.min(12.0), depth.min(2.0), rules.crater_max_depth);
    }
    for (pos, radius, height, volume, player) in effects.terraform.drain(..) {
        let targets = terraform_targets(map, ground, &nav.occ, pos, radius, height, rules.terraform_max);
        let total: f32 = targets.iter().map(|(i, t)| (t - map.heights[*i]).abs()).sum();
        let Some(pl) = game.players.get_mut(player as usize) else { continue };
        if total <= 0.0 {
            continue;
        }
        // Moving earth takes energy; with too little of it the work slows down.
        let mut moved = volume.min(total);
        let cost = moved * rules.terraform_energy;
        if cost > pl.energy {
            moved *= (pl.energy / cost).clamp(0.0, 1.0);
        }
        pl.energy = (pl.energy - moved * rules.terraform_energy).max(0.0);
        let share = moved / total;
        for (i, t) in targets {
            let h = map.heights[i];
            ground.set(map, i, if share >= 0.999 { t } else { h + (t - h) * share });
        }
    }
    // Slopes changed: the next tick rebuilds the navigation grid from the new passability.
    nav.dirty = true;
}

// **Control points** (`Game.mode == MODE_POINTS`): positions come from map (`MapData.points`, map.json
// `points`, placed with editor's "Control point" tool like starts; `MapData::generate` and premade maps have them);
// map without any gets `MapData::auto_points` (`MapData::control_points` picks). `Game.points` / `Game.scores` (points
// left per team, start `Game.cp_start`: typed in lobby, else `rules.cp_start_points`, default 8000). A team holding fewer points than its strongest opponent loses
// `cp_bleed` per second per point of difference, equal holdings change nothing, team at 0 is defeated. Rules `cp_*`.
/// Control points mode: a team alone at a point takes it over. A team that holds fewer points
/// than its strongest opponent loses score for every point of difference (nothing moves while
/// they hold equally many); a team that runs out is defeated.
fn control_points(mut game: ResMut<Game>, census: Res<Census>, defs: Res<Defs>) {
    if game.mode != MODE_POINTS || game.winner.is_some() {
        return;
    }
    let rules = &defs.rules;
    let step = DT / rules.cp_capture_secs.max(0.1);
    let game = &mut *game;
    if game.scores.len() < 16 {
        game.scores.resize(16, game.cp_start);
    }
    let mut alive = 0u16;
    for p in &game.players {
        if p.alive && p.team < 14 {
            alive |= 1 << p.team;
        }
    }
    // Riders inside buildings (garrisons, trenches) hold a point too: (building id, rider's team).
    let mut inside: Vec<(u32, u8)> = Vec::new();
    for c in &census.list {
        if c.hosted && c.done && c.hp > 0.0 && c.team < 14 {
            if let Some(h) = census.get(c.host) {
                if defs.get(h.def).is_garrison() {
                    inside.push((h.id, c.team));
                }
            }
        }
    }
    for p in game.points.iter_mut() {
        // Only finished ground units of living players hold a point (no aircraft, no buildings),
        // and the riders of a building on it.
        let mut teams = 0u16;
        census.near(p.pos, rules.cp_radius, |c| {
            if c.done && !c.hosted && !c.air && !c.structure && c.hp > 0.0 && c.team < 14 {
                teams |= 1 << c.team;
            }
            if c.structure && !inside.is_empty() {
                for &(b, t) in &inside {
                    if b == c.id {
                        teams |= 1 << t;
                    }
                }
            }
        });
        teams &= alive;
        if teams.count_ones() == 1 {
            let t = teams.trailing_zeros() as u8;
            if p.owner == t {
                p.by = t;
                p.progress = 0.0;
            } else if p.by == t {
                p.progress += step;
                if p.progress >= 1.0 {
                    p.owner = t;
                    p.progress = 0.0;
                }
            } else {
                // Somebody else's capture has to be undone first.
                p.progress -= step * 2.0;
                if p.progress <= 0.0 {
                    p.by = t;
                    p.progress = 0.0;
                }
            }
        } else if teams == 0 && p.progress > 0.0 {
            p.progress = (p.progress - step * 0.5).max(0.0);
        }
    }
    let mut held = [0u32; 16];
    for p in &game.points {
        if p.owner < 14 && alive & (1 << p.owner) != 0 {
            held[p.owner as usize] += 1;
        }
    }
    let mut out = 0u16;
    for t in 0..14usize {
        if alive & (1 << t) == 0 {
            continue;
        }
        let lead = (0..14usize).filter(|&o| o != t && alive & (1 << o) != 0).map(|o| held[o]).max().unwrap_or(0);
        if lead > held[t] {
            game.scores[t] = (game.scores[t] - (lead - held[t]) as f32 * rules.cp_bleed.max(0.0) * DT).max(0.0);
            if game.scores[t] <= 0.0 {
                out |= 1 << t;
            }
        }
    }
    if out != 0 {
        // Out of points: the team is defeated (`deaths` removes what it owned).
        for p in game.players.iter_mut() {
            if p.team < 14 && out & (1 << p.team) != 0 {
                p.alive = false;
            }
        }
        let left = alive & !out;
        if left.count_ones() == 1 {
            game.winner = Some(left.trailing_zeros() as u8);
        }
    }
}

fn victory(mut game: ResMut<Game>) {
    if game.winner.is_some() || game.players.len() < 2 {
        return;
    }
    let mut teams: Vec<u8> = game.players.iter().filter(|p| p.alive).map(|p| p.team).collect();
    teams.sort();
    teams.dedup();
    if teams.len() == 1 {
        game.winner = Some(teams[0]);
    } else if teams.is_empty() {
        game.winner = Some(255);
    }
}
