//! Math helpers, the heightmap terrain, map generation, saving/loading and grid pathfinding.
use bevy::math::{Vec2, Vec3};
use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::collections::BinaryHeap;
use std::ops::{Add, Mul, Neg, Sub};
use std::path::Path;

/// Serializable 2D vector on the ground plane (x, z).
#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct V2 {
    pub x: f32,
    pub y: f32,
}
pub fn v2(x: f32, y: f32) -> V2 {
    V2 { x, y }
}
impl Add for V2 {
    type Output = V2;
    fn add(self, o: V2) -> V2 {
        v2(self.x + o.x, self.y + o.y)
    }
}
impl Sub for V2 {
    type Output = V2;
    fn sub(self, o: V2) -> V2 {
        v2(self.x - o.x, self.y - o.y)
    }
}
impl Mul<f32> for V2 {
    type Output = V2;
    fn mul(self, k: f32) -> V2 {
        v2(self.x * k, self.y * k)
    }
}
impl Neg for V2 {
    type Output = V2;
    fn neg(self) -> V2 {
        v2(-self.x, -self.y)
    }
}
impl V2 {
    pub fn len(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }
    pub fn len2(self) -> f32 {
        self.x * self.x + self.y * self.y
    }
    pub fn dist(self, o: V2) -> f32 {
        (self - o).len()
    }
    pub fn norm(self) -> V2 {
        let l = self.len();
        if l > 1e-5 {
            self * (1.0 / l)
        } else {
            v2(0.0, 0.0)
        }
    }
    pub fn dot(self, o: V2) -> f32 {
        self.x * o.x + self.y * o.y
    }
    pub fn perp(self) -> V2 {
        v2(-self.y, self.x)
    }
    pub fn from_angle(a: f32) -> V2 {
        v2(fm::cos(a), fm::sin(a))
    }
    pub fn angle(self) -> f32 {
        fm::atan2(self.y, self.x)
    }
    pub fn lerp(self, o: V2, t: f32) -> V2 {
        self + (o - self) * t
    }
    /// To a 3D point at height `h`.
    pub fn at(self, h: f32) -> Vec3 {
        Vec3::new(self.x, h, self.y)
    }
    pub fn vec(self) -> Vec2 {
        Vec2::new(self.x, self.y)
    }
}
pub fn flat(p: Vec3) -> V2 {
    v2(p.x, p.z)
}

/// Serializable 3D vector.
#[derive(Clone, Copy, PartialEq, Debug, Default, Serialize, Deserialize)]
pub struct V3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}
impl V3 {
    pub fn v(self) -> Vec3 {
        Vec3::new(self.x, self.y, self.z)
    }
}
impl From<Vec3> for V3 {
    fn from(p: Vec3) -> V3 {
        V3 { x: p.x, y: p.y, z: p.z }
    }
}

/// View-projection matrix of a perspective camera looking at `target` (used for picking and overlays).
pub fn view_proj(fov: f32, aspect: f32, eye: Vec3, target: Vec3) -> bevy::math::Mat4 {
    use bevy::math::{Mat4, Vec4};
    let (near, far) = (0.5, 6000.0);
    // Right-handed look-at.
    let f = (target - eye).normalize();
    let s = f.cross(Vec3::Y).normalize();
    let u = s.cross(f);
    let view = Mat4::from_cols(Vec4::new(s.x, u.x, -f.x, 0.0), Vec4::new(s.y, u.y, -f.y, 0.0), Vec4::new(s.z, u.z, -f.z, 0.0), Vec4::new(-s.dot(eye), -u.dot(eye), f.dot(eye), 1.0));
    // Right-handed perspective with depth 0..1.
    let h = 1.0 / (fov * 0.5).tan();
    let r = far / (near - far);
    let proj = Mat4::from_cols(Vec4::new(h / aspect, 0.0, 0.0, 0.0), Vec4::new(0.0, h, 0.0, 0.0), Vec4::new(0.0, 0.0, r, -1.0), Vec4::new(0.0, 0.0, r * near, 0.0));
    proj * view
}

/// Wrap an angle to (-PI, PI].
pub fn wrap(mut a: f32) -> f32 {
    use std::f32::consts::PI;
    if !a.is_finite() {
        return 0.0;
    }
    while a > PI {
        a -= 2.0 * PI;
    }
    while a < -PI {
        a += 2.0 * PI;
    }
    a
}
/// Rotate `from` toward `to` by at most `max` radians.
pub fn turn_toward(from: f32, to: f32, max: f32) -> f32 {
    let d = wrap(to - from);
    if d.abs() <= max {
        to
    } else {
        wrap(from + max * d.signum())
    }
}

