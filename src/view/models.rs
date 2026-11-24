//! Unit models: glTF meshes converted from the mod (one part per bone) or JSON box models,
//! plus the materials (team colours are baked into a copy of the albedo texture).
use crate::data::{asset_root, ColorSpec, DefId, Defs, ModelDef};
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone)]
pub enum PartMat {
    /// Uses the unit's baked texture.
    Unit,
    Flat(ColorSpec),
}

#[derive(Clone)]
pub struct ModelPart {
    pub name: String,
    pub parent: Option<usize>,
    pub local: Transform,
    pub mesh: Option<Handle<Mesh>>,
    pub mat: PartMat,
    /// Axis and degrees per second.
    pub spin: Option<(Vec3, f32)>,
}

#[derive(Clone, Default)]
pub struct Model {
    pub parts: Vec<ModelPart>,
    pub scale: f32,
    /// The GLB's animations (the mod's .sca files, baked by tools/convert_assets.py), in `AnimKind` order.
    pub anims: Vec<Arc<Anim>>,
}

/// What plays an animation (`view::drive_anims`), from its glTF name (tools/convert_assets.py `ANIMS`).
/// In order of precedence: a part that several playing animations move follows the first.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum AnimKind {
    /// Follows an upgrade's progress.
    Upgrade,
    /// Factory platform: out and back when a finished unit leaves.
    Rolloff,
    /// Land factory doors, one per side (+x, -x, +z, -z): the one on the side a unit leaves by rolls up and down.
    GateE,
    GateW,
    GateS,
    GateN,
    /// Engineering station: opens to the middle while working, plays on to the end when done.
    Doors,
    /// Engineers: unfold while building, fold back after. The first frame (folded) is the idle pose.
    Open,
    /// Mass extractors: loops while producing, at the owner's power share.
    Work,
    /// Weapon unpack (Bohdana's stabilisers): out while it has a target and stands, back otherwise.
    Unpack,
    /// Landing legs: the first frame has them out; played forward while flying, back when hovering still.
    Land,
    /// On water (amphibious wheels fold away).
    Water,
    /// Cycle while moving, at ground speed.
    Walk,
}

impl AnimKind {
    fn from_name(n: &str) -> Option<AnimKind> {
        Some(match n {
            "upgrade" => AnimKind::Upgrade,
            "rolloff" => AnimKind::Rolloff,
            "gate_e" => AnimKind::GateE,
            "gate_w" => AnimKind::GateW,
            "gate_s" => AnimKind::GateS,
            "gate_n" => AnimKind::GateN,
            "doors" => AnimKind::Doors,
            "open" => AnimKind::Open,
            "work" => AnimKind::Work,
            "unpack" => AnimKind::Unpack,
            "land" => AnimKind::Land,
            "water" => AnimKind::Water,
            "walk" => AnimKind::Walk,
            _ => return None,
        })
    }
}

/// A keyframe animation of model parts. Looping ones (walk) repeat the first key at the end.
pub struct Anim {
    pub kind: AnimKind,
    /// Key times in seconds, from 0 to the cycle length.
    pub times: Vec<f32>,
    /// Part index, then translation and rotation per key.
    pub tracks: Vec<(usize, Vec<Vec3>, Vec<Quat>)>,
    /// Walk cycles: ground one cycle covers with the feet planted, in model units (0 if unknown).
    pub stride: f32,
}

impl Anim {
    pub fn duration(&self) -> f32 {
        self.times.last().copied().unwrap_or(0.0)
    }

    /// Local transform of a track at `t` seconds into the cycle (linear keys).
    pub fn pose(&self, track: usize, t: f32) -> (Vec3, Quat) {
        let (_, tr, rot) = &self.tracks[track];
        let k = self.times.partition_point(|&x| x <= t).clamp(1, self.times.len().max(2) - 1);
        let (a, b) = (self.times[k - 1], self.times[k]);
        let f = if b > a { ((t - a) / (b - a)).clamp(0.0, 1.0) } else { 0.0 };
        (tr[k - 1].lerp(tr[k], f), rot[k - 1].slerp(rot[k], f))
    }

    /// Stride from the animation itself: the lowest moving part is a foot; it sweeps back and forth once
    /// per cycle, and with two feet taking turns the body advances twice that sweep.
    fn measure_stride(&mut self, parts: &[ModelPart]) {
        let mut world: Vec<Vec<Vec3>> = vec![vec![]; parts.len()];
        for k in 0..self.times.len() {
            let mut xf: Vec<Transform> = Vec::with_capacity(parts.len());
            for (i, p) in parts.iter().enumerate() {
                let mut local = p.local;
                if let Some((_, tr, rot)) = self.tracks.iter().find(|t| t.0 == i) {
                    local.translation = tr[k];
                    local.rotation = rot[k];
                }
                // Parts come parent first (`load_glb` walks the tree depth first).
                let w = p.parent.map_or(local, |q| xf[q] * local);
                world[i].push(w.translation);
                xf.push(w);
            }
        }
        let foot = world
            .iter()
            .filter(|ps| ps.iter().any(|p| p.distance(ps[0]) > 1e-3))
            .min_by(|a, b| a.iter().map(|p| p.y).sum::<f32>().total_cmp(&b.iter().map(|p| p.y).sum::<f32>()));
        self.stride = foot.map_or(0.0, |ps| {
            let (lo, hi) = ps.iter().fold((f32::MAX, f32::MIN), |(lo, hi), p| (lo.min(p.z), hi.max(p.z)));
            2.0 * (hi - lo)
        });
    }
}

