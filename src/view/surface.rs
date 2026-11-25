//! Ground textures on the GPU: the layer textures (procedural, or `assets/textures/terrain/<kind>.png`),
//! the layer masks, the road field and the scorch map, bound to the terrain chunks' material
//! (`SurfaceMaterial`, shader `surface.wesl`). The data lives in `MapData` (`crate::surface`).
//!
//! Scorch marks are view-only: every blast near the ground darkens the scorch map a little
//! (`SurfaceView::burn`, called by `fx::spawn_fx`); marks stay for the rest of the game.
use crate::map::*;
use crate::surface::{kind, Kind, KINDS, MAX_LAYERS, MAX_ROAD_KINDS};
use bevy::asset::{embedded_asset, RenderAssetUsages};
use bevy::image::{ImageAddressMode, ImageSampler, ImageSamplerDescriptor};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension};
use bevy::shader::ShaderRef;
use std::collections::HashMap;

pub type SurfaceMaterial = ExtendedMaterial<StandardMaterial, SurfaceExt>;

/// Side of every layer texture in pixels.
const TEX: usize = 512;
/// Scorch map texels per world unit.
const BURN_PER_UNIT: f32 = 2.0;
/// Road field texels per world unit (the field is distance-like, so this can be coarse).
const ROAD_PER_UNIT: f32 = 1.0;

#[derive(Clone, Copy, Default, ShaderType, Debug)]
pub struct SurfaceParams {
    /// Size of the map in world units (x, z).
    pub world: Vec2,
    pub layers: u32,
    pub roads: u32,
    pub bump: f32,
    pub scales: [Vec4; 4],
    pub road_soft: Vec4,
    /// Debug view (`LR_SURFACE_DEBUG`, see surface.wesl).
    pub debug: u32,
    /// 1 = cliffs on steep slopes, broad colour variation and a wet shoreline (`Settings::terrain_detail`).
    pub quality: u32,
    /// Sea level (the shoreline darkens just above it).
    pub water: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct SurfaceExt {
    #[uniform(100)]
    pub params: SurfaceParams,
    #[texture(101, dimension = "2d_array")]
    #[sampler(102)]
    pub albedo: Handle<Image>,
    #[texture(103, dimension = "2d_array")]
    #[sampler(104)]
    pub masks: Handle<Image>,
    #[texture(105)]
    #[sampler(106)]
    pub roads: Handle<Image>,
    #[texture(107)]
    #[sampler(108)]
    pub scorch: Handle<Image>,
}

impl MaterialExtension for SurfaceExt {
    fn fragment_shader() -> ShaderRef {
        "embedded://longrange/view/surface.wesl".into()
    }
}

pub struct SurfacePlugin;
impl Plugin for SurfacePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "surface.wesl");
        app.add_plugins(MaterialPlugin::<SurfaceMaterial>::default());
        app.init_resource::<TextureCache>();
        app.add_systems(Update, surface_sync);
    }
}

/// Layer textures made so far (with all their mip levels), by kind id.
#[derive(Resource, Default)]
pub struct TextureCache(HashMap<String, std::sync::Arc<Vec<u8>>>);

/// The textures of the map on show, and what has to be sent to the GPU again.
#[derive(Resource)]
pub struct SurfaceView {
    pub material: Handle<SurfaceMaterial>,
    albedo: Handle<Image>,
    masks: Handle<Image>,
    roads: Handle<Image>,
    scorch: Handle<Image>,
    /// Scorch map on the CPU (`side` x `side`, 0 = untouched).
    burn: Vec<u8>,
    /// Scorch texels across and deep.
    side: (usize, usize),
    burn_dirty: bool,
    burn_wait: f32,
    /// Layer kinds or scales changed: rebuild the layer textures.
    pub layers_dirty: bool,
    pub masks_dirty: bool,
    pub roads_dirty: bool,
}

