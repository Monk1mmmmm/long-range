//! Ultra detail (`Settings::flora_detail` 3) for trees, bushes and rocks: real branching wood and
//! single leaves, needle sprays and angular rocks, all made in code (no textures: the flora
//! material's colour texture is the fog of war, so cut-out leaf cards are not possible).
//!
//! - Broadleaf (oak, birch): a bent, flared trunk, a spiral of limbs that rise and then droop, twigs
//!   off them, and about 450 leaves in clusters at the twig ends. A leaf is a folded kite of two
//!   triangles; its normal leans towards the outside of the crown so the canopy shades like one
//!   soft mass from afar, and leaves deep inside the crown are darker.
//! - Conifers (pine, fir): a tapering trunk with whorls of drooping branches, each carrying sprays of
//!   needle kites, so the tree shows the layered boughs of the real thing.
//! - Bushes: stems and leaves in a mound. Rocks: sub-divided blobs cut by random planes (angular,
//!   with crags and moss), the big ones a finer mesh, with pebbles at their foot.
//!
//! The meshes are heavy (thousands of triangles), so `flora` only builds them near the camera and
//! draws them double-sided (`FloraView::material_ds`).
use super::{hash, scale3, Model};
use crate::data::{FeatureDef, FeatureShape};
use bevy::prelude::*;
use std::f32::consts::TAU;

/// A small deterministic random stream for one model.
struct Rng {
    seed: u64,
    n: u64,
}

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng { seed, n: 0 }
    }
    fn f(&mut self) -> f32 {
        self.n += 1;
        hash(self.seed, 1000 + self.n)
    }
    fn range(&mut self, a: f32, b: f32) -> f32 {
        a + (b - a) * self.f()
    }
    /// A random direction.
    fn dir(&mut self) -> Vec3 {
        let z = self.range(-1.0, 1.0);
        let a = self.f() * TAU;
        let r = (1.0 - z * z).sqrt();
        Vec3::new(r * a.cos(), z, r * a.sin())
    }
}

type Corner = (Vec3, Vec3, [f32; 3]);

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