/// Material variants of one unit type for one owner colour.
#[derive(Clone)]
pub struct UnitMats {
    pub normal: Handle<StandardMaterial>,
    pub building: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
pub struct ModelCache {
    models: HashMap<DefId, Model>,
    mats: HashMap<(DefId, u8, u8), UnitMats>,
    /// `Settings::building_detail`, set every frame by `view::model_detail_sync`.
    pub detail: u8,
    /// Seed of the building about to be spawned (its unit id): at high detail it picks the variant.
    pub variant: u32,
    ruins: HashMap<(DefId, u8), Vec<Option<Handle<Mesh>>>>,
    ruin_mats: HashMap<DefId, Handle<StandardMaterial>>,
    rubble: Option<(Handle<Mesh>, Handle<Mesh>, Vec<Handle<StandardMaterial>>)>,
    flat: HashMap<(u32, u8), Handle<StandardMaterial>>,
    wreck: HashMap<DefId, Handle<StandardMaterial>>,
    /// Dark copies of unit materials (remembered buildings destroyed out of sight).
    dim: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    pub ghost_ok: Handle<StandardMaterial>,
    pub ghost_bad: Handle<StandardMaterial>,
    pub ghost_queued: Handle<StandardMaterial>,
}

/// How a glTF primitive is drawn. Materials named "team", "light", "dark" or "metal" take that flat colour
/// (as box models do); with an albedo texture (`textured`) everything else uses the unit's baked texture;
/// without one a material's base colour is used, except the converter's placeholder material "unit".
fn prim_mat(prim: &gltf::Primitive, textured: bool) -> PartMat {
    let m = prim.material();
    let name = m.name().unwrap_or("").to_ascii_lowercase();
    if ["team", "light", "dark", "metal"].contains(&name.as_str()) {
        return PartMat::Flat(ColorSpec::Name(name));
    }
    if textured || m.index().is_none() || name == "unit" {
        return PartMat::Unit;
    }
    // glTF colours are linear; ColorSpec::Rgb is sRGB.
    let c = m.pbr_metallic_roughness().base_color_factor();
    let srgb = |v: f32| Color::LinearRgba(LinearRgba::rgb(v, v, v)).to_srgba().red;
    PartMat::Flat(ColorSpec::Rgb([srgb(c[0]), srgb(c[1]), srgb(c[2])]))
}

fn prim_mesh(prim: &gltf::Primitive, blob: &[u8], meshes: &mut Assets<Mesh>, want_tangents: bool) -> Option<Handle<Mesh>> {
    let reader = prim.reader(|_| Some(blob));
    let pos: Vec<[f32; 3]> = reader.read_positions()?.collect();
    let n = pos.len();
    let nrm: Vec<[f32; 3]> = reader.read_normals().map(|i| i.collect()).unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; n]);
    let uv: Vec<[f32; 2]> = reader.read_tex_coords(0).map(|t| t.into_f32().collect()).unwrap_or_else(|| vec![[0.0, 0.0]; n]);
    let idx: Vec<u32> = reader.read_indices().map(|i| i.into_u32().collect()).unwrap_or_else(|| (0..n as u32).collect());
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_indices(Indices::U32(idx));
    if want_tangents {
        let _ = mesh.generate_tangents();
    }
    Some(meshes.add(mesh))
}

/// Reads a GLB: one part per node. A mesh with several primitives (an object with several materials)
/// gets one extra child part per further primitive.
fn load_glb(path: &std::path::Path, meshes: &mut Assets<Mesh>, want_tangents: bool, textured: bool) -> Result<(Vec<ModelPart>, Vec<Arc<Anim>>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let g = gltf::Gltf::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let blob = g.blob.clone().unwrap_or_default();
    let mut parts: Vec<ModelPart> = vec![];
    // glTF node index -> part index.
    let mut part_of: HashMap<usize, usize> = HashMap::new();
    struct Load<'a> {
        blob: &'a [u8],
        meshes: &'a mut Assets<Mesh>,
        want_tangents: bool,
        textured: bool,
        parts: &'a mut Vec<ModelPart>,
        part_of: &'a mut HashMap<usize, usize>,
    }
    fn walk(node: gltf::Node, parent: Option<usize>, l: &mut Load) {
        let local = Transform::from_matrix(Mat4::from_cols_array_2d(&node.transform().matrix()));
        let name = node.name().unwrap_or("").to_string();
        let prims: Vec<gltf::Primitive> = node.mesh().map(|m| m.primitives().collect()).unwrap_or_default();
        let me = l.parts.len();
        l.part_of.insert(node.index(), me);
        let first = prims.first();
        let mesh = first.and_then(|p| prim_mesh(p, l.blob, l.meshes, l.want_tangents));
        let mat = first.map_or(PartMat::Unit, |p| prim_mat(p, l.textured));
        l.parts.push(ModelPart { name: name.clone(), parent, local, mesh, mat, spin: None });
        for (k, p) in prims.iter().enumerate().skip(1) {
            let mesh = prim_mesh(p, l.blob, l.meshes, l.want_tangents);
            let mat = prim_mat(p, l.textured);
            l.parts.push(ModelPart { name: format!("{name}#{k}"), parent: Some(me), local: Transform::IDENTITY, mesh, mat, spin: None });
        }
        for c in node.children() {
            walk(c, Some(me), l);
        }
    }
    let mut l = Load { blob: &blob, meshes, want_tangents, textured, parts: &mut parts, part_of: &mut part_of };
    for scene in g.document.scenes() {
        for node in scene.nodes() {
            walk(node, None, &mut l);
        }
    }
    let mut anims: Vec<Arc<Anim>> = vec![];
    for a in g.document.animations() {
        match a.name().and_then(AnimKind::from_name) {
            Some(kind) if !anims.iter().any(|x| x.kind == kind) => anims.extend(read_anim(a, kind, &blob, &parts, &part_of)),
            _ => warn!("{}: animation {:?} ignored (unknown or twice)", path.display(), a.name()),
        }
    }
    anims.sort_by_key(|a| a.kind);
    Ok((parts, anims))
}

