//! Simulation systems, run in order once per tick (host only).
use super::*;
use crate::data::{Defs, MoveKind, WeaponDef, WeaponKind};
use std::f32::consts::PI;

pub fn register(app: &mut App) {
    app.add_systems(
        SimTick,
        (
            // Inner tuples need their own chain(): the outer one only orders the groups.
            (begin_tick, census_build, crate::ai::ai_system, apply_commands, visibility, income).chain(),
            (think, factories, build_progress, locomotion, separation).chain(),
            (weapons, projectiles, apply_reclaim, deaths, victory).chain(),
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
    q: Query<(Entity, &Unit, &Body, &Health, &Intel, Has<UnderConstruction>, Has<Hosted>, Option<&Builder>, Option<&Factory>, Has<Upgrading>, Has<Paused>)>,
    wq: Query<&Wreck>,
) {
    census.clear();
    for (e, u, b, h, i, uc, hosted, builder, factory, upgrading, paused) in &q {
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
            hosted,
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
    wrecks.0.extend(wq.iter().map(|w| (w.id, w.pos, w.mass)));
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn apply_commands(
    mut queue: ResMut<CommandQueue>,
    census: Res<Census>,
    defs: Res<Defs>,
    map: Res<MapData>,
    game: Res<Game>,
    mut effects: ResMut<Effects>,
    mut commands: Commands,
    mut q: Query<(&Unit, &Body, &mut Orders, Option<&mut Factory>, Option<&mut Mover>, Option<&mut Weapons>, Has<Upgrading>, Has<UnderConstruction>)>,
) {
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
                    let Ok((u, b, mut ord, factory, mover, _, _, _)) = q.get_mut(e) else { continue };
                    let df = defs.get(u.def);
                    let mut o = if orders.len() == ids.len() { orders[k] } else { orders[0] };
                    match &mut o {
                        Order::Move(p) | Order::AttackMove(p) | Order::Patrol(p) => *p = map.clamp(*p),
                        Order::AttackGround { pos, .. } | Order::ReclaimArea { pos, .. } => *pos = map.clamp(*pos),
                        Order::Build { def: bd, pos } => {
                            if !df.builds.contains(bd) || !df.is_engineer() {
                                continue;
                            }
                            *pos = snap(&defs, &map, *bd, *pos);
                        }
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

/// Who sees whom. Radar needs a clear line over the terrain.
fn visibility(mut census: ResMut<Census>, game: Res<Game>, defs: Res<Defs>, map: Res<MapData>, mut q: Query<&mut Intel>) {
    struct Viewer {
        pos: V2,
        eye: Vec3,
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
            vis2: d.vision * d.vision,
            radar2: radar * radar,
            team: c.team,
        });
    }
    for c in census.list.iter_mut() {
        let own = 1u16 << c.team;
        let (mut seen, mut radar) = (own, own);
        let head = c.pos.at(c.y + c.height + 0.5);
        for v in &viewers {
            let bit = 1u16 << v.team;
            if seen & bit != 0 && radar & bit != 0 {
                continue;
            }
            let d2 = (v.pos - c.pos).len2();
            if d2 < v.vis2 {
                seen |= bit;
            }
            if radar & bit == 0 && d2 < v.radar2 && map.los(v.eye, head) {
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
    mut commands: Commands,
) {
    for (u, body, mut orders, mut mover, mut builder, mut weapons) in &mut q {
        let df = defs.get(u.def);
        if df.is_factory() {
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
                    if df.can_assist() {
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
            Some(Order::Reclaim(wid)) => match wrecks.0.iter().find(|w| w.0 == wid) {
                Some(w) if df.can_assist() => {
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
                _ => pop = true,
            },
            Some(Order::ReclaimArea { pos: c, radius }) => {
                let best = wrecks
                    .0
                    .iter()
                    .filter(|w| w.1.dist(c) <= radius && w.2 > 0.0)
                    .min_by(|a, b| a.1.dist(pos).partial_cmp(&b.1.dist(pos)).unwrap_or(std::cmp::Ordering::Equal));
                match best {
                    Some(w) if df.can_assist() => {
                        if pos.dist(w.1) > reach(0.5) {
                            if is_struct {
                                pop = true;
                            } else {
                                goal = Some(w.1);
                            }
                        } else {
                            effects.reclaim.push((w.0, df.build_rate() * defs.rules.reclaim_rate * DT, owner));
                            work = w.0;
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
    for (u, body, mut f, mut builder, upgrading) in &mut q {
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
    mut game: ResMut<Game>,
    mut commands: Commands,
) {
    // (payer, target entity, build rate)
    let mut jobs: Vec<(u8, Entity, f32)> = vec![];
    for (e, u, _, builder, uc, up) in q.iter() {
        if uc.is_some() || census.get(u.id).map_or(false, |c| c.paused) {
            continue;
        }
        let d = defs.get(u.def);
        if up.is_some() {
            jobs.push((u.owner, e, d.build_rate().max(5.0)));
        }
        if let Some(b) = builder {
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
    for (u, mut body, mut m, mut orders) in &mut q {
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
    for (e, u, b, m) in q.iter() {
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

/// Launch solution for a ballistic shot: (pitch, flight time, muzzle speed).
/// `boost` lets indirect weapons raise their charge to reach the target.
pub fn ballistic(d: f32, h: f32, v: f32, g: f32, high: bool, boost: bool) -> Option<(f32, f32, f32)> {
    let d = d.max(0.3);
    let mut v = v.max(1.0);
    let mut disc = v.powi(4) - g * (g * d * d + 2.0 * h * v * v);
    if disc < 0.0 {
        if !boost {
            return None;
        }
        v = (g * (h + (h * h + d * d).sqrt())).max(1.0).sqrt() * 1.01;
        disc = (v.powi(4) - g * (g * d * d + 2.0 * h * v * v)).max(0.0);
    }
    let root = disc.sqrt();
    let tan = (v * v + if high { root } else { -root }) / (g * d);
    let pitch = tan.atan();
    Some((pitch, d / (v * pitch.cos()).max(0.01), v))
}

fn dir_from(yaw: f32, pitch: f32) -> Vec3 {
    Vec3::new(yaw.cos() * pitch.cos(), pitch.sin(), yaw.sin() * pitch.cos())
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
        let yaw = to.z.atan2(to.x);
        let (pitch, t, speed) = match w.kind {
            WeaponKind::Direct => ballistic(d, to.y, w.speed, g, false, false).unwrap_or((to.y.atan2(d.max(0.1)), d / w.speed.max(1.0), w.speed)),
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
    for (u, body, mut ws, mut mover) in &mut q {
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
    fx.push(pos, FX_MUZZLE, (w.damage.sqrt() * 0.05).clamp(0.12, 1.2));
    let id = game.alloc();
    commands.spawn(Projectile {
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
    });
}

/// Moves projectiles under gravity and resolves hits against terrain and unit volumes.
fn projectiles(
    mut q: Query<(Entity, &mut Projectile)>,
    census: Res<Census>,
    defs: Res<Defs>,
    map: Res<MapData>,
    game: Res<Game>,
    mut effects: ResMut<Effects>,
    mut fx: ResMut<FxLog>,
    mut commands: Commands,
) {
    let g = defs.rules.gravity;
    let ff = defs.rules.friendly_fire;
    for (e, mut p) in &mut q {
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
                let ang = cur.dot(want).clamp(-1.0, 1.0).acos();
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
    }
}

fn apply_reclaim(mut q: Query<(Entity, &mut Wreck)>, mut effects: ResMut<Effects>, mut game: ResMut<Game>, mut commands: Commands) {
    if effects.reclaim.is_empty() {
        return;
    }
    let jobs: Vec<(u32, f32, u8)> = effects.reclaim.drain(..).collect();
    for (e, mut w) in &mut q {
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

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn deaths(
    mut q: Query<(Entity, &Unit, &Body, &mut Health, Has<UnderConstruction>, Option<&Hosted>)>,
    wrecks: Query<(Entity, &Wreck)>,
    census: Res<Census>,
    defs: Res<Defs>,
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
    for (e, u, b, mut h, uc, hosted) in &mut q {
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
        fx.push(b.pos.at(b.y + d.height() * 0.5), FX_DEATH, d.radius() * 1.6 + 0.6);
        if uc {
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
            commands.spawn(Wreck { id, def: u.def, mass: d.cost.mass * defs.rules.wreck_mass_fraction, pos: b.pos, y: b.y, yaw: b.yaw });
            new_wrecks += 1;
        }
    }
    for (pos, y, damage, radius, owner) in blasts {
        let team = game.team_of(owner);
        census.near(pos, radius + 4.0, |c| {
            let d = ((c.pos.dist(pos) - c.radius * 0.7).max(0.0).powi(2) + (c.y - y).powi(2)).sqrt();
            if d < radius && c.hp > 0.0 && (c.team != team || defs.rules.friendly_fire || owner >= NEUTRAL) {
                effects.damage.push((c.id, damage * (1.0 - 0.6 * d / radius), 3.min(defs.rules.damage_types.len() - 1)));
            }
        });
    }
    // Keep the wreck count bounded: drop the oldest.
    let total = wrecks.iter().count() + new_wrecks;
    if total > defs.rules.max_wrecks {
        let mut all: Vec<(u32, Entity)> = wrecks.iter().map(|(e, w)| (w.id, e)).collect();
        all.sort();
        for (_, e) in all.into_iter().take(total - defs.rules.max_wrecks) {
            commands.entity(e).despawn();
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
