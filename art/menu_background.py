"""Renders the main-menu background (steppe at dusk, tanks on a ridge) with Blender Cycles to assets/ui/menu_bg.png.
.venv/bin/python art/menu_background.py [--size 1920x1080] [--samples 128] [--out file.png]"""
import math, os, sys
import bpy
import numpy as np
from mathutils import Vector

HERE = os.path.dirname(os.path.abspath(__file__))
MODELS = os.path.join(HERE, "..", "assets", "models")
OUT = os.path.join(HERE, "..", "assets", "ui", "menu_bg.png")
TEAM = (0.22, 0.34, 0.52)
SKY_H = (0.20, 0.17, 0.15)   # horizon: restrained warm band
SKY_M = (0.075, 0.09, 0.115)
SKY_T = (0.03, 0.04, 0.055)
HAZE = (0.115, 0.115, 0.12)
CAM_LOC = (-1.0, -14.0, 3.0)


def noise_grid(n, cells, rng):
    g = rng.random((cells + 2, cells + 2))
    t = np.linspace(0, cells, n, endpoint=False)
    i = t.astype(int); f = t - i; f = f * f * (3 - 2 * f)
    a = g[np.ix_(i, i)]; b = g[np.ix_(i, i + 1)]; c = g[np.ix_(i + 1, i)]; d = g[np.ix_(i + 1, i + 1)]
    fx = f[None, :]; fy = f[:, None]
    return (a * (1 - fx) + b * fx) * (1 - fy) + (c * (1 - fx) + d * fx) * fy


W, DEPTH, N = 700.0, 700.0, 700   # terrain: x in +-W/2, y in -60..DEPTH-60
Y0 = -60.0
rng = np.random.default_rng(7)
_oct = [(noise_grid(N, 5, rng), 30.0), (noise_grid(N, 12, rng), 9.0), (noise_grid(N, 30, rng), 2.6),
        (noise_grid(N, 90, rng), 1.3), (noise_grid(N, 230, rng), 0.6)]


def height_fn():
    xs = np.linspace(-W / 2, W / 2, N); ys = np.linspace(Y0, Y0 + DEPTH, N)
    X, Y = np.meshgrid(xs, ys)  # [row=y, col=x]
    h = sum((g - 0.5) * a for g, a in _oct)
    # far ridges rising with distance, calm low land in the middle distance
    h += 14 * np.clip((Y - 180) / 300, 0, 1) ** 1.3 + 6 * np.exp(-((Y - 330) / 70) ** 2)
    # foreground ridge on the right where the tanks stand
    h += 3.2 * np.exp(-(((X - 6) / 16) ** 2 + ((Y - 12) / 18) ** 2))
    # calm the centre of the picture
    h *= 1 - 0.35 * np.exp(-((X + 5) / 40) ** 2) * np.exp(-((Y - 60) / 90) ** 2)
    return xs, ys, h


def sample(xs, ys, h, x, y):
    i = np.searchsorted(xs, x) - 1; j = np.searchsorted(ys, y) - 1
    return float(h[j, i])


def haze(nt, bs, out):
    """Far surfaces fade to the horizon colour with camera distance."""
    geo = nt.nodes.new("ShaderNodeNewGeometry")
    vd = nt.nodes.new("ShaderNodeVectorMath"); vd.operation = "DISTANCE"; vd.inputs[1].default_value = CAM_LOC
    nt.links.new(geo.outputs["Position"], vd.inputs[0])
    mr = nt.nodes.new("ShaderNodeMapRange"); mr.inputs[1].default_value = 40; mr.inputs[2].default_value = 520
    mr.inputs[3].default_value = 0.0; mr.inputs[4].default_value = 0.93
    mr.interpolation_type = "SMOOTHSTEP"; mr.clamp = True
    nt.links.new(vd.outputs["Value"], mr.inputs[0])
    em = nt.nodes.new("ShaderNodeEmission"); em.inputs["Color"].default_value = (*HAZE, 1)
    mixs = nt.nodes.new("ShaderNodeMixShader")
    nt.links.new(mr.outputs[0], mixs.inputs[0]); nt.links.new(bs.outputs[0], mixs.inputs[1]); nt.links.new(em.outputs[0], mixs.inputs[2])
    nt.links.new(mixs.outputs[0], out.inputs[0])