impl Model {
    /// A triangle with the given vertex normals, wound to face the way they point.
    fn smooth_tri(&mut self, v: [Corner; 3]) {
        let g = (v[1].0 - v[0].0).cross(v[2].0 - v[0].0);
        let order = if g.dot(v[0].1 + v[1].1 + v[2].1) < 0.0 { [0, 2, 1] } else { [0, 1, 2] };
        for k in order {
            self.pos.push(v[k].0);
            self.nrm.push(v[k].1);
            self.col.push(v[k].2);
        }
    }
    /// A smooth tapered tube along `pts`, ring radii `rad`, colour from `c0` at the start to `c1` at the end.
    fn tube(&mut self, pts: &[Vec3], rad: &[f32], sides: usize, c0: [f32; 3], c1: [f32; 3]) {
        let n = pts.len();
        if n < 2 {
            return;
        }
        let mut rings: Vec<Vec<(Vec3, Vec3)>> = Vec::with_capacity(n);
        let mut u = Vec3::ZERO;
        for i in 0..n {
            let t = (pts[(i + 1).min(n - 1)] - pts[i.saturating_sub(1)]).normalize_or_zero();
            let t = if t == Vec3::ZERO { Vec3::Y } else { t };
            // Carry the frame along the curve so the rings do not twist.
            u = if i == 0 { t.any_orthonormal_vector() } else { (u - t * u.dot(t)).normalize_or_zero() };
            if u == Vec3::ZERO {
                u = t.any_orthonormal_vector();
            }
            let w = t.cross(u);
            rings.push(
                (0..sides)
                    .map(|k| {
                        let a = k as f32 / sides as f32 * TAU;
                        let d = u * a.cos() + w * a.sin();
                        (pts[i] + d * rad[i.min(rad.len() - 1)], d)
                    })
                    .collect(),
            );
        }
        for i in 0..n - 1 {
            let (ca, cb) = (mix(c0, c1, i as f32 / (n - 1) as f32), mix(c0, c1, (i + 1) as f32 / (n - 1) as f32));
            for j in 0..sides {
                let j2 = (j + 1) % sides;
                let (a, b, c, d) = (rings[i][j], rings[i][j2], rings[i + 1][j2], rings[i + 1][j]);
                self.smooth_tri([(a.0, a.1, ca), (b.0, b.1, ca), (c.0, c.1, cb)]);
                self.smooth_tri([(a.0, a.1, ca), (c.0, c.1, cb), (d.0, d.1, cb)]);
            }
        }
    }
    /// One leaf or needle clump: a folded kite from `base` along `dir`, its face turned towards `out`.
    fn leaf(&mut self, base: Vec3, dir: Vec3, len: f32, wid: f32, out: Vec3, col: [f32; 3]) {
        let dir = dir.normalize_or_zero();
        if dir == Vec3::ZERO {
            return;
        }
        let mut side = dir.cross(out);
        if side.length_squared() < 1e-4 {
            side = dir.any_orthonormal_vector();
        }
        let side = side.normalize();
        let mut face = side.cross(dir).normalize();
        if face.dot(out) < 0.0 {
            face = -face;
        }
        // The shading normal leans half way to the outside of the crown.
        let soft = (face + out.normalize_or_zero()).normalize_or_zero();
        let mid = base + dir * len * 0.45;
        let (l, r) = (mid + side * wid * 0.5 - face * wid * 0.1, mid - side * wid * 0.5 - face * wid * 0.1);
        let tip = base + dir * len + face * len * 0.06;
        let (cb, cm, ct) = (scale3(col, 0.72), scale3(col, 0.97), scale3(col, 1.12));
        let (nl, nr) = ((soft + side * 0.25).normalize(), (soft - side * 0.25).normalize());
        self.smooth_tri([(base, soft, cb), (l, nl, cm), (tip, soft, ct)]);
        self.smooth_tri([(base, soft, cb), (tip, soft, ct), (r, nr, cm)]);
    }
    /// `n` leaves scattered in a ball at `centre`. `crown` (centre) and `crown_r` (radius) of the whole
    /// tree: leaves nearer the middle of the crown are darker, those outside are turned outwards.
    #[allow(clippy::too_many_arguments)]
    fn leaves(&mut self, rng: &mut Rng, centre: Vec3, rad: f32, n: usize, crown: Vec3, crown_r: f32, size: f32, col: [f32; 3]) {
        for _ in 0..n {
            let p = centre + rng.dir() * rad * rng.f().cbrt();
            let off = p - crown;
            let out = if off.length_squared() > 1e-4 { off.normalize() } else { Vec3::Y };
            let dir = (rng.dir() * 0.7 + out * 0.9 + Vec3::NEG_Y * 0.1).normalize();
            let ao = 0.6 + 0.4 * (off.length() / crown_r).min(1.0);
            let c = scale3(col, ao * rng.range(0.82, 1.2));
            self.leaf(p, dir, size * rng.range(0.8, 1.25), size * rng.range(0.5, 0.65), (out + Vec3::Y * 0.35).normalize(), c);
        }
    }
}

/// The point at `t` (0..1) along a polyline.
fn along(pts: &[Vec3], t: f32) -> Vec3 {
    let x = t.clamp(0.0, 1.0) * (pts.len() - 1) as f32;
    let i = (x.floor() as usize).min(pts.len() - 2);
    pts[i].lerp(pts[i + 1], x - i as f32)
}

pub fn build(d: &FeatureDef, seed: u64) -> Model {
    let mut m = Model::default();
    let mut rng = Rng::new(seed);
    match d.shape {
        FeatureShape::Pine => conifer(&mut m, &mut rng, d),
        FeatureShape::Broadleaf => broadleaf(&mut m, &mut rng, d),
        FeatureShape::Bush => bush(&mut m, &mut rng, d),
        FeatureShape::Rock => rock(&mut m, &mut rng, d, seed),
    }
    if matches!(d.shape, FeatureShape::Rock) {
        m.weather(d.shape, d.size, seed);
    }
    m
}