/// Reads a glTF animation of translation/rotation keys that all share one timeline (as the converter writes them).
fn read_anim(a: gltf::Animation, kind: AnimKind, blob: &[u8], parts: &[ModelPart], part_of: &HashMap<usize, usize>) -> Option<Arc<Anim>> {
    use gltf::animation::util::ReadOutputs;
    let mut times: Vec<f32> = vec![];
    let mut tracks: Vec<(usize, Vec<Vec3>, Vec<Quat>)> = vec![];
    for ch in a.channels() {
        let Some(&part) = part_of.get(&ch.target().node().index()) else { continue };
        let r = ch.reader(|_| Some(blob));
        let t: Vec<f32> = r.read_inputs()?.collect();
        if times.is_empty() {
            times = t.clone();
        }
        if t != times {
            warn!("animation {:?}: channels with different key times are not supported", a.name());
            return None;
        }
        let i = match tracks.iter().position(|x| x.0 == part) {
            Some(i) => i,
            None => {
                let rest = parts[part].local;
                tracks.push((part, vec![rest.translation; times.len()], vec![rest.rotation; times.len()]));
                tracks.len() - 1
            }
        };
        match r.read_outputs()? {
            ReadOutputs::Translations(v) => tracks[i].1 = v.map(Vec3::from_array).collect(),
            ReadOutputs::Rotations(v) => tracks[i].2 = v.into_f32().map(|q| Quat::from_array(q).normalize()).collect(),
            _ => {}
        }
    }
    if times.len() < 2 || tracks.iter().any(|t| t.1.len() != times.len() || t.2.len() != times.len()) {
        return None;
    }
    let mut anim = Anim { kind, times, tracks, stride: 0.0 };
    if kind == AnimKind::Walk {
        anim.measure_stride(parts);
    }
    Some(Arc::new(anim))
}

fn read_rgba(rel: &str) -> Option<image::RgbaImage> {
    image::open(asset_root().join(rel)).ok().map(|i| i.into_rgba8())
}

fn to_image(img: &image::RgbaImage, srgb: bool) -> Image {
    Image::new(
        Extent3d { width: img.width(), height: img.height(), depth_or_array_layers: 1 },
        TextureDimension::D2,
        img.as_raw().clone(),
        if srgb { TextureFormat::Rgba8UnormSrgb } else { TextureFormat::Rgba8Unorm },
        RenderAssetUsages::default(),
    )
}

/// Same image with a full mipmap chain and anisotropic filtering: far buildings no longer shimmer
/// and close ones stay sharp at a slant (`Settings::building_detail` high).
pub(super) fn to_image_mips(img: &image::RgbaImage, srgb: bool) -> Image {
    use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    let (mut w, mut h) = img.dimensions();
    let mut data = img.as_raw().clone();
    let mut cur = img.as_raw().clone();
    let mut levels = 1;
    while w > 1 || h > 1 {
        let (nw, nh) = ((w / 2).max(1), (h / 2).max(1));
        let mut next = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((x * 2 + dx).min(w - 1), (y * 2 + dy).min(h - 1));
                        sum += cur[((sy * w + sx) * 4 + c) as usize] as u32;
                    }
                    next[((y * nw + x) * 4 + c) as usize] = (sum / 4) as u8;
                }
            }
        }
        data.extend_from_slice(&next);
        cur = next;
        (w, h) = (nw, nh);
        levels += 1;
    }
    let mut im = to_image(img, srgb);
    im.data = Some(data);
    im.texture_descriptor.mip_level_count = levels;
    im.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    im
}

