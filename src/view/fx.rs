//! Projectile visuals and explosion effects (simple growing/shrinking unlit blobs).
use super::intel::FogGrid;
use super::terrain::GameScene;
use super::ViewState;
use crate::map::*;
use crate::sim::*;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

#[derive(Resource)]
pub struct FxAssets {
    sphere: Handle<Mesh>,
    shell: Handle<Mesh>,
    flash: Handle<StandardMaterial>,
    fire: Handle<StandardMaterial>,
    smoke: Handle<StandardMaterial>,
    dust: Handle<StandardMaterial>,
    shell_mat: Handle<StandardMaterial>,
    missile_mat: Handle<StandardMaterial>,
    bomb_mat: Handle<StandardMaterial>,
}

impl FxAssets {
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> FxAssets {
        let glow = |c: LinearRgba| StandardMaterial { base_color: Color::BLACK, emissive: c, unlit: false, ..default() };
        FxAssets {
            sphere: meshes.add(Sphere::new(1.0).mesh().ico(2).unwrap()),
            shell: meshes.add(Cuboid::new(0.12, 0.12, 1.0)),
            flash: materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.95, 0.75), unlit: true, ..default() }),
            fire: materials.add(StandardMaterial { base_color: Color::srgb(1.0, 0.5, 0.1), unlit: true, ..default() }),
            smoke: materials.add(StandardMaterial { base_color: Color::srgba(0.12, 0.11, 0.1, 0.55), alpha_mode: AlphaMode::Blend, perceptual_roughness: 1.0, ..default() }),
            dust: materials.add(StandardMaterial { base_color: Color::srgba(0.45, 0.4, 0.32, 0.4), alpha_mode: AlphaMode::Blend, perceptual_roughness: 1.0, ..default() }),
            shell_mat: materials.add(glow(LinearRgba::rgb(9.0, 6.0, 2.0))),
            missile_mat: materials.add(StandardMaterial { base_color: Color::srgb(0.9, 0.9, 0.92), ..default() }),
            bomb_mat: materials.add(StandardMaterial { base_color: Color::srgb(0.15, 0.16, 0.15), ..default() }),
        }
    }
}

#[derive(Component)]
pub struct Puff {
    vel: Vec3,
    life: f32,
    max: f32,
    start: f32,
    end: f32,
}

#[derive(Component)]
pub struct FlashLight(f32, f32);

#[derive(Component)]
pub struct ProjVisual {
    trail: f32,
}

