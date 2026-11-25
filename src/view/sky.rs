//! Skies: a map names one (`MapData::sky`), the view draws it as a cubemap skybox behind everything,
//! tints the distance haze with its horizon, and takes the sun and ambient light from it.
//! Everything is made in code (no files): `cube_image` shades each face pixel from a direction,
//! `cloud_image` makes the tiling cloud layer that shows only when the camera is far up
//! (`Settings::clouds`). Purely visual: never part of the simulation.
use super::terrain::{GameScene, MainCamera, Sun};
use crate::map::MapData;
use bevy::asset::RenderAssetUsages;
use bevy::light::{EnvironmentMapLight, NotShadowCaster, NotShadowReceiver, Skybox};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};

/// One sky. Colours are sRGB.
pub struct Sky {
    pub id: &'static str,
    pub name: &'static str,
    pub zenith: [f32; 3],
    pub horizon: [f32; 3],
    pub ground: [f32; 3],
    /// Direction to the sun (or the moon at night); also where the sun light comes from.
    pub sun: [f32; 3],
    pub sun_col: [f32; 3],
    /// Strength of the glow round the sun.
    pub glow: f32,
    /// Share of the sky under clouds, 0..1.
    pub clouds: f32,
    pub cloud_col: [f32; 3],
    /// Stars (night), 0 = none.
    pub stars: f32,
    pub light_col: [f32; 3],
    pub illuminance: f32,
    pub ambient_col: [f32; 3],
    pub ambient: f32,
    /// Opacity of the cloud layer seen from far above (0 = none).
    pub layer: f32,
}

pub const SKIES: [Sky; 6] = [
    Sky { id: "clear", name: "Clear day", zenith: [0.16, 0.38, 0.78], horizon: [0.64, 0.78, 0.93], ground: [0.42, 0.48, 0.5], sun: [0.45, 0.8, 0.38], sun_col: [1.0, 0.95, 0.8], glow: 0.5, clouds: 0.35, cloud_col: [1.0, 1.0, 1.0], stars: 0.0, light_col: [1.0, 1.0, 1.0], illuminance: 13000.0, ambient_col: [0.75, 0.82, 1.0], ambient: 260.0, layer: 0.55 },
    Sky { id: "hazy", name: "Hazy", zenith: [0.34, 0.5, 0.74], horizon: [0.86, 0.79, 0.63], ground: [0.55, 0.5, 0.4], sun: [0.5, 0.75, 0.4], sun_col: [1.0, 0.92, 0.7], glow: 0.9, clouds: 0.12, cloud_col: [1.0, 0.95, 0.85], stars: 0.0, light_col: [1.0, 0.93, 0.8], illuminance: 13500.0, ambient_col: [0.95, 0.86, 0.72], ambient: 260.0, layer: 0.3 },
    Sky { id: "overcast", name: "Overcast", zenith: [0.5, 0.54, 0.6], horizon: [0.76, 0.78, 0.81], ground: [0.45, 0.47, 0.5], sun: [0.45, 0.8, 0.38], sun_col: [0.0, 0.0, 0.0], glow: 0.0, clouds: 1.0, cloud_col: [0.86, 0.88, 0.91], stars: 0.0, light_col: [0.9, 0.93, 1.0], illuminance: 8500.0, ambient_col: [0.8, 0.85, 0.95], ambient: 340.0, layer: 0.8 },
    Sky { id: "sunset", name: "Sunset", zenith: [0.14, 0.18, 0.42], horizon: [1.0, 0.56, 0.26], ground: [0.35, 0.28, 0.3], sun: [0.78, 0.26, 0.58], sun_col: [1.0, 0.8, 0.5], glow: 1.2, clouds: 0.5, cloud_col: [1.0, 0.62, 0.42], stars: 0.0, light_col: [1.0, 0.7, 0.45], illuminance: 9500.0, ambient_col: [0.6, 0.52, 0.72], ambient: 210.0, layer: 0.5 },
    Sky { id: "night", name: "Night", zenith: [0.008, 0.016, 0.06], horizon: [0.06, 0.09, 0.19], ground: [0.03, 0.04, 0.07], sun: [0.3, 0.6, -0.5], sun_col: [0.85, 0.9, 1.0], glow: 0.25, clouds: 0.2, cloud_col: [0.18, 0.22, 0.34], stars: 1.0, light_col: [0.5, 0.6, 1.0], illuminance: 3800.0, ambient_col: [0.35, 0.45, 0.85], ambient: 230.0, layer: 0.25 },
    Sky { id: "arctic", name: "Cold sky", zenith: [0.42, 0.6, 0.84], horizon: [0.86, 0.92, 0.98], ground: [0.8, 0.85, 0.9], sun: [0.5, 0.42, 0.4], sun_col: [1.0, 0.97, 0.9], glow: 0.6, clouds: 0.25, cloud_col: [0.97, 0.98, 1.0], stars: 0.0, light_col: [0.92, 0.96, 1.0], illuminance: 11000.0, ambient_col: [0.85, 0.92, 1.0], ambient: 300.0, layer: 0.45 },
];

