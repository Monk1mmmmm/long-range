//! Pieces of debris buildings drop (wrecks with `piece` > 0): jagged chunks that fall and tumble,
//! then lie where they landed, until a blast throws them again or shoves them rolling along. The effects setting (`Settings::effects`) decides how much of that
//! is shown: low draws a piece only once it lies on the ground, as a plain box without shadow.
use super::intel::FogGrid;
use super::ViewState;
use crate::map::*;
use crate::sim::*;
use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;

/// Shapes of the chunks (medium and high effects).
const CHUNKS: usize = 5;

#[derive(Resource)]
pub struct DebrisAssets {
    chunks: Vec<Handle<Mesh>>,
    cube: Handle<Mesh>,
    /// Concrete, rusty metal, burnt metal.
    mats: Vec<Handle<StandardMaterial>>,
}

impl DebrisAssets {
    pub fn new(meshes: &mut Assets<Mesh>, materials: &mut Assets<StandardMaterial>) -> DebrisAssets {
        let mat = |c: Color, metal: f32| StandardMaterial { base_color: c, perceptual_roughness: 0.9, metallic: metal, ..default() };
        DebrisAssets {
            chunks: (0..CHUNKS).map(|k| meshes.add(chunk_mesh(k as u64 * 7919 + 13))).collect(),
            cube: meshes.add(Cuboid::new(1.0, 1.0, 1.0)),
            mats: vec![
                materials.add(mat(Color::srgb(0.36, 0.35, 0.33), 0.0)),
                materials.add(mat(Color::srgb(0.33, 0.22, 0.15), 0.3)),
                materials.add(mat(Color::srgb(0.16, 0.15, 0.15), 0.4)),
            ],
        }
    }
}

/// A unit cube with its corners pushed about: a broken-off lump, flat shaded.
fn chunk_mesh(seed: u64) -> Mesh {
    let mut rng = Rng(seed | 1);
    let corners: Vec<Vec3> = (0..8)
        .map(|i| {
            let s = Vec3::new(if i & 1 == 0 { -0.5 } else { 0.5 }, if i & 2 == 0 { -0.5 } else { 0.5 }, if i & 4 == 0 { -0.5 } else { 0.5 });
            s * Vec3::new(rng.range(0.55, 1.25), rng.range(0.55, 1.25), rng.range(0.55, 1.25))
        })
        .collect();
    // Faces as corner indices, counter-clockwise seen from outside.
    const FACES: [[usize; 4]; 6] = [[0, 2, 3, 1], [4, 5, 7, 6], [0, 1, 5, 4], [2, 6, 7, 3], [0, 4, 6, 2], [1, 3, 7, 5]];
    let mut pos: Vec<[f32; 3]> = vec![];
    let mut nrm: Vec<[f32; 3]> = vec![];
    for f in FACES {
        for tri in [[f[0], f[1], f[2]], [f[0], f[2], f[3]]] {
            let (a, b, c) = (corners[tri[0]], corners[tri[1]], corners[tri[2]]);
            let n = (b - a).cross(c - a).normalize_or_zero();
            for p in [a, b, c] {
                pos.push(p.into());
                nrm.push(n.into());
            }
        }
    }
    Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default()).with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos).with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
}

/// Drawing state of one piece: interpolation between ticks and its tumbling.
#[derive(Component)]
pub struct DebrisVisual {
    tick: u32,
    prev: Vec3,
    cur: Vec3,
    /// Orientation lying on the ground (before the terrain tilt).
    rest: Quat,
    /// Tumbling: axis times turn rate (radians per second).
    spin: Vec3,
    /// Seconds it has been shown falling; then seconds since it landed.
    age: f32,
    landed: bool,
    /// Effects level the mesh and shadow were chosen for.
    level: u8,
}

