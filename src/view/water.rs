//! The water surface (`WaterMaterial`, shader `water.wesl`), purely visual. At every level a shore
//! field made from the heightmap (`shore_field`: depth below the water and distance to the waterline on
//! a fine grid) fades the water out smoothly at the shore and draws lapping foam along it. High
//! (`Settings::water_detail` 2) adds drifting wave normals (sun glints, ripples); ultra (3) adds a
//! planar reflection: a second camera, mirrored at the water level and clipped there
//! (`MirrorProjection`), renders the scene into a half-size texture the water samples
//! at its screen position (`reflection_sync`).
use super::terrain::{MainCamera, WaterPlane};
use crate::map::*;
use bevy::asset::{embedded_asset, RenderAssetUsages};
use bevy::camera::RenderTarget;
use bevy::light::Skybox;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::window::PrimaryWindow;

/// World units one repeat of the wave texture covers (mesh UVs; the shader works from world positions).
pub const TILE: f32 = 22.0;

pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExt>;

#[derive(Clone, Copy, Default, ShaderType, Debug)]
pub struct WaterParams {
    pub shore_scale: Vec2,
    pub shore_offset: Vec2,
    pub lo: Vec2,
    pub hi: Vec2,
    pub time: f32,
    pub level: u32,
    pub mirror: f32,
    pub reflect: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct WaterExt {
    #[uniform(100)]
    pub params: WaterParams,
    #[texture(101)]
    #[sampler(102)]
    pub shore: Handle<Image>,
    #[texture(103)]
    #[sampler(104)]
    pub waves: Handle<Image>,
    #[texture(105)]
    #[sampler(106)]
    pub foam: Handle<Image>,
    #[texture(107)]
    #[sampler(108)]
    pub reflection: Handle<Image>,
}

impl MaterialExtension for WaterExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://longrange/view/water.wesl".into()
    }
}

pub struct WaterPlugin;
impl Plugin for WaterPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "water.wesl");
        app.add_plugins(MaterialPlugin::<WaterMaterial>::default());
        app.add_systems(Update, (water_sync, gizmo_layers));
        // After the camera moved (Update), before transforms and frusta are worked out.
        app.add_systems(PostUpdate, reflection_sync.before(bevy::transform::TransformSystems::Propagate));
    }
}

/// The water of the scene shown: its material, the shore field and the reflection (ultra).
#[derive(Resource)]
pub struct WaterFx {
    pub mat: Handle<WaterMaterial>,
    shore: Handle<Image>,
    /// What the shore field was made from (`shore_sig`), when it was last checked, and whether the
    /// ground or water may have changed since.
    sig: u64,
    built: f32,
    pending: bool,
    /// The reflection texture and its size in pixels, while the reflection camera exists.
    reflection: Option<(Handle<Image>, UVec2)>,
    /// A 1x1 texture bound while there is no reflection.
    blank: Handle<Image>,
}

/// Render layer of the gizmo lines (rings, cones, paths): only the main camera has it, so they do not
/// show in the water's reflection.
const GIZMO_LAYER: usize = 7;

fn gizmo_layers(mut store: ResMut<GizmoConfigStore>, cams: Query<(Entity, Option<&bevy::camera::visibility::RenderLayers>), With<MainCamera>>, mut commands: Commands) {
    use bevy::camera::visibility::RenderLayers;
    if store.config::<DefaultGizmoConfigGroup>().0.render_layers != RenderLayers::layer(GIZMO_LAYER) {
        store.config_mut::<DefaultGizmoConfigGroup>().0.render_layers = RenderLayers::layer(GIZMO_LAYER);
    }
    for (e, layers) in &cams {
        if layers.is_none_or(|l| !l.intersects(&RenderLayers::layer(GIZMO_LAYER))) {
            commands.entity(e).insert(RenderLayers::from_layers(&[0, GIZMO_LAYER]));
        }
    }
}

/// The reflection camera's projection: the main camera's perspective with its near plane moved onto
/// the water surface (an oblique near plane, Lengyel 2005), so nothing below the water shows in the
/// mirror. `plane` is the water plane in the camera's view space (normal pointing to the kept side,
/// away from the camera). Bevy's own `near_clip_plane` fits the far plane through the frustum's far
/// corner, which for a camera looking down at the water (the plane almost facing it) leaves no depth
/// range at all; a fixed small scale instead keeps everything up to `1 / SCALE` times as far above
/// the water as it is in front of the camera.
#[derive(Debug, Clone)]
pub struct MirrorProjection {
    pub persp: PerspectiveProjection,
    pub plane: Vec4,
}

