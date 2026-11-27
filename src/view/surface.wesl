// Ground texturing: SupCom-style strata. Layer 0 covers everything; layers 1.. are drawn over the
// ones below by their masks, with height blending (stones show through sand before the sand thins
// out). Roads come next from a distance-like field, then darkening from explosions (scorch).
// Lighting is Bevy's standard PBR; only the base colour and the normal (a little relief from the
// textures' height channel) change.
import bevy_pbr::render::{
    forward_io::{VertexOutput, FragmentOutput},
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
};

struct SurfaceParams {
    // Size of the map in world units, x and z (masks, roads and scorch cover it once).
    world: vec2<f32>,
    layers: u32,
    roads: u32,
    // Relief from the height channel, in world units per full height step.
    bump: f32,
    // Tile size of every texture slot (layers, then road kinds), four per vector.
    scales: array<vec4<f32>, 5>,
    // Edge sharpness per road kind (higher = harder edge).
    road_soft: array<vec4<f32>, 2>,
    // Road slot of the sidewalks (99 = none): kerbs where a paved road meets them.
    sidewalk: u32,
    // Road slot of the kind lane markings are painted on (99 = none).
    marked: u32,
    // LR_SURFACE_DEBUG: 0 normal; 1 base layer, 2 scorch, 3 masks 1-3, 4 roads 1-3, 5 colour, 6 vertex tint, 7 roads 5-7 (unlit).
    debug: u32,
    // 1: cliffs, broad colour variation, wet shoreline.
    quality: u32,
    // Sea level.
    water: f32,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> params: SurfaceParams;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var albedo: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var albedo_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(103) var masks: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(104) var masks_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(105) var roads: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(106) var roads_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(107) var scorch: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(108) var scorch_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(109) var roads2: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(110) var roads2_sampler: sampler;
// Lane markings (`MapData::centre_raster`): r distance along the road mod 16, g signed offset from
// the centre line (-4..4 -> 0..1), b drawn here, a half the road's width (low 7 bits, 0..4) + 128 for
// a solid centre line.
@group(constants::MATERIAL_BIND_GROUP) @binding(111) var centre: texture_2d<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(112) var centre_sampler: sampler;

fn tile_scale(i: u32) -> f32 {
    return params.scales[i / 4u][i % 4u];
}

// Texture slot `i` at world position `p` (rgb, height in alpha). A second, larger and turned
// sample of the same texture modulates the first, which hides the tiling.
fn layer_sample(i: u32, p: vec2<f32>, dx: vec2<f32>, dy: vec2<f32>) -> vec4<f32> {
    let s = 1.0 / max(tile_scale(i), 0.5);
    let a = textureSampleGrad(albedo, albedo_sampler, p * s, i, dx * s, dy * s);
    let r = mat2x2<f32>(0.8, -0.6, 0.6, 0.8);
    let s2 = s * 0.27;
    let b = textureSampleGrad(albedo, albedo_sampler, r * (p * s2) + vec2<f32>(0.37, 0.71), i, r * (dx * s2), r * (dy * s2));
    let k = 0.7 + 0.6 * b.a;
    return vec4<f32>(mix(a.rgb, b.rgb, 0.35) * mix(1.0, k, 0.5), mix(a.a, b.a, 0.3));
}

// How much a layer of mask value `m` and texture height `h` covers ground of height `hc`:
// exactly 0 at m = 0 and 1 at m = 1, in between the higher texture wins. `wobble` (0..1, broad
// noise) moves the border around, so it does not follow the mask's cells.
fn cover(m: f32, h: f32, hc: f32, wobble: f32) -> f32 {
    let mw = m + (wobble - 0.5) * 1.6 * m * (1.0 - m);
    return saturate((mw - 0.5) * 2.6 + (h - hc) * 0.7 + 0.5);
}

// Mask coordinates with a smooth (quintic) step between texel centres: plain bilinear filtering
// of one value per cell shows the cells as diamonds once the blend is sharpened.
fn smooth_uv(uv: vec2<f32>, size: vec2<f32>) -> vec2<f32> {
    let t = uv * size - 0.5;
    let i = floor(t);
    var f = t - i;
    f = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    return (i + f + 0.5) / size;
}

// 1 where `a < v < b`, antialiased over the field's screen-space width `fw`.
fn band(v: f32, fw: f32, a: f32, b: f32) -> f32 {
    return saturate((v - a) / fw + 0.5) * saturate((b - v) / fw + 0.5);
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let p = in.world_position.xz;
    let dx = dpdx(p);
    let dy = dpdy(p);
    let uv = p / params.world;

    let base = layer_sample(0u, p, dx, dy);
    var col = base.rgb;
    var hc = base.a;
    let muv = smooth_uv(uv, vec2<f32>(textureDimensions(masks).xy));
    let m0 = textureSampleLevel(masks, masks_sampler, muv, 0, 0.0);
    let m1 = textureSampleLevel(masks, masks_sampler, muv, 1, 0.0);
    // Broad noise for the layer borders: the base texture's height channel, stretched far.
    let ws = 0.09 / max(tile_scale(0u), 0.5);
    let wob = textureSampleGrad(albedo, albedo_sampler, p * ws + vec2<f32>(0.21, 0.63), 0u, dx * ws, dy * ws).a;
    for (var i = 1u; i < params.layers; i++) {
        let j = i - 1u;
        var m = m1[j % 4u];
        if j < 4u {
            m = m0[j];
        }
        if m > 0.002 {
            let l = layer_sample(i, p, dx, dy);
            let w = cover(m, l.a, hc, fract(wob + f32(i) * 0.37));
            col = mix(col, l.rgb, w);
            hc = mix(hc, l.a, w);
        }
    }

    let rv = textureSampleLevel(roads, roads_sampler, uv, 0.0);
    var rv2 = vec4<f32>(0.0);
    if params.roads > 4u {
        rv2 = textureSampleLevel(roads2, roads2_sampler, uv, 0.0);
    }
    var walk = 0.0;
    if params.sidewalk < 8u {
        walk = select(rv2[params.sidewalk % 4u], rv[params.sidewalk % 4u], params.sidewalk < 4u);
    }
    // Screen-space width of the fields (derivatives only in uniform control flow).
    let fw1 = max(fwidth(rv), vec4<f32>(1e-4));
    let fw2 = max(fwidth(rv2), vec4<f32>(1e-4));
    // Centre line (derivatives taken here, in uniform control flow).
    let cl = textureSampleLevel(centre, centre_sampler, uv, 0.0);
    let arc = cl.r * 16.0;
    let off = cl.g * 8.0 - 4.0;
    // The width and solid flag are not filtered (texels of different roads meet).
    let a8 = textureLoad(centre, vec2<i32>(uv * vec2<f32>(textureDimensions(centre))), 0).a * 255.0;
    let solid = select(0.0, 1.0, a8 > 127.5);
    let half_w = (a8 - solid * 128.0) / 127.0 * 4.0;
    let fw_arc = fwidth(arc);
    let fw_off = max(fwidth(off), 1e-4);
    var centre_line = 0.0;
    var edge_line = 0.0;
    var on_marked = 0.0;
    // A jump in either field (seams between roads) shows as a large screen-space width: no line there.
    if cl.b > 0.5 && fw_arc < 3.0 && fw_off < 0.6 {
        // 0.14 wide; 2 units of paint, 2 of gap (the wrap at 0 falls in a gap and is hidden by fw_arc).
        let half = 0.07;
        let across = saturate((half - abs(off)) / fw_off + 0.5) * min(1.0, 2.0 * half / fw_off);
        let ph = fract(arc * 0.25);
        let dash = max(solid, saturate((ph - 0.25) / max(fw_arc * 0.25, 1e-3) + 0.5) * saturate((0.75 - ph) / max(fw_arc * 0.25, 1e-3) + 0.5));
        centre_line = across * dash * saturate((cl.b - 0.5) * 4.0);
        // Edge lines 0.45 inside each edge, 0.12 wide.
        let edge = abs(abs(off) - (half_w - 0.45));
        edge_line = saturate((0.06 - edge) / fw_off + 0.5) * min(1.0, 0.12 / fw_off) * saturate((cl.b - 0.5) * 4.0);
    }
    for (var k = 0u; k < params.roads; k++) {
        let v = select(rv2[k % 4u], rv[k % 4u], k < 4u);
        if v > 0.002 {
            let l = layer_sample(params.layers + k, p, dx, dy);
            let soft = params.road_soft[k / 4u][k % 4u];
            // 0 beyond the edge whatever the heights (road_soft >= 2), 1 well inside.
            let w = saturate((v - 0.5) * soft + (l.a - hc) * 0.5 + 0.5);
            col = mix(col, l.rgb, w);
            hc = mix(hc, l.a * 0.6 + 0.2, w);
            if k == params.marked {
                on_marked = w;
            }
            // The field is linear within a unit of the edge (ROAD_RAMP 2: 0.1 of it = 0.2 units), so
            // thin bands along the edge can be cut from it: a kerb where a sidewalk runs beside the road.
            let fw = select(fw2[k % 4u], fw1[k % 4u], k < 4u);
            if k != params.sidewalk && soft > 3.0 && v < 0.82 {
                let side = smoothstep(0.55, 0.8, walk);
                if side > 0.0 {
                    // Kerb stone, a gutter just inside it.
                    let kerb = band(v, fw, 0.5, 0.56) * side;
                    let gutter = band(v, fw, 0.56, 0.6) * side;
                    col = mix(col, vec3<f32>(0.42, 0.41, 0.39) * (0.85 + 0.3 * l.a), kerb);
                    col = col * (1.0 - 0.35 * gutter);
                    hc = mix(hc, 1.0, kerb);
                    hc = mix(hc, 0.1, gutter);
                }
            }
        }
    }

    // Edge lines only where no sidewalk runs beside the road (towns have kerbs instead).
    let paint = max(centre_line, edge_line * (1.0 - smoothstep(0.55, 0.8, walk)));
    col = mix(col, vec3<f32>(0.62, 0.62, 0.58), paint * on_marked * 0.9);

    if params.quality > 0u {
        // Broad variation of the colour, so large fields of one texture are not flat.
        let ms = 0.011;
        let big = textureSampleGrad(albedo, albedo_sampler, p * ms + vec2<f32>(0.53, 0.11), 0u, dx * ms, dy * ms).a;
        col = col * (0.84 + 0.32 * big);
        // Steep slopes show bare rock whatever is painted there.
        let slope = 1.0 - saturate(pbr_input.N.y);
        let cw = smoothstep(0.28, 0.58, slope);
        let rock = mix(vec3<f32>(0.40, 0.38, 0.35), col * 0.7, 0.3) * (0.62 + 0.75 * hc);
        col = mix(col, rock, cw * 0.85);
        // Ground just above the sea is wet: darker and shinier.
        let wet = 1.0 - smoothstep(params.water, params.water + 0.9, in.world_position.y);
        col = col * mix(1.0, 0.62, wet);
        pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, 0.4, wet);
    }

