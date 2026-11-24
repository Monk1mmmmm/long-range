//! Static scene: heightmap terrain chunks, water, deposits, fog-of-war overlay, sun and camera.
use crate::map::*;
use bevy::asset::RenderAssetUsages;
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

/// Brightness variation of the ground over tens of units (vertex colour; the textures come from
/// the surface material, see `surface.rs`).
fn ground_tint(map: &MapData, x: f32, z: f32) -> f32 {
    let n1 = noise(x * 0.09, z * 0.09, map.seed ^ 5);
    let n2 = noise(x * 0.5, z * 0.5, map.seed ^ 9);
    0.88 + 0.16 * n1 + 0.06 * n2
}

/// Colour of the ground without texture detail (minimap, lobby picture, land beyond the border).
pub fn terrain_color(map: &MapData, tint: &super::surface::Tint, x: f32, z: f32) -> [f32; 3] {
    let k = ground_tint(map, x, z);
    let c = tint.color(map, x, z);
    [c[0] * k, c[1] * k, c[2] * k]
}

fn chunk_mesh(map: &MapData, cx: usize, cy: usize) -> Mesh {
    let (x1, y1) = ((cx + CHUNK).min(map.nx), (cy + CHUNK).min(map.ny));
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
            let k = ground_tint(map, wx, wz);
            col.push([k, k, k, 1.0]);
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

/// The map's heights and colours (`[h, r, g, b]` per vertex), blurred more and more: level 0 is the
/// map itself, level `L` is blurred about `2^(L-1)` cells wide. The land beyond the border picks the
/// level that fits its distance from the edge.
struct BlurLevels {
    n: usize,
    ny: usize,
    levels: Vec<Vec<[f32; 4]>>,
}

impl BlurLevels {
    fn new(map: &MapData, count: usize) -> Self {
        let (n, ny) = map.dims();
        let (w, wz) = (n + 1, ny + 1);
        let tint = super::surface::Tint::new(map);
        let base: Vec<[f32; 4]> = (0..w * wz)
            .map(|i| {
                let c = terrain_color(map, &tint, (i % w) as f32 * CELL, (i / w) as f32 * CELL);
                [map.heights[i], c[0], c[1], c[2]]
            })
            .collect();
        // Mirrored at the ends, like the land outside (folded again for radii wider than the map).
        let fold = |j: i64, w: usize| -> usize {
            let p = (w as i64 - 1).max(1) * 2;
            let j = j.rem_euclid(p);
            (if j >= w as i64 { p - j } else { j }) as usize
        };
        // One box blur of radius `r` along rows or columns, with running sums.
        let pass = |v: &mut Vec<[f32; 4]>, r: usize, along_x: bool| {
            // `len` vertices along the blur, `lines` of them.
            let (len, lines) = if along_x { (w, wz) } else { (wz, w) };
            let mut sums = vec![[0f32; 4]; len + 2 * r + 1];
            for line in 0..lines {
                let at = |i: usize| if along_x { line * w + i } else { i * w + line };
                for k in 0..len + 2 * r {
                    let s = v[at(fold(k as i64 - r as i64, len))];
                    let p = sums[k];
                    sums[k + 1] = [p[0] + s[0], p[1] + s[1], p[2] + s[2], p[3] + s[3]];
                }
                let inv = 1.0 / (2 * r + 1) as f32;
                for i in 0..len {
                    let (a, b) = (sums[i], sums[i + 2 * r + 1]);
                    v[at(i)] = [(b[0] - a[0]) * inv, (b[1] - a[1]) * inv, (b[2] - a[2]) * inv, (b[3] - a[3]) * inv];
                }
            }
        };
        let mut levels = vec![base];
        for l in 1..count {
            let mut v = levels[l - 1].clone();
            let r = 1usize << (l - 1);
            // Two passes each way: close enough to a gaussian.
            for _ in 0..2 {
                pass(&mut v, r, true);
                pass(&mut v, r, false);
            }
            levels.push(v);
        }
        BlurLevels { n, ny, levels }
    }
    /// Bilinear sample of level `l` at a world position inside the map.
    fn sample(&self, l: usize, x: f32, z: f32) -> [f32; 4] {
        let (w, v) = (self.n + 1, &self.levels[l.min(self.levels.len() - 1)]);
        let fx = (x / CELL).clamp(0.0, self.n as f32 - 0.001);
        let fz = (z / CELL).clamp(0.0, self.ny as f32 - 0.001);
        let (ix, iz) = (fx as usize, fz as usize);
        let (tx, tz) = (fx - ix as f32, fz - iz as f32);
        let i = iz * w + ix;
        let k = [(1.0 - tx) * (1.0 - tz), tx * (1.0 - tz), (1.0 - tx) * tz, tx * tz];
        let mut out = [0f32; 4];
        for (j, id) in [i, i + 1, i + w, i + w + 1].into_iter().enumerate() {
            for (o, s) in out.iter_mut().zip(v[id]) {
                *o += s * k[j];
            }
        }
        out
    }
    /// Blurred `radius` world units wide (0 = sharp), between the two nearest levels.
    fn blurred(&self, radius: f32, x: f32, z: f32) -> [f32; 4] {
        let s = radius / CELL;
        let l = if s <= 1.0 { s } else { 1.0 + s.log2() };
        let l = l.clamp(0.0, (self.levels.len() - 1) as f32);
        let (l0, t) = (l.floor() as usize, l.fract());
        let a = self.sample(l0, x, z);
        if t < 1e-3 {
            return a;
        }
        let b = self.sample(l0 + 1, x, z);
        std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
    }
}

/// Folds a coordinate back into `0..=world` the way a mirror would, again and again.
fn mirror(x: f32, world: f32) -> f32 {
    let p = x.rem_euclid(2.0 * world);
    if p > world { 2.0 * world - p } else { p }
}

/// How wide the land beyond the border is blurred at this distance from the edge.
const APRON_BLUR: f32 = 0.1;

/// The land beyond the playable area, like BAR's: the map mirrored at its border on all four sides,
/// sharp at the edge and blurred more the further out it lies, a little darker and hazier with
/// distance, sinking into the sea at the far end. Its inner rim is the map border vertex for vertex.
fn apron_mesh(map: &MapData) -> Mesh {
    let (n, ny) = map.dims();
    let (world, world_z) = (map.world_x(), map.world_z());
    let reach = world.max(world_z).max(160.0);
    // Rows outwards: one cell apart at the edge, wider in step with the blur further out.
    let mut dist = vec![0.0f32];
    while *dist.last().unwrap() < reach {
        let d = *dist.last().unwrap();
        dist.push(d + (d * 0.12).max(CELL));
    }
    let levels = 2 + (reach * APRON_BLUR / CELL).max(1.0).log2().ceil() as usize;
    let blur = BlurLevels::new(map, levels);
    let sea = map.water - 4.0;
    let smooth = |t: f32| {
        let t = t.clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    // Height and colour at a point `d` outside the map.
    let sample = |x: f32, z: f32, d: f32| -> [f32; 4] {
        let mut s = blur.blurred(d * APRON_BLUR, mirror(x, world), mirror(z, world_z));
        let t = smooth((d - reach * 0.55) / (reach * 0.45));
        s[0] += (sea - s[0]) * t;
        s
    };
    let (mut pos, mut nrm, mut uv, mut col) = (vec![], vec![], vec![], vec![]);
    let mut vertex = |x: f32, z: f32, d: f32| -> u32 {
        let s = sample(x, z, d);
        let n = if d <= 0.0 {
            map.normal(x, z)
        } else {
            // Slopes over about the blur width, so far-out normals are as smooth as the heights.
            let e = (d * APRON_BLUR * 0.5).max(CELL * 0.5);
            let h = |x: f32, z: f32| sample(x, z, d)[0];
            Vec3::new(h(x - e, z) - h(x + e, z), 2.0 * e, h(x, z - e) - h(x, z + e)).normalize()
        };
        // No textures out here: the ground's average colours, darkened and greyed a little, and
        // a light haze far out.
        let c = [s[1], s[2], s[3]];
        let grey = c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11;
        let far = smooth(d / reach);
        let desat = 0.08 + 0.22 * far;
        let dim = 0.97 - 0.22 * far;
        let haze = 0.18 * far;
        let hz = [0.07, 0.085, 0.11];
        let out: [f32; 3] = std::array::from_fn(|i| {
            let v = (c[i] + (grey - c[i]) * desat) * dim;
            v + (hz[i] - v) * haze
        });
        pos.push([x, s[0], z]);
        nrm.push(n.to_array());
        uv.push([x / 12.0, z / 12.0]);
        col.push([out[0], out[1], out[2], 1.0]);
        pos.len() as u32 - 1
    };
    let rows = dist.len();
    let mut idx: Vec<u32> = vec![];
    // A grid of vertex indices, `w` wide, in triangles (turned to face up below).
    let quads = |ids: &[u32], w: usize, idx: &mut Vec<u32>| {
        for r in 0..ids.len() / w - 1 {
            for c in 0..w - 1 {
                let (a, b, d, e) = (ids[r * w + c], ids[r * w + c + 1], ids[(r + 1) * w + c], ids[(r + 1) * w + c + 1]);
                idx.extend_from_slice(&[a, d, b, b, d, e]);
            }
        }
    };
    // Four sides: one column per border vertex, rows going outwards.
    for (ox, oz) in [(0.0f32, -1.0f32), (1.0, 0.0), (0.0, 1.0), (-1.0, 0.0)] {
        // Border vertices along this side: along x on the near and far side, along z on the others.
        let along = if oz != 0.0 { n } else { ny };
        let mut ids = Vec::with_capacity((along + 1) * rows);
        for i in 0..=along {
            let f = i as f32 * CELL;
            let edge = match (ox as i32, oz as i32) {
                (0, -1) => v2(f, 0.0),
                (1, 0) => v2(world, f),
                (0, 1) => v2(f, world_z),
                _ => v2(0.0, f),
            };
            for &d in &dist {
                ids.push(vertex(edge.x + ox * d, edge.y + oz * d, d));
            }
        }
        quads(&ids, rows, &mut idx);
    }
    // Four corners.
    for (cx, cz) in [(0.0f32, 0.0f32), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)] {
        let (sx, sz) = (cx * 2.0 - 1.0, cz * 2.0 - 1.0);
        let mut ids = Vec::with_capacity(rows * rows);
        for &a in &dist {
            for &b in &dist {
                ids.push(vertex(cx * world + sx * b, cz * world_z + sz * a, (a * a + b * b).sqrt()));
            }
        }
        quads(&ids, rows, &mut idx);
    }
    // Every triangle faces up (same turn as the terrain chunks' triangles seen from above).
    for t in idx.chunks_exact_mut(3) {
        let [p, q, r] = [pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]];
        if (q[0] - p[0]) * (r[2] - p[2]) - (q[2] - p[2]) * (r[0] - p[0]) > 0.0 {
            t.swap(1, 2);
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

/// Spawns terrain, water, deposits and lighting for `map`. `with_fog` adds the fog-of-war layer.
/// Returns the fog-of-war texture.
pub fn spawn_scene(commands: &mut Commands, map: &MapData, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>, with_fog: bool) -> Handle<Image> {
    let (world, world_z) = (map.world_x(), map.world_z());
    let fog_img = images.add(Image::new_fill(
        Extent3d { width: map.nx as u32, height: map.ny as u32, depth_or_array_layers: 1 },
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
        uv_transform: bevy::math::Affine2::from_scale(Vec2::new(12.0 / world, 12.0 / world_z)),
        ..default()
    });
    commands.insert_resource(FogTexture(fog_img.clone()));
    for cy in (0..map.ny).step_by(CHUNK) {
        for cx in (0..map.nx).step_by(CHUNK) {
            let mesh = meshes.add(chunk_mesh(map, cx, cy));
            // The ground material (textures for this map) is added by `surface::surface_sync`.
            commands.spawn((Mesh3d(mesh.clone()), Transform::IDENTITY, TerrainChunk { cx, cy }, GameScene));
            if with_fog {
                commands.spawn((Mesh3d(mesh), MeshMaterial3d(fog_mat.clone()), Transform::from_xyz(0.0, 0.06, 0.0), NotShadowCaster, FogOverlay, GameScene));
            }
        }
    }
    // Beyond the border: the map mirrored and blurred, sinking into the sea further out. No
    // textures there, so it looks soft next to the playable ground.
    let soft = materials.add(StandardMaterial { perceptual_roughness: 1.0, reflectance: 0.1, ..default() });
    commands.spawn((Mesh3d(meshes.add(apron_mesh(map))), MeshMaterial3d(soft), Transform::IDENTITY, NotShadowCaster, GameScene));
    // The map border as a dashed line (its width follows the camera, see `map_edge_sync`).
    commands.spawn((
        Mesh3d(meshes.add(edge_mesh(map, 1.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgba(0.95, 0.93, 0.80, 0.8),
            alpha_mode: AlphaMode::Blend,
            unlit: true,
            depth_bias: 12.0,
            ..default()
        })),
        Transform::IDENTITY,
        NotShadowCaster,
        NotShadowReceiver,
        MapEdge { width: 1.0 },
        GameScene,
    ));
    // Further out: a dark sea floor (unlit, so it shows no lighting or shadow seams) under the water.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(world.max(world_z) * 12.0, world.max(world_z) * 12.0))),
        MeshMaterial3d(materials.add(StandardMaterial { base_color: Color::srgb(0.03, 0.07, 0.10), unlit: true, ..default() })),
        Transform::from_xyz(world * 0.5, map.water - 6.0, world_z * 0.5),
        NotShadowCaster,
        NotShadowReceiver,
        GameScene,
    ));
    // Water covers the map and the sea around it; it takes no shadows (cascade borders showed up
    // as rectangles on it).
    let water_mat = materials.add(StandardMaterial {
        base_color: Color::srgba(0.07, 0.22, 0.32, 0.82),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.12,
        reflectance: 0.6,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(water_mesh(world.max(world_z) * 12.0))),
        MeshMaterial3d(water_mat.clone()),
        Transform::from_xyz(world * 0.5, map.water, world_z * 0.5),
        NotShadowCaster,
        NotShadowReceiver,
        WaterPlane,
        GameScene,
    ));
    // Lakes above the sea have their own surface.
    if let Some(mesh) = lake_mesh(map) {
        commands.spawn((Mesh3d(meshes.add(mesh)), MeshMaterial3d(water_mat), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, GameScene));
    }
    spawn_deposits(commands, map, meshes, materials);
    let sky = super::sky::find(&map.sky);
    commands.spawn((
        sky.light(),
        sky.sun_transform(),
        CascadeShadowConfigBuilder { num_cascades: 3, maximum_distance: 220.0, first_cascade_far_bound: 30.0, ..default() }.build(),
        Sun,
        GameScene,
    ));
    commands.insert_resource(sky.ambient_light());
    super::sky::spawn_clouds(commands, map, meshes, materials, images);
    fog_img
}

