//! Low-poly 3D drawing helpers: batched flat-shaded boxes, unit models, terrain.
use crate::defs::*;
use crate::map::*;
use macroquad::prelude::*;

pub const PLAYER_COLORS: [Color; 4] = [
    Color::new(0.25, 0.50, 1.00, 1.0),
    Color::new(0.95, 0.25, 0.20, 1.0),
    Color::new(0.20, 0.80, 0.45, 1.0),
    Color::new(0.95, 0.75, 0.15, 1.0),
];

pub fn shade(c: Color, k: f32) -> Color {
    Color::new((c.r * k).min(1.0), (c.g * k).min(1.0), (c.b * k).min(1.0), c.a)
}
pub fn mix(a: Color, b: Color, t: f32) -> Color {
    Color::new(a.r + (b.r - a.r) * t, a.g + (b.g - a.g) * t, a.b + (b.b - a.b) * t, a.a + (b.a - a.a) * t)
}

/// Accumulates triangles and submits them in chunks that fit a draw call.
pub struct Batch {
    pub m: Mesh,
}

const LIGHT: Vec3 = Vec3::new(0.38, 0.80, 0.46);

impl Batch {
    pub fn new() -> Batch {
        Batch { m: Mesh { vertices: Vec::with_capacity(32000), indices: Vec::with_capacity(48000), texture: None } }
    }
    pub fn flush(&mut self) {
        if !self.m.indices.is_empty() {
            draw_mesh(&self.m);
            self.m.vertices.clear();
            self.m.indices.clear();
        }
    }
    pub fn quad(&mut self, p: [Vec3; 4], c: Color) {
        if self.m.vertices.len() > 30000 || self.m.indices.len() > 45000 {
            self.flush();
        }
        let b = self.m.vertices.len() as u16;
        for v in p {
            self.m.vertices.push(Vertex::new2(v, Vec2::ZERO, c));
        }
        self.m.indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    /// A box given in the local frame of an object at `origin` rotated by `yaw` around Y.
    pub fn boxy(&mut self, origin: Vec3, yaw: f32, c: Vec3, h: Vec3, col: Color) {
        let (s, co) = yaw.sin_cos();
        let tf = |x: f32, y: f32, z: f32| -> Vec3 {
            let (lx, lz) = (c.x + x * h.x, c.z + z * h.z);
            vec3(origin.x + lx * co - lz * s, origin.y + c.y + y * h.y, origin.z + lx * s + lz * co)
        };
        let lit = |nx: f32, nz: f32| -> Color {
            let n = vec3(nx * co - nz * s, 0.0, nx * s + nz * co);
            shade(col, 0.55 + 0.4 * n.dot(LIGHT).max(0.0))
        };
        self.quad([tf(-1., 1., -1.), tf(1., 1., -1.), tf(1., 1., 1.), tf(-1., 1., 1.)], shade(col, 1.08));
        self.quad([tf(1., -1., -1.), tf(1., 1., -1.), tf(1., 1., 1.), tf(1., -1., 1.)], lit(1.0, 0.0));
        self.quad([tf(-1., -1., -1.), tf(-1., 1., -1.), tf(-1., 1., 1.), tf(-1., -1., 1.)], lit(-1.0, 0.0));
        self.quad([tf(-1., -1., 1.), tf(1., -1., 1.), tf(1., 1., 1.), tf(-1., 1., 1.)], lit(0.0, 1.0));
        self.quad([tf(-1., -1., -1.), tf(1., -1., -1.), tf(1., 1., -1.), tf(-1., 1., -1.)], lit(0.0, -1.0));
    }
}

/// Colour roles of model parts.
pub const C_TEAM: u8 = 0;
pub const C_DARK: u8 = 1;
pub const C_METAL: u8 = 2;
pub const C_LIGHT: u8 = 3;
pub const C_PANEL: u8 = 4;

#[derive(Clone, Copy)]
pub struct Part {
    pub c: Vec3,
    pub h: Vec3,
    pub col: u8,
    /// 0 = fixed to the hull, 1 = follows the turret aim, 2 = spins continuously.
    pub rot: u8,
}

fn p(c: (f32, f32, f32), h: (f32, f32, f32), col: u8, rot: u8) -> Part {
    Part { c: vec3(c.0, c.1, c.2), h: vec3(h.0, h.1, h.2), col, rot }
}

/// Builds the box model for a unit type. Local +X is forward, +Y is up.
pub fn model(d: &UnitDef) -> Vec<Part> {
    let r = if d.foot > 0 { d.foot as f32 * CELL * 0.5 * 0.92 } else { d.radius };
    let mut v = vec![];
    let tracks = |v: &mut Vec<Part>, len: f32| {
        v.push(p((0.0, 0.2 * r, 0.62 * r), (len * r, 0.2 * r, 0.16 * r), C_DARK, 0));
        v.push(p((0.0, 0.2 * r, -0.62 * r), (len * r, 0.2 * r, 0.16 * r), C_DARK, 0));
    };
    match d.shape {
        Shape::Tank => {
            tracks(&mut v, 1.05);
            v.push(p((0.0, 0.42 * r, 0.0), (1.0 * r, 0.2 * r, 0.5 * r), C_TEAM, 0));
            v.push(p((-0.1 * r, 0.78 * r, 0.0), (0.45 * r, 0.17 * r, 0.38 * r), C_LIGHT, 1));
            v.push(p((0.85 * r, 0.8 * r, 0.0), (0.7 * r, 0.06 * r, 0.06 * r), C_DARK, 1));
        }
        Shape::Spg => {
            tracks(&mut v, 1.05);
            v.push(p((0.0, 0.42 * r, 0.0), (1.05 * r, 0.2 * r, 0.52 * r), C_TEAM, 0));
            v.push(p((-0.45 * r, 0.85 * r, 0.0), (0.5 * r, 0.25 * r, 0.42 * r), C_LIGHT, 1));
            v.push(p((0.75 * r, 0.95 * r, 0.0), (1.0 * r, 0.07 * r, 0.07 * r), C_DARK, 1));
            v.push(p((1.7 * r, 0.95 * r, 0.0), (0.1 * r, 0.1 * r, 0.1 * r), C_METAL, 1));
        }
        Shape::Launcher => {
            tracks(&mut v, 1.0);
            v.push(p((0.0, 0.4 * r, 0.0), (1.0 * r, 0.18 * r, 0.5 * r), C_TEAM, 0));
            v.push(p((0.6 * r, 0.7 * r, 0.0), (0.3 * r, 0.14 * r, 0.4 * r), C_LIGHT, 0));
            v.push(p((-0.25 * r, 0.9 * r, 0.25 * r), (0.6 * r, 0.14 * r, 0.14 * r), C_METAL, 1));
            v.push(p((-0.25 * r, 0.9 * r, -0.25 * r), (0.6 * r, 0.14 * r, 0.14 * r), C_METAL, 1));
        }
        Shape::Flak => {
            tracks(&mut v, 1.0);
            v.push(p((0.0, 0.4 * r, 0.0), (1.0 * r, 0.18 * r, 0.5 * r), C_TEAM, 0));
            v.push(p((0.0, 0.8 * r, 0.0), (0.35 * r, 0.22 * r, 0.35 * r), C_LIGHT, 1));
            v.push(p((0.6 * r, 1.0 * r, 0.15 * r), (0.55 * r, 0.04 * r, 0.04 * r), C_DARK, 1));
            v.push(p((0.6 * r, 1.0 * r, -0.15 * r), (0.55 * r, 0.04 * r, 0.04 * r), C_DARK, 1));
        }
        Shape::Buggy => {
            for (x, z) in [(0.6, 0.6), (0.6, -0.6), (-0.6, 0.6), (-0.6, -0.6)] {
                v.push(p((x * r, 0.22 * r, z * r), (0.26 * r, 0.22 * r, 0.14 * r), C_DARK, 0));
            }
            v.push(p((0.0, 0.42 * r, 0.0), (1.0 * r, 0.14 * r, 0.42 * r), C_TEAM, 0));
            v.push(p((-0.2 * r, 0.7 * r, 0.0), (0.4 * r, 0.14 * r, 0.36 * r), C_LIGHT, 0));
            v.push(p((0.3 * r, 0.9 * r, 0.0), (0.4 * r, 0.04 * r, 0.04 * r), C_DARK, 1));
        }
        Shape::Bot => {
            v.push(p((0.0, 0.5 * r, 0.3 * r), (0.2 * r, 0.5 * r, 0.14 * r), C_DARK, 0));
            v.push(p((0.0, 0.5 * r, -0.3 * r), (0.2 * r, 0.5 * r, 0.14 * r), C_DARK, 0));
            v.push(p((0.0, 1.35 * r, 0.0), (0.32 * r, 0.38 * r, 0.5 * r), C_TEAM, 1));
            v.push(p((0.05 * r, 1.9 * r, 0.0), (0.18 * r, 0.16 * r, 0.18 * r), C_LIGHT, 1));
            v.push(p((0.5 * r, 1.3 * r, 0.6 * r), (0.5 * r, 0.07 * r, 0.07 * r), C_METAL, 1));
        }
        Shape::Commander => {
            v.push(p((0.0, 0.6 * r, 0.42 * r), (0.3 * r, 0.6 * r, 0.2 * r), C_DARK, 0));
            v.push(p((0.0, 0.6 * r, -0.42 * r), (0.3 * r, 0.6 * r, 0.2 * r), C_DARK, 0));
            v.push(p((0.0, 1.7 * r, 0.0), (0.45 * r, 0.55 * r, 0.7 * r), C_TEAM, 1));
            v.push(p((0.1 * r, 2.45 * r, 0.0), (0.25 * r, 0.2 * r, 0.25 * r), C_LIGHT, 1));
            v.push(p((0.4 * r, 1.7 * r, 0.95 * r), (0.7 * r, 0.14 * r, 0.14 * r), C_METAL, 1));
            v.push(p((0.2 * r, 1.7 * r, -0.95 * r), (0.4 * r, 0.16 * r, 0.16 * r), C_LIGHT, 1));
            v.push(p((-0.45 * r, 2.2 * r, 0.0), (0.12 * r, 0.5 * r, 0.3 * r), C_METAL, 1));
        }
        Shape::Engineer => {
            tracks(&mut v, 0.95);
            v.push(p((0.0, 0.45 * r, 0.0), (0.95 * r, 0.2 * r, 0.5 * r), C_TEAM, 0));
            v.push(p((-0.4 * r, 0.9 * r, 0.0), (0.35 * r, 0.25 * r, 0.4 * r), C_LIGHT, 0));
            v.push(p((0.5 * r, 1.1 * r, 0.0), (0.6 * r, 0.07 * r, 0.07 * r), C_PANEL, 0));
            v.push(p((1.05 * r, 0.85 * r, 0.0), (0.08 * r, 0.3 * r, 0.08 * r), C_METAL, 0));
        }
        Shape::RadarCar => {
            tracks(&mut v, 0.95);
            v.push(p((0.0, 0.42 * r, 0.0), (0.95 * r, 0.18 * r, 0.5 * r), C_TEAM, 0));
            v.push(p((0.0, 1.0 * r, 0.0), (0.07 * r, 0.45 * r, 0.07 * r), C_METAL, 0));
            v.push(p((0.0, 1.6 * r, 0.0), (0.08 * r, 0.3 * r, 0.7 * r), C_LIGHT, 2));
        }
        Shape::Jet => {
            v.push(p((0.0, 0.0, 0.0), (1.2 * r, 0.14 * r, 0.2 * r), C_TEAM, 0));
            v.push(p((-0.15 * r, 0.0, 0.0), (0.35 * r, 0.05 * r, 1.1 * r), C_LIGHT, 0));
            v.push(p((-1.0 * r, 0.05 * r, 0.0), (0.2 * r, 0.04 * r, 0.5 * r), C_LIGHT, 0));
            v.push(p((-1.0 * r, 0.3 * r, 0.0), (0.2 * r, 0.25 * r, 0.04 * r), C_DARK, 0));
            v.push(p((0.6 * r, 0.16 * r, 0.0), (0.25 * r, 0.08 * r, 0.12 * r), C_PANEL, 0));
        }
        Shape::Gunship => {
            v.push(p((0.0, 0.0, 0.0), (0.9 * r, 0.22 * r, 0.32 * r), C_TEAM, 0));
            v.push(p((0.0, 0.2 * r, 0.8 * r), (0.5 * r, 0.05 * r, 0.5 * r), C_DARK, 0));
            v.push(p((0.0, 0.2 * r, -0.8 * r), (0.5 * r, 0.05 * r, 0.5 * r), C_DARK, 0));
            v.push(p((-1.1 * r, 0.1 * r, 0.0), (0.4 * r, 0.07 * r, 0.07 * r), C_LIGHT, 0));
            v.push(p((0.6 * r, -0.25 * r, 0.0), (0.4 * r, 0.05 * r, 0.05 * r), C_METAL, 1));
        }
        Shape::Factory => {
            v.push(p((0.0, 0.15, 0.0), (r, 0.15, r), C_DARK, 0));
            v.push(p((-0.1 * r, 1.6, 0.8 * r), (0.85 * r, 1.5, 0.14 * r), C_TEAM, 0));
            v.push(p((-0.1 * r, 1.6, -0.8 * r), (0.85 * r, 1.5, 0.14 * r), C_TEAM, 0));
            v.push(p((-0.85 * r, 2.0, 0.0), (0.12 * r, 1.9, 0.7 * r), C_LIGHT, 0));
            v.push(p((-0.3 * r, 3.3, 0.0), (0.45 * r, 0.2, 0.9 * r), C_METAL, 0));
            v.push(p((0.55 * r, 3.5, 0.75 * r), (0.1 * r, 0.5, 0.1 * r), C_PANEL, 0));
        }
        Shape::AirFactory => {
            v.push(p((0.0, 0.15, 0.0), (r, 0.15, r), C_DARK, 0));
            v.push(p((0.15 * r, 0.4, 0.0), (0.7 * r, 0.12, 0.7 * r), C_METAL, 0));
            v.push(p((-0.75 * r, 2.5, -0.7 * r), (0.2 * r, 2.4, 0.2 * r), C_TEAM, 0));
            v.push(p((-0.75 * r, 5.1, -0.7 * r), (0.3 * r, 0.3, 0.3 * r), C_PANEL, 0));
            v.push(p((-0.75 * r, 1.0, 0.3 * r), (0.2 * r, 0.9, 0.6 * r), C_LIGHT, 0));
        }
        Shape::Pump => {
            let tall = if d.prod_m > 3.0 { 1.5 } else { 1.0 };
            v.push(p((0.0, 0.2, 0.0), (r, 0.2, r), C_DARK, 0));
            v.push(p((0.0, 0.9 * tall, 0.0), (0.45 * r, 0.7 * tall, 0.45 * r), C_TEAM, 0));
            v.push(p((0.0, 1.8 * tall, 0.0), (0.2 * r, 0.25, 0.2 * r), C_METAL, 0));
            v.push(p((0.0, 1.3 * tall, 0.0), (0.85 * r, 0.1, 0.1 * r), C_LIGHT, 2));
        }
        Shape::Windmill => {
            v.push(p((0.0, 0.15, 0.0), (0.6 * r, 0.15, 0.6 * r), C_DARK, 0));
            v.push(p((0.0, 2.2, 0.0), (0.12 * r, 2.2, 0.12 * r), C_METAL, 0));
            v.push(p((0.0, 4.3, 0.0), (1.1 * r, 0.06, 0.14 * r), C_TEAM, 2));
            v.push(p((0.0, 4.3, 0.0), (0.14 * r, 0.06, 1.1 * r), C_TEAM, 2));
        }
        Shape::Solar => {
            v.push(p((0.0, 0.2, 0.0), (r, 0.2, r), C_DARK, 0));
            for (x, z) in [(0.5, 0.5), (0.5, -0.5), (-0.5, 0.5), (-0.5, -0.5)] {
                v.push(p((x * r, 0.55, z * r), (0.42 * r, 0.1, 0.42 * r), C_PANEL, 0));
            }
            v.push(p((0.0, 0.8, 0.0), (0.12 * r, 0.5, 0.12 * r), C_TEAM, 0));
        }
        Shape::Storage => {
            v.push(p((0.0, 0.15, 0.0), (r, 0.15, r), C_DARK, 0));
            let col = if d.store_e > 0.0 { C_PANEL } else { C_METAL };
            v.push(p((0.0, 0.9, 0.45 * r), (0.8 * r, 0.6, 0.35 * r), col, 0));
            v.push(p((0.0, 0.9, -0.45 * r), (0.8 * r, 0.6, 0.35 * r), col, 0));
            v.push(p((0.0, 1.6, 0.0), (0.2 * r, 0.15, 0.9 * r), C_TEAM, 0));
        }
        Shape::Turret => {
            let long = (d.max_range() / 90.0).clamp(0.6, 1.6);
            v.push(p((0.0, 0.4, 0.0), (0.85 * r, 0.4, 0.85 * r), C_DARK, 0));
            v.push(p((0.0, 1.2, 0.0), (0.5 * r, 0.4, 0.5 * r), C_TEAM, 1));
            if d.weapons.first().map_or(false, |w| w.air) {
                v.push(p((0.7 * r, 1.5, 0.2 * r), (0.6 * r, 0.06, 0.06 * r), C_METAL, 1));
                v.push(p((0.7 * r, 1.5, -0.2 * r), (0.6 * r, 0.06, 0.06 * r), C_METAL, 1));
            } else {
                v.push(p((0.8 * r * long, 1.3, 0.0), (0.8 * r * long, 0.1, 0.1 * r), C_METAL, 1));
            }
        }
        Shape::RadarTower => {
            v.push(p((0.0, 0.15, 0.0), (0.7 * r, 0.15, 0.7 * r), C_DARK, 0));
            v.push(p((0.0, 2.0, 0.0), (0.14 * r, 2.0, 0.14 * r), C_TEAM, 0));
            v.push(p((0.0, 4.4, 0.0), (0.1 * r, 0.5, 1.0 * r), C_LIGHT, 2));
        }
    }
    v
}

pub fn part_color(role: u8, team: Color) -> Color {
    match role {
        C_TEAM => team,
        C_DARK => Color::new(0.16, 0.16, 0.18, 1.0),
        C_METAL => Color::new(0.52, 0.54, 0.58, 1.0),
        C_LIGHT => mix(team, Color::new(0.85, 0.85, 0.85, 1.0), 0.55),
        _ => Color::new(0.10, 0.16, 0.36, 1.0),
    }
}

/// Bakes the terrain into a texture (`px` pixels per cell).
pub fn ground_image(map: &Map, px: usize) -> Image {
    let n = map.n;
    let size = n * px;
    let mut img = Image::gen_image_color(size as u16, size as u16, BLACK);
    let seed = map.seed;
    for y in 0..size {
        for x in 0..size {
            let (wx, wy) = (x as f32 / px as f32, y as f32 / px as f32);
            let t = map.terrain_at(wx as i32, wy as i32);
            let n1 = noise(wx * 0.35, wy * 0.35, seed ^ 5);
            let n2 = noise(wx * 1.7, wy * 1.7, seed ^ 9);
            let n3 = noise(wx * 0.06, wy * 0.06, seed ^ 21);
            let c = match t {
                T_WATER => {
                    let k = 0.8 + 0.25 * n1;
                    Color::new(0.10 * k, 0.24 * k, 0.36 * k, 1.0)
                }
                T_ROCK => {
                    let k = 0.75 + 0.3 * n2;
                    Color::new(0.36 * k, 0.34 * k, 0.31 * k, 1.0)
                }
                _ => {
                    let k = 0.72 + 0.22 * n1 + 0.14 * n2;
                    let dry = n3;
                    Color::new((0.27 + 0.16 * dry) * k, (0.38 + 0.02 * dry) * k, (0.19 + 0.03 * dry) * k, 1.0)
                }
            };
            img.set_pixel(x as u32, y as u32, c);
        }
    }
    // Mass deposits.
    for d in &map.deposits {
        let (cx, cy) = (d.x / CELL * px as f32, d.y / CELL * px as f32);
        let r = px as f32 * 0.55;
        for y in (cy - r) as i32 - 1..=(cy + r) as i32 + 1 {
            for x in (cx - r) as i32 - 1..=(cx + r) as i32 + 1 {
                if x < 0 || y < 0 || x >= size as i32 || y >= size as i32 {
                    continue;
                }
                let dd = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt() / r;
                if dd < 1.0 {
                    let k = if dd < 0.45 { 0.85 } else { 0.55 };
                    img.set_pixel(x as u32, y as u32, Color::new(0.62 * k, 0.66 * k, 0.7 * k, 1.0));
                }
            }
        }
    }
    img
}

/// Rock formations as static mesh chunks: (centre, radius, mesh).
pub fn rock_chunks(map: &Map) -> Vec<(V2, f32, Mesh)> {
    const CH: usize = 16;
    let n = map.n;
    let mut out = vec![];
    for cy in (0..n).step_by(CH) {
        for cx in (0..n).step_by(CH) {
            let mut b = Batch::new();
            for y in cy..(cy + CH).min(n) {
                for x in cx..(cx + CH).min(n) {
                    if map.terrain[y * n + x] != T_ROCK {
                        continue;
                    }
                    let k = noise(x as f32 * 0.9, y as f32 * 0.9, map.seed ^ 3);
                    let h = 1.2 + 2.6 * noise(x as f32 * 0.3, y as f32 * 0.3, map.seed ^ 4) + k;
                    let g = 0.34 + 0.12 * k;
                    b.boxy(
                        vec3((x as f32 + 0.5) * CELL, 0.0, (y as f32 + 0.5) * CELL),
                        0.0,
                        vec3(0.0, h * 0.5, 0.0),
                        vec3(CELL * 0.5, h * 0.5, CELL * 0.5),
                        Color::new(g * 1.05, g, g * 0.92, 1.0),
                    );
                }
            }
            if !b.m.indices.is_empty() {
                let c = v2((cx + CH / 2) as f32 * CELL, (cy + CH / 2) as f32 * CELL);
                out.push((c, CH as f32 * CELL, b.m));
            }
        }
    }
    out
}
