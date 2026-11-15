//! Static scene: heightmap terrain chunks, water, deposits, fog-of-war overlay, sun and camera.
use crate::map::*;
use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{CascadeShadowConfigBuilder, NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// Everything spawned for one game; despawned when leaving it.
#[derive(Component)]
pub struct GameScene;

#[derive(Component)]
pub struct Sun;

#[derive(Component)]
pub struct MainCamera;

#[derive(Component)]
pub struct FogOverlay;

/// Handle of the fog-of-war texture (one texel per map cell, alpha = darkness).
#[derive(Resource)]
pub struct FogTexture(pub Handle<Image>);

/// Terrain chunk size in cells (feature meshes use the same chunks).
pub const CHUNK: usize = 32;

/// A terrain mesh chunk starting at cell (cx, cy).
#[derive(Component)]
pub struct TerrainChunk {
    pub cx: usize,
    pub cy: usize,
}

#[derive(Component)]
pub struct WaterPlane;

/// Sits on the terrain at this height offset (deposit markers, editor objects).
#[derive(Component)]
pub struct GroundSnap(pub f32);

/// Rewrites the meshes of the chunks touched by a terrain edit, in place. Replacing the mesh
/// data keeps the entities and GPU resources, so nothing flickers (unlike respawning the scene).
pub fn update_chunks(map: &MapData, meshes: &mut Assets<Mesh>, chunks: &Query<(&TerrainChunk, &Mesh3d)>, center: V2, radius: f32) {
    let r = radius + CELL * 2.0;
    for (c, mesh) in chunks.iter() {
        let (x0, y0) = (c.cx as f32 * CELL, c.cy as f32 * CELL);
        let size = CHUNK as f32 * CELL;
        if center.x + r < x0 || center.y + r < y0 || center.x - r > x0 + size || center.y - r > y0 + size {
            continue;
        }
        if let Some(m) = meshes.get_mut(&mesh.0) {
            *m.into_inner() = chunk_mesh(map, c.cx, c.cy);
        }
    }
}

/// Rebuilds the terrain meshes around craters and earthworks made since the last frame, and keeps
/// what lies on the ground there (deposit markers, wrecks) on it.
pub fn terrain_sync(
    ground: Option<ResMut<crate::sim::Ground>>,
    map: Res<MapData>,
    mut meshes: ResMut<Assets<Mesh>>,
    chunks: Query<(&TerrainChunk, &Mesh3d)>,
    mut snapped: Query<(&GroundSnap, &mut Transform), Without<crate::sim::Wreck>>,
    mut wrecks: Query<(&crate::sim::Wreck, &mut Transform)>,
    mut flora: Option<ResMut<super::flora::FloraView>>,
) {
    let Some(mut ground) = ground else { return };
    if ground.touched.is_empty() && !ground.touched_all {
        return;
    }
    let touched = std::mem::take(&mut ground.touched);
    let all = std::mem::take(&mut ground.touched_all);
    let near = |p: V2, r: f32| all || touched.iter().any(|t| (t.x - p.x).abs() < r && (t.y - p.y).abs() < r);
    let size = CHUNK as f32 * CELL;
    // A touched spot stands for the vertices within a few units of it; normals reach one cell further.
    let margin = 6.0 + CELL * 2.0;
    for (c, mesh) in chunks.iter() {
        let (x0, y0) = (c.cx as f32 * CELL, c.cy as f32 * CELL);
        if !all && !touched.iter().any(|t| t.x > x0 - margin && t.y > y0 - margin && t.x < x0 + size + margin && t.y < y0 + size + margin) {
            continue;
        }
        if let Some(m) = meshes.get_mut(&mesh.0) {
            *m.into_inner() = chunk_mesh(&map, c.cx, c.cy);
        }
        // Trees and rocks there follow the ground.
        if let Some(f) = flora.as_mut() {
            let half = size * 0.5 - 0.01;
            f.mark_area(&map, v2(x0 + half, y0 + half), half);
        }
    }
    for (s, mut t) in &mut snapped {
        if near(v2(t.translation.x, t.translation.z), 14.0) {
            t.translation.y = map.height(t.translation.x, t.translation.z) + s.0;
        }
    }
    for (w, mut t) in &mut wrecks {
        if near(w.pos, 14.0) {
            t.translation.y = map.h(w.pos);
        }
    }
}

/// Terrain colour by height, slope and a little noise.
pub fn terrain_color(map: &MapData, x: f32, z: f32) -> [f32; 3] {
    let h = map.height(x, z);
    let nrm = map.normal(x, z);
    let slope = 1.0 - nrm.y;
    let n1 = noise(x * 0.09, z * 0.09, map.seed ^ 5);
    let n2 = noise(x * 0.5, z * 0.5, map.seed ^ 9);
    let dry = noise(x * 0.02, z * 0.02, map.seed ^ 21);
    let k = 0.82 + 0.2 * n1 + 0.1 * n2;
    let grass = [(0.24 + 0.22 * dry) * k, (0.42 + 0.04 * dry) * k, (0.14 + 0.04 * dry) * k];
    let rock = [0.36 * k, 0.33 * k, 0.30 * k];
    let sand = [0.52 * k, 0.46 * k, 0.33 * k];
    let mud = [0.20 * k, 0.19 * k, 0.15 * k];
    let mix = |a: [f32; 3], b: [f32; 3], t: f32| [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t];
    let mut c = grass;
    let shore = ((map.water + 0.9 - h) / 0.9).clamp(0.0, 1.0);
    c = mix(c, sand, shore);
    c = mix(c, mud, ((map.water - 0.2 - h) / 1.5).clamp(0.0, 1.0));
    c = mix(c, rock, ((slope - 0.10) / 0.16).clamp(0.0, 1.0));
    // High ground gets paler.
    mix(c, [0.45, 0.44, 0.40], ((h - 9.0) / 10.0).clamp(0.0, 0.6))
}

fn chunk_mesh(map: &MapData, cx: usize, cy: usize) -> Mesh {
    let (x1, y1) = ((cx + CHUNK).min(map.n), (cy + CHUNK).min(map.n));
    let (w, h) = (x1 - cx + 1, y1 - cy + 1);
    let mut pos = Vec::with_capacity(w * h);
    let mut nrm = Vec::with_capacity(w * h);
    let mut uv = Vec::with_capacity(w * h);
    let mut col = Vec::with_capacity(w * h);
    for y in cy..=y1 {
        for x in cx..=x1 {
            let (wx, wz) = (x as f32 * CELL, y as f32 * CELL);
            pos.push([wx, map.vertex(x, y), wz]);
            nrm.push(map.normal(wx, wz).to_array());
            uv.push([wx / 12.0, wz / 12.0]);
            let c = terrain_color(map, wx, wz);
            col.push([c[0], c[1], c[2], 1.0]);
        }
    }
    let mut idx = Vec::with_capacity((w - 1) * (h - 1) * 6);
    for y in 0..h - 1 {
        for x in 0..w - 1 {
            let i = (y * w + x) as u32;
            let (a, b, c, d) = (i, i + 1, i + w as u32, i + w as u32 + 1);
            idx.extend_from_slice(&[a, c, b, b, c, d]);
        }
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(Indices::U32(idx));
    mesh
}

/// Distances from the map border at which the apron has rows of vertices.
const APRON: [f32; 8] = [0.0, 2.0, 6.0, 14.0, 30.0, 62.0, 110.0, 170.0];

/// The land beyond the playable area: the border heights carried outwards and sloping down into
/// the sea, in dimmed colours. It meets the terrain vertex for vertex, so there is no gap.
fn apron_mesh(map: &MapData) -> Mesh {
    let (n, world) = (map.n, map.world());
    let reach = APRON[APRON.len() - 1];
    let sea = map.water - 5.0;
    let (mut pos, mut nrm, mut uv, mut col, mut idx) = (vec![], vec![], vec![], vec![], vec![]);
    // One vertex: `edge` is the nearest point on the border, `out` how far outside it lies.
    let mut vertex = |edge: V2, at: V2| -> u32 {
        let out = at.dist(edge);
        let t = (out / reach).clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        let wobble = (noise(at.x * 0.03, at.y * 0.03, map.seed ^ 77) - 0.5) * 9.0 * (t * (1.0 - t) * 4.0);
        let h0 = map.height(edge.x, edge.y);
        let c = terrain_color(map, edge.x, edge.y);
        let dim = 1.0 - 0.5 * t;
        pos.push([at.x, h0 + (sea - h0) * t + wobble, at.y]);
        nrm.push([0.0, 1.0, 0.0]);
        uv.push([at.x / 12.0, at.y / 12.0]);
        col.push([c[0] * dim * 0.5, c[1] * dim * 0.5, c[2] * dim * 0.56, 1.0]);
        pos.len() as u32 - 1
    };
    // A grid of vertex indices, `w` wide, turned into triangles that face up.
    let quads = |ids: &[u32], w: usize, flip: bool, idx: &mut Vec<u32>| {
        for r in 0..ids.len() / w - 1 {
            for c in 0..w - 1 {
                let (a, b, d, e) = (ids[r * w + c], ids[r * w + c + 1], ids[(r + 1) * w + c], ids[(r + 1) * w + c + 1]);
                if flip {
                    idx.extend_from_slice(&[a, b, d, b, e, d]);
                } else {
                    idx.extend_from_slice(&[a, d, b, b, d, e]);
                }
            }
        }
    };
    // Four sides: rows run along the border, columns go outwards.
    for (side, (ox, oz)) in [(0.0f32, -1.0f32), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)].into_iter().enumerate() {
        let mut ids = vec![];
        for i in 0..=n {
            let f = i as f32 * CELL;
            let edge = match side {
                0 => v2(f, 0.0),
                1 => v2(world, f),
                2 => v2(f, world),
                _ => v2(0.0, f),
            };
            for d in APRON {
                ids.push(vertex(edge, edge + v2(ox, oz) * d));
            }
        }
        quads(&ids, APRON.len(), side == 0 || side == 3, &mut idx);
    }
    // Four corners.
    for (cx, cz) in [(0.0f32, 0.0f32), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        let corner = v2(cx * world, cz * world);
        let (sx, sz) = (cx * 2.0 - 1.0, cz * 2.0 - 1.0);
        let mut ids = vec![];
        for a in APRON {
            for b in APRON {
                ids.push(vertex(corner, corner + v2(sx * b, sz * a)));
            }
        }
        quads(&ids, APRON.len(), sx * sz > 0.0, &mut idx);
    }
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(Indices::U32(idx));
    mesh
}

fn detail_texture() -> Image {
    const S: usize = 256;
    let mut data = Vec::with_capacity(S * S * 4);
    for y in 0..S {
        for x in 0..S {
            // Tileable noise: blend four shifted samples.
            let (fx, fy) = (x as f32 / S as f32, y as f32 / S as f32);
            let f = |ox: f32, oy: f32| {
                let (px, py) = ((fx + ox) * 24.0, (fy + oy) * 24.0);
                noise(px, py, 11) * 0.5 + noise(px * 3.1, py * 3.1, 12) * 0.3 + noise(px * 8.0, py * 8.0, 13) * 0.2
            };
            let v = f(0.0, 0.0) * (1.0 - fx) * (1.0 - fy) + f(-1.0, 0.0) * fx * (1.0 - fy) + f(0.0, -1.0) * (1.0 - fx) * fy + f(-1.0, -1.0) * fx * fy;
            let g = (95.0 + v * 160.0) as u8;
            data.extend_from_slice(&[g, g, g, 255]);
        }
    }
    let mut img = Image::new(Extent3d { width: S as u32, height: S as u32, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor { address_mode_u: ImageAddressMode::Repeat, address_mode_v: ImageAddressMode::Repeat, ..ImageSamplerDescriptor::linear() });
    img
}

/// Spawns terrain, water, deposits and lighting for `map`. `with_fog` adds the fog-of-war layer.
/// Returns the fog-of-war texture.
pub fn spawn_scene(commands: &mut Commands, map: &MapData, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>, with_fog: bool) -> Handle<Image> {
    let world = map.world();
    let ground = materials.add(StandardMaterial {
        base_color_texture: Some(images.add(detail_texture())),
        perceptual_roughness: 0.95,
        reflectance: 0.15,
        ..default()
    });
    let n = map.n as u32;
    let fog_img = images.add(Image::new_fill(
        Extent3d { width: n, height: n, depth_or_array_layers: 1 },
        TextureDimension::D2,
        // Alpha: darkness of the overlay. Colour: brightness of trees and rocks (see `flora`).
        &[255, 255, 255, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let fog_mat = materials.add(StandardMaterial {
        // Black: the overlay only uses the texture's alpha.
        base_color: Color::BLACK,
        base_color_texture: Some(fog_img.clone()),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        depth_bias: 8.0,
        // The terrain UVs tile every 12 units; stretch them over the whole map for the fog.
        uv_transform: bevy::math::Affine2::from_scale(Vec2::splat(12.0 / world)),
        ..default()
    });
    commands.insert_resource(FogTexture(fog_img.clone()));
    for cy in (0..map.n).step_by(CHUNK) {
        for cx in (0..map.n).step_by(CHUNK) {
            let mesh = meshes.add(chunk_mesh(map, cx, cy));
            commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(ground.clone()), Transform::IDENTITY, TerrainChunk { cx, cy }, GameScene));
            if with_fog {
                commands.spawn((Mesh3d(mesh), MeshMaterial3d(fog_mat.clone()), Transform::from_xyz(0.0, 0.06, 0.0), NotShadowCaster, FogOverlay, GameScene));
            }
        }
    }
    // Beyond the border the land carries on for a stretch and sinks into the sea.
    commands.spawn((Mesh3d(meshes.add(apron_mesh(map))), MeshMaterial3d(ground.clone()), Transform::IDENTITY, NotShadowCaster, GameScene));
    // Further out: a dark sea floor (unlit, so it shows no lighting or shadow seams) under the water.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(world * 12.0, world * 12.0))),
        MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.03, 0.07, 0.10), unlit: true, ..default() })),
        Transform::from_xyz(world * 0.5, map.water - 6.0, world * 0.5),
        NotShadowCaster,
        NotShadowReceiver,
        GameScene,
    ));
    // Water covers the map and the sea around it; it takes no shadows (cascade borders showed up
    // as rectangles on it).
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(world * 12.0, world * 12.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(0.07, 0.22, 0.32, 0.82),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.12,
            reflectance: 0.6,
            ..default()
        })),
        Transform::from_xyz(world * 0.5, map.water, world * 0.5),
        NotShadowCaster,
        NotShadowReceiver,
        WaterPlane,
        GameScene,
    ));
    spawn_deposits(commands, map, meshes, materials);
    commands.spawn((
        DirectionalLight { illuminance: 13000.0, shadow_maps_enabled: true, ..default() },
        Transform::from_xyz(0.0, 0.0, 0.0).looking_to(Vec3::new(-0.45, -0.8, -0.38), Vec3::Y),
        CascadeShadowConfigBuilder { num_cascades: 3, maximum_distance: 220.0, first_cascade_far_bound: 30.0, ..default() }.build(),
        Sun,
        GameScene,
    ));
    commands.insert_resource(GlobalAmbientLight { color: Color::srgb(0.75, 0.82, 1.0), brightness: 260.0, ..default() });
    fog_img
}