impl SurfaceView {
    /// Darkens the ground around `p` after a blast of size `size` (about its radius).
    pub fn burn(&mut self, p: Vec2, size: f32) {
        // Soft decal: dense in the middle, a long faint skirt (the shader breaks the edge up).
        let r = (1.0 + size * 1.1).min(11.0);
        let strength = (0.30 + size * 0.12).min(0.8);
        let k = BURN_PER_UNIT;
        let (side, side_y) = (self.side.0 as i32, self.side.1 as i32);
        let (x0, x1) = (((p.x - r) * k).floor() as i32, ((p.x + r) * k).ceil() as i32);
        let (y0, y1) = (((p.y - r) * k).floor() as i32, ((p.y + r) * k).ceil() as i32);
        for y in y0.max(0)..=y1.min(side_y - 1) {
            for x in x0.max(0)..=x1.min(side - 1) {
                let q = Vec2::new((x as f32 + 0.5) / k, (y as f32 + 0.5) / k);
                // Lumpy outline: the reach varies with the angle round the centre.
                let a = (q.y - p.y).atan2(q.x - p.x);
                let lump = 1.0 + 0.22 * (a * 3.0 + p.x * 1.7).sin() + 0.12 * (a * 7.0 + p.y * 2.3).sin();
                let d = q.distance(p) / (r * lump);
                if d >= 1.0 {
                    continue;
                }
                let f = (1.0 - d * d).powi(2) * strength;
                let c = &mut self.burn[y as usize * self.side.0 + x as usize];
                *c = (*c as f32 + (255.0 - *c as f32) * f).min(255.0) as u8;
            }
        }
        self.burn_dirty = true;
    }
}

/// Texture slots: the layers first, then one per road kind.
fn slots(map: &MapData) -> (Vec<(String, f32)>, Vec<String>) {
    let layers: Vec<(String, f32)> = if map.layers.is_empty() { vec![("grass".into(), 0.0)] } else { map.layers.iter().take(MAX_LAYERS).map(|l| (l.kind.clone(), l.scale)).collect() };
    (layers, map.road_kinds())
}

fn params(map: &MapData) -> SurfaceParams {
    let (layers, roads) = slots(map);
    let mut scales = [Vec4::splat(8.0); 4];
    let all: Vec<f32> = layers.iter().map(|(k, s)| if *s > 0.0 { *s } else { kind(k).map_or(8.0, |k| k.scale) }).chain(roads.iter().map(|k| kind(k).map_or(6.0, |k| k.scale))).collect();
    for (i, s) in all.iter().enumerate().take(16) {
        scales[i / 4][i % 4] = *s;
    }
    let mut road_soft = Vec4::splat(2.4);
    for (i, k) in roads.iter().enumerate().take(MAX_ROAD_KINDS) {
        road_soft[i] = if kind(k).map_or(false, |k| k.hard) { 7.0 } else { 2.4 };
    }
    let debug = std::env::var("LR_SURFACE_DEBUG").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    SurfaceParams { world: Vec2::new(map.world_x().max(1.0), map.world_z().max(1.0)), layers: layers.len() as u32, roads: roads.len() as u32, bump: 0.05, scales, road_soft, debug, quality: 1, water: map.water }
}

fn sampler(repeat: bool, aniso: bool) -> ImageSampler {
    let mut d = ImageSamplerDescriptor::linear();
    let mode = if repeat { ImageAddressMode::Repeat } else { ImageAddressMode::ClampToEdge };
    d.address_mode_u = mode;
    d.address_mode_v = mode;
    if aniso {
        d.set_anisotropic_filter(8);
    }
    ImageSampler::Descriptor(d)
}

fn array_view() -> Option<TextureViewDescriptor<'static>> {
    Some(TextureViewDescriptor { dimension: Some(TextureViewDimension::D2Array), ..default() })
}

/// The layer textures of all slots as one array texture with mip levels. Padded with spare
/// copies for wgpu's OpenGL backend: it cannot view a single-layer texture as an array, and it
/// makes textures of 6, 12... layers cube maps (the terrain then sampled black).
fn albedo_image(map: &MapData, cache: &mut TextureCache) -> Image {
    let (layers, roads) = slots(map);
    let mut ids: Vec<String> = layers.into_iter().map(|(k, _)| k).chain(roads).collect();
    while ids.len() < 2 || ids.len() % 6 == 0 {
        ids.push(ids[0].clone());
    }
    let mut data = vec![];
    for id in &ids {
        let chain = cache.0.entry(id.clone()).or_insert_with(|| std::sync::Arc::new(with_mips(&layer_pixels(id), TEX))).clone();
        data.extend_from_slice(&chain);
    }
    let mut img = Image::new_uninit(Extent3d { width: TEX as u32, height: TEX as u32, depth_or_array_layers: ids.len() as u32 }, TextureDimension::D2, TextureFormat::Rgba8UnormSrgb, RenderAssetUsages::RENDER_WORLD);
    img.data = Some(data);
    img.texture_descriptor.mip_level_count = mip_count(TEX);
    img.texture_view_descriptor = array_view();
    img.sampler = sampler(true, true);
    img
}