/// The water surface: a flat square of side `size` centred on the origin.
///
/// Transparent meshes are drawn in the order of their bounding-box centres. The fog-of-war
/// overlays are one mesh per terrain chunk, so with a plain plane (centre = map centre) the chunks
/// nearer the camera were drawn after the water and the far ones before it, which showed as
/// squares of darker and lighter water. An extra vertex far below, used by no triangle, moves the
/// water's centre behind everything else, so it is always drawn first and the fog lies evenly on it.
fn water_mesh(size: f32) -> Mesh {
    let h = size * 0.5;
    let positions = vec![[-h, 0.0, -h], [h, 0.0, -h], [h, 0.0, h], [-h, 0.0, h], [0.0, -100_000.0, 0.0]];
    let uvs = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0], [0.5, 0.5]];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 5])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(vec![0, 3, 2, 0, 2, 1]))
}

/// The surfaces of the lakes (`MapData::lakes`): every lake cell at its level, cells in a row
/// joined into strips. Cells the ground has since risen above stay in; the terrain hides them.
/// Drawn before the fog overlays like the sea (the extra vertex of `water_mesh`).
fn lake_mesh(map: &MapData) -> Option<Mesh> {
    let (n, ny) = map.dims();
    if map.lakes.is_empty() || map.lake_of.len() != n * ny {
        return None;
    }
    let (mut pos, mut uvs, mut idx) = (vec![], vec![], vec![]);
    for y in 0..ny {
        let mut x = 0;
        while x < n {
            let k = map.lake_of[y * n + x];
            let x0 = x;
            while x < n && map.lake_of[y * n + x] == k {
                x += 1;
            }
            let level = map.water_cell(x0, y);
            if k == 0 || level <= map.water {
                continue;
            }
            let (a, b, z) = (x0 as f32 * CELL, x as f32 * CELL, y as f32 * CELL);
            let first = pos.len() as u32;
            for (px, pz) in [(a, z), (b, z), (b, z + CELL), (a, z + CELL)] {
                pos.push([px, level, pz]);
                uvs.push([px / 12.0, pz / 12.0]);
            }
            idx.extend_from_slice(&[first, first + 3, first + 2, first, first + 2, first + 1]);
        }
    }
    if pos.is_empty() {
        return None;
    }
    pos.push([map.world_x() * 0.5, -100_000.0, map.world_z() * 0.5]);
    uvs.push([0.0, 0.0]);
    let count = pos.len();
    Some(
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
            .with_inserted_indices(Indices::U32(idx)),
    )
}

