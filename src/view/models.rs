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

// `models::AnimKind` (one per name, in precedence order) says what plays each, `view::drive_anims` drives unit's
// `Animator` from sim state, `AnimPart::pose` picks first playing animation per part, `animate_parts` layers
// turret aim on top: `walk` (trooper; cycles with ground speed at mod's rate at full speed, slower where feet would
// slide - commander - backwards when reversing), `upgrade` (factories, pumps: follows `Upgrading` progress),
// `work` (pumps: loops at owner's `power`, frozen when paused), `open` (engineers: folded = first frame when idle,
// unfold while `Builder.work`), `doors` (eng station: open to middle while working, then on to end), `unpack`
// (Bohdana: target and standing), `land` (Stork: gear out when hovering still, folded when flying - it never lands),
// `water` (ground under unit below water level; no current model has one), `rolloff` (air factory platform: out, hold,
// back when `Factory.building` changes; SupCom only does it for land units). `models::tests` loads every GLB.
// Wrecks (`spawn_model` with wreck material) get no animated, aimed or spinning parts: rest pose, still.
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

// **Glass and metal reflections** (ultra building detail, requested close up only): `models::gloss_map` (from stored albedo: blue-tinted texels not on roofs = glass, vehicle's grey = steel) gives glossy material with metallic-roughness map and lightened panes; `gloss_sync` swaps every model between it and plain one when `cam_dist` crosses `GLOSS_NEAR`/`GLOSS_FAR` (38/46; `ModelCache::pair`, `set_close`). Flat "metal" parts likewise.
/// Roughness (green) and metalness (blue) per texel of an albedo map as stored (ultra building detail):
/// window glass (blue-tinted texels: the art kit's blue-grey panes and canopies, the mod's pale panes)
/// is smooth and mirror-like, a vehicle's bare grey steel is semi-polished metal, everything else keeps
/// the plain material's 0.55 / 0.25. Lit (warm) windows are left as they are.
/// `surfaces` (`roof_mask`, empty = no limit): where it marks roofs, there is no glass. Glass texels have
/// metalness `GLASS_METAL` (255), and only they.
fn gloss_map(albedo: &image::RgbaImage, vehicle: bool, surfaces: &[u8]) -> image::RgbaImage {
    let mut out = image::RgbaImage::new(albedo.width(), albedo.height());
    for (i, (px, o)) in albedo.pixels().zip(out.pixels_mut()).enumerate() {
        let c = [px[0] as f32 / 255.0, px[1] as f32 / 255.0, px[2] as f32 / 255.0];
        let (l, sat) = (luminance(c), saturation(c));
        let glass = c[2] > c[0] + 0.065 && c[2] >= c[1] - 0.01 && (0.08..0.97).contains(&l) && (0.06..0.7).contains(&sat) && surfaces.get(i).copied() != Some(255);
        let steel = vehicle && sat < 0.16 && (0.22..0.85).contains(&l);
        let (rough, metal) = if glass {
            (0.05, GLASS_METAL as f32 / 255.0)
        } else if steel {
            (0.32, 0.6)
        } else {
            (0.55, 0.25)
        };
        *o = image::Rgba([0, (rough * 255.0) as u8, (metal * 255.0) as u8, 255]);
    }
    out
}

/// Metalness byte of glass in `gloss_map` (full: a mirror tinted by the pane's colour).
const GLASS_METAL: u8 = 255;

/// Material variants of one unit type for one owner colour.
#[derive(Clone)]
pub struct UnitMats {
    pub normal: Handle<StandardMaterial>,
    pub building: Handle<StandardMaterial>,
}

#[derive(Resource, Default)]
pub struct ModelCache {
    models: HashMap<(DefId, bool), Model>,
    mats: HashMap<(DefId, u8, u8), UnitMats>,
    /// `Settings::building_detail`, set every frame by `view::model_detail_sync`.
    pub detail: u8,
    /// Buildings with a low-poly version show it (building detail below high; set by `model_detail_sync`).
    pub low_poly: bool,
    /// Seed of the building about to be spawned (its unit id): at high detail it picks the variant.
    pub variant: u32,
    ruins: HashMap<(DefId, u8), Vec<Option<Handle<Mesh>>>>,
    ruin_mats: HashMap<DefId, Handle<StandardMaterial>>,
    /// The map's theme (`set_theme`).
    pub theme: Theme,
    theme_name: Option<String>,
    /// Ultra: part meshes with eaves and window sills added.
    detailed: HashMap<DefId, Vec<Option<Handle<Mesh>>>>,
    /// Per part of a model (keyed like `models`): its simplified mesh for far away, if it gets much
    /// smaller (`decimate::simplify`).
    far: HashMap<(DefId, bool), Vec<Option<Handle<Mesh>>>>,
    /// Per model (keyed like `models`): the skirt meshes of its parts (`skirt_meshes`).
    skirts: HashMap<(DefId, bool), Vec<(usize, Handle<Mesh>)>>,
    rubble: Option<(Handle<Mesh>, Handle<Mesh>, Vec<Handle<StandardMaterial>>)>,
    flat: HashMap<(u32, u8), Handle<StandardMaterial>>,
    /// Glossy close-up versions of plain materials and the way back (`pair`, `set_close`).
    shiny: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    plain: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    /// The camera is close enough for glass and metal to show reflections.
    pub close: bool,
    wreck: HashMap<(DefId, bool), Handle<StandardMaterial>>,
    /// Dark copies of unit materials (remembered buildings destroyed out of sight).
    dim: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    pub ghost_ok: Handle<StandardMaterial>,
    pub ghost_bad: Handle<StandardMaterial>,
    pub ghost_queued: Handle<StandardMaterial>,
}

// Models made in Blender go through `tools/blend.py` (`blend_export.py` runs inside Blender): objects are parts,
// actions named like `AnimKind`s are animations, materials named team/light/dark/metal take that flat colour
// (`models::prim_mat`; other materials keep their colour when unit has no albedo texture), object with several
// materials becomes one part per primitive. `ModelDef.brightness` multiplies albedo (2.1 default for mod's dark
// textures, 1 for ours). mod ships no commander or radar tower model: commander is trooper mesh at 3x scale
// (with radio pack and pauldrons). No unit uses JSON box models any more (format remains).
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

