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
//! Only the low-detail meshes exist for the whole map; the detailed ones are streamed in around the
//! camera (`stream_near`, `near_range`, nearest first, within `near_budget` vertices) and dropped when it
//! leaves (`near_drop`). At high detail all of them at once came to gigabytes on a big forest map and
//! lost the device (DeviceLost on a GT 730 / Vulkan). A chunk without a detailed mesh shows the
//! low-detail one at every distance (its `VisibilityRange` is only there while the other exists).
//!
//! Vertex UVs hold the feature's map position, so the fog-of-war texture (whose colour channels
//! carry the brightness, see `intel.rs`) darkens features in the fog like the terrain.
mod ultra;

use super::terrain::GameScene;
use bevy::light::NotShadowCaster;
use crate::data::{Defs, FeatureDef, FeatureShape};
use crate::map::*;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::VisibilityRange;
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

/// `LOD_FADE` at a detail level: ultra models are heavy, so they only show close to the camera.
pub fn lod_fade(detail: u8) -> std::ops::Range<f32> {
    if detail >= 3 { 110.0..140.0 } else { LOD_FADE }
}

/// A chunk gets its detailed mesh only while the camera is within this distance of its centre (a
/// little beyond where the crossfade to the low-detail mesh ends); further away only the cheap
/// low-detail mesh exists. At high detail the whole map's detailed meshes came to gigabytes of vertex
/// data (frankivsk: 20 million triangles), which lost the device on small GPUs.
fn near_range(detail: u8) -> f32 {
    lod_fade(detail).end + 30.0
}
/// A detailed mesh is dropped again beyond this distance (hysteresis, so a camera hovering at the
/// edge does not rebuild it over and over).
fn near_drop(detail: u8) -> f32 {
    near_range(detail) + 50.0
}
/// Most vertices all detailed chunk meshes together may hold (48 bytes each, about 190 MB).
fn near_budget(detail: u8) -> usize {
    if detail >= 3 { 10_000_000 } else { 4_000_000 }
}
/// Detailed chunks are streamed in every this many frames, at most `STREAM_PER_CALL` at a time.
const STREAM_EVERY: u32 = 2;
fn per_call(detail: u8) -> usize {
    if detail >= 3 { 1 } else { 4 }
}

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
    /// The same, double-sided: ultra detail's leaves and needles are single sheets.
    pub material_ds: Handle<StandardMaterial>,
    /// Per map feature: drawn or not.
    pub shown: Vec<bool>,
    /// `sim::Features::version` the view last looked at.
    pub version: u64,
    /// Chunks (corner cell) waiting for a new mesh.
    pub dirty: Vec<(usize, usize)>,
    /// Every chunk needs a new mesh (the detail level changed).
    pub all_dirty: bool,
    /// Model detail the meshes are built with (0 low, 1 medium, 2 high, 3 ultra).
    pub detail: u8,
    /// Where the camera is (set by whoever draws: the game or the editor); the detailed meshes are
    /// streamed in around it. `None`: only low-detail meshes are made.
    pub cam: Option<Vec3>,
    /// Chunks (corner cell) that have a detailed mesh, with its vertex count (only with LOD).
    near_verts: std::collections::HashMap<(usize, usize), usize>,
    /// Frame counter for `STREAM_EVERY`.
    frame: u32,
    /// Models per feature type of `rules.features`.
    models: Vec<Vec<Model>>,
    /// The same at low detail, for distant chunks (empty when `detail` is already low).
    far_models: Vec<Vec<Model>>,
    /// `models` / `far_models` blasted (`ruin`): trees stripped and charred (upright: `fallen_pose`
    /// lays them down), rocks broken into rubble.
    ruined: Vec<Vec<Model>>,
    far_ruined: Vec<Vec<Model>>,
    /// Per map feature: `sim::FeatureState::fall` as drawn (0 standing).
    pub fall: Vec<u8>,
    /// Trees in the air (`Flying`): not drawn in their chunk until they land.
    flying: Vec<u32>,
    /// Shape per feature type of `rules.features`.
    shapes: Vec<FeatureShape>,
}

