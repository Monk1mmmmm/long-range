//! High water detail (`Settings::water_detail` 2): a tiling wave normal map that drifts over the
//! surface (sun glints, ripples) and foam that laps along every shore. Purely visual.
use crate::map::*;
use bevy::asset::RenderAssetUsages;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// World units one repeat of the wave texture covers.
pub const TILE: f32 = 22.0;

/// The animated materials of a game with high water.
#[derive(Resource)]
pub struct WaterFx {
    pub sea: Handle<StandardMaterial>,
    pub foam: Handle<StandardMaterial>,
}

fn lattice(x: i32, y: i32, period: i32, seed: u32) -> f32 {
    let (x, y) = (x.rem_euclid(period), y.rem_euclid(period));
    let mut h = (x as u32).wrapping_mul(0x9E3779B1) ^ (y as u32).wrapping_mul(0x85EBCA6B) ^ seed.wrapping_mul(0xC2B2AE35);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A2D39);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65535.0
}

/// Value noise that repeats every `period` lattice cells.
fn pnoise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let (xf, yf) = (x.floor(), y.floor());
    let (fx, fy) = (x - xf, y - yf);
    let (fx, fy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let (xi, yi) = (xf as i32, yf as i32);
    let a = lattice(xi, yi, period, seed);
    let b = lattice(xi + 1, yi, period, seed);
    let c = lattice(xi, yi + 1, period, seed);
    let d = lattice(xi + 1, yi + 1, period, seed);
    let top = a + (b - a) * fx;
    top + ((c + (d - c) * fx) - top) * fy
}

fn repeating(mut im: Image) -> Image {
    use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    im.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    im
}

/// A tiling normal map of choppy ripples (several scales of noise), with mipmaps.
fn wave_normals() -> Image {
    const N: usize = 256;
    let height = |x: i32, y: i32| -> f32 {
        let (u, v) = (x.rem_euclid(N as i32) as f32 / N as f32, y.rem_euclid(N as i32) as f32 / N as f32);
        let mut h = 0.0;
        let mut amp = 1.0;
        for o in 0..4 {
            let p = 4 << o;
            // Crossed ridges read as ripples rather than blobs.
            let n = pnoise(u * p as f32, v * p as f32, p, 11 + o as u32);
            h += amp * (1.0 - (2.0 * n - 1.0).abs());
            amp *= 0.5;
        }
        h
    };
    let mut data = vec![0u8; N * N * 4];
    for y in 0..N as i32 {
        for x in 0..N as i32 {
            let dx = height(x + 1, y) - height(x - 1, y);
            let dy = height(x, y + 1) - height(x, y - 1);
            let n = Vec3::new(-dx * 9.0, -dy * 9.0, 1.0).normalize();
            let i = (y as usize * N + x as usize) * 4;
            for k in 0..3 {
                data[i + k] = ((n[k] * 0.5 + 0.5) * 255.0) as u8;
            }
            data[i + 3] = 255;
        }
    }
    let img = image::RgbaImage::from_raw(N as u32, N as u32, data).expect("size");
    repeating(super::models::to_image_mips(&img, false))
}

/// The surface of the sea and the lakes at high detail.
pub fn material(images: &mut Assets<Image>) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgba(0.05, 0.2, 0.3, 0.8),
        alpha_mode: AlphaMode::Blend,
        perceptual_roughness: 0.07,
        reflectance: 0.75,
        normal_map_texture: Some(images.add(wave_normals())),
        ..default()
    }
}

fn foam_texture() -> Image {
    const N: usize = 128;
    let mut data = vec![255u8; N * N * 4];
    for y in 0..N {
        for x in 0..N {
            let (u, v) = (x as f32 / N as f32, y as f32 / N as f32);
            let n = 0.55 * pnoise(u * 8.0, v * 4.0, 8, 3) + 0.3 * pnoise(u * 16.0, v * 8.0, 16, 5) + 0.15 * pnoise(u * 32.0, v * 16.0, 32, 9);
            let a = ((n - 0.35) * 3.2).clamp(0.0, 1.0);
            data[(y * N + x) * 4 + 3] = (a * 255.0) as u8;
        }
    }
    let img = image::RgbaImage::from_raw(N as u32, N as u32, data).expect("size");
    repeating(super::models::to_image_mips(&img, true))
}

