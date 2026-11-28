//! Ultra effects (`Settings::effects` 3): smoke, dust, fireballs and flashes drawn as camera-facing
//! billboards with a procedural puff texture instead of the lower levels' plain spheres (`fx.rs`).
//!
//! - **puff atlas**: four cauliflower-shaped puffs (overlapping blobs, warped by noise, with a noisy
//!   density), made once on the CPU (`atlas`); its other channels hold the density's gradient, so the
//!   shader can light the puff as a lumpy ball (sun side bright, far side in shade) through Bevy's own
//!   PBR lighting, shadows and distance fog (`smoke.wesl`);
//! - **one mesh, one material per kind** (smoke, dust, soot, trail, muzzle, fire, flash), so the
//!   particles are drawn instanced; each one's age, random seed and opacity ride in its `MeshTag`;
//! - **erosion**: as a puff ages, its thin parts dissolve first (a rising density threshold), so smoke
//!   breaks up into wisps instead of fading as a disc; fire cools from white-yellow through orange and red
//!   into dark smoke;
//! - **motion** (`Billow`): burst outwards, slowed by drag, buoyant lift, drifting with the weather's wind
//!   as it gets older; sizes grow fast and then slowly (ease-out).
//!
//! Purely visual, never in the simulation.
use super::terrain::GameScene;
use bevy::asset::{embedded_asset, RenderAssetUsages};
use bevy::camera::primitives::Aabb;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::NotShadowCaster;
use bevy::mesh::{MeshTag, MeshVertexBufferLayoutRef};
use bevy::pbr::{ExtendedMaterial, MaterialExtension, MaterialExtensionKey, MaterialExtensionPipeline};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;

pub type SmokeMaterial = ExtendedMaterial<StandardMaterial, SmokeExt>;

#[derive(Clone, Copy, Default, ShaderType, Debug)]
pub struct SmokeParams {
    /// Colour of the puff (rgb, linear) and its overall opacity (a).
    pub tint: Vec4,
    /// x: 1 = fire (glows, cools into smoke), 2 = flash (pure glow); y: emissive strength;
    /// z: how far the erosion threshold climbs over the life (0..1); w: how fast fire cools.
    pub kind: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SmokeExt {
    #[uniform(100)]
    pub params: SmokeParams,
    #[texture(101)]
    #[sampler(102)]
    pub atlas: Handle<Image>,
}

impl MaterialExtension for SmokeExt {
    fn vertex_shader() -> ShaderRef {
        "embedded://longrange/view/smoke.wesl".into()
    }
    fn fragment_shader() -> ShaderRef {
        "embedded://longrange/view/smoke.wesl".into()
    }
    fn enable_prepass() -> bool {
        false
    }
    fn enable_shadows() -> bool {
        false
    }
    fn specialize(_: &MaterialExtensionPipeline, descriptor: &mut RenderPipelineDescriptor, _: &MeshVertexBufferLayoutRef, _: MaterialExtensionKey<Self>) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }
}

pub struct SmokePlugin;
impl Plugin for SmokePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "smoke.wesl");
        app.add_plugins(MaterialPlugin::<SmokeMaterial>::default());
        app.add_systems(Startup, make_assets);
    }
}

/// Mesh and materials of the ultra effects.
#[derive(Resource)]
pub struct SmokeAssets {
    quad: Handle<Mesh>,
    pub smoke: Handle<SmokeMaterial>,
    pub dust: Handle<SmokeMaterial>,
    pub soot: Handle<SmokeMaterial>,
    pub trail: Handle<SmokeMaterial>,
    pub muzzle: Handle<SmokeMaterial>,
    pub fire: Handle<SmokeMaterial>,
    pub flash: Handle<SmokeMaterial>,
}

/// Side of one puff in the atlas (2 x 2 puffs).
const CELL_PX: usize = 128;

fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343) ^ (y as u32).wrapping_mul(0xD816_3841) ^ seed.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

fn vnoise(x: f32, y: f32, seed: u32) -> f32 {
    let (ix, iy) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let a = hash2(ix, iy, seed) + (hash2(ix + 1, iy, seed) - hash2(ix, iy, seed)) * sx;
    let b = hash2(ix, iy + 1, seed) + (hash2(ix + 1, iy + 1, seed) - hash2(ix, iy + 1, seed)) * sx;
    a + (b - a) * sy
}

