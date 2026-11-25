//! Grass tufts on grassy ground (`Settings::grass_detail`: 1 high, 2 ultra; 0 draws none). Ultra: denser,
//! blades that bend (three triangles each) and scattered wildflowers.
//!
//! Only the ground near where the camera looks has grass: the chunks (`CHUNK` cells) within
//! `reach` of that point get a mesh of blade triangles when the camera is low enough, built a
//! couple per frame, and chunks far from it are dropped again, so a big map costs nothing while
//! zoomed out. Each blade is one thin, double-sided triangle with vertex colours (dark at the
//! root, light and a little yellow at the tip); the UVs hold the map position so the fog of war
//! darkens grass like trees and rocks (`flora`). It is purely visual and never in the simulation.
use super::flora::FloraView;
use super::terrain::GameScene;
use crate::map::*;
use crate::settings::Settings;
use crate::surface::KINDS;
use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::PrimitiveTopology;
use bevy::prelude::*;

/// Side of a grass chunk in cells.
const CHUNK: usize = 8;
/// Above this camera height there is no grass.
const MAX_CAM: f32 = 170.0;

#[derive(Component)]
pub struct GrassChunk {
    cx: usize,
    cy: usize,
    level: u8,
}

fn hash(a: u64, b: u64) -> f32 {
    let mut h = a.wrapping_mul(0x9E3779B97F4A7C15) ^ b.wrapping_mul(0xC2B2AE3D27D4EB4F);
    h ^= h >> 31;
    h = h.wrapping_mul(0xBF58476D1CE4E5B9);
    h ^= h >> 29;
    (h & 0xFFFFFF) as f32 / 16777216.0
}

/// Colour of the ground a cell shows if it is grassy (the top layer whose mask covers the cell).
fn grass_colour(map: &MapData, i: usize) -> Option<[f32; 3]> {
    let len = map.nx * map.ny;
    let mut kind = &map.layers.first()?.kind;
    for l in 1..map.layers.len() {
        if map.masks.get((l - 1) * len + i).is_some_and(|&m| m >= 128) {
            kind = &map.layers[l].kind;
        }
    }
    if !matches!(kind.as_str(), "grass" | "meadow" | "forest_floor") {
        return None;
    }
    KINDS.iter().find(|k| k.id == kind.as_str()).map(|k| k.color)
}

/// Whether the ground of a cell is meadow (the flowery kind).
fn kind_meadow(map: &MapData, i: usize) -> bool {
    let len = map.nx * map.ny;
    let mut kind = map.layers.first().map(|l| l.kind.as_str());
    for l in 1..map.layers.len() {
        if map.masks.get((l - 1) * len + i).is_some_and(|&m| m >= 128) {
            kind = Some(map.layers[l].kind.as_str());
        }
    }
    kind == Some("meadow")
}

