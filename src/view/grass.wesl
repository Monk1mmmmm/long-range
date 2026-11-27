// Grass blades (`grass.rs`): the vertex shader of the grass material; lighting and the fog of war
// are the standard material's (vertex colours times the fog texture, which UV 0 maps).
//
// Every vertex carries, in UV 1, its height above the blade's root and the blade's rank (0..1,
// random per blade). The shader:
// - thins the grass out with the distance from the point the camera looks at: a blade shows only
//   while its rank is below the density there, and grows out of / sinks into the ground over a
//   short band of ranks, so nothing pops (the mesh of a distant chunk holds only the low ranks);
// - flattens grass where the ground is scorched or churned up by blasts (the terrain's scorch map),
//   withering what is left at the edge;
// - sways the blades in the wind (gusts travelling across the map), more towards the tips.
import bevy_pbr::render::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
};

struct GrassParams {
    // xz: the point the camera looks at; z: fade with camera height (1 shown, 0 none); w: time (s).
    focus: vec4<f32>,
    // x: full density out to here; y: `far` density (z) from here; w: no grass beyond.
    dist: vec4<f32>,
    // xy: wind direction, z: strength, w: unused.
    wind: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> params: GrassParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var scorch: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var scorch_sampler: sampler;

fn density(d: f32) -> f32 {
    let far = params.dist.z;
    if d < params.dist.x {
        return 1.0;
    }
    if d < params.dist.y {
        return mix(1.0, far, smoothstep(params.dist.x, params.dist.y, d));
    }
    return far * (1.0 - smoothstep(params.dist.y, params.dist.w, d));
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let v = decompress_vertex(vertex_in, vertex_in.instance_index);
    let world_from_local = mesh_functions::get_world_from_local(vertex_in.instance_index);
    var wp = mesh_functions::mesh_position_local_to_world(world_from_local, vec4<f32>(v.position, 1.0)).xyz;
    let h = v.uv_b.x;
    let rank = v.uv_b.y;

    // How much of the blade shows: distance thinning, camera height, scorch.
    let d = distance(wp.xz, params.focus.xy);
    var grow = saturate((density(d) * params.focus.z - rank) / 0.06);
    let sc = textureSampleLevel(scorch, scorch_sampler, v.uv, 0.0).rg;
    let burnt = max(sc.r, sc.g * 0.85);
    grow = grow * (1.0 - smoothstep(0.1, 0.32, burnt));
    // Shrink towards the root and sink a little below the ground when gone, so a hidden blade is
    // never a sliver lying on the terrain.
    wp.y = wp.y - (h + 0.25) * (1.0 - grow);

    // Wind: a slow sway plus gusts rolling across the map along the wind.
    let wdir = params.wind.xy;
    let t = params.focus.w;
    let along = dot(wp.xz, wdir);
    let gust = 0.5 + 0.5 * sin(along * 0.08 - t * 1.3) * sin(along * 0.021 - t * 0.47 + wp.x * 0.013);
    let sway = sin(t * 2.1 + along * 0.6 + wp.z * 0.37) * 0.35 + gust * 1.1;
    let bend = h * h * params.wind.z * sway * grow;
    wp = vec3<f32>(wp.x + wdir.x * bend, wp.y - abs(bend) * 0.25, wp.z + wdir.y * bend);

    out.world_position = vec4<f32>(wp, 1.0);
    out.position = position_world_to_clip(wp);
    // Lit like the ground under it (the normal is the terrain's), so blades do not glitter.
    out.world_normal = mesh_functions::mesh_normal_local_to_world(v.normal, vertex_in.instance_index);
    out.uv = v.uv;
    out.uv_b = v.uv_b;
    // Withered (dry, brown) at the edge of scorched ground.
    let dry = smoothstep(0.02, 0.2, burnt);
    let c = v.color.rgb;
    let withered = vec3<f32>(0.20, 0.15, 0.07) * (0.6 + 0.6 * saturate(h * 3.0));
    out.color = vec4<f32>(mix(c, withered, dry * 0.85), 1.0);
    // The fragment shader reads the mesh flags (shadow receiver...) by this index: left at 0 it
    // took another mesh's, and the grass got no shadows.
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
    return out;
}