/// The sky of that id; an empty or unknown one is the clear day.
pub fn find(id: &str) -> &'static Sky {
    SKIES.iter().find(|s| s.id == id).unwrap_or(&SKIES[0])
}

/// The sky a biome gets when the map does not name one.
pub fn for_biome(b: crate::surface::Biome) -> &'static str {
    use crate::surface::Biome;
    match b {
        Biome::Temperate => "clear",
        Biome::Desert => "hazy",
        Biome::Arctic => "arctic",
        Biome::Rocky => "overcast",
    }
}

impl Sky {
    pub fn sun_dir(&self) -> Vec3 {
        Vec3::from(self.sun).normalize()
    }
    pub fn horizon_color(&self) -> Color {
        Color::srgb(self.horizon[0], self.horizon[1], self.horizon[2])
    }
    pub fn light(&self) -> DirectionalLight {
        DirectionalLight { color: Color::srgb(self.light_col[0], self.light_col[1], self.light_col[2]), illuminance: self.illuminance, shadow_maps_enabled: true, ..default() }
    }
    pub fn sun_transform(&self) -> Transform {
        Transform::IDENTITY.looking_to(-self.sun_dir(), Vec3::Y)
    }
    pub fn ambient_light(&self) -> GlobalAmbientLight {
        GlobalAmbientLight { color: Color::srgb(self.ambient_col[0], self.ambient_col[1], self.ambient_col[2]), brightness: self.ambient, ..default() }
    }
}

// ---- noise ----

fn hash(x: i32, y: i32, z: i32) -> f32 {
    let mut n = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263) ^ (z as u32).wrapping_mul(2_246_822_519);
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    n ^= n >> 16;
    (n & 0x00ff_ffff) as f32 / 0x0100_0000 as f32
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    smooth(((x - a) / (b - a)).clamp(0.0, 1.0))
}

/// Value noise; `wrap` > 0 repeats it every `wrap` lattice steps in x and y (tiling textures).
fn noise(p: Vec3, wrap: i32) -> f32 {
    let (f, i) = (p.floor(), p.floor().as_ivec3());
    let t = p - f;
    let w = |v: i32| if wrap > 0 { v.rem_euclid(wrap) } else { v };
    let mut acc = 0.0;
    for dz in 0..2 {
        for dy in 0..2 {
            for dx in 0..2 {
                let wgt = (if dx == 0 { 1.0 - smooth(t.x) } else { smooth(t.x) }) * (if dy == 0 { 1.0 - smooth(t.y) } else { smooth(t.y) }) * (if dz == 0 { 1.0 - smooth(t.z) } else { smooth(t.z) });
                acc += wgt * hash(w(i.x + dx), w(i.y + dy), i.z + dz);
            }
        }
    }
    acc
}

fn fbm(p: Vec3, octaves: u32, wrap: i32) -> f32 {
    let (mut sum, mut amp, mut norm, mut q, mut wr) = (0.0, 0.5, 0.0, p, wrap);
    for k in 0..octaves {
        sum += amp * noise(q + Vec3::splat(k as f32 * 17.3), wr);
        norm += amp;
        amp *= 0.5;
        q *= 2.0;
        wr *= 2;
    }
    sum / norm
}

// ---- the cubemap ----