/// A ribbon of foam along every shoreline (where the ground crosses its water's level), fading out
/// into the water. Marching squares over the height vertices.
fn foam_mesh(map: &MapData) -> Option<Mesh> {
    let (n, ny) = map.dims();
    let vert = |i: usize, j: usize| map.height(i as f32 * CELL, j as f32 * CELL);
    let (mut pos, mut uv, mut col, mut idx) = (vec![], vec![], vec![], vec![]);
    for y in 0..ny {
        for x in 0..n {
            let w = map.water_cell(x, y);
            // Underwater depth at the corners: (0,0) (1,0) (1,1) (0,1).
            let d = [w - vert(x, y), w - vert(x + 1, y), w - vert(x + 1, y + 1), w - vert(x, y + 1)];
            if d.iter().all(|&v| v > 0.0) || d.iter().all(|&v| v <= 0.0) {
                continue;
            }
            let corner = [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)];
            let mut cut: Vec<Vec2> = vec![];
            for k in 0..4 {
                let (a, b) = (k, (k + 1) % 4);
                if (d[a] > 0.0) != (d[b] > 0.0) {
                    let t = d[a] / (d[a] - d[b]);
                    let (ca, cb) = (corner[a], corner[b]);
                    cut.push(Vec2::new((x as f32 + ca.0 + (cb.0 - ca.0) * t) * CELL, (y as f32 + ca.1 + (cb.1 - ca.1) * t) * CELL));
                }
            }
            if cut.len() < 2 {
                continue;
            }
            // Towards deeper water.
            let grad = Vec2::new((d[1] + d[2] - d[0] - d[3]) * 0.5, (d[2] + d[3] - d[0] - d[1]) * 0.5).normalize_or_zero();
            for pair in cut.chunks_exact(2) {
                let (p, q) = (pair[0], pair[1]);
                let width = 0.9 + 0.7 * lattice(x as i32, y as i32, 1 << 20, 4);
                let out = grad * width;
                let first = pos.len() as u32;
                let y0 = w + 0.04;
                for (pt, v, a) in [(p, 0.0, 0.85), (q, 0.0, 0.85), (q + out, 1.0, 0.0), (p + out, 1.0, 0.0)] {
                    pos.push([pt.x, y0, pt.y]);
                    uv.push([(pt.x + pt.y) * 0.18, v]);
                    col.push([1.0, 1.0, 1.0, a]);
                }
                idx.extend_from_slice(&[first, first + 2, first + 1, first, first + 3, first + 2]);
            }
        }
    }
    if pos.is_empty() {
        return None;
    }
    // Sorts after the sea and before the fog overlay (see `terrain::water_mesh`).
    pos.push([map.world_x() * 0.5, -20_000.0, map.world_z() * 0.5]);
    uv.push([0.0, 0.0]);
    col.push([1.0, 1.0, 1.0, 0.0]);
    let count = pos.len();
    Some(
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count])
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col)
            .with_inserted_indices(Indices::U32(idx)),
    )
}

/// Spawns the shore foam and returns its (animated) material.
pub fn spawn_foam(commands: &mut Commands, map: &MapData, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>) -> Handle<StandardMaterial> {
    let foam = materials.add(StandardMaterial {
        base_color: Color::srgb(0.92, 0.96, 1.0),
        base_color_texture: Some(images.add(foam_texture())),
        alpha_mode: AlphaMode::Blend,
        unlit: true,
        depth_bias: 2.0,
        double_sided: true,
        cull_mode: None,
        ..default()
    });
    if let Some(m) = foam_mesh(map) {
        commands.spawn((Mesh3d(meshes.add(m)), MeshMaterial3d(foam.clone()), Transform::IDENTITY, NotShadowCaster, NotShadowReceiver, super::terrain::GameScene));
    }
    foam
}

/// Drifts the wave texture and makes the foam lap in and out.
pub fn water_sync(time: Res<Time>, fx: Option<Res<WaterFx>>, mut materials: ResMut<Assets<StandardMaterial>>) {
    let Some(fx) = fx else { return };
    let t = time.elapsed_secs();
    if let Some(mut m) = materials.get_mut(&fx.sea) {
        m.uv_transform = bevy::math::Affine2::from_translation(Vec2::new(t * 0.011, t * 0.007));
    }
    if let Some(mut m) = materials.get_mut(&fx.foam) {
        m.uv_transform = bevy::math::Affine2::from_translation(Vec2::new(t * 0.02, (t * 0.7).sin() * 0.12));
    }
}