pub fn projectile_visuals(
    mut commands: Commands,
    assets: Res<FxAssets>,
    view: Res<ViewState>,
    time: Res<Time>,
    fog: Res<FogGrid>,
    fresh: Query<(Entity, &Projectile), Without<ProjVisual>>,
    mut q: Query<(&Projectile, &mut Transform, &mut Visibility, &mut ProjVisual)>,
) {
    for (e, p) in &fresh {
        let big = (p.damage.sqrt() * 0.07).clamp(0.35, 1.6);
        let (mat, scale) = match p.kind {
            PROJ_MISSILE => (assets.missile_mat.clone(), Vec3::new(1.2, 1.2, 0.9)),
            PROJ_BOMB => (assets.bomb_mat.clone(), Vec3::new(2.0, 2.0, 0.7) * big),
            _ => (assets.shell_mat.clone(), Vec3::new(big, big, (0.6 + big).min(2.2))),
        };
        commands.entity(e).insert((Mesh3d(assets.shell.clone()), MeshMaterial3d(mat), Transform::from_translation(p.pos.v()).with_scale(scale), Visibility::Hidden, NotShadowCaster, ProjVisual { trail: 0.0 }));
    }
    let alpha = (((time.elapsed_secs_f64() - view.tick_time) / DT as f64) as f32).clamp(0.0, 1.0);
    for (p, mut t, mut vis, mut pv) in &mut q {
        let vel = p.vel.v();
        let pos = p.pos.v() - vel * (DT * (1.0 - alpha));
        t.translation = pos;
        if vel.length_squared() > 0.01 {
            t.look_to(vel.normalize(), Vec3::Y);
        }
        let want = if fog.visible(flat(pos)) { Visibility::Visible } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
        if p.kind == PROJ_MISSILE && want == Visibility::Visible {
            pv.trail -= time.delta_secs();
            if pv.trail <= 0.0 {
                pv.trail = 0.04;
                commands.spawn((
                    Mesh3d(assets.sphere.clone()),
                    MeshMaterial3d(assets.dust.clone()),
                    Transform::from_translation(pos).with_scale(Vec3::splat(0.1)),
                    Puff { vel: Vec3::Y * 0.3, life: 0.0, max: 0.9, start: 0.1, end: 0.3 },
                    NotShadowCaster,
                    GameScene,
                ));
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_fx(
    mut commands: Commands,
    mut log: ResMut<FxLog>,
    assets: Res<FxAssets>,
    fog: Res<FogGrid>,
    map: Res<MapData>,
    view: Res<super::ViewState>,
    settings: Res<crate::settings::Settings>,
    sounds: Option<Res<super::sound::Sounds>>,
) {
    let mut lights = 0;
    let mut heard: Vec<(usize, f32, f32)> = vec![];
    for (k, f) in log.pending.drain(..).enumerate() {
        let pos = f.pos.v();
        if !fog.visible(flat(pos)) {
            continue;
        }
        // Sound: louder for bigger events, fading with the distance from the camera.
        let name = match f.kind {
            FX_MUZZLE => SOUNDS.get(f.snd as usize).copied().unwrap_or("cannon"),
            FX_DEATH => "death",
            _ => "blast",
        };
        if let (Some(i), true) = (SOUNDS.iter().position(|s| *s == name), settings.sfx_volume > 0.0) {
            let far = pos.distance(view.cam_pos) / 130.0;
            let volume = settings.sfx_volume * (0.35 + f.size.min(1.5) * 0.6) / (1.0 + far * far);
            if volume > 0.012 {
                // A little pitch variation, so a salvo is not the same sample eight times.
                heard.push((i, volume.min(1.0), 0.9 + ((k * 37 + (pos.x * 13.0) as usize) % 21) as f32 * 0.01));
            }
        }
        let mut puff = |mat: &Handle<StandardMaterial>, at: Vec3, vel: Vec3, max: f32, start: f32, end: f32| {
            commands.spawn((
                Mesh3d(assets.sphere.clone()),
                MeshMaterial3d(mat.clone()),
                Transform::from_translation(at).with_scale(Vec3::splat(start)),
                Puff { vel, life: 0.0, max, start, end },
                NotShadowCaster,
                GameScene,
            ));
        };
        let s = f.size;
        match f.kind {
            FX_MUZZLE => {
                puff(&assets.flash, pos, Vec3::ZERO, 0.09, s * 0.5, s * 1.2);
                if s > 0.4 {
                    puff(&assets.dust, pos, Vec3::Y * 0.6, 0.8, s * 0.4, s * 1.1);
                }
            }
            _ => {
                let on_ground = pos.y - map.height(pos.x, pos.z) < 1.0;
                puff(&assets.flash, pos, Vec3::ZERO, 0.16 + s * 0.02, s * 0.3, s * 0.9);
                puff(&assets.fire, pos + Vec3::Y * 0.1 * s, Vec3::Y * 0.8, 0.32 + s * 0.04, s * 0.25, s * 0.7);
                let n = if f.kind == FX_DEATH { 4 } else { 2 } + (s as usize).min(4);
                for k in 0..n {
                    let a = k as f32 * 2.4 + s;
                    let off = Vec3::new(a.cos(), 0.3, a.sin()) * s * 0.3;
                    let mat = if on_ground && k % 2 == 0 { &assets.dust } else { &assets.smoke };
                    puff(mat, pos + off, Vec3::new(a.cos() * 0.4, 1.2 + (k as f32 * 0.37) % 0.8, a.sin() * 0.4), 1.0 + s * 0.15 + (k as f32 * 0.21) % 0.6, s * 0.25, s * 0.6);
                }
                if s >= 2.0 && lights < 3 {
                    lights += 1;
                    commands.spawn((PointLight { color: Color::srgb(1.0, 0.7, 0.4), intensity: 400000.0 * s, range: s * 8.0, ..default() }, Transform::from_translation(pos + Vec3::Y * (1.0 + s * 0.3)), FlashLight(0.0, 0.25), GameScene));
                }
            }
        }
    }
    super::sound::play(&mut commands, sounds.as_deref(), heard);
}

pub fn update_fx(mut commands: Commands, time: Res<Time>, mut q: Query<(Entity, &mut Puff, &mut Transform)>, mut lights: Query<(Entity, &mut FlashLight, &mut PointLight)>) {
    let dt = time.delta_secs();
    for (e, mut p, mut t) in &mut q {
        p.life += dt;
        let k = p.life / p.max;
        if k >= 1.0 {
            commands.entity(e).despawn();
            continue;
        }
        t.translation += p.vel * dt;
        // Grow, then collapse at the very end.
        let size = (p.start + (p.end - p.start) * k.sqrt()) * (1.0 - k.powi(4));
        t.scale = Vec3::splat(size.max(0.001));
    }
    for (e, mut f, mut l) in &mut lights {
        f.0 += dt;
        if f.0 >= f.1 {
            commands.entity(e).despawn();
        } else {
            l.intensity *= 0.8;
        }
    }
}