def scatter(xs, ys, h):
    """Low rocks near the ridge (tree clumps were tried: they read as boulders at that distance)."""
    r = np.random.default_rng(11)
    m = bpy.data.materials.new("rock"); m.use_nodes = True; nt = m.node_tree; nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial"); bs = nt.nodes.new("ShaderNodeBsdfPrincipled")
    bs.inputs["Base Color"].default_value = (0.09, 0.085, 0.08, 1); bs.inputs["Roughness"].default_value = 0.9
    haze(nt, bs, out)
    mt = bpy.data.materials.new("trees"); mt.use_nodes = True; nt = mt.node_tree; nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial"); bs = nt.nodes.new("ShaderNodeBsdfPrincipled")
    bs.inputs["Base Color"].default_value = (0.02, 0.03, 0.015, 1); bs.inputs["Roughness"].default_value = 1.0
    haze(nt, bs, out)
    def blob(loc, sc, mat, seed, verts=4):
        bpy.ops.mesh.primitive_ico_sphere_add(subdivisions=2, location=loc)
        o = bpy.context.object; o.scale = sc; o.rotation_euler = (0, 0, seed)
        for v in o.data.vertices:
            v.co *= 1 + 0.25 * math.sin(v.co.x * 7 + seed) * math.cos(v.co.y * 6 + seed * 2)
        o.data.materials.append(mat); bpy.ops.object.shade_smooth()
    for _ in range(26):  # rocks
        x = r.uniform(-30, 40); y = r.uniform(-8, 45)
        if abs(x - 4) < 3 and abs(y - 8) < 14: continue
        z = sample(xs, ys, h, x, y); s = r.uniform(0.25, 0.9)
        blob((x, y, z + s * 0.1), (s * 1.3, s, s * 0.6), m, r.uniform(0, 6))


def tex_nodes(nt, img, name):
    n = nt.nodes.new("ShaderNodeTexImage"); n.image = img; return n


def build_terrain(xs, ys, h):
    X, Y = np.meshgrid(xs, ys)
    verts = np.stack([X, Y, h], -1).reshape(-1, 3)
    idx = np.arange(N * N).reshape(N, N)
    faces = np.stack([idx[:-1, :-1], idx[:-1, 1:], idx[1:, 1:], idx[1:, :-1]], -1).reshape(-1, 4)
    me = bpy.data.meshes.new("terrain")
    me.from_pydata(verts.tolist(), [], faces.tolist())
    me.update()
    ob = bpy.data.objects.new("terrain", me); bpy.context.scene.collection.objects.link(ob)
    me.shade_smooth()
    m = bpy.data.materials.new("ground"); m.use_nodes = True
    nt = m.node_tree; nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial"); bs = nt.nodes.new("ShaderNodeBsdfPrincipled")
    tc = nt.nodes.new("ShaderNodeTexCoord")
    def noise(scale, detail=6, rough=0.6, w=0.0):
        n = nt.nodes.new("ShaderNodeTexNoise"); n.inputs["Scale"].default_value = scale
        n.inputs["Detail"].default_value = detail; n.inputs["Roughness"].default_value = rough
        nt.links.new(tc.outputs["Object"], n.inputs["Vector"]); return n
    def ramp(src, stops):
        r = nt.nodes.new("ShaderNodeValToRGB"); e = r.color_ramp.elements
        for k, (pos, col) in enumerate(stops):
            if k >= len(e): e.new(pos)
            e[k].position = pos; e[k].color = (*col, 1)
        nt.links.new(src.outputs["Fac"], r.inputs[0]); return r
    def mixc(fac_out, a, b):
        x = nt.nodes.new("ShaderNodeMix"); x.data_type = "RGBA"
        nt.links.new(fac_out, x.inputs[0]); nt.links.new(a, x.inputs[6]); nt.links.new(b, x.inputs[7]); return x.outputs[2]
    patch, mid, fine, grain = noise(0.05, 5), noise(0.35, 6), noise(2.5, 5), noise(14, 3)
    # olive green <-> dry yellow-brown grass, driven by a broad patch noise, with finer variation
    grass = ramp(patch, [(0.38, (0.08, 0.14, 0.03)), (0.5, (0.17, 0.2, 0.06)), (0.64, (0.36, 0.29, 0.1))])
    dark = ramp(mid, [(0.35, (0.35, 0.35, 0.35)), (0.65, (1.0, 1.0, 1.0))])
    c = nt.nodes.new("ShaderNodeMix"); c.data_type = "RGBA"; c.blend_type = "MULTIPLY"; c.inputs[0].default_value = 1.0
    nt.links.new(grass.outputs[0], c.inputs[6]); nt.links.new(dark.outputs[0], c.inputs[7])
    pn = noise(0.11, 4)
    pm = nt.nodes.new("ShaderNodeMapRange"); pm.inputs[1].default_value = 0.56; pm.inputs[2].default_value = 0.64
    nt.links.new(pn.outputs["Fac"], pm.inputs[0])
    dirt = ramp(fine, [(0.3, (0.12, 0.09, 0.06)), (0.7, (0.2, 0.16, 0.1))])
    col = mixc(pm.outputs[0], c.outputs[2], dirt.outputs[0])
    gr = ramp(grain, [(0.3, (0.75, 0.75, 0.75)), (0.7, (1.2, 1.2, 1.2))])
    g2 = nt.nodes.new("ShaderNodeMix"); g2.data_type = "RGBA"; g2.blend_type = "MULTIPLY"; g2.inputs[0].default_value = 1.0
    nt.links.new(col, g2.inputs[6]); nt.links.new(gr.outputs[0], g2.inputs[7])
    nt.links.new(g2.outputs[2], bs.inputs["Base Color"])
    bs.inputs["Roughness"].default_value = 1.0
    bump = nt.nodes.new("ShaderNodeBump"); bump.inputs["Strength"].default_value = 1.0; bump.inputs["Distance"].default_value = 0.25
    bm = nt.nodes.new("ShaderNodeMath"); bm.operation = "ADD"
    nt.links.new(grain.outputs["Fac"], bm.inputs[0]); nt.links.new(fine.outputs["Fac"], bm.inputs[1])
    nt.links.new(bm.outputs[0], bump.inputs["Height"]); nt.links.new(bump.outputs[0], bs.inputs["Normal"])
    haze(nt, bs, out)
    ob.data.materials.append(m)
    return ob