impl MirrorProjection {
    const SCALE: f32 = 0.05;
    fn oblique(&self, mut m: Mat4) -> Mat4 {
        // Row 3 becomes row 4 minus the scaled plane: z <= w is then "above the water".
        let r = Vec4::NEG_Z - self.plane * Self::SCALE;
        m.x_axis.z = r.x;
        m.y_axis.z = r.y;
        m.z_axis.z = r.z;
        m.w_axis.z = r.w;
        m
    }
}

impl bevy::camera::CameraProjection for MirrorProjection {
    fn get_clip_from_view(&self) -> Mat4 {
        self.oblique(self.persp.get_clip_from_view())
    }
    fn get_clip_from_view_for_sub(&self, sub_view: &bevy::camera::SubCameraView) -> Mat4 {
        self.oblique(self.persp.get_clip_from_view_for_sub(sub_view))
    }
    fn update(&mut self, width: f32, height: f32) {
        self.persp.update(width, height);
    }
    fn far(&self) -> f32 {
        self.persp.far
    }
    fn get_frustum_corners(&self, z_near: f32, z_far: f32) -> [bevy::math::Vec3A; 8] {
        self.persp.get_frustum_corners(z_near, z_far)
    }
}

/// Marks the camera that renders the mirrored scene for the water's reflection.
#[derive(Component)]
pub struct ReflectionCamera;

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


/// Margin of the shore field beyond the map border (the land there is the map mirrored, `terrain::apron_mesh`).
const MARGIN: f32 = 64.0;
/// How far from the waterline distances are measured (further counts as this far).
const REACH: f32 = 10.0;
// Encoding of the field, as in water.wesl.
const DEPTH_LO: f32 = -2.0;
const DEPTH_SPAN: f32 = 8.0;
const DIST_LO: f32 = -4.0;
const DIST_SPAN: f32 = 14.0;

fn mirror(x: f32, world: f32) -> f32 {
    let p = if x < 0.0 { -x } else if x > world { 2.0 * world - x } else { x };
    p.clamp(0.0, world)
}