/// A map's building theme (`assets/data/themes.json`): paints for plaster walls and coloured roofs, and
/// which models may stand in for a building type (`looks`: type id -> model ids).
#[derive(Clone, Default)]
pub struct Theme {
    pub walls: Vec<[f32; 3]>,
    pub roofs: Vec<[f32; 3]>,
    pub looks: HashMap<String, Vec<String>>,
}

/// The building themes of `assets/data/themes.json` that maps can pick (id, English name), for the editor.
pub const THEMES: [(&str, &str); 6] = [("default", "Default"), ("adriatic", "Adriatic"), ("central", "Central European"), ("village", "Village"), ("soviet", "Soviet"), ("industrial", "Industrial")];

impl Theme {
    pub fn load(name: &str) -> Theme {
        let read = || -> Option<serde_json::Value> { serde_json::from_str(&std::fs::read_to_string(asset_root().join("data/themes.json")).ok()?).ok() };
        let json = read();
        let themes = json.as_ref().and_then(|j| j.get("themes"));
        let pick = themes.and_then(|t| t.get(name).or_else(|| t.get("default")));
        let colours = |v: Option<&serde_json::Value>| -> Vec<[f32; 3]> {
            v.and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|c| serde_json::from_value::<[f32; 3]>(c.clone()).ok()).collect()).unwrap_or_default()
        };
        let mut t = Theme { walls: colours(pick.and_then(|p| p.get("walls"))), roofs: colours(pick.and_then(|p| p.get("roofs"))), looks: HashMap::new() };
        if let Some(l) = pick.and_then(|p| p.get("looks")).and_then(|l| l.as_object()) {
            for (k, v) in l {
                if let Ok(list) = serde_json::from_value::<Vec<String>>(v.clone()) {
                    t.looks.insert(k.clone(), list);
                }
            }
        }
        if t.walls.is_empty() {
            t.walls = vec![[0.9, 0.86, 0.78], [0.82, 0.85, 0.88]];
        }
        if t.roofs.is_empty() {
            t.roofs = vec![[0.6, 0.28, 0.2], [0.3, 0.33, 0.38]];
        }
        t
    }
}

fn luminance(c: [f32; 3]) -> f32 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}

fn saturation(c: [f32; 3]) -> f32 {
    let mx = c[0].max(c[1]).max(c[2]);
    if mx > 0.001 { (mx - c[0].min(c[1]).min(c[2])) / mx } else { 0.0 }
}

/// How many looks of one building type there are at high detail.
const BUILDING_VARIANTS: u32 = 6;

