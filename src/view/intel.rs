//! What the local player knows: fog of war grid, radar coverage (shadowed by terrain)
//! and the memory of contacts that are no longer detected.
use super::terrain::FogTexture;
use super::ViewState;
use crate::data::{DefId, Defs};
use crate::map::*;
use crate::sim::*;
use bevy::prelude::*;
use std::collections::HashMap;

pub const FOG_DARK: u8 = 140;
pub const FOG_RADAR: u8 = 80;

/// CPU copy of the fog texture: alpha per map cell.
#[derive(Resource)]
pub struct FogGrid {
    /// Cells across; `alpha` is one byte per map cell, row by row.
    pub n: usize,
    pub ny: usize,
    pub alpha: Vec<u8>,
    /// Cached radar coverage per radar unit: (position it was computed for, radius, covered cells).
    radar: HashMap<u32, (V2, f32, Vec<u32>)>,
    /// The same for what each unit sees directly.
    sight: HashMap<u32, (V2, f32, Vec<u32>)>,
    /// What binoculars see: (position, direction, range, covered cells).
    cone: HashMap<u32, (V2, f32, f32, Vec<u32>)>,
}

impl FogGrid {
    pub fn new((n, ny): crate::map::Dims) -> FogGrid {
        FogGrid { n, ny, alpha: vec![0; n * ny], radar: HashMap::new(), sight: HashMap::new(), cone: HashMap::new() }
    }
    /// Is this spot currently in direct sight?
    pub fn visible(&self, p: V2) -> bool {
        let (x, y) = ((p.x / CELL) as i32, (p.y / CELL) as i32);
        if x < 0 || y < 0 || x >= self.n as i32 || y >= self.ny as i32 {
            return false;
        }
        self.alpha[y as usize * self.n + x as usize] < 40
    }
}

/// A remembered enemy contact.
#[derive(Clone, Debug)]
pub struct Ghost {
    pub id: u32,
    /// Known once the unit has been seen (structures keep their identity).
    pub def: Option<DefId>,
    pub owner: u8,
    pub pos: V2,
    pub y: f32,
    pub yaw: f32,
    /// Construction progress when last seen (`None`: finished).
    pub built: Option<f32>,
    pub structure: bool,
    pub last_tick: u32,
    /// Detected right now (by sight or radar).
    pub live: bool,
    /// In sight right now (its own model shows; otherwise a remembered building shows a ghost model).
    pub seen: bool,
    /// The unit no longer exists (destroyed out of sight; `rules.ghosts_show_destroyed` shows it dimmed).
    pub gone: bool,
}

#[derive(Resource, Default)]
pub struct IntelMemory {
    pub ghosts: HashMap<u32, Ghost>,
    tick: u32,
    /// `Game.reveal_props` has been applied.
    props_known: bool,
}

pub fn reveal(game: &Game, me: u8) -> bool {
    // LR_REVEAL=1 switches the fog of war off (debugging).
    static FORCE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *FORCE.get_or_init(|| std::env::var("LR_REVEAL").is_ok()) || game.winner.is_some() || game.players.get(me as usize).map_or(true, |p| !p.alive)
}

/// Map cells within `radius` of `pos` that `eye` has a clear line to (everything within `near`
/// counts regardless, like in the simulation's own visibility test). `block`: trees, rocks, wrecks
/// and buildings that block the line too (sight; radar passes them, empty); `from` the building the
/// viewer looks out of (it does not block). `terrain`: hills block it.
#[allow(clippy::too_many_arguments)]
fn covered_cells(map: &MapData, eye: Vec3, pos: V2, radius: f32, near: f32, block: &SightBlock, terrain: bool, from: u32) -> Vec<u32> {
    let (n, ny) = (map.nx as i32, map.ny as i32);
    let (cx, cy, rc) = ((pos.x / CELL) as i32, (pos.y / CELL) as i32, (radius / CELL) as i32);
    let mut out = vec![];
    // Sample every second cell and fill 2x2 blocks: plenty for a fog overlay.
    let mut y = (cy - rc).max(0) & !1;
    while y <= (cy + rc).min(ny - 1) {
        let mut x = (cx - rc).max(0) & !1;
        while x <= (cx + rc).min(n - 1) {
            let c = v2((x as f32 + 1.0) * CELL, (y as f32 + 1.0) * CELL);
            let dist = c.dist(pos);
            // The ground under a building counts as seen when the building is: it does not hide itself.
            let at = block.building((y * n + x) as usize);
            if dist <= radius && (dist < near || map.sight_line(eye, c.at(map.h(c) + 1.5), block, terrain, [from, at])) {
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    if x + dx < n && y + dy < ny {
                        out.push(((y + dy) * n + x + dx) as u32);
                    }
                }
            }
            x += 2;
        }
        y += 2;
    }
    out
}