fn fbm(x: f32, y: f32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut f, mut norm) = (0.0, 0.5, 1.0, 0.0);
    for o in 0..5 {
        sum += vnoise(x * f, y * f, seed + o * 101) * amp;
        norm += amp;
        amp *= 0.5;
        f *= 2.03;
    }
    sum / norm
}

/// The puff atlas: r density, g/b its gradient (0.5 = flat), a fine detail.
fn atlas() -> Image {
    let side = CELL_PX * 2;
    let mut dens = vec![0.0f32; side * side];
    for v in 0..4u32 {
        let (ox, oy) = ((v as usize % 2) * CELL_PX, (v as usize / 2) * CELL_PX);
        let seed = 17 + v * 7919;
        // Blobs whose union makes the puff's billows.
        let blobs: Vec<(f32, f32, f32)> = (0..7)
            .map(|k| {
                let a = hash2(k, 1, seed) * std::f32::consts::TAU;
                let r = if k == 0 { 0.0 } else { 0.2 + hash2(k, 2, seed) * 0.28 };
                (a.cos() * r, a.sin() * r, if k == 0 { 0.5 } else { 0.24 + hash2(k, 3, seed) * 0.16 })
            })
            .collect();
        for py in 0..CELL_PX {
            for px in 0..CELL_PX {
                let x = (px as f32 + 0.5) / (CELL_PX as f32 * 0.5) - 1.0;
                let y = (py as f32 + 0.5) / (CELL_PX as f32 * 0.5) - 1.0;
                // Warped, so the blob edges are ragged.
                let wx = x + (fbm(x * 2.6 + 3.0, y * 2.6, seed) - 0.5) * 0.32;
                let wy = y + (fbm(x * 2.6, y * 2.6 + 5.0, seed + 1) - 0.5) * 0.32;
                let mut b = 0.0f32;
                for &(cx, cy, r) in &blobs {
                    let d2 = ((wx - cx).powi(2) + (wy - cy).powi(2)) / (r * r);
                    if d2 < 1.0 {
                        let t = 1.0 - d2;
                        b = b.max(t * t * (3.0 - 2.0 * t));
                    }
                }
                let n = fbm(wx * 3.5 + 11.0, wy * 3.5, seed + 2);
                let edge = {
                    let r = (x * x + y * y).sqrt();
                    let t = ((0.97 - r) / 0.12).clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                };
                dens[(oy + py) * side + ox + px] = ((b * 1.25).min(1.0) * (0.35 + 0.95 * n)).clamp(0.0, 1.0) * edge;
            }
        }
    }
    // Gradient of a blurred copy (lighting wants the broad shape, not every speck).
    let at = |d: &[f32], x: i64, y: i64| d[(y.clamp(0, side as i64 - 1) as usize) * side + x.clamp(0, side as i64 - 1) as usize];
    let mut blur = vec![0.0f32; side * side];
    for y in 0..side as i64 {
        for x in 0..side as i64 {
            let mut s = 0.0;
            for dy in -2..=2 {
                for dx in -2..=2 {
                    s += at(&dens, x + dx, y + dy);
                }
            }
            blur[y as usize * side + x as usize] = s / 25.0;
        }
    }
    let mut data = Vec::with_capacity(side * side * 4);
    for y in 0..side as i64 {
        for x in 0..side as i64 {
            let gx = (at(&blur, x + 2, y) - at(&blur, x - 2, y)) * 4.0;
            let gy = (at(&blur, x, y - 2) - at(&blur, x, y + 2)) * 4.0;
            let detail = fbm(x as f32 * 0.09, y as f32 * 0.09, 999);
            data.push((at(&dens, x, y) * 255.0).round() as u8);
            data.push(((0.5 + gx * 0.5).clamp(0.0, 1.0) * 255.0).round() as u8);
            data.push(((0.5 + gy * 0.5).clamp(0.0, 1.0) * 255.0).round() as u8);
            data.push((detail * 255.0).round() as u8);
        }
    }
    let mut img = Image::new(Extent3d { width: side as u32, height: side as u32, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD);
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    img
}

fn make_assets(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<SmokeMaterial>>, mut images: ResMut<Assets<Image>>) {
    let atlas = images.add(atlas());
    // Vertex colours: the shader reads nothing from them, but they make the standard material's
    // VERTEX_COLORS path exist, which carries age / seed / opacity to the fragment shader.
    let mut quad = Rectangle::new(2.0, 2.0).mesh().build();
    quad.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[1.0f32; 4]; 4]);
    let mut mat = |rgb: [f32; 3], alpha: f32, kind: f32, glow: f32, erode: f32, cool: f32, mode: AlphaMode| {
        materials.add(SmokeMaterial {
            base: StandardMaterial {
                base_color: Color::WHITE,
                alpha_mode: mode,
                perceptual_roughness: 1.0,
                reflectance: 0.0,
                metallic: 0.0,
                double_sided: true,
                cull_mode: None,
                ..default()
            },
            extension: SmokeExt { params: SmokeParams { tint: Vec4::new(rgb[0], rgb[1], rgb[2], alpha), kind: Vec4::new(kind, glow, erode, cool) }, atlas: atlas.clone() },
        })
    };
    let assets = SmokeAssets {
        quad: meshes.add(quad),
        smoke: mat([0.11, 0.105, 0.1], 0.85, 0.0, 0.0, 0.6, 0.0, AlphaMode::Blend),
        dust: mat([0.36, 0.30, 0.22], 0.7, 0.0, 0.0, 0.65, 0.0, AlphaMode::Blend),
        soot: mat([0.035, 0.033, 0.032], 0.9, 0.0, 0.0, 0.55, 0.0, AlphaMode::Blend),
        trail: mat([0.62, 0.62, 0.62], 0.55, 0.0, 0.0, 0.7, 0.0, AlphaMode::Blend),
        muzzle: mat([0.42, 0.40, 0.37], 0.6, 0.0, 0.0, 0.7, 0.0, AlphaMode::Blend),
        fire: mat([0.07, 0.06, 0.055], 0.95, 1.0, 14.0, 0.55, 1.6, AlphaMode::Blend),
        flash: mat([0.0, 0.0, 0.0], 1.0, 2.0, 22.0, 0.2, 0.0, AlphaMode::Add),
    };
    commands.insert_resource(assets);
}

