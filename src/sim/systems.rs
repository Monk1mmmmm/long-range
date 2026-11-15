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
    app.add_systems(
        SimTick,
        (
            // Inner tuples need their own chain(): the outer one only orders the groups.
            (begin_tick, census_build, crate::ai::ai_system, apply_commands, visibility, income).chain(),
            (think, factories, build_progress, locomotion, separation, carry).chain(),
            (weapons, projectiles, apply_reclaim, apply_capture, deaths, apply_ground, control_points, victory).chain(),
        )
            .chain(),
    );
}

fn begin_tick(
    mut game: ResMut<Game>,
    mut nav: ResMut<Nav>,
    mut fx: ResMut<FxLog>,
    mut effects: ResMut<Effects>,
    map: Res<MapData>,
    defs: Res<Defs>,
    structures: Query<(&Unit, &Body), With<Structure>>,
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
            nav.stamp(&map, b.pos, defs.get(u.def).footprint, 2);
        }
        nav.dirty = false;
    }
}

#[allow(clippy::type_complexity)]
fn census_build(
    mut census: ResMut<Census>,
    mut wrecks: ResMut<WreckList>,
    game: Res<Game>,
    defs: Res<Defs>,
    q: Query<(Entity, &Unit, &Body, &Health, &Intel, Has<UnderConstruction>, Option<&Hosted>, Option<&Builder>, Option<&Factory>, Has<Upgrading>, Has<Paused>)>,
    wq: Query<&Wreck>,
) {
    census.clear();
    for (e, u, b, h, i, uc, host, builder, factory, upgrading, paused) in q.iter().sort_by_key::<&Unit, _>(|u| u.id) {
        let d = defs.get(u.def);
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
            seen: i.seen,
            radar: i.radar,
            work: builder.map_or(0, |b| b.work),
            building: factory.map_or(0, |f| f.building),
            upgrading,
            paused,
        });
    }
    wrecks.0.clear();
    wrecks.0.extend(wq.iter().map(|w| (w.id, w.pos, w.mass, w.def)));
    wrecks.0.sort_by_key(|w| w.0);
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn apply_commands(
    mut queue: ResMut<CommandQueue>,
    log: Option<ResMut<TurnLog>>,
    census: Res<Census>,
    defs: Res<Defs>,
    map: Res<MapData>,
    game: Res<Game>,
    mut effects: ResMut<Effects>,
    mut commands: Commands,
    mut q: Query<(&Unit, &Body, &mut Orders, Option<&mut Factory>, Option<&mut Mover>, Option<&mut Weapons>, Has<Upgrading>, Has<UnderConstruction>)>,
) {
    if let Some(mut log) = log {
        log.0 = queue.0.clone();
    }
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
                    // Units riding in a transport take no orders.
                    if census.get(*id).map_or(false, |c| c.hosted && c.done) {
                        continue;
                    }
                    let Ok((u, b, mut ord, factory, mover, _, _, _)) = q.get_mut(e) else { continue };
                    let df = defs.get(u.def);
                    let mut o = if orders.len() == ids.len() { orders[k] } else { orders[0] };
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
                            if !df.is_engineer() || !radius.is_finite() || !height.is_finite() {
                                continue;
                            }
                            *pos = map.clamp(*pos);
                            *radius = radius.clamp(1.0, 24.0);
                        }
                        Order::LoadArea { pos, radius } | Order::Unload { pos, radius } => {
                            if df.transport.is_none() || !radius.is_finite() {
                                continue;
                            }
                            *pos = map.clamp(*pos);
                            *radius = radius.clamp(0.0, 60.0);
                        }
                        Order::Load(_) if df.transport.is_none() && !carriable(df) => continue,
                        Order::Build { def: bd, pos } => {
                            if !df.builds.contains(bd) || !df.is_engineer() {
                                continue;
                            }
                            *pos = snap(&defs, &map, *bd, *pos);
                        }
                        Order::Capture(_) if !df.is_engineer() => continue,
                        Order::Revive(_) if !df.is_scavenger() => continue,
                        // Scavengers work on wrecks only.
                        Order::Assist(_) | Order::Attack(_) if df.is_scavenger() => continue,
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
                        }
                    }
                    if ord.0.len() >= 96 {
                        continue;
                    }
                    if let Order::Patrol(_) = o {
                        let has = ord.0.iter().any(|x| matches!(x, Order::Patrol(_)));
                        ord.0.push(o);
                        if !has {
                            ord.0.push(Order::Patrol(b.pos));
                        }
                    } else {
                        ord.0.push(o);
                    }
                }
            }
            Command::Stop { ids } => {
                for id in ids {
                    let Some(e) = owned(id) else { continue };
                    let Ok((_, _, mut ord, factory, mover, weapons, upgrading, _)) = q.get_mut(e) else { continue };
                    ord.0.clear();
                    if let Some(mut m) = mover {
                        m.path.clear();
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
                            f.queue.push(d);
                        }
                    }
                } else {
                    for _ in 0..(-count).min(50) {
                        let Some(k) = f.queue.iter().rposition(|x| *x == d) else { break };
                        if k == 0 && f.building != 0 {
                            if let Some(p) = census.get(f.building) {
                                commands.entity(p.e).despawn();
                            }
                            f.building = 0;
                        }
                        f.queue.remove(k);
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
                let first = f.queue.first().copied();
                let run: Vec<DefId> = f.queue.drain(from..from + len).collect();
                let to = (to as usize).min(f.queue.len());
                for (k, d) in run.into_iter().enumerate() {
                    f.queue.insert(to + k, d);
                }
                // Something else is at the front now: the unit in the works is scrapped.
                if f.queue.first().copied() != first && f.building != 0 {
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
                if index == 0 && f.building != 0 {
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
}

/// Who sees whom. Both sight and radar need a clear line over the terrain; radar looks from higher
/// up and reaches further, sight tells what the contact is.
#[allow(clippy::too_many_arguments)]
fn visibility(mut census: ResMut<Census>, game: Res<Game>, defs: Res<Defs>, map: Res<MapData>, features: Res<Features>, wrecks: Res<WreckList>, mut q: Query<&mut Intel>) {
    // Trees, rocks and wrecks block sight like hills do (not radar).
    let block = sight_block(&features, &wrecks, &defs, &map);
    struct Viewer {
        pos: V2,
        eye: Vec3,
        look: Vec3,
        vis2: f32,
        radar2: f32,
        team: u8,
    }
    let mut viewers = vec![];
    for c in &census.list {
        if !c.done || c.hosted || c.team == TEAM_NEUTRAL {
            continue;
        }
        let d = defs.get(c.def);
        let power_out = game.players.get(c.owner as usize).map_or(false, |p| p.power_out);
        let radar = if power_out || c.paused { 0.0 } else { d.radar };
        if d.vision <= 0.0 && radar <= 0.0 {
            continue;
        }
        viewers.push(Viewer {
            pos: c.pos,
            eye: c.pos.at(c.y + c.height.max(1.0) + defs.rules.radar_los_height),
            look: c.pos.at(c.y + c.height.max(1.0) + defs.rules.vision_los_height),
            vis2: d.vision * d.vision,
            radar2: radar * radar,
            team: c.team,
        });
    }
    for c in census.list.iter_mut() {
        let own = 1u16 << c.team;
        let (mut seen, mut radar) = (own, own);
        let head = c.pos.at(c.y + c.height + 0.5);
        let stealth = defs.get(c.def).radar_stealth;
        for v in &viewers {
            let bit = 1u16 << v.team;
            if seen & bit != 0 && radar & bit != 0 {
                continue;
            }
            let d2 = (v.pos - c.pos).len2();
            // Anything this close is seen regardless (no hiding behind the viewer's own bump).
            if seen & bit == 0 && d2 < v.vis2 && (d2 < 36.0 || map.los_blocked(v.look, head, &block)) {
                seen |= bit;
            }
            if radar & bit == 0 && !stealth && d2 < v.radar2 && map.los(v.eye, head) {
                radar |= bit;
            }
        }
        c.seen = seen;
        c.radar = radar;
        if let Ok(mut i) = q.get_mut(c.e) {
            i.seen = seen;
            i.radar = radar;
        }
    }
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
        let (pos, id, owner) = (body.pos, u.id, u.owner);
        let team = game.team_of(owner);
        let is_struct = df.is_structure();
        let plane = df.is_plane();
        let arrive = if plane { 9.0 } else { 0.8 };
        let mut goal: Option<V2> = None;
        let mut pop = false;
        let mut work = 0u32;
        let mut focus = 0u32;
        let mut ground: Option<(V2, f32)> = None;
        let w0 = df.weapons.first();
        let reach = |target_r: f32| df.build_range().max(2.0) + target_r + df.radius();

        match orders.0.first().copied() {
            None => {
                if plane {
                    if let Some(w) = w0.filter(|w| w.hits_air) {
                        if let Some(t) = census.nearest_enemy(pos, team, w.range + 45.0, 0.0, false, true) {
                            goal = Some(t.pos);
                        }
                    }
                }
                // Engineering stations pick their own work.
                if df.is_station() {
                    let r = df.build_range();
                    let mut best: Option<(f32, u32)> = None;
                    census.near(pos, r + 6.0, |c| {
                        if c.team != team || c.id == id || c.hosted || c.hp <= 0.0 || pos.dist(c.pos) > r + c.radius {
                            return;
                        }
                        let job = if !c.done || c.upgrading {
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
            }
            Some(Order::Move(p)) => {
                if is_struct || pos.dist(p) <= arrive {
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
                if let Some(w) = w0 {
                    if plane {
                        if let Some(t) = census.nearest_enemy(pos, team, w.range + 45.0, 0.0, w.hits_land, w.hits_air) {
                            goal = Some(t.pos);
                            engaged = true;
                        }
                    } else if census.nearest_enemy(pos, team, w.range * 0.95, w.min_range, w.hits_land, w.hits_air).is_some() {
                        engaged = true;
                    }
                }
                if is_struct {
                    pop = true;
                } else if !engaged {
                    if pos.dist(p) <= arrive.max(1.5) {
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
                    if range > 0.0 && t.hp > 0.0 && t.known_to(team) && t.team != team {
                        pop = false;
                        focus = tid;
                        if plane || (!is_struct && pos.dist(t.pos) > range * 0.9) {
                            goal = Some(t.pos);
                        }
                    }
                }
            }
            Some(Order::AttackGround { pos: gp, radius }) => {
                let range = df.weapons.iter().filter(|w| w.hits_land).map(|w| w.range).fold(0.0, f32::max);
                if range <= 0.0 {
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
                    match (site, pending) {
                        (Some(s), _) => {
                            if s.done || s.def != bd || s.team != team {
                                pop = true;
                            } else if pos.dist(bp) > r {
                                goal = Some(bp);
                            } else {
                                work = s.id;
                            }
                        }
                        (None, Some(p)) => work = if p.0 == bd { p.2 } else { 0 },
                        (None, None) => {
                            if pos.dist(bp) > r {
                                goal = Some(bp);
                            } else if can_place(&defs, &map, &nav.occ, bd, bp) {
                                let sid = spawn_unit(&mut commands, &defs, &mut game, &map, bd, owner, bp, PI * 0.5, false, 0);
                                nav.stamp(&map, bp, defs.get(bd).footprint, 2);
                                // Trees and rocks on the plot are flattened.
                                features.clear_square(bp, defs.get(bd).footprint);
                                effects.sites.push((bd, bp, sid));
                                work = sid;
                            } else {
                                pop = true;
                            }
                        }
                    }
                }
            }
            Some(Order::Assist(tid)) => match census.get(tid) {
                Some(t) if t.hp > 0.0 && tid != id => {
                    let r = reach(t.radius);
                    if df.can_assist() || df.is_scavenger() {
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
                        if pos.dist(t.pos) > r {
                            if is_struct {
                                pop = true;
                            } else {
                                goal = Some(t.pos);
                            }
                        } else {
                            work = job;
                        }
                        let td = defs.get(t.def);
                        if job == 0 && !(td.is_factory() || td.can_assist()) {
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
                // Trees and rocks: engineers only (scavengers work on wrecks).
                Some(f) if df.can_assist() => {
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
            Some(Order::Capture(tid)) => match census.get(tid) {
                Some(t) if df.is_engineer() && t.hp > 0.0 && t.done && !t.hosted && !t.air && t.team != team && t.known_to(team) && !defs.factions.iter().any(|f| f.commander == Some(t.def)) => {
                    if pos.dist(t.pos) > reach(t.radius) {
                        goal = Some(t.pos);
                    } else {
                        effects.capture.push((tid, df.build_rate() * DT, owner));
                        work = tid;
                    }
                }
                _ => pop = true,
            },
            Some(Order::Revive(wid)) => match wrecks.0.iter().find(|w| w.0 == wid).copied() {
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
                _ => pop = true,
            },
            Some(Order::ReviveArea { pos: c, radius }) => {
                // The nearest wreck in the circle that can come back (a building needs its plot free).
                let best = wrecks
                    .0
                    .iter()
                    .filter(|w| w.1.dist(c) <= radius && w.2 > 0.0 && !effects.revived.iter().any(|r| r.0 == w.0))
                    .filter(|w| !defs.get(w.3).is_structure() || can_place(&defs, &map, &nav.occ, w.3, w.1))
                    .min_by(|a, b| a.1.dist(pos).partial_cmp(&b.1.dist(pos)).unwrap_or(std::cmp::Ordering::Equal))
                    .copied();
                match best {
                    Some(w) if df.is_scavenger() => {
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
                if !df.is_engineer() || terraform_left(&map, &terrain, &nav.occ, c, radius, height, limit) < 0.05 {
                    pop = true;
                } else if pos.dist(c) > df.build_range().max(2.0) + radius + df.radius() {
                    goal = Some(c);
                } else if !census.get(id).map_or(false, |c| c.paused) {
                    effects.terraform.push((c, radius, height, df.build_rate() * defs.rules.terraform_rate * DT, owner));
                }
            }
            Some(Order::Load(tid)) => {
                pop = true;
                let near = |t: &Cen| pos.dist(t.pos) <= 2.5 + t.radius + df.radius();
                if let Some(tr) = &df.transport {
                    // A transport fetching one unit.
                    if let Some(t) = census.get(tid).filter(|t| t.owner == owner && t.done && !t.hosted && t.hp > 0.0 && carriable(defs.get(t.def))) {
                        let free = room(&effects, id, tr.capacity) >= defs.get(t.def).cargo as i32 && !effects.loaded.iter().any(|l| l.0 == tid);
                        if free && !near(t) {
                            goal = Some(t.pos);
                            pop = false;
                        } else if free {
                            commands.entity(t.e).insert(Hosted(id));
                            effects.loaded.push((tid, id));
                        }
                    }
                } else if let Some(t) = census.get(tid).filter(|t| t.team == team && t.done && !t.hosted && t.hp > 0.0 && carriable(df)) {
                    // A unit walking up to a transport and boarding it.
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
            Some(Order::LoadArea { pos: c, radius }) => {
                pop = true;
                if let Some(tr) = &df.transport {
                    let free = room(&effects, id, tr.capacity);
                    let mut best: Option<(f32, u32)> = None;
                    census.near(c, radius.max(1.0), |t| {
                        if t.owner != owner || !t.done || t.hosted || t.hp <= 0.0 || t.id == id || !carriable(defs.get(t.def)) || defs.get(t.def).cargo as i32 > free {
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
                        if pos.dist(t.pos) <= 2.5 + t.radius + df.radius() {
                            commands.entity(t.e).insert(Hosted(id));
                            effects.loaded.push((t.id, id));
                        } else {
                            goal = Some(t.pos);
                        }
                    }
                }
            }
            Some(Order::Unload { pos: c, radius }) => {
                // A ground transport cannot stop on a cliff or a building: the nearest free spot will do.
                let mut c = c;
                if !df.is_air() && nav.at(&map, c) != 0 {
                    let (cx, cy) = map.cell_of(c);
                    if let Some((x, y)) = nearest_free(&nav.occ, map.n, cx, cy) {
                        c = v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL);
                    }
                }
                let cargo: Vec<&Cen> = census.list.iter().filter(|t| t.host == id).collect();
                if df.transport.is_none() || cargo.is_empty() {
                    pop = true;
                } else if pos.dist(c) > arrive.max(1.5) {
                    goal = Some(c);
                } else {
                    // Everybody out, spread over the circle (a spiral from the middle).
                    let n = cargo.len() as f32;
                    for (k, t) in cargo.iter().enumerate() {
                        let mut p = map.clamp(c + V2::from_angle(k as f32 * 2.4) * (radius.max(1.5) * ((k as f32 + 0.5) / n).sqrt()));
                        if nav.at(&map, p) != 0 {
                            let (cx, cy) = map.cell_of(p);
                            if let Some((x, y)) = nearest_free(&nav.occ, map.n, cx, cy) {
                                p = v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL);
                            }
                        }
                        commands.entity(t.e).remove::<Hosted>().insert((Body { pos: p, y: map.h(p), yaw: body.yaw, vel: v2(0.0, 0.0) }, Orders::default(), Mover { anchor: p, path_goal: p, ..default() }));
                    }
                    pop = true;
                }
            }
            Some(Order::ReclaimArea { pos: c, radius }) => {
                // The nearest wreck, tree or rock in the circle (scavengers: wrecks only).
                let wreck = wrecks
                    .0
                    .iter()
                    .filter(|w| w.1.dist(c) <= radius && w.2 > 0.0)
                    .min_by(|a, b| a.1.dist(pos).partial_cmp(&b.1.dist(pos)).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|w| (w.0, w.1, 0.5));
                let feature = if df.can_assist() {
                    features.nearest(c, radius, pos).map(|(fid, fp)| (fid, fp, features.get(fid).and_then(|f| defs.rules.features.get(f.def as usize)).map_or(0.5, |d| d.radius())))
                } else {
                    None
                };
                let best = match (wreck, feature) {
                    (Some(w), Some(f)) => Some(if f.1.dist(pos) < w.1.dist(pos) { f } else { w }),
                    (w, f) => w.or(f),
                };
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
        }
        if let Some(m) = mover.as_mut() {
            m.goal = goal;
        }
        if let Some(w) = weapons.as_mut() {
            w.focus = focus;
            w.ground = ground;
        }
        if pop && !orders.0.is_empty() {
            let o = orders.0.remove(0);
            if let Order::Patrol(_) = o {
                orders.0.push(o);
            }
            if let Some(m) = mover.as_mut() {
                m.stuck = 0;
            }
        }
    }
}

/// Turns a wreck into a construction site of `owner` that already holds the wreck's mass;
/// finishing it costs the rest, like any building work. Returns the new unit's id.
#[allow(clippy::too_many_arguments)]
fn revive(commands: &mut Commands, defs: &Defs, game: &mut Game, map: &MapData, nav: &mut Nav, effects: &mut Effects, w: (u32, V2, f32, DefId), owner: u8) -> u32 {
    let wd = defs.get(w.3);
    let built = (w.2 / wd.cost.mass.max(1.0)).clamp(0.05, 0.95);
    let sid = spawn_unit_built(commands, defs, game, map, w.3, owner, w.1, PI * 0.5, Some(built), 0);
    if wd.is_structure() {
        nav.stamp(map, w.1, wd.footprint, 2);
    }
    effects.reclaim.push((w.0, 1.0e9, u8::MAX));
    effects.revived.push((w.0, sid));
    sid
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
        if f.building != 0 {
            match census.get(f.building) {
                None => f.building = 0,
                Some(p) if p.done => {
                    let pd = defs.get(p.def);
                    let mut exit = body.pos + v2(0.0, df.footprint as f32 * CELL * 0.5 + 1.5);
                    if !pd.is_air() && nav.at(&map, exit) != 0 {
                        let (cx, cy) = map.cell_of(exit);
                        if let Some((x, y)) = nearest_free(&nav.occ, map.n, cx, cy) {
                            exit = v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL);
                        }
                    }
                    let rally = f.rally.unwrap_or(exit + v2(0.0, 6.0));
                    let pos = if pd.is_air() { body.pos } else { exit };
                    commands.entity(p.e).remove::<Hosted>().insert((
                        Body { pos, y: map.h(pos) + pd.mv.altitude, yaw: PI * 0.5, vel: v2(0.0, 0.0) },
                        Orders(vec![Order::Move(rally)]),
                        Mover { anchor: rally, path_goal: pos, ..default() },
                    ));
                    f.building = 0;
                    if !f.queue.is_empty() {
                        let d = f.queue.remove(0);
                        if f.repeat {
                            f.queue.push(d);
                        }
                    }
                }
                Some(_) => builder.work = f.building,
            }
        }
        if f.building == 0 && !upgrading {
            if let Some(d) = f.queue.first().copied() {
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

/// Flow economy: every builder requests resources for its job; if a player cannot pay,
/// all of that player's jobs slow down together.
#[allow(clippy::type_complexity)]
fn build_progress(
    mut q: Query<(Entity, &mut Unit, &mut Health, Option<&Builder>, Option<&mut UnderConstruction>, Option<&mut Upgrading>)>,
    census: Res<Census>,
    defs: Res<Defs>,
    effects: Res<Effects>,
    mut game: ResMut<Game>,
    mut commands: Commands,
) {
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
                if t.e != e {
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
                commands.entity(te).remove::<UnderConstruction>();
                // Marked complete right away so later contributions in this tick see it.
                uc.0 = 2.0;
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
        if ratio[p] < 1.0 || power[p] < 1.0 {
            // Show what was asked for, so the deficit is visible.
            pl.spend_m = dm[p];
            pl.spend_e = de[p] + pl.upkeep;
        }
        pl.power = power[p];
        // Radars drop out when less than half of the energy demand is met.
        pl.power_out = power[p] < 0.5;
        pl.mass = pl.mass.clamp(0.0, pl.max_mass.max(1.0));
        pl.energy = pl.energy.clamp(0.0, pl.max_energy.max(1.0));
    }
}

#[allow(clippy::type_complexity)]
fn locomotion(
    mut q: Query<(&Unit, &mut Body, &mut Mover, &mut Orders), (Without<UnderConstruction>, Without<Hosted>)>,
    defs: Res<Defs>,
    map: Res<MapData>,
    mut nav: ResMut<Nav>,
) {
    let world = map.world();
    let n = map.n as i32;
    for (u, mut body, mut m, mut orders) in q.iter_mut().sort_by_key::<&Unit, _>(|u| u.id) {
        let df = defs.get(u.def);
        let mv = &df.mv;
        let old = body.pos;
        let turn = mv.turn.to_radians();
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
                    if body.pos.dist(g) < turn_radius * 1.1 && off > 1.0 {
                        m.extend = 2.5;
                        desired = body.yaw;
                    }
                } else if body.pos.dist(m.anchor) > mv.speed / turn.max(0.1) * 1.5 {
                    desired = (m.anchor - body.pos).angle();
                } else {
                    desired = body.yaw + 1.0;
                }
                if body.pos.x < 3.0 || body.pos.y < 3.0 || body.pos.x > world - 3.0 || body.pos.y > world - 3.0 {
                    desired = (map.center() - body.pos).angle();
                    m.extend = 0.0;
                }
                body.yaw = turn_toward(body.yaw, desired, turn * DT);
                body.pos = body.pos + V2::from_angle(body.yaw) * (mv.speed * DT);
                let want = map.h(body.pos).max(map.water) + mv.altitude;
                body.y += (want - body.y).clamp(-5.0 * DT, 5.0 * DT);
            }
            MoveKind::Hover => {
                let mut want_speed = 0.0;
                if let Some(g) = m.goal {
                    let to = g - body.pos;
                    let dist = to.len();
                    if dist > 0.4 {
                        body.yaw = turn_toward(body.yaw, to.angle(), turn * DT);
                        want_speed = mv.speed.min((2.0 * mv.accel * dist).sqrt() + 0.5);
                        m.speed += (want_speed - m.speed).clamp(-mv.accel * 2.0 * DT, mv.accel * DT);
                        body.pos = body.pos + to.norm() * (m.speed * DT).min(dist);
                    }
                }
                if want_speed == 0.0 {
                    m.speed = 0.0;
                }
                let want = map.h(body.pos).max(map.water) + mv.altitude;
                body.y += (want - body.y).clamp(-4.0 * DT, 4.0 * DT);
            }
            MoveKind::Land => {
                let Some(g) = m.goal else {
                    m.path.clear();
                    m.stuck = 0;
                    m.speed = 0.0;
                    body.vel = v2(0.0, 0.0);
                    body.y = map.h(body.pos);
                    continue;
                };
                if m.path.is_empty() || m.path_goal.dist(g) > 3.0 {
                    if nav.paths_left <= 0 {
                        continue;
                    }
                    nav.paths_left -= 1;
                    let nav = &mut *nav;
                    m.path = find_path(&nav.occ, map.n, body.pos, g, &mut nav.scratch);
                    m.path_goal = g;
                }
                let occ = &nav.occ;
                let blocked = |p: V2| -> bool {
                    let (cx, cy) = ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32);
                    cx < 0 || cy < 0 || cx >= n || cy >= n || occ[(cy * n + cx) as usize] != 0
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
                let top = if m.reversing { mv.reverse } else { mv.speed };
                let mut want = if off < 0.35 {
                    top
                } else if off < 1.0 {
                    top * 0.4
                } else {
                    0.0
                };
                let brake = mv.accel * 2.5;
                want = want.min((2.0 * brake * remaining).sqrt() + 0.4);
                m.speed += (want - m.speed).clamp(-brake * DT, mv.accel * DT);
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
                body.y = map.h(body.pos);
            }
        }
        body.pos = v2(body.pos.x.clamp(0.5, world - 0.5), body.pos.y.clamp(0.5, world - 0.5));
        body.vel = (body.pos - old) * (1.0 / DT);
    }
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
        items.push(It { e, pos: b.pos, r: d.radius(), air: d.is_air(), moving: m.goal.is_some() });
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
            if a.air || nav.at(&map, np) == 0 {
                if let Ok((_, _, mut body, _)) = q.get_mut(a.e) {
                    body.pos = np;
                    if !a.air {
                        body.y = map.h(np);
                    }
                }
            }
        }
    }
}

/// Units riding in a transport (or waiting inside a factory) travel with it.
fn carry(census: Res<Census>, mut riders: Query<(&Hosted, &mut Body)>, hosts: Query<&Body, Without<Hosted>>) {
    for (h, mut b) in &mut riders {
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
            WeaponKind::Missile => (0.35, d / w.speed.max(1.0), w.speed),
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

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn weapons(
    mut q: Query<(&Unit, &Body, &mut Weapons, Option<&mut Mover>), (Without<UnderConstruction>, Without<Hosted>)>,
    census: Res<Census>,
    defs: Res<Defs>,
    map: Res<MapData>,
    mut game: ResMut<Game>,
    mut rng: ResMut<SimRng>,
    mut fx: ResMut<FxLog>,
    mut commands: Commands,
) {
    let g = defs.rules.gravity;
    let tick = game.tick;
    for (u, body, mut ws, mut mover) in q.iter_mut().sort_by_key::<&Unit, _>(|u| u.id) {
        let df = defs.get(u.def);
        let team = game.team_of(u.owner);
        let origin = body.pos.at(body.y + df.height() * 0.8);
        let plane = df.is_plane();
        let (focus, ground) = (ws.focus, ws.ground);
        for (wi, w) in df.weapons.iter().enumerate() {
            let Some(st) = ws.list.get_mut(wi) else { continue };
            st.cd = (st.cd - DT).max(0.0);
            let fixed = w.turret.is_none();

            // --- target selection
            let in_envelope = |c: &Cen| -> bool {
                let d = c.pos.dist(body.pos);
                d <= w.range && d >= w.min_range && c.hp > 0.0 && !c.hosted && if c.air { w.hits_air } else { w.hits_land }
            };
            let mut target: Option<&Cen> = None;
            let mut gpoint: Option<Vec3> = None;
            if let Some((gp, radius)) = ground.filter(|_| w.hits_land) {
                let d = gp.dist(body.pos);
                if d <= w.range + radius && d >= w.min_range {
                    // A stable pseudo-random spot inside the area, re-rolled every shot.
                    let salt = (u.id as u64 * 7919 + (tick as u64 / ((w.reload * 10.0) as u64).max(1)) * 31 + wi as u64) | 1;
                    let mut r = Rng(salt.wrapping_mul(0x9E3779B97F4A7C15) | 1);
                    r.next();
                    let off = V2::from_angle(r.range(0.0, 6.283)) * (r.f().sqrt() * radius);
                    let p = map.clamp(gp + off);
                    gpoint = Some(p.at(map.h(p).max(map.water)));
                }
            }
            if gpoint.is_none() {
                if let Some(c) = census.get(focus).filter(|c| in_envelope(c) && c.team != team && c.known_to(team)) {
                    target = Some(c);
                } else if let Some(c) = census.get(st.target).filter(|c| in_envelope(c) && census.valid_target(c, team, w.hits_land, w.hits_air)) {
                    if (tick + u.id) % 20 != 0 {
                        target = Some(c);
                    }
                }
                if target.is_none() && (st.target != 0 || (tick + u.id) % 3 == 0) {
                    target = census.nearest_enemy(body.pos, team, w.range, w.min_range, w.hits_land, w.hits_air);
                }
            }
            st.target = target.map_or(0, |c| c.id);

            // --- bursts keep firing along the barrel
            if st.burst > 0 {
                st.burst_cd -= DT;
                if st.burst_cd <= 0.0 {
                    st.burst -= 1;
                    st.burst_cd += w.salvo_delay;
                    fire(&mut commands, &mut game, &mut rng.0, &mut fx, w, u, body, origin, st);
                }
            }

            let aim_at = gpoint.map(|p| (p, v2(0.0, 0.0))).or(target.map(|c| (c.center(), c.vel)));
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

            // --- bombs: release when the fall will carry them onto the target
            if w.kind == WeaponKind::Bomb {
                st.yaw = body.yaw;
                let fall = (2.0 * (body.y - tp.y).max(0.5) / g).sqrt();
                let speed = body.vel.len();
                let aim = flat(tp) + tv * fall;
                let to = aim - body.pos;
                let off = wrap(to.angle() - body.yaw).abs();
                if st.cd <= 0.0 && st.burst == 0 && off < 0.25 && (to.len() - speed * fall).abs() < speed * DT + 1.0 + w.salvo as f32 * 0.5 {
                    st.cd = w.reload;
                    st.burst = w.salvo.saturating_sub(1);
                    st.burst_cd = w.salvo_delay;
                    fire(&mut commands, &mut game, &mut rng.0, &mut fx, w, u, body, origin, st);
                    if let Some(m) = mover.as_mut() {
                        m.extend = 3.5;
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
            if aligned {
                st.aim += DT;
            } else {
                st.aim = 0.0;
            }
            if st.cd > 0.0 || st.burst > 0 || !aligned || st.aim < w.aim_time {
                continue;
            }
            // Direct fire needs a clear line; otherwise hold fire rather than shoot the hill.
            if w.kind == WeaponKind::Direct && !map.los(origin, sol.point + Vec3::Y * 0.3) {
                continue;
            }
            let _ = fixed;
            st.cd = w.reload;
            st.burst = w.salvo.saturating_sub(1);
            st.burst_cd = w.salvo_delay;
            fire(&mut commands, &mut game, &mut rng.0, &mut fx, w, u, body, origin, st);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn fire(commands: &mut Commands, game: &mut Game, rng: &mut Rng, fx: &mut FxLog, w: &WeaponDef, u: &Unit, body: &Body, origin: Vec3, st: &WeaponState) {
    let spread = w.spread.to_radians();
    let (vel, kind, pos) = match w.kind {
        WeaponKind::Bomb => (body.vel.at(0.0), PROJ_BOMB, body.pos.at(body.y - 0.3)),
        WeaponKind::Missile => {
            let d = dir_from(st.yaw, st.pitch.max(0.3));
            (d * w.speed, PROJ_MISSILE, origin + d * 0.5)
        }
        _ => {
            let speed = if st.speed > 0.0 { st.speed } else { w.speed };
            let d = dir_from(st.yaw + rng.bell() * spread, st.pitch + rng.bell() * spread * 0.6);
            (d * speed, PROJ_SHELL, origin + d * 0.4)
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
    };
    spawn_projectile(commands, shot);
}

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
    mut commands: Commands,
) {
    let mut wreck_hits: Vec<(u32, f32)> = vec![];
    let g = defs.rules.gravity;
    let ff = defs.rules.friendly_fire;
    for (e, mut p) in q.iter_mut().sort_by_key::<&Projectile, _>(|p| p.id) {
        let team = game.team_of(p.owner);
        p.life -= DT;
        let a = p.pos.v();
        let mut vel = p.vel.v();
        let mut hit: Option<(Vec3, u32)> = None;
        if p.kind == PROJ_MISSILE {
            let speed = vel.length().max(1.0);
            if let Some(t) = census.get(p.target).filter(|t| t.hp > 0.0) {
                let want = (t.center() - a).normalize_or_zero();
                let cur = vel / speed;
                let ang = fm::acos(cur.dot(want).clamp(-1.0, 1.0));
                let max = p.turn.to_radians() * DT;
                let nd = if ang <= max || ang < 1e-4 { want } else { cur.lerp(want, max / ang).normalize_or_zero() };
                vel = nd * speed;
                if (t.center() - a).length() < speed * DT + t.radius * 0.7 {
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
            let len2 = seg2.length_squared().max(1e-6);
            let mut best: Option<(f32, u32)> = None;
            let mid = flat((a + b) * 0.5);
            census.near(mid, seg2.length() * 0.5 + 8.0, |c| {
                if c.hp <= 0.0 || c.hosted || (c.team == team && !ff) {
                    return;
                }
                if if c.air { !p.hits_air } else { !p.hits_land } {
                    return;
                }
                let rel = Vec2::new(c.pos.x - a.x, c.pos.y - a.z);
                let t = (rel.dot(seg2) / len2).clamp(0.0, 1.0);
                let at = a + seg * t;
                let horiz = Vec2::new(at.x - c.pos.x, at.z - c.pos.y).length();
                let r = if c.structure { c.radius * 1.1 } else { c.radius } + 0.12;
                if horiz < r && at.y > c.y - 0.3 && at.y < c.y + c.height + 0.3 && best.map_or(true, |x| t < x.0) {
                    best = Some((t, c.id));
                }
            });
            // Terrain along the segment.
            let steps = (seg.length() / 0.8).ceil().max(1.0) as i32;
            let mut ground_t: Option<f32> = None;
            for i in 1..=steps {
                let t = i as f32 / steps as f32;
                let s = a + seg * t;
                if s.y <= map.height(s.x, s.z).max(map.water - 0.5) {
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
        let w = map.world();
        let Some((at, victim)) = hit else {
            if p.life <= 0.0 || b.x < -20.0 || b.z < -20.0 || b.x > w + 20.0 || b.z > w + 20.0 {
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
        if rules.crater_scale > 0.0 && p.damage >= rules.crater_min_damage && (victim == 0 || p.radius > 0.0) && at.y - map.height(at.x, at.z) < 1.5 && at.y > map.water - 0.5 {
            let depth = rules.crater_scale * p.damage.sqrt() * if p.radius > 0.0 { 1.0 } else { 0.5 };
            effects.craters.push((flat(at), p.radius * 0.5, depth));
        }
        if p.radius > 0.0 {
            census.near(flat(at), p.radius + 6.0, |c| {
                if c.hp <= 0.0 || c.hosted || (c.team == team && !ff) || (c.air && !p.hits_air) {
                    return;
                }
                let horiz = (c.pos.dist(flat(at)) - c.radius * 0.7).max(0.0);
                let vert = if at.y < c.y { c.y - at.y } else { (at.y - c.y - c.height).max(0.0) };
                let d = (horiz * horiz + vert * vert).sqrt();
                if d < p.radius {
                    effects.damage.push((c.id, p.damage * (1.0 - 0.6 * d / p.radius), p.dtype as usize));
                }
            });
        } else if victim != 0 {
            effects.damage.push((victim, p.damage, p.dtype as usize));
        }
        // Trees, rocks and wrecks never stop a shot, but what lands on them or blasts next to them wears them down.
        if p.radius > 0.0 || victim == 0 {
            hurt_reclaimables(at, p.radius, p.damage, p.dtype as usize, &map, &defs, &mut features, &wreck_list, &mut wreck_hits);
        }
    }
    damage_wrecks(&wreck_hits, &mut wq, &defs, &mut fx, &mut commands);
}

/// Weapon damage to the reclaimables around an impact at `at`: a shell (`radius` 0) hurts what it
/// lands on, a blast everything within its radius, with the same fall-off as for units. A tree or
/// rock loses the share of its worth that the damage is of its hit points (changed at once); hits
/// on wrecks are added to `wreck_hits` as (wreck id, damage after armor) for `damage_wrecks`.
#[allow(clippy::too_many_arguments)]
pub fn hurt_reclaimables(at: Vec3, radius: f32, damage: f32, dtype: usize, map: &MapData, defs: &Defs, features: &mut Features, wrecks: &WreckList, wreck_hits: &mut Vec<(u32, f32)>) {
    let reach = radius.max(0.3);
    let centre = flat(at);
    let hurt = |horiz: f32, ground: f32, top: f32| -> Option<f32> {
        let vert = (at.y - top).max(0.0) + (ground - 0.5 - at.y).max(0.0);
        let d = (horiz * horiz + vert * vert).sqrt();
        (d < reach).then(|| if radius > 0.0 { damage * (1.0 - 0.6 * d / radius) } else { damage })
    };
    let mut hit: Vec<(usize, f32)> = vec![];
    features.near(centre, reach + 6.0, |i, s| {
        let ground = map.h(s.pos);
        if let Some(dmg) = hurt((s.pos.dist(centre) - s.radius).max(0.0), ground, ground + s.height) {
            hit.push((i, s.left - dmg / s.hp));
        }
    });
    for (i, left) in hit {
        features.set_left(i, left);
    }
    for &(id, pos, mass, def) in &wrecks.0 {
        let d = defs.get(def);
        let horiz = (pos.dist(centre) - d.radius() * 0.7).max(0.0);
        if mass <= 0.0 || horiz >= reach {
            continue;
        }
        let ground = map.h(pos);
        // (As high as it blocks sight, see `sight_block`.)
        let top = ground + d.height() * if d.is_structure() { 0.35 } else { 0.8 };
        if let Some(dmg) = hurt(horiz, ground, top) {
            wreck_hits.push((id, dmg * defs.armor_mult(dtype, d.armor_id)));
        }
    }
}

/// Applies weapon damage to wrecks: every hit point a whole wreck of that unit has
/// (`rules.wreck_hp` of the unit's) is worth the same share of its mass. Returns the wrecks
/// destroyed (already despawned).
fn damage_wrecks(hits: &[(u32, f32)], q: &mut Query<(Entity, &mut Wreck)>, defs: &Defs, fx: &mut FxLog, commands: &mut Commands) -> Vec<Entity> {
    let mut gone = vec![];
    if hits.is_empty() {
        return gone;
    }
    for (e, mut w) in q.iter_mut().sort_by_key::<&Wreck, _>(|w| w.id) {
        if w.mass <= 0.0 {
            continue;
        }
        let dmg: f32 = hits.iter().filter(|h| h.0 == w.id).map(|h| h.1).sum();
        if dmg <= 0.0 {
            continue;
        }
        let d = defs.get(w.def);
        let per_hp = d.cost.mass * defs.rules.wreck_mass_fraction / (d.hp * defs.rules.wreck_hp).max(1.0);
        w.mass -= dmg * per_hp;
        if w.mass <= 0.01 {
            w.mass = 0.0;
            fx.push(w.pos.at(w.y + d.height() * 0.3), FX_BLAST, d.radius() * 0.8 + 0.3);
            commands.entity(e).despawn();
            gone.push(e);
        }
    }
    gone
}

#[allow(clippy::too_many_arguments)]
fn apply_reclaim(
    mut q: Query<(Entity, &mut Wreck)>,
    mut units: Query<(&Unit, &mut Health)>,
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
        let Some((u, mut h)) = census.get(id).and_then(|c| units.get_mut(c.e).ok()) else { continue };
        if h.0 <= 0.0 {
            continue;
        }
        let d = defs.get(u.def);
        let per_hp = (d.cost.mass * defs.rules.wreck_mass_fraction / d.hp.max(1.0)).max(1e-4);
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

/// Capture work queued by engineers this tick. A unit nobody works on slowly shakes it off.
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
    for (e, mut u, mut orders, cap, factory, weapons, mover) in q.iter_mut().sort_by_key::<&Unit, _>(|u| u.id) {
        let Some(by) = jobs.iter().find(|j| j.0 == u.id).map(|j| j.2) else {
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
        if pl.energy < energy {
            continue;
        }
        pl.energy -= energy;
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
            if let Some(p) = census.get(f.building) {
                commands.entity(p.e).despawn();
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
    mut q: Query<(Entity, &Unit, &Body, &mut Health, Option<&UnderConstruction>, Option<&Hosted>)>,
    mut wrecks: Query<(Entity, &mut Wreck)>,
    (mut features, wreck_list): (ResMut<Features>, Res<WreckList>),
    census: Res<Census>,
    defs: Res<Defs>,
    map: Res<MapData>,
    mut game: ResMut<Game>,
    mut effects: ResMut<Effects>,
    mut nav: ResMut<Nav>,
    mut fx: ResMut<FxLog>,
    mut commands: Commands,
) {
    for (id, dmg, dtype) in effects.damage.drain(..) {
        if let Some(c) = census.get(id) {
            if let Ok((_, u, _, mut h, _, _)) = q.get_mut(c.e) {
                h.0 -= dmg * defs.armor_mult(dtype, defs.get(u.def).armor_id);
            }
        }
    }
    let tick = game.tick;
    let mut blasts: Vec<(V2, f32, f32, f32, u8)> = vec![];
    let mut new_wrecks = 0;
    let reclaimed = std::mem::take(&mut effects.reclaimed);
    for (e, u, b, mut h, uc, hosted) in q.iter_mut().sort_by_key::<&Unit, _>(|u| u.id) {
        let d = defs.get(u.def);
        let player_dead = game.players.get(u.owner as usize).map_or(false, |p| !p.alive);
        if player_dead && (u.id + tick) % 6 == 0 {
            h.0 = -1.0;
        }
        if let Some(host) = hosted {
            if census.get(host.0).is_none() {
                commands.entity(e).despawn();
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
        // Taken apart by an engineer: nothing is left, and nothing blows up.
        if reclaimed.contains(&u.id) {
            continue;
        }
        fx.push(b.pos.at(b.y + d.height() * 0.5), FX_DEATH, d.radius() * 1.6 + 0.6);
        // A wreck holds a share of the mass that actually went into the unit.
        let invested = d.cost.mass * uc.map_or(1.0, |x| x.0.clamp(0.0, 1.0));
        if uc.is_some() {
            if d.wreck && !d.is_air() && invested * defs.rules.wreck_mass_fraction >= 5.0 {
                let id = game.alloc();
                spawn_wreck(&mut commands, Wreck { id, def: u.def, mass: invested * defs.rules.wreck_mass_fraction, pos: b.pos, y: b.y, yaw: b.yaw });
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
        } else if d.wreck && !d.is_air() && d.cost.mass * defs.rules.wreck_mass_fraction >= 5.0 {
            let id = game.alloc();
            spawn_wreck(&mut commands, Wreck { id, def: u.def, mass: d.cost.mass * defs.rules.wreck_mass_fraction, pos: b.pos, y: b.y, yaw: b.yaw });
            new_wrecks += 1;
        }
    }
    let mut wreck_hits: Vec<(u32, f32)> = vec![];
    let blast_type = 3.min(defs.rules.damage_types.len() - 1);
    for (pos, y, damage, radius, owner) in blasts {
        hurt_reclaimables(pos.at(y + 0.5), radius, damage, blast_type, &map, &defs, &mut features, &wreck_list, &mut wreck_hits);
        let team = game.team_of(owner);
        if defs.rules.crater_scale > 0.0 && y - map.h(pos) < 3.0 {
            effects.craters.push((pos, radius * 0.4, defs.rules.crater_scale * damage.sqrt()));
        }
        census.near(pos, radius + 4.0, |c| {
            let flat = (c.pos.dist(pos) - c.radius * 0.7).max(0.0);
            let d = (flat * flat + (c.y - y) * (c.y - y)).sqrt();
            if d < radius && c.hp > 0.0 && (c.team != team || defs.rules.friendly_fire || owner >= NEUTRAL) {
                effects.damage.push((c.id, damage * (1.0 - 0.6 * d / radius), blast_type));
            }
        });
    }
    let gone = damage_wrecks(&wreck_hits, &mut wrecks, &defs, &mut fx, &mut commands);
    // Keep the wreck count bounded: drop the oldest.
    let total = wrecks.iter().count() - gone.len() + new_wrecks;
    if total > defs.rules.max_wrecks {
        let mut all: Vec<(u32, Entity)> = wrecks.iter().filter(|(e, _)| !gone.contains(e)).map(|(e, w)| (w.id, e)).collect();
        all.sort();
        for (_, e) in all.into_iter().take(total - defs.rules.max_wrecks) {
            commands.entity(e).despawn();
        }
    }
}

/// Applies this tick's craters and terraforming to the terrain.
fn apply_ground(mut effects: ResMut<Effects>, mut map: ResMut<MapData>, mut ground: ResMut<Ground>, mut nav: ResMut<Nav>, mut game: ResMut<Game>, defs: Res<Defs>) {
    if effects.craters.is_empty() && effects.terraform.is_empty() {
        return;
    }
    let rules = &defs.rules;
    let (map, ground) = (&mut *map, &mut *ground);
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
    for p in game.points.iter_mut() {
        // Only finished ground units of living players hold a point (no aircraft, no buildings).
        let mut teams = 0u16;
        census.near(p.pos, rules.cp_radius, |c| {
            if c.done && !c.hosted && !c.air && !c.structure && c.hp > 0.0 && c.team < 14 {
                teams |= 1 << c.team;
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
