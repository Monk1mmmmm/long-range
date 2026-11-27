//! What the ground looks like: texture layers painted over each other (SupCom strata) and roads.
//! Purely visual: nothing here reaches the simulation. The view (`view::surface`) turns it into
//! textures; this module holds the data, the automatic painting for each biome and the road raster.
use crate::map::*;
use serde::{Deserialize, Serialize};

/// Most layers a map can have: the base layer plus eight painted ones (two RGBA mask textures).
pub const MAX_LAYERS: usize = 9;
/// Most road kinds one map can show (one channel of the two road textures each; the sidewalks
/// take one when a road has them).
pub const MAX_ROAD_KINDS: usize = 8;
/// Centre lines: roads of a `lines` kind at least this wide get one, dashed ...
pub const CENTRE_MIN: f32 = 3.5;
/// ... and solid from this width on.
pub const CENTRE_SOLID: f32 = 6.5;
/// ... and at least this long (shorter ones are the links of big junctions).
pub const CENTRE_MIN_LEN: f32 = 15.0;
/// Distance along a road kept in the centre-line raster (a multiple of the dash period, 4).
pub const CENTRE_WRAP: f32 = 16.0;
/// Ground kind of the sidewalks along roads with `Road::sidewalk`.
pub const SIDEWALK: &str = "sidewalk";
/// Road raster: the field falls from 1 to 0 over this many world units across the road's edge
/// (0.5 on the edge itself), so bilinear filtering of a coarse texture still gives a crisp line.
pub const ROAD_RAMP: f32 = 2.0;

/// One texture layer. Layer 0 covers the whole map; every further layer is drawn over the ones
/// below where its mask says so.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Layer {
    /// Ground kind id (`KINDS`).
    pub kind: String,
    /// Size of one texture tile in world units; 0 = the kind's default.
    #[serde(default)]
    pub scale: f32,
}

/// A road: a line of points with a width, drawn as a texture of its own kind (asphalt, dirt...),
/// with sidewalks `sidewalk` wide on both sides (0 = none; kind `SIDEWALK`, drawn under every road).
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Road {
    pub kind: String,
    pub width: f32,
    pub points: Vec<V2>,
    #[serde(default)]
    pub sidewalk: f32,
}

impl Road {
    pub fn new(kind: &str, width: f32, points: Vec<V2>) -> Road {
        Road { kind: kind.into(), width, points, sidewalk: 0.0 }
    }
    /// Half the width it covers, sidewalks included.
    pub fn reach(&self) -> f32 {
        self.width * 0.5 + self.sidewalk.max(0.0)
    }
}

/// A ground texture: id, name for the editor, default tile size and its average colour (linear
/// RGB, used for the minimap and the lobby picture, and as the base of the procedural texture).
pub struct Kind {
    pub id: &'static str,
    pub name: &'static str,
    pub scale: f32,
    pub color: [f32; 3],
    /// Roads of this kind have a hard edge (paved) rather than a frayed one (tracks).
    pub hard: bool,
    /// Roads of this kind get lane markings: a centre line, and edge lines where they have no
    /// sidewalk (main roads; `MapData::centre_raster`).
    pub lines: bool,
}