def tank(name, loc, yaw, scale, team_img_cache={}):
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=os.path.join(MODELS, name + ".glb"))
    new = [o for o in bpy.data.objects if o not in before]
    root = bpy.data.objects.new(name + "_root", None); bpy.context.scene.collection.objects.link(root)
    for o in new:
        if o.parent is None: o.parent = root
    alb = os.path.join(MODELS, name + "_albedo.png"); tm = os.path.join(MODELS, name + "_team.png")
    ia = bpy.data.images.load(alb); it = bpy.data.images.load(tm)
    a = np.array(ia.pixels[:], np.float32).reshape(-1, 4); t = np.array(it.pixels[:], np.float32).reshape(-1, 4)[:, :1]
    a[:, :3] = np.clip(a[:, :3] * 1.25, 0, 1)
    lum = a[:, :3].mean(1, keepdims=True)
    c = a[:, :3] + (np.array(TEAM) * (0.45 + lum * 1.3) - a[:, :3]) * t
    from PIL import Image  # baked texture goes through a temp PNG: a pixel-set image lost its data at render time
    import tempfile
    px = np.ones_like(a); px[:, :3] = np.clip(c, 0, 1)
    side = ia.size[0]
    srgb = np.where(px[:, :3] <= 0.0031308, px[:, :3] * 12.92, 1.055 * np.power(px[:, :3], 1 / 2.4) - 0.055)
    tmp = os.path.join(tempfile.gettempdir(), f"_lrmb_{name}_{os.getpid()}.png")
    Image.fromarray((np.clip(srgb, 0, 1) * 255 + 0.5).astype(np.uint8).reshape(ia.size[1], side, 3)[::-1]).save(tmp)
    img = bpy.data.images.load(tmp); img.pack(); os.remove(tmp)
    for o in new:
        for s in o.material_slots:
            m = s.material
            if not m or not m.node_tree: continue
            m = s.material = m.copy()
            nt = m.node_tree
            bs = next((n for n in nt.nodes if n.type == "BSDF_PRINCIPLED"), None)
            if not bs: continue
            for n in nt.nodes:
                if n.type == "TEX_IMAGE": nt.nodes.remove(n)
            ti = nt.nodes.new("ShaderNodeTexImage"); ti.image = img
            nt.links.new(ti.outputs["Color"], bs.inputs["Base Color"])
            for k in ("Normal",):
                for l in list(bs.inputs[k].links): nt.links.remove(l)
            bs.inputs["Roughness"].default_value = 0.65; bs.inputs["Metallic"].default_value = 0.0
    root.location = loc; root.scale = (scale,) * 3; root.rotation_euler = (0, 0, yaw)
    return root