fn ruin_models(defs: &Defs, models: &[Vec<Model>]) -> Vec<Vec<Model>> {
    models.iter().enumerate().map(|(k, vs)| vs.iter().enumerate().map(|(v, m)| ruin(m, defs.rules.features.get(k).map_or(FeatureShape::Pine, |d| d.shape), (k * 31 + v * 7 + 5) as u64)).collect()).collect()
}

/// A model after a blast hit it (`sim::Features::fell`). A tree: most of its foliage blown off, what
/// is left withered brown, the wood charred - still upright, `fallen_pose` lays it on the ground.
/// A rock: shattered - the boulder squashed into a low heap with broken pieces round it, darker.
fn ruin(m: &Model, shape: FeatureShape, seed: u64) -> Model {
    let mut out = Model::default();
    if shape == FeatureShape::Rock {
        let (lo, hi) = m.pos.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(*p), b.max(*p)));
        let size = (hi - lo).max_element().max(0.1);
        // The heap, then a ring of pieces (the same rock, small and turned) thrown round it.
        let mut place = |rot: Quat, scale: Vec3, at: Vec3, dark: f32| {
            for v in 0..m.pos.len() {
                out.pos.push(at + rot * (m.pos[v] * scale));
                out.nrm.push((rot * (m.nrm[v] / scale)).normalize_or_zero());
                out.col.push(scale3(m.col[v], dark));
            }
        };
        place(Quat::IDENTITY, Vec3::new(1.2, 0.36, 1.2), Vec3::new(0.0, -0.02 * size, 0.0), 0.72);
        for k in 0..5u64 {
            let a = (k as f32 + hash(seed, 60 + k)) / 5.0 * std::f32::consts::TAU;
            let d = size * (0.55 + 0.35 * hash(seed, 70 + k));
            let r = Quat::from_euler(EulerRot::YXZ, hash(seed, 80 + k) * 6.3, hash(seed, 90 + k) * 1.5, hash(seed, 95 + k) * 1.5);
            let k2 = 0.22 + 0.14 * hash(seed, 99 + k);
            place(r, Vec3::splat(k2), Vec3::new(a.cos() * d, -0.05 * size * k2, a.sin() * d), 0.62 + 0.15 * hash(seed, 50 + k));
        }
        return out;
    }
    let leafy = |c: [f32; 3]| c[1] > c[0] * 1.12 && c[1] > c[2] * 1.05;
    // Only finely made crowns keep some withered leaves: on a low-poly crown they would be big shards.
    let leaves = (0..m.pos.len() / 3).filter(|t| leafy(m.col[t * 3])).count();
    let hang = (leaves as f32 / 2000.0).min(0.22);
    for t in 0..m.pos.len() / 3 {
        let c = m.col[t * 3];
        let (keep, col) = if leafy(c) {
            let k = hash(seed ^ t as u64, 31);
            (k < hang, [0.17 + 0.08 * k, 0.12 + 0.03 * k, 0.05])
        } else {
            // Charred bark: much darker, a little brown.
            (true, [c[0] * 0.38 + 0.02, c[1] * 0.33 + 0.012, c[2] * 0.3 + 0.008])
        };
        if keep {
            for v in t * 3..t * 3 + 3 {
                out.pos.push(m.pos[v]);
                out.nrm.push(m.nrm[v]);
                out.col.push(col);
            }
        }
    }
    out
}

/// How a blasted feature lies, relative to its foot: (rotation, lift). Trees are laid along the
/// direction they fell (`sim::fall_yaw`), their trunk resting on the ground; rubble keeps the yaw.
fn fallen_pose(shape: FeatureShape, own_yaw: f32, fall: u8, size: f32) -> (Quat, f32) {
    if shape == FeatureShape::Rock {
        return (Quat::from_rotation_y(own_yaw), 0.0);
    }
    // Ry(-a) turns +x towards (cos a, sin a) on the map; Rz(-90 deg) lays the trunk (+y) along +x.
    let a = crate::sim::fall_yaw(fall);
    (Quat::from_rotation_y(-a) * Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2 + 0.06) * Quat::from_rotation_y(own_yaw), 0.1 * size)
}

