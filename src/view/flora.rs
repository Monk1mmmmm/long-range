//! Trees and rocks: procedural low-poly models, merged into one mesh per chunk of the map
//! (thousands of separate entities would be too much for small GPUs). A chunk's mesh is rebuilt
//! when one of its features is reclaimed or comes back, or when the ground under it changes.
//!
//! Culling: Bevy's frustum culling skips every chunk whose bounding box lies outside the camera's
//! view; the box is recomputed from the vertices whenever a chunk's mesh changes, so nothing on
//! screen is dropped. Chunks are kept small (`FLORA_CHUNK`) so that little off-screen geometry is
//! drawn along with the visible parts. (Only GPU occlusion culling is off in the compatible
//! renderer; frustum culling runs on the CPU either way.)
//!
//! Model detail is a setting (`Settings::flora_detail`): 0 = low (fewest triangles, no shadows),
//! 1 = medium, 2 = high.
//!
//! Level of detail: at medium and high every chunk has two meshes, one at the chosen detail and
//! one at low detail, and Bevy's `VisibilityRange` crossfades (dithered) from the first to the
//! second as the camera's distance to the chunk crosses `LOD_FADE`. Each chunk mesh is built
//! around the chunk's centre (its `Transform`), which is where that distance is measured from.
//!
//! Vertex UVs hold the feature's map position, so the fog-of-war texture (whose colour channels
//! carry the brightness, see `intel.rs`) darkens features in the fog like the terrain.
use super::terrain::GameScene;
use bevy::light::NotShadowCaster;
use crate::data::{Defs, FeatureDef, FeatureShape};
use crate::map::*;
use bevy::asset::RenderAssetUsages;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;

/// Model variants per feature type (picked per feature).
const VARIANTS: usize = 3;

/// Side of a feature mesh chunk in cells (32 world units): a quarter of a terrain chunk's side,
/// so frustum culling works in fine steps.
pub const FLORA_CHUNK: usize = 16;

/// Camera distance to a chunk's centre (world units) over which its detailed mesh fades into the
/// low-detail one. At the closest zoom chunks around the view centre are about 45-100 away.
pub const LOD_FADE: std::ops::Range<f32> = 170.0..210.0;

/// The mesh entity of the features in the chunk starting at cell (cx, cy).
#[derive(Component)]
pub struct FloraChunk {
    pub cx: usize,
    pub cy: usize,
    /// The low-detail mesh shown from `LOD_FADE` on (only when the detail setting is above low).
    pub far: bool,
}

pub type FloraChunks<'w, 's> = Query<'w, 's, (Entity, &'static FloraChunk, &'static Mesh3d, &'static mut Visibility)>;

#[derive(Default, Clone)]
struct Model {
    pos: Vec<Vec3>,
    nrm: Vec<Vec3>,
    col: Vec<[f32; 3]>,
}

/// What the view draws of the features.
#[derive(Resource, Default)]
pub struct FloraView {
    pub material: Handle<StandardMaterial>,
    /// Per map feature: drawn or not.
    pub shown: Vec<bool>,
    /// `sim::Features::version` the view last looked at.
    pub version: u64,
    /// Chunks (corner cell) waiting for a new mesh.
    pub dirty: Vec<(usize, usize)>,
    /// Every chunk needs a new mesh (the detail level changed).
    pub all_dirty: bool,
    /// Model detail the meshes are built with (0 low, 1 medium, 2 high).
    pub detail: u8,
    /// Models per feature type of `rules.features`.
    models: Vec<Vec<Model>>,
    /// The same at low detail, for distant chunks (empty when `detail` is already low).
    far_models: Vec<Vec<Model>>,
}

fn build_models(defs: &Defs, detail: u8) -> Vec<Vec<Model>> {
    defs.rules.features.iter().enumerate().map(|(k, d)| (0..VARIANTS).map(|v| build_model(d, (k * 31 + v * 7 + 1) as u64, detail)).collect()).collect()
}

impl FloraView {
    pub fn new(defs: &Defs, material: Handle<StandardMaterial>, shown: Vec<bool>, detail: u8) -> FloraView {
        let detail = detail.min(2);
        let far_models = if detail > 0 { build_models(defs, 0) } else { vec![] };
        FloraView { material, shown, models: build_models(defs, detail), far_models, detail, ..default() }
    }
    /// Mesh levels a chunk has: `false` the detailed one, `true` the distant one.
    fn levels(&self) -> &'static [bool] {
        if self.detail > 0 { &[false, true] } else { &[false] }
    }
    /// Switches the model detail; every chunk is rebuilt by the next `rebuild_flora`.
    pub fn set_detail(&mut self, defs: &Defs, detail: u8) {
        let detail = detail.min(2);
        if detail != self.detail {
            self.detail = detail;
            self.models = build_models(defs, detail);
            self.far_models = if detail > 0 { build_models(defs, 0) } else { vec![] };
            self.all_dirty = true;
        }
    }
    pub fn mark(&mut self, c: (usize, usize)) {
        if !self.dirty.contains(&c) {
            self.dirty.push(c);
        }
    }
    /// Marks the chunks within `r` of `p`.
    pub fn mark_area(&mut self, map: &MapData, p: V2, r: f32) {
        let size = FLORA_CHUNK as f32 * CELL;
        let (last, last_y) = (map.nx.saturating_sub(1) / FLORA_CHUNK, map.ny.saturating_sub(1) / FLORA_CHUNK);
        let (x0, x1) = (((p.x - r) / size).floor().max(0.0) as usize, (((p.x + r) / size).floor().max(0.0) as usize).min(last));
        let (y0, y1) = (((p.y - r) / size).floor().max(0.0) as usize, (((p.y + r) / size).floor().max(0.0) as usize).min(last_y));
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.mark((x * FLORA_CHUNK, y * FLORA_CHUNK));
            }
        }
    }
}

