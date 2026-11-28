// Ultra-effects particles (`smoke.rs`): a quad turned to face the camera in the vertex shader, a puff
// from the atlas in the fragment shader, lit by Bevy's PBR lighting (sun, shadows, ambient, distance
// fog) as a lumpy ball. The particle's age, seed and opacity come from its `MeshTag` and travel to the
// fragment shader in the vertex colour.
import bevy_pbr::render::{
    mesh_functions,
    forward_io::{Vertex, VertexOutput, FragmentOutput, decompress_vertex},
    view_transformations::position_world_to_clip,
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::view,
};

struct SmokeParams {
    // rgb: colour of the puff (linear), a: opacity.
    tint: vec4<f32>,
    // x: 0 smoke, 1 fire, 2 flash; y: glow strength; z: erosion over the life; w: how fast fire cools.
    kind: vec4<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> params: SmokeParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var atlas: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var atlas_sampler: sampler;

// Turn of the puff: random start, a slow roll over its life.
fn angle(life: f32, seed: f32) -> f32 {
    return seed * 6.2831853 + life * (seed - 0.5) * 2.4;
}

@vertex
fn vertex(vertex_in: Vertex) -> VertexOutput {
    var out: VertexOutput;
    let v = decompress_vertex(vertex_in, vertex_in.instance_index);
    let m = mesh_functions::get_world_from_local(vertex_in.instance_index);
    let centre = m[3].xyz;
    let size = length(m[0].xyz);
    let tag = mesh_functions::get_tag(vertex_in.instance_index);
    let life = f32(tag & 255u) / 255.0;
    let seed = f32((tag >> 8u) & 255u) / 255.0;
    let alpha = f32((tag >> 16u) & 255u) / 255.0;
    let a = angle(life, seed);
    let c = cos(a);
    let s = sin(a);
    let q = v.position.xy;
    let r = vec2<f32>(q.x * c - q.y * s, q.x * s + q.y * c);
    let right = view.world_from_view[0].xyz;
    let up = view.world_from_view[1].xyz;
    let back = view.world_from_view[2].xyz;
    // Pulled a little towards the camera, so a puff on the ground is not cut in half by it.
    let wp = centre + (right * r.x + up * r.y) * size + back * size * 0.35;
    out.world_position = vec4<f32>(wp, 1.0);
    out.position = position_world_to_clip(wp);
    out.world_normal = back;
    out.uv = v.uv;
    out.color = vec4<f32>(life, seed, alpha, 1.0);
@if(VERTEX_OUTPUT_INSTANCE_INDEX)
    out.instance_index = vertex_in.instance_index;
    return out;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let life = in.color.r;
    let seed = in.color.g;
    let opacity = in.color.b;
    let variant = u32(seed * 255.0 + 0.5) & 3u;
    let cell = vec2<f32>(f32(variant & 1u), f32(variant >> 1u));
    let t = textureSample(atlas, atlas_sampler, (in.uv + cell) * 0.5);

    // Thin parts dissolve first as the puff ages; fades in quickly at birth, out at the end.
    let erode = params.kind.z * life;
    let d = saturate((t.r - erode) / max(1.0 - erode, 0.05));
    let fade = smoothstep(0.0, 0.06, life) * (1.0 - smoothstep(0.75, 1.0, life));
    var alpha = smoothstep(0.0, 0.6, d) * fade * opacity * params.tint.a;

    // A lumpy ball: the sphere the quad stands for, bent by the density's slope.
    let a = angle(life, seed);
    let c = cos(a);
    let s = sin(a);
    let right = view.world_from_view[0].xyz;
    let up = view.world_from_view[1].xyz;
    let back = view.world_from_view[2].xyz;
    let ru = right * c + up * s;
    let uu = -right * s + up * c;
    let q = vec2<f32>(in.uv.x * 2.0 - 1.0, 1.0 - in.uv.y * 2.0);
    let z = sqrt(max(1.0 - dot(q, q), 0.0));
    let g = (t.gb - vec2<f32>(0.5)) * 2.0;
    let n = normalize(ru * (q.x * 0.8 - g.x * 1.4) + uu * (q.y * 0.8 - g.y * 1.4) + back * (z * 0.8 + 0.35));
    pbr_input.N = n;
    pbr_input.world_normal = n;

    // Denser middles a touch darker (light scattered away), specks of detail.
    var col = params.tint.rgb * (0.8 + 0.4 * t.a) * (1.15 - 0.3 * d);
    var glow = vec3<f32>(0.0);
    let kind = params.kind.x;
    if kind > 1.5 {
        // Flash: pure light, added to the scene (premultiplied).
        let h = d * (1.0 - life);
        glow = mix(vec3<f32>(1.0, 0.55, 0.18), vec3<f32>(1.0, 0.93, 0.75), h) * params.kind.y * h;
        alpha = alpha * h;
    } else if kind > 0.5 {
        // Fire: hottest in the dense core and young, cooling to soot.
        let heat = saturate(d * 1.9 + 0.25 - life * params.kind.w * 1.4 + 0.2 * (t.a - 0.5));
        let hot = mix(vec3<f32>(0.9, 0.16, 0.02), vec3<f32>(1.0, 0.5, 0.1), smoothstep(0.15, 0.55, heat));
        let white = mix(hot, vec3<f32>(1.0, 0.88, 0.6), smoothstep(0.7, 1.0, heat));
        glow = white * params.kind.y * heat * heat;
        col = mix(col, vec3<f32>(0.0), smoothstep(0.0, 0.4, heat));
    }
    pbr_input.material.base_color = vec4<f32>(col, alpha);
    // Alpha = exposure weight: 0, like the standard material's default, so the glow is not dimmed by exposure.
    pbr_input.material.emissive = vec4<f32>(glow, 0.0);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    // Premultiplies (and zeroes alpha) for the flash's additive mode.
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
