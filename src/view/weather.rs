//! Weather in the view (the lobby's choice, `Game.weather` -> `data::WeatherDef`): rain and snow
//! falling round the camera, the ground made wet or snowy (`SurfaceView::wet` / `snow`), and the
//! tracks land units leave, deeper and longer-lasting in rain and snow. The sky and haze are set by
//! `sky::sky_sync`; what weather does to the fight (sight, radar, spread, speed) is in the simulation.
//! Purely visual: nothing here is part of the simulation state.
use super::surface::SurfaceView;
use super::terrain::{GameScene, MainCamera};
use super::ViewState;
use crate::data::{Defs, MoveKind};
use crate::map::*;
use crate::sim::*;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::light::{NotShadowCaster, NotShadowReceiver};
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use std::collections::HashMap;

/// The falling rain or snow: one mesh, rebuilt every frame from the drops' places.
#[derive(Component)]
pub struct Precip {
    /// Each drop's place in the box round the camera's view (0-1 on every axis) and its phase (sway).
    drops: Vec<(Vec3, f32)>,
    snow: bool,
    seed: u32,
}

fn hash01(k: u32) -> f32 {
    let mut x = k.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    x = x.wrapping_mul(0x297A_2D39);
    x ^= x >> 15;
    (x >> 8) as f32 / (1u32 << 24) as f32
}

/// Wind direction of a game (fixed, from the map seed) on the ground plane.
fn wind_dir(map: &MapData) -> Vec2 {
    let a = hash01(map.seed as u32 ^ 0x5157) * std::f32::consts::TAU;
    Vec2::new(a.cos(), a.sin())
}

/// Puts the weather on the ground material, and keeps the rain or snow falling round the view.
#[allow(clippy::too_many_arguments)]
pub fn weather_sync(
    mut commands: Commands,
    game: Res<Game>,
    defs: Res<Defs>,
    map: Res<MapData>,
    view: Res<ViewState>,
    settings: Res<crate::settings::Settings>,
    time: Res<Time>,
    surface: Option<ResMut<SurfaceView>>,
    cams: Query<&GlobalTransform, With<MainCamera>>,
    mut precip: Query<(Entity, &mut Precip, &Mesh3d)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let wx = defs.rules.weather_of(game.weather);
    if let Some(mut s) = surface {
        let (wet, snow) = (wx.wet.clamp(0.0, 1.0), wx.cover.clamp(0.0, 1.0));
        if s.wet != wet || s.snow != snow || s.track_secs != wx.track_secs {
            s.wet = wet;
            s.snow = snow;
            s.track_secs = wx.track_secs;
        }
    }
    // Rain or snow, whichever is heavier (a weather with both shows the snow).
    let snow = wx.snow >= wx.rain;
    let amount = wx.snow.max(wx.rain).clamp(0.0, 1.0);
    let per = match settings.weather_detail {
        0 => 0.0,
        1 => 1500.0,
        2 => 3000.0,
        _ => 5000.0,
    } * if snow { 0.7 } else { 1.0 };
    let count = (per * amount) as usize;
    let Some(cam) = cams.iter().next() else { return };
    if count == 0 {
        for (e, ..) in &precip {
            commands.entity(e).despawn();
        }
        return;
    }
    let Ok((_, mut p, mesh)) = precip.single_mut() else {
        // First frame: the entity, then the drops on the next.
        let color = if snow { Color::srgba(1.0, 1.0, 1.0, 0.9) } else { Color::srgba(0.72, 0.76, 0.84, 0.45) };
        let mat = materials.add(StandardMaterial { base_color: color, unlit: true, alpha_mode: AlphaMode::Blend, ..default() });
        let m = meshes.add(Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD));
        commands.spawn((Mesh3d(m), MeshMaterial3d(mat), Transform::IDENTITY, Visibility::Visible, NoFrustumCulling, NotShadowCaster, NotShadowReceiver, Precip { drops: vec![], snow, seed: 1 }, GameScene));
        return;
    };
    if p.snow != snow {
        for (e, ..) in &precip {
            commands.entity(e).despawn();
        }
        return;
    }
    while p.drops.len() < count {
        let k = p.seed;
        p.seed += 1;
        p.drops.push((Vec3::new(hash01(k * 4), hash01(k * 4 + 1), hash01(k * 4 + 2)), hash01(k * 4 + 3) * std::f32::consts::TAU));
    }
    p.drops.truncate(count);

    // The box: centred on the view ray a little more than half way to the ground point, sized by
    // the zoom, so the fall looks the same at every zoom.
    let ct = cam.compute_transform();
    let fwd = ct.forward().as_vec3();
    let d = view.cam_dist.max(20.0);
    let size = d * 0.9;
    let centre = ct.translation + fwd * (d * 0.55);
    let base = centre - Vec3::splat(size * 0.5);
    let dt = time.delta_secs().min(0.1);
    let t = time.elapsed_secs();
    let wind = wind_dir(&map) * wx.wind;
    // Box units per second.
    let fall = if snow { 0.11 + 0.05 * wx.wind } else { 1.1 };
    let drift = Vec3::new(wind.x, 0.0, wind.y) * if snow { 0.25 } else { 0.55 };
    let vel = Vec3::new(drift.x, -fall, drift.z);
    for (u, ph) in p.drops.iter_mut() {
        let sway = if snow { Vec3::new((t * 1.3 + *ph).sin(), 0.0, (t * 1.1 + *ph * 1.7).cos()) * 0.02 } else { Vec3::ZERO };
        *u += (vel + sway) * dt;
        *u = u.fract_gl();
    }
    let wrap = |u: Vec3| -> Vec3 {
        let w = u * size - base;
        base + Vec3::new(w.x.rem_euclid(size), w.y.rem_euclid(size), w.z.rem_euclid(size))
    };
    let (right, up) = (ct.right().as_vec3(), ct.up().as_vec3());
    let mut pos = Vec::with_capacity(count * 4);
    let mut nor = Vec::with_capacity(count * 4);
    let mut col = Vec::with_capacity(count * 4);
    let mut idx: Vec<u32> = Vec::with_capacity(count * 6);
    let streak = vel.normalize() * (size * 0.035);
    let (rain_w, flake) = (size * 0.0011, size * 0.0016);
    for (k, (u, ph)) in p.drops.iter().enumerate() {
        let w = wrap(*u);
        let to_cam = (ct.translation - w).normalize_or_zero();
        let i = (k * 4) as u32;
        if snow {
            let s = flake * (0.7 + 0.6 * hash01(k as u32 ^ 0x3A1));
            let (r, uu) = (right * s, up * s);
            pos.extend([w - r - uu, w + r - uu, w + r + uu, w - r + uu]);
            let a = 0.6 + 0.4 * (*ph).sin().abs();
            col.extend([[1.0, 1.0, 1.0, a]; 4]);
        } else {
            let side = streak.cross(to_cam).normalize_or_zero() * rain_w;
            let tail = w - streak;
            pos.extend([tail - side, tail + side, w + side, w - side]);
            col.extend([[1.0, 1.0, 1.0, 0.0], [1.0, 1.0, 1.0, 0.0], [1.0, 1.0, 1.0, 1.0], [1.0, 1.0, 1.0, 1.0]]);
        }
        nor.extend([to_cam; 4]);
        idx.extend([i, i + 1, i + 2, i, i + 2, i + 3]);
    }
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::RENDER_WORLD);
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, nor);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    m.insert_indices(Indices::U32(idx));
    let _ = meshes.insert(&mesh.0, m);
}