/// Colour (sRGB) of the sky in direction `d`.
fn shade(s: &Sky, d: Vec3) -> Vec3 {
    let (zenith, horizon, ground) = (Vec3::from(s.zenith), Vec3::from(s.horizon), Vec3::from(s.ground));
    let e = d.y;
    let mut col = if e >= 0.0 { horizon.lerp(zenith, e.powf(0.45)) } else { horizon.lerp(ground, smoothstep(0.0, -0.25, e)) };
    let sun = s.sun_dir();
    let c = d.dot(sun).max(0.0);
    // Glow round the sun, strongest and widest at low suns (sunset).
    let glow = (c.powf(48.0) * 0.55 + c.powf(6.0) * 0.18) * s.glow;
    col += Vec3::from(s.sun_col) * glow;
    // Stars.
    if s.stars > 0.0 && e > -0.05 {
        let cell = (d * 170.0).floor().as_ivec3();
        let h = hash(cell.x, cell.y, cell.z);
        if h > 0.9965 {
            let b = ((h - 0.9965) / 0.0035).powf(2.0) * 0.9 + 0.1;
            col += Vec3::splat(b * s.stars) * smoothstep(-0.05, 0.15, e);
        }
    }
    // Sun disc, or the moon.
    let disc_edge = if s.stars > 0.0 { 0.9988 } else { 0.99965 };
    let disc = smoothstep(disc_edge, disc_edge + 0.0004, c);
    if s.sun_col[0] > 0.0 {
        col = col.lerp(Vec3::from(s.sun_col) * 1.6, disc);
    }
    // Clouds: a stretched noise field, thinning out towards the horizon.
    if s.clouds > 0.0 && e > -0.02 {
        let p = Vec3::new(d.x * 3.2, d.y * 7.0, d.z * 3.2) / (0.35 + d.y.max(0.0) * 0.65);
        let n = fbm(p, 5, 0);
        let cover = smoothstep(0.62 - 0.3 * s.clouds, 0.78 - 0.1 * s.clouds, n) * smoothstep(-0.02, 0.16, e);
        // Lit side towards the sun, darker belly.
        let lit = 0.78 + 0.3 * (n - 0.5) + 0.25 * c;
        col = col.lerp(Vec3::from(s.cloud_col) * lit.clamp(0.4, 1.15), cover * 0.92);
    }
    col
}

/// Developer aid (`--skies out.png`): every sky as an equirectangular picture, one under the other.
pub fn preview_sheet(path: &str) {
    let (w, h) = (768usize, 320usize);
    let mut img = image::RgbaImage::new(w as u32, (h * SKIES.len()) as u32);
    for (k, s) in SKIES.iter().enumerate() {
        for y in 0..h {
            for x in 0..w {
                let (yaw, pitch) = (x as f32 / w as f32 * std::f32::consts::TAU, (0.5 - y as f32 / h as f32) * std::f32::consts::PI);
                let d = Vec3::new(pitch.cos() * yaw.sin(), pitch.sin(), -pitch.cos() * yaw.cos());
                let c = shade(s, d);
                img.put_pixel(x as u32, (k * h + y) as u32, image::Rgba([(c.x.clamp(0.0, 1.0) * 255.0) as u8, (c.y.clamp(0.0, 1.0) * 255.0) as u8, (c.z.clamp(0.0, 1.0) * 255.0) as u8, 255]));
            }
        }
    }
    img.save(path).expect("save the sky sheet");
}

pub const FACE: usize = 256;

/// Direction of texel (`u`, `v` in -1..1) of cube face `f` (the layout of `cube_image`).
fn face_dir(f: usize, u: f32, v: f32) -> Vec3 {
    match f {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        _ => Vec3::new(-u, -v, -1.0),
    }
    .normalize()
}

/// The face and texel coordinates (0..1) direction `d` falls on (inverse of `face_dir`).
fn dir_face(d: Vec3) -> (usize, f32, f32) {
    let a = d.abs();
    let (f, u, v) = if a.x >= a.y && a.x >= a.z {
        if d.x > 0.0 { (0, -d.z / a.x, -d.y / a.x) } else { (1, d.z / a.x, -d.y / a.x) }
    } else if a.y >= a.z {
        if d.y > 0.0 { (2, d.x / a.y, d.z / a.y) } else { (3, d.x / a.y, -d.z / a.y) }
    } else if d.z > 0.0 {
        (4, d.x / a.z, -d.y / a.z)
    } else {
        (5, -d.x / a.z, -d.y / a.z)
    };
    (f, (u + 1.0) * 0.5, (v + 1.0) * 0.5)
}

