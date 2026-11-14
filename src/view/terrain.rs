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

const CHUNK: usize = 32;

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
pub fn spawn_scene(commands: &mut Commands, map: &MapData, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>, with_fog: bool) {
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
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    ));
    let fog_mat = materials.add(StandardMaterial {
        base_color_texture: Some(fog_img.clone()),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        depth_bias: 8.0,
        // The terrain UVs tile every 12 units; stretch them over the whole map for the fog.
        uv_transform: bevy::math::Affine2::from_scale(Vec2::splat(12.0 / world)),
        ..default()
    });
    commands.insert_resource(FogTexture(fog_img));
    for cy in (0..map.n).step_by(CHUNK) {
        for cx in (0..map.n).step_by(CHUNK) {
            let mesh = meshes.add(chunk_mesh(map, cx, cy));
            commands.spawn((Mesh3d(mesh.clone()), MeshMaterial3d(ground.clone()), Transform::IDENTITY, TerrainChunk { cx, cy }, GameScene));
            if with_fog {
                commands.spawn((Mesh3d(mesh), MeshMaterial3d(fog_mat.clone()), Transform::from_xyz(0.0, 0.06, 0.0), NotShadowCaster, FogOverlay, GameScene));
            }
        }
    }
    // Everything outside the map is a flat dark void (unlit, so it shows no lighting or shadow seams).
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(world * 12.0, world * 12.0))),
        MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.035, 0.045, 0.06), unlit: true, ..default() })),
        Transform::from_xyz(world * 0.5, map.water - 6.0, world * 0.5),
        NotShadowCaster,
        NotShadowReceiver,
        GameScene,
    ));
    // Water covers exactly the map; it takes no shadows (cascade borders showed up as rectangles on it).
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(world, world))),
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
