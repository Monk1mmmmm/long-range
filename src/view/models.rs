//! Unit models: glTF meshes converted from the mod (one part per bone) or JSON box models,
//! plus the materials (team colours are baked into a copy of the albedo texture).
use crate::data::{asset_root, ColorSpec, DefId, Defs, ModelDef};
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use std::collections::HashMap;

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
    pub ghost_ok: Handle<StandardMaterial>,
    pub ghost_bad: Handle<StandardMaterial>,
    pub ghost_queued: Handle<StandardMaterial>,
}

fn load_glb(path: &std::path::Path, meshes: &mut Assets<Mesh>, want_tangents: bool) -> Result<Vec<ModelPart>, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let g = gltf::Gltf::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let blob = g.blob.clone().unwrap_or_default();
    let mut parts: Vec<ModelPart> = vec![];
    fn walk(node: gltf::Node, parent: Option<usize>, blob: &[u8], meshes: &mut Assets<Mesh>, want_tangents: bool, parts: &mut Vec<ModelPart>) {
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
        parts.push(ModelPart { name: node.name().unwrap_or("").to_string(), parent, local, mesh: mesh_handle, mat: PartMat::Unit, spin: None });
        for c in node.children() {
            walk(c, Some(me), blob, meshes, want_tangents, parts);
        }
    }
    for scene in g.document.scenes() {
        for node in scene.nodes() {
            walk(node, None, &blob, meshes, want_tangents, &mut parts);
        }
    }
    Ok(parts)
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
            let mut model = Model { parts: vec![], scale: md.scale };
            if let Some(mesh) = &md.mesh {
                match load_glb(&asset_root().join(mesh), meshes, md.normal.is_some()) {
                    Ok(parts) => model.parts = parts,
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
    let model = cache.model(defs, def, meshes).clone();
    let mats = cache.unit_mats(defs, def, color, rgb, images, materials);
    let d = defs.get(def);
    let root = commands.spawn((ModelRoot { def, building }, Transform::from_scale(Vec3::splat(model.scale)), Visibility::Inherited)).id();
    let mut ents: Vec<Entity> = Vec::with_capacity(model.parts.len());
    for part in &model.parts {
        let mut e = commands.spawn((part.local, Visibility::Inherited));
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
            e.insert((Mesh3d(mesh.clone()), MeshMaterial3d(mat)));
        }
        for (wi, w) in d.weapons.iter().enumerate() {
            if let Some(t) = &w.turret {
                if t.yaw_bone.as_deref() == Some(part.name.as_str()) {
                    e.insert(AimPart { weapon: wi, pitch: false, rest: part.local });
                } else if t.pitch_bone.as_deref() == Some(part.name.as_str()) {
                    e.insert(AimPart { weapon: wi, pitch: true, rest: part.local });
                }
            }
        }
        if let Some((axis, speed)) = part.spin {
            e.insert(SpinPart { axis, speed, rest: part.local });
        }
        let id = e.id();
        commands.entity(part.parent.map_or(root, |p| ents[p])).add_child(id);
        ents.push(id);
    }
    root
}