/// Side of the environment map's sharpest level.
const ENV_FACE: usize = 64;

/// The sky as an environment map for reflections on glass and metal (ultra building detail): a
/// specular cubemap whose mip levels are the sky blurred over wider and wider cones (Bevy picks the
/// level by roughness), prefiltered here once per sky, and a black diffuse map, so the lighting of
/// rough surfaces stays as the sun and ambient light make it.
pub fn env_light(s: &Sky, images: &mut Assets<Image>) -> EnvironmentMapLight {
    let n = ENV_FACE;
    let base: Vec<Vec3> = (0..6 * n * n)
        .map(|i| {
            let (f, y, x) = (i / (n * n), (i / n) % n, i % n);
            let d = face_dir(f, (x as f32 + 0.5) / n as f32 * 2.0 - 1.0, (y as f32 + 0.5) / n as f32 * 2.0 - 1.0);
            // Below the horizon the skybox is dark "ground"; a window seen from above should
            // mirror hazy land instead: the horizon, dimmed further down.
            let c = if d.y >= 0.0 { shade(s, d) } else { shade(s, Vec3::new(d.x, 0.0, d.z).normalize_or(Vec3::X)) * (0.78 - 0.33 * smoothstep(0.0, -0.6, d.y)) };
            c.min(Vec3::splat(1.0))
        })
        .collect();
    let look = |d: Vec3| -> Vec3 {
        let (f, u, v) = dir_face(d);
        let (x, y) = (((u * n as f32) as usize).min(n - 1), ((v * n as f32) as usize).min(n - 1));
        base[f * n * n + y * n + x]
    };
    let levels = n.trailing_zeros() as usize + 1;
    // Layer-major: every level of face 0, then of face 1...
    let mut data = Vec::new();
    for f in 0..6 {
        for k in 0..levels {
            let m = (n >> k).max(1);
            let rough = k as f32 / (levels - 1) as f32;
            // Half-angle of the blur cone: about the width of the GGX lobe at this roughness.
            let cone = (rough * rough * 1.25).min(1.4);
            for y in 0..m {
                for x in 0..m {
                    let d = face_dir(f, (x as f32 + 0.5) / m as f32 * 2.0 - 1.0, (y as f32 + 0.5) / m as f32 * 2.0 - 1.0);
                    let c = if k == 0 {
                        look(d)
                    } else {
                        let (t, b) = d.any_orthonormal_pair();
                        let (mut sum, mut wsum) = (Vec3::ZERO, 0.0);
                        const SAMPLES: usize = 40;
                        for i in 0..SAMPLES {
                            // Golden-angle spiral over the cone, weighted towards its axis.
                            let r = ((i as f32 + 0.5) / SAMPLES as f32).sqrt();
                            let a = i as f32 * 2.399_963;
                            let ang = r * cone;
                            let dir = (d * ang.cos() + (t * a.cos() + b * a.sin()) * ang.sin()).normalize();
                            let w = 1.0 - 0.7 * r * r;
                            sum += look(dir) * w;
                            wsum += w;
                        }
                        sum / wsum
                    };
                    for j in 0..3 {
                        data.push((c[j].clamp(0.0, 1.0) * 255.0).round() as u8);
                    }
                    data.push(255);
                }
            }
        }
    }
    // `Image::new` checks the data against level 0 only; the mip levels go in afterwards.
    let mut spec = Image::new(Extent3d { width: n as u32, height: n as u32, depth_or_array_layers: 6 }, TextureDimension::D2, vec![0; 6 * n * n * 4], TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    spec.data = Some(data);
    spec.texture_descriptor.mip_level_count = levels as u32;
    spec.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    let mut diffuse = Image::new(Extent3d { width: 1, height: 1, depth_or_array_layers: 6 }, TextureDimension::D2, vec![0, 0, 0, 255].repeat(6), TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    diffuse.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    EnvironmentMapLight { diffuse_map: images.add(diffuse), specular_map: images.add(spec), intensity: 1000.0, ..default() }
}

/// The six faces of `s` as a cubemap image.
pub fn cube_image(s: &Sky) -> Image {
    let mut data = Vec::with_capacity(6 * FACE * FACE * 4);
    for f in 0..6 {
        for y in 0..FACE {
            for x in 0..FACE {
                let (u, v) = ((x as f32 + 0.5) / FACE as f32 * 2.0 - 1.0, (y as f32 + 0.5) / FACE as f32 * 2.0 - 1.0);
                let d = match f {
                    0 => Vec3::new(1.0, -v, -u),
                    1 => Vec3::new(-1.0, -v, u),
                    2 => Vec3::new(u, 1.0, v),
                    3 => Vec3::new(u, -1.0, -v),
                    4 => Vec3::new(u, -v, 1.0),
                    _ => Vec3::new(-u, -v, -1.0),
                }
                .normalize();
                let c = shade(s, d);
                // A little dither against banding in the smooth gradients.
                let dither = (hash(x as i32, y as i32, f as i32 + 99) - 0.5) / 255.0;
                for k in 0..3 {
                    data.push(((c[k] + dither).clamp(0.0, 1.0) * 255.0).round() as u8);
                }
                data.push(255);
            }
        }
    }
    let mut img = Image::new(Extent3d { width: FACE as u32, height: FACE as u32, depth_or_array_layers: 6 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    img.texture_view_descriptor = Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::Cube), ..default() });
    img
}

/// A tiling cloud texture: white, the alpha is the cloud.
pub fn cloud_image(s: &Sky) -> Image {
    const N: usize = 256;
    const PERIOD: i32 = 4;
    let mut data = Vec::with_capacity(N * N * 4);
    for y in 0..N {
        for x in 0..N {
            let p = Vec3::new(x as f32 / N as f32 * PERIOD as f32, y as f32 / N as f32 * PERIOD as f32, 0.0);
            let n = fbm(p, 5, PERIOD);
            let a = smoothstep(0.62 - 0.28 * s.clouds, 0.8, n);
            data.extend_from_slice(&[255, 255, 255, (a * 255.0) as u8]);
        }
    }
    let mut img = Image::new(Extent3d { width: N as u32, height: N as u32, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default());
    img.sampler = bevy::image::ImageSampler::Descriptor(bevy::image::ImageSamplerDescriptor {
        address_mode_u: bevy::image::ImageAddressMode::Repeat,
        address_mode_v: bevy::image::ImageAddressMode::Repeat,
        address_mode_w: bevy::image::ImageAddressMode::Repeat,
        mag_filter: bevy::image::ImageFilterMode::Linear,
        min_filter: bevy::image::ImageFilterMode::Linear,
        mipmap_filter: bevy::image::ImageFilterMode::Linear,
        ..default()
    });
    img
}

// ---- the cloud layer ----

/// The plane of clouds high above the map (shown only from far up, see `sky_sync`).
#[derive(Component)]
pub struct CloudLayer;

/// Height of the cloud plane; the camera must be above it for them to show at all.
pub const CLOUD_Y: f32 = 220.0;
/// Camera heights over which the clouds fade in.
const CLOUD_FADE: (f32, f32) = (320.0, 700.0);

/// A flat square of `size` at the cloud height. Like the water it carries an unused vertex (here far
/// above) so that its centre counts as the nearest transparent thing and it is drawn last.
fn cloud_mesh(size: f32, tiles: f32) -> Mesh {
    let h = size * 0.5;
    let positions = vec![[-h, 0.0, -h], [h, 0.0, -h], [h, 0.0, h], [-h, 0.0, h], [0.0, 100_000.0, 0.0]];
    let uvs = vec![[0.0, 0.0], [tiles, 0.0], [tiles, tiles], [0.0, tiles], [0.5, 0.5]];
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; 5])
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(vec![0, 2, 1, 0, 3, 2]))
}