/// The mesh of the chunk at cell (cx, cy), relative to `origin`, or `None` without any grass.
fn chunk_mesh(map: &MapData, road: &[bool], cx: usize, cy: usize, level: u8, origin: Vec3) -> Option<Mesh> {
    let per_cell = if level >= 2 { 7 } else { 3 };
    let blades = if level >= 2 { 5 } else { 4 };
    let world = map.size();
    let (mut pos, mut nrm, mut col, mut uv) = (vec![], vec![], vec![], vec![]);
    for y in cy..(cy + CHUNK).min(map.ny) {
        for x in cx..(cx + CHUNK).min(map.nx) {
            let i = y * map.nx + x;
            if map.blocked.get(i).copied().unwrap_or(0) != 0 || road.get(i).copied().unwrap_or(false) {
                continue;
            }
            let Some(base) = grass_colour(map, i) else { continue };
            for t in 0..per_cell {
                let seed = (i as u64) * 16 + t as u64;
                let p = v2((x as f32 + hash(seed, 1)) * CELL, (y as f32 + hash(seed, 2)) * CELL);
                if map.normal(p.x, p.y).y < 0.88 {
                    continue;
                }
                let h = map.h(p);
                if h < map.water_at(p) + 0.2 {
                    continue;
                }
                // Dry patches and dark patches, broad over the map.
                let dry = hash((p.x * 0.07) as u64 + 91, (p.y * 0.07) as u64 + 17);
                let k = 0.8 + 0.5 * hash(seed, 3);
                let tip = [
                    (base[0] * 2.2 + 0.05 * dry) * k * (1.0 + 0.5 * dry),
                    base[1] * 2.1 * k,
                    base[2] * 1.5 * k * (1.0 - 0.3 * dry),
                    1.0,
                ];
                let root = [base[0] * 0.5, base[1] * 0.5, base[2] * 0.45, 1.0];
                let tuft_h = 0.22 + 0.2 * hash(seed, 4);
                let uvp = [p.x / world.x.max(1.0), p.y / world.y.max(1.0)];
                for b in 0..blades {
                    let s2 = seed * 8 + b as u64;
                    let a = hash(s2, 5) * std::f32::consts::TAU;
                    let (sa, ca) = a.sin_cos();
                    let off = Vec2::new(hash(s2, 6) - 0.5, hash(s2, 7) - 0.5) * 0.3;
                    let base_p = Vec3::new(p.x + off.x, h, p.y + off.y) - origin;
                    let bh = tuft_h * (0.6 + 0.6 * hash(s2, 8));
                    let w = 0.03 + 0.025 * hash(s2, 9);
                    let lean = Vec3::new(ca, 0.0, sa) * bh * (0.15 + 0.35 * hash(s2, 10));
                    let side = Vec3::new(-sa, 0.0, ca) * w;
                    let n = (Vec3::Y + lean / bh.max(0.01)).normalize().to_array();
                    let tip_p = base_p + lean + Vec3::Y * bh;
                    let shade = 0.85 + 0.3 * hash(s2, 11);
                    let rc = [root[0] * shade, root[1] * shade, root[2] * shade, 1.0];
                    let mut push = |v: Vec3, c: [f32; 4]| {
                        pos.push(v.to_array());
                        nrm.push(n);
                        col.push(c);
                        uv.push(uvp);
                    };
                    if level >= 2 {
                        // Ultra: a blade that bends - a quad to the knee and a point above it, the
                        // tip hanging over (the lean grows towards the end), colour fading up.
                        let knee = base_p + lean * 0.22 + Vec3::Y * bh * 0.55;
                        let mc = [(rc[0] + tip[0]) * 0.5, (rc[1] + tip[1]) * 0.5, (rc[2] + tip[2]) * 0.5, 1.0];
                        let tip_p = tip_p + Vec3::Y * -bh * 0.08;
                        for (v, c) in [(base_p - side, rc), (base_p + side, rc), (knee + side * 0.7, mc), (base_p - side, rc), (knee + side * 0.7, mc), (knee - side * 0.7, mc), (knee - side * 0.7, mc), (knee + side * 0.7, mc), (tip_p, tip)] {
                            push(v, c);
                        }
                    } else {
                        for (v, c) in [(base_p - side, rc), (base_p + side, rc), (tip_p, tip)] {
                            push(v, c);
                        }
                    }
                }
            }
            // Ultra: now and then a wildflower among the grass (meadows more than lawns).
            if level >= 2 && hash(i as u64 * 7 + 5, 40) < if kind_meadow(map, i) { 0.22 } else { 0.07 } {
                let fp = v2((x as f32 + hash(i as u64, 41)) * CELL, (y as f32 + hash(i as u64, 42)) * CELL);
                if map.normal(fp.x, fp.y).y >= 0.9 && map.h(fp) >= map.water_at(fp) + 0.2 {
                    let palette: [[f32; 3]; 5] = [[0.95, 0.95, 0.9], [0.95, 0.8, 0.2], [0.6, 0.4, 0.8], [0.9, 0.45, 0.55], [0.95, 0.95, 0.9]];
                    let petal = palette[(hash(i as u64, 43) * 5.0) as usize % 5];
                    let stem_h = 0.2 + 0.12 * hash(i as u64, 44);
                    let b = Vec3::new(fp.x, map.h(fp), fp.y) - origin;
                    let top = b + Vec3::Y * stem_h + Vec3::new(hash(i as u64, 45) - 0.5, 0.0, hash(i as u64, 46) - 0.5) * 0.08;
                    let uvp = [fp.x / world.x.max(1.0), fp.y / world.y.max(1.0)];
                    let green = [0.12, 0.3, 0.08, 1.0];
                    let n = Vec3::Y.to_array();
                    let mut push = |v: Vec3, c: [f32; 4]| {
                        pos.push(v.to_array());
                        nrm.push(n);
                        col.push(c);
                        uv.push(uvp);
                    };
                    let w = Vec3::new(0.012, 0.0, 0.0);
                    for v in [b - w, b + w, top] {
                        push(v, green);
                    }
                    // Five petals round the centre, tilted a little outwards and up.
                    let a0 = hash(i as u64, 47) * std::f32::consts::TAU;
                    let (pc, cc) = ([petal[0], petal[1], petal[2], 1.0], [0.9, 0.7, 0.1, 1.0]);
                    for k in 0..5 {
                        let (a1, a2) = (a0 + k as f32 / 5.0 * std::f32::consts::TAU, a0 + (k as f32 + 0.8) / 5.0 * std::f32::consts::TAU);
                        let d1 = Vec3::new(a1.cos(), 0.0, a1.sin());
                        let d2 = Vec3::new(a2.cos(), 0.0, a2.sin());
                        push(top, cc);
                        push(top + d1 * 0.07 + Vec3::Y * 0.025, pc);
                        push(top + d2 * 0.07 + Vec3::Y * 0.025, pc);
                    }
                }
            }
        }
    }
    if pos.is_empty() {
        return None;
    }
    Some(
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, nrm)
            .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
            .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, col),
    )
}