/// Trigonometry for the simulation. The system maths library gives slightly different results on
/// Windows and Linux, which would make the same game diverge between machines; these are computed
/// in software and are identical everywhere.
pub mod fm {
    pub fn sin(x: f32) -> f32 {
        libm::sinf(x)
    }
    pub fn cos(x: f32) -> f32 {
        libm::cosf(x)
    }
    pub fn atan(x: f32) -> f32 {
        libm::atanf(x)
    }
    pub fn atan2(y: f32, x: f32) -> f32 {
        libm::atan2f(y, x)
    }
    pub fn acos(x: f32) -> f32 {
        libm::acosf(x)
    }
}

#[derive(Clone)]
pub struct Rng(pub u64);
impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    /// Uniform in [0, 1).
    pub fn f(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    /// Roughly normal distributed in about [-1, 1].
    pub fn bell(&mut self) -> f32 {
        (self.f() + self.f() + self.f()) * 2.0 / 3.0 - 1.0
    }
}

fn hash2(x: i32, y: i32, seed: u64) -> f32 {
    let mut h = (x as i64 as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ (y as i64 as u64).wrapping_mul(0xC2B2AE3D27D4EB4F) ^ seed;
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58476D1CE4E5B9);
    h ^= h >> 32;
    (h & 0xFFFFFF) as f32 / 16777216.0
}
/// Smooth value noise in [0, 1].
pub fn noise(x: f32, y: f32, seed: u64) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let a = hash2(xi, yi, seed);
    let b = hash2(xi + 1, yi, seed);
    let c = hash2(xi, yi + 1, seed);
    let d = hash2(xi + 1, yi + 1, seed);
    a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy
}
pub fn fbm(x: f32, y: f32, seed: u64) -> f32 {
    noise(x, y, seed) * 0.55 + noise(x * 2.1, y * 2.1, seed ^ 0x51) * 0.28 + noise(x * 4.3, y * 4.3, seed ^ 0xA7) * 0.17
}
fn smooth(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Size of one build/navigation grid cell in world units.
pub const CELL: f32 = 2.0;
/// Owner codes for map props.
pub const PROP_NEUTRAL: u8 = 0;
pub const PROP_HOSTILE: u8 = 1;

/// A pre-placed unit or building belonging to the neutral or the hostile side.
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub struct Prop {
    pub def: String,
    pub pos: V2,
    #[serde(default)]
    pub yaw: f32,
    #[serde(default)]
    pub owner: u8,
}

/// A reclaimable tree or rock. `kind` indexes `MapData::feature_kinds` (the feature type ids of
/// `rules.json: features`); `scale` only changes the look.
#[derive(Clone, Copy, Serialize, Deserialize, Debug, PartialEq)]
pub struct Feature {
    pub kind: u16,
    pub pos: V2,
    pub yaw: f32,
    pub scale: f32,
}

#[derive(Clone, Serialize, Deserialize, Default, Resource)]
pub struct MapData {
    pub name: String,
    /// Side length in cells; the heightmap has (n+1)^2 vertices.
    pub n: usize,
    pub heights: Vec<f32>,
    pub water: f32,
    pub deposits: Vec<V2>,
    pub starts: Vec<V2>,
    pub props: Vec<Prop>,
    /// Feature type ids used by `features`.
    pub feature_kinds: Vec<String>,
    /// Trees and rocks. Their index (plus `sim::FEATURE_ID0`) is their id in the game.
    pub features: Vec<Feature>,
    pub seed: u64,
    /// Terrain passability per cell (1 = blocked). Derived, see `rebuild_blocked`.
    #[serde(skip)]
    pub blocked: Vec<u8>,
}

#[derive(Serialize, Deserialize)]
struct MapFile {
    name: String,
    n: usize,
    water: f32,
    min_height: f32,
    max_height: f32,
    deposits: Vec<V2>,
    starts: Vec<V2>,
    #[serde(default)]
    props: Vec<Prop>,
    #[serde(default)]
    seed: u64,
    /// One line per tree or rock: "kind x z yaw scale".
    #[serde(default)]
    features: Vec<String>,
}

impl MapData {
    pub fn world(&self) -> f32 {
        self.n as f32 * CELL
    }
    pub fn center(&self) -> V2 {
        v2(self.world() * 0.5, self.world() * 0.5)
    }
    pub fn cell_of(&self, p: V2) -> (i32, i32) {
        ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
    }
    pub fn inside(&self, cx: i32, cy: i32) -> bool {
        cx >= 0 && cy >= 0 && (cx as usize) < self.n && (cy as usize) < self.n
    }
    pub fn clamp(&self, p: V2) -> V2 {
        let w = self.world() - 0.5;
        v2(p.x.clamp(0.5, w), p.y.clamp(0.5, w))
    }
    pub fn vertex(&self, x: usize, y: usize) -> f32 {
        self.heights[y.min(self.n) * (self.n + 1) + x.min(self.n)]
    }
    /// Bilinear terrain height at a world position.
    pub fn height(&self, x: f32, z: f32) -> f32 {
        let fx = (x / CELL).clamp(0.0, self.n as f32 - 0.001);
        let fz = (z / CELL).clamp(0.0, self.n as f32 - 0.001);
        let (ix, iz) = (fx as usize, fz as usize);
        let (tx, tz) = (fx - ix as f32, fz - iz as f32);
        let a = self.vertex(ix, iz);
        let b = self.vertex(ix + 1, iz);
        let c = self.vertex(ix, iz + 1);
        let d = self.vertex(ix + 1, iz + 1);
        a + (b - a) * tx + (c - a) * tz + (a - b - c + d) * tx * tz
    }
    pub fn h(&self, p: V2) -> f32 {
        self.height(p.x, p.y)
    }
    /// Height units stand on (never below the water surface for display purposes).
    pub fn normal(&self, x: f32, z: f32) -> Vec3 {
        let e = CELL * 0.5;
        let dx = self.height(x + e, z) - self.height(x - e, z);
        let dz = self.height(x, z + e) - self.height(x, z - e);
        Vec3::new(-dx, 2.0 * e, -dz).normalize()
    }

    /// Is this cell too steep or too deep under water to drive on?
    fn cell_blocked(&self, x: usize, y: usize) -> bool {
        let hs = [self.vertex(x, y), self.vertex(x + 1, y), self.vertex(x, y + 1), self.vertex(x + 1, y + 1)];
        let (mn, mx) = (hs.iter().cloned().fold(f32::MAX, f32::min), hs.iter().cloned().fold(f32::MIN, f32::max));
        let avg = (hs[0] + hs[1] + hs[2] + hs[3]) * 0.25;
        (mx - mn) / CELL > 0.85 || avg < self.water - 0.3
    }

    pub fn rebuild_blocked(&mut self) {
        let n = self.n;
        self.blocked = vec![0; n * n];
        for y in 0..n {
            for x in 0..n {
                if self.cell_blocked(x, y) {
                    self.blocked[y * n + x] = 1;
                }
            }
        }
    }

    /// Updates the passability of the (up to four) cells around vertex (x, y) after its height changed.
    pub fn reblock_around(&mut self, x: usize, y: usize) {
        let n = self.n;
        if n == 0 || self.blocked.len() != n * n {
            return;
        }
        for cy in y.saturating_sub(1)..=y.min(n - 1) {
            for cx in x.saturating_sub(1)..=x.min(n - 1) {
                self.blocked[cy * n + cx] = self.cell_blocked(cx, cy) as u8;
            }
        }
    }

    /// True if the straight line between two 3D points clears the terrain.
    pub fn los(&self, a: Vec3, b: Vec3) -> bool {
        let d = b - a;
        let len = Vec2::new(d.x, d.z).length();
        let steps = (len / CELL).ceil().max(1.0) as i32;
        for i in 1..steps {
            let p = a + d * (i as f32 / steps as f32);
            if self.height(p.x, p.z) > p.y {
                return false;
            }
        }
        true
    }

    /// Like `los`, but `block[cell]` (trees, rocks, wrecks: height above the ground) is added to
    /// the terrain. The two end cells never block, so a unit next to a tree still sees past its
    /// own cover in other directions and is not hidden by the tree it stands beside.
    pub fn los_blocked(&self, a: Vec3, b: Vec3, block: &[f32]) -> bool {
        let d = b - a;
        let len = Vec2::new(d.x, d.z).length();
        let steps = (len / CELL).ceil().max(1.0) as i32;
        let n = self.n as i32;
        let ends = [((a.x / CELL).floor() as i32, (a.z / CELL).floor() as i32), ((b.x / CELL).floor() as i32, (b.z / CELL).floor() as i32)];
        for i in 1..steps {
            let p = a + d * (i as f32 / steps as f32);
            let mut h = self.height(p.x, p.z);
            let (cx, cy) = ((p.x / CELL).floor() as i32, (p.z / CELL).floor() as i32);
            if cx >= 0 && cy >= 0 && cx < n && cy < n && !ends.contains(&(cx, cy)) {
                h += block.get((cy * n + cx) as usize).copied().unwrap_or(0.0);
            }
            if h > p.y {
                return false;
            }
        }
        true
    }

    /// First intersection of a ray with the terrain (or the water surface).
    pub fn raycast(&self, origin: Vec3, dir: Vec3) -> Option<Vec3> {
        let dir = dir.normalize_or_zero();
        if dir == Vec3::ZERO {
            return None;
        }
        let w = self.world();
        let ground = |p: Vec3| self.height(p.x, p.z).max(self.water);
        let mut t = 0.0;
        let mut prev = origin;
        // Skip quickly down to the highest possible terrain.
        if origin.y > 60.0 && dir.y < 0.0 {
            t = (origin.y - 60.0) / -dir.y;
            prev = origin + dir * t;
        }
        for _ in 0..4000 {
            let step = ((prev.y - ground(prev)) * 0.4).clamp(0.5, 40.0);
            t += step;
            let p = origin + dir * t;
            let inside = p.x >= 0.0 && p.z >= 0.0 && p.x <= w && p.z <= w;
            if inside && p.y <= ground(p) {
                let (mut lo, mut hi) = (prev, p);
                for _ in 0..16 {
                    let mid = (lo + hi) * 0.5;
                    if mid.y <= ground(mid) {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                return Some(hi);
            }
            if p.y < -50.0 {
                // Left the map: fall back to the horizontal plane.
                break;
            }
            prev = p;
        }
        if dir.y < -1e-4 {
            let p = origin + dir * (origin.y / -dir.y);
            return Some(Vec3::new(p.x.clamp(0.0, w), 0.0, p.z.clamp(0.0, w)));
        }
        None
    }

    /// Procedural map for 2-10 players with rotational symmetry (every start is equal).
    pub fn generate(n: usize, seed: u64, players: usize) -> MapData {
        let kk = players.clamp(2, 10);
        let step = std::f32::consts::TAU / kk as f32;
        let mut rng = Rng(seed.wrapping_mul(0x2545F4914F6CDD1D) | 1);
        for _ in 0..8 {
            rng.next();
        }
        let w = n as f32 * CELL;
        let c = v2(w * 0.5, w * 0.5);
        let rot = |p: V2, k: usize| -> V2 {
            let q = p - c;
            let (sn, cs) = (step * k as f32).sin_cos();
            v2(q.x * cs - q.y * sn, q.x * sn + q.y * cs) + c
        };
        // Averaging a noise field over the four rotations makes it symmetric and seamless.
        let sym = |p: V2, freq: f32, s: u64| -> f32 {
            let mut acc = 0.0;
            for k in 0..kk {
                let q = rot(p, k);
                acc += fbm(q.x * freq + 300.0, q.y * freq + 300.0, s);
            }
            ((acc / kk as f32 - 0.5) * 1.2 * (kk as f32).sqrt() + 0.5).clamp(0.0, 1.0)
        };
        // Two and four players sit in the corners, other counts on a circle.
        let start0 = if kk == 2 || kk == 4 { v2(w * 0.14, w * 0.14) } else { c + V2::from_angle(-2.356) * (w * 0.38) };
        let starts: Vec<V2> = (0..kk).map(|k| rot(start0, k)).collect();
        let seg_dist = |p: V2, a: V2, b: V2| -> f32 {
            let ab = b - a;
            let t = ((p - a).dot(ab) / ab.len2().max(1e-6)).clamp(0.0, 1.0);
            p.dist(a + ab * t)
        };
        let mut heights = vec![0.0f32; (n + 1) * (n + 1)];
        for y in 0..=n {
            for x in 0..=n {
                let p = v2(x as f32 * CELL, y as f32 * CELL);
                let elev = sym(p, 1.0 / 110.0, seed);
                let ridge = sym(p, 1.0 / 55.0, seed ^ 0x77AA);
                let detail = sym(p, 1.0 / 9.0, seed ^ 0x1234);
                let rolling = 2.6 + (elev - 0.42) * 15.0;
                let mut cliffs = smooth(0.66, 0.72, ridge) * 7.0 + (ridge - 0.66).max(0.0) * 14.0;
                // Open lanes: start <-> centre and start <-> neighbouring starts.
                let mut lane = f32::MAX;
                let mut base_d = f32::MAX;
                for k in 0..kk {
                    lane = lane.min(seg_dist(p, starts[k], c)).min(seg_dist(p, starts[k], starts[(k + 1) % kk]));
                    base_d = base_d.min(p.dist(starts[k]));
                }
                let open = smooth(22.0, 10.0, lane).max(smooth(48.0, 30.0, base_d)).max(smooth(30.0, 18.0, p.dist(c)));
                cliffs *= 1.0 - open;
                let mut h = rolling + cliffs + (detail - 0.5) * 0.7;
                // Lanes and bases stay above water; bases are levelled.
                h = h + (h.max(1.2) - h) * open;
                let flat_w = smooth(40.0, 22.0, base_d);
                h = h + (3.0 - h) * flat_w;
                // Map border falls away slightly so the edge reads as an edge.
                let edge = (p.x.min(p.y).min(w - p.x).min(w - p.y) / 10.0).clamp(0.0, 1.0);
                heights[y * (n + 1) + x] = h * (0.6 + 0.4 * edge);
            }
        }
        let mut m = MapData { name: format!("Random {seed}"), n, heights, water: 0.0, deposits: vec![], starts, props: vec![], feature_kinds: vec![], features: vec![], seed, blocked: vec![] };
        m.rebuild_blocked();
        // Deposits: four by each start, the rest scattered in one quadrant and rotated.
        let mut q: Vec<V2> = vec![];
        for (dx, dy) in [(7.0, 3.0), (-3.0, 7.0), (-7.0, -5.0), (5.0, -7.0)] {
            q.push(start0 + v2(dx, dy));
        }
        // Roughly the same total number of deposits whatever the player count.
        let want = ((7 + n / 48) * 4 / kk).max(3);
        let mut tries = 0;
        while q.len() < 4 + want && tries < 6000 {
            tries += 1;
            let p = v2(rng.range(10.0, w - 10.0), rng.range(10.0, w - 10.0));
            // Keep only points in the first player's sector; the rotations fill the rest.
            let rel = wrap((p - c).angle() - (start0 - c).angle());
            if rel.abs() > step * 0.5 {
                continue;
            }
            let (cx, cy) = m.cell_of(p);
            if m.blocked[cy as usize * n + cx as usize] != 0 || p.dist(start0) < 30.0 {
                continue;
            }
            if q.iter().any(|o| o.dist(p) < 18.0) {
                continue;
            }
            q.push(p);
        }
        q.push(c + v2(-9.0, -9.0));
        for k in 0..kk {
            for p in &q {
                let r = rot(*p, k);
                let (cx, cy) = m.cell_of(r);
                let cc = v2((cx as f32 + 0.5) * CELL, (cy as f32 + 0.5) * CELL);
                if m.inside(cx, cy) && m.blocked[cy as usize * n + cx as usize] == 0 && !m.deposits.iter().any(|o| o.dist(cc) < 5.0) {
                    m.deposits.push(cc);
                }
            }
        }
        m
    }

    /// Index of a feature type id in `feature_kinds`, added if new.
    pub fn feature_kind(&mut self, id: &str) -> u16 {
        match self.feature_kinds.iter().position(|k| k == id) {
            Some(k) => k as u16,
            None => {
                self.feature_kinds.push(id.to_string());
                (self.feature_kinds.len() - 1) as u16
            }
        }
    }

    /// Removes the features inside a square of `foot` cells centred on `pos` (under a building).
    pub fn clear_features_square(&mut self, pos: V2, foot: i32) {
        let half = foot as f32 * CELL * 0.5 + 0.3;
        self.features.retain(|f| (f.pos.x - pos.x).abs() > half || (f.pos.y - pos.y).abs() > half);
    }

    /// Scatters forests, lone trees and rock fields with the map's rotational symmetry (every start
    /// gets the same), keeping bases, deposits, buildings, water and cliffs clear. `amount` scales
    /// the number of trees and rocks (1 = normal). Existing features stay.
    pub fn scatter_features(&mut self, kinds: &[crate::data::FeatureDef], seed: u64, amount: f32) {
        use crate::data::FeatureClass;
        let pick_list = |class: FeatureClass| -> Vec<(usize, f32)> { kinds.iter().enumerate().filter(|(_, k)| k.class == class && k.weight > 0.0).map(|(i, k)| (i, k.weight)).collect() };
        let (trees, rocks) = (pick_list(FeatureClass::Tree), pick_list(FeatureClass::Rock));
        if self.n == 0 || (trees.is_empty() && rocks.is_empty()) {
            return;
        }
        if self.blocked.len() != self.n * self.n {
            self.rebuild_blocked();
        }
        let mut rng = Rng(seed.wrapping_mul(0x9E3779B97F4A7C15) ^ 0x7EE5 | 1);
        for _ in 0..8 {
            rng.next();
        }
        let w = self.world();
        let c = self.center();
        let kk = self.starts.len().max(1);
        let step = std::f32::consts::TAU / kk as f32;
        let start0 = self.starts.first().copied().unwrap_or(c);
        // `fm` trigonometry: the self-test map must come out the same on every platform.
        let rot = |p: V2, k: usize| -> V2 {
            let q = p - c;
            let (sn, cs) = (fm::sin(step * k as f32), fm::cos(step * k as f32));
            v2(q.x * cs - q.y * sn, q.x * sn + q.y * cs) + c
        };
        // Spatial hash of everything placed so far, 4 units per cell.
        const G: f32 = 4.0;
        let gn = (w / G).ceil() as usize + 1;
        let mut grid: Vec<Vec<(V2, f32)>> = vec![vec![]; gn * gn];
        let gcell = |p: V2| ((p.x / G).floor().clamp(0.0, gn as f32 - 1.0) as usize, (p.y / G).floor().clamp(0.0, gn as f32 - 1.0) as usize);
        for f in &self.features {
            let r = self.feature_kinds.get(f.kind as usize).and_then(|id| kinds.iter().find(|k| k.id == *id)).map_or(1.0, |k| k.radius() * f.scale);
            let (x, y) = gcell(f.pos);
            grid[y * gn + x].push((f.pos, r));
        }
        let fits = |grid: &Vec<Vec<(V2, f32)>>, p: V2, r: f32| -> bool {
            let (x, y) = gcell(p);
            for gy in y.saturating_sub(2)..(y + 3).min(gn) {
                for gx in x.saturating_sub(2)..(x + 3).min(gn) {
                    if grid[gy * gn + gx].iter().any(|(q, qr)| q.dist(p) < (r + qr) * 0.85) {
                        return false;
                    }
                }
            }
            true
        };
        let foots: Vec<(V2, f32)> = self.props.iter().map(|p| (p.pos, 6.0)).collect();
        let ok_spot = |m: &MapData, p: V2, r: f32| -> bool {
            if p.x < 3.0 || p.y < 3.0 || p.x > w - 3.0 || p.y > w - 3.0 {
                return false;
            }
            let (cx, cy) = m.cell_of(p);
            if !m.inside(cx, cy) || m.blocked[cy as usize * m.n + cx as usize] != 0 || m.h(p) < m.water + 0.4 {
                return false;
            }
            !m.starts.iter().any(|s| s.dist(p) < 26.0) && !m.deposits.iter().any(|d| d.dist(p) < 3.5 + r) && !foots.iter().any(|(q, fr)| q.dist(p) < fr + r)
        };
        let choose = |rng: &mut Rng, list: &[(usize, f32)]| -> usize {
            let total: f32 = list.iter().map(|l| l.1).sum();
            let mut t = rng.f() * total;
            for (i, wt) in list {
                if t < *wt {
                    return *i;
                }
                t -= wt;
            }
            list[list.len() - 1].0
        };
        let area = w * w;
        // (class list, attempts, noise scale, seed salt, threshold low/high, base chance)
        let passes: [(&[(usize, f32)], f32, f32, u64, f32, f32, f32); 2] = [(&trees, area / 4.0 * amount, 1.0 / 70.0, 0xF0E5, 0.54, 0.62, 0.01), (&rocks, area / 160.0 * amount, 1.0 / 40.0, 0x50C5, 0.58, 0.70, 0.04)];
        for (list, attempts, freq, salt, lo, hi, base) in passes {
            if list.is_empty() {
                continue;
            }
            // Points outside the first player's sector are skipped, so 1/kk of the attempts land
            // there, and each one that is kept is placed kk times.
            for _ in 0..attempts as usize {
                let p = v2(rng.range(3.0, w - 3.0), rng.range(3.0, w - 3.0));
                if kk > 1 && wrap((p - c).angle() - (start0 - c).angle()).abs() > step * 0.5 {
                    // Outside the first player's sector: its rotations cover that ground.
                    continue;
                }
                let dens = fbm(p.x * freq + 50.0, p.y * freq + 50.0, seed ^ salt);
                let chance = base + (1.0 - base) * smooth(lo, hi, dens);
                let roll = rng.f();
                let kind = choose(&mut rng, list);
                if roll > chance {
                    continue;
                }
                let k = &kinds[kind];
                let scale = rng.range(0.8, 1.2);
                let yaw = rng.range(0.0, std::f32::consts::TAU);
                let r = k.radius() * scale;
                let copies: Vec<V2> = (0..kk).map(|j| rot(p, j)).collect();
                let apart = kk == 1 || copies[0].dist(copies[1]) > r * 2.0;
                if !apart || !copies.iter().all(|q| ok_spot(self, *q, r) && fits(&grid, *q, r)) {
                    continue;
                }
                let ki = self.feature_kind(&k.id);
                for (j, q) in copies.into_iter().enumerate() {
                    let (x, y) = gcell(q);
                    grid[y * gn + x].push((q, r));
                    self.features.push(Feature { kind: ki, pos: q, yaw: wrap(yaw + step * j as f32), scale });
                }
            }
        }
    }

    pub fn save(&self, dir: &Path) -> Result<(), String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        let mn = self.heights.iter().cloned().fold(f32::MAX, f32::min);
        let mx = self.heights.iter().cloned().fold(f32::MIN, f32::max).max(mn + 0.01);
        let side = (self.n + 1) as u32;
        let px: Vec<u16> = self.heights.iter().map(|h| (((h - mn) / (mx - mn)) * 65535.0).round() as u16).collect();
        let img = image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_raw(side, side, px).ok_or("bad heightmap size")?;
        img.save(dir.join("height.png")).map_err(|e| e.to_string())?;
        let f = MapFile {
            name: self.name.clone(),
            n: self.n,
            water: self.water,
            min_height: mn,
            max_height: mx,
            deposits: self.deposits.clone(),
            starts: self.starts.clone(),
            props: self.props.clone(),
            seed: self.seed,
            features: self.features.iter().map(|f| format!("{} {:.2} {:.2} {:.2} {:.2}", self.feature_kinds.get(f.kind as usize).map_or("?", |k| k.as_str()), f.pos.x, f.pos.y, f.yaw, f.scale)).collect(),
        };
        let text = serde_json::to_string_pretty(&f).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("map.json"), text).map_err(|e| e.to_string())
    }

    pub fn load(dir: &Path) -> Result<MapData, String> {
        let text = std::fs::read_to_string(dir.join("map.json")).map_err(|e| format!("{}: {e}", dir.display()))?;
        let f: MapFile = serde_json::from_str(&text).map_err(|e| format!("map.json: {e}"))?;
        let img = image::open(dir.join("height.png")).map_err(|e| format!("height.png: {e}"))?.into_luma16();
        let side = f.n as u32 + 1;
        if img.width() != side || img.height() != side {
            return Err(format!("height.png must be {side}x{side} pixels for a map of {} cells", f.n));
        }
        let heights = img.as_raw().iter().map(|v| f.min_height + (*v as f32 / 65535.0) * (f.max_height - f.min_height)).collect();
        let mut m = MapData { name: f.name, n: f.n, heights, water: f.water, deposits: f.deposits, starts: f.starts, props: f.props, seed: f.seed, ..Default::default() };
        for line in &f.features {
            let mut it = line.split_whitespace();
            let kind = it.next().unwrap_or("");
            let v: Vec<f32> = it.filter_map(|x| x.parse().ok()).collect();
            if kind.is_empty() || v.len() < 2 || !v.iter().all(|x| x.is_finite()) {
                return Err(format!("map.json: bad feature line '{line}'"));
            }
            let k = m.feature_kind(kind);
            m.features.push(Feature { kind: k, pos: v2(v[0], v[1]), yaw: v.get(2).copied().unwrap_or(0.0), scale: v.get(3).copied().unwrap_or(1.0) });
        }
        m.rebuild_blocked();
        Ok(m)
    }

    /// Number of start positions of a saved map, without loading the heightmap.
    pub fn start_count(dir: &Path) -> usize {
        std::fs::read_to_string(dir.join("map.json")).ok().and_then(|t| serde_json::from_str::<MapFile>(&t).ok()).map_or(0, |f| f.starts.len())
    }

    /// Names of the maps in `assets/maps`.
    pub fn list(root: &Path) -> Vec<String> {
        let mut out: Vec<String> = std::fs::read_dir(root.join("maps"))
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.path().join("map.json").exists())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .collect()
            })
            .unwrap_or_default();
        out.sort();
        out
    }
}