/// Which texels of a model's albedo belong to roofs (255, faces turned up) and which to walls (120),
/// by rasterising its triangles in UV space; 0 where nothing is drawn. Upright faces textured like the
/// roof and standing on it count as roof too: dormer cheeks carry strips of tiles.
fn roof_mask(parts: &[ModelPart], meshes: &Assets<Mesh>, albedo: &image::RgbaImage) -> Vec<u8> {
    use bevy::mesh::VertexAttributeValues as V;
    let (w, h) = albedo.dimensions();
    let mut mask = vec![0u8; (w * h) as usize];
    // Texels inside a triangle given by its UV corners (in texels).
    let texels = |q: &[Vec2], out: &mut Vec<u32>| {
        out.clear();
        let (lo, hi) = (q[0].min(q[1]).min(q[2]), q[0].max(q[1]).max(q[2]));
        let den = (q[1].y - q[2].y) * (q[0].x - q[2].x) + (q[2].x - q[1].x) * (q[0].y - q[2].y);
        if den.abs() < 1e-6 {
            return;
        }
        for y in (lo.y.floor().max(0.0) as u32)..=(hi.y.ceil().max(0.0) as u32).min(h - 1) {
            for x in (lo.x.floor().max(0.0) as u32)..=(hi.x.ceil().max(0.0) as u32).min(w - 1) {
                let c = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let a = ((q[1].y - q[2].y) * (c.x - q[2].x) + (q[2].x - q[1].x) * (c.y - q[2].y)) / den;
                let b = ((q[2].y - q[0].y) * (c.x - q[2].x) + (q[0].x - q[2].x) * (c.y - q[2].y)) / den;
                let g = 1.0 - a - b;
                if a >= -0.03 && b >= -0.03 && g >= -0.03 {
                    out.push(y * w + x);
                }
            }
        }
    };
    let mut upright: Vec<([Vec2; 3], f32, f32)> = Vec::new();
    let (mut roof_lo, mut roof_hi) = (f32::MAX, f32::MIN);
    let mut buf = Vec::new();
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
            // Steep roofs count too: a mansard's hipped corners and church spires lean ~65 deg (n.y ~0.43).
            let roof = n.y.abs() > 0.3;
            let q: Vec<Vec2> = t.iter().map(|&i| Vec2::new(uv[i as usize][0] * w as f32, uv[i as usize][1] * h as f32)).collect();
            let (low, high) = (p[0].y.min(p[1].y).min(p[2].y), p[0].y.max(p[1].y).max(p[2].y));
            if roof {
                // Pitched faces only: floors and flat slabs lie at every height.
                if n.y.abs() < 0.97 {
                    roof_lo = roof_lo.min(low);
                    roof_hi = roof_hi.max(high);
                }
            } else {
                upright.push(([q[0], q[1], q[2]], low, high));
            }
            texels(&q, &mut buf);
            for &i in &buf {
                mask[i as usize] = if roof { 255 } else { 120 };
            }
        }
    }
    // Colour of the roof (its coloured texels) and of the walls below it.
    let rgb = |i: u32| -> Vec3 {
        let px = &albedo.as_raw()[i as usize * 4..];
        Vec3::new(px[0] as f32, px[1] as f32, px[2] as f32) / 255.0
    };
    let coloured = |c: Vec3| c.max_element() - c.min_element() > 0.15 * c.max_element();
    let (mut roof_sum, mut roof_sq, mut roof_n) = (Vec3::ZERO, 0.0f32, 0u32);
    for i in 0..w * h {
        let c = rgb(i);
        if mask[i as usize] == 255 && coloured(c) {
            roof_sum += c;
            roof_sq += c.length_squared();
            roof_n += 1;
        }
    }
    let (mut wall_sum, mut wall_n) = (Vec3::ZERO, 0u32);
    for (q, low, _) in &upright {
        if *low < roof_lo - 0.05 {
            texels(q, &mut buf);
            for &i in &buf {
                wall_sum += rgb(i);
                wall_n += 1;
            }
        }
    }
    if roof_n < 50 || wall_n < 50 {
        return mask;
    }
    let roof = roof_sum / roof_n as f32;
    let spread = (roof_sq / roof_n as f32 - roof.length_squared()).max(0.0).sqrt().max(0.03);
    let wall = wall_sum / wall_n as f32;
    // An upright face up on the roof (a dormer's cheek: not the wall below, not a gable reaching the
    // ridge) of which most texels have the roof's colour rather than the walls' is roof.
    let ridge = roof_hi - 0.08 * (roof_hi - roof_lo);
    for (q, low, high) in upright {
        if low < roof_lo - 0.05 || high > ridge {
            continue;
        }
        texels(&q, &mut buf);
        let like = buf.iter().filter(|&&i| {
            let c = rgb(i);
            coloured(c) && c.distance(roof) < (1.5 * spread).min(c.distance(wall))
        });
        if !buf.is_empty() && like.count() * 10 >= buf.len() * 7 {
            for &i in &buf {
                mask[i as usize] = 255;
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

/// Reads a mesh's positions, normals, uvs and indices.
type MeshData = (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 2]>, Vec<u32>);
fn mesh_data(src: &Mesh) -> Option<MeshData> {
    use bevy::mesh::VertexAttributeValues as V;
    let V::Float32x3(pos) = src.attribute(Mesh::ATTRIBUTE_POSITION)? else { return None };
    let V::Float32x3(nrm) = src.attribute(Mesh::ATTRIBUTE_NORMAL)? else { return None };
    let V::Float32x2(uv) = src.attribute(Mesh::ATTRIBUTE_UV_0)? else { return None };
    let idx: Vec<u32> = match src.indices()? {
        Indices::U32(i) => i.clone(),
        Indices::U16(i) => i.iter().map(|&x| x as u32).collect(),
    };
    Some((pos.clone(), nrm.clone(), uv.clone(), idx))
}

/// Ultra detail for a house part: the pitched roof's rim is pushed out into an eave, tapering to the
/// ridge, and a sill box is added under every window found in the albedo (a small dark or bluish
/// rectangle on a wall island: `walls` marks wall texels with 120). Same material and one mesh.
fn detail_mesh(src: &Mesh, albedo: &image::RgbaImage, walls: &[u8], brightness: f32) -> Option<Mesh> {
    let (mut pos, mut nrm, mut uv, mut idx) = mesh_data(src)?;
    let (w, h) = albedo.dimensions();
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in &pos {
        lo = lo.min(Vec3::from_array(*p));
        hi = hi.max(Vec3::from_array(*p));
    }
    let ext = hi - lo;
    if ext.y < 0.3 || ext.x < 0.3 || ext.z < 0.3 {
        return None;
    }
    let big = ext.x.max(ext.z);
    let centre = (lo + hi) * 0.5;
    // Eaves: vertices of pitched-roof triangles in the upper part of the model.
    let orig = pos.clone();
    let tri_n = |t: &[u32]| (Vec3::from_array(orig[t[1] as usize]) - Vec3::from_array(orig[t[0] as usize])).cross(Vec3::from_array(orig[t[2] as usize]) - Vec3::from_array(orig[t[0] as usize])).normalize_or_zero();
    let mut roof: Vec<u32> = vec![];
    for t in idx.chunks_exact(3) {
        let n = tri_n(t);
        let cy = (pos[t[0] as usize][1] + pos[t[1] as usize][1] + pos[t[2] as usize][1]) / 3.0;
        if n.y > 0.35 && n.y < 0.97 && cy > lo.y + ext.y * 0.4 {
            roof.extend_from_slice(t);
        }
    }
    if !roof.is_empty() {
        let (eave, ridge) = roof.iter().fold((f32::MAX, f32::MIN), |(a, b), &i| (a.min(pos[i as usize][1]), b.max(pos[i as usize][1])));
        if ridge - eave > 0.05 {
            let mut moved = vec![false; pos.len()];
            for &i in &roof {
                let i = i as usize;
                if moved[i] {
                    continue;
                }
                moved[i] = true;
                let taper = 1.0 - ((pos[i][1] - eave) / (ridge - eave)).clamp(0.0, 1.0);
                let out = Vec2::new(pos[i][0] - centre.x, pos[i][2] - centre.z).normalize_or_zero() * big * 0.055 * taper * taper;
                pos[i][0] += out.x;
                pos[i][2] += out.y;
            }
        }
    }
    // Window sills.
    let lum_px = |x: u32, y: u32| {
        let p = albedo.get_pixel(x, y);
        [p[0] as f32 / 255.0 * brightness, p[1] as f32 / 255.0 * brightness, p[2] as f32 / 255.0 * brightness]
    };
    let glass = |x: u32, y: u32| -> bool {
        let c = lum_px(x, y);
        let l = luminance(c);
        walls[(y * w + x) as usize] == 120 && ((c[2] >= c[0] * 0.9 && l < 0.5) || (c[2] > c[0] + 0.04 && l > 0.55 && saturation(c) > 0.08))
    };
    let mut seen = vec![false; (w * h) as usize];
    let mut sills = 0;
    let base_tris: Vec<[u32; 3]> = idx.chunks_exact(3).map(|t| [t[0], t[1], t[2]]).collect();
    for y0 in 0..h {
        for x0 in 0..w {
            if seen[(y0 * w + x0) as usize] || !glass(x0, y0) {
                continue;
            }
            // Flood fill the window.
            let (mut x_lo, mut x_hi, mut y_lo, mut y_hi, mut count) = (x0, x0, y0, y0, 0u32);
            let mut stack = vec![(x0, y0)];
            seen[(y0 * w + x0) as usize] = true;
            while let Some((x, y)) = stack.pop() {
                count += 1;
                (x_lo, x_hi, y_lo, y_hi) = (x_lo.min(x), x_hi.max(x), y_lo.min(y), y_hi.max(y));
                for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
                    if nx < w && ny < h && !seen[(ny * w + nx) as usize] && glass(nx, ny) {
                        seen[(ny * w + nx) as usize] = true;
                        stack.push((nx, ny));
                    }
                }
            }
            let (bw, bh) = (x_hi - x_lo + 1, y_hi - y_lo + 1);
            if !(5..=90).contains(&bw) || !(6..=110).contains(&bh) || (count as f32) < 0.5 * (bw * bh) as f32 || !(0.6..=3.0).contains(&(bh as f32 / bw as f32)) || sills >= 90 {
                continue;
            }
            // The wall triangle under the window's bottom middle.
            let probe = Vec2::new((x_lo + x_hi) as f32 * 0.5 + 0.5, y_hi as f32 + 0.5);
            let uvp = |u: Vec2| Vec2::new(u.x / w as f32, u.y / h as f32);
            for t in &base_tris {
                let q: Vec<Vec2> = t.iter().map(|&i| Vec2::from_array(uv[i as usize])).collect();
                let bary = |p: Vec2| -> Option<[f32; 3]> {
                    let den = (q[1].y - q[2].y) * (q[0].x - q[2].x) + (q[2].x - q[1].x) * (q[0].y - q[2].y);
                    if den.abs() < 1e-9 {
                        return None;
                    }
                    let a = ((q[1].y - q[2].y) * (p.x - q[2].x) + (q[2].x - q[1].x) * (p.y - q[2].y)) / den;
                    let b = ((q[2].y - q[0].y) * (p.x - q[2].x) + (q[0].x - q[2].x) * (p.y - q[2].y)) / den;
                    Some([a, b, 1.0 - a - b])
                };
                let Some(bc) = bary(uvp(probe)) else { continue };
                if bc.iter().any(|&v| v < -1e-4) {
                    continue;
                }
                let n = tri_n(t);
                if n.y.abs() > 0.3 {
                    continue;
                }
                let at = |p: Vec2| -> Option<Vec3> {
                    let b = bary(uvp(p))?;
                    Some(b.iter().zip(t).map(|(k, &i)| Vec3::from_array(orig[i as usize]) * *k).sum())
                };
                let (Some(pl), Some(pr)) = (at(Vec2::new(x_lo as f32, y_hi as f32 + 1.0)), at(Vec2::new(x_hi as f32 + 1.0, y_hi as f32 + 1.0))) else { break };
                let span = pr - pl;
                let wl = span.length();
                if wl < 0.02 || wl > big * 0.4 {
                    break;
                }
                let side = Vec3::new(span.x, 0.0, span.z).normalize_or_zero();
                if side == Vec3::ZERO {
                    break;
                }
                let (len, depth, hgt) = (wl * 1.3, wl * 0.2, wl * 0.11);
                let c = (pl + pr) * 0.5 + n * depth * 0.5 - Vec3::Y * hgt * 0.5;
                let (hs, hn, hu) = (side * len * 0.5, n * depth * 0.5, Vec3::Y * hgt * 0.5);
                // Texture: the plaster just under the window.
                let tex = [probe.x / w as f32, ((y_hi + 3).min(h - 1)) as f32 / h as f32];
                // (normal, centre offset, half-extents spanning the face)
                let faces: [(Vec3, Vec3, Vec3, Vec3); 5] = [(n, hn, hs, hu), (Vec3::Y, hu, hs, hn), (-side, -hs, hn, hu), (side, hs, hn, hu), (-n, -hn, hs, hu)];
                for (nn, off, a, b) in faces {
                    let first = pos.len() as u32;
                    for (sa, sb) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                        pos.push((c + off + a * sa + b * sb).to_array());
                        nrm.push(nn.to_array());
                        uv.push(tex);
                    }
                    // Wound to face along nn.
                    let e1 = Vec3::from_array(pos[first as usize + 1]) - Vec3::from_array(pos[first as usize]);
                    let e2 = Vec3::from_array(pos[first as usize + 2]) - Vec3::from_array(pos[first as usize]);
                    if e1.cross(e2).dot(nn) >= 0.0 {
                        idx.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3]);
                    } else {
                        idx.extend_from_slice(&[first, first + 2, first + 1, first, first + 3, first + 2]);
                    }
                }
                sills += 1;
                break;
            }
        }
    }
    Some(
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_indices(Indices::U32(idx)),
    )
}