pub const KINDS: &[Kind] = &[
    Kind { id: "grass", name: "Grass", scale: 10.0, color: [0.12, 0.22, 0.06], hard: false, lines: false },
    Kind { id: "meadow", name: "Dry meadow", scale: 12.0, color: [0.25, 0.25, 0.10], hard: false, lines: false },
    Kind { id: "forest_floor", name: "Forest floor", scale: 9.0, color: [0.13, 0.15, 0.07], hard: false, lines: false },
    Kind { id: "dirt", name: "Dirt", scale: 8.0, color: [0.27, 0.19, 0.12], hard: false, lines: false },
    Kind { id: "mud", name: "Mud", scale: 7.0, color: [0.14, 0.12, 0.08], hard: false, lines: false },
    Kind { id: "sand", name: "Sand", scale: 9.0, color: [0.52, 0.43, 0.27], hard: false, lines: false },
    Kind { id: "dunes", name: "Desert sand", scale: 16.0, color: [0.62, 0.45, 0.25], hard: false, lines: false },
    Kind { id: "cracked", name: "Cracked earth", scale: 15.0, color: [0.45, 0.32, 0.20], hard: false, lines: false },
    Kind { id: "snow", name: "Snow", scale: 14.0, color: [0.80, 0.83, 0.88], hard: false, lines: false },
    Kind { id: "ice", name: "Ice", scale: 16.0, color: [0.55, 0.66, 0.76], hard: false, lines: false },
    Kind { id: "stone", name: "Stone", scale: 12.0, color: [0.30, 0.28, 0.26], hard: false, lines: false },
    Kind { id: "gravel", name: "Gravel", scale: 6.0, color: [0.36, 0.34, 0.31], hard: false, lines: false },
    Kind { id: "paving", name: "Paving", scale: 6.0, color: [0.40, 0.39, 0.37], hard: true, lines: false },
    Kind { id: "cobble", name: "Cobblestones", scale: 4.0, color: [0.20, 0.17, 0.14], hard: true, lines: false },
    Kind { id: "concrete", name: "Concrete slabs", scale: 8.0, color: [0.28, 0.27, 0.25], hard: true, lines: false },
    Kind { id: "asphalt_worn", name: "Worn asphalt", scale: 7.0, color: [0.10, 0.10, 0.10], hard: true, lines: false },
    Kind { id: "asphalt", name: "Asphalt", scale: 6.0, color: [0.055, 0.055, 0.06], hard: true, lines: true },
    Kind { id: "dirt_road", name: "Dirt track", scale: 7.0, color: [0.32, 0.24, 0.15], hard: false, lines: false },
    Kind { id: "sidewalk", name: "Sidewalk", scale: 3.0, color: [0.31, 0.30, 0.29], hard: true, lines: false },
];

pub fn kind(id: &str) -> Option<&'static Kind> {
    KINDS.iter().find(|k| k.id == id)
}

/// Kind of ground random maps (and the editor's automatic painting) use.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Biome {
    Temperate,
    Desert,
    Arctic,
    Rocky,
}

pub const BIOMES: [Biome; 4] = [Biome::Temperate, Biome::Desert, Biome::Arctic, Biome::Rocky];

impl Biome {
    pub fn name(self) -> &'static str {
        match self {
            Biome::Temperate => "Temperate",
            Biome::Desert => "Desert",
            Biome::Arctic => "Arctic",
            Biome::Rocky => "Rocky highland",
        }
    }
    /// The biome a random map of this seed gets when the lobby leaves it to chance.
    pub fn from_seed(seed: u64) -> Biome {
        let mut h = seed.wrapping_mul(0x9E3779B97F4A7C15) ^ 0xB10E;
        h ^= h >> 31;
        h = h.wrapping_mul(0xBF58476D1CE4E5B9);
        h ^= h >> 29;
        // Temperate comes up twice as often as each of the others.
        match h % 5 {
            0 | 1 => Biome::Temperate,
            2 => Biome::Desert,
            3 => Biome::Arctic,
            _ => Biome::Rocky,
        }
    }
}

fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl MapData {
    /// Number of bytes of one layer mask (one per cell).
    pub fn mask_len(&self) -> usize {
        self.cells()
    }
    /// Mask value (0-255) of layer `layer` (1..) at cell (x, y).
    pub fn mask(&self, layer: usize, x: usize, y: usize) -> u8 {
        if layer == 0 {
            return 255;
        }
        self.masks.get((layer - 1) * self.mask_len() + y.min(self.ny.saturating_sub(1)) * self.nx + x.min(self.nx.saturating_sub(1))).copied().unwrap_or(0)
    }
    /// Bilinear mask value (0..1) at a world position (texel centres are the cell centres).
    pub fn mask_at(&self, layer: usize, p: V2) -> f32 {
        if layer == 0 {
            return 1.0;
        }
        let fx = (p.x / CELL - 0.5).clamp(0.0, self.nx as f32 - 1.0);
        let fy = (p.y / CELL - 0.5).clamp(0.0, self.ny as f32 - 1.0);
        let (x, y) = (fx.floor() as usize, fy.floor() as usize);
        let (tx, ty) = (fx - x as f32, fy - y as f32);
        let g = |x: usize, y: usize| self.mask(layer, x, y) as f32 / 255.0;
        let a = g(x, y) + (g(x + 1, y) - g(x, y)) * tx;
        let b = g(x, y + 1) + (g(x + 1, y + 1) - g(x, y + 1)) * tx;
        a + (b - a) * ty
    }
    /// Makes `masks` fit the layer list (after layers were added or removed at the end).
    pub fn fit_masks(&mut self) {
        let want = self.layers.len().saturating_sub(1) * self.mask_len();
        self.masks.resize(want, 0);
    }
    /// Adds a layer of `kind` on top with an empty mask; returns its index (or None when full).
    pub fn add_layer(&mut self, kind: &str) -> Option<usize> {
        if self.layers.len() >= MAX_LAYERS {
            return None;
        }
        if self.layers.is_empty() {
            self.layers.push(Layer { kind: kind.into(), scale: 0.0 });
            return Some(0);
        }
        self.layers.push(Layer { kind: kind.into(), scale: 0.0 });
        self.fit_masks();
        Some(self.layers.len() - 1)
    }
    /// Index of the topmost layer of this kind, added if there is none (and room).
    pub fn layer_of(&mut self, kind: &str) -> Option<usize> {
        match self.layers.iter().rposition(|l| l.kind == kind) {
            Some(i) => Some(i),
            None => self.add_layer(kind),
        }
    }
    /// Removes layer `i` (> 0) and its mask.
    pub fn remove_layer(&mut self, i: usize) {
        if i == 0 || i >= self.layers.len() {
            return;
        }
        let len = self.mask_len();
        self.masks.drain((i - 1) * len..i * len);
        self.layers.remove(i);
    }
    /// Swaps layers `i` and `i + 1` (both > 0) with their masks.
    pub fn swap_layers(&mut self, i: usize) {
        if i == 0 || i + 1 >= self.layers.len() {
            return;
        }
        let len = self.mask_len();
        let (a, b) = self.masks.split_at_mut(i * len);
        a[(i - 1) * len..].swap_with_slice(&mut b[..len]);
        self.layers.swap(i, i + 1);
    }

    /// Brush on a layer mask: moves the cells within `radius` of `c` towards `target` (0..1) by
    /// up to `amount`, less towards the rim. On layer 0 (which covers everything anyway) it takes
    /// every layer above away instead. Returns whether a cell changed.
    pub fn paint_mask(&mut self, layer: usize, c: V2, radius: f32, amount: f32, target: f32) -> bool {
        let (n, ny) = self.dims();
        if n == 0 || ny == 0 || layer >= self.layers.len() {
            return false;
        }
        self.fit_masks();
        let r = radius.max(CELL * 0.5);
        let (x0, x1) = (((c.x - r) / CELL).floor().max(0.0) as usize, (((c.x + r) / CELL).ceil() as usize).min(n - 1));
        let (y0, y1) = (((c.y - r) / CELL).floor().max(0.0) as usize, (((c.y + r) / CELL).ceil() as usize).min(ny - 1));
        let planes: Vec<usize> = if layer == 0 { (1..self.layers.len()).collect() } else { vec![layer] };
        let target = if layer == 0 { 0.0 } else { target.clamp(0.0, 1.0) } * 255.0;
        let mut changed = false;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL).dist(c) / r;
                if d >= 1.0 {
                    continue;
                }
                let fall = smooth(1.0, 0.35, d) * amount.clamp(0.0, 1.0);
                for l in &planes {
                    let m = &mut self.masks[(l - 1) * n * ny + y * n + x];
                    let v = *m as f32;
                    // At least one step per stroke, so slow brushes still get there.
                    let step = (target - v) * fall;
                    let new = if step.abs() < 1.0 && fall > 0.0 && v != target { v + (target - v).signum() } else { v + step };
                    let new = new.round().clamp(0.0, 255.0) as u8;
                    if new != *m {
                        *m = new;
                        changed = true;
                    }
                }
            }
        }
        changed
    }

    /// Paints the ground from the heights for a biome: replaces all layers and masks (roads stay).
    pub fn paint_ground(&mut self, biome: Biome) {
        let (n, ny) = self.dims();
        if n == 0 || ny == 0 {
            return;
        }
        if self.sky.is_empty() {
            self.sky = crate::view::sky::for_biome(biome).to_string();
        }
        let seed = self.seed;
        let (lo, hi) = self.heights.iter().fold((f32::MAX, f32::MIN), |(a, b), h| (a.min(*h), b.max(*h)));
        let water = self.water;
        // Height above the water as a share of the highest ground (peaks get snow, etc.).
        let top = (hi - water.max(lo)).max(1.0);
        // (kind, weight function of (height, slope, height share, position))
        type Rule = (&'static str, fn(&MapData, f32, f32, f32, V2, u64) -> f32);
        fn patches(p: V2, freq: f32, seed: u64, salt: u64, a: f32, b: f32) -> f32 {
            smooth(a, b, fbm(p.x * freq + 17.0, p.y * freq + 17.0, seed ^ salt))
        }
        // Shores follow the water nearby: the sea, or a lake a cell or two away.
        fn shore(m: &MapData, h: f32, p: V2) -> f32 {
            ((m.water_near(p, 2) + 0.9 - h) / 0.9).clamp(0.0, 1.0)
        }
        fn under(m: &MapData, h: f32, p: V2) -> f32 {
            ((m.water_near(p, 1) - 0.2 - h) / 1.5).clamp(0.0, 1.0)
        }
        fn cliff(s: f32, a: f32, b: f32) -> f32 {
            smooth(a, b, s)
        }
        let rules: Vec<Rule> = match biome {
            Biome::Temperate => vec![
                ("grass", |_, _, _, _, _, _| 1.0),
                ("meadow", |_, _, _, _, p, s| patches(p, 1.0 / 60.0, s, 0x21, 0.52, 0.66)),
                ("dirt", |_, _, sl, _, p, s| patches(p, 1.0 / 22.0, s, 0x33, 0.62, 0.72).max(smooth(0.06, 0.11, sl) * 0.8)),
                ("sand", |m, h, _, _, p, _| shore(m, h, p)),
                ("mud", |m, h, _, _, p, _| under(m, h, p)),
                ("stone", |_, _, sl, f, _, _| cliff(sl, 0.12, 0.22).max(smooth(0.82, 0.95, f))),
            ],
            Biome::Desert => vec![
                ("dunes", |_, _, _, _, _, _| 1.0),
                ("cracked", |_, _, _, f, p, s| patches(p, 1.0 / 45.0, s, 0x45, 0.50, 0.62) * (1.0 - smooth(0.5, 0.8, f))),
                ("gravel", |_, _, sl, _, p, s| patches(p, 1.0 / 25.0, s, 0x57, 0.64, 0.74).max(smooth(0.05, 0.10, sl))),
                ("sand", |m, h, _, _, p, _| shore(m, h, p)),
                ("mud", |m, h, _, _, p, _| under(m, h, p)),
                ("stone", |_, _, sl, _, _, _| cliff(sl, 0.10, 0.20)),
            ],
            Biome::Arctic => vec![
                ("snow", |_, _, _, _, _, _| 1.0),
                // Frozen shores and wind-blown ice on flat low ground.
                ("ice", |m, h, sl, f, p, s| ((m.water_near(p, 2) + 0.6 - h) / 0.8).clamp(0.0, 1.0).max(patches(p, 1.0 / 50.0, s, 0x69, 0.66, 0.74) * (1.0 - smooth(0.02, 0.05, sl)) * (1.0 - smooth(0.2, 0.4, f)))),
                ("gravel", |_, _, sl, _, p, s| patches(p, 1.0 / 30.0, s, 0x71, 0.64, 0.74) * 0.9 + smooth(0.07, 0.13, sl) * 0.3),
                ("mud", |m, h, _, _, p, _| under(m, h, p)),
                ("stone", |_, _, sl, _, _, _| cliff(sl, 0.14, 0.26)),
                ("snow", |_, _, sl, f, _, _| smooth(0.75, 0.9, f) * (1.0 - cliff(sl, 0.3, 0.45))),
            ],
            Biome::Rocky => vec![
                ("dirt", |_, _, _, _, _, _| 1.0),
                ("grass", |_, _, sl, f, p, s| (1.0 - smooth(0.04, 0.09, sl)) * (1.0 - smooth(0.45, 0.65, f)) * patches(p, 1.0 / 40.0, s, 0x83, 0.40, 0.52)),
                ("gravel", |_, _, sl, _, p, s| patches(p, 1.0 / 26.0, s, 0x95, 0.60, 0.70).max(smooth(0.06, 0.12, sl) * 0.7)),
                ("mud", |m, h, _, _, p, _| under(m, h, p).max(shore(m, h, p) * 0.6)),
                ("stone", |_, _, sl, f, _, _| cliff(sl, 0.10, 0.18).max(smooth(0.55, 0.75, f))),
                ("snow", |_, _, sl, f, _, _| smooth(0.86, 0.97, f) * (1.0 - cliff(sl, 0.25, 0.4))),
            ],
        };
        self.layers = rules.iter().map(|(k, _)| Layer { kind: (*k).into(), scale: 0.0 }).collect();
        self.masks = vec![0; (rules.len() - 1) * n * ny];
        for y in 0..ny {
            for x in 0..n {
                let p = v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL);
                let h = self.h(p);
                let slope = 1.0 - self.normal(p.x, p.y).y;
                let f = ((h - water) / top).clamp(0.0, 1.0);
                for (i, (_, rule)) in rules.iter().enumerate().skip(1) {
                    let v = rule(self, h, slope, f, p, seed).clamp(0.0, 1.0);
                    self.masks[(i - 1) * n * ny + y * n + x] = (v * 255.0).round() as u8;
                }
            }
        }
    }

    /// Distinct road kinds, at most `MAX_ROAD_KINDS`, in drawing order: sidewalks first, then the
    /// tracks, then the paved kinds in `KINDS` order (main asphalt last, over the side streets).
    pub fn road_kinds(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        if self.roads.iter().any(|r| r.sidewalk > 0.0) {
            out.push(SIDEWALK.into());
        }
        for r in &self.roads {
            if !out.contains(&r.kind) && out.len() < MAX_ROAD_KINDS {
                out.push(r.kind.clone());
            }
        }
        let rank = |id: &str| {
            let i = KINDS.iter().position(|k| k.id == id).unwrap_or(KINDS.len());
            let class = if id == SIDEWALK { 0 } else if kind(id).is_some_and(|k| k.hard) { 2 } else { 1 };
            (class, i)
        };
        out.sort_by_key(|k| rank(k));
        out
    }

    /// The roads as a raster of `per_unit` texels per world unit (texels across and deep, then
    /// the texels row by row), one channel per road kind (`road_kinds`): 1 inside, 0.5 on the
    /// edge, 0 beyond `ROAD_RAMP / 2` outside. Sidewalks are a band `sidewalk` wider on both sides
    /// in the `SIDEWALK` channel. Nothing is drawn under water (sea or lake): the field fades out
    /// at the waterline.
    pub fn road_raster(&self, per_unit: f32) -> ((usize, usize), Vec<[u8; MAX_ROAD_KINDS]>) {
        let side = ((self.world_x() * per_unit).round() as usize).max(1);
        let side_y = ((self.world_z() * per_unit).round() as usize).max(1);
        let mut out = vec![[0u8; MAX_ROAD_KINDS]; side * side_y];
        let kinds = self.road_kinds();
        let walk = kinds.iter().position(|k| k == SIDEWALK);
        let texel = 1.0 / per_unit;
        // Dryness of every texel, made once: 1 well above the water, 0.5 at the waterline.
        let mut dry: Vec<u8> = vec![];
        let mut dry_at = |x: usize, y: usize| -> f32 {
            if dry.is_empty() {
                dry = vec![0; side * side_y];
            }
            let c = &mut dry[y * side + x];
            if *c == 0 {
                let (px, pz) = ((x as f32 + 0.5) * texel, (y as f32 + 0.5) * texel);
                let v = (0.5 + (self.height(px, pz) - self.water_xz(px, pz) - 0.08) * 2.5).clamp(0.0, 1.0);
                *c = 1 + (v * 254.0).round() as u8;
            }
            (*c - 1) as f32 / 254.0
        };
        for r in &self.roads {
            let Some(ch) = kinds.iter().position(|k| *k == r.kind) else { continue };
            let bands: Vec<(usize, f32)> = std::iter::once((ch, r.width * 0.5)).chain(walk.filter(|_| r.sidewalk > 0.0).map(|w| (w, r.reach()))).collect();
            let reach = r.reach() + ROAD_RAMP * 0.5 + texel;
            for seg in r.points.windows(2) {
                let (a, b) = (seg[0], seg[1]);
                let ab = b - a;
                let l2 = ab.len2().max(1e-6);
                let x0 = ((a.x.min(b.x) - reach) * per_unit).floor().max(0.0) as usize;
                let x1 = (((a.x.max(b.x) + reach) * per_unit).ceil().max(0.0) as usize).min(side - 1);
                let y0 = ((a.y.min(b.y) - reach) * per_unit).floor().max(0.0) as usize;
                let y1 = (((a.y.max(b.y) + reach) * per_unit).ceil().max(0.0) as usize).min(side_y - 1);
                if x0 > x1 || y0 > y1 {
                    continue;
                }
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let p = v2((x as f32 + 0.5) * texel, (y as f32 + 0.5) * texel);
                        let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
                        let d = p.dist(a + ab * t);
                        if d > reach {
                            continue;
                        }
                        let wet = dry_at(x, y);
                        for &(c, hw) in &bands {
                            let v = (0.5 + (hw - d) / ROAD_RAMP).clamp(0.0, 1.0).min(wet);
                            let o = &mut out[y * side + x][c];
                            *o = (*o).max((v * 255.0).round() as u8);
                        }
                    }
                }
            }
        }
        ((side, side_y), out)
    }

    /// Lane markings of the roads whose kind has `lines` (main asphalt, at least `CENTRE_MIN` wide and
    /// `CENTRE_MIN_LEN` long),
    /// as a raster of `per_unit` texels per world unit: per texel the distance along the road modulo
    /// `CENTRE_WRAP` (R, 0..1) and the signed distance from the centre line (G, -4..4 mapped to 0..1),
    /// both linear across a texel so bilinear filtering keeps the lines exact; whether lines are drawn
    /// here (B: not under water, not where two marked roads cross); A: half the road's width (low 7
    /// bits, 0..4) and in the top bit whether the centre line is solid (the widest roads, and the
    /// divide of a road drawn as two parallel carriageways) rather than dashed. The shader draws the
    /// centre line and the edge lines from it.
    pub fn centre_raster(&self, per_unit: f32) -> ((usize, usize), Vec<[u8; 4]>) {
        // Short pieces are the links of big junctions: no markings there.
        let long = |r: &Road| r.points.windows(2).map(|s| s[0].dist(s[1])).sum::<f32>() >= CENTRE_MIN_LEN;
        let marked: Vec<&Road> = self.roads.iter().filter(|r| r.width >= CENTRE_MIN && kind(&r.kind).is_some_and(|k| k.lines) && long(r)).collect();
        if marked.is_empty() {
            return ((1, 1), vec![[0; 4]]);
        }
        let side = ((self.world_x() * per_unit).round() as usize).max(1);
        let side_y = ((self.world_z() * per_unit).round() as usize).max(1);
        let texel = 1.0 / per_unit;
        // Per texel: the nearest marked road (distance to its centre line, road, direction, offset,
        // arc) and the nearest other one.
        type Near = (f32, u32, V2, f32, f32);
        let none: Near = (f32::MAX, u32::MAX, v2(0.0, 0.0), 0.0, 0.0);
        let mut best = vec![(none, none); side * side_y];
        // Looked at well beyond each road, so every texel of one carriageway of a divided road also
        // sees the other (else its lines would jump where that coverage ends).
        const BEYOND: f32 = 7.0;
        for (ri, r) in marked.iter().enumerate() {
            let hw = r.width * 0.5 + BEYOND;
            let mut arc = 0.0;
            for seg in r.points.windows(2) {
                let (a, b) = (seg[0], seg[1]);
                let ab = b - a;
                let len = ab.len();
                if len < 1e-4 {
                    continue;
                }
                let dir = ab * (1.0 / len);
                let x0 = ((a.x.min(b.x) - hw) * per_unit).floor().max(0.0) as usize;
                let x1 = (((a.x.max(b.x) + hw) * per_unit).ceil().max(0.0) as usize).min(side - 1);
                let y0 = ((a.y.min(b.y) - hw) * per_unit).floor().max(0.0) as usize;
                let y1 = (((a.y.max(b.y) + hw) * per_unit).ceil().max(0.0) as usize).min(side_y - 1);
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let p = v2((x as f32 + 0.5) * texel, (y as f32 + 0.5) * texel);
                        let along = (p - a).dot(dir);
                        let t = along.clamp(0.0, len);
                        let d = p.dist(a + dir * t);
                        if d > hw {
                            continue;
                        }
                        let off = dir.x * (p.y - a.y) - dir.y * (p.x - a.x);
                        let n: Near = (d, ri as u32, dir, off, arc + along);
                        let (b0, b1) = &mut best[y * side + x];
                        if n.1 == b0.1 {
                            if d < b0.0 {
                                *b0 = n;
                            }
                        } else if d < b0.0 {
                            *b1 = *b0;
                            *b0 = n;
                        } else if n.1 == b1.1 {
                            if d < b1.0 {
                                *b1 = n;
                            }
                        } else if d < b1.0 {
                            *b1 = n;
                        }
                    }
                }
                arc += len;
            }
        }
        let mut out = vec![[0u8; 4]; side * side_y];
        for y in 0..side_y {
            for x in 0..side {
                let i = y * side + x;
                let (b0, b1) = best[i];
                // A little beyond the edge too, so filtering does not pull the edge lines out of place.
                if b0.1 == u32::MAX || b0.0 > marked[b0.1 as usize].width * 0.5 + 1.5 {
                    continue;
                }
                let w = marked[b0.1 as usize].width;
                let (mut off, mut solid, mut drawn, mut half) = (b0.3, w >= CENTRE_SOLID, true, w * 0.5);
                if b1.1 != u32::MAX {
                    let dot = b0.2.dot(b1.2);
                    let (w0, w1) = (marked[b0.1 as usize].width, marked[b1.1 as usize].width);
                    let other = b1.3 * dot.signum();
                    let sep = (b0.3 - other).abs();
                    let parallel = dot.abs() > 0.97;
                    if parallel && sep < 0.4 && b1.0 <= w1 * 0.5 + 0.5 {
                        // The next way of the same road: the same centre line.
                    } else if parallel && sep > 1.0 && sep < (w0 + w1) * 0.5 + 1.5 {
                        // The second carriageway of a divided road: one solid line between the two,
                        // measured the same way on both sides of the divide (texels nearer the other
                        // way take that one's direction), so it stays linear across it; edge lines
                        // along the outer edges of the two.
                        let canon = if b0.2.x > 0.0 || (b0.2.x == 0.0 && b0.2.y > 0.0) { 1.0 } else { -1.0 };
                        off = (b0.3 + other) * 0.5 * canon;
                        solid = true;
                        half = sep * 0.5 + (w0 + w1) * 0.25;
                    } else if b1.0 > w1 * 0.5 + 0.5 {
                        // Only near the other road, not on it: this road's lines.
                    } else {
                        // A crossing, a junction or roads merging: no lines through it.
                        drawn = false;
                    }
                }
                let (px, pz) = ((x as f32 + 0.5) * texel, (y as f32 + 0.5) * texel);
                let dry = self.height(px, pz) > self.water_xz(px, pz) + 0.05;
                // Drawn within half a unit beyond the edge: filtering then keeps the lines whole.
                let on = b0.0 <= w * 0.5 + 0.5;
                out[i] = [
                    ((b0.4.rem_euclid(CENTRE_WRAP) / CENTRE_WRAP) * 255.0).round() as u8,
                    (((off.clamp(-4.0, 4.0) + 4.0) * 0.125) * 255.0).round() as u8,
                    if dry && drawn && on { 255 } else { 0 },
                    ((half.clamp(0.0, 4.0) / 4.0) * 127.0).round() as u8 + if solid { 128 } else { 0 },
                ];
            }
        }
        ((side, side_y), out)
    }

    /// Distance from `p` to the nearest road's edge (negative on the road); `f32::MAX` without roads.
    pub fn road_gap(&self, p: V2) -> f32 {
        let mut best = f32::MAX;
        for r in &self.roads {
            for s in r.points.windows(2) {
                let ab = s[1] - s[0];
                let t = ((p - s[0]).dot(ab) / ab.len2().max(1e-6)).clamp(0.0, 1.0);
                best = best.min(p.dist(s[0] + ab * t) - r.reach());
            }
        }
        best
    }

    /// Cells (row by row) whose centre lies within `margin` of a road's edge or on it.
    pub fn road_cells(&self, margin: f32) -> Vec<bool> {
        let (n, ny) = self.dims();
        let mut out = vec![false; n * ny];
        for r in &self.roads {
            let reach = r.reach() + margin;
            for s in r.points.windows(2) {
                let (a, b) = (s[0], s[1]);
                let ab = b - a;
                let l2 = ab.len2().max(1e-6);
                let x0 = ((a.x.min(b.x) - reach) / CELL).floor().max(0.0) as usize;
                let x1 = (((a.x.max(b.x) + reach) / CELL).ceil() as usize).min(n.saturating_sub(1));
                let y0 = ((a.y.min(b.y) - reach) / CELL).floor().max(0.0) as usize;
                let y1 = (((a.y.max(b.y) + reach) / CELL).ceil() as usize).min(ny.saturating_sub(1));
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let p = v2((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL);
                        let t = ((p - a).dot(ab) / l2).clamp(0.0, 1.0);
                        if p.dist(a + ab * t) <= reach {
                            out[y * n + x] = true;
                        }
                    }
                }
            }
        }
        out
    }

    /// Removes the trees and rocks that stand on the roads `from..` (crown or body touching one).
    pub fn clear_features_on_roads(&mut self, kinds: &[crate::data::FeatureDef], from: usize) {
        if from >= self.roads.len() {
            return;
        }
        let radius: Vec<f32> = self.feature_kinds.iter().map(|id| kinds.iter().find(|k| k.id == *id).map_or(1.0, |k| k.radius())).collect();
        let roads = MapData { roads: self.roads[from..].to_vec(), ..Default::default() };
        self.features.retain(|f| roads.road_gap(f.pos) > radius.get(f.kind as usize).copied().unwrap_or(1.0) * f.scale * 0.8);
    }

    /// Removes the roads with a point or segment within `r` of `p`. Returns whether any went.
    pub fn erase_roads(&mut self, p: V2, r: f32) -> bool {
        let before = self.roads.len();
        self.roads.retain(|road| {
            !road.points.windows(2).any(|s| {
                let ab = s[1] - s[0];
                let t = ((p - s[0]).dot(ab) / ab.len2().max(1e-6)).clamp(0.0, 1.0);
                p.dist(s[0] + ab * t) < r + road.reach()
            }) && !road.points.iter().any(|q| q.dist(p) < r)
        });
        self.roads.len() != before
    }
}