/// Spawns the cloud layer of the map's sky (part of the scene, hidden until the camera is far up).
pub fn spawn_clouds(commands: &mut Commands, map: &MapData, meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>, images: &mut Assets<Image>) {
    let sky = find(&map.sky);
    if sky.layer <= 0.0 {
        return;
    }
    let world = map.world_x().max(map.world_z());
    let size = world * 8.0;
    let mat = materials.add(StandardMaterial {
        base_color: Color::srgba(sky.cloud_col[0], sky.cloud_col[1], sky.cloud_col[2], 0.0),
        base_color_texture: Some(images.add(cloud_image(sky))),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        fog_enabled: false,
        ..default()
    });
    commands.spawn((
        Mesh3d(meshes.add(cloud_mesh(size, size / 700.0))),
        MeshMaterial3d(mat),
        Transform::from_xyz(map.world_x() * 0.5, CLOUD_Y, map.world_z() * 0.5),
        Visibility::Hidden,
        NotShadowCaster,
        NotShadowReceiver,
        CloudLayer,
        GameScene,
    ));
}

// ---- applying a sky ----

/// What `sky_sync` has put on the camera.
#[derive(Default)]
pub struct Applied {
    key: Option<(String, bool, bool, bool)>,
}

/// Puts the map's sky on the camera (skybox, haze colour, clear colour), the lights of the sky on the
/// sun, and fades / drifts the cloud layer. Runs in the game and in the editor.
#[allow(clippy::too_many_arguments)]
pub fn sky_sync(
    map: Option<Res<MapData>>,
    settings: Option<Res<crate::settings::Settings>>,
    time: Res<Time>,
    mut applied: Local<Applied>,
    mut images: ResMut<Assets<Image>>,
    mut commands: Commands,
    mut clear: ResMut<ClearColor>,
    ambient: Option<ResMut<GlobalAmbientLight>>,
    mut cams: Query<(Entity, &Transform, Option<&mut DistanceFog>), With<MainCamera>>,
    mut sun: Query<(&mut DirectionalLight, &mut Transform), (With<Sun>, Without<MainCamera>)>,
    mut clouds: Query<(&MeshMaterial3d<StandardMaterial>, &mut Visibility), With<CloudLayer>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(map) = map else { return };
    let (show_sky, show_haze, show_clouds) = settings.as_ref().map_or((true, true, true), |s| (s.sky, s.haze, s.clouds));
    let sky = find(&map.sky);
    // Ultra building detail: glass and metal reflect the sky (`env_light`).
    let shine = settings.as_ref().is_some_and(|s| s.building_detail >= 3);
    let key = (sky.id.to_string(), show_sky, show_haze, shine);
    let horizon = sky.horizon_color();
    if applied.key.as_ref() != Some(&key) {
        applied.key = Some(key);
        clear.0 = horizon;
        let image = show_sky.then(|| images.add(cube_image(sky)));
        let env = shine.then(|| env_light(sky, &mut images));
        for (e, _, fog) in &mut cams {
            match &env {
                Some(l) => {
                    commands.entity(e).insert(l.clone());
                }
                None => {
                    commands.entity(e).remove::<EnvironmentMapLight>();
                }
            }
            match &image {
                Some(h) => {
                    commands.entity(e).insert(Skybox { image: Some(h.clone()), brightness: 1000.0, rotation: Quat::IDENTITY });
                }
                None => {
                    commands.entity(e).remove::<Skybox>();
                }
            }
            if let Some(mut fog) = fog {
                fog.color = horizon;
                fog.falloff = if show_haze { FogFalloff::Linear { start: 900.0, end: 4000.0 } } else { FogFalloff::Linear { start: 1.0e6, end: 2.0e6 } };
            }
        }
        for (mut light, mut t) in &mut sun {
            let (shadows, lit) = (light.shadow_maps_enabled, sky.light());
            *light = DirectionalLight { shadow_maps_enabled: shadows, ..lit };
            *t = sky.sun_transform();
        }
        if let Some(mut a) = ambient {
            *a = sky.ambient_light();
        }
    }
    // Clouds: invisible until the camera is far up, then they fade in and drift.
    let height = cams.iter().next().map_or(0.0, |(_, t, _)| t.translation.y);
    let fade = smoothstep(CLOUD_FADE.0, CLOUD_FADE.1, height) * sky.layer;
    let drift = time.elapsed_secs();
    for (mat, mut vis) in &mut clouds {
        let on = show_clouds && fade > 0.01;
        let want = if on { Visibility::Inherited } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if on {
            if let Some(mut m) = materials.get_mut(&mat.0) {
                let base = m.base_color.to_srgba();
                m.base_color = Color::srgba(base.red, base.green, base.blue, fade);
                m.uv_transform = bevy::math::Affine2::from_translation(Vec2::new(drift * 0.0012, drift * 0.0007));
            }
        }
    }
}
