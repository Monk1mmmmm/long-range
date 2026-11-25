// The surface of the sea and the lakes (`water.rs`). Bevy's standard PBR lighting, plus:
// - a shore field (depth below the water and distance to the waterline, made from the heightmap) that
//   fades the water out smoothly at the shore, lightens the shallows and draws lapping foam - every level;
// - drifting wave normals (high and ultra);
// - a planar reflection: the scene rendered by a camera mirrored at the water level (ultra).
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    mesh_view_bindings::view,
};

struct WaterParams {
    // Shore field coordinates: uv = world xz * scale + offset.
    shore_scale: vec2<f32>,
    shore_offset: vec2<f32>,
    // The area the field covers (world xz); beyond it the water counts as open sea.
    lo: vec2<f32>,
    hi: vec2<f32>,
    time: f32,
    // Settings::water_detail: 0 low, 1 medium, 2 high (waves), 3 ultra (reflection).
    level: u32,
    // Height of the plane the reflection camera mirrors at.
    mirror: f32,
    // 1 while the reflection texture holds the mirrored scene.
    reflect: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> params: WaterParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var shore: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var shore_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var waves: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var waves_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var foam_tex: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var foam_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var reflection: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var reflection_sampler: sampler;

// Must match `water.rs`: how the shore field encodes depth and distance.
const DEPTH_LO: f32 = -2.0;
const DEPTH_SPAN: f32 = 8.0;
const DIST_LO: f32 = -4.0;
const DIST_SPAN: f32 = 14.0;

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let p = in.world_position.xz;
    let t = params.time;

    // Depth below this water and signed distance to the waterline (> 0 in the water), both
    // interpolated from a fine grid, so the shoreline is a smooth curve rather than cell edges.
    let s = textureSampleLevel(shore, shore_sampler, p * params.shore_scale + params.shore_offset, 0.0);
    let o = max(max(params.lo - p, p - params.hi), vec2<f32>(0.0));
    let open = saturate(length(o) / 24.0);
    let depth = mix(s.r * DEPTH_SPAN + DEPTH_LO, DEPTH_SPAN + DEPTH_LO, open);
    let dist = mix(s.g * DIST_SPAN + DIST_LO, DIST_SPAN + DIST_LO, open);

    // Waves: two layers of the ripple normal map drifting across each other, calmer in the shallows.
    var n = vec3<f32>(0.0, 1.0, 0.0);
    if params.level >= 2u {
        let r = mat2x2<f32>(0.8, -0.6, 0.6, 0.8);
        let a = textureSample(waves, waves_sampler, p / 22.0 + vec2<f32>(t * 0.011, t * 0.007)).xyz * 2.0 - 1.0;
        let b = textureSample(waves, waves_sampler, r * (p / 37.0) + vec2<f32>(-t * 0.006, t * 0.009)).xyz * 2.0 - 1.0;
        let k = mix(0.3, 1.0, smoothstep(0.0, 3.0, depth));
        n = normalize(vec3<f32>((a.x + b.x) * 0.5 * k, 1.0, (a.y + b.y) * 0.5 * k));
    }
    pbr_input.N = n;

    // Shallows are lighter and clearer; the water thins out to nothing at the waterline.
    let base = pbr_input.material.base_color;
    let deep = smoothstep(0.0, 4.0, depth);
    var col = mix(vec3<f32>(0.09, 0.27, 0.28), base.rgb, deep);
    let edge = smoothstep(0.0, 0.9, dist);
    // Deep water is nearly opaque (the sea floor beyond the map is no more than a dark plane).
    var alpha = mix(mix(0.38, base.a, smoothstep(0.0, 2.5, depth)), 0.95, smoothstep(2.5, 6.0, depth)) * edge;

    // Foam: a band along the shore whose width laps in and out, broken up by a drifting noise
    // texture, and a thinner line running in towards the shore behind it. All functions of the
    // smooth distance field, so the band follows the coastline without steps.
    let f1 = textureSampleLevel(foam_tex, foam_sampler, p * 0.09 + vec2<f32>(t * 0.013, -t * 0.008), 0.0).a;
    let f2 = textureSampleLevel(foam_tex, foam_sampler, vec2<f32>(p.y, -p.x) * 0.21 + vec2<f32>(-t * 0.02, t * 0.011), 0.0).a;
    let noise = f1 * 0.6 + f2 * 0.4;
    let lap = sin(t * 0.8 + f1 * 4.0 + (p.x + p.y) * 0.04);
    let width = 1.1 + 0.45 * lap + 0.7 * f2;
    let band = 1.0 - smoothstep(0.0, width, dist);
    let phase = fract(t * 0.08 + f1 * 0.3);
    let line_at = mix(3.4, 0.5, phase);
    let line = (1.0 - smoothstep(0.0, 0.45, abs(dist - line_at))) * phase * (1.0 - phase) * 2.4;
    var foam = saturate((band + line) * smoothstep(0.2, 0.65, noise + band * 0.35));
    foam = foam * (1.0 - open) * smoothstep(0.0, 0.3, dist);
    col = mix(col, vec3<f32>(0.88, 0.93, 0.96), foam);
    alpha = max(alpha, foam * 0.92);

    pbr_input.material.base_color = vec4<f32>(col, 1.0);
    pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, 0.85, foam);
    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);

    // The mirrored scene (already lit, fogged and tonemapped by its own camera), distorted by the
    // waves; stronger at grazing angles. Water at another level than the mirror (a lake while the
    // camera looks at the sea, or the other way round) reflects only the sky's light.
    if params.reflect > 0.5 {
        let suv = (in.position.xy - view.viewport.xy) / view.viewport.zw;
        let ruv = clamp(suv + n.xz * 0.035, vec2<f32>(0.001), vec2<f32>(0.999));
        let refl = textureSampleLevel(reflection, reflection_sampler, ruv, 0.0).rgb;
        let ndv = saturate(dot(n, pbr_input.V));
        let fres = 0.02 + 0.98 * pow(1.0 - ndv, 5.0);
        let same = 1.0 - smoothstep(0.3, 1.5, abs(in.world_position.y - params.mirror));
        let k = saturate(0.32 + 0.68 * fres) * (1.0 - foam) * same * edge;
        out.color = vec4<f32>(mix(out.color.rgb, refl, k), out.color.a);
        alpha = max(alpha, k * 0.9);
    }
    out.color.a = alpha;
    return out;
}
