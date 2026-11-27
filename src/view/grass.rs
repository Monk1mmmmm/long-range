//! Grass on grassy ground (`Settings::grass_detail`: 1 high, 2 ultra; 0 draws none).
//!
//! Done the way open-world games do it, scaled for an RTS camera:
//! - **dense blades in clumps**: tens of thin blades per square unit near the view, grouped round
//!   clump centres that share a height and a lean, so the grass has structure instead of being an even
//!   fuzz; ultra blades bend (a quad to the knee, a tip above it);
//! - **lit like the ground**: every blade carries the terrain's normal, its root has the ground's
//!   colour and the tip is lighter, so the grass blends into the terrain texture instead of standing out
//!   as dark strands;
//! - **density falls off smoothly with distance** from the point the camera looks at (`grass.wesl`):
//!   each blade has a random rank and shows only while the rank is below the density there, growing out
//!   of the ground over a short band. A chunk far away gets a mesh of only the low ranks; as the same
//!   blades are the only ones the shader would show there, swapping meshes never pops;
//! - **wind** sway and gusts in the vertex shader;
//! - none on roads (exact distance to the road lines), on paved/asphalt/sand... layers (blended masks,
//!   thinning out at their edges), on blocked cells, steep slopes or under water, and **none where blasts
//!   scorched or churned the ground**: the shader reads the terrain's scorch map, so craters clear the
//!   grass at once without rebuilding anything (withered brown at their edge).
//!
//! Chunks (`CHUNK` cells) are built a few per frame around the view point and dropped far from it; only
//! chunks whose terrain actually changed are rebuilt (`GrassTouched`, filled by `terrain_sync`). Purely
//! visual, never in the simulation.
use super::flora::FloraView;
use super::terrain::GameScene;
use crate::map::*;
use crate::settings::Settings;
use crate::surface::KINDS;
use bevy::asset::{embedded_asset, RenderAssetUsages};
use bevy::camera::primitives::Aabb;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError};
use bevy::shader::ShaderRef;

pub type GrassMaterial = ExtendedMaterial<StandardMaterial, GrassExt>;

/// Side of a grass chunk in cells.
const CHUNK: usize = 8;
/// Above this camera height there is no grass (it fades out over the last quarter).
const MAX_CAM: f32 = 170.0;

#[derive(Clone, Copy, Default, ShaderType, Debug)]
pub struct GrassParams {
    /// xz of the view point, fade with camera height, time.
    pub focus: Vec4,
    /// Full density to x, `far` density (z) from y, none beyond w.
    pub dist: Vec4,
    /// Wind direction (xy) and strength (z).
    pub wind: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct GrassExt {
    #[uniform(100)]
    pub params: GrassParams,
    #[texture(101)]
    #[sampler(102)]
    pub scorch: Handle<Image>,
}

impl MaterialExtension for GrassExt {
    fn vertex_shader() -> ShaderRef {
        "embedded://longrange/view/grass.wesl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(_: &MaterialExtensionPipeline, descriptor: &mut RenderPipelineDescriptor, _: &MeshVertexBufferLayoutRef, _: MaterialExtensionKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        // Both faces, and never the back face's flipped normal (the normal is the ground's).
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

pub struct GrassPlugin;
impl Plugin for GrassPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "grass.wesl");
        app.add_plugins(MaterialPlugin::<GrassMaterial>::default());
        app.init_resource::<GrassTouched>();
    }
}

/// Places whose terrain changed (craters, earthworks), pushed by `terrain_sync`: the grass chunks
/// there are rebuilt to follow the ground. `.1`: everything changed.
#[derive(Resource, Default)]
pub struct GrassTouched(pub Vec<V2>, pub bool);

#[derive(Component)]
pub struct GrassChunk {
    cx: usize,
    cy: usize,
    level: u8,
    /// Holds only the low ranks (`Tune::far`): a chunk away from the view point.
    far: bool,
}

/// Numbers per detail level.
struct Tune {
    /// Blades per square unit at full density.
    per_unit: f32,
    /// Density far from the view point (share of the blades), from `dist.y` on.
    far: f32,
    /// Full density to, far density from, none beyond (distance from the view point).
    dist: [f32; 3],
    /// Ultra: bent blades of three triangles.
    bent: bool,
}

fn tune(level: u8) -> Tune {
    if level >= 2 {
        Tune { per_unit: 22.0, far: 0.3, dist: [40.0, 65.0, 115.0], bent: true }
    } else {
        Tune { per_unit: 11.0, far: 0.3, dist: [30.0, 50.0, 85.0], bent: false }
    }
}