/// Layer masks 1..8 as a two-layer RGBA array (layer 0 holds masks 1-4, layer 1 masks 5-8).
fn mask_image(map: &MapData) -> Image {
    let (n, ny) = (map.nx.max(1), map.ny.max(1));
    let cells = n * ny;
    let mut data = vec![0u8; cells * 8];
    for l in 1..map.layers.len().min(MAX_LAYERS) {
        let (plane, ch) = ((l - 1) / 4, (l - 1) % 4);
        let src = &map.masks[((l - 1) * cells).min(map.masks.len())..(l * cells).min(map.masks.len())];
        for (i, v) in src.iter().enumerate() {
            data[plane * cells * 4 + i * 4 + ch] = *v;
        }
    }
    let mut img = Image::new(Extent3d { width: n as u32, height: ny as u32, depth_or_array_layers: 2 }, TextureDimension::D2, data, TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD);
    img.texture_view_descriptor = array_view();
    img.sampler = sampler(false, false);
    img
}

fn road_image(map: &MapData) -> Image {
    let ((side, side_y), px) = map.road_raster(ROAD_PER_UNIT);
    let mut img = Image::new(Extent3d { width: side as u32, height: side_y as u32, depth_or_array_layers: 1 }, TextureDimension::D2, px.concat(), TextureFormat::Rgba8Unorm, RenderAssetUsages::RENDER_WORLD);
    img.sampler = sampler(false, false);
    img
}

fn scorch_image(side: (usize, usize), burn: &[u8]) -> Image {
    let mut img = Image::new(Extent3d { width: side.0 as u32, height: side.1 as u32, depth_or_array_layers: 1 }, TextureDimension::D2, burn.to_vec(), TextureFormat::R8Unorm, RenderAssetUsages::RENDER_WORLD);
    img.sampler = sampler(false, false);
    img
}

impl SurfaceView {
    fn new(map: &MapData, images: &mut Assets<Image>, mats: &mut Assets<SurfaceMaterial>, cache: &mut TextureCache) -> SurfaceView {
        let side = (((map.world_x() * BURN_PER_UNIT).round() as usize).max(1), ((map.world_z() * BURN_PER_UNIT).round() as usize).max(1));
        let burn = vec![0u8; side.0 * side.1];
        let albedo = images.add(albedo_image(map, cache));
        let masks = images.add(mask_image(map));
        let roads = images.add(road_image(map));
        let scorch = images.add(scorch_image(side, &burn));
        let material = mats.add(ExtendedMaterial {
            base: StandardMaterial { perceptual_roughness: 0.95, reflectance: 0.15, ..default() },
            extension: SurfaceExt { params: params(map), albedo: albedo.clone(), masks: masks.clone(), roads: roads.clone(), scorch: scorch.clone() },
        });
        SurfaceView { material, albedo, masks, roads, scorch, burn, side, burn_dirty: false, burn_wait: 0.0, layers_dirty: false, masks_dirty: false, roads_dirty: false }
    }
}