/// The cells inside a binocular cone (direction `yaw`, full width `angle` radians) that `eye` can see.
#[allow(clippy::too_many_arguments)]
fn covered_cone(map: &MapData, eye: Vec3, pos: V2, yaw: f32, angle: f32, radius: f32, block: &SightBlock, terrain: bool, from: u32) -> Vec<u32> {
    let (n, ny) = (map.nx as i32, map.ny as i32);
    let dir = V2::from_angle(yaw);
    let cos_half = (angle * 0.5).cos();
    let mut out = vec![];
    // Every second cell of the bounding square, 2x2 blocks filled (as `covered_cells`); each sample
    // gets its own sight line, since a hill can hide a dip and still show the slope behind it.
    let rc = (radius / CELL) as i32;
    let (cx, cy) = ((pos.x / CELL) as i32, (pos.y / CELL) as i32);
    let mut y = (cy - rc).max(0) & !1;
    while y <= (cy + rc).min(ny - 1) {
        let mut x = (cx - rc).max(0) & !1;
        while x <= (cx + rc).min(n - 1) {
            let c = v2((x as f32 + 1.0) * CELL, (y as f32 + 1.0) * CELL);
            let off = c - pos;
            let dist = off.len();
            let at = block.building((y * n + x) as usize);
            if dist <= radius && dist > 0.0 && off.dot(dir) >= cos_half * dist && map.sight_line(eye, c.at(map.h(c) + 1.5), block, terrain, [from, at]) {
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    if x + dx < n && y + dy < ny {
                        out.push(((y + dy) * n + x + dx) as u32);
                    }
                }
            }
            x += 2;
        }
        y += 2;
    }
    out
}