/// The dashed line along the map border. `width` is the line's width the mesh was built for; it
/// follows the camera distance (rebuilt in steps), so the line stays about as wide on screen.
#[derive(Component)]
pub struct MapEdge {
    pub width: f32,
}

fn edge_mesh(map: &MapData, width: f32) -> Mesh {
    let (world, world_z) = (map.world_x(), map.world_z());
    let (dash, gap) = ((width * 14.0).max(5.0), (width * 9.0).max(3.0));
    let half = width * 0.5;
    let lift = 0.12 + width * 0.4;
    let (mut pos, mut idx) = (vec![], vec![]);
    // Corners in turn; each side runs from one to the next, `inward` points into the map.
    let corners = [v2(0.0, 0.0), v2(world, 0.0), v2(world, world_z), v2(0.0, world_z)];
    for s in 0..4 {
        let (a, b) = (corners[s], corners[(s + 1) % 4]);
        let len = a.dist(b).max(1e-3);
        let dir = (b - a) * (1.0 / len);
        let inward = v2(-dir.y, dir.x);
        let mut t = 0.0;
        while t < len {
            let end = (t + dash).min(len);
            let pieces = ((end - t) / CELL).ceil().max(1.0) as usize;
            let first = pos.len() as u32;
            for i in 0..=pieces {
                let p = a + dir * (t + (end - t) * i as f32 / pieces as f32);
                let y = map.height(p.x, p.y).max(map.height(p.x + inward.x * half, p.y + inward.y * half)).max(map.water_at(p)) + lift;
                for side in [-1.0, 1.0] {
                    let q = p + inward * (half * side);
                    pos.push([q.x, y, q.y]);
                }
            }
            for i in 0..pieces as u32 {
                let (p0, p1, q0, q1) = (first + i * 2, first + i * 2 + 1, first + i * 2 + 2, first + i * 2 + 3);
                idx.extend_from_slice(&[p0, p1, q0, q0, p1, q1]);
            }
            t += dash + gap;
        }
    }
    for t in idx.chunks_exact_mut(3) {
        let [p, q, r] = [pos[t[0] as usize], pos[t[1] as usize], pos[t[2] as usize]];
        if (q[0] - p[0]) * (r[2] - p[2]) - (q[2] - p[2]) * (r[0] - p[0]) > 0.0 {
            t.swap(1, 2);
        }
    }
    let nrm = vec![[0.0, 1.0, 0.0]; pos.len()];
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_indices(Indices::U32(idx));
    mesh
}