/// Gives new terrain chunks the ground material (making the textures for a new map) and sends
/// changed masks, roads, layers and scorch marks to the GPU.
#[allow(clippy::too_many_arguments)]
fn surface_sync(
    mut commands: Commands,
    map: Option<Res<MapData>>,
    view: Option<ResMut<SurfaceView>>,
    bare: Query<Entity, (With<super::terrain::TerrainChunk>, Without<MeshMaterial3d<SurfaceMaterial>>)>,
    mut images: ResMut<Assets<Image>>,
    mut mats: ResMut<Assets<SurfaceMaterial>>,
    mut cache: ResMut<TextureCache>,
    time: Res<Time>,
    settings: Option<Res<crate::settings::Settings>>,
) {
    let Some(map) = map else { return };
    let quality = settings.as_ref().map_or(1, |s| s.terrain_detail as u32);
    if !bare.is_empty() {
        // A new scene (game start, or the editor respawned it): textures for this map.
        let v = SurfaceView::new(&map, &mut images, &mut mats, &mut cache);
        for e in &bare {
            commands.entity(e).insert(MeshMaterial3d(v.material.clone()));
        }
        commands.insert_resource(v);
        return;
    }
    let Some(mut v) = view else { return };
    let v = &mut *v;
    let mut touched = false;
    if mats.get(&v.material).is_some_and(|m| m.extension.params.quality != quality) {
        touched = true;
    }
    if v.layers_dirty || v.roads_dirty {
        // Road kinds are texture slots too.
        let _ = images.insert(&v.albedo, albedo_image(&map, &mut cache));
        v.layers_dirty = false;
        touched = true;
    }
    if v.roads_dirty {
        let _ = images.insert(&v.roads, road_image(&map));
        v.roads_dirty = false;
    }
    if v.masks_dirty {
        let _ = images.insert(&v.masks, mask_image(&map));
        v.masks_dirty = false;
        touched = true;
    }
    v.burn_wait -= time.delta_secs();
    if v.burn_dirty && v.burn_wait <= 0.0 {
        // A few uploads a second are plenty for marks that only ever grow.
        let _ = images.insert(&v.scorch, scorch_image(v.side, &v.burn));
        v.burn_dirty = false;
        v.burn_wait = 0.3;
        touched = true;
    }
    if touched {
        // The material's bind group points at the old textures until the material changes too.
        if let Some(mut m) = mats.get_mut(&v.material) {
            m.extension.params = params(&map);
            m.extension.params.quality = quality;
        }
    }
}

// --- average colours (minimap, lobby picture, the land beyond the border)

/// The ground's colour without texture detail: the layers' average colours blended by mask, and
/// the roads. Linear RGB, as bright as the textured ground on average.
pub struct Tint {
    colors: Vec<[f32; 3]>,
    road_colors: Vec<[f32; 3]>,
    /// Texels across and deep of `roads` (0 without roads).
    road_side: (usize, usize),
    roads: Vec<[u8; 4]>,
}

fn kind_color(id: &str) -> [f32; 3] {
    kind(id).map_or([0.3, 0.3, 0.3], |k| k.color)
}

impl Tint {
    pub fn new(map: &MapData) -> Tint {
        let (side, roads) = if map.roads.is_empty() { ((0, 0), vec![]) } else { map.road_raster(0.5) };
        Tint { colors: slots(map).0.iter().map(|(k, _)| kind_color(k)).collect(), road_colors: map.road_kinds().iter().map(|k| kind_color(k)).collect(), road_side: side, roads }
    }
    pub fn color(&self, map: &MapData, x: f32, z: f32) -> [f32; 3] {
        let mut c = self.colors[0];
        let p = v2(x, z);
        for (i, lc) in self.colors.iter().enumerate().skip(1) {
            let m = map.mask_at(i, p);
            if m > 0.0 {
                let t = ((m - 0.5) * 2.6 + 0.5).clamp(0.0, 1.0);
                c = std::array::from_fn(|j| c[j] + (lc[j] - c[j]) * t);
            }
        }
        let (side, side_y) = self.road_side;
        if side > 0 && side_y > 0 {
            // Texels per world unit, as `road_raster` was asked for.
            let k = side as f32 / map.world_x().max(1.0);
            let (rx, ry) = (((x.max(0.0) * k) as usize).min(side - 1), ((z.max(0.0) * k) as usize).min(side_y - 1));
            let r = self.roads[ry * side + rx];
            for (i, rc) in self.road_colors.iter().enumerate() {
                let t = ((r[i] as f32 / 255.0 - 0.5) * 4.0 + 0.5).clamp(0.0, 1.0);
                c = std::array::from_fn(|j| c[j] + (rc[j] - c[j]) * t);
            }
        }
        c
    }
}

// --- procedural layer textures

fn hash(x: i32, y: i32, s: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ s.wrapping_mul(0xcb1a_b31f).wrapping_add(0x9e37_79b9);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xFF_FFFF) as f32 / 16_777_216.0
}

/// Value noise that tiles over the unit square with `p` lattice cells per side.
fn vnoise(u: f32, v: f32, p: i32, s: u32) -> f32 {
    let (x, y) = (u * p as f32, v * p as f32);
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let w = |a: i32| a.rem_euclid(p);
    let a = hash(w(xi), w(yi), s);
    let b = hash(w(xi + 1), w(yi), s);
    let c = hash(w(xi), w(yi + 1), s);
    let d = hash(w(xi + 1), w(yi + 1), s);
    a + (b - a) * sx + (c - a) * sy + (a - b - c + d) * sx * sy
}