impl ModelCache {
    /// Switches to a map's building theme (no theme: the default one); what was made for another theme is dropped.
    pub fn set_theme(&mut self, name: &str) {
        if self.theme_name.as_deref() != Some(name) {
            self.theme = Theme::load(name);
            self.theme_name = Some(name.to_string());
            self.mats.clear();
            self.ruin_mats.clear();
            self.ruins.clear();
            self.detailed.clear();
        }
    }

    /// The model that stands in for building type `def` (the theme's choice for this building, `variant`).
    pub fn look(&self, defs: &Defs, def: DefId) -> DefId {
        if self.detail < 2 {
            return def;
        }
        let Some(list) = self.theme.looks.get(&defs.get(def).id) else { return def };
        let ids: Vec<DefId> = list.iter().filter_map(|n| defs.id(n)).filter(|&i| defs.get(i).footprint == defs.get(def).footprint && defs.get(i).model.mesh.is_some()).collect();
        if ids.is_empty() {
            return def;
        }
        ids[(lattice(9, 9, self.variant) * ids.len() as f32) as usize % ids.len()]
    }

    /// Ultra: the part meshes of a house with roofs that overhang and sills under its windows (`detail_mesh`).
    fn detailed_meshes(&mut self, defs: &Defs, def: DefId, model: &Model, meshes: &mut Assets<Mesh>) -> Vec<Option<Handle<Mesh>>> {
        if self.detail < 3 || !defs.get(def).has_category("residential") {
            return vec![];
        }
        if let Some(v) = self.detailed.get(&def) {
            return v.clone();
        }
        let albedo = defs.get(def).model.albedo.as_deref().and_then(read_rgba);
        let made: Vec<Option<Mesh>> = model
            .parts
            .iter()
            .map(|p| {
                let src = p.mesh.as_ref().and_then(|h| meshes.get(h))?;
                let walls = roof_mask(std::slice::from_ref(p), meshes, albedo.as_ref()?);
                detail_mesh(src, albedo.as_ref()?, &walls, defs.get(def).model.brightness)
            })
            .collect();
        let handles: Vec<Option<Handle<Mesh>>> = made.into_iter().map(|m| m.map(|m| meshes.add(m))).collect();
        self.detailed.insert(def, handles.clone());
        handles
    }