fn lattice(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E3779B1) ^ (y as u32).wrapping_mul(0x85EBCA6B) ^ seed.wrapping_mul(0xC2B2AE35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A2D39);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Smooth value noise, 0..1.
fn vnoise(x: f32, y: f32, seed: u32) -> f32 {
    let (xf, yf) = (x.floor(), y.floor());
    let (fx, fy) = (x - xf, y - yf);
    let (fx, fy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (xi, yi) = (xf as i32, yf as i32);
    let (a, b, c, d) = (lattice(xi, yi, seed), lattice(xi + 1, yi, seed), lattice(xi, yi + 1, seed), lattice(xi + 1, yi + 1, seed));
    let top = a + (b - a) * fx;
    top + ((c + (d - c) * fx) - top) * fy
}

/// How many looks of one building type there are at high detail.
const BUILDING_VARIANTS: u32 = 6;

/// Paints that multiply the baked wall and roof colours (mild: plaster and brick stay plaster and brick).
const WALL_PAINT: [[f32; 3]; 7] = [[1.0, 1.0, 1.0], [1.1, 1.03, 0.88], [0.9, 0.95, 1.06], [1.08, 0.94, 0.84], [0.95, 1.05, 0.92], [1.12, 1.02, 0.78], [0.85, 0.85, 0.88]];
const ROOF_PAINT: [[f32; 3]; 6] = [[1.0, 1.0, 1.0], [1.3, 0.75, 0.65], [0.7, 0.78, 0.9], [0.85, 1.1, 0.8], [0.72, 0.66, 0.6], [1.15, 0.9, 0.7]];

/// Which texels of a model's albedo belong to roofs (255, faces turned up) and which to walls (120),
/// by rasterising its triangles in UV space; 0 where nothing is drawn.
fn roof_mask(parts: &[ModelPart], meshes: &Assets<Mesh>, w: u32, h: u32) -> Vec<u8> {
    use bevy::mesh::VertexAttributeValues as V;
    let mut mask = vec![0u8; (w * h) as usize];
    for part in parts {
        let Some(mesh) = part.mesh.as_ref().and_then(|m| meshes.get(m)) else { continue };
        let (Some(V::Float32x3(pos)), Some(V::Float32x2(uv)), Some(idx)) = (mesh.attribute(Mesh::ATTRIBUTE_POSITION), mesh.attribute(Mesh::ATTRIBUTE_UV_0), mesh.indices()) else { continue };
        let idx: Vec<u32> = match idx {
            Indices::U32(i) => i.clone(),
            Indices::U16(i) => i.iter().map(|&x| x as u32).collect(),
        };
        for t in idx.chunks_exact(3) {
            let p: Vec<Vec3> = t.iter().map(|&i| Vec3::from_array(pos[i as usize])).collect();
            let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or_zero();
            let val = if n.y.abs() > 0.55 { 255 } else { 120 };
            let q: Vec<Vec2> = t.iter().map(|&i| Vec2::new(uv[i as usize][0] * w as f32, uv[i as usize][1] * h as f32)).collect();
            let (lo, hi) = (q[0].min(q[1]).min(q[2]), q[0].max(q[1]).max(q[2]));
            let den = (q[1].y - q[2].y) * (q[0].x - q[2].x) + (q[2].x - q[1].x) * (q[0].y - q[2].y);
            if den.abs() < 1e-6 {
                continue;
            }
            for y in (lo.y.floor().max(0.0) as u32)..=(hi.y.ceil().max(0.0) as u32).min(h - 1) {
                for x in (lo.x.floor().max(0.0) as u32)..=(hi.x.ceil().max(0.0) as u32).min(w - 1) {
                    let c = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                    let a = ((q[1].y - q[2].y) * (c.x - q[2].x) + (q[2].x - q[1].x) * (c.y - q[2].y)) / den;
                    let b = ((q[2].y - q[0].y) * (c.x - q[2].x) + (q[0].x - q[2].x) * (c.y - q[2].y)) / den;
                    let g = 1.0 - a - b;
                    if a >= -0.03 && b >= -0.03 && g >= -0.03 {
                        mask[(y * w + x) as usize] = val;
                    }
                }
            }
        }
    }
    mask
}

/// A broken, burnt-out copy of a part: the top is knocked down to a ragged edge, a few holes are
/// blown through the walls, and soot (vertex colours) darkens the edges of the holes and the top.
fn ruin_mesh(src: &Mesh, seed: u32, style: u32) -> Option<Mesh> {
    use bevy::mesh::VertexAttributeValues as V;
    let mut pos = match src.attribute(Mesh::ATTRIBUTE_POSITION)? {
        V::Float32x3(v) => v.clone(),
        _ => return None,
    };
    let nrm = match src.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(V::Float32x3(v)) => v.clone(),
        _ => vec![[0.0, 1.0, 0.0]; pos.len()],
    };
    let uv = match src.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(V::Float32x2(v)) => v.clone(),
        _ => vec![[0.0, 0.0]; pos.len()],
    };
    let idx: Vec<u32> = match src.indices()? {
        Indices::U32(i) => i.clone(),
        Indices::U16(i) => i.iter().map(|&x| x as u32).collect(),
    };
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &pos {
        lo = lo.min(Vec3::from_array(*p));
        hi = hi.max(Vec3::from_array(*p));
    }
    let ext = hi - lo;
    if ext.y < 0.05 || ext.x < 0.05 || ext.z < 0.05 {
        return None;
    }
    let big = ext.max_element();
    // Holes.
    let n_holes = match style {
        2 => 6 + seed % 3,
        5 => 7 + seed % 4,
        _ => 3 + seed % 3,
    };
    let holes: Vec<(Vec3, f32)> = (0..n_holes)
        .map(|k| {
            let r = |n: u32| lattice(k as i32, n as i32, seed ^ 0x51);
            let rad = if style == 5 { 0.06 + 0.08 * r(4) } else { 0.1 + 0.14 * r(4) };
            (lo + Vec3::new(ext.x * (0.1 + 0.8 * r(1)), ext.y * (0.12 + 0.55 * r(2)), ext.z * (0.1 + 0.8 * r(3))), big * rad)
        })
        .collect();
    let mut cuts = vec![0.0f32; pos.len()];
    for (i, p) in pos.iter_mut().enumerate() {
        let (u, v) = ((p[0] - lo.x) / ext.x * 4.0, (p[2] - lo.z) / ext.z * 4.0);
        let nz = vnoise(u, v, seed);
        let (fu, fv) = ((p[0] - lo.x) / ext.x, (p[2] - lo.z) / ext.z);
        let corner = Vec2::new(if lattice(5, 0, seed) > 0.5 { 1.0 } else { 0.0 }, if lattice(6, 0, seed) > 0.5 { 1.0 } else { 0.0 });
        let frac = match style {
            // Flat and low.
            0 => 0.25 + 0.3 * nz,
            // One corner still stands tall.
            1 => 0.3 + 0.3 * nz + 0.5 * (1.0 - Vec2::new(fu, fv).distance(corner) * 1.5).max(0.0),
            // Gutted: walls mostly stand, windows blown out.
            2 => 0.6 + 0.3 * nz,
            // One side collapsed.
            3 => (0.2 + 0.7 * (if corner.x > 0.5 { fu } else { 1.0 - fu })) * (0.8 + 0.3 * nz),
            _ => 0.4 + 0.45 * nz,
        };
        let cut = lo.y + ext.y * frac.clamp(0.12, 1.0);
        cuts[i] = cut;
        if p[1] > cut {
            p[1] = cut + (p[1] - cut) * 0.1;
        }
    }
    let col: Vec<[f32; 4]> = pos
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let q = Vec3::from_array(*p);
            let mut soot = 0.8 + 0.2 * vnoise(q.x * 3.0 / big * 4.0, q.z * 3.0 / big * 4.0 + q.y * 2.0, seed ^ 7);
            for (c, r) in &holes {
                let d = q.distance(*c);
                if d < r * 1.8 {
                    soot = soot.min(0.12 + 0.88 * (d / (r * 1.8)).powi(2));
                }
            }
            let top = ((q.y - lo.y) / (cuts[i] - lo.y).max(0.01)).clamp(0.0, 1.0);
            soot *= 1.0 - 0.6 * ((top - 0.7) / 0.3).clamp(0.0, 1.0);
            // Ash at the foot.
            soot *= 0.7 + 0.3 * ((q.y - lo.y) / (ext.y * 0.3)).clamp(0.0, 1.0);
            let s = soot.max(0.1);
            [s, s * 0.95, s * 0.9, 1.0]
        })
        .collect();
    let mut keep: Vec<u32> = Vec::with_capacity(idx.len());
    for t in idx.chunks_exact(3) {
        let c = (Vec3::from_array(pos[t[0] as usize]) + Vec3::from_array(pos[t[1] as usize]) + Vec3::from_array(pos[t[2] as usize])) / 3.0;
        if !holes.iter().any(|(h, r)| c.distance(*h) < *r) {
            keep.extend_from_slice(t);
        }
    }
    Some(
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
            .with_inserted_indices(Indices::U32(keep)),
    )
}

