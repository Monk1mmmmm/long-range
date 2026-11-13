//! 2D vector type, map generation and grid pathfinding.
use serde::{Deserialize, Serialize};
use std::collections::BinaryHeap;
use std::ops::{Add, Mul, Neg, Sub};

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
        v2(a.cos(), a.sin())
    }
    pub fn angle(self) -> f32 {
        self.y.atan2(self.x)
    }
    pub fn lerp(self, o: V2, t: f32) -> V2 {
        self + (o - self) * t
    }
}

/// Wrap an angle to (-PI, PI].
pub fn wrap(mut a: f32) -> f32 {
    use std::f32::consts::PI;
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
}

pub const CELL: f32 = 4.0;
pub const T_GROUND: u8 = 0;
pub const T_ROCK: u8 = 1;
pub const T_WATER: u8 = 2;

#[derive(Clone, Serialize, Deserialize, Default)]
pub struct Map {
    /// Side length in cells.
    pub n: usize,
    pub terrain: Vec<u8>,
    pub deposits: Vec<V2>,
    pub starts: Vec<V2>,
    pub seed: u64,
}

fn hash2(x: i32, y: i32, seed: u64) -> f32 {
    let mut h = (x as u64).wrapping_mul(0x9E3779B97F4A7C15) ^ (y as u64).wrapping_mul(0xC2B2AE3D27D4EB4F) ^ seed;
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

impl Map {
    pub fn world(&self) -> f32 {
        self.n as f32 * CELL
    }
    pub fn cell_of(&self, p: V2) -> (i32, i32) {
        ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
    }
    pub fn inside(&self, cx: i32, cy: i32) -> bool {
        cx >= 0 && cy >= 0 && (cx as usize) < self.n && (cy as usize) < self.n
    }
    pub fn terrain_at(&self, cx: i32, cy: i32) -> u8 {
        if self.inside(cx, cy) {
            self.terrain[cy as usize * self.n + cx as usize]
        } else {
            T_ROCK
        }
    }
    pub fn clamp(&self, p: V2) -> V2 {
        let w = self.world() - 1.0;
        v2(p.x.clamp(1.0, w), p.y.clamp(1.0, w))
    }

    /// Generates a map with 4-fold rotational symmetry so every corner start is equal.
    pub fn generate(n: usize, seed: u64) -> Map {
        let mut rng = Rng(seed.wrapping_mul(0x2545F4914F6CDD1D) | 1);
        for _ in 0..8 {
            rng.next();
        }
        let ni = n as i32;
        let half = ni / 2;
        // Map any cell to its canonical cell in the top-left quadrant.
        let canon = |mut x: i32, mut y: i32| -> (i32, i32) {
            for _ in 0..4 {
                if x < half && y < half {
                    break;
                }
                let (nx, ny) = (y, ni - 1 - x);
                x = nx;
                y = ny;
            }
            (x, y)
        };
        let rot = |p: V2, k: usize| -> V2 {
            let w = n as f32 * CELL;
            let mut p = p;
            for _ in 0..k {
                p = v2(w - p.y, p.x);
            }
            p
        };
        let w = n as f32 * CELL;
        let start0 = v2(w * 0.14, w * 0.14);
        let center = v2(w * 0.5, w * 0.5);
        let mut terrain = vec![T_GROUND; n * n];
        for y in 0..ni {
            for x in 0..ni {
                let (cx, cy) = canon(x, y);
                let p = v2((cx as f32 + 0.5) * CELL, (cy as f32 + 0.5) * CELL);
                let r = noise(cx as f32 * 0.11, cy as f32 * 0.11, seed) * 0.65
                    + noise(cx as f32 * 0.31, cy as f32 * 0.31, seed ^ 77) * 0.35;
                let wtr = noise(cx as f32 * 0.07 + 40.0, cy as f32 * 0.07, seed ^ 991);
                let mut t = T_GROUND;
                if wtr > 0.76 {
                    t = T_WATER;
                }
                if r > 0.69 {
                    t = T_ROCK;
                }
                // Keep bases, the centre and the lanes between them open.
                if p.dist(start0) < 70.0 || p.dist(center) < 40.0 {
                    t = T_GROUND;
                }
                let d = center - start0;
                let tproj = ((p - start0).dot(d) / d.len2()).clamp(0.0, 1.0);
                if p.dist(start0 + d * tproj) < 12.0 {
                    t = T_GROUND;
                }
                // Lane along the map edge toward the neighbouring corner.
                if ((p.y - start0.y).abs() < 10.0 && p.x > start0.x) || ((p.x - start0.x).abs() < 10.0 && p.y > start0.y) {
                    t = T_GROUND;
                }
                terrain[(y * ni + x) as usize] = t;
            }
        }
        let mut m = Map { n, terrain, deposits: vec![], starts: vec![], seed };
        // Deposits in the canonical quadrant, then rotated.
        let mut q: Vec<V2> = vec![];
        for (dx, dy) in [(12.0, 4.0), (-4.0, 12.0), (-12.0, -8.0), (8.0, -12.0)] {
            q.push(start0 + v2(dx, dy));
        }
        let want = 6 + n / 40;
        let mut tries = 0;
        while q.len() < 4 + want && tries < 4000 {
            tries += 1;
            let p = v2(rng.range(16.0, w * 0.5 - 8.0), rng.range(16.0, w * 0.5 - 8.0));
            let (cx, cy) = m.cell_of(p);
            if m.terrain_at(cx, cy) != T_GROUND || p.dist(start0) < 40.0 {
                continue;
            }
            if q.iter().any(|o| o.dist(p) < 26.0) {
                continue;
            }
            q.push(p);
        }
        q.push(center + v2(-14.0, -14.0));
        for k in 0..4 {
            for p in &q {
                let r = rot(*p, k);
                let (cx, cy) = m.cell_of(r);
                let c = v2((cx as f32 + 0.5) * CELL, (cy as f32 + 0.5) * CELL);
                if m.inside(cx, cy) && !m.deposits.iter().any(|o: &V2| o.dist(c) < 8.0) {
                    m.terrain[cy as usize * n + cx as usize] = T_GROUND;
                    m.deposits.push(c);
                }
            }
            m.starts.push(rot(start0, k));
        }
        m
    }
}

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
    let side = d.norm().perp() * 1.2;
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

/// Finds the nearest free cell to (cx, cy), searching outward in rings.
pub fn nearest_free(occ: &[u8], n: usize, cx: i32, cy: i32) -> Option<(i32, i32)> {
    let ni = n as i32;
    if free(occ, ni, cx, cy) {
        return Some((cx, cy));
    }
    for r in 1..40i32 {
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
/// ending at `to` (or the nearest reachable point to it).
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
        dx.max(dy) + 0.4142 * dx.min(dy)
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
        if expanded > 40000 {
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
    // Raw cell path, goal first.
    let mut raw = vec![if found { goal_pos } else { center(end) }];
    let mut i = end;
    while i != start {
        i = sc.from[i as usize];
        raw.push(center(i));
    }
    raw.pop();
    raw.push(from);
    raw.reverse(); // now start..goal
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