/// Tileable fractal noise in about [0, 1].
fn tfbm(u: f32, v: f32, p: i32, octaves: u32, s: u32) -> f32 {
    let (mut acc, mut amp, mut norm) = (0.0, 1.0, 0.0);
    for o in 0..octaves {
        acc += vnoise(u, v, p << o, s.wrapping_add(o * 101)) * amp;
        norm += amp;
        amp *= 0.5;
    }
    acc / norm
}

/// Tileable cell noise: distances to the nearest and second nearest feature point (in cells)
/// and an id of the nearest one's cell.
fn worley(u: f32, v: f32, p: i32, s: u32) -> (f32, f32, u32) {
    let (x, y) = (u * p as f32, v * p as f32);
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (mut f1, mut f2, mut id) = (9.0f32, 9.0f32, 0u32);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let (cx, cy) = (xi + dx, yi + dy);
            let (wx, wy) = (cx.rem_euclid(p), cy.rem_euclid(p));
            let px = cx as f32 + hash(wx, wy, s);
            let py = cy as f32 + hash(wx, wy, s + 1);
            let d = ((px - x) * (px - x) + (py - y) * (py - y)).sqrt();
            if d < f1 {
                f2 = f1;
                f1 = d;
                id = (wy * p + wx) as u32;
            } else if d < f2 {
                f2 = d;
            }
        }
    }
    (f1, f2, id)
}