impl ModelCache {
    pub fn init(&mut self, materials: &mut Assets<StandardMaterial>) {
        let ghost = |c: Color| StandardMaterial { base_color: c, alpha_mode: AlphaMode::Blend, unlit: true, ..default() };
        self.ghost_ok = materials.add(ghost(Color::srgba(0.3, 1.0, 0.4, 0.45)));
        self.ghost_bad = materials.add(ghost(Color::srgba(1.0, 0.25, 0.2, 0.45)));
        self.ghost_queued = materials.add(ghost(Color::srgba(0.5, 0.8, 1.0, 0.3)));
    }

    pub fn model(&mut self, defs: &Defs, def: DefId, meshes: &mut Assets<Mesh>) -> &Model {
        if !self.models.contains_key(&def) {
            let d = defs.get(def);
            let md: &ModelDef = &d.model;
            let mut model = Model { parts: vec![], scale: md.scale, anims: vec![] };
            if let Some(mesh) = &md.mesh {
                match load_glb(&asset_root().join(mesh), meshes, md.normal.is_some(), md.albedo.is_some()) {
                    Ok((parts, anims)) => {
                        model.parts = parts;
                        model.anims = anims;
                    }
                    Err(e) => warn!("model for {}: {e}", d.id),
                }
                for s in &md.spin {
                    if let Some(p) = model.parts.iter_mut().find(|p| p.name == s.bone) {
                        p.spin = Some((Vec3::from_array(s.axis).normalize_or_zero(), s.speed));
                    }
                }
            }
            for (i, b) in md.boxes.iter().enumerate() {
                model.parts.push(ModelPart {
                    name: format!("box{i}"),
                    parent: None,
                    local: Transform::from_translation(Vec3::from_array(b.pos)),
                    mesh: Some(meshes.add(Cuboid::new(b.size[0], b.size[1], b.size[2]))),
                    mat: PartMat::Flat(b.color.clone()),
                    spin: b.spin.map(|s| (Vec3::Y, s)),
                });
            }
            if model.parts.is_empty() {
                // Something to look at even when the model is missing.
                model.scale = 1.0;
                model.parts.push(ModelPart {
                    name: "fallback".into(),
                    parent: None,
                    local: Transform::from_xyz(0.0, d.size[1] * 0.5, 0.0),
                    mesh: Some(meshes.add(Cuboid::new(d.size[0], d.size[1], d.size[2]))),
                    mat: PartMat::Flat(ColorSpec::Name("team".into())),
                    spin: None,
                });
            }
            self.models.insert(def, model);
        }
        &self.models[&def]
    }

    /// Textured materials of a unit type in an owner colour (index into `palette`).
    pub fn unit_mats(&mut self, defs: &Defs, def: DefId, color: u8, rgb: [f32; 3], parts: &[ModelPart], meshes: &Assets<Mesh>, images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>) -> UnitMats {
        let high = self.detail >= 2;
        // Neutral map buildings differ one from the next (colour, wear); our own keep the team's look.
        let var = if high && defs.get(def).has_category("civilian") { 1 + (self.variant.wrapping_mul(2654435761) >> 16) % BUILDING_VARIANTS } else { 0 } as u8;
        if let Some(m) = self.mats.get(&(def, color, var)) {
            return m.clone();
        }
        let md = &defs.get(def).model;
        let mut tex = None;
        if let Some(mut albedo) = md.albedo.as_deref().and_then(read_rgba) {
            let mask = md.team_mask.as_deref().and_then(read_rgba).filter(|m| m.dimensions() == albedo.dimensions());
            let width = albedo.width();
            let roofs = if var > 0 { roof_mask(parts, meshes, albedo.width(), albedo.height()) } else { vec![] };
            let wall_paint = WALL_PAINT[(var as usize * 3 + def as usize) % WALL_PAINT.len()];
            let roof_paint = ROOF_PAINT[(var as usize * 5 + def as usize * 2 + 1) % ROOF_PAINT.len()];
            for (i, px) in albedo.pixels_mut().enumerate() {
                let (x, y) = (i as u32 % width, i as u32 / width);
                // The mod's albedo maps are dark (`brightness` 2.1 lifts them); paint the team areas.
                let b = md.brightness;
                let mut c = [px[0] as f32 / 255.0 * b, px[1] as f32 / 255.0 * b, px[2] as f32 / 255.0 * b];
                if var > 0 {
                    // This variant's paint and wear: a tint, blotchy weathering and rain streaks.
                    let sd = var as u32 * 7919 + def as u32 * 104729;
                    let paint = if roofs.get(i).copied().unwrap_or(0) == 255 { roof_paint } else { wall_paint };
                    let tint = [paint[0] * (0.95 + 0.1 * lattice(1, 0, sd)), paint[1] * (0.95 + 0.1 * lattice(2, 0, sd)), paint[2] * (0.95 + 0.1 * lattice(3, 0, sd))];
                    let blot = vnoise(x as f32 / 22.0, y as f32 / 22.0, sd);
                    let streak = vnoise(x as f32 / 5.0, y as f32 / 70.0, sd ^ 77);
                    let wear = (0.84 + 0.26 * blot) * (1.0 - 0.22 * streak * streak) * (0.94 + 0.12 * lattice(x as i32, y as i32, sd));
                    for j in 0..3 {
                        c[j] *= tint[j] * wear;
                    }
                }
                if let Some(m) = &mask {
                    let k = m.get_pixel(x, y)[0] as f32 / 255.0;
                    if k > 0.0 {
                        let lum = (c[0] + c[1] + c[2]) / 3.0;
                        for j in 0..3 {
                            c[j] += (rgb[j] * (0.45 + lum * 1.3) - c[j]) * k.min(1.0);
                        }
                    }
                }
                for j in 0..3 {
                    px[j] = (c[j].clamp(0.0, 1.0) * 255.0) as u8;
                }
                px[3] = 255;
            }
            tex = Some(images.add(if high { to_image_mips(&albedo, true) } else { to_image(&albedo, true) }));
        }
        let normal = md.normal.as_deref().and_then(read_rgba).map(|n| images.add(if high { to_image_mips(&n, false) } else { to_image(&n, false) }));
        let base = StandardMaterial {
            base_color: if tex.is_some() { Color::WHITE } else { Color::srgb(rgb[0], rgb[1], rgb[2]) },
            base_color_texture: tex.clone(),
            normal_map_texture: normal,
            perceptual_roughness: 0.55,
            metallic: 0.25,
            ..default()
        };
        let building = StandardMaterial {
            base_color: Color::srgba(0.55, 1.0, 0.7, 0.55),
            base_color_texture: tex,
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.8,
            ..default()
        };
        let m = UnitMats { normal: materials.add(base), building: materials.add(building) };
        self.mats.insert((def, color, var), m.clone());
        m
    }