def main():
    a = sys.argv[1:]; W_, H_, samples, out = 1920, 1080, 128, OUT
    i = 0
    while i < len(a):
        if a[i] == "--size": W_, H_ = map(int, a[i + 1].lower().split("x")); i += 1
        elif a[i] == "--samples": samples = int(a[i + 1]); i += 1
        elif a[i] == "--out": out = a[i + 1]; i += 1
        i += 1
    bpy.ops.wm.read_factory_settings(use_empty=True)
    sc = bpy.context.scene
    sc.render.engine = "CYCLES"; sc.cycles.device = "CPU"; sc.cycles.samples = samples
    sc.cycles.use_denoising = True; sc.cycles.seed = 3
    sc.render.threads_mode = "AUTO"
    sc.render.resolution_x, sc.render.resolution_y = W_, H_; sc.render.resolution_percentage = 100
    sc.view_settings.view_transform = "AgX"; sc.view_settings.look = "None"; sc.view_settings.exposure = 0.7
    sc.render.image_settings.file_format = "PNG"; sc.render.image_settings.color_mode = "RGB"
    sc.render.image_settings.color_depth = "8"; sc.render.image_settings.compression = 100
    sc.cycles.max_bounces = 4; sc.cycles.volume_bounces = 1; sc.cycles.volume_step_rate = 4.0

    xs, ys, h = height_fn()
    build_terrain(xs, ys, h)
    scatter(xs, ys, h)

    # tanks on the right ridge, barrels toward the far left
    spec = [("overtaker", 1.0, 3.0, 0.2), ("crony", 5.0, 11.0, 0.17), ("overtaker", 12.5, 20.0, 0.27)]
    for k, (n, x, y, yaw_extra) in enumerate(spec):
        z = sample(xs, ys, h, x, y)
        tank(n, (x, y, z - 0.03), math.radians(52 + 8 * k - 6 * (k == 2)), yaw_extra)

    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam")); sc.collection.objects.link(cam)
    cam.data.lens = 30; cam.data.sensor_width = 36
    cam.location = (CAM_LOC[0], CAM_LOC[1], sample(xs, ys, h, CAM_LOC[0], CAM_LOC[1]) + 2.4)
    tgt = Vector((-8.0, 60.0, 5.0))
    cam.rotation_euler = (tgt - Vector(cam.location)).to_track_quat("-Z", "Y").to_euler()
    cam.data.clip_end = 2000; sc.camera = cam

    # low sun from behind-left (towards the camera), soft
    sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN")); sc.collection.objects.link(sun)
    sun.data.energy = 4.2; sun.data.angle = math.radians(6); sun.data.color = (1.0, 0.9, 0.8)
    sun.rotation_euler = (math.radians(80), 0, math.radians(-55))  # points down, towards -x,+y? set below
    d = Vector((0.7, -0.55, -0.2)).normalized()   # light travels toward camera-right, slightly down
    sun.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()

    # world: gradient sky + haze volume
    wd = bpy.data.worlds.new("w"); wd.use_nodes = True; sc.world = wd
    nt = wd.node_tree; nt.nodes.clear()
    out_n = nt.nodes.new("ShaderNodeOutputWorld")
    bg = nt.nodes.new("ShaderNodeBackground"); bg.inputs["Strength"].default_value = 1.0
    tc = nt.nodes.new("ShaderNodeTexCoord"); sep = nt.nodes.new("ShaderNodeSeparateXYZ")
    nt.links.new(tc.outputs["Generated"], sep.inputs[0])
    # Generated for world = view direction (unit vector), z = 0 at the horizon
    mr = nt.nodes.new("ShaderNodeMapRange"); mr.inputs[1].default_value = 0.0; mr.inputs[2].default_value = 0.32
    mr.clamp = True; mr.interpolation_type = "SMOOTHERSTEP"
    nt.links.new(sep.outputs["Z"], mr.inputs[0])
    ramp = nt.nodes.new("ShaderNodeValToRGB"); cr = ramp.color_ramp
    cr.elements[0].position = 0.0; cr.elements[0].color = (*SKY_H, 1)
    e = cr.elements.new(0.3); e.color = (*SKY_M, 1)
    cr.elements[-1].position = 1.0; cr.elements[-1].color = (*SKY_T, 1)
    nt.links.new(mr.outputs[0], ramp.inputs[0]); nt.links.new(ramp.outputs[0], bg.inputs[0])
    # mottled cloud darkness
    nz = nt.nodes.new("ShaderNodeTexNoise"); nz.inputs["Scale"].default_value = 2.5; nz.inputs["Detail"].default_value = 5
    nt.links.new(tc.outputs["Generated"], nz.inputs["Vector"])
    mu = nt.nodes.new("ShaderNodeMix"); mu.data_type = "RGBA"; mu.blend_type = "MULTIPLY"
    mu.inputs[0].default_value = 0.5
    nt.links.new(ramp.outputs[0], mu.inputs[6]); nt.links.new(nz.outputs["Color"], mu.inputs[7])
    nt.links.new(mu.outputs[2], bg.inputs[0])
    nt.links.new(bg.outputs[0], out_n.inputs["Surface"])
    os.makedirs(os.path.dirname(os.path.abspath(out)), exist_ok=True)
    sc.render.filepath = out
    bpy.ops.render.render(write_still=True)


if __name__ == "__main__":
    main()