fn hash(a: u64, b: u64) -> f32 {
    let mut h = a.wrapping_mul(0x9E3779B97F4A7C15) ^ b.wrapping_mul(0xC2B2AE3D27D4EB4F);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58476D1CE4E5B9);
    h ^= h >> 29;
    (h & 0xFFFFFF) as f32 / 16777216.0
}

/// What the grass needs to know about one kind of ground: its colour, how dense and how tall the
/// grass on it is. `None`: no grass grows there.
fn grass_kind(kind: &str) -> Option<([f32; 3], f32, f32)> {
    let (dense, tall) = match kind {
        "grass" => (1.0, 1.0),
        "meadow" => (1.0, 1.3),
        "forest_floor" => (0.45, 0.8),
        _ => return None,
    };
    KINDS.iter().find(|k| k.id == kind).map(|k| (k.color, dense, tall))
}

/// Per ground layer of the map: its grass (see `grass_kind`).
fn layer_grass(map: &MapData) -> Vec<Option<([f32; 3], f32, f32)>> {
    if map.layers.is_empty() {
        return vec![grass_kind("grass")];
    }
    map.layers.iter().map(|l| grass_kind(&l.kind)).collect()
}

/// A layer mask at a world position, bilinear (0..1).
fn mask_at(map: &MapData, layer: usize, p: V2) -> f32 {
    let len = map.nx * map.ny;
    let fx = (p.x / CELL - 0.5).clamp(0.0, map.nx as f32 - 1.0);
    let fy = (p.y / CELL - 0.5).clamp(0.0, map.ny as f32 - 1.0);
    let (x0, y0) = (fx.floor() as usize, fy.floor() as usize);
    let (x1, y1) = ((x0 + 1).min(map.nx - 1), (y0 + 1).min(map.ny - 1));
    let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
    let m = |x: usize, y: usize| map.masks.get((layer - 1) * len + y * map.nx + x).copied().unwrap_or(0) as f32 / 255.0;
    let a = m(x0, y0) + (m(x1, y0) - m(x0, y0)) * tx;
    let b = m(x0, y1) + (m(x1, y1) - m(x0, y1)) * tx;
    a + (b - a) * ty
}

/// The grass at a world position: (how grassy 0..1, colour, density, height), the layers blended as
/// the ground shader does (each layer over the ones below by its mask, a smooth step at the edge).
fn grass_at(map: &MapData, layers: &[Option<([f32; 3], f32, f32)>], p: V2) -> (f32, [f32; 3], f32, f32) {
    let base = layers.first().copied().flatten();
    let (mut g, mut col, mut dense, mut tall) = match base {
        Some((c, d, t)) => (1.0, c, d, t),
        None => (0.0, [0.12, 0.22, 0.06], 1.0, 1.0),
    };
    for (l, lg) in layers.iter().enumerate().skip(1) {
        let m = mask_at(map, l, p);
        if m <= 0.0 {
            continue;
        }
        let w = { let t = ((m - 0.3) / 0.4).clamp(0.0, 1.0); t * t * (3.0 - 2.0 * t) };
        match lg {
            Some((c, d, t)) => {
                g += (1.0 - g) * w;
                if w > 0.5 {
                    (col, dense, tall) = (*c, *d, *t);
                }
            }
            None => g *= 1.0 - w,
        }
    }
    (g, col, dense, tall)
}

/// Road segments near a chunk: (a, b, half width + margin).
fn roads_near(map: &MapData, lo: V2, hi: V2) -> Vec<(V2, V2, f32)> {
    let mut out = vec![];
    for r in &map.roads {
        let reach = r.width * 0.5 + 0.35;
        for s in r.points.windows(2) {
            let (a, b) = (s[0], s[1]);
            if a.x.max(b.x) + reach < lo.x || a.x.min(b.x) - reach > hi.x || a.y.max(b.y) + reach < lo.y || a.y.min(b.y) - reach > hi.y {
                continue;
            }
            out.push((a, b, reach));
        }
    }
    out
}

fn on_road(roads: &[(V2, V2, f32)], p: V2) -> bool {
    roads.iter().any(|&(a, b, reach)| {
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.len2().max(1e-6)).clamp(0.0, 1.0);
        p.dist(a + ab * t) <= reach
    })
}