#[allow(clippy::too_many_arguments)]
pub fn update_intel(
    view: Res<ViewState>,
    game: Res<Game>,
    defs: Res<Defs>,
    map: Res<MapData>,
    mut fog: ResMut<FogGrid>,
    mut mem: ResMut<IntelMemory>,
    tex: Option<Res<FogTexture>>,
    mut images: ResMut<Assets<Image>>,
    q: Query<(&Unit, &Body, &Intel, Option<&UnderConstruction>, Option<&Hosted>, Has<Paused>)>,
    (features, wrecks): (Res<Features>, Res<WreckList>),
) {
    if mem.tick == game.tick {
        return;
    }
    let standing = q.iter().filter(|(u, _, _, uc, hosted, _)| uc.is_none() && hosted.is_none() && defs.get(u.def).is_structure()).map(|(u, b, ..)| (u.id, u.def, b.pos));
    let block = sight_block(&features, &wrecks, standing, &defs, &map);
    mem.tick = game.tick;
    let team = game.team_of(view.me);
    let bit = 1u16 << team;
    let reveal = reveal(&game, view.me);
    let n = map.nx;
    // Weather shortens sight and radar (as in `sim::systems::visibility`).
    let wx = defs.rules.weather_of(game.weather);

    // --- fog grid
    let fog = &mut *fog;
    fog.alpha.fill(if reveal { 0 } else { FOG_DARK });
    if !reveal {
        let mut budget = 1;
        let mut sight_budget = 60;
        let mut alive = std::collections::HashSet::new();
        let mut looking = std::collections::HashSet::new();
        let mut spotting = std::collections::HashSet::new();
        let mut cone_budget = 24;
        // Infantry inside a building look out from its roof (as in `sim::systems::visibility`).
        let garrisons: HashMap<u32, (f32, f32)> = q.iter().filter(|(u, ..)| defs.get(u.def).is_garrison()).map(|(u, ..)| (u.id, (defs.get(u.def).height(), defs.get(u.def).roof_vision(&defs.rules)))).collect();
        for (u, b, intel, uc, hosted, paused) in &q {
            let roof = match hosted {
                Some(h) => match garrisons.get(&h.0) {
                    Some(r) => Some(*r),
                    None => continue,
                },
                None => None,
            };
            if game.team_of(u.owner) != team || uc.is_some() {
                continue;
            }
            let d = defs.get(u.def);
            let top = roof.map_or(d.height().max(1.0), |r| r.0);
            let from = hosted.map_or(u.id, |h| h.0);
            // From a building's roof they see further (as in `sim::systems::visibility`).
            let gain = roof.map_or(1.0, |r| r.1);
            let vision = d.vision * gain * wx.vision;
            let power_out = game.players.get(u.owner as usize).map_or(false, |p| p.power_out);
            // The fog shows the furthest radar reach (what unstealthed units are found within).
            let reach = d.radar_reach() * wx.radar;
            if reach > 0.0 && !power_out && !paused {
                alive.insert(u.id);
                let stale = fog.radar.get(&u.id).map_or(true, |c| c.0.dist(b.pos) > CELL * 1.5 || c.1 != reach);
                if stale && (budget > 0 || !fog.radar.contains_key(&u.id)) {
                    budget -= 1;
                    let eye = b.pos.at(b.y + top + defs.rules.radar_los_height);
                    fog.radar.insert(u.id, (b.pos, reach, covered_cells(&map, eye, b.pos, reach, 0.0, &SightBlock::default(), defs.rules.radar_blocked_by_terrain, 0)));
                }
                if let Some(c) = fog.radar.get(&u.id) {
                    for i in &c.2 {
                        fog.alpha[*i as usize] = FOG_RADAR;
                    }
                }
            }
            if vision > 0.0 {
                looking.insert(u.id);
                // Sight is shadowed by hills like radar. A unit's coverage is recomputed when it
                // has moved a cell or so; a few dozen per tick keep large armies cheap.
                let stale = fog.sight.get(&u.id).map_or(true, |c| c.0.dist(b.pos) > CELL || c.1 != vision);
                if stale && (sight_budget > 0 || !fog.sight.contains_key(&u.id)) {
                    sight_budget -= 1;
                    let eye = b.pos.at(b.y + top + defs.rules.vision_los_height);
                    fog.sight.insert(u.id, (b.pos, vision, covered_cells(&map, eye, b.pos, vision, defs.rules.near_sight, &block, defs.rules.sight_blocked_by_terrain, from)));
                }
            }
            // Binoculars turn all the time: their cone is redone when it has turned a degree or so.
            if let Some(bd) = d.binoculars.as_ref().filter(|bd| bd.range > 0.0) {
                spotting.insert(u.id);
                let range = bd.range * gain * wx.binoculars;
                let stale = fog.cone.get(&u.id).map_or(true, |c| c.0.dist(b.pos) > CELL || crate::map::wrap(c.1 - intel.look).abs() > 0.02 || c.2 != range);
                if stale && (cone_budget > 0 || !fog.cone.contains_key(&u.id)) {
                    cone_budget -= 1;
                    let eye = b.pos.at(b.y + top + defs.rules.vision_los_height);
                    let cells = covered_cone(&map, eye, b.pos, intel.look, bd.angle.clamp(1.0, 170.0).to_radians(), range, &block, defs.rules.sight_blocked_by_terrain, from);
                    fog.cone.insert(u.id, (b.pos, intel.look, range, cells));
                }
            }
        }
        // A working satellite uplink: radar everywhere, sight round every enemy commander (as in
        // `sim::systems::visibility`).
        let sat = q
            .iter()
            .filter(|(u, _, _, uc, hosted, paused)| game.team_of(u.owner) == team && uc.is_none() && hosted.is_none() && !paused && game.players.get(u.owner as usize).map_or(true, |p| !p.power_out))
            .filter_map(|(u, ..)| defs.get(u.def).satellite.map(|s| s.vision))
            .fold(0.0f32, f32::max);
        if sat > 0.0 {
            for a in fog.alpha.iter_mut() {
                *a = (*a).min(FOG_RADAR);
            }
            let r = (sat / CELL).ceil() as i32;
            for (u, b, ..) in &q {
                let t = game.team_of(u.owner);
                if t == team || !hostile(team, t) || !defs.factions.iter().any(|f| f.commander == Some(u.def)) {
                    continue;
                }
                let (cx, cy) = ((b.pos.x / CELL) as i32, (b.pos.y / CELL) as i32);
                for y in (cy - r).max(0)..=(cy + r).min(fog.ny as i32 - 1) {
                    for x in (cx - r).max(0)..=(cx + r).min(fog.n as i32 - 1) {
                        if v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL).dist(b.pos) <= sat {
                            fog.alpha[y as usize * fog.n + x as usize] = 0;
                        }
                    }
                }
            }
        }
        fog.radar.retain(|id, _| alive.contains(id));
        fog.sight.retain(|id, _| looking.contains(id));
        fog.cone.retain(|id, _| spotting.contains(id));
        // Sight goes on last: it clears the fog completely, radar only thins it.
        for c in fog.sight.values() {
            for i in &c.2 {
                fog.alpha[*i as usize] = 0;
            }
        }
        for c in fog.cone.values() {
            for i in &c.3 {
                fog.alpha[*i as usize] = 0;
            }
        }
    }
    let _ = n;
    if let Some(img) = tex.and_then(|t| images.get_mut(&t.0)) {
        if let Some(data) = img.into_inner().data.as_mut() {
            // The colour channels carry the brightness that trees and rocks are drawn with: the
            // same darkening as the black overlay of that alpha gives the ground (sRGB encoded).
            let mut light = [0u8; 256];
            for (a, l) in light.iter_mut().enumerate() {
                let lin = 1.0 - a as f32 / 255.0;
                let enc = if lin <= 0.0031308 { lin * 12.92 } else { 1.055 * lin.powf(1.0 / 2.4) - 0.055 };
                *l = (enc * 255.0).round().clamp(0.0, 255.0) as u8;
            }
            for (px, a) in data.chunks_exact_mut(4).zip(fog.alpha.iter()) {
                let l = light[*a as usize];
                px[0] = l;
                px[1] = l;
                px[2] = l;
                px[3] = *a;
            }
        }
    }

    // --- contact memory
    // Lobby option: the map's neutral and hostile buildings start out as remembered buildings.
    if game.reveal_props && !mem.props_known {
        mem.props_known = true;
        for (u, b, _, uc, hosted, _) in &q {
            let d = defs.get(u.def);
            if (u.owner == NEUTRAL || u.owner == HOSTILE) && d.is_structure() && hosted.is_none() && game.team_of(u.owner) != team {
                let built = uc.map(|p| p.0);
                mem.ghosts.insert(u.id, Ghost { id: u.id, def: Some(u.def), owner: u.owner, pos: b.pos, y: b.y, yaw: b.yaw, built, structure: true, last_tick: game.tick, live: false, seen: false, gone: false });
            }
        }
    }
    let exists: std::collections::HashSet<u32> = q.iter().map(|(u, ..)| u.id).collect();
    for g in mem.ghosts.values_mut() {
        g.live = false;
        g.seen = false;
        g.gone = !exists.contains(&g.id);
    }
    for (u, b, intel, uc, hosted, _) in &q {
        let ut = game.team_of(u.owner);
        if ut == team || hosted.is_some() {
            continue;
        }
        let d = defs.get(u.def);
        let seen = reveal || intel.seen & bit != 0;
        let radar = intel.radar & bit != 0;
        if !seen && !radar {
            continue;
        }
        let g = mem.ghosts.entry(u.id).or_insert(Ghost { id: u.id, def: None, owner: u.owner, pos: b.pos, y: b.y, yaw: b.yaw, built: None, structure: d.is_structure(), last_tick: game.tick, live: true, seen, gone: false });
        g.pos = b.pos;
        g.y = b.y;
        g.last_tick = game.tick;
        g.live = true;
        g.seen = seen;
        if seen {
            // What it looks like now: a ghost model shows this once it is out of sight.
            g.def = Some(u.def);
            g.owner = u.owner;
            g.yaw = b.yaw;
            g.built = uc.map(|p| p.0);
        } else if !g.structure {
            // A mobile blip tells us where, not what.
            g.def = None;
        }
    }
    let fade = (defs.rules.radar_fade_secs / DT) as u32;
    let tick = game.tick;
    mem.ghosts.retain(|_, g| {
        if g.live {
            return true;
        }
        if g.structure {
            // Buildings stay on the map until we look at the spot again and find it empty.
            !fog.visible(g.pos)
        } else {
            tick.saturating_sub(g.last_tick) < fade
        }
    });
}