/// Where each land unit stood last tick (tracks are laid from there to where it stands now).
#[derive(Default)]
pub struct TrackState {
    tick: u32,
    last: HashMap<u32, V2>,
}

/// Every sim tick, presses the tracks of the land units the player sees into the ground: two
/// tread marks for vehicles, a narrow trodden path for infantry. Rain and snow make them deeper
/// (`WeatherDef::tracks`); how long they stay is the weather's `track_secs`.
pub fn lay_tracks(
    game: Res<Game>,
    defs: Res<Defs>,
    map: Res<MapData>,
    view: Res<ViewState>,
    surface: Option<ResMut<SurfaceView>>,
    q: Query<(&Unit, &Body, &Intel), (Without<Hosted>, Without<UnderConstruction>)>,
    mut st: Local<TrackState>,
) {
    let Some(mut surface) = surface else { return };
    if st.tick == game.tick {
        return;
    }
    st.tick = game.tick;
    let wx = defs.rules.weather_of(game.weather);
    let team = game.team_of(view.me);
    let reveal = super::intel::reveal(&game, view.me);
    let mut now = HashMap::with_capacity(st.last.len());
    for (u, b, intel) in &q {
        let d = defs.get(u.def);
        if d.mv.kind != MoveKind::Land || d.is_structure() || d.mv.speed <= 0.0 {
            continue;
        }
        now.insert(u.id, b.pos);
        let Some(&from) = st.last.get(&u.id) else { continue };
        let step = from.dist(b.pos);
        // Standing still, or a jump (a rollback, a drop from a plane): nothing to press.
        if !(0.02..3.0).contains(&step) || (!reveal && intel.seen & (1 << team) == 0) {
            continue;
        }
        // Afloat (amphibious carriers): no track.
        if map.water_at(b.pos) > map.h(b.pos) + 0.05 {
            continue;
        }
        let (a, z) = (Vec2::new(from.x, from.y), Vec2::new(b.pos.x, b.pos.y));
        let w = d.size[0].max(0.3);
        if d.is_infantry() {
            surface.track(a, z, 0.25, (0.22 * wx.tracks).min(1.0));
        } else {
            let dir = (z - a).normalize_or_zero();
            let side = dir.perp() * (w * 0.36);
            let depth = ((0.3 + 0.05 * w) * wx.tracks).min(1.0);
            let half = w * 0.13;
            surface.track(a + side, z + side, half, depth);
            surface.track(a - side, z - side, half, depth);
        }
    }
    st.last = now;
}