    pub fn flat_mat(&mut self, spec: &ColorSpec, color: u8, rgb: [f32; 3], materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
        let c = match spec {
            ColorSpec::Rgb(c) => *c,
            ColorSpec::Name(n) => match n.as_str() {
                "team" => rgb,
                "light" => [rgb[0] * 0.45 + 0.5, rgb[1] * 0.45 + 0.5, rgb[2] * 0.45 + 0.5],
                "dark" => [0.14, 0.14, 0.16],
                _ => [0.5, 0.52, 0.56],
            },
        };
        let key = (((c[0] * 255.0) as u32) << 16 | ((c[1] * 255.0) as u32) << 8 | (c[2] * 255.0) as u32, color);
        self.flat.entry(key).or_insert_with(|| materials.add(StandardMaterial { base_color: Color::srgb(c[0], c[1], c[2]), perceptual_roughness: 0.75, ..default() })).clone()
    }

    /// A darkened copy of `mat` (opaque: see-through parts of one model would sort badly).
    pub fn dim_mat(&mut self, mat: &Handle<StandardMaterial>, materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
        if let Some(m) = self.dim.get(&mat.id()) {
            return m.clone();
        }
        let mut m = materials.get(mat).cloned().unwrap_or_default();
        let c = m.base_color.to_srgba();
        m.base_color = Color::srgba(c.red * 0.3, c.green * 0.3, c.blue * 0.32, c.alpha);
        m.emissive = LinearRgba::BLACK;
        m.reflectance = 0.1;
        let h = materials.add(m);
        self.dim.insert(mat.id(), h.clone());
        h
    }

    /// The wreck material of a destroyed building at high detail: the burnt texture, seen from both sides
    /// (the holes show the inside), darkened further by the ruin mesh's vertex colours.
    pub fn ruin_mat(&mut self, defs: &Defs, def: DefId, images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
        if let Some(m) = self.ruin_mats.get(&def) {
            return m.clone();
        }
        let tex = defs.get(def).model.albedo.as_deref().and_then(read_rgba).map(|a| images.add(to_image_mips(&a, true)));
        let m = materials.add(StandardMaterial {
            base_color: Color::srgb(0.62, 0.56, 0.52),
            base_color_texture: tex,
            perceptual_roughness: 0.97,
            double_sided: true,
            cull_mode: None,
            ..default()
        });
        self.ruin_mats.insert(def, m.clone());
        m
    }

    pub fn wreck_mat(&mut self, defs: &Defs, def: DefId, images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>) -> Handle<StandardMaterial> {
        if let Some(m) = self.wreck.get(&def) {
            return m.clone();
        }
        let tex = defs.get(def).model.albedo.as_deref().and_then(read_rgba).map(|a| images.add(to_image(&a, true)));
        let m = materials.add(StandardMaterial { base_color: Color::srgb(0.55, 0.45, 0.4), base_color_texture: tex, perceptual_roughness: 0.95, ..default() });
        self.wreck.insert(def, m.clone());
        m
    }
}

/// The ruin of a destroyed building (`Settings::building_detail` high): its model with the top broken
/// off and holes burnt through, one of six shapes picked by `seed`.
pub fn spawn_ruin(commands: &mut Commands, cache: &mut ModelCache, defs: &Defs, def: DefId, seed: u32, meshes: &mut Assets<Mesh>, images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>) -> Entity {
    let model = cache.model(defs, def, meshes).clone();
    let mat = cache.ruin_mat(defs, def, images, materials);
    let key = (def, (seed % 6) as u8);
    if !cache.ruins.contains_key(&key) {
        let made: Vec<Option<Mesh>> = model
            .parts
            .iter()
            .enumerate()
            .map(|(pi, p)| p.mesh.as_ref().and_then(|h| meshes.get(h)).and_then(|m| ruin_mesh(m, (seed % 6) * 977 + pi as u32 * 13 + 1, seed % 6)))
            .collect();
        let handles = made.into_iter().map(|m| m.map(|m| meshes.add(m))).collect();
        cache.ruins.insert(key, handles);
    }
    let ruined = &cache.ruins[&key];
    let root = commands.spawn((ModelRoot { def, building: false }, Transform::from_scale(Vec3::splat(model.scale)), Visibility::Inherited)).id();
    let mut ents: Vec<Entity> = Vec::with_capacity(model.parts.len());
    for (pi, part) in model.parts.iter().enumerate() {
        let mut e = commands.spawn((part.local, Visibility::Inherited));
        if let Some(mesh) = ruined[pi].as_ref().or(part.mesh.as_ref()) {
            e.insert((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone())));
        }
        let id = e.id();
        commands.entity(part.parent.map_or(root, |p| ents[p])).add_child(id);
        ents.push(id);
    }
    root
}