fn hash(id: u32, k: u32) -> f32 {
    let mut r = Rng(((id as u64) << 8 | k as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    r.next();
    r.f()
}

/// Gives new pieces their model; moves, tumbles and shows them by the effects level.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn debris_visuals(
    mut commands: Commands,
    assets: Res<DebrisAssets>,
    settings: Res<crate::settings::Settings>,
    view: Res<ViewState>,
    time: Res<Time>,
    map: Res<MapData>,
    fog: Res<FogGrid>,
    fresh: Query<(Entity, &Wreck), Without<DebrisVisual>>,
    mut q: Query<(Entity, &Wreck, &mut DebrisVisual, &mut Transform, &mut Visibility, &mut Mesh3d)>,
) {
    let level = settings.effects.min(2);
    let mesh_for = |id: u32, level: u8| if level == 0 { assets.cube.clone() } else { assets.chunks[id as usize % CHUNKS].clone() };
    for (e, w) in &fresh {
        if w.piece <= 0.0 {
            continue;
        }
        let p = w.pos.at(w.y);
        let tilt = Quat::from_rotation_x((hash(w.id, 1) - 0.5) * 0.5) * Quat::from_rotation_z((hash(w.id, 2) - 0.5) * 0.5);
        let axis = Vec3::new(hash(w.id, 3) - 0.5, hash(w.id, 4) - 0.5, hash(w.id, 5) - 0.5).normalize_or(Vec3::X);
        let dv = DebrisVisual {
            tick: view.last_tick,
            prev: p,
            cur: p,
            rest: Quat::from_rotation_y(w.yaw) * tilt,
            spin: axis * (4.0 + hash(w.id, 6) * 6.0),
            age: 0.0,
            landed: !w.falling,
            level,
        };
        let mat = assets.mats[(w.id as usize / CHUNKS) % assets.mats.len()].clone();
        let mut ec = commands.entity(e);
        ec.insert((Mesh3d(mesh_for(w.id, level)), MeshMaterial3d(mat), Transform::from_translation(p), Visibility::Hidden, dv));
        if level == 0 {
            ec.insert(NotShadowCaster);
        }
    }
    let alpha = (((time.elapsed_secs_f64() - view.tick_time) / DT as f64) as f32).clamp(0.0, 1.0);
    let dt = time.delta_secs();
    for (e, w, mut dv, mut t, mut vis, mut mesh) in &mut q {
        if dv.level != level {
            dv.level = level;
            mesh.0 = mesh_for(w.id, level);
            if level == 0 {
                commands.entity(e).insert(NotShadowCaster);
            } else {
                commands.entity(e).remove::<NotShadowCaster>();
            }
        }
        let now = w.pos.at(w.y);
        if dv.tick != view.last_tick {
            dv.tick = view.last_tick;
            dv.prev = dv.cur;
            dv.cur = now;
        }
        // Seen in some unusual way (a rollback moved it): no gliding across the map.
        if dv.cur.distance(now) > 0.01 {
            dv.prev = now;
            dv.cur = now;
        }
        if dv.landed != !w.falling {
            dv.landed = !w.falling;
            dv.age = 0.0;
            if w.falling {
                // Thrown up again by a blast: tumble on from how it lay.
                dv.rest = t.rotation;
            }
        }
        dv.age += dt;
        let size = w.piece;
        let shape = Vec3::new(1.0 + hash(w.id, 7) * 0.5, 0.45 + hash(w.id, 8) * 0.35, 0.8 + hash(w.id, 9) * 0.4) * size;
        let lerped = dv.prev.lerp(dv.cur, alpha);
        if !w.falling {
            // Sliding: it rolls over the ground the way it goes.
            let step = Vec3::new(dv.cur.x - dv.prev.x, 0.0, dv.cur.z - dv.prev.z);
            let speed = step.length() / DT;
            if speed > 0.05 {
                let axis = Vec3::Y.cross(step).normalize_or_zero();
                dv.rest = Quat::from_axis_angle(axis, speed * dt / (size * 0.45).max(0.1)) * dv.rest;
            }
        }
        let ground_rot = Quat::from_rotation_arc(Vec3::Y, map.normal(lerped.x, lerped.z)) * dv.rest;
        let (pos, rot) = if w.falling {
            (lerped, Quat::from_scaled_axis(dv.spin * dv.age) * dv.rest)
        } else {
            // Settles from its tumbling within a moment.
            let k = (dv.age / 0.15).min(1.0);
            (lerped + Vec3::Y * shape.y * 0.35, if k >= 1.0 { ground_rot } else { t.rotation.slerp(ground_rot, k) })
        };
        t.translation = pos;
        t.rotation = rot;
        t.scale = shape;
        // Low effects: not shown while falling. Falling pieces only where we see (landed ones are
        // remembered like wrecks).
        let shown = if w.falling { level > 0 && fog.visible(flat(pos)) } else { true };
        let want = if shown { Visibility::Visible } else { Visibility::Hidden };
        if *vis != want {
            *vis = want;
        }
    }
}