/// The mesh of the chunk at cell (cx, cy), relative to `origin`, or `None` without any grass.
/// `far`: only the blades of rank below `Tune::far`.
fn chunk_mesh(map: &MapData, layers: &[Option<([f32; 3], f32, f32)>], cx: usize, cy: usize, level: u8, far: bool, origin: Vec3) -> Option<(Mesh, Aabb)> {
    let tn = tune(level);
    let keep = if far { tn.far } else { 1.0 };
    let world = map.size();
    let lo = v2(cx as f32 * CELL, cy as f32 * CELL);
    let hi = v2(((cx + CHUNK).min(map.nx)) as f32 * CELL, ((cy + CHUNK).min(map.ny)) as f32 * CELL);
    let roads = roads_near(map, lo, hi);
    let (mut pos, mut nrm, mut col, mut uv, mut uv1, mut idx): (Vec<[f32; 3]>, Vec<[f32; 3]>, Vec<[f32; 4]>, Vec<[f32; 2]>, Vec<[f32; 2]>, Vec<u32>) = (vec![], vec![], vec![], vec![], vec![], vec![]);
    // Clumps: 4 per cell, each with blades round it sharing a height and a lean.
    let clumps = 4usize;
    let per_clump = (tn.per_unit * CELL * CELL / clumps as f32).round() as usize;
    for y in cy..(cy + CHUNK).min(map.ny) {
        for x in cx..(cx + CHUNK).min(map.nx) {
            let i = y * map.nx + x;
            if map.blocked.get(i).copied().unwrap_or(0) != 0 {
                continue;
            }
            for c in 0..clumps {
                let cs = (i as u64) * 8 + c as u64;
                let cp = v2((x as f32 + hash(cs, 1)) * CELL, (y as f32 + hash(cs, 2)) * CELL);
                let (g, base, dense, tall) = grass_at(map, layers, cp);
                if g <= 0.02 {
                    continue;
                }
                let clump_h = (0.2 + 0.26 * hash(cs, 3)) * tall;
                let la = hash(cs, 4) * std::f32::consts::TAU;
                let clump_lean = Vec2::new(la.cos(), la.sin()) * (0.1 + 0.3 * hash(cs, 5));
                // Broad patches: drier (yellower) and lusher (darker) stretches.
                let dry = (hash((cp.x * 0.05) as u64 + 91, (cp.y * 0.05) as u64 + 17) * 0.6 + hash((cp.x * 0.13) as u64 + 3, (cp.y * 0.13) as u64 + 5) * 0.4).powf(1.6);
                for b in 0..per_clump {
                    let s = cs * 64 + b as u64;
                    let rank = hash(s, 6);
                    if rank >= keep {
                        continue;
                    }
                    // Thinner where the grass layer fades out, and per kind of ground.
                    if hash(s, 7) > g * dense {
                        continue;
                    }
                    let rr = 0.75 * hash(s, 8).sqrt();
                    let ra = hash(s, 9) * std::f32::consts::TAU;
                    let p = cp + v2(ra.cos() * rr, ra.sin() * rr);
                    if !map.on_map(p) || on_road(&roads, p) {
                        continue;
                    }
                    let n = map.normal(p.x, p.y);
                    if n.y < 0.82 {
                        continue;
                    }
                    let h = map.h(p);
                    if h < map.water_at(p) + 0.15 {
                        continue;
                    }
                    // Taller in the middle of a clump.
                    let bh = clump_h * (0.55 + 0.6 * hash(s, 10)) * (1.15 - 0.4 * rr / 0.75);
                    let w = (0.045 + 0.035 * hash(s, 11)) * (0.7 + bh * 1.2);
                    let fa = hash(s, 12) * std::f32::consts::TAU;
                    let (sa, ca) = fa.sin_cos();
                    let lean2 = clump_lean + Vec2::new(hash(s, 13) - 0.5, hash(s, 14) - 0.5) * 0.35;
                    let lean = Vec3::new(lean2.x, 0.0, lean2.y) * bh;
                    let side = Vec3::new(-sa, 0.0, ca) * w;
                    let root = Vec3::new(p.x, h - 0.03, p.y) - origin;
                    let k = 0.82 + 0.36 * hash(s, 15);
                    // Some blades yellower, some bluer: the grass reads as a texture from above.
                    let hue = (hash(s, 16) - 0.5) * 0.35;
                    // Root: the ground's own colour, darker (the shade down in the grass); tip lighter
                    // and, in dry patches, a little yellower.
                    let rc = [base[0] * 0.62 * k, base[1] * 0.66 * k, base[2] * 0.6 * k, 1.0];
                    let tc = [base[0] * (1.45 + 0.7 * dry + hue) * k, base[1] * (1.5 + 0.2 * dry) * k, base[2] * (1.15 - 0.25 * dry - hue) * k, 1.0];
                    let mid = [(rc[0] + tc[0]) * 0.5, (rc[1] + tc[1]) * 0.5, (rc[2] + tc[2]) * 0.5, 1.0];
                    let uvp = [p.x / world.x.max(1.0), p.y / world.y.max(1.0)];
                    let nn = n.to_array();
                    let first = pos.len() as u32;
                    let mut push = |v: Vec3, c: [f32; 4], hgt: f32| {
                        pos.push(v.to_array());
                        nrm.push(nn);
                        col.push(c);
                        uv.push(uvp);
                        uv1.push([hgt, rank]);
                    };
                    if tn.bent {
                        // A quad to the knee and a tip above it, the lean growing towards the end.
                        let knee = root + lean * 0.3 + Vec3::Y * bh * 0.55;
                        let tip = root + lean * 1.1 + Vec3::Y * bh * 0.92;
                        push(root - side, rc, 0.0);
                        push(root + side, rc, 0.0);
                        push(knee + side * 0.7, mid, bh * 0.55);
                        push(knee - side * 0.7, mid, bh * 0.55);
                        push(tip, tc, bh);
                        idx.extend_from_slice(&[first, first + 1, first + 2, first, first + 2, first + 3, first + 3, first + 2, first + 4]);
                    } else {
                        push(root - side, rc, 0.0);
                        push(root + side, rc, 0.0);
                        push(root + lean + Vec3::Y * bh, tc, bh);
                        idx.extend_from_slice(&[first, first + 1, first + 2]);
                    }
                }
            }
        }
    }
    if pos.is_empty() {
        return None;
    }
    // The wind bends tips a little beyond the vertices' own bounds.
    let (lo, hi) = pos.iter().fold((Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)), |(a, b), p| (a.min(Vec3::from(*p)), b.max(Vec3::from(*p))));
    let aabb = Aabb::from_min_max(lo - Vec3::splat(0.6), hi + Vec3::splat(0.6));
    let mut mesh = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nrm);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_1, uv1);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(Indices::U32(idx));
    Some((mesh, aabb))
}