/// The shore field: per texel (a quarter cell apart on maps up to 640 cells across, else half a cell)
/// the depth below the water there (red) and the signed distance to the waterline (green, > 0 in the
/// water), the waterline being the zero line of the bilinearly interpolated depth (marching squares).
/// Returns the image and the shader's `shore_scale`, `shore_offset`, `lo`, `hi`.
fn shore_field(map: &MapData) -> (Image, [Vec2; 4]) {
    let s = if map.nx.max(map.ny) <= 640 { 2 } else { 1 };
    let h = CELL / s as f32;
    let (wx, wz) = (map.world_x(), map.world_z());
    let w = ((wx + 2.0 * MARGIN) / h).ceil() as usize + 1;
    let d = ((wz + 2.0 * MARGIN) / h).ceil() as usize + 1;
    let at = |i: usize, j: usize| (i as f32 * h - MARGIN, j as f32 * h - MARGIN);
    // Depth below the water at every texel: the highest water level of the cells around it, so a
    // lake's shore is measured against the lake.
    let mut depth = vec![0.0f32; w * d];
    for j in 0..d {
        for i in 0..w {
            let (x, z) = at(i, j);
            let (mx, mz) = (mirror(x, wx), mirror(z, wz));
            let mut lvl = map.water;
            if !map.lake_of.is_empty() {
                for (ox, oz) in [(-0.5, -0.5), (0.5, -0.5), (-0.5, 0.5), (0.5, 0.5)] {
                    lvl = lvl.max(map.water_xz((mx + ox * h).clamp(0.0, wx - 0.01), (mz + oz * h).clamp(0.0, wz - 0.01)));
                }
            }
            depth[j * w + i] = lvl - map.height(mx, mz);
        }
    }
    // Distance to the waterline segments within REACH of each.
    let mut dist = vec![REACH; w * d];
    let corner = [(0usize, 0usize), (1, 0), (1, 1), (0, 1)];
    let r = (REACH / h).ceil() as isize;
    for j in 0..d - 1 {
        for i in 0..w - 1 {
            let v = corner.map(|(a, b)| depth[(j + b) * w + i + a]);
            if v.iter().all(|&x| x > 0.0) || v.iter().all(|&x| x <= 0.0) {
                continue;
            }
            let mut cut: Vec<Vec2> = Vec::with_capacity(4);
            for k in 0..4 {
                let (a, b) = (k, (k + 1) % 4);
                if (v[a] > 0.0) != (v[b] > 0.0) {
                    let t = v[a] / (v[a] - v[b]);
                    let (ca, cb) = (corner[a], corner[b]);
                    cut.push(Vec2::new(i as f32 + ca.0 as f32 + (cb.0 as f32 - ca.0 as f32) * t, j as f32 + ca.1 as f32 + (cb.1 as f32 - ca.1 as f32) * t));
                }
            }
            // A saddle (four crossings): join them the way the cell's centre value says.
            let segs: Vec<(Vec2, Vec2)> = if cut.len() == 4 {
                let centre = v.iter().sum::<f32>() * 0.25;
                if (centre > 0.0) == (v[0] > 0.0) { vec![(cut[0], cut[1]), (cut[2], cut[3])] } else { vec![(cut[0], cut[3]), (cut[1], cut[2])] }
            } else if cut.len() == 2 {
                vec![(cut[0], cut[1])]
            } else {
                vec![]
            };
            for (a, b) in segs {
                let ab = b - a;
                let len2 = ab.length_squared().max(1e-12);
                for y in (j as isize - r).max(0)..=(j as isize + 1 + r).min(d as isize - 1) {
                    for x in (i as isize - r).max(0)..=(i as isize + 1 + r).min(w as isize - 1) {
                        let q = Vec2::new(x as f32, y as f32);
                        let t = ((q - a).dot(ab) / len2).clamp(0.0, 1.0);
                        let e = (q - (a + ab * t)).length() * h;
                        let k = y as usize * w + x as usize;
                        if e < dist[k] {
                            dist[k] = e;
                        }
                    }
                }
            }
        }
    }
    let mut data = vec![0u8; w * d * 4];
    for k in 0..w * d {
        let sd = if depth[k] > 0.0 { dist[k] } else { -dist[k] };
        data[k * 4] = (((depth[k] - DEPTH_LO) / DEPTH_SPAN).clamp(0.0, 1.0) * 255.0).round() as u8;
        data[k * 4 + 1] = (((sd - DIST_LO) / DIST_SPAN).clamp(0.0, 1.0) * 255.0).round() as u8;
        data[k * 4 + 3] = 255;
    }
    let mut im = Image::new(Extent3d { width: w as u32, height: d as u32, depth_or_array_layers: 1 }, TextureDimension::D2, data, TextureFormat::Rgba8Unorm, RenderAssetUsages::default());
    use bevy::image::{ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
    im.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor { mag_filter: ImageFilterMode::Linear, min_filter: ImageFilterMode::Linear, ..default() });
    let scale = Vec2::new(1.0 / (h * w as f32), 1.0 / (h * d as f32));
    let offset = Vec2::new((MARGIN / h + 0.5) / w as f32, (MARGIN / h + 0.5) / d as f32);
    (im, [scale, offset, Vec2::splat(-MARGIN), Vec2::new(wx + MARGIN, wz + MARGIN)])
}

/// What the shore field depends on: heights, water level and lakes.
fn shore_sig(map: &MapData) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut mix = |v: u32| h = (h ^ v as u64).wrapping_mul(0x100000001b3);
    mix(map.nx as u32);
    mix(map.ny as u32);
    mix(map.water.to_bits());
    for l in &map.lakes {
        mix(l.to_bits());
    }
    for &k in &map.lake_of {
        mix(k as u32);
    }
    for v in &map.heights {
        mix(v.to_bits());
    }
    h
}

fn blank_image() -> Image {
    Image::new_fill(Extent3d { width: 1, height: 1, depth_or_array_layers: 1 }, TextureDimension::D2, &[0, 0, 0, 0], TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::default())
}