/// Top-left cell of a square footprint of `f` cells centred on `p`.
pub fn foot_origin(p: V2, f: i32) -> (i32, i32) {
    let half = f as f32 * CELL * 0.5;
    (((p.x - half + 0.01) / CELL).floor() as i32, ((p.y - half + 0.01) / CELL).floor() as i32)
}

// ---------------------------------------------------------------------------- pathfinding

/// Reusable A* working memory.
#[derive(Default)]
pub struct PathScratch {
    g: Vec<f32>,
    from: Vec<u32>,
    stamp: Vec<u32>,
    gen: u32,
}

#[derive(PartialEq)]
struct Node(f32, u32);
impl Eq for Node {}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        o.0.partial_cmp(&self.0).unwrap_or(std::cmp::Ordering::Equal)
    }
}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

fn free(occ: &[u8], n: i32, x: i32, y: i32) -> bool {
    x >= 0 && y >= 0 && x < n && y < n && occ[(y * n + x) as usize] == 0
}

/// True if a straight walk from a to b stays on free cells.
pub fn line_clear(occ: &[u8], n: usize, a: V2, b: V2) -> bool {
    let d = b - a;
    let steps = (d.len() / (CELL * 0.4)).ceil().max(1.0) as i32;
    let side = d.norm().perp() * 0.5;
    for i in 0..=steps {
        let p = a + d * (i as f32 / steps as f32);
        for q in [p + side, p - side] {
            if !free(occ, n as i32, (q.x / CELL).floor() as i32, (q.y / CELL).floor() as i32) {
                return false;
            }
        }
    }
    true
}