/// Line width for a camera this far from the nearest point of the border, in steps of 1.4x.
fn edge_width(dist: f32) -> f32 {
    let w = (dist * 0.003).clamp(0.25, 12.0);
    0.25 * 1.4f32.powf(((w / 0.25).ln() / 1.4f32.ln()).round())
}

/// Rebuilds the map-edge line when the camera has moved far enough nearer or further for its width
/// to change a step. Runs in the game and in the editor.
pub fn map_edge_sync(map: Option<Res<MapData>>, camera: Query<&GlobalTransform, With<MainCamera>>, mut edges: Query<(&mut MapEdge, &Mesh3d)>, mut meshes: ResMut<Assets<Mesh>>) {
    let (Some(map), Ok(cam)) = (map, camera.single()) else { return };
    let c = cam.translation();
    let (world, world_z) = (map.world_x(), map.world_z());
    // Nearest border point.
    let (x, z) = (c.x.clamp(0.0, world), c.z.clamp(0.0, world_z));
    let b = if (x.min(world - x)) < (z.min(world_z - z)) { v2(if x < world - x { 0.0 } else { world }, z) } else { v2(x, if z < world_z - z { 0.0 } else { world_z }) };
    let dist = c.distance(b.at(map.surface(b)));
    let w = edge_width(dist);
    for (mut e, mesh) in &mut edges {
        if (e.width - w).abs() > 1e-3 {
            e.width = w;
            if let Some(m) = meshes.get_mut(&mesh.0) {
                *m.into_inner() = edge_mesh(&map, w);
            }
        }
    }
}