/// Makes the water material of `map` (sea and lakes share it) and the `WaterFx` resource, whose
/// `mat` the caller sets once the material is stored.
pub fn material(map: &MapData, images: &mut Assets<Image>) -> (WaterMaterial, WaterFx) {
    let (field, [shore_scale, shore_offset, lo, hi]) = shore_field(map);
    let shore = images.add(field);
    let blank = images.add(blank_image());
    let level = crate::settings::Settings::load().water_detail.min(3) as u32;
    let mat = ExtendedMaterial {
        base: StandardMaterial {
            base_color: Color::srgba(0.05, 0.2, 0.3, 0.82),
            alpha_mode: AlphaMode::Blend,
            perceptual_roughness: 0.08,
            reflectance: 0.7,
            ..default()
        },
        extension: WaterExt {
            params: WaterParams { shore_scale, shore_offset, lo, hi, time: 0.0, level, mirror: map.water, reflect: 0.0 },
            shore: shore.clone(),
            waves: images.add(wave_normals()),
            foam: images.add(foam_texture()),
            reflection: blank.clone(),
        },
    };
    let fx = WaterFx { mat: Handle::default(), shore, sig: shore_sig(map), built: 0.0, pending: false, reflection: None, blank };
    (mat, fx)
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


/// Animates the water, follows the water setting and rebuilds the shore field when the ground or the
/// water changed (craters, terraforming, the editor), at most once a second.
pub fn water_sync(
    time: Res<Time>,
    settings: Option<Res<crate::settings::Settings>>,
    map: Option<Res<MapData>>,
    ground: Option<Res<crate::sim::Ground>>,
    fx: Option<ResMut<WaterFx>>,
    mut mats: ResMut<Assets<WaterMaterial>>,
    mut images: ResMut<Assets<Image>>,
    mut last_ground: Local<u64>,
) {
    let Some(mut fx) = fx else { return };
    let now = time.elapsed_secs();
    if let Some(map) = &map {
        // In a game the ground only changes through `Ground`; the editor edits the map itself.
        let changed = match &ground {
            Some(g) => std::mem::replace(&mut *last_ground, g.hash) != g.hash,
            None => map.is_changed(),
        };
        fx.pending |= changed;
        if fx.pending && now - fx.built >= 1.0 {
            fx.pending = false;
            fx.built = now;
            let sig = shore_sig(map);
            if sig != fx.sig {
                let (field, [shore_scale, shore_offset, lo, hi]) = shore_field(map);
                if let Some(mut im) = images.get_mut(&fx.shore) {
                    *im = field;
                }
                if let Some(mut m) = mats.get_mut(&fx.mat) {
                    let p = &mut m.extension.params;
                    (p.shore_scale, p.shore_offset, p.lo, p.hi) = (shore_scale, shore_offset, lo, hi);
                }
                fx.sig = sig;
            }
        }
    }
    let level = settings.as_ref().map(|s| s.water_detail.min(3) as u32);
    let reflection = fx.reflection.as_ref().map(|r| r.0.clone());
    if let Some(mut m) = mats.get_mut(&fx.mat) {
        let p = &mut m.extension.params;
        p.time = now % 3600.0;
        if let Some(l) = level {
            p.level = l;
        }
        p.reflect = if reflection.is_some() { 1.0 } else { 0.0 };
        let want = reflection.unwrap_or_else(|| fx.blank.clone());
        if m.extension.reflection != want {
            m.extension.reflection = want;
        }
    }
}

/// The reflection camera (ultra water): exists while a scene with water is shown at that level,
/// follows the main camera mirrored at the water level the camera looks at, renders at half the
/// window's size, with the main camera's sky and haze.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn reflection_sync(
    mut commands: Commands,
    settings: Option<Res<crate::settings::Settings>>,
    map: Option<Res<MapData>>,
    fx: Option<ResMut<WaterFx>>,
    water: Query<(), With<WaterPlane>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    main: Query<(&Transform, &Projection, Option<&Skybox>, Option<&DistanceFog>), (With<MainCamera>, Without<ReflectionCamera>)>,
    mut cams: Query<(Entity, &mut Transform, &mut Projection, &mut RenderTarget, Option<&Skybox>), With<ReflectionCamera>>,
    mut images: ResMut<Assets<Image>>,
) {
    let level = settings.map_or(0, |s| s.water_detail);
    let main = main.iter().next();
    let window = windows.iter().next();
    let Some(mut fx) = fx else {
        for (e, ..) in cams.iter() {
            commands.entity(e).despawn();
        }
        return;
    };
    let (Some(map), Some((cam, Projection::Perspective(proj), sky, fog)), Some(window), true, true) = (map, main, window, level >= 3, !water.is_empty()) else {
        for (e, ..) in cams.iter() {
            commands.entity(e).despawn();
        }
        // `WaterFx` outlives the scene (main menu): drop the texture with the camera.
        if let Some((h, _)) = fx.reflection.take() {
            images.remove(h.id());
        }
        return;
    };
    let size = UVec2::new(window.physical_width() / 2, window.physical_height() / 2).max(UVec2::ONE);
    let image = match &fx.reflection {
        Some((h, s)) if *s == size => h.clone(),
        old => {
            if let Some((h, _)) = old {
                images.remove(h.id());
            }
            let h = images.add(Image::new_target_texture(size.x, size.y, TextureFormat::Bgra8UnormSrgb, None));
            fx.reflection = Some((h.clone(), size));
            h
        }
    };
    // Mirror at the water level where the camera looks (the sea, or the lake in the middle of the view).
    let f = cam.forward();
    let mut level_y = map.water;
    if f.y < -0.01 {
        let t = (map.water - cam.translation.y) / f.y;
        let hit = cam.translation + f * t;
        level_y = map.water_xz(hit.x.clamp(0.0, map.world_x() - 0.01), hit.z.clamp(0.0, map.world_z() - 0.01));
    }
    if cam.translation.y <= level_y + 0.05 {
        level_y = map.water;
    }
    let m = Mat4::from_translation(Vec3::Y * level_y) * Mat4::from_scale(Vec3::new(1.0, -1.0, 1.0)) * Mat4::from_translation(Vec3::NEG_Y * level_y) * cam.to_matrix();
    let t = Transform::from_matrix(m);
    // Only what lies above the water: the near plane is the water surface (`MirrorProjection`). The
    // mirror camera's view space is the main camera's of the mirrored world, so the kept side lies
    // below the plane there.
    let clip = (cam.rotation.inverse() * Vec3::NEG_Y).extend(level_y + 0.02 - cam.translation.y);
    let mut found = false;
    for (e, mut tr, mut pr, mut target, own_sky) in cams.iter_mut() {
        if found {
            commands.entity(e).despawn();
            continue;
        }
        found = true;
        *tr = t;
        if let Projection::Custom(c) = &mut *pr {
            if let Some(m) = c.get_mut::<MirrorProjection>() {
                m.persp.fov = proj.fov;
                m.persp.near = proj.near;
                m.persp.far = proj.far;
                m.plane = clip;
            }
        }
        if !matches!(&*target, RenderTarget::Image(i) if i.handle == image) {
            *target = RenderTarget::Image(image.clone().into());
        }
        match (sky, own_sky) {
            (Some(s), Some(o)) if s.image == o.image && s.brightness == o.brightness => {}
            (Some(s), _) => {
                commands.entity(e).insert(s.clone());
            }
            (None, Some(_)) => {
                commands.entity(e).remove::<Skybox>();
            }
            (None, None) => {}
        }
        match fog {
            Some(f) => {
                commands.entity(e).insert(f.clone());
            }
            None => {
                commands.entity(e).remove::<DistanceFog>();
            }
        }
    }
    if !found {
        let mut e = commands.spawn((
            Camera3d::default(),
            Camera {
                order: -1,
                // The mirror flips the winding of every triangle.
                invert_culling: true,
                clear_color: ClearColorConfig::Custom(Color::srgb(0.5, 0.6, 0.7)),
                ..default()
            },
            RenderTarget::Image(image.clone().into()),
            Msaa::Off,
            t,
            Projection::custom(MirrorProjection { persp: PerspectiveProjection { near_clip_plane: PerspectiveProjection::default().near_clip_plane, ..proj.clone() }, plane: clip }),
            ReflectionCamera,
        ));
        if let Some(s) = sky {
            e.insert(s.clone());
        }
        if let Some(f) = fog {
            e.insert(f.clone());
        }
    }
}