    // Scorched ground: a crater-like decal, not a black disc. Red is char: darkest in the middle of a
    // blast, fading out smoothly (a gradient, the ground texture still showing through); green is the
    // earth the blast threw out and singed, a ring round it lighter than the char but still darker than
    // the untouched ground. Noise from the ground's own height
    // texture breaks both edges into ragged flecks.
    let sc = textureSampleLevel(scorch, scorch_sampler, uv, 0.0).rg;
    let s = sc.r;
    if s > 0.004 || sc.g > 0.004 {
        let n1 = textureSampleGrad(albedo, albedo_sampler, p * 0.22 + vec2<f32>(0.37, 0.71), 0u, dx * 0.22, dy * 0.22).a;
        let n2 = textureSampleGrad(albedo, albedo_sampler, p * 0.9 + vec2<f32>(0.13, 0.29), 0u, dx * 0.9, dy * 0.9).a;
        let t = saturate(s * 1.6 + (n1 - 0.5) * 0.45 * (0.3 + s) + (n2 - 0.5) * 0.22 * s);
        let core = t * t * (3.0 - 2.0 * t);
        let rg = saturate(sc.g * 1.2 + (n2 - 0.5) * 0.5 * sc.g + (n1 - 0.5) * 0.25);
        let ring = smoothstep(0.08, 0.75, rg) * (1.0 - core);
        // Churned, singed earth: darker and browner than the ground, but well short of the char.
        let dust = col * vec3<f32>(0.55, 0.5, 0.45) * (0.85 + 0.3 * n2) + vec3<f32>(0.012, 0.009, 0.005);
        let charred = col * vec3<f32>(0.10, 0.09, 0.08) + vec3<f32>(0.012, 0.010, 0.008);
        col = mix(col, dust, ring * 0.85);
        col = mix(col, charred, core * 0.94);
        pbr_input.material.perceptual_roughness = mix(pbr_input.material.perceptual_roughness, 1.0, max(core, ring));
    }

    // Relief: tilt the normal along the slope of the blended height (surface gradient).
    let n = pbr_input.N;
    let wp = in.world_position.xyz;
    let sx = dpdx(wp);
    let sy = dpdy(wp);
    let r1 = cross(sy, n);
    let r2 = cross(n, sx);
    let det = dot(sx, r1);
    let grad = sign(det) * (dpdx(hc) * r1 + dpdy(hc) * r2);
    let v = abs(det) * n - params.bump * grad;
    if dot(v, v) > 1e-20 {
        pbr_input.N = normalize(v);
    }

    let tint = pbr_input.material.base_color.rgb;
    pbr_input.material.base_color = vec4<f32>(tint * col, 1.0);
    var out: FragmentOutput;
    if params.debug != 0u {
        var d = col;
        switch params.debug {
            case 1u: { d = base.rgb; }
            case 2u: { d = vec3<f32>(s); }
            case 3u: { d = m0.rgb; }
            case 4u: { d = rv.rgb; }
            case 7u: { d = rv2.rgb; }
            case 6u: { d = tint; }
            default: {}
        }
        out.color = vec4<f32>(d, 1.0);
        return out;
    }
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