#[derive(Component)]
pub struct DepositMarker;

pub fn spawn_deposits(commands: &mut Commands, map: &MapData, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) {
    let disc = meshes.add(Cylinder::new(0.8, 0.12));
    let core = meshes.add(Cylinder::new(0.35, 0.2));
    let m1 = materials.add(StandardMaterial { base_color: Color::srgb(0.32, 0.34, 0.36), metallic: 0.8, perceptual_roughness: 0.45, ..default() });
    let m2 = materials.add(StandardMaterial { base_color: Color::srgb(0.25, 0.95, 0.4), emissive: LinearRgba::rgb(0.3, 2.2, 0.6), perceptual_roughness: 0.4, ..default() });
    for d in &map.deposits {
        let y = map.h(*d);
        commands.spawn((Mesh3d(disc.clone()), MeshMaterial3d(m1.clone()), Transform::from_xyz(d.x, y + 0.03, d.y), NotShadowCaster, DepositMarker, GroundSnap(0.03), GameScene));
        commands.spawn((Mesh3d(core.clone()), MeshMaterial3d(m2.clone()), Transform::from_xyz(d.x, y + 0.05, d.y), NotShadowCaster, DepositMarker, GroundSnap(0.05), GameScene));
    }
}

/// The ground colour, brightened for the small pictures (they are drawn without lighting).
fn picture_color(map: &MapData, tint: &super::surface::Tint, x: f32, z: f32) -> [f32; 3] {
    let c = terrain_color(map, tint, x, z);
    [c[0] * 1.9, c[1] * 1.9, c[2] * 1.9]
}