/// Rubble round a ruin: broken blocks half sunk in the ground, burnt beams, and a patch of ash. In the
/// wreck's own frame, `size` the building's footprint (world units).
pub fn spawn_rubble(commands: &mut Commands, cache: &mut ModelCache, size: [f32; 3], seed: u32, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> Vec<Entity> {
    let (block, disc, mats) = cache.rubble.get_or_insert_with(|| {
        let mut list = vec![];
        for c in [[0.34, 0.32, 0.3], [0.26, 0.2, 0.17], [0.2, 0.19, 0.19], [0.08, 0.07, 0.06]] {
            list.push(materials.add(StandardMaterial { base_color: Color::srgb(c[0], c[1], c[2]), perceptual_roughness: 1.0, ..default() }));
        }
        list.push(materials.add(StandardMaterial { base_color: Color::srgba(0.03, 0.03, 0.03, 0.55), alpha_mode: AlphaMode::Blend, perceptual_roughness: 1.0, depth_bias: 1.0, ..default() }));
        (meshes.add(Cuboid::new(1.0, 1.0, 1.0)), meshes.add(Cylinder::new(1.0, 0.02)), list)
    }).clone();
    let r = |k: u32| lattice(k as i32, 77, seed);
    let (hx, hz) = (size[0] * 0.5, size[2] * 0.5);
    let scale = ((size[0] + size[2]) / 8.0).clamp(0.5, 2.5);
    let mut out = vec![];
    let mut put = |commands: &mut Commands, mesh: &Handle<Mesh>, mat: &Handle<StandardMaterial>, t: Transform| {
        out.push(commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(mat.clone()), t, Visibility::Inherited)).id());
    };
    put(commands, &disc, &mats[4], Transform::from_xyz(0.0, 0.03, 0.0).with_scale(Vec3::new(hx.max(hz) * 1.15, 1.0, hx.max(hz) * 1.15)));
    let n = ((size[0] * size[2] * 1.4) as u32).clamp(10, 44);
    for k in 0..n {
        let s = (0.14 + 0.34 * r(k * 7 + 3).powi(2)) * scale;
        let (x, z) = ((r(k * 7 + 1) * 2.0 - 1.0) * hx * 1.2, (r(k * 7 + 2) * 2.0 - 1.0) * hz * 1.2);
        let rot = Quat::from_euler(EulerRot::YXZ, r(k * 7 + 4) * 6.28, (r(k * 7 + 5) - 0.5) * 0.9, (r(k * 7 + 6) - 0.5) * 0.9);
        let stretch = Vec3::new(1.0 + r(k * 7 + 8), 0.5 + 0.6 * r(k * 7 + 9), 0.8 + 0.6 * r(k * 7 + 10));
        put(commands, &block, &mats[(r(k * 7 + 11) * 3.0) as usize % 3], Transform::from_xyz(x, s * 0.2, z).with_rotation(rot).with_scale(stretch * s));
    }
    // Burnt beams lying across it.
    for k in 0..(3 + n / 8) {
        let len = (0.9 + 1.2 * r(k * 5 + 300)) * scale;
        let (x, z) = ((r(k * 5 + 301) * 2.0 - 1.0) * hx, (r(k * 5 + 302) * 2.0 - 1.0) * hz);
        let rot = Quat::from_euler(EulerRot::YXZ, r(k * 5 + 303) * 6.28, 0.0, (r(k * 5 + 304) - 0.5) * 0.5);
        put(commands, &block, &mats[3], Transform::from_xyz(x, 0.12 * scale, z).with_rotation(rot).with_scale(Vec3::new(len, 0.07 * scale, 0.07 * scale)));
    }
    out
}

/// Marks a model part that follows a weapon's yaw or pitch.
#[derive(Component)]
pub struct AimPart {
    pub weapon: usize,
    pub pitch: bool,
    pub rest: Transform,
}

/// A model part moved by animations of its unit (`Animator` on the unit entity).
#[derive(Component)]
pub struct AnimPart {
    pub rest: Transform,
    /// (slot in `Animator::slots` = index in `Model::anims`, track in that animation), in precedence order.
    pub tracks: Vec<(usize, usize)>,
}

impl AnimPart {
    /// The local transform: the first playing animation that moves this part, blended over the rest pose
    /// by its weight; the rest pose when none does.
    pub fn pose(&self, animator: Option<&Animator>) -> Transform {
        let Some(a) = animator else { return self.rest };
        for &(slot, track) in &self.tracks {
            let Some(s) = a.slots.get(slot) else { continue };
            if s.weight <= 0.0 {
                continue;
            }
            let (tr, rot) = s.anim.pose(track, s.time);
            let mut t = self.rest;
            t.translation = self.rest.translation.lerp(tr, s.weight);
            t.rotation = self.rest.rotation.slerp(rot, s.weight);
            return t;
        }
        self.rest
    }
}

/// Animation state of a unit whose model has animations (view only): one slot per `Model::anims` entry.
#[derive(Component)]
pub struct Animator {
    pub slots: Vec<AnimSlot>,
}

pub struct AnimSlot {
    pub anim: Arc<Anim>,
    /// Seconds into the animation.
    pub time: f32,
    /// 0 = not shown (rest pose), 1 = fully.
    pub weight: f32,
    /// Walk: stride in world units (0: unknown).
    pub stride: f32,
    /// Rolloff: the unit the factory was building last frame.
    pub mark: u32,
    /// Rolloff: seconds into the out-hold-back sequence, negative when idle.
    pub seq: f32,
}