/// A gently winding line from `a` to `b`: a point every ~10 units, pushed sideways by noise,
/// with the ends where they were. Uses only `+ - * /` and libm-free noise, so it is the same on
/// every machine (not that it matters: roads are visual).
pub fn winding(a: V2, b: V2, wobble: f32, seed: u64) -> Vec<V2> {
    let d = a.dist(b);
    let steps = ((d / 10.0).ceil() as usize).max(1);
    let side = (b - a).perp().norm();
    (0..=steps)
        .map(|i| {
            let t = i as f32 / steps as f32;
            let w = (noise(t * d / 40.0, 3.7, seed) - 0.5) * 2.0 * wobble * (t * (1.0 - t) * 4.0).min(1.0);
            a + (b - a) * t + side * w
        })
        .collect()
}

/// Road line format in map.json: "kind width x z x z ...", the width "road+sidewalk" (e.g. "4.00+1.20")
/// when it has sidewalks.
pub fn road_line(r: &Road) -> String {
    let mut s = if r.sidewalk > 0.0 { format!("{} {:.2}+{:.2}", r.kind, r.width, r.sidewalk) } else { format!("{} {:.2}", r.kind, r.width) };
    for p in &r.points {
        s += &format!(" {:.2} {:.2}", p.x, p.y);
    }
    s
}

pub fn parse_road(line: &str) -> Result<Road, String> {
    let mut it = line.split_whitespace();
    let kind = it.next().unwrap_or("").to_string();
    let w = it.next().unwrap_or("");
    let (w, walk) = w.split_once('+').unwrap_or((w, "0"));
    let mut v: Vec<f32> = vec![w.parse().unwrap_or(f32::NAN)];
    v.extend(it.filter_map(|x| x.parse::<f32>().ok()));
    let walk: f32 = walk.parse().unwrap_or(f32::NAN);
    if kind.is_empty() || v.len() < 5 || v.len() % 2 == 0 || !v.iter().all(|x| x.is_finite()) || !walk.is_finite() || walk < 0.0 {
        return Err(format!("map.json: bad road line '{line}'"));
    }
    Ok(Road { kind, width: v[0], points: v[1..].chunks(2).map(|c| v2(c[0], c[1])).collect(), sidewalk: walk })
}
