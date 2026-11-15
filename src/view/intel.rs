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
    pub n: usize,
    pub alpha: Vec<u8>,
    /// Cached radar coverage per radar unit: (position it was computed for, radius, covered cells).
    radar: HashMap<u32, (V2, f32, Vec<u32>)>,
    /// The same for what each unit sees directly.
    sight: HashMap<u32, (V2, f32, Vec<u32>)>,
}

impl FogGrid {
    pub fn new(n: usize) -> FogGrid {
        FogGrid { n, alpha: vec![0; n * n], radar: HashMap::new(), sight: HashMap::new() }
    }
    /// Is this spot currently in direct sight?
    pub fn visible(&self, p: V2) -> bool {
        let (x, y) = ((p.x / CELL) as i32, (p.y / CELL) as i32);
        if x < 0 || y < 0 || x >= self.n as i32 || y >= self.n as i32 {
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
    pub structure: bool,
    pub last_tick: u32,
    /// Detected right now (by sight or radar).
    pub live: bool,
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
/// counts regardless, like in the simulation's own visibility test). `block`: trees, rocks and
/// wrecks that block the line too (sight; radar passes them, empty). `terrain`: hills block it.
fn covered_cells(map: &MapData, eye: Vec3, pos: V2, radius: f32, near: f32, block: &[f32], terrain: bool) -> Vec<u32> {
    let n = map.n as i32;
    let (cx, cy, rc) = ((pos.x / CELL) as i32, (pos.y / CELL) as i32, (radius / CELL) as i32);
    let mut out = vec![];
    // Sample every second cell and fill 2x2 blocks: plenty for a fog overlay.
    let mut y = (cy - rc).max(0) & !1;
    while y <= (cy + rc).min(n - 1) {
        let mut x = (cx - rc).max(0) & !1;
        while x <= (cx + rc).min(n - 1) {
            let c = v2((x as f32 + 1.0) * CELL, (y as f32 + 1.0) * CELL);
            let dist = c.dist(pos);
            if dist <= radius && (dist < near || map.sight_line(eye, c.at(map.h(c) + 1.5), block, terrain)) {
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    if x + dx < n && y + dy < n {
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
    q: Query<(&Unit, &Body, &Intel, Has<UnderConstruction>, Option<&Hosted>, Has<Paused>)>,
    (features, wrecks): (Res<Features>, Res<WreckList>),
) {
    if mem.tick == game.tick {
        return;
    }
    let block = sight_block(&features, &wrecks, &defs, &map);
    mem.tick = game.tick;
    let team = game.team_of(view.me);
    let bit = 1u16 << team;
    let reveal = reveal(&game, view.me);
    let n = map.n;

    // --- fog grid
    let fog = &mut *fog;
    fog.alpha.fill(if reveal { 0 } else { FOG_DARK });
    if !reveal {
        let mut budget = 1;
        let mut sight_budget = 60;
        let mut alive = std::collections::HashSet::new();
        let mut looking = std::collections::HashSet::new();
        // Infantry inside a building look out from its roof (as in `sim::systems::visibility`).
        let garrisons: HashMap<u32, f32> = q.iter().filter(|(u, ..)| defs.get(u.def).is_garrison()).map(|(u, ..)| (u.id, defs.get(u.def).height())).collect();
        for (u, b, _, uc, hosted, paused) in &q {
            let roof = match hosted {
                Some(h) => match garrisons.get(&h.0) {
                    Some(r) => Some(*r),
                    None => continue,
                },
                None => None,
            };
            if game.team_of(u.owner) != team || uc {
                continue;
            }
            let d = defs.get(u.def);
            let top = roof.unwrap_or(d.height().max(1.0));
            let power_out = game.players.get(u.owner as usize).map_or(false, |p| p.power_out);
            if d.radar > 0.0 && !power_out && !paused {
                alive.insert(u.id);
                let stale = fog.radar.get(&u.id).map_or(true, |c| c.0.dist(b.pos) > CELL * 1.5 || c.1 != d.radar);
                if stale && (budget > 0 || !fog.radar.contains_key(&u.id)) {
                    budget -= 1;
                    let eye = b.pos.at(b.y + top + defs.rules.radar_los_height);
                    fog.radar.insert(u.id, (b.pos, d.radar, covered_cells(&map, eye, b.pos, d.radar, 0.0, &[], defs.rules.radar_blocked_by_terrain)));
                }
                if let Some(c) = fog.radar.get(&u.id) {
                    for i in &c.2 {
                        fog.alpha[*i as usize] = FOG_RADAR;
                    }
                }
            }
            if d.vision > 0.0 {
                looking.insert(u.id);
                // Sight is shadowed by hills like radar. A unit's coverage is recomputed when it
                // has moved a cell or so; a few dozen per tick keep large armies cheap.
                let stale = fog.sight.get(&u.id).map_or(true, |c| c.0.dist(b.pos) > CELL || c.1 != d.vision);
                if stale && (sight_budget > 0 || !fog.sight.contains_key(&u.id)) {
                    sight_budget -= 1;
                    let eye = b.pos.at(b.y + top + defs.rules.vision_los_height);
                    fog.sight.insert(u.id, (b.pos, d.vision, covered_cells(&map, eye, b.pos, d.vision, defs.rules.near_sight, &block, defs.rules.sight_blocked_by_terrain)));
                }
            }
        }
        fog.radar.retain(|id, _| alive.contains(id));
        fog.sight.retain(|id, _| looking.contains(id));
        // Sight goes on last: it clears the fog completely, radar only thins it.
        for c in fog.sight.values() {
            for i in &c.2 {
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
        for (u, b, _, _, hosted, _) in &q {
            let d = defs.get(u.def);
            if (u.owner == NEUTRAL || u.owner == HOSTILE) && d.is_structure() && hosted.is_none() && game.team_of(u.owner) != team {
                mem.ghosts.insert(u.id, Ghost { id: u.id, def: Some(u.def), owner: u.owner, pos: b.pos, y: b.y, structure: true, last_tick: game.tick, live: false });
            }
        }
    }
    for g in mem.ghosts.values_mut() {
        g.live = false;
    }
    for (u, b, intel, _, hosted, _) in &q {
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
        let g = mem.ghosts.entry(u.id).or_insert(Ghost { id: u.id, def: None, owner: u.owner, pos: b.pos, y: b.y, structure: d.is_structure(), last_tick: game.tick, live: true });
        g.pos = b.pos;
        g.y = b.y;
        g.last_tick = game.tick;
        g.live = true;
        if seen {
            g.def = Some(u.def);
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