    /// The simplified far-away mesh of every part of a model (`None` where the part is small or does not
    /// get much smaller), made once per model.
    fn far_meshes(&mut self, defs: &Defs, def: DefId, model: &Model, meshes: &mut Assets<Mesh>) -> Vec<Option<Handle<Mesh>>> {
        let key = (def, self.is_low(defs, def));
        if let Some(v) = self.far.get(&key) {
            return v.clone();
        }
        let made: Vec<Option<Mesh>> = model.parts.iter().map(|p| p.mesh.as_ref().and_then(|h| meshes.get(h)).and_then(super::decimate::simplify)).collect();
        let handles: Vec<Option<Handle<Mesh>>> = made.into_iter().map(|m| m.map(|m| meshes.add(m))).collect();
        self.far.insert(key, handles.clone());
        handles
    }

    /// The skirt of a building model, made once per model: every wall edge standing on the ground (y = 0 in the
    /// model) of a part that never moves, carried straight down to y = -1 with the texture of the wall's bottom
    /// row stretched along it. In the model root's space, one mesh per part (it takes that part's material);
    /// `view::foundation_fit` stretches it to the ground under a building on a slope, so the model's own walls
    /// run on down instead of the whole model being stretched.
    fn skirt_meshes(&mut self, defs: &Defs, def: DefId, model: &Model, meshes: &mut Assets<Mesh>) -> Vec<(usize, Handle<Mesh>)> {
        let key = (def, self.is_low(defs, def));
        if let Some(v) = self.skirts.get(&key) {
            return v.clone();
        }
        let mut glob: Vec<Transform> = Vec::with_capacity(model.parts.len());
        let mut out = vec![];
        for (pi, part) in model.parts.iter().enumerate() {
            let g = part.parent.map_or(part.local, |p| glob[p].mul_transform(part.local));
            glob.push(g);
            let moves = part.spin.is_some() || model.anims.iter().any(|a| a.tracks.iter().any(|t| t.0 == pi));
            let Some(m) = part.mesh.as_ref().and_then(|h| meshes.get(h)).filter(|_| !moves) else { continue };
            if let Some(sk) = skirt_of(m, &g) {
                out.push((pi, meshes.add(sk)));
            }
        }
        self.skirts.insert(key, out.clone());
        out
    }

    pub fn init(&mut self, materials: &mut Assets<StandardMaterial>) {
        let ghost = |c: Color| StandardMaterial { base_color: c, alpha_mode: AlphaMode::Blend, unlit: true, ..default() };
        self.ghost_ok = materials.add(ghost(Color::srgba(0.3, 1.0, 0.4, 0.45)));
        self.ghost_bad = materials.add(ghost(Color::srgba(1.0, 0.25, 0.2, 0.45)));
        self.ghost_queued = materials.add(ghost(Color::srgba(0.5, 0.8, 1.0, 0.3)));
    }

    /// Whether this building shows its low-poly model.
    pub fn is_low(&self, defs: &Defs, def: DefId) -> bool {
        self.low_poly && defs.get(def).model.mesh_low.is_some()
    }