fn sm(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

fn mul(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

/// Colour (linear RGB) and height (0..1) of kind `k` at tile position (u, v), pixel (px, py).
fn texel(k: &Kind, u: f32, v: f32, px: i32, py: i32) -> ([f32; 3], f32) {
    let base = k.color;
    let grain = hash(px, py, 7);
    let tau = std::f32::consts::TAU;
    match k.id {
        "grass" | "meadow" => {
            let n = tfbm(u, v, 8, 3, 1);
            let blades = vnoise(u, v, 160, 2) * 0.6 + vnoise(u, v, 320, 3) * 0.4;
            // Tufts about half a unit across: the detail that still shows at play distance.
            let clump = vnoise(u, v, 20, 5) * 0.5 + vnoise(u, v, 44, 6) * 0.5;
            let dry = sm(0.5, 0.8, tfbm(u, v, 10, 3, 4)) * if k.id == "meadow" { 0.8 } else { 0.35 };
            let yellow = [base[0] * 1.7 + 0.05, base[1] * 1.15 + 0.01, base[2] * 0.8];
            let c = mix3(base, yellow, dry);
            let kk = 0.72 + 0.3 * n + 0.45 * (clump - 0.5) + 0.35 * (blades - 0.5) + 0.1 * (grain - 0.5);
            (mul(c, kk), (0.3 * n + 0.4 * clump + 0.3 * blades).clamp(0.0, 1.0))
        }
        "forest_floor" => {
            let n = tfbm(u, v, 8, 3, 11);
            let (f1, _, id) = worley(u, v, 40, 12);
            let r = 0.3 + 0.35 * hash(id as i32, 4, 14);
            let leaf = sm(r, r - 0.25, f1);
            let tone = hash(id as i32, 3, 13);
            let leafc = mix3([base[0] * 1.8, base[1] * 1.1, base[2] * 0.7], [base[0] * 0.9, base[1] * 1.5, base[2] * 0.9], tone);
            let c = mix3(mul(base, 0.6), leafc, leaf);
            (mul(c, 0.8 + 0.4 * n), (leaf * 0.6 + n * 0.4).clamp(0.0, 1.0))
        }
        "dirt" | "dirt_road" => {
            let n = tfbm(u, v, 5, 5, 21);
            let (f1, _, id) = worley(u, v, if k.id == "dirt" { 18 } else { 12 }, 22);
            let size = 0.12 + 0.22 * hash(id as i32, 3, 25);
            let pebble = sm(size + 0.08, size - 0.04, f1) * (hash(id as i32, 1, 23) > if k.id == "dirt" { 0.4 } else { 0.75 }) as u8 as f32;
            // Pebbles in the ground's own hues, some lighter, some darker.
            let stone = mul(mix3(base, [0.30, 0.28, 0.25], 0.5), 0.65 + 0.7 * hash(id as i32, 2, 24));
            let c = mix3(mul(base, 0.72 + 0.45 * n + 0.1 * (grain - 0.5)), stone, pebble * 0.6);
            (c, (0.45 * n + 0.55 * pebble).clamp(0.0, 1.0))
        }
        "mud" => {
            let n = tfbm(u, v, 6, 4, 31);
            let wet = sm(0.5, 0.3, n);
            let c = mul(base, (0.8 + 0.4 * n + 0.06 * (grain - 0.5)) * (1.0 - 0.35 * wet));
            (c, n)
        }
        "sand" => {
            let n = tfbm(u, v, 6, 4, 41);
            let c = mul(base, 0.86 + 0.25 * n + 0.14 * (grain - 0.5));
            (c, (0.65 * n + 0.35 * grain).clamp(0.0, 1.0))
        }
        "dunes" => {
            let warp = tfbm(u, v, 2, 3, 51);
            let ripple = 0.5 + 0.5 * (tau * (v * 7.0 + u * 2.0 + warp * 1.6)).sin();
            let n = tfbm(u, v, 6, 4, 52);
            let c = mul(base, 0.82 + 0.2 * ripple + 0.12 * n + 0.1 * (grain - 0.5));
            (c, (ripple * 0.65 + n * 0.35).clamp(0.0, 1.0))
        }
        "cracked" => {
            let n = tfbm(u, v, 4, 4, 61);
            let (f1, f2, id) = worley(u, v, 9, 62);
            let edge = f2 - f1;
            let crack = sm(0.09, 0.025, edge);
            let plate = 0.88 + 0.24 * hash(id as i32, 5, 63);
            let c = mul(base, plate * (0.85 + 0.25 * n + 0.06 * (grain - 0.5)) * (1.0 - 0.4 * crack));
            (c, ((1.0 - crack) * (0.55 + 0.45 * sm(0.0, 0.5, edge))).clamp(0.0, 1.0))
        }
        "snow" => {
            let n = tfbm(u, v, 6, 4, 71);
            let sparkle = (grain > 0.985) as u8 as f32 * 0.08;
            let c = mul(base, 0.9 + 0.12 * n + sparkle);
            (mix3(c, [base[0] * 0.85, base[1] * 0.9, base[2] * 1.0], sm(0.45, 0.3, n)), n)
        }
        "ice" => {
            let n = tfbm(u, v, 6, 4, 81);
            let (_, f2_f1, _) = { let (a, b, c) = worley(u, v, 5, 82); (a, b - a, c) };
            let line = sm(0.04, 0.0, f2_f1) * 0.5;
            let c = mix3(mul(base, 0.85 + 0.25 * n), [0.85, 0.9, 0.95], line);
            (c, (0.5 + 0.3 * n - 0.2 * line).clamp(0.0, 1.0))
        }
        "stone" => {
            let n = tfbm(u, v, 6, 4, 91);
            let ridge = 1.0 - (2.0 * tfbm(u, v, 6, 4, 92) - 1.0).abs();
            let strata = 0.5 + 0.5 * (tau * (v * 5.0 + n * 1.5)).sin();
            let mut c = mul(base, 0.62 + 0.5 * n + 0.12 * strata + 0.08 * (grain - 0.5));
            c = mul(c, 1.0 - 0.55 * sm(0.86, 0.97, ridge));
            let lichen = sm(0.62, 0.72, tfbm(u, v, 8, 3, 93)) * 0.4;
            c = mix3(c, [0.20, 0.22, 0.10], lichen);
            (c, (0.6 * n + 0.4 * (1.0 - ridge)).clamp(0.0, 1.0))
        }
        "gravel" => {
            let (f1, f2, id) = worley(u, v, 26, 101);
            // Rounded stones: dark where two stones meet and towards each stone's rim.
            let stone = sm(0.0, 0.2, f2 - f1) * sm(0.85, 0.35, f1);
            let tone = 0.7 + 0.6 * hash(id as i32, 7, 102);
            let warm = hash(id as i32, 8, 103);
            let c = mix3(mul(base, tone), mul([base[0] * 1.1, base[1], base[2] * 0.85], tone), warm);
            (mul(c, 0.45 + 0.55 * stone), (stone * (1.0 - f1 * 0.7)).clamp(0.0, 1.0))
        }
        "paving" => {
            // Slabs in offset rows, four by four per tile, with dark joints.
            let (gx, gy) = (u * 4.0, v * 4.0);
            let row = gy.floor();
            let gx = gx + if row as i32 % 2 == 1 { 0.5 } else { 0.0 };
            let (fx, fy) = (gx - gx.floor(), gy - row);
            let joint = sm(0.05, 0.015, fx.min(1.0 - fx).min(fy).min(1.0 - fy));
            let slab = hash((gx.floor() as i32).rem_euclid(4), row as i32, 111);
            let n = tfbm(u, v, 6, 4, 112);
            let c = mul(base, (0.85 + 0.2 * slab + 0.18 * (n - 0.5) + 0.08 * (grain - 0.5)) * (1.0 - 0.6 * joint));
            (c, ((1.0 - joint) * (0.6 + 0.3 * n)).clamp(0.0, 1.0))
        }
        "asphalt" => {
            let n = tfbm(u, v, 8, 3, 121);
            let speck = (grain > 0.9) as u8 as f32 * 0.6;
            let c = mul(base, 0.85 + 0.3 * (grain - 0.5) + 0.25 * (n - 0.5) + speck);
            (c, (0.5 * grain + 0.5 * n).clamp(0.0, 1.0))
        }
        _ => {
            let n = tfbm(u, v, 4, 4, 131);
            (mul(base, 0.8 + 0.4 * n), n)
        }
    }
}

fn to_srgb(l: f32) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let e = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    (e * 255.0).round() as u8
}

/// RGBA (sRGB colour, height in alpha) of a layer texture: `assets/textures/terrain/<id>.png`
/// if there is one (its alpha, or else its brightness, is the height), else made here.
fn layer_pixels(id: &str) -> Vec<u8> {
    let file = crate::data::asset_root().join("textures").join("terrain").join(format!("{id}.png"));
    if let Ok(img) = image::open(&file) {
        let has_alpha = img.color().has_alpha();
        let img = image::imageops::resize(&img.into_rgba8(), TEX as u32, TEX as u32, image::imageops::FilterType::Triangle);
        let mut px = img.into_raw();
        if !has_alpha {
            for p in px.chunks_exact_mut(4) {
                p[3] = ((p[0] as u32 * 3 + p[1] as u32 * 6 + p[2] as u32) / 10) as u8;
            }
        }
        return px;
    }
    let k = kind(id).unwrap_or(&KINDS[0]);
    let mut px = Vec::with_capacity(TEX * TEX * 4);
    for y in 0..TEX {
        for x in 0..TEX {
            let (u, v) = ((x as f32 + 0.5) / TEX as f32, (y as f32 + 0.5) / TEX as f32);
            let (c, h) = texel(k, u, v, x as i32, y as i32);
            px.extend_from_slice(&[to_srgb(c[0]), to_srgb(c[1]), to_srgb(c[2]), (h.clamp(0.0, 1.0) * 255.0).round() as u8]);
        }
    }
    px
}

fn mip_count(side: usize) -> u32 {
    usize::BITS - side.leading_zeros()
}

/// The image followed by all its mip levels (2x2 box filter, colour averaged in linear light).
fn with_mips(px: &[u8], side: usize) -> Vec<u8> {
    let lin: Vec<f32> = (0..256).map(|v| {
        let e = v as f32 / 255.0;
        if e <= 0.04045 { e / 12.92 } else { ((e + 0.055) / 1.055).powf(2.4) }
    }).collect();
    let mut out = px.to_vec();
    let mut cur: Vec<[f32; 4]> = px.chunks_exact(4).map(|p| [lin[p[0] as usize], lin[p[1] as usize], lin[p[2] as usize], p[3] as f32 / 255.0]).collect();
    let mut s = side;
    while s > 1 {
        let h = s / 2;
        let mut next = vec![[0f32; 4]; h * h];
        for y in 0..h {
            for x in 0..h {
                let mut acc = [0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let p = cur[(y * 2 + dy) * s + x * 2 + dx];
                    for i in 0..4 {
                        acc[i] += p[i] * 0.25;
                    }
                }
                next[y * h + x] = acc;
            }
        }
        for p in &next {
            out.extend_from_slice(&[to_srgb(p[0]), to_srgb(p[1]), to_srgb(p[2]), (p[3] * 255.0).round() as u8]);
        }
        cur = next;
        s = h;
    }
    out
}

/// Average colour of a kind's texture as it is drawn (for checking the `KINDS` table).
pub fn measured_color(id: &str) -> [f32; 3] {
    let px = layer_pixels(id);
    let lin = |e: u8| {
        let e = e as f32 / 255.0;
        if e <= 0.04045 { e / 12.92 } else { ((e + 0.055) / 1.055).powf(2.4) }
    };
    let mut acc = [0f64; 3];
    for p in px.chunks_exact(4) {
        for i in 0..3 {
            acc[i] += lin(p[i]) as f64;
        }
    }
    let n = (px.len() / 4) as f64;
    [(acc[0] / n) as f32, (acc[1] / n) as f32, (acc[2] / n) as f32]
}

/// Every kind's texture side by side (`--ground out.png`): 4 per row, 256 px each.
pub fn ground_sheet(out: &std::path::Path) -> Result<(), String> {
    let cell = 256u32;
    let cols = 4u32;
    let rows = (KINDS.len() as u32).div_ceil(cols);
    let mut img = image::RgbaImage::new(cell * cols, cell * rows);
    for (i, k) in KINDS.iter().enumerate() {
        let px = layer_pixels(k.id);
        let (ox, oy) = ((i as u32 % cols) * cell, (i as u32 / cols) * cell);
        for y in 0..cell {
            for x in 0..cell {
                let j = ((y * 2) as usize * TEX + (x * 2) as usize) * 4;
                img.put_pixel(ox + x, oy + y, image::Rgba([px[j], px[j + 1], px[j + 2], 255]));
            }
        }
    }
    img.save(out).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::{Biome, BIOMES};

    /// Layers, masks and roads survive saving and loading; every biome paints something.
    /// With `LR_SURFACE_OUT=<dir>` the minimap picture of each biome is written there.
    #[test]
    fn save_load_and_biomes() {
        let dir = std::env::temp_dir().join(format!("lr_surface_test_{}", std::process::id()));
        let mut m = MapData::generate(96, 7, 4);
        assert!(!m.roads.is_empty(), "random maps get tracks");
        m.paint_ground(Biome::Desert);
        let l = m.layer_of("paving").unwrap();
        assert!(m.paint_mask(l, m.center(), 10.0, 1.0, 1.0));
        m.save(&dir).unwrap();
        let back = MapData::load(&dir).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(back.layers, m.layers);
        assert_eq!(back.masks, m.masks);
        assert_eq!(back.roads.len(), m.roads.len());
        for (a, b) in back.roads.iter().zip(&m.roads) {
            assert_eq!(a.kind, b.kind);
            assert!(a.points.iter().zip(&b.points).all(|(p, q)| p.dist(*q) < 0.01));
        }
        // Swapping and removing layers keeps each mask with its layer.
        let mut s = m.clone();
        let top = s.layers.len() - 1;
        let mask_of = |m: &MapData, i: usize| m.masks[(i - 1) * m.mask_len()..i * m.mask_len()].to_vec();
        let (a, b) = (mask_of(&s, top - 1), mask_of(&s, top));
        s.swap_layers(top - 1);
        assert_eq!((mask_of(&s, top - 1), mask_of(&s, top)), (b, a.clone()));
        s.remove_layer(top);
        assert_eq!(mask_of(&s, top - 1), mask_of(&m, top));
        assert_eq!(s.masks.len(), (s.layers.len() - 1) * s.mask_len());
        let out = std::env::var("LR_SURFACE_OUT").ok();
        for b in BIOMES {
            let mut g = MapData::generate(128, 11, 4);
            g.paint_ground(b);
            assert!(g.masks.iter().any(|v| *v > 128), "{b:?} paints its layers");
            if let Some(o) = &out {
                let px = super::super::terrain::minimap_pixels(&g);
                image::RgbaImage::from_raw(g.nx as u32, g.ny as u32, px).unwrap().save(std::path::Path::new(o).join(format!("{b:?}.png"))).unwrap();
            }
        }
        let (side, raster) = m.road_raster(1.0);
        assert_eq!(side, (m.world_x() as usize, m.world_z() as usize));
        assert!(raster.iter().any(|p| p[0] == 255));
    }
}