/// Corner cell of the chunk a position lies in.
pub fn chunk_of(map: &MapData, p: V2) -> (usize, usize) {
    let (cx, cy) = map.cell_of(p);
    let (last, last_y) = (map.nx.saturating_sub(1) as i32, map.ny.saturating_sub(1) as i32);
    ((cx.clamp(0, last) as usize / FLORA_CHUNK) * FLORA_CHUNK, (cy.clamp(0, last_y) as usize / FLORA_CHUNK) * FLORA_CHUNK)
}

/// The shown features of every chunk: chunk (x, y) is at index `y * side + x`. Returns (side, lists).
fn buckets(map: &MapData, view: &FloraView) -> (usize, Vec<Vec<u32>>) {
    let side = map.nx.div_ceil(FLORA_CHUNK).max(1);
    let mut out = vec![vec![]; side * map.ny.div_ceil(FLORA_CHUNK).max(1)];
    for (i, f) in map.features.iter().enumerate() {
        if view.shown.get(i).copied().unwrap_or(true) {
            let (cx, cy) = chunk_of(map, f.pos);
            out[(cy / FLORA_CHUNK) * side + cx / FLORA_CHUNK].push(i as u32);
        }
    }
    (side, out)
}

fn hash(i: u64, salt: u64) -> f32 {
    let mut h = i.wrapping_mul(0x9E3779B97F4A7C15) ^ salt.wrapping_mul(0xC2B2AE3D27D4EB4F);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58476D1CE4E5B9);
    h ^= h >> 29;
    (h & 0xFFFFFF) as f32 / 16777216.0
}

fn scale3(c: [f32; 3], k: f32) -> [f32; 3] {
    [c[0] * k, c[1] * k, c[2] * k]
}

