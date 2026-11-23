"""Renders exported models (assets/models/<name>.glb) with Blender for a look at them: a contact sheet of
3/4 views, team areas in blue.   .venv/bin/python art/render.py out.png pump apc_heavy3 [--size 360] [--anim work:0.5]"""
import math, os, sys
import bpy
import numpy as np
from mathutils import Vector

HERE = os.path.dirname(os.path.abspath(__file__))
MODELS = os.environ.get("LR_MODELS_OUT") or os.path.join(HERE, "..", "assets", "models")
TEAM = tuple(float(x) for x in os.environ.get("LR_TEAM", "0.2,0.4,0.95").split(","))


def setup(size):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    sc = bpy.context.scene
    sc.render.engine = "BLENDER_WORKBENCH"
    sc.display.shading.light = "STUDIO"
    sc.display.shading.color_type = "TEXTURE"
    sc.display.shading.show_shadows = True
    sc.display.shading.show_cavity = True
    sc.render.resolution_x = sc.render.resolution_y = size
    sc.render.film_transparent = False
    sc.view_settings.view_transform = "Standard"
    sc.world = bpy.data.worlds.new("w")
    sc.display.shading.background_type = "VIEWPORT" if False else "THEME"
    return sc


def teamed(img_a, img_t, name):
    a = np.array(img_a.pixels[:]).reshape(-1, 4)
    t = np.array(img_t.pixels[:]).reshape(-1, 4)[:, 0:1]
    lum = a[:, :3].mean(1, keepdims=True)
    c = a[:, :3] + (np.array(TEAM) * (0.45 + lum * 1.3) - a[:, :3]) * t
    out = bpy.data.images.new(name, img_a.size[0], img_a.size[1])
    px = np.ones_like(a)
    px[:, :3] = np.clip(c, 0, 1)
    out.pixels.foreach_set(px.ravel().astype(np.float32))
    return out


def render(name, size, anim, yaw):
    sc = setup(size)
    bpy.ops.import_scene.gltf(filepath=os.path.join(MODELS, name + ".glb"))
    alb = os.path.join(MODELS, name + "_albedo.png")
    tm = os.path.join(MODELS, name + "_team.png")
    if os.path.exists(alb):
        ia = bpy.data.images.load(alb)
        ia.colorspace_settings.name = "Non-Color"
        bright = float(os.environ.get("LR_BRIGHT", "0") or 0) or (2.1 if np.array(ia.pixels[:]).reshape(-1, 4)[:, :3].mean() < 0.22 else 1.0)
        if bright != 1.0:
            px = np.clip(np.array(ia.pixels[:], np.float32).reshape(-1, 4) * [bright, bright, bright, 1], 0, 1)
            ia.pixels.foreach_set(px.ravel().astype(np.float32))
        it = bpy.data.images.load(tm) if os.path.exists(tm) else None
        img = teamed(ia, it, "t") if it is not None and tuple(it.size) == tuple(ia.size) else ia
        for m in bpy.data.materials:
            if m.node_tree:
                for n in m.node_tree.nodes:
                    if n.type == "TEX_IMAGE":
                        n.image = img
                if not any(n.type == "TEX_IMAGE" for n in m.node_tree.nodes):
                    bsdf = next((n for n in m.node_tree.nodes if n.type == "BSDF_PRINCIPLED"), None)
                    if bsdf:
                        t = m.node_tree.nodes.new("ShaderNodeTexImage")
                        t.image = img
                        m.node_tree.links.new(t.outputs["Color"], bsdf.inputs["Base Color"])
    if anim:
        an, frac = anim
        for ob in sc.objects:
            ad = ob.animation_data
            if not ad:
                continue
            for tr in ad.nla_tracks:
                tr.mute = tr.name != an and not any(s.action and s.action.name.startswith(an) for s in tr.strips)
            act = next((s.action for tr in ad.nla_tracks for s in tr.strips if s.action and s.action.name.startswith(an)), None)
            if act:
                f0, f1 = act.frame_range
                sc.frame_set(int(f0 + (f1 - f0) * frac))
    else:
        for ob in sc.objects:
            if ob.animation_data:
                for tr in ob.animation_data.nla_tracks:
                    tr.mute = True
        sc.frame_set(0)
    bpy.context.view_layer.update()
    lo, hi = Vector((1e9,) * 3), Vector((-1e9,) * 3)
    for ob in sc.objects:
        if ob.type == "MESH":
            for c in ob.bound_box:
                w = ob.matrix_world @ Vector(c)
                lo, hi = Vector(map(min, lo, w)), Vector(map(max, hi, w))
    cen, rad = (lo + hi) / 2, (hi - lo).length / 2
    cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
    sc.collection.objects.link(cam)
    cam.data.type = "ORTHO"
    cam.data.ortho_scale = rad * 2.1
    a, p = math.radians(yaw), math.radians(32)
    d = Vector((-math.sin(a) * math.cos(p), math.cos(a) * math.cos(p), math.sin(p)))  # glTF import: front is +y
    cam.location = cen + d * rad * 4
    cam.rotation_euler = (-d).to_track_quat("-Z", "Y").to_euler()
    cam.data.clip_end = rad * 10
    sc.camera = cam
    path = os.path.join(os.environ.get("TMPDIR", "/tmp"), f"_lr_{name}.png")
    sc.render.filepath = path
    if os.environ.get("LR_TRANSPARENT"):  # tools/render_icons.py: no background, alpha kept
        sc.render.film_transparent = True
        sc.render.image_settings.color_mode = "RGBA"
    bpy.ops.render.render(write_still=True)
    return path


def main():
    a = sys.argv[1:]
    out, names, size, anim, yaw = a[0], [], 360, None, 35.0
    i = 1
    while i < len(a):
        if a[i] == "--size":
            size = int(a[i + 1]); i += 1
        elif a[i] == "--anim":
            n, f = a[i + 1].split(":"); anim = (n, float(f)); i += 1
        elif a[i] == "--yaw":
            yaw = float(a[i + 1]); i += 1
        else:
            names.append(a[i])
        i += 1
    paths = [render(n, size, anim, yaw) for n in names]
    from PIL import Image, ImageDraw
    cols = min(len(paths), 4)
    rows = (len(paths) + cols - 1) // cols
    sheet = Image.new("RGB", (cols * size, rows * size), (40, 40, 40))
    for k, (p, n) in enumerate(zip(paths, names)):
        im = Image.open(p).convert("RGB")
        ImageDraw.Draw(im).text((4, 4), n, fill=(255, 255, 255))
        sheet.paste(im, ((k % cols) * size, (k // cols) * size))
    sheet.save(out)
    print(out)


if __name__ == "__main__":
    main()