fn broadleaf(m: &mut Model, rng: &mut Rng, d: &FeatureDef) {
    let s = d.size;
    let (c, bark) = (d.color, d.trunk);
    let dark_bark = scale3(bark, 0.7);
    let crown = Vec3::new(0.0, 2.1 * s, 0.0);
    let crown_r = 1.2 * s;
    // A trunk that leans and wanders a little, flared at the foot.
    let lean = Vec3::new(rng.range(-0.12, 0.12), 0.0, rng.range(-0.12, 0.12)) * s;
    let trunk: Vec<Vec3> = (0..7)
        .map(|i| {
            let t = i as f32 / 6.0;
            let wob = Vec3::new((t * 5.0 + rng.f()).sin(), 0.0, (t * 4.0 + rng.f()).cos()) * 0.03 * s;
            Vec3::new(0.0, (-0.3 + 2.3 * t) * s, 0.0) + lean * t * t + wob * t
        })
        .collect();
    let trunk_r: Vec<f32> = (0..7).map(|i| (0.11 - 0.065 * i as f32 / 6.0) * s * [1.6, 1.25, 1.0, 1.0, 1.0, 1.0, 1.0][i]).collect();
    m.tube(&trunk, &trunk_r, 8, dark_bark, bark);
    let limbs = 6 + (rng.f() * 3.0) as usize;
    for i in 0..limbs {
        let t0 = 0.5 + 0.42 * i as f32 / limbs as f32;
        let start = along(&trunk, t0);
        let az = i as f32 * 2.399 + rng.f() * 0.5;
        let h = Vec3::new(az.cos(), 0.0, az.sin());
        let l = s * rng.range(0.95, 1.35) * (1.15 - 0.4 * t0);
        // Rises, then droops under the weight of the leaves.
        let limb: Vec<Vec3> = (0..4)
            .map(|k| {
                let f = k as f32 / 3.0;
                start + h * (l * f * 0.85) + Vec3::Y * (l * (0.6 * f - 0.38 * f * f))
            })
            .collect();
        m.tube(&limb, &[0.05 * s, 0.038 * s, 0.027 * s, 0.015 * s], 5, dark_bark, bark);
        m.leaves(rng, limb[3], 0.36 * s, 24, crown, crown_r, 0.16 * s, c);
        for (k, turn) in [(2usize, 0.8f32), (3, -0.8), (1, -0.7)] {
            let p = limb[k];
            let (sn, cs) = turn.sin_cos();
            let hr = Vec3::new(h.x * cs - h.z * sn, 0.0, h.x * sn + h.z * cs);
            let tl = s * rng.range(0.4, 0.6);
            let twig: Vec<Vec3> = (0..3)
                .map(|j| {
                    let f = j as f32 / 2.0;
                    p + hr * (tl * f * 0.9) + Vec3::Y * (tl * (0.35 * f - 0.2 * f * f))
                })
                .collect();
            m.tube(&twig, &[0.022 * s, 0.014 * s, 0.007 * s], 3, bark, bark);
            m.leaves(rng, twig[2], 0.3 * s, 18, crown, crown_r, 0.15 * s, c);
            m.leaves(rng, twig[1], 0.2 * s, 8, crown, crown_r, 0.15 * s, c);
        }
    }
    // The leader and the top of the crown.
    let top = trunk[6] + Vec3::Y * 0.35 * s;
    m.tube(&[trunk[6], trunk[6].lerp(top, 0.5) + lean * 0.3, top], &[0.06 * s, 0.035 * s, 0.012 * s], 4, bark, bark);
    m.leaves(rng, top + Vec3::Y * 0.1 * s, 0.45 * s, 34, crown, crown_r, 0.16 * s, scale3(c, 1.06));
    // Fill the middle of the crown so it does not read as hollow from the side.
    m.leaves(rng, crown, 0.75 * s, 70, crown, crown_r, 0.16 * s, scale3(c, 0.95));
}