/// Builds the grass chunks near the point the camera looks at and drops the far ones.
#[allow(clippy::too_many_arguments)]
pub fn grass_sync(
    mut commands: Commands,
    settings: Res<Settings>,
    map: Res<MapData>,
    view: Option<Res<FloraView>>,
    cams: Query<&GlobalTransform, With<super::terrain::MainCamera>>,
    chunks: Query<(Entity, &GrassChunk)>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut mat: Local<Option<Handle<StandardMaterial>>>,
    mut road: Local<Vec<bool>>,
    ground: Option<Res<crate::sim::Ground>>,
    mut known: Local<(usize, u64)>,
) {
    let level = settings.grass_detail.min(2);
    let cam = cams.iter().next().map(|c| c.compute_transform());
    let (Some(view), Some(cam)) = (view, cam) else { return };
    // The terrain changed (craters) or a new game: start over.
    let sig = (map.nx * map.ny, ground.as_ref().map_or(0, |g| g.hash));
    if *known != sig {
        for (e, _) in &chunks {
            commands.entity(e).despawn();
        }
        *road = map.road_cells(1.3);
        *known = sig;
        *mat = None;
        return;
    }
    let fwd = cam.forward().as_vec3();
    let low = cam.translation.y < MAX_CAM && level > 0;
    // Where the view axis meets the ground.
    let focus = if fwd.y < -0.05 { cam.translation + fwd * ((cam.translation.y - map.water).max(0.0) / -fwd.y).min(400.0) } else { cam.translation };
    let reach = if level >= 2 { 95.0 } else { 70.0 };
    let size = CHUNK as f32 * CELL;
    let centre = |cx: usize, cy: usize| Vec2::new((cx as f32 + CHUNK as f32 * 0.5) * CELL, (cy as f32 + CHUNK as f32 * 0.5) * CELL);
    let mut have: Vec<(usize, usize)> = vec![];
    for (e, c) in &chunks {
        let d = centre(c.cx, c.cy).distance(Vec2::new(focus.x, focus.z));
        if !low || c.level != level || d > reach + size {
            commands.entity(e).despawn();
        } else {
            have.push((c.cx, c.cy));
        }
    }
    if !low {
        return;
    }
    if mat.is_none() {
        let mut m = materials.get(&view.material).cloned().unwrap_or_default();
        m.double_sided = true;
        m.cull_mode = None;
        m.perceptual_roughness = 0.9;
        m.reflectance = 0.08;
        *mat = Some(materials.add(m));
    }
    let Some(material) = mat.clone() else { return };
    // Missing chunks in reach, nearest first.
    let (x0, x1) = (((focus.x - reach) / size).floor().max(0.0) as usize, ((focus.x + reach) / size).floor().max(0.0) as usize);
    let (y0, y1) = (((focus.z - reach) / size).floor().max(0.0) as usize, ((focus.z + reach) / size).floor().max(0.0) as usize);
    let mut want: Vec<(f32, usize, usize)> = vec![];
    for gy in y0..=y1 {
        for gx in x0..=x1 {
            let (cx, cy) = (gx * CHUNK, gy * CHUNK);
            if cx >= map.nx || cy >= map.ny || have.contains(&(cx, cy)) {
                continue;
            }
            let d = centre(cx, cy).distance(Vec2::new(focus.x, focus.z));
            if d <= reach {
                want.push((d, cx, cy));
            }
        }
    }
    want.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, cx, cy) in want.into_iter().take(3) {
        let c = centre(cx, cy);
        let origin = Vec3::new(c.x, map.height(c.x, c.y), c.y);
        // Always spawn an entity (even without grass) so the chunk is not tried again every frame.
        let mesh = chunk_mesh(&map, &road, cx, cy, level, origin).map(|m| meshes.add(m));
        let mut e = commands.spawn((Transform::from_translation(origin), Visibility::Inherited, GrassChunk { cx, cy, level }, GameScene));
        if let Some(m) = mesh {
            let fade_end = if level >= 2 { 190.0 } else { 140.0 };
            e.insert((
                Mesh3d(m),
                MeshMaterial3d(material.clone()),
                NotShadowCaster,
                bevy::camera::visibility::VisibilityRange { start_margin: 0.0..0.0, end_margin: (fade_end - 40.0)..fade_end, use_aabb: false },
            ));
        }
    }
}