/// One ultra-effects particle.
#[derive(Component)]
pub struct Billow {
    pub vel: Vec3,
    pub life: f32,
    pub max: f32,
    pub start: f32,
    pub end: f32,
    /// Share of the velocity lost per second (exponential).
    pub drag: f32,
    /// Upward acceleration (hot gas rises).
    pub lift: f32,
    /// Wind drift (world units / s) reached after a second of age.
    pub wind: Vec3,
    /// Seconds before it appears.
    pub delay: f32,
    /// Opacity 0..1.
    pub alpha: f32,
    pub seed: u8,
}

impl Billow {
    pub fn new(vel: Vec3, max: f32, start: f32, end: f32) -> Billow {
        Billow { vel, life: 0.0, max, start, end, drag: 1.0, lift: 0.0, wind: Vec3::ZERO, delay: 0.0, alpha: 1.0, seed: 0 }
    }
}

/// Spawns one particle (`seed` varies its puff and turn).
pub fn spawn(commands: &mut Commands, a: &SmokeAssets, mat: &Handle<SmokeMaterial>, at: Vec3, mut b: Billow, seed: u32) {
    b.seed = (seed.wrapping_mul(2654435761) >> 24) as u8;
    commands.spawn((
        Mesh3d(a.quad.clone()),
        MeshMaterial3d(mat.clone()),
        // The quad turns to face the camera in the shader: bounds of a ball, never culled edge-on.
        Aabb::from_min_max(Vec3::splat(-1.0), Vec3::splat(1.0)),
        Transform::from_translation(at).with_scale(Vec3::ZERO),
        MeshTag::new(b.seed as u32 * 256),
        b,
        NotShadowCaster,
        GameScene,
    ));
}

pub fn update_billows(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Billow, &mut Transform, &mut MeshTag)>) {
    let dt = time.delta_secs().min(0.1);
    for (e, mut b, mut t, mut tag) in &mut q {
        if b.delay > 0.0 {
            b.delay -= dt;
            continue;
        }
        b.life += dt;
        let k = b.life / b.max;
        if k >= 1.0 {
            commands.entity(e).despawn();
            continue;
        }
        let keep = (-b.drag * dt).exp();
        b.vel *= keep;
        b.vel.y += b.lift * dt;
        let drift = b.wind * b.life.min(1.0);
        t.translation += (b.vel + drift) * dt;
        let grow = 1.0 - (1.0 - k) * (1.0 - k);
        let size = b.start + (b.end - b.start) * grow;
        t.scale = Vec3::splat(size.max(0.001));
        let life = (k * 255.0) as u32;
        let alpha = (b.alpha.clamp(0.0, 1.0) * 255.0) as u32;
        let want = life | (b.seed as u32) << 8 | alpha << 16;
        if tag.value != want {
            tag.value = want;
        }
    }
}