fn conifer(m: &mut Model, rng: &mut Rng, d: &FeatureDef) {
    let s = d.size;
    let (c, bark) = (d.color, d.trunk);
    let top = 3.6 * s;
    let trunk: Vec<Vec3> = (0..7)
        .map(|i| {
            let t = i as f32 / 6.0;
            Vec3::new((t * 6.0 + rng.f()).sin() * 0.02 * s * t, (-0.3 * s) + (top + 0.3 * s) * t, (t * 5.0).cos() * 0.015 * s * t)
        })
        .collect();
    let trunk_r: Vec<f32> = (0..7).map(|i| (0.13 * (1.0 - i as f32 / 6.0) + 0.008) * s * if i == 0 { 1.4 } else { 1.0 }).collect();
    m.tube(&trunk, &trunk_r, 7, scale3(bark, 0.8), bark);
    let tiers = 12;
    for t in 0..tiers {
        let f = t as f32 / (tiers - 1) as f32;
        let y = (0.55 + 2.75 * f) * s;
        let n = if t < 4 { 6 } else if t < 8 { 5 } else { 4 };
        let len = (0.95 - 0.8 * f.powf(0.85)) * s * rng.range(0.92, 1.08);
        let droop = 0.55 * (1.0 - f) + 0.08;
        let az0 = rng.f() * TAU;
        let tier_c = scale3(c, 0.92 + 0.12 * f);
        for b in 0..n {
            let az = az0 + (b as f32 + rng.range(-0.2, 0.2)) / n as f32 * TAU;
            let h = Vec3::new(az.cos(), 0.0, az.sin());
            let at = |fr: f32| Vec3::new(0.0, y, 0.0) + h * (len * fr) + Vec3::Y * (len * (0.16 * fr - droop * fr * fr));
            let pts = [at(0.0), at(0.5), at(1.0)];
            m.tube(&pts, &[0.032 * s * (1.0 - 0.5 * f), 0.02 * s, 0.008 * s], 3, bark, bark);
            let out_up = (Vec3::Y * 0.85 + h * 0.3).normalize();
            for fr in [0.25f32, 0.5, 0.75, 1.0] {
                let p = at(fr);
                let along = (h + Vec3::Y * (0.16 - 2.0 * droop * fr)).normalize();
                for yaw in [-0.55f32, 0.0, 0.55] {
                    let (sn, cs) = yaw.sin_cos();
                    let dir = Vec3::new(along.x * cs - along.z * sn, along.y, along.x * sn + along.z * cs);
                    let sc = rng.range(0.9, 1.15);
                    let c2 = scale3(tier_c, rng.range(0.85, 1.15));
                    m.leaf(p, dir, 0.42 * s * (1.0 - 0.45 * f) * sc, 0.24 * s * (1.0 - 0.3 * f), out_up, c2);
                }
            }
        }
    }
    // The leader's tuft.
    let tip = Vec3::new(0.0, top, 0.0);
    for k in 0..5 {
        let a = k as f32 / 5.0 * TAU;
        m.leaf(tip - Vec3::Y * 0.3 * s, Vec3::new(a.cos() * 0.3, 1.0, a.sin() * 0.3), 0.5 * s, 0.14 * s, Vec3::new(a.cos(), 0.6, a.sin()), scale3(c, 1.15));
    }
}

fn bush(m: &mut Model, rng: &mut Rng, d: &FeatureDef) {
    let s = d.size;
    let (c, bark) = (d.color, scale3(d.trunk, 0.8));
    let crown = Vec3::new(0.0, 0.3 * s, 0.0);
    for i in 0..7 {
        let a = i as f32 / 7.0 * TAU + rng.range(-0.2, 0.2);
        let h = Vec3::new(a.cos(), 0.0, a.sin());
        let l = s * rng.range(0.35, 0.55);
        let stem = [Vec3::new(0.0, -0.1 * s, 0.0), h * (l * 0.4) + Vec3::Y * 0.2 * s, h * (l * 0.9) + Vec3::Y * 0.35 * s];
        m.tube(&stem, &[0.02 * s, 0.014 * s, 0.007 * s], 3, bark, bark);
        m.leaves(rng, stem[2], 0.26 * s, 26, crown, 0.6 * s, 0.11 * s, c);
    }
    m.leaves(rng, crown, 0.45 * s, 80, crown, 0.6 * s, 0.11 * s, scale3(c, 0.95));
}