impl Model {
    /// High detail: shade the model by where it is. Foliage is darker deep inside and near the ground
    /// (ambient occlusion), rocks darker at their foot with moss on their upward faces.
    fn weather(&mut self, shape: FeatureShape, size: f32, seed: u64) {
        for v in 0..self.pos.len() {
            let (p, n) = (self.pos[v], self.nrm[v]);
            let k = match shape {
                FeatureShape::Rock => {
                    let foot = 0.65 + 0.35 * ((p.y / (0.5 * size)).clamp(0.0, 1.0));
                    let moss = ((n.y - 0.55) / 0.45).clamp(0.0, 1.0) * (0.35 + 0.45 * hash(seed, 21));
                    let c = self.col[v];
                    let m = [0.18, 0.27, 0.09];
                    self.col[v] = [(c[0] + (m[0] - c[0]) * moss) * foot, (c[1] + (m[1] - c[1]) * moss) * foot, (c[2] + (m[2] - c[2]) * moss) * foot];
                    continue;
                }
                FeatureShape::Bush => 0.7 + 0.3 * (p.y / (0.6 * size)).clamp(0.0, 1.0),
                _ => 0.62 + 0.38 * ((p.y - 0.2 * size) / (2.6 * size)).clamp(0.0, 1.0),
            };
            // Faces turned down are in shade.
            let k = k * (0.85 + 0.15 * (n.y * 0.5 + 0.5));
            self.col[v] = scale3(self.col[v], k);
        }
    }
    /// One flat-shaded triangle, wound to face away from `inside`.
    fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3, inside: Vec3, col: [f32; 3]) {
        let mut n = (b - a).cross(c - a);
        let (b, c) = if n.dot((a + b + c) / 3.0 - inside) < 0.0 {
            n = -n;
            (c, b)
        } else {
            (b, c)
        };
        let n = n.normalize_or_zero();
        for p in [a, b, c] {
            self.pos.push(p);
            self.nrm.push(n);
            self.col.push(col);
        }
    }
    /// Open cone (no base) from a ring at `y0` to the tip at `y1`.
    fn cone(&mut self, y0: f32, y1: f32, r: f32, sides: usize, turn: f32, col: [f32; 3]) {
        let tip = Vec3::new(0.0, y1, 0.0);
        let inside = Vec3::new(0.0, y0 + (y1 - y0) * 0.25, 0.0);
        for i in 0..sides {
            let a0 = turn + i as f32 / sides as f32 * std::f32::consts::TAU;
            let a1 = turn + (i + 1) as f32 / sides as f32 * std::f32::consts::TAU;
            let p0 = Vec3::new(r * a0.cos(), y0, r * a0.sin());
            let p1 = Vec3::new(r * a1.cos(), y0, r * a1.sin());
            // A little shading towards the bottom edge.
            self.tri(p0, p1, tip, inside, scale3(col, 0.92 + 0.08 * (i % 2) as f32));
        }
    }
    /// Open prism (a trunk).
    fn prism(&mut self, y0: f32, y1: f32, r: f32, sides: usize, col: [f32; 3]) {
        let inside = Vec3::new(0.0, (y0 + y1) * 0.5, 0.0);
        for i in 0..sides {
            let a0 = i as f32 / sides as f32 * std::f32::consts::TAU;
            let a1 = (i + 1) as f32 / sides as f32 * std::f32::consts::TAU;
            let (c0, s0, c1, s1) = (r * a0.cos(), r * a0.sin(), r * a1.cos(), r * a1.sin());
            let (b0, b1, t0, t1) = (Vec3::new(c0, y0, s0), Vec3::new(c1, y0, s1), Vec3::new(c0, y1, s0), Vec3::new(c1, y1, s1));
            self.tri(b0, b1, t1, inside, col);
            self.tri(b0, t1, t0, inside, col);
        }
    }
    /// A lumpy ball: an icosahedron (subdivided `sub` times; `sub` < 0: an octahedron) pushed in
    /// and out by noise.
    #[allow(clippy::too_many_arguments)]
    fn blob(&mut self, center: Vec3, radii: Vec3, lump: f32, sub: i32, seed: u64, col: [f32; 3], speckle: f32) {
        let t = (1.0 + 5f32.sqrt()) * 0.5;
        let v = [
            Vec3::new(-1.0, t, 0.0),
            Vec3::new(1.0, t, 0.0),
            Vec3::new(-1.0, -t, 0.0),
            Vec3::new(1.0, -t, 0.0),
            Vec3::new(0.0, -1.0, t),
            Vec3::new(0.0, 1.0, t),
            Vec3::new(0.0, -1.0, -t),
            Vec3::new(0.0, 1.0, -t),
            Vec3::new(t, 0.0, -1.0),
            Vec3::new(t, 0.0, 1.0),
            Vec3::new(-t, 0.0, -1.0),
            Vec3::new(-t, 0.0, 1.0),
        ];
        let faces: [[usize; 3]; 20] = [
            [0, 11, 5],
            [0, 5, 1],
            [0, 1, 7],
            [0, 7, 10],
            [0, 10, 11],
            [1, 5, 9],
            [5, 11, 4],
            [11, 10, 2],
            [10, 7, 6],
            [7, 1, 8],
            [3, 9, 4],
            [3, 4, 2],
            [3, 2, 6],
            [3, 6, 8],
            [3, 8, 9],
            [4, 9, 5],
            [2, 4, 11],
            [6, 2, 10],
            [8, 6, 7],
            [9, 8, 1],
        ];
        let mut tris: Vec<[Vec3; 3]> = if sub < 0 {
            // Octahedron, turned a little so it does not read as a diamond.
            let q = Quat::from_rotation_y(0.4) * Quat::from_rotation_x(0.3);
            let o = [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::NEG_Y, Vec3::Z, Vec3::NEG_Z].map(|d| q * d);
            let mut t = vec![];
            for a in [0, 1] {
                for b in [2, 3] {
                    for c in [4, 5] {
                        t.push([o[a], o[b], o[c]]);
                    }
                }
            }
            t
        } else {
            faces.iter().map(|f| [v[f[0]].normalize(), v[f[1]].normalize(), v[f[2]].normalize()]).collect()
        };
        for _ in 0..sub.max(0) {
            let mut next = Vec::with_capacity(tris.len() * 4);
            for [a, b, c] in tris {
                let (ab, bc, ca) = ((a + b).normalize(), (b + c).normalize(), (c + a).normalize());
                next.extend_from_slice(&[[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
            }
            tris = next;
        }
        // The same direction always gets the same bump, so shared corners stay together.
        let s = (seed % 997) as f32 * 3.7;
        let shape = |d: Vec3| -> Vec3 {
            let k = 1.0 + lump * (noise(d.x * 1.7 + d.y * 0.9 + s, d.z * 1.7 - d.y * 1.3 + s, seed) - 0.5) * 2.0;
            // High subdivision gets a second, finer layer of crags.
            let fine = if sub >= 2 { 0.4 * lump * (noise(d.x * 5.1 + s, d.z * 5.1 - d.y * 4.3 + s, seed ^ 3) - 0.5) * 2.0 } else { 0.0 };
            center + d * radii * (k + fine)
        };
        for [a, b, c] in tris {
            let mid = (a + b + c).normalize();
            let shade = 1.0 + speckle * (noise(mid.x * 4.0 + s, mid.z * 4.0 + mid.y * 3.0, seed ^ 9) - 0.5) * 2.0;
            // Undersides are darker.
            let shade = shade * (0.82 + 0.18 * (mid.y * 0.5 + 0.5));
            self.tri(shape(a), shape(b), shape(c), center, scale3(col, shade));
        }
    }
}

/// The model of one variant of a feature type, in its own units (origin on the ground), at a
/// detail level (0 low, 1 medium, 2 high). Triangles per model, low / medium / high: pine 16 / 26 /
/// 86, broadleaf 14 / 28 / 480, shrub 8 / 20 / 240, rock 20 / 20-80 / 320-1300.
fn build_model(d: &FeatureDef, seed: u64, detail: u8) -> Model {
    let s = d.size;
    let j = |k: u64| 0.9 + 0.2 * hash(seed, k);
    let mut m = Model::default();
    let c = d.color;
    let big = s >= 1.5;
    match d.shape {
        FeatureShape::Pine => {
            let turn = hash(seed, 1) * 1.0;
            match detail {
                0 => {
                    m.prism(-0.3, 0.8 * s, 0.1 * s, 3, d.trunk);
                    m.cone(0.45 * s, 2.2 * s * j(2), 0.72 * s * j(3), 5, turn, c);
                    m.cone(1.5 * s, 3.1 * s * j(6), 0.45 * s * j(7), 5, turn + 0.6, scale3(c, 1.12));
                }
                1 => {
                    m.prism(-0.3, 0.7 * s, 0.09 * s, 4, d.trunk);
                    m.cone(0.45 * s, 1.75 * s * j(2), 0.75 * s * j(3), 6, turn, c);
                    m.cone(1.15 * s, 2.45 * s * j(4), 0.56 * s * j(5), 6, turn + 0.5, scale3(c, 1.08));
                    m.cone(1.85 * s, 3.1 * s * j(6), 0.36 * s * j(7), 6, turn + 1.0, scale3(c, 1.16));
                }
                _ => {
                    // Five tiers of boughs, each a 14-sided skirt, narrowing to the top.
                    m.prism(-0.3, 0.8 * s, 0.1 * s, 8, d.trunk);
                    for t in 0..5 {
                        let f = t as f32 / 4.0;
                        let y0 = (0.4 + 1.45 * f) * s;
                        let h = (1.3 - 0.25 * f) * s * j(2 + t);
                        let r = (0.82 - 0.5 * f) * s * j(8 + t);
                        m.cone(y0, y0 + h, r, 14, turn + 0.3 * t as f32, scale3(c, 1.0 + 0.04 * t as f32));
                    }
                }
            }
        }
        FeatureShape::Broadleaf => {
            let trunk = [3, 4, 6][detail.min(2) as usize];
            m.prism(-0.3, 1.4 * s, 0.11 * s, trunk, d.trunk);
            match detail {
                0 => m.blob(Vec3::new(0.0, 2.0 * s, 0.0), Vec3::new(1.0, 0.85, 1.0) * s * j(2), 0.2, -1, seed, c, 0.1),
                1 => m.blob(Vec3::new(0.0, 2.0 * s, 0.0), Vec3::new(1.0, 0.85, 1.0) * s * j(2), 0.28, 0, seed, c, 0.1),
                _ => {
                    // A crown of one big lumpy ball and two smaller lobes.
                    m.blob(Vec3::new(0.0, 1.95 * s, 0.0), Vec3::new(0.95, 0.8, 0.95) * s * j(2), 0.2, 2, seed, c, 0.08);
                    let a = hash(seed, 3) * std::f32::consts::TAU;
                    m.blob(Vec3::new(0.45 * a.cos(), 2.35, 0.45 * a.sin()) * s, Vec3::splat(0.6 * s * j(4)), 0.2, 1, seed ^ 5, scale3(c, 1.1), 0.08);
                    m.blob(Vec3::new(-0.4 * a.sin(), 1.7, 0.4 * a.cos()) * s, Vec3::splat(0.5 * s * j(5)), 0.2, 1, seed ^ 11, scale3(c, 0.94), 0.08);
                }
            }
        }
        FeatureShape::Bush => match detail {
            0 => m.blob(Vec3::new(0.0, 0.3 * s, 0.0), Vec3::new(0.65, 0.5, 0.65) * s * j(2), 0.2, -1, seed, c, 0.12),
            1 => m.blob(Vec3::new(0.0, 0.3 * s, 0.0), Vec3::new(0.65, 0.5, 0.65) * s * j(2), 0.3, 0, seed, c, 0.12),
            _ => {
                m.blob(Vec3::new(0.0, 0.3 * s, 0.0), Vec3::new(0.62, 0.48, 0.62) * s * j(2), 0.22, 1, seed, c, 0.1);
                let a = hash(seed, 3) * std::f32::consts::TAU;
                m.blob(Vec3::new(0.38 * a.cos(), 0.22, 0.38 * a.sin()) * s, Vec3::splat(0.38 * s), 0.2, 1, seed ^ 7, scale3(c, 1.08), 0.1);
                m.blob(Vec3::new(-0.3 * a.sin(), 0.2, 0.3 * a.cos()) * s, Vec3::splat(0.3 * s), 0.2, 1, seed ^ 9, scale3(c, 0.93), 0.1);
            }
        },
        FeatureShape::Rock => {
            let r = Vec3::new(0.62 * j(2), 0.42 * j(3), 0.5 * j(4)) * s;
            let sub = match detail {
                0 => 0,
                1 => big as i32,
                _ => 2 + big as i32,
            };
            m.blob(Vec3::new(0.0, r.y * 0.3, 0.0), r, 0.3, sub, seed, c, 0.12);
            if detail >= 2 {
                // A smaller boulder leaning against it.
                let a = hash(seed, 5) * std::f32::consts::TAU;
                m.blob(Vec3::new(0.55 * r.x * a.cos(), r.y * 0.15, 0.55 * r.z * a.sin()), r * 0.45, 0.3, 1, seed ^ 13, scale3(c, 0.92), 0.12);
            }
        }
    }
    if detail >= 2 {
        m.weather(d.shape, s, seed);
    }
    m
}

/// Centre of the chunk starting at cell (cx, cy), on the ground: its mesh's origin.
fn chunk_origin(map: &MapData, cx: usize, cy: usize) -> Vec3 {
    let half = FLORA_CHUNK as f32 * CELL * 0.5;
    let c = v2((cx as f32 * CELL + half).min(map.world_x()), (cy as f32 * CELL + half).min(map.world_z()));
    c.at(map.h(c))
}

/// The mesh of the given features (one chunk's) at a level (`far`: low detail), relative to the
/// chunk's centre, or `None` if there are none.
fn chunk_mesh(map: &MapData, kinds: &[Option<usize>], view: &FloraView, list: &[u32], origin: Vec3, far: bool) -> Option<Mesh> {
    let models = if far { &view.far_models } else { &view.models };
    let world = map.size();
    let (mut pos, mut nrm, mut col, mut uv) = (vec![], vec![], vec![], vec![]);
    for &i in list {
        let Some(f) = map.features.get(i as usize) else { continue };
        let Some(Some(k)) = kinds.get(f.kind as usize) else { continue };
        let Some(variants) = models.get(*k) else { continue };
        let model = &variants[(hash(i as u64, 11) * VARIANTS as f32) as usize % VARIANTS];
        let rot = Quat::from_rotation_y(f.yaw);
        let at = f.pos.at(map.h(f.pos)) - origin;
        let tint = 0.86 + 0.26 * hash(i as u64, 12);
        // High detail: trees differ in hue too (some yellower, some bluer).
        let hue = if view.detail >= 2 { 0.2 * (hash(i as u64, 13) - 0.5) } else { 0.0 };
        let tint3 = [tint * (1.0 + hue), tint, tint * (1.0 - hue)];
        let t = [f.pos.x / world.x.max(1.0), f.pos.y / world.y.max(1.0)];
        for v in 0..model.pos.len() {
            pos.push((at + rot * (model.pos[v] * f.scale)).to_array());
            nrm.push((rot * model.nrm[v]).to_array());
            let c = model.col[v];
            col.push([c[0] * tint3[0], c[1] * tint3[1], c[2] * tint3[2], 1.0]);
            uv.push(t);
        }
    }
    if pos.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    Some(mesh)
}

fn kinds_of(map: &MapData, defs: &Defs) -> Vec<Option<usize>> {
    map.feature_kinds.iter().map(|k| defs.feature_id(k).map(|i| i as usize)).collect()
}

/// The material features are drawn with; `fog` is the fog-of-war texture in a game.
pub fn flora_material(materials: &mut Assets<StandardMaterial>, fog: Option<Handle<Image>>) -> Handle<StandardMaterial> {
    materials.add(StandardMaterial { base_color: Color::WHITE, base_color_texture: fog, perceptual_roughness: 0.92, reflectance: 0.2, ..default() })
}

/// Spawns the mesh entities (one per level) of the chunk at cell (cx, cy), if it has features.
#[allow(clippy::too_many_arguments)]
fn spawn_chunk(commands: &mut Commands, map: &MapData, kinds: &[Option<usize>], view: &FloraView, list: &[u32], meshes: &mut Assets<Mesh>, cx: usize, cy: usize) {
    let origin = chunk_origin(map, cx, cy);
    let lod = view.levels().len() > 1;
    for &far in view.levels() {
        let Some(m) = chunk_mesh(map, kinds, view, list, origin, far) else { return };
        let mut e = commands.spawn((Mesh3d(meshes.add(m)), MeshMaterial3d(view.material.clone()), Transform::from_translation(origin), Visibility::Inherited, FloraChunk { cx, cy, far }, GameScene));
        if view.detail == 0 {
            e.insert(NotShadowCaster);
        }
        if lod {
            e.insert(if far {
                bevy::camera::visibility::VisibilityRange { start_margin: LOD_FADE, end_margin: f32::MAX..f32::MAX, use_aabb: false }
            } else {
                bevy::camera::visibility::VisibilityRange { start_margin: 0.0..0.0, end_margin: LOD_FADE, use_aabb: false }
            });
        }
    }
}

/// Spawns the chunk meshes of every chunk that has features.
pub fn spawn_flora(commands: &mut Commands, map: &MapData, defs: &Defs, view: &FloraView, meshes: &mut Assets<Mesh>) {
    let kinds = kinds_of(map, defs);
    let (side, lists) = buckets(map, view);
    for (k, list) in lists.iter().enumerate() {
        spawn_chunk(commands, map, &kinds, view, list, meshes, (k % side) * FLORA_CHUNK, (k / side) * FLORA_CHUNK);
    }
}

/// Rebuilds the meshes of the chunks in `view.dirty`, spawning chunk entities that did not
/// exist; after a detail change (`all_dirty`) every chunk entity is made anew.
pub fn rebuild_flora(commands: &mut Commands, map: &MapData, defs: &Defs, view: &mut FloraView, chunks: &mut FloraChunks, meshes: &mut Assets<Mesh>) {
    let all = std::mem::take(&mut view.all_dirty);
    let dirty = std::mem::take(&mut view.dirty);
    if dirty.is_empty() && !all {
        return;
    }
    if all {
        // The number of levels and the shadows change with the detail: start over.
        for (e, ..) in chunks.iter() {
            commands.entity(e).despawn();
        }
        spawn_flora(commands, map, defs, view, meshes);
        return;
    }
    let kinds = kinds_of(map, defs);
    let (side, lists) = buckets(map, view);
    let list_of = |cx: usize, cy: usize| lists.get((cy / FLORA_CHUNK) * side + cx / FLORA_CHUNK).map_or(&[][..], |l| &l[..]);
    let mut done = vec![false; lists.len()];
    for (_, c, mesh, mut vis) in chunks.iter_mut() {
        if !dirty.contains(&(c.cx, c.cy)) {
            continue;
        }
        if let Some(d) = done.get_mut((c.cy / FLORA_CHUNK) * side + c.cx / FLORA_CHUNK) {
            *d = true;
        }
        match chunk_mesh(map, &kinds, view, list_of(c.cx, c.cy), chunk_origin(map, c.cx, c.cy), c.far) {
            Some(m) => {
                // Changing the mesh asset makes Bevy recompute the chunk's bounding box, so
                // frustum culling keeps working with the new shape.
                if let Some(old) = meshes.get_mut(&mesh.0) {
                    *old.into_inner() = m;
                }
                if *vis != Visibility::Inherited {
                    *vis = Visibility::Inherited;
                }
            }
            None => {
                if *vis != Visibility::Hidden {
                    *vis = Visibility::Hidden;
                }
            }
        }
    }
    for (cx, cy) in dirty {
        if done.get((cy / FLORA_CHUNK) * side + cx / FLORA_CHUNK).copied().unwrap_or(true) {
            continue;
        }
        spawn_chunk(commands, map, &kinds, view, list_of(cx, cy), meshes, cx, cy);
    }
}

/// In a game: redraws chunks whose features were reclaimed (or came back after a rollback),
/// chunks whose ground changed (marked by `terrain_sync`), and everything when the detail
/// setting changes.
#[allow(clippy::too_many_arguments)]
pub fn flora_sync(
    mut commands: Commands,
    features: Option<Res<crate::sim::Features>>,
    map: Res<MapData>,
    defs: Res<Defs>,
    settings: Res<crate::settings::Settings>,
    view: Option<ResMut<FloraView>>,
    mut chunks: FloraChunks,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    let (Some(features), Some(mut view)) = (features, view) else { return };
    let view = &mut *view;
    view.set_detail(&defs, settings.flora_detail);
    if features.version != view.version {
        view.version = features.version;
        view.shown.resize(features.list.len(), false);
        for i in 0..features.list.len() {
            let alive = features.alive(i);
            if view.shown[i] != alive {
                view.shown[i] = alive;
                if let Some(f) = map.features.get(i) {
                    view.mark(chunk_of(&map, f.pos));
                }
            }
        }
    }
    rebuild_flora(&mut commands, &map, &defs, view, &mut chunks, &mut meshes);
}