/// Nearest free cell to (cx, cy), searching outward in rings.
pub fn nearest_free(occ: &[u8], n: usize, cx: i32, cy: i32) -> Option<(i32, i32)> {
    let ni = n as i32;
    if free(occ, ni, cx, cy) {
        return Some((cx, cy));
    }
    for r in 1..60i32 {
        let mut best: Option<(i32, i32, i32)> = None;
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() != r && dy.abs() != r {
                    continue;
                }
                if free(occ, ni, cx + dx, cy + dy) {
                    let d = dx * dx + dy * dy;
                    if best.map_or(true, |b| d < b.2) {
                        best = Some((cx + dx, cy + dy, d));
                    }
                }
            }
        }
        if let Some(b) = best {
            return Some((b.0, b.1));
        }
    }
    None
}

/// A* over the occupancy grid. Returns waypoints in reverse order (pop from the back),
/// ending at `to` or the nearest reachable point.
pub fn find_path(occ: &[u8], n: usize, from: V2, to: V2, sc: &mut PathScratch) -> Vec<V2> {
    let ni = n as i32;
    let cell = |p: V2| ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32);
    let (sx, sy) = cell(from);
    let (tx0, ty0) = cell(to);
    let Some((tx, ty)) = nearest_free(occ, n, tx0.clamp(0, ni - 1), ty0.clamp(0, ni - 1)) else {
        return vec![to];
    };
    let goal_pos = if (tx, ty) == (tx0, ty0) { to } else { v2((tx as f32 + 0.5) * CELL, (ty as f32 + 0.5) * CELL) };
    if line_clear(occ, n, from, goal_pos) {
        return vec![goal_pos];
    }
    if sc.g.len() != n * n {
        sc.g = vec![0.0; n * n];
        sc.from = vec![0; n * n];
        sc.stamp = vec![0; n * n];
        sc.gen = 0;
    }
    sc.gen += 1;
    let gen = sc.gen;
    let (sx, sy) = (sx.clamp(0, ni - 1), sy.clamp(0, ni - 1));
    let start = (sy * ni + sx) as u32;
    let goal = (ty * ni + tx) as u32;
    let h = |i: u32| -> f32 {
        let (x, y) = ((i as i32 % ni), (i as i32 / ni));
        let (dx, dy) = ((x - tx).abs() as f32, (y - ty).abs() as f32);
        (dx.max(dy) + 0.4142 * dx.min(dy)) * 1.02
    };
    let mut heap = BinaryHeap::new();
    sc.g[start as usize] = 0.0;
    sc.from[start as usize] = start;
    sc.stamp[start as usize] = gen;
    heap.push(Node(h(start), start));
    let mut best = (h(start), start);
    let mut found = false;
    let mut expanded = 0;
    while let Some(Node(f, i)) = heap.pop() {
        if i == goal {
            found = true;
            break;
        }
        let gi = sc.g[i as usize];
        if f > gi + h(i) + 0.001 {
            continue;
        }
        expanded += 1;
        if expanded > 60000 {
            break;
        }
        let hi = h(i);
        if hi < best.0 {
            best = (hi, i);
        }
        let (x, y) = (i as i32 % ni, i as i32 / ni);
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            let (nx, ny) = (x + dx, y + dy);
            if !free(occ, ni, nx, ny) {
                continue;
            }
            if dx != 0 && dy != 0 && (!free(occ, ni, x + dx, y) || !free(occ, ni, x, y + dy)) {
                continue;
            }
            let j = (ny * ni + nx) as usize;
            let ng = gi + if dx != 0 && dy != 0 { 1.4142 } else { 1.0 };
            if sc.stamp[j] != gen || ng < sc.g[j] {
                sc.stamp[j] = gen;
                sc.g[j] = ng;
                sc.from[j] = i;
                heap.push(Node(ng + h(j as u32), j as u32));
            }
        }
    }
    let end = if found { goal } else { best.1 };
    let center = |i: u32| v2(((i as i32 % ni) as f32 + 0.5) * CELL, ((i as i32 / ni) as f32 + 0.5) * CELL);
    let mut raw = vec![if found { goal_pos } else { center(end) }];
    let mut i = end;
    while i != start {
        i = sc.from[i as usize];
        raw.push(center(i));
    }
    raw.pop();
    raw.push(from);
    raw.reverse();
    // String pulling.
    let mut out = vec![];
    let mut a = 0;
    while a < raw.len() - 1 {
        let mut b = raw.len() - 1;
        while b > a + 1 && !line_clear(occ, n, raw[a], raw[b]) {
            b -= 1;
        }
        out.push(raw[b]);
        a = b;
    }
    out.reverse();
    if out.is_empty() {
        out.push(goal_pos);
    }
    out
}