    pub fn model(&mut self, defs: &Defs, def: DefId, meshes: &mut Assets<Mesh>) -> &Model {
        let low = self.is_low(defs, def);
        if !self.models.contains_key(&(def, low)) {
            let d = defs.get(def);
            let md: &ModelDef = &d.model;
            let mut model = Model { parts: vec![], scale: md.scale, anims: vec![] };
            let (mesh_p, albedo_p, _) = md.paths(low);
            if let Some(mesh) = mesh_p {
                match load_glb(&asset_root().join(mesh), meshes, md.normal.is_some() && !low, albedo_p.is_some()) {
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
            self.models.insert((def, low), model);
        }
        &self.models[&(def, low)]
    }

    /// Textured materials of a unit type in an owner colour (index into `palette`).
    pub fn unit_mats(&mut self, defs: &Defs, def: DefId, color: u8, rgb: [f32; 3], parts: &[ModelPart], meshes: &Assets<Mesh>, images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>) -> UnitMats {
        let high = self.detail >= 2;
        // Neutral map buildings differ one from the next (colour, wear); our own keep the team's look.
        let var = if high && defs.get(def).has_category("civilian") { 1 + (self.variant.wrapping_mul(2654435761) >> 16) % BUILDING_VARIANTS } else { 0 } as u8;
        let low = self.is_low(defs, def);
        let var = if low { var | 0x80 } else { var };
        // Ultra: glass and metal are glossy and reflect the sky (`sky::env_light`).
        let shine = self.detail >= 3;
        let var = if shine { var | 0x40 } else { var };
        if let Some(m) = self.mats.get(&(def, color, var)) {
            return m.clone();
        }
        let md = &defs.get(def).model;
        let mut tex = None;
        let mut gloss = None;
        let mut plain_tex = None;
        let (_, albedo_p, mask_p) = md.paths(low);
        if let Some(mut albedo) = albedo_p.map(|s| s.as_str()).and_then(read_rgba) {
            let mask = mask_p.map(|s| s.as_str()).and_then(read_rgba).filter(|m| m.dimensions() == albedo.dimensions());
            // From the texture as drawn, before brightness, paints and wear.
            if shine {
                let d = defs.get(def);
                let vehicle = !d.is_structure() && !d.is_infantry();
                // Buildings: only texels on walls can be windows (slate roofs are blue-grey too).
                let surfaces = if d.is_structure() { roof_mask(parts, meshes, &albedo) } else { vec![] };
                gloss = Some(gloss_map(&albedo, vehicle, &surfaces));
            }
            let width = albedo.width();
            let themed = defs.get(def).has_category("themed");
            let roofs = if var > 0 && !themed { roof_mask(parts, meshes, &albedo) } else { vec![] };
            let wall_paint = self.theme.walls[(var as usize * 3 + def as usize) % self.theme.walls.len()];
            let roof_paint = self.theme.roofs[(var as usize * 5 + def as usize * 2 + 1) % self.theme.roofs.len()];
            // Mean brightness of this model's coloured roof tiles and of its plaster: the paints replace the
            // hue and keep each texel's own light and dark (tile edges, stains, window frames).
            let (mut roof_sum, mut roof_n, mut wall_sum, mut wall_n) = (0.0f32, 0u32, 0.0f32, 0u32);
            if var > 0 {
                for (i, px) in albedo.pixels().enumerate() {
                    let c = [px[0] as f32 / 255.0 * md.brightness, px[1] as f32 / 255.0 * md.brightness, px[2] as f32 / 255.0 * md.brightness];
                    let (l, sat) = (luminance(c), saturation(c));
                    if roofs.get(i).copied() == Some(255) {
                        if sat > 0.15 {
                            roof_sum += l;
                            roof_n += 1;
                        }
                    } else if l > 0.5 && sat < 0.3 {
                        wall_sum += l;
                        wall_n += 1;
                    }
                }
            }
            let roof_ref = if roof_n > 0 { (roof_sum / roof_n as f32).max(0.1) } else { 0.3 };
            let wall_ref = if wall_n > 0 { (wall_sum / wall_n as f32).max(0.3) } else { 0.8 };
            for (i, px) in albedo.pixels_mut().enumerate() {
                let (x, y) = (i as u32 % width, i as u32 / width);
                // The mod's albedo maps are dark (`brightness` 2.1 lifts them); paint the team areas.
                let b = md.brightness;
                let mut c = [px[0] as f32 / 255.0 * b, px[1] as f32 / 255.0 * b, px[2] as f32 / 255.0 * b];
                if var > 0 {
                    // This variant's paint and wear: a tint, blotchy weathering and rain streaks.
                    let sd = var as u32 * 7919 + def as u32 * 104729;
                    let (l, sat) = (luminance(c), saturation(c));
                    if themed {
                        // Models made for a theme keep their own materials; only a little tint.
                        for j in 0..3 {
                            c[j] *= 0.94 + 0.12 * lattice(j as i32 + 1, 0, sd);
                        }
                    } else if roofs.get(i).copied() == Some(255) {
                        if sat > 0.15 {
                            for j in 0..3 {
                                c[j] += (roof_paint[j] * l / roof_ref - c[j]) * 0.9;
                            }
                        }
                    } else if l > 0.5 && sat < 0.3 {
                        for j in 0..3 {
                            c[j] += (wall_paint[j] * l / wall_ref - c[j]) * 0.85;
                        }
                    }
                    let blot = vnoise(x as f32 / 22.0, y as f32 / 22.0, sd);
                    let streak = vnoise(x as f32 / 5.0, y as f32 / 70.0, sd ^ 77);
                    let wear = (0.84 + 0.26 * blot) * (1.0 - 0.22 * streak * streak) * (0.94 + 0.12 * lattice(x as i32, y as i32, sd));
                    for j in 0..3 {
                        c[j] = (c[j] * wear).min(0.97);
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
            // Glass is a tinted mirror: metal reflections take the base colour, and dark panes would
            // reflect next to nothing, so they are lightened towards a pale grey-blue.
            if let Some(g) = &gloss {
                // The plain (far) material keeps the panes as they are.
                plain_tex = Some(images.add(if high { to_image_mips(&albedo, true) } else { to_image(&albedo, true) }));
                for (px, m) in albedo.pixels_mut().zip(g.pixels()) {
                    if m[2] == GLASS_METAL {
                        for (j, t) in [190.0f32, 204.0, 220.0].iter().enumerate() {
                            px[j] = (px[j] as f32 * 0.35 + t * 0.65) as u8;
                        }
                    }
                }
            }
            tex = Some(images.add(if high { to_image_mips(&albedo, true) } else { to_image(&albedo, true) }));
        }
        let normal = md.normal.as_deref().filter(|_| !low).and_then(read_rgba).map(|n| images.add(if high { to_image_mips(&n, false) } else { to_image(&n, false) }));
        let base = StandardMaterial {
            base_color: if tex.is_some() { Color::WHITE } else { Color::srgb(rgb[0], rgb[1], rgb[2]) },
            base_color_texture: tex.clone(),
            normal_map_texture: normal,
            // With a gloss map its texels carry the values (scaled by these).
            perceptual_roughness: if gloss.is_some() { 1.0 } else { 0.55 },
            metallic: if gloss.is_some() { 1.0 } else { 0.25 },
            metallic_roughness_texture: gloss.map(|g| images.add(to_image_mips(&g, false))),
            ..default()
        };
        let building = StandardMaterial {
            base_color: Color::srgba(0.55, 1.0, 0.7, 0.55),
            base_color_texture: tex,
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.8,
            ..default()
        };
        // The glossy version is only shown close up (`gloss_sync`); the plain one is the material as
        // below ultra.
        let plain_gloss = base.metallic_roughness_texture.is_some();
        let plain = StandardMaterial { perceptual_roughness: 0.55, metallic: 0.25, metallic_roughness_texture: None, base_color_texture: plain_tex.or(base.base_color_texture.clone()), ..base.clone() };
        let normal = if plain_gloss { self.pair(materials.add(plain), materials.add(base)) } else { materials.add(base) };
        let m = UnitMats { normal, building: materials.add(building) };
        self.mats.insert((def, color, var), m.clone());
        m
    }

    /// Registers a plain material and its glossy close-up version; returns the one to show now.
    fn pair(&mut self, plain: Handle<StandardMaterial>, shiny: Handle<StandardMaterial>) -> Handle<StandardMaterial> {
        self.shiny.insert(plain.id(), shiny.clone());
        self.plain.insert(shiny.id(), plain.clone());
        if self.close { shiny } else { plain }
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
        // Ultra: parts painted "metal" are polished steel close up (`gloss_sync`).
        let metal = self.detail >= 3 && matches!(spec, ColorSpec::Name(n) if n == "metal");
        let key = (((c[0] * 255.0) as u32) << 16 | ((c[1] * 255.0) as u32) << 8 | (c[2] * 255.0) as u32 | (metal as u32) << 24, color);
        if let Some(h) = self.flat.get(&key) {
            return if self.close { self.shiny.get(&h.id()).unwrap_or(h).clone() } else { h.clone() };
        }
        let mat = StandardMaterial { base_color: Color::srgb(c[0], c[1], c[2]), perceptual_roughness: 0.75, ..default() };
        let plain = materials.add(mat.clone());
        self.flat.insert(key, plain.clone());
        if metal {
            self.pair(plain, materials.add(StandardMaterial { perceptual_roughness: 0.3, metallic: 0.8, ..mat }))
        } else {
            plain
        }
    }

    /// Shows the glossy materials (glass, bare metal: ultra building detail) on every model while
    /// the camera is close (`close`), the plain ones otherwise. Returns false if nothing changed.
    pub fn set_close(&mut self, close: bool, parts: &mut Query<&mut MeshMaterial3d<StandardMaterial>>) -> bool {
        if self.close == close {
            return false;
        }
        self.close = close;
        let map = if close { &self.shiny } else { &self.plain };
        if map.is_empty() {
            return true;
        }
        for mut m in parts.iter_mut() {
            if let Some(h) = map.get(&m.0.id()) {
                m.0 = h.clone();
            }
        }
        true
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
        let low = self.is_low(defs, def);
        if let Some(m) = self.wreck.get(&(def, low)) {
            return m.clone();
        }
        let tex = defs.get(def).model.paths(low).1.map(|s| s.as_str()).and_then(read_rgba).map(|a| images.add(to_image(&a, true)));
        let m = materials.add(StandardMaterial { base_color: Color::srgb(0.55, 0.45, 0.4), base_color_texture: tex, perceptual_roughness: 0.95, ..default() });
        self.wreck.insert((def, low), m.clone());
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

/// A part with a simplified mesh for far away (`decimate`): `model_lod_swap` puts the right one in its `Mesh3d`.
#[derive(Component)]
pub struct LodMesh {
    near: Handle<Mesh>,
    far: Handle<Mesh>,
    /// Camera distance from which the far mesh shows.
    dist: f32,
    is_far: bool,
}

/// The far mesh shows from this many unit radii of camera distance, within these limits (world units).
const LOD_RADII: f32 = 30.0;
const LOD_MIN: f32 = 60.0;
const LOD_MAX: f32 = 260.0;

/// Level of detail for models: parts beyond their distance show the simplified mesh, a little hysteresis
/// keeps one at the edge from flickering between the two.
pub fn model_lod_swap(cams: Query<&GlobalTransform, With<super::terrain::MainCamera>>, mut parts: Query<(&GlobalTransform, &mut Mesh3d, &mut LodMesh)>) {
    let Some(cam) = cams.iter().next().map(|c| c.translation()) else { return };
    for (t, mut mesh, mut lod) in &mut parts {
        let d2 = t.translation().distance_squared(cam);
        let far = if lod.is_far { d2 > (lod.dist * 0.93).powi(2) } else { d2 > (lod.dist * 1.07).powi(2) };
        if far != lod.is_far {
            lod.is_far = far;
            mesh.0 = if far { lod.far.clone() } else { lod.near.clone() };
        }
    }
}

/// Ground-level wall edges of a part mesh (placed by `g` in the model) carried down from y = 0 to y = -1
/// (`ModelCache::skirt_meshes`). Faces lower than `SKIRT_MIN_RISE` (aprons, kerbs) and flat faces are left out.
fn skirt_of(m: &Mesh, g: &Transform) -> Option<Mesh> {
    use bevy::mesh::VertexAttributeValues as V;
    const EPS: f32 = 0.03;
    const SKIRT_MIN_RISE: f32 = 0.1;
    let Some(V::Float32x3(pos)) = m.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    let nrm = match m.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(V::Float32x3(n)) => Some(n),
        _ => None,
    };
    let uv = match m.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(V::Float32x2(u)) => Some(u),
        _ => None,
    };
    let tan = match m.attribute(Mesh::ATTRIBUTE_TANGENT) {
        Some(V::Float32x4(t)) => Some(t),
        _ => None,
    };
    let idx: Vec<u32> = match m.indices() {
        Some(Indices::U32(i)) => i.clone(),
        Some(Indices::U16(i)) => i.iter().map(|&v| v as u32).collect(),
        None => (0..pos.len() as u32).collect(),
    };
    let wp: Vec<Vec3> = pos.iter().map(|p| g.transform_point(Vec3::from_array(*p))).collect();
    let (mut p_out, mut n_out, mut uv_out, mut t_out, mut i_out) = (vec![], vec![], vec![], vec![], vec![]);
    for t in idx.chunks_exact(3) {
        let [a, b, c] = [t[0] as usize, t[1] as usize, t[2] as usize];
        let face = (wp[b] - wp[a]).cross(wp[c] - wp[a]);
        if face.length_squared() < 1e-12 || face.normalize().y.abs() > 0.5 {
            continue;
        }
        for (i, j, k) in [(a, b, c), (b, c, a), (c, a, b)] {
            if wp[i].y.abs() > EPS || wp[j].y.abs() > EPS || wp[k].y < SKIRT_MIN_RISE {
                continue;
            }
            let n0 = p_out.len() as u32;
            // Top i, top j, bottom j, bottom i; the opposite corner is above the edge, so the quad turns the other way.
            for (v, y) in [(i, 0.0), (j, 0.0), (j, -1.0), (i, -1.0)] {
                p_out.push([wp[v].x, y, wp[v].z]);
                let n = nrm.map_or(face, |n| g.rotation * Vec3::from_array(n[v]));
                n_out.push(Vec3::new(n.x, 0.0, n.z).try_normalize().unwrap_or(Vec3::new(face.x, 0.0, face.z).normalize()).to_array());
                uv_out.push(uv.map_or([0.0, 0.0], |u| u[v]));
                if let Some(tn) = tan {
                    let d = g.rotation * Vec3::new(tn[v][0], tn[v][1], tn[v][2]);
                    t_out.push([d.x, d.y, d.z, tn[v][3]]);
                }
            }
            i_out.extend_from_slice(&[n0 + 1, n0, n0 + 3, n0 + 1, n0 + 3, n0 + 2]);
        }
    }
    if i_out.is_empty() {
        return None;
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, p_out)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, n_out)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv_out)
        .with_inserted_indices(Indices::U32(i_out));
    if tan.is_some() {
        mesh.insert_attribute(Mesh::ATTRIBUTE_TANGENT, t_out);
    }
    Some(mesh)
}

/// A building model's skirt (`ModelCache::skirt_meshes`): hidden until `view::foundation_fit` finds the
/// building standing on a slope.
#[derive(Component)]
pub struct Skirt;

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
    // What the map's theme shows for this building type (the type itself unless the theme says otherwise).
    let look = cache.look(defs, def);
    let model = cache.model(defs, look, meshes).clone();
    let mats = cache.unit_mats(defs, look, color, rgb, &model.parts, meshes, images, materials);
    let d = defs.get(def);
    let detailed = cache.detailed_meshes(defs, look, &model, meshes);
    let far = cache.far_meshes(defs, look, &model, meshes);
    // From this camera distance on the parts show their simplified meshes.
    let far_dist = (d.radius() * LOD_RADII).clamp(LOD_MIN, LOD_MAX);
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
        if let Some(mesh) = detailed.get(pi).and_then(|m| m.as_ref()).or(part.mesh.as_ref()) {
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
            if let Some(Some(f)) = far.get(pi) {
                e.insert(LodMesh { near: mesh.clone(), far: f.clone(), dist: far_dist, is_far: false });
            }
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
    if live && d.is_structure() && d.footprint > 0 && !d.is_fieldwork() {
        for (pi, mesh) in cache.skirt_meshes(defs, look, &model, meshes) {
            let mat = match &model.parts[pi].mat {
                PartMat::Unit if building => mats.building.clone(),
                PartMat::Unit => mats.normal.clone(),
                PartMat::Flat(spec) => cache.flat_mat(spec, color, rgb, materials),
            };
            let mat = if dim { cache.dim_mat(&mat, materials) } else { mat };
            let s = commands.spawn((Skirt, Mesh3d(mesh), MeshMaterial3d(mat), Transform::IDENTITY, Visibility::Hidden)).id();
            commands.entity(root).add_child(s);
        }
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Building models get skirts (their ground-level walls carried down), with valid indices.
    #[test]
    fn skirts_are_made() {
        let mut meshes = Assets::<Mesh>::default();
        let dir = asset_root().join("models");
        let (mut with, mut total) = (0, 0);
        for f in std::fs::read_dir(&dir).unwrap() {
            let p = f.unwrap().path();
            let name = p.file_stem().unwrap().to_string_lossy().to_string();
            if !p.extension().is_some_and(|e| e == "glb") || !["civ_", "hotel", "residential_tower", "sov_"].iter().any(|k| name.starts_with(k)) {
                continue;
            }
            let (parts, _) = load_glb(&p, &mut meshes, false, true).unwrap();
            total += 1;
            let mut quads = 0;
            for part in &parts {
                let Some(m) = part.mesh.as_ref().and_then(|h| meshes.get(h)) else { continue };
                if let Some(s) = skirt_of(m, &part.local) {
                    let Some(Indices::U32(idx)) = s.indices() else { panic!() };
                    assert!(idx.iter().all(|&i| (i as usize) < s.count_vertices()));
                    quads += idx.len() / 6;
                }
            }
            if quads > 0 {
                with += 1;
            }
            println!("{name}: {quads} skirt quads");
        }
        assert!(with * 2 > total, "{with} of {total}");
    }

    /// Every part mesh of every model simplifies into a valid mesh (indices in range, finite) and the far
    /// meshes together are much smaller; prints the totals.
    #[test]
    fn far_meshes_are_valid() {
        use bevy::mesh::{Indices, VertexAttributeValues};
        let mut meshes = Assets::<Mesh>::default();
        let dir = asset_root().join("models");
        let (mut before, mut after, mut simplified, mut total) = (0usize, 0usize, 0usize, 0usize);
        for f in std::fs::read_dir(&dir).unwrap() {
            let p = f.unwrap().path();
            if !p.extension().is_some_and(|e| e == "glb") {
                continue;
            }
            let (parts, _) = load_glb(&p, &mut meshes, false, true).unwrap();
            for part in &parts {
                let Some(m) = part.mesh.as_ref().and_then(|h| meshes.get(h)) else { continue };
                let tris = m.indices().map_or(0, |i| i.len() / 3);
                total += 1;
                before += tris;
                match crate::view::decimate::simplify(m) {
                    Some(s) => {
                        let Some(VertexAttributeValues::Float32x3(pos)) = s.attribute(Mesh::ATTRIBUTE_POSITION) else { panic!() };
                        let Some(Indices::U32(idx)) = s.indices() else { panic!() };
                        assert!(idx.iter().all(|&i| (i as usize) < pos.len()) && idx.len() % 3 == 0, "{}", p.display());
                        assert!(pos.iter().all(|p| p.iter().all(|x| x.is_finite())), "{}", p.display());
                        assert!(idx.len() / 3 < tris);
                        simplified += 1;
                        after += idx.len() / 3;
                    }
                    None => after += tris,
                }
            }
        }
        println!("parts {total}, simplified {simplified}; triangles {before} -> {after} ({:.0}%)", 100.0 * after as f32 / before.max(1) as f32);
        assert!(after < before);
    }

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

#[cfg(test)]
mod detail_tests {
    use super::*;

    /// Ultra detail finds windows in the real houses and adds geometry to them without breaking the mesh.
    #[test]
    fn ultra_houses_get_sills_and_eaves() {
        let mut meshes = Assets::<Mesh>::default();
        for name in ["civ_old_house1", "civ_corner_house1", "civ_house1", "civ_apartments2"] {
            let (parts, _) = load_glb(&asset_root().join(format!("models/{name}.glb")), &mut meshes, false, true).unwrap();
            let albedo = read_rgba(&format!("models/{name}_albedo.png")).unwrap();
            let p = parts.iter().find(|p| p.mesh.is_some()).unwrap();
            let before = meshes.get(p.mesh.as_ref().unwrap()).unwrap().count_vertices();
            let walls = roof_mask(std::slice::from_ref(p), &meshes, &albedo);
            let m = detail_mesh(meshes.get(p.mesh.as_ref().unwrap()).unwrap(), &albedo, &walls, 1.0).unwrap();
            println!("{name}: {} -> {} vertices", before, m.count_vertices());
            assert!(m.count_vertices() > before, "{name}: no sills found");
        }
    }

    /// Dormer cheeks are upright but textured with tiles: they take the roof's paint, the gable keeps the walls'.
    #[test]
    fn dormer_cheeks_are_roof() {
        let mut meshes = Assets::<Mesh>::default();
        for name in ["civ_corner_house1", "civ_old_house1"] {
            let (parts, _) = load_glb(&asset_root().join(format!("models/{name}.glb")), &mut meshes, false, true).unwrap();
            let albedo = read_rgba(&format!("models/{name}_albedo.png")).unwrap();
            let mask = roof_mask(&parts, &meshes, &albedo);
            let at = |u: f32, v: f32| mask[((v * albedo.height() as f32) as u32 * albedo.width() + (u * albedo.width() as f32) as u32) as usize];
            assert_eq!(at(370.0 / 512.0, 190.0 / 512.0), 255, "{name}: dormer cheek");
            assert_eq!(at(450.0 / 512.0, 300.0 / 512.0), 120, "{name}: gable");
        }
    }
}