#[derive(Component)]
pub struct DepositMarker;

pub fn spawn_deposits(commands: &mut Commands, map: &MapData, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    let disc = meshes.add(Cylinder::new(0.8, 0.12));
    let core = meshes.add(Cylinder::new(0.35, 0.2));
    let m1 = materials.add(StandardMaterial { base_color: Color::srgb(0.32, 0.34, 0.36), metallic: 0.8, perceptual_roughness: 0.45, ..default() });
    let m2 = materials.add(StandardMaterial { base_color: Color::srgb(0.75, 0.8, 0.85), metallic: 0.9, perceptual_roughness: 0.25, ..default() });
    for d in &map.deposits {
        let y = map.h(*d);
        commands.spawn((Mesh3d(disc.clone()), MeshMaterial3d(m1.clone()), Transform::from_xyz(d.x, y + 0.03, d.y), NotShadowCaster, DepositMarker, GroundSnap(0.03), GameScene));
        commands.spawn((Mesh3d(core.clone()), MeshMaterial3d(m2.clone()), Transform::from_xyz(d.x, y + 0.05, d.y), NotShadowCaster, DepositMarker, GroundSnap(0.05), GameScene));
    }
}

/// A square picture of a map for the lobby: terrain, water, forests (dark green), rocks (grey) and
/// deposits (white), `side` x `side` RGBA pixels. Start positions are drawn on top by the lobby.
pub fn map_preview(map: &MapData, defs: &crate::data::Defs, side: usize) -> Vec<u8> {
    let side = side.max(8);
    let w = map.world().max(1.0);
    let k = w / side as f32;
    let mut px = vec![0f32; side * side * 3];
    for y in 0..side {
        for x in 0..side {
            let (wx, wz) = ((x as f32 + 0.5) * k, (y as f32 + 0.5) * k);
            let mut c = terrain_color(map, wx, wz);
            let h = map.height(wx, wz);
            if h < map.water {
                let t = ((map.water - h) / 2.0).clamp(0.3, 1.0);
                c = [c[0] * (1.0 - t) + 0.07 * t, c[1] * (1.0 - t) + 0.22 * t, c[2] * (1.0 - t) + 0.34 * t];
            }
            let nrm = map.normal(wx, wz);
            let l = (0.55 + 0.6 * nrm.dot(Vec3::new(0.45, 0.8, 0.38).normalize())).clamp(0.3, 1.3);
            let i = (y * side + x) * 3;
            for j in 0..3 {
                px[i + j] = c[j] * l;
            }
        }
    }
    // Trees and rocks: each darkens (or greys) the pixel it stands on a little, so forests read
    // as dark patches whatever the picture's scale.
    let class: Vec<Option<crate::data::FeatureClass>> = map.feature_kinds.iter().map(|id| defs.feature_id(id).map(|i| defs.rules.features[i as usize].class)).collect();
    let per = (CELL * CELL * 2.0 / (k * k)).min(1.0) * 0.5;
    for f in &map.features {
        let (x, y) = ((f.pos.x / k) as usize, (f.pos.y / k) as usize);
        if x >= side || y >= side {
            continue;
        }
        let i = (y * side + x) * 3;
        let (target, a) = match class.get(f.kind as usize).copied().flatten() {
            Some(crate::data::FeatureClass::Rock) => ([0.62, 0.6, 0.56], per),
            Some(_) => ([0.05, 0.16, 0.06], per),
            None => continue,
        };
        for j in 0..3 {
            px[i + j] += (target[j] - px[i + j]) * a;
        }
    }
    let mut out = Vec::with_capacity(side * side * 4);
    for i in 0..side * side {
        for j in 0..3 {
            out.push(((px[i * 3 + j] * 1.5).clamp(0.0, 1.0).powf(0.6) * 255.0) as u8);
        }
        out.push(255);
    }
    for d in &map.deposits {
        let (x, y) = ((d.x / k) as i32, (d.y / k) as i32);
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let (px, py) = (x + dx, y + dy);
            if px >= 0 && py >= 0 && (px as usize) < side && (py as usize) < side {
                let i = (py as usize * side + px as usize) * 4;
                out[i..i + 3].copy_from_slice(&[235, 240, 250]);
            }
        }
    }
    out
}

/// Small colour image of the map for the minimap (one pixel per cell, RGBA).
pub fn minimap_pixels(map: &MapData) -> Vec<u8> {
    let n = map.n;
    let mut out = Vec::with_capacity(n * n * 4);
    for y in 0..n {
        for x in 0..n {
            let (wx, wz) = ((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL);
            let mut c = terrain_color(map, wx, wz);
            let h = map.height(wx, wz);
            if h < map.water {
                let k = ((map.water - h) / 2.0).clamp(0.3, 1.0);
                c = [c[0] * (1.0 - k) + 0.07 * k, c[1] * (1.0 - k) + 0.22 * k, c[2] * (1.0 - k) + 0.34 * k];
            }
            // Simple hill shading.
            let nrm = map.normal(wx, wz);
            let l = (0.55 + 0.6 * nrm.dot(Vec3::new(0.45, 0.8, 0.38).normalize())).clamp(0.3, 1.3);
            for j in 0..3 {
                out.push(((c[j] * l * 1.5).clamp(0.0, 1.0).powf(0.6) * 255.0) as u8);
            }
            out.push(255);
        }
    }
    out
}