impl Animator {
    pub fn new(model: &Model) -> Option<Animator> {
        if model.anims.is_empty() {
            return None;
        }
        let slots = model
            .anims
            .iter()
            .map(|a| AnimSlot { anim: a.clone(), time: 0.0, weight: 0.0, stride: a.stride * model.scale, mark: 0, seq: -1.0 })
            .collect();
        Some(Animator { slots })
    }
}

#[derive(Component)]
pub struct SpinPart {
    pub axis: Vec3,
    pub speed: f32,
    pub rest: Transform,
}

/// Root of the visual parts under a unit entity.
#[derive(Component)]
pub struct ModelRoot {
    pub def: DefId,
    pub building: bool,
}

/// Which material slot a part uses, so it can be swapped when construction finishes.
#[derive(Component)]
pub struct UnitSkin;

/// Spawns the part hierarchy of a model as children of `parent`. Returns the root entity.
/// `override_mat` is the material of a wreck: a wreck's parts stay as they are (no animations, aiming or
/// spinning parts), in the rest pose.
#[allow(clippy::too_many_arguments)]
pub fn spawn_model(
    commands: &mut Commands,
    cache: &mut ModelCache,
    defs: &Defs,
    def: DefId,
    color: u8,
    rgb: [f32; 3],
    building: bool,
    override_mat: Option<Handle<StandardMaterial>>,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Entity {
    spawn_model_dim(commands, cache, defs, def, color, rgb, building, override_mat, false, meshes, images, materials)
}

/// `spawn_model`, `dim`: every part drawn with a darkened copy of its material.
#[allow(clippy::too_many_arguments)]
pub fn spawn_model_dim(
    commands: &mut Commands,
    cache: &mut ModelCache,
    defs: &Defs,
    def: DefId,
    color: u8,
    rgb: [f32; 3],
    building: bool,
    override_mat: Option<Handle<StandardMaterial>>,
    dim: bool,
    meshes: &mut Assets<Mesh>,
    images: &mut Assets<Image>,
    materials: &mut Assets<StandardMaterial>,
) -> Entity {
    let model = cache.model(defs, def, meshes).clone();
    let mats = cache.unit_mats(defs, def, color, rgb, &model.parts, meshes, images, materials);
    let d = defs.get(def);
    let jitter = if cache.detail >= 2 && d.has_category("civilian") {
        let k = cache.variant;
        Vec3::new(0.97 + 0.06 * lattice(1, 1, k), 0.94 + 0.12 * lattice(2, 1, k), 0.97 + 0.06 * lattice(3, 1, k))
    } else {
        Vec3::ONE
    };
    let root = commands.spawn((ModelRoot { def, building }, Transform::from_scale(Vec3::splat(model.scale) * jitter), Visibility::Inherited)).id();
    let live = override_mat.is_none();
    let mut ents: Vec<Entity> = Vec::with_capacity(model.parts.len());
    for (pi, part) in model.parts.iter().enumerate() {
        let mut e = commands.spawn((part.local, Visibility::Inherited));
        let tracks: Vec<(usize, usize)> = model
            .anims
            .iter()
            .enumerate()
            .filter_map(|(slot, a)| a.tracks.iter().position(|t| t.0 == pi).map(|track| (slot, track)))
            .collect();
        if live && !tracks.is_empty() {
            e.insert(AnimPart { rest: part.local, tracks });
        }
        if let Some(mesh) = &part.mesh {
            let mat = match (&override_mat, &part.mat) {
                (Some(m), _) => m.clone(),
                (None, PartMat::Unit) => {
                    e.insert(UnitSkin);
                    if building {
                        mats.building.clone()
                    } else {
                        mats.normal.clone()
                    }
                }
                (None, PartMat::Flat(spec)) => cache.flat_mat(spec, color, rgb, materials),
            };
            let mat = if dim { cache.dim_mat(&mat, materials) } else { mat };
            e.insert((Mesh3d(mesh.clone()), MeshMaterial3d(mat)));
        }
        for (wi, w) in d.weapons.iter().enumerate().filter(|_| live) {
            if let Some(t) = &w.turret {
                if t.yaw_bone.as_deref() == Some(part.name.as_str()) {
                    e.insert(AimPart { weapon: wi, pitch: false, rest: part.local });
                } else if t.pitch_bone.as_deref() == Some(part.name.as_str()) {
                    e.insert(AimPart { weapon: wi, pitch: true, rest: part.local });
                }
            }
        }
        if let Some((axis, speed)) = part.spin.filter(|_| live) {
            e.insert(SpinPart { axis, speed, rest: part.local });
        }
        let id = e.id();
        commands.entity(part.parent.map_or(root, |p| ents[p])).add_child(id);
        ents.push(id);
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every converted model loads, and its animations have tracks for real parts with one key per time.
    #[test]
    fn model_animations_load() {
        let mut meshes = Assets::<Mesh>::default();
        let dir = asset_root().join("models");
        let mut found = vec![];
        for f in std::fs::read_dir(&dir).unwrap() {
            let p = f.unwrap().path();
            if p.extension().is_some_and(|e| e == "glb") {
                let (parts, anims) = load_glb(&p, &mut meshes, false, true).unwrap();
                for a in &anims {
                    assert!(a.times.len() >= 2 && a.duration() > 0.0, "{}", p.display());
                    for (part, tr, rot) in &a.tracks {
                        assert!(*part < parts.len() && tr.len() == a.times.len() && rot.len() == a.times.len());
                        assert!(rot.iter().all(|q| q.is_normalized()));
                    }
                    found.push(format!("{}:{:?}", p.file_stem().unwrap().to_string_lossy(), a.kind));
                }
                if p.file_stem().is_some_and(|s| s == "trooper") {
                    let walk = anims.iter().find(|a| a.kind == AnimKind::Walk).expect("trooper walk");
                    assert!((5.0..9.0).contains(&walk.stride), "trooper stride {}", walk.stride);
                }
            }
        }
        found.sort();
        println!("{}", found.join(" "));
        assert!(found.len() >= 19, "{found:?}");
    }
}