/// A square picture of a map for the lobby: terrain, water, forests (dark green), rocks (grey) and
/// deposits (white), `side` x `side` RGBA pixels. Start positions are drawn on top by the lobby.
pub fn map_preview(map: &MapData, defs: &crate::data::Defs, side: usize) -> Vec<u8> {
    let side = side.max(8);
    let w = map.world_max().max(1.0);
    let k = w / side as f32;
    // A map that is not square is centred in the square picture (`MapData::pic_uv`).
    let off = (map.size() - v2(w, w)) * 0.5;
    let mut px = vec![0f32; side * side * 3];
    let tint = super::surface::Tint::new(map);
    for y in 0..side {
        for x in 0..side {
            let (wx, wz) = ((x as f32 + 0.5) * k + off.x, (y as f32 + 0.5) * k + off.y);
            if !map.on_map(v2(wx, wz)) {
                let i = (y * side + x) * 3;
                px[i..i + 3].copy_from_slice(&[0.05, 0.06, 0.07]);
                continue;
            }
            let mut c = picture_color(map, &tint, wx, wz);
            let h = map.height(wx, wz);
            let water = map.water_xz(wx, wz);
            if h < water {
                let t = ((water - h) / 2.0).clamp(0.3, 1.0);
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
        let (x, y) = (((f.pos.x - off.x) / k) as usize, ((f.pos.y - off.y) / k) as usize);
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
        let (x, y) = (((d.x - off.x) / k) as i32, ((d.y - off.y) / k) as i32);
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let (px, py) = (x + dx, y + dy);
            if px >= 0 && py >= 0 && (px as usize) < side && (py as usize) < side {
                let i = (py as usize * side + px as usize) * 4;
                out[i..i + 3].copy_from_slice(&[90, 255, 120]);
            }
        }
    }
    out
}

/// Small colour image of the map for the minimap (one pixel per cell, RGBA).
pub fn minimap_pixels(map: &MapData) -> Vec<u8> {
    let (n, ny) = map.dims();
    let mut out = Vec::with_capacity(n * ny * 4);
    let tint = super::surface::Tint::new(map);
    for y in 0..ny {
        for x in 0..n {
            let (wx, wz) = ((x as f32 + 0.5) * CELL, (y as f32 + 0.5) * CELL);
            let mut c = picture_color(map, &tint, wx, wz);
            let h = map.height(wx, wz);
            let water = map.water_xz(wx, wz);
            if h < water {
                let k = ((water - h) / 2.0).clamp(0.3, 1.0);
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
