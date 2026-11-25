//! Automatic low-detail meshes for every model (units and buildings alike): vertex clustering.
//!
//! The part's bounding box is cut into a coarse grid and all vertices of a cell are merged into one
//! (positions, normals and UVs averaged), triangles that collapse are dropped. Vertices are only merged
//! when they also share a UV cell and the face direction they point to (the nearest of the six axes), so
//! texture seams stay seams and the hard edges of boxy buildings stay hard: the simplified mesh uses the
//! same texture and material as the full one, which is why a part can swap between the two by distance
//! (`models::LodMesh`, `models::model_lod_swap`). Purely visual.
use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology, VertexAttributeValues};
use bevy::prelude::*;
use std::collections::HashMap;

/// Meshes with fewer triangles than this are not simplified.
const MIN_TRIS: usize = 60;
/// Cells along the longest side of a part.
const CELLS: f32 = 12.0;
/// UV cells per unit of texture coordinate.
const UV_CELLS: f32 = 24.0;
/// The simplified mesh is only used when it has at most this share of the triangles.
const WORTH: f32 = 0.7;

/// The strongest axis of a normal and its sign, 0..6.
fn facing(n: [f32; 3]) -> i32 {
    let a = [n[0].abs(), n[1].abs(), n[2].abs()];
    let k = if a[0] >= a[1] && a[0] >= a[2] { 0 } else if a[1] >= a[2] { 1 } else { 2 };
    k as i32 * 2 + (n[k] < 0.0) as i32
}

/// A coarser copy of `mesh` (triangle list with positions, normals and UVs), or `None` when it is small
/// already or would not get much smaller.
pub fn simplify(mesh: &Mesh) -> Option<Mesh> {
    if mesh.primitive_topology() != PrimitiveTopology::TriangleList {
        return None;
    }
    let Some(VertexAttributeValues::Float32x3(pos)) = mesh.attribute(Mesh::ATTRIBUTE_POSITION) else { return None };
    let nrm: Vec<[f32; 3]> = match mesh.attribute(Mesh::ATTRIBUTE_NORMAL) {
        Some(VertexAttributeValues::Float32x3(n)) => n.clone(),
        _ => vec![[0.0, 1.0, 0.0]; pos.len()],
    };
    let uv: Vec<[f32; 2]> = match mesh.attribute(Mesh::ATTRIBUTE_UV_0) {
        Some(VertexAttributeValues::Float32x2(u)) => u.clone(),
        _ => vec![[0.0, 0.0]; pos.len()],
    };
    let idx: Vec<u32> = match mesh.indices()? {
        Indices::U32(i) => i.clone(),
        Indices::U16(i) => i.iter().map(|&x| x as u32).collect(),
    };
    if idx.len() / 3 < MIN_TRIS || nrm.len() != pos.len() || uv.len() != pos.len() {
        return None;
    }
    let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
    for p in pos {
        lo = lo.min(Vec3::from_array(*p));
        hi = hi.max(Vec3::from_array(*p));
    }
    let size = (hi - lo).max_element();
    if !size.is_finite() || size <= 1e-6 {
        return None;
    }
    let cell = size / CELLS;
    let mut ids: HashMap<[i32; 6], u32> = HashMap::new();
    // Per vertex: the merged vertex it belongs to; per merged vertex: sums and the count.
    let mut of = Vec::with_capacity(pos.len());
    let (mut sp, mut sn, mut su): (Vec<Vec3>, Vec<Vec3>, Vec<Vec2>) = (vec![], vec![], vec![]);
    let mut count: Vec<f32> = vec![];
    for v in 0..pos.len() {
        let p = Vec3::from_array(pos[v]);
        let key = [
            ((p.x - lo.x) / cell).floor() as i32,
            ((p.y - lo.y) / cell).floor() as i32,
            ((p.z - lo.z) / cell).floor() as i32,
            (uv[v][0] * UV_CELLS).floor() as i32,
            (uv[v][1] * UV_CELLS).floor() as i32,
            facing(nrm[v]),
        ];
        let id = *ids.entry(key).or_insert_with(|| {
            sp.push(Vec3::ZERO);
            sn.push(Vec3::ZERO);
            su.push(Vec2::ZERO);
            count.push(0.0);
            (sp.len() - 1) as u32
        });
        let k = id as usize;
        sp[k] += p;
        sn[k] += Vec3::from_array(nrm[v]);
        su[k] += Vec2::from_array(uv[v]);
        count[k] += 1.0;
        of.push(id);
    }
    let mut out: Vec<u32> = Vec::with_capacity(idx.len() / 2);
    for t in idx.chunks_exact(3) {
        let (a, b, c) = (of[t[0] as usize], of[t[1] as usize], of[t[2] as usize]);
        if a != b && b != c && a != c {
            out.push(a);
            out.push(b);
            out.push(c);
        }
    }
    if out.is_empty() || out.len() as f32 > idx.len() as f32 * WORTH {
        return None;
    }
    let positions: Vec<[f32; 3]> = (0..sp.len()).map(|k| (sp[k] / count[k]).to_array()).collect();
    let normals: Vec<[f32; 3]> = (0..sn.len()).map(|k| sn[k].normalize_or_zero().to_array()).collect();
    let uvs: Vec<[f32; 2]> = (0..su.len()).map(|k| (su[k] / count[k]).to_array()).collect();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    m.insert_indices(Indices::U32(out));
    Some(m)
}