fn build_models(defs: &Defs, detail: u8) -> Vec<Vec<Model>> {
    defs.rules.features.iter().enumerate().map(|(k, d)| (0..VARIANTS).map(|v| build_model(d, (k * 31 + v * 7 + 1) as u64, detail)).collect()).collect()
}

impl FloraView {
    pub fn new(defs: &Defs, materials: &mut Assets<StandardMaterial>, fog: Option<Handle<Image>>, shown: Vec<bool>, detail: u8) -> FloraView {
        let material = flora_material(materials, fog);
        let mut ds = materials.get(&material).cloned().unwrap_or_default();
        ds.double_sided = true;
        ds.cull_mode = None;
        let material_ds = materials.add(ds);
        let detail = detail.min(3);
        let far_models = if detail > 0 { build_models(defs, 0) } else { vec![] };
        let models = build_models(defs, detail);
        let (ruined, far_ruined) = (ruin_models(defs, &models), ruin_models(defs, &far_models));
        let shapes = defs.rules.features.iter().map(|d| d.shape).collect();
        FloraView { material, material_ds, shown, models, far_models, ruined, far_ruined, detail, shapes, ..default() }
    }
    /// Mesh levels a chunk has: `false` the detailed one, `true` the distant one.
    fn levels(&self) -> &'static [bool] {
        if self.detail > 0 { &[false, true] } else { &[false] }
    }
    /// Switches the model detail; every chunk is rebuilt by the next `rebuild_flora`.
    pub fn set_detail(&mut self, defs: &Defs, detail: u8) {
        let detail = detail.min(3);
        if detail != self.detail {
            self.detail = detail;
            self.models = build_models(defs, detail);
            self.far_models = if detail > 0 { build_models(defs, 0) } else { vec![] };
            self.ruined = ruin_models(defs, &self.models);
            self.far_ruined = ruin_models(defs, &self.far_models);
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
        if view.shown.get(i).copied().unwrap_or(true) && !view.flying.contains(&(i as u32)) {
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
        self.blob_cut(center, radii, lump, sub, seed, col, speckle, &[]);
    }
    /// `blob` cut by planes (unit normal, distance from the centre in unit-sphere terms): angular rocks.
    #[allow(clippy::too_many_arguments)]
    fn blob_cut(&mut self, center: Vec3, radii: Vec3, lump: f32, sub: i32, seed: u64, col: [f32; 3], speckle: f32, planes: &[(Vec3, f32)]) {
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
            let lim = planes.iter().filter_map(|(n, o)| (d.dot(*n) > 0.05).then(|| o / d.dot(*n))).fold(f32::MAX, f32::min);
            center + d * radii * (k + fine).min(lim)
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
    if detail >= 3 {
        return ultra::build(d, seed);
    }
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
    let world = map.size();
    let (mut pos, mut nrm, mut col, mut uv) = (vec![], vec![], vec![], vec![]);
    for &i in list {
        let Some(f) = map.features.get(i as usize) else { continue };
        let Some(Some(k)) = kinds.get(f.kind as usize) else { continue };
        let fall = view.fall.get(i as usize).copied().unwrap_or(0);
        let models = match (far, fall != 0) {
            (false, false) => &view.models,
            (true, false) => &view.far_models,
            (false, true) => &view.ruined,
            (true, true) => &view.far_ruined,
        };
        let Some(variants) = models.get(*k) else { continue };
        let model = &variants[(hash(i as u64, 11) * VARIANTS as f32) as usize % VARIANTS];
        let (rot, lift) = if fall != 0 { fallen_pose(view.shapes.get(*k).copied().unwrap_or(FeatureShape::Pine), f.yaw, fall, f.scale) } else { (Quat::from_rotation_y(f.yaw), 0.0) };
        let at = f.pos.at(map.h(f.pos) + lift) - origin;
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

/// The entity of one mesh of the chunk at cell (cx, cy). `range`: the crossfade between the two levels.
#[allow(clippy::too_many_arguments)]
fn chunk_entity(commands: &mut Commands, meshes: &mut Assets<Mesh>, view: &FloraView, mesh: Mesh, origin: Vec3, cx: usize, cy: usize, far: bool, range: Option<VisibilityRange>) -> Entity {
    let material = if !far && view.detail >= 3 { view.material_ds.clone() } else { view.material.clone() };
    let mut e = commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material), Transform::from_translation(origin), Visibility::Inherited, FloraChunk { cx, cy, far }, GameScene));
    if view.detail == 0 {
        e.insert(NotShadowCaster);
    }
    if let Some(r) = range {
        e.insert(r);
    }
    e.id()
}

/// Shown from the camera's distance `LOD_FADE` on, instead of the detailed mesh.
fn vis_far(detail: u8) -> VisibilityRange {
    VisibilityRange { start_margin: lod_fade(detail), end_margin: f32::MAX..f32::MAX, use_aabb: false }
}

/// Shown up to the camera's distance `LOD_FADE`.
fn vis_near(detail: u8) -> VisibilityRange {
    VisibilityRange { start_margin: 0.0..0.0, end_margin: lod_fade(detail), use_aabb: false }
}

/// Spawns the mesh entities of the chunk at cell (cx, cy), if it has features: with LOD the
/// low-detail one, and the detailed one only when `near` and the camera is close (within the vertex
/// budget). Returns the vertices of the detailed mesh made (0: none).
#[allow(clippy::too_many_arguments)]
fn spawn_chunk(commands: &mut Commands, map: &MapData, kinds: &[Option<usize>], view: &FloraView, list: &[u32], meshes: &mut Assets<Mesh>, cx: usize, cy: usize, near: bool) -> usize {
    let origin = chunk_origin(map, cx, cy);
    if view.levels().len() < 2 {
        if let Some(m) = chunk_mesh(map, kinds, view, list, origin, false) {
            chunk_entity(commands, meshes, view, m, origin, cx, cy, false, None);
        }
        return 0;
    }
    let Some(far_mesh) = chunk_mesh(map, kinds, view, list, origin, true) else { return 0 };
    let close = near && view.cam.is_some_and(|c| c.distance(origin) < near_range(view.detail)) && view.near_verts.values().sum::<usize>() < near_budget(view.detail);
    let near_mesh = if close { chunk_mesh(map, kinds, view, list, origin, false) } else { None };
    let verts = near_mesh.as_ref().map_or(0, |m| m.count_vertices());
    // Without a detailed mesh the low-detail one is shown at every distance.
    chunk_entity(commands, meshes, view, far_mesh, origin, cx, cy, true, near_mesh.is_some().then(|| vis_far(view.detail)));
    if let Some(m) = near_mesh {
        chunk_entity(commands, meshes, view, m, origin, cx, cy, false, Some(vis_near(view.detail)));
    }
    verts
}

/// Spawns the chunk meshes of every chunk that has features: the detailed ones of a level of
/// detail follow by `rebuild_flora` as the camera comes close.
pub fn spawn_flora(commands: &mut Commands, map: &MapData, defs: &Defs, view: &mut FloraView, meshes: &mut Assets<Mesh>) {
    let kinds = kinds_of(map, defs);
    let (side, lists) = buckets(map, view);
    view.near_verts.clear();
    for (k, list) in lists.iter().enumerate() {
        spawn_chunk(commands, map, &kinds, view, list, meshes, (k % side) * FLORA_CHUNK, (k / side) * FLORA_CHUNK, false);
    }
}

/// Rebuilds the meshes of the chunks in `view.dirty`, spawning chunk entities that did not
/// exist; after a detail change (`all_dirty`) every chunk entity is made anew. Then streams the
/// detailed meshes in and out around the camera (`view.cam`).
pub fn rebuild_flora(commands: &mut Commands, map: &MapData, defs: &Defs, view: &mut FloraView, chunks: &mut FloraChunks, meshes: &mut Assets<Mesh>) {
    let all = std::mem::take(&mut view.all_dirty);
    let dirty = std::mem::take(&mut view.dirty);
    if all {
        // The number of levels and the shadows change with the detail: start over.
        for (e, ..) in chunks.iter() {
            commands.entity(e).despawn();
        }
        spawn_flora(commands, map, defs, view, meshes);
        return;
    }
    if !dirty.is_empty() {
        rebuild_dirty(commands, map, defs, view, chunks, meshes, &dirty);
    }
    stream_near(commands, map, defs, view, chunks, meshes);
}

fn rebuild_dirty(commands: &mut Commands, map: &MapData, defs: &Defs, view: &mut FloraView, chunks: &mut FloraChunks, meshes: &mut Assets<Mesh>, dirty: &[(usize, usize)]) {
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
    for &(cx, cy) in dirty {
        if done.get((cy / FLORA_CHUNK) * side + cx / FLORA_CHUNK).copied().unwrap_or(true) {
            continue;
        }
        let verts = spawn_chunk(commands, map, &kinds, view, list_of(cx, cy), meshes, cx, cy, true);
        if verts > 0 {
            view.near_verts.insert((cx, cy), verts);
        }
    }
}

/// Level of detail: makes the detailed mesh of chunks the camera has come close to (nearest first,
/// a few at a time, within the vertex budget) and drops those it has left behind.
fn stream_near(commands: &mut Commands, map: &MapData, defs: &Defs, view: &mut FloraView, chunks: &mut FloraChunks, meshes: &mut Assets<Mesh>) {
    let Some(cam) = view.cam else { return };
    if view.levels().len() < 2 {
        return;
    }
    view.frame = view.frame.wrapping_add(1);
    if view.frame % STREAM_EVERY != 0 {
        return;
    }
    let mut far_of = std::collections::HashMap::new();
    let mut drop: Vec<(Entity, (usize, usize))> = vec![];
    for (e, c, ..) in chunks.iter() {
        let at = (c.cx, c.cy);
        if c.far {
            far_of.insert(at, e);
        } else if chunk_origin(map, c.cx, c.cy).distance(cam) > near_drop(view.detail) {
            drop.push((e, at));
        }
    }
    for (e, at) in drop {
        commands.entity(e).despawn();
        view.near_verts.remove(&at);
        if let Some(far) = far_of.get(&at) {
            // The low-detail mesh is now all there is: show it from every distance.
            commands.entity(*far).remove::<VisibilityRange>();
        }
    }
    let mut want: Vec<(f32, (usize, usize), Entity)> = far_of
        .iter()
        .filter(|(at, _)| !view.near_verts.contains_key(*at))
        .map(|(at, e)| (chunk_origin(map, at.0, at.1).distance(cam), *at, *e))
        .filter(|w| w.0 < near_range(view.detail))
        .collect();
    want.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    if want.is_empty() {
        return;
    }
    let kinds = kinds_of(map, defs);
    let (side, lists) = buckets(map, view);
    let mut total: usize = view.near_verts.values().sum();
    for (_, at, far) in want.into_iter().take(per_call(view.detail)) {
        if total >= near_budget(view.detail) {
            break;
        }
        let list = lists.get((at.1 / FLORA_CHUNK) * side + at.0 / FLORA_CHUNK).map_or(&[][..], |l| &l[..]);
        let origin = chunk_origin(map, at.0, at.1);
        let Some(m) = chunk_mesh(map, &kinds, view, list, origin, false) else { continue };
        let verts = m.count_vertices();
        total += verts;
        chunk_entity(commands, meshes, view, m, origin, at.0, at.1, false, Some(vis_near(view.detail)));
        commands.entity(far).insert(vis_far(view.detail));
        view.near_verts.insert(at, verts);
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
    cams: Query<&GlobalTransform, With<super::terrain::MainCamera>>,
    mut flights: Query<(Entity, &mut Flying, &mut Transform)>,
    time: Res<Time>,
) {
    let (Some(features), Some(mut view)) = (features, view) else { return };
    let view = &mut *view;
    view.cam = cams.iter().next().map(|g| g.translation());
    view.set_detail(&defs, settings.flora_detail);
    if features.version != view.version {
        view.version = features.version;
        view.shown.resize(features.list.len(), false);
        view.fall.resize(features.list.len(), 0);
        let kinds = kinds_of(&map, &defs);
        for i in 0..features.list.len() {
            let alive = features.alive(i);
            let fall = features.list[i].fall;
            let Some(f) = map.features.get(i) else { continue };
            if view.shown[i] != alive || view.fall[i] != fall {
                // Blown down just now (not a rollback or a restored state): it flies first.
                if alive && fall != 0 && view.fall[i] == 0 && view.shown[i] {
                    launch(&mut commands, &map, &kinds, view, &mut meshes, i, fall);
                }
                view.shown[i] = alive;
                view.fall[i] = fall;
                view.mark(chunk_of(&map, f.pos));
            }
        }
    }
    fly(&mut commands, &map, view, &mut flights, time.delta_secs());
    rebuild_flora(&mut commands, &map, &defs, view, &mut chunks, &mut meshes);
}

/// A tree thrown down by a blast on its way to the ground, or a piece of a shattered rock.
#[derive(Component)]
pub struct Flying {
    /// Map feature (a tree is drawn in its chunk again once it has landed).
    idx: u32,
    /// A tree (else a rock splinter, which lies a while, then sinks away).
    tree: bool,
    t: f32,
    secs: f32,
    from: Vec3,
    to: Vec3,
    hop: f32,
    r0: Quat,
    r1: Quat,
}

/// One model as its own mesh (vertices relative to its foot), fog UVs at `p`.
fn single_mesh(map: &MapData, model: &Model, p: V2, scale: f32, tint: f32) -> Mesh {
    let world = map.size();
    let t = [p.x / world.x.max(1.0), p.y / world.y.max(1.0)];
    let pos: Vec<[f32; 3]> = model.pos.iter().map(|v| (*v * scale).to_array()).collect();
    let nrm: Vec<[f32; 3]> = model.nrm.iter().map(|v| v.to_array()).collect();
    let col: Vec<[f32; 4]> = model.col.iter().map(|c| [c[0] * tint, c[1] * tint, c[2] * tint, 1.0]).collect();
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![t; pos.len()]);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh
}

/// Starts the flight of a feature a blast just threw down: a tree is flung up and topples over
/// onto where it will lie; a rock sends splinters flying (its rubble is drawn at once).
fn launch(commands: &mut Commands, map: &MapData, kinds: &[Option<usize>], view: &mut FloraView, meshes: &mut Assets<Mesh>, i: usize, fall: u8) {
    let Some(f) = map.features.get(i) else { return };
    let Some(Some(k)) = kinds.get(f.kind as usize) else { return };
    let shape = view.shapes.get(*k).copied().unwrap_or(FeatureShape::Pine);
    let seed = i as u64;
    let material = if view.detail >= 3 { view.material_ds.clone() } else { view.material.clone() };
    let foot = f.pos.at(map.h(f.pos));
    let a = crate::sim::fall_yaw(fall);
    let dir = Vec3::new(a.cos(), 0.0, a.sin());
    let variant = (hash(i as u64, 11) * VARIANTS as f32) as usize % VARIANTS;
    if shape == FeatureShape::Rock {
        let Some(model) = view.models.get(*k).and_then(|v| v.get(variant)) else { return };
        let size = 0.9 * f.scale;
        for n in 0..4u64 {
            let s = 0.2 + 0.12 * hash(seed, 200 + n);
            let mesh = meshes.add(single_mesh(map, model, f.pos, f.scale * s, 0.7));
            let side = Vec3::new(-dir.z, 0.0, dir.x) * (hash(seed, 210 + n) - 0.5) * 2.4;
            let out = (dir * (1.2 + 2.5 * hash(seed, 220 + n)) + side) * size;
            let land = foot.xz() + out.xz();
            let to = Vec3::new(land.x, map.height(land.x, land.y), land.y);
            let r1 = Quat::from_euler(EulerRot::YXZ, hash(seed, 230 + n) * 6.3, 2.0 + hash(seed, 240 + n) * 4.0, hash(seed, 250 + n) * 3.0);
            let fl = Flying { idx: i as u32, tree: false, t: 0.0, secs: 0.6 + 0.4 * hash(seed, 260 + n), from: foot + Vec3::Y * 0.4 * size, to, hop: (1.0 + 1.5 * hash(seed, 270 + n)) * size, r0: Quat::IDENTITY, r1 };
            commands.spawn((Mesh3d(mesh), MeshMaterial3d(material.clone()), Transform::from_translation(fl.from), Visibility::Inherited, NotShadowCaster, fl, super::terrain::GameScene));
        }
        return;
    }
    if shape == FeatureShape::Bush {
        return;
    }
    let Some(model) = view.ruined.get(*k).and_then(|v| v.get(variant)) else { return };
    let mesh = meshes.add(single_mesh(map, model, f.pos, f.scale, 0.86 + 0.26 * hash(i as u64, 12)));
    let (r1, lift) = fallen_pose(shape, f.yaw, fall, f.scale);
    // Upright at first (turned so that toppling about one axis ends in the pose it lies in).
    let start = Quat::from_rotation_y(-a) * Quat::from_rotation_y(f.yaw);
    let fl = Flying { idx: i as u32, tree: true, t: 0.0, secs: 0.9 + 0.3 * hash(seed, 300), from: foot - dir * 1.2 * f.scale, to: foot + Vec3::Y * lift, hop: (1.6 + 1.6 * hash(seed, 301)) * f.scale, r0: start, r1 };
    commands.spawn((Mesh3d(mesh), MeshMaterial3d(material), Transform::from_translation(fl.from).with_rotation(start), Visibility::Inherited, fl, super::terrain::GameScene));
    view.flying.push(i as u32);
}

/// Moves what is in the air; a tree that has landed goes back into its chunk's mesh.
fn fly(commands: &mut Commands, map: &MapData, view: &mut FloraView, q: &mut Query<(Entity, &mut Flying, &mut Transform)>, dt: f32) {
    for (e, mut f, mut tr) in q.iter_mut() {
        f.t += dt / f.secs.max(0.05);
        let t = f.t.min(1.0);
        if f.tree {
            if f.t >= 1.0 {
                commands.entity(e).despawn();
                view.flying.retain(|&i| i != f.idx);
                if let Some(ft) = map.features.get(f.idx as usize) {
                    view.mark(chunk_of(map, ft.pos));
                }
                continue;
            }
            // Flung up and out, toppling faster as it goes.
            tr.translation = f.from.lerp(f.to, t) + Vec3::Y * f.hop * 4.0 * t * (1.0 - t);
            tr.rotation = f.r0.slerp(f.r1, t * t * (3.0 - 2.0 * t) * 0.4 + t * t * 0.6);
        } else if f.t < 1.0 {
            tr.translation = f.from.lerp(f.to, t) + Vec3::Y * f.hop * 4.0 * t * (1.0 - t);
            tr.rotation = f.r0.slerp(f.r1, t);
        } else if f.t < 4.0 {
            // Lies a while, then sinks into the ground.
            tr.translation = f.to - Vec3::Y * ((f.t - 2.5).max(0.0) * 0.25);
        } else {
            commands.entity(e).despawn();
        }
    }
}