/// The view point: where the view axis meets the ground (or below the camera when looking up).
fn view_point(map: &MapData, cam: &Transform) -> Vec3 {
    let fwd = cam.forward().as_vec3();
    if fwd.y < -0.05 {
        cam.translation + fwd * ((cam.translation.y - map.water).max(0.0) / -fwd.y).min(400.0)
    } else {
        cam.translation
    }
}

#[derive(Default)]
pub struct GrassState {
    /// The map the chunks were made for.
    sig: Option<(usize, usize, u64, usize, usize, usize)>,
    layers: Vec<Option<([f32; 3], f32, f32)>>,
    mat: Option<Handle<GrassMaterial>>,
    /// `SurfaceView::burn_gen` the material last saw.
    burn_gen: u32,
}

/// Distance from `p` to the nearest point of the chunk at (cx, cy).
fn chunk_dist(cx: usize, cy: usize, p: Vec2) -> f32 {
    let lo = Vec2::new(cx as f32 * CELL, cy as f32 * CELL);
    let hi = lo + Vec2::splat(CHUNK as f32 * CELL);
    p.clamp(lo, hi).distance(p)
}

/// Builds the grass chunks near the view point, drops the far ones and keeps the material's view
/// point, time and scorch map up to date.
#[allow(clippy::too_many_arguments)]
pub fn grass_sync(
    mut commands: Commands,
    settings: Res<Settings>,
    map: Res<MapData>,
    view: Option<Res<FloraView>>,
    surface: Option<Res<super::surface::SurfaceView>>,
    cams: Query<&GlobalTransform, With<super::terrain::MainCamera>>,
    chunks: Query<(Entity, &GrassChunk)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GrassMaterial>>,
    std_mats: Res<Assets<StandardMaterial>>,
    mut touched: ResMut<GrassTouched>,
    time: Res<Time>,
    mut state: Local<GrassState>,
) {
    let level = settings.grass_detail.min(2);
    let cam = cams.iter().next().map(|c| c.compute_transform());
    let (Some(view), Some(cam), Some(surface)) = (view, cam, surface) else { return };
    let tn = tune(level);
    // A new map: start over.
    let sig = (map.nx, map.ny, map.seed, map.roads.len(), map.layers.len(), map.masks.len());
    if state.sig != Some(sig) {
        for (e, _) in &chunks {
            commands.entity(e).despawn();
        }
        state.sig = Some(sig);
        state.layers = layer_grass(&map);
        state.mat = None;
        touched.0.clear();
        touched.1 = false;
        return;
    }
    let focus = view_point(&map, &cam);
    let fp = Vec2::new(focus.x, focus.z);
    let height_fade = 1.0 - ((cam.translation.y - map.h(v2(cam.translation.x, cam.translation.z)).max(map.water) - MAX_CAM * 0.75) / (MAX_CAM * 0.25)).clamp(0.0, 1.0);
    // Snow weather buries the grass.
    let low = height_fade > 0.0 && level > 0 && surface.snow < 0.5;
    // The material: made once per map; its uniform follows the view every frame.
    let scorch = surface.scorch();
    let mat = match state.mat.clone() {
        Some(m) => m,
        None => {
            // The fog of war darkens grass like trees and rocks: the flora material's texture.
            let fog = std_mats.get(&view.material).and_then(|m| m.base_color_texture.clone());
            let m = materials.add(ExtendedMaterial {
                base: StandardMaterial { base_color: Color::WHITE, base_color_texture: fog, perceptual_roughness: 0.85, reflectance: 0.1, ..default() },
                extension: GrassExt { params: GrassParams::default(), scorch: scorch.clone() },
            });
            state.mat = Some(m.clone());
            m
        }
    };
    if let Some(mut m) = materials.get_mut(&mat) {
        let a = 0.6f32 + 0.25 * (time.elapsed_secs() * 0.013).sin();
        m.extension.params = GrassParams {
            focus: Vec4::new(focus.x, focus.z, height_fade, time.elapsed_secs_wrapped()),
            dist: Vec4::new(tn.dist[0], tn.dist[1], tn.far, tn.dist[2]),
            wind: Vec4::new(a.cos(), a.sin(), 0.9 + 0.5 * (time.elapsed_secs() * 0.05).sin(), 0.0),
        };
        if m.extension.scorch != scorch || state.burn_gen != surface.burn_gen {
            m.extension.scorch = scorch.clone();
        }
    }
    state.burn_gen = surface.burn_gen;
    // Chunks whose ground changed are made again.
    let (spots, all) = (std::mem::take(&mut touched.0), std::mem::take(&mut touched.1));
    let size = CHUNK as f32 * CELL;
    let mut have: Vec<(usize, usize, bool)> = vec![];
    for (e, c) in &chunks {
        let d = chunk_dist(c.cx, c.cy, fp);
        let hit = all || spots.iter().any(|t| t.x > c.cx as f32 * CELL - 8.0 && t.y > c.cy as f32 * CELL - 8.0 && t.x < c.cx as f32 * CELL + size + 8.0 && t.y < c.cy as f32 * CELL + size + 8.0);
        // A full chunk is swapped for a far one only well beyond the full-density ring (hysteresis);
        // a far one for a full one as soon as it comes inside it.
        let wrong_ring = if c.far { d < tn.dist[1] } else { d > tn.dist[1] + 12.0 };
        if !low || c.level != level || d > tn.dist[2] + 10.0 || hit || wrong_ring {
            commands.entity(e).despawn();
        } else {
            have.push((c.cx, c.cy, c.far));
        }
    }
    if !low {
        return;
    }
    // Missing chunks in reach, nearest first, a few per frame.
    let reach = tn.dist[2];
    let (x0, x1) = (((fp.x - reach) / size).floor().max(0.0) as usize, ((fp.x + reach) / size).floor().max(0.0) as usize);
    let (y0, y1) = (((fp.y - reach) / size).floor().max(0.0) as usize, ((fp.y + reach) / size).floor().max(0.0) as usize);
    let mut want: Vec<(f32, usize, usize)> = vec![];
    for gy in y0..=y1 {
        for gx in x0..=x1 {
            let (cx, cy) = (gx * CHUNK, gy * CHUNK);
            if cx >= map.nx || cy >= map.ny || have.iter().any(|h| h.0 == cx && h.1 == cy) {
                continue;
            }
            let d = chunk_dist(cx, cy, fp);
            if d <= reach {
                want.push((d, cx, cy));
            }
        }
    }
    want.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (d, cx, cy) in want.into_iter().take(if level >= 2 { 2 } else { 3 }) {
        let far = d >= tn.dist[1];
        let c = Vec2::new((cx as f32 + CHUNK as f32 * 0.5) * CELL, (cy as f32 + CHUNK as f32 * 0.5) * CELL);
        let origin = Vec3::new(c.x, map.height(c.x, c.y), c.y);
        // Always spawn an entity (even without grass) so the chunk is not tried again every frame.
        let mesh = chunk_mesh(&map, &state.layers, cx, cy, level, far, origin);
        let mut e = commands.spawn((Transform::from_translation(origin), Visibility::Inherited, GrassChunk { cx, cy, level, far }, GameScene));
        if let Some((m, aabb)) = mesh {
            e.insert((Mesh3d(meshes.add(m)), MeshMaterial3d(mat.clone()), NotShadowCaster, aabb));
        }
    }
}