fn rock(m: &mut Model, rng: &mut Rng, d: &FeatureDef, seed: u64) {
    let s = d.size;
    let c = d.color;
    let big = s >= 1.5;
    let j = |r: &mut Rng| 0.9 + 0.2 * r.f();
    let r = Vec3::new(0.62 * j(rng), 0.42 * j(rng), 0.5 * j(rng)) * s;
    // Planes cut the ball into facets: a few steep ones round the sides, one flat-ish top.
    let mut planes: Vec<(Vec3, f32)> = vec![(Vec3::new(rng.range(-0.3, 0.3), 1.0, rng.range(-0.3, 0.3)).normalize(), rng.range(0.7, 0.85))];
    for _ in 0..(5 + big as usize * 2) {
        let n = Vec3::new(rng.range(-1.0, 1.0), rng.range(-0.3, 0.7), rng.range(-1.0, 1.0)).normalize();
        planes.push((n, rng.range(0.72, 0.92)));
    }
    let sub = 2 + big as i32;
    m.blob_cut(Vec3::new(0.0, r.y * 0.3, 0.0), r, 0.22, sub, seed, c, 0.14, &planes);
    // Pebbles and a smaller boulder at the foot.
    for k in 0..(2 + big as usize * 2) {
        let a = rng.f() * TAU;
        let dist = rng.range(1.05, 1.4);
        let scale = rng.range(0.12, 0.26);
        let pl: Vec<(Vec3, f32)> = (0..4).map(|_| (Vec3::new(rng.range(-1.0, 1.0), rng.range(-0.2, 0.9), rng.range(-1.0, 1.0)).normalize(), rng.range(0.7, 0.9))).collect();
        m.blob_cut(Vec3::new(a.cos() * r.x * dist, r.y * 0.02, a.sin() * r.z * dist), r * scale, 0.25, 1, seed ^ (31 + k as u64), scale3(c, rng.range(0.85, 1.05)), 0.12, &pl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every ultra model is finite and not empty; prints the triangle counts, and writes an OBJ per
    /// model into `LR_OBJ_DIR` when it is set (to look at them outside the game).
    #[test]
    fn ultra_models() {
        let defs = crate::data::Defs::load(&crate::data::asset_root()).unwrap();
        for d in &defs.rules.features {
            for v in 0..3u64 {
                let seed = (v * 7 + 1) + 31;
                let m = build(d, seed);
                assert!(!m.pos.is_empty() && m.pos.len() % 3 == 0, "{}", d.id);
                assert!(m.pos.iter().chain(&m.nrm).all(|p| p.is_finite()), "{} has a non-finite vertex", d.id);
                assert!(m.col.iter().all(|c| c.iter().all(|x| x.is_finite())));
                // Indexed: the same triangles, fewer vertices.
                let p = super::super::pack(&m);
                assert_eq!(p.idx.len(), m.pos.len());
                assert!(p.idx.iter().enumerate().all(|(i, &k)| p.pos[k as usize] == m.pos[i] && p.nrm[k as usize] == m.nrm[i]));
                if v == 0 {
                    println!("{:8} {:6} triangles, {:6} -> {:6} vertices indexed", d.id, m.pos.len() / 3, m.pos.len(), p.pos.len());
                }
                if let Ok(dir) = std::env::var("LR_OBJ_DIR") {
                    let mut o = String::new();
                    for (i, p) in m.pos.iter().enumerate() {
                        let c = m.col[i];
                        o += &format!("v {} {} {} {} {} {}\n", p.x, p.y, p.z, c[0], c[1], c[2]);
                    }
                    for t in 0..m.pos.len() / 3 {
                        o += &format!("f {} {} {}\n", t * 3 + 1, t * 3 + 2, t * 3 + 3);
                    }
                    std::fs::write(format!("{dir}/{}_{v}.obj", d.id), o).unwrap();
                }
            }
        }
    }
}
