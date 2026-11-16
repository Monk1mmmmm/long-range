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
    mats: HashMap<(DefId, u8), UnitMats>,
    flat: HashMap<(u32, u8), Handle<StandardMaterial>>,
    wreck: HashMap<DefId, Handle<StandardMaterial>>,
    /// Dark copies of unit materials (remembered buildings destroyed out of sight).
    dim: HashMap<AssetId<StandardMaterial>, Handle<StandardMaterial>>,
    pub ghost_ok: Handle<StandardMaterial>,
    pub ghost_bad: Handle<StandardMaterial>,
    pub ghost_queued: Handle<StandardMaterial>,
}

fn load_glb(path: &std::path::Path, meshes: &mut Assets<Mesh>, want_tangents: bool) -> Result<(Vec<ModelPart>, Vec<Arc<Anim>>), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let g = gltf::Gltf::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let blob = g.blob.clone().unwrap_or_default();
    let mut parts: Vec<ModelPart> = vec![];
    // glTF node index -> part index.
    let mut part_of: HashMap<usize, usize> = HashMap::new();
    #[allow(clippy::too_many_arguments)]
    fn walk(node: gltf::Node, parent: Option<usize>, blob: &[u8], meshes: &mut Assets<Mesh>, want_tangents: bool, parts: &mut Vec<ModelPart>, part_of: &mut HashMap<usize, usize>) {
        let local = Transform::from_matrix(Mat4::from_cols_array_2d(&node.transform().matrix()));
        let mut mesh_handle = None;
        if let Some(m) = node.mesh() {
            if let Some(prim) = m.primitives().next() {
                let reader = prim.reader(|_| Some(blob));
                if let Some(pos) = reader.read_positions() {
                    let pos: Vec<[f32; 3]> = pos.collect();
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
                    mesh_handle = Some(meshes.add(mesh));
                }
            }
        }
        let me = parts.len();
        part_of.insert(node.index(), me);
        parts.push(ModelPart { name: node.name().unwrap_or("").to_string(), parent, local, mesh: mesh_handle, mat: PartMat::Unit, spin: None });
        for c in node.children() {
            walk(c, Some(me), blob, meshes, want_tangents, parts, part_of);
        }
    }
    for scene in g.document.scenes() {
        for node in scene.nodes() {
            walk(node, None, &blob, meshes, want_tangents, &mut parts, &mut part_of);
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
                match load_glb(&asset_root().join(mesh), meshes, md.normal.is_some()) {
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
    pub fn unit_mats(&mut self, defs: &Defs, def: DefId, color: u8, rgb: [f32; 3], images: &mut Assets<Image>, materials: &mut Assets<StandardMaterial>) -> UnitMats {
        if let Some(m) = self.mats.get(&(def, color)) {
            return m.clone();
        }
        let md = &defs.get(def).model;
        let mut tex = None;
        if let Some(mut albedo) = md.albedo.as_deref().and_then(read_rgba) {
            let mask = md.team_mask.as_deref().and_then(read_rgba).filter(|m| m.dimensions() == albedo.dimensions());
            let width = albedo.width();
            for (i, px) in albedo.pixels_mut().enumerate() {
                let (x, y) = (i as u32 % width, i as u32 / width);
                // The mod's albedo maps are dark; lift them and paint the team areas.
                let mut c = [px[0] as f32 / 255.0 * 2.1, px[1] as f32 / 255.0 * 2.1, px[2] as f32 / 255.0 * 2.1];
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
            tex = Some(images.add(to_image(&albedo, true)));
        }
        let normal = md.normal.as_deref().and_then(read_rgba).map(|n| images.add(to_image(&n, false)));
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
        self.mats.insert((def, color), m.clone());
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
    let mats = cache.unit_mats(defs, def, color, rgb, images, materials);
    let d = defs.get(def);
    let root = commands.spawn((ModelRoot { def, building }, Transform::from_scale(Vec3::splat(model.scale)), Visibility::Inherited)).id();
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
                let (parts, anims) = load_glb(&p, &mut meshes, false).unwrap();
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
