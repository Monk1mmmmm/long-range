"""Runs inside Blender: exports the open .blend as a Long Range model.

    blender -b model.blend --python tools/blend_export.py -- OUT_DIR NAME [--front -y|+y|-x|+x] [--brightness B]

Usually called through tools/blend.py. Writes OUT_DIR/NAME.glb and, when the materials use image textures,
NAME_albedo.png (Base Color), NAME_team.png (an Image Texture node whose name or label contains "team"),
NAME_normal.png (the image of a Normal Map node). Prints the unit JSON "model" block to use.

What the game reads (src/view/models.rs::load_glb):
- every object is one part; the names of objects (or of bones, for objects parented to a bone) are what the
  unit JSON's turret block (yaw_bone, pitch_bone, muzzle_bone) and "spin" refer to;
- no skinning: an Armature modifier is ignored by the game, animate whole objects (or bones that objects
  are parented to);
- materials named team / light / dark / metal are drawn in that colour (team = the owner's colour);
  without an albedo texture other materials keep their Base Color; with one they use the texture;
- animations are Blender actions named after what the game plays them for (KNOWN below). One action may
  animate several objects (Blender's action slots). Keys are sampled on every frame of the action.
The .blend file itself is never changed (nothing is saved).
"""
import os
import shutil
import sys

import bpy
from mathutils import Matrix

KNOWN = {
    "walk": "cycle while moving, played at ground speed",
    "open": "engineers: unfold while building (first frame = folded, idle)",
    "upgrade": "follows an upgrade's progress",
    "work": "loops while a pump/extractor produces",
    "doors": "engineering station: open to the middle while working, then on to the end",
    "unpack": "out while the weapon has a target and the unit stands",
    "land": "landing legs: first frame legs out; played forward while flying",
    "water": "on water",
    "rolloff": "factory platform: out and back when a finished unit leaves",
}
TEAM_NAMES = {"team", "light", "dark", "metal"}


def args():
    a = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    if len(a) < 2:
        sys.exit("usage: blender -b file.blend --python tools/blend_export.py -- OUT_DIR NAME [--front -y] [--brightness 1]")
    out, name, front, brightness = a[0], a[1], "-y", 1.0
    i = 2
    while i < len(a):
        if a[i] == "--front":
            front = a[i + 1]
            i += 2
        elif a[i] == "--brightness":
            brightness = float(a[i + 1])
            i += 2
        else:
            sys.exit(f"unknown option {a[i]}")
    if front not in ("-y", "+y", "-x", "+x"):
        sys.exit("--front is one of -y, +y, -x, +x")
    return out, name, front, brightness


def warn(msg):
    print(f"WARNING: {msg}")


def check_scene(scene):
    for ob in scene.objects:
        for m in getattr(ob, "modifiers", []):
            if m.type == "ARMATURE":
                warn(f"{ob.name}: Armature modifier (skinning) is not supported, the game shows the rest pose. "
                     "Split the mesh into objects and parent them to the bones (Ctrl+P > Bone) instead.")
        if ob.type == "MESH" and ob.data.shape_keys:
            warn(f"{ob.name}: shape keys are not supported")
    for act in bpy.data.actions:
        if act.users == 0:
            continue
        if act.name not in KNOWN:
            warn(f"action '{act.name}' is not one the game plays; known: {', '.join(KNOWN)}")


def find_images(scene):
    """The images of the roles albedo / team / normal used by the scene's materials."""
    found = {"albedo": [], "team": [], "normal": []}

    def add(role, img):
        if img and img not in found[role]:
            found[role].append(img)

    mats = {s.material for ob in scene.objects if ob.type == "MESH" for s in ob.material_slots if s.material}
    for mat in mats:
        if not mat.node_tree:  # Blender 5 always uses nodes; older files may have none
            continue
        for node in mat.node_tree.nodes:
            if node.type == "TEX_IMAGE" and ("team" in node.name.lower() or "team" in node.label.lower()):
                add("team", node.image)
            if node.type == "BSDF_PRINCIPLED":
                for link in node.inputs["Base Color"].links:
                    if link.from_node.type == "TEX_IMAGE":
                        add("albedo", link.from_node.image)
            if node.type == "NORMAL_MAP":
                for link in node.inputs["Color"].links:
                    if link.from_node.type == "TEX_IMAGE":
                        add("normal", link.from_node.image)
    return found


def save_png(img, path):
    """Writes an image's pixels as they are stored (no view transform, so normal maps stay right)."""
    src = bpy.path.abspath(img.filepath) if img.source == "FILE" and img.filepath else ""
    if src and os.path.isfile(src) and src.lower().endswith(".png") and not img.is_dirty and not img.packed_file:
        if os.path.abspath(src) != os.path.abspath(path):
            shutil.copyfile(src, path)
        return
    # Packed, painted in Blender, generated or another format: copy the pixels into a plain image.
    w, h = img.size
    out = bpy.data.images.new("_lr_save", w, h, alpha=True)
    try:
        out.colorspace_settings.name = img.colorspace_settings.name
        out.pixels.foreach_set(_pixels(img))
        out.filepath_raw = path
        out.file_format = "PNG"
        out.save()
    finally:
        bpy.data.images.remove(out)


def _pixels(img):
    import array
    buf = array.array("f", [0.0]) * (img.size[0] * img.size[1] * 4)
    img.pixels.foreach_get(buf)
    return buf


def main():
    out, name, front, brightness = args()
    os.makedirs(out, exist_ok=True)
    scene = bpy.context.scene
    check_scene(scene)

    # The game's model forward is -Z (glTF, Y up); Blender's -Y becomes +Z on export.
    # Turn every root object under a temporary empty so the model's front ends up on -Z.
    angle = {"-y": 3.14159265358979, "+y": 0.0, "-x": -1.5707963267949, "+x": 1.5707963267949}[front]
    if angle != 0.0:
        root = bpy.data.objects.new("_lr_root", None)
        scene.collection.objects.link(root)
        root.matrix_world = Matrix.Rotation(angle, 4, "Z")
        for ob in list(scene.objects):
            if ob.parent is None and ob is not root:
                # Identity parent inverse: the object's own transform (and its animation) now sits
                # inside the turned root.
                ob.parent = root
                ob.matrix_parent_inverse = Matrix.Identity(4)
        bpy.context.view_layer.update()

    glb = os.path.join(out, name + ".glb")
    bpy.ops.export_scene.gltf(
        filepath=glb,
        export_format="GLB",
        export_yup=True,
        export_apply=True,  # modifiers (mirror, bevel...) applied
        export_materials="EXPORT",
        export_image_format="NONE",  # textures go to separate PNGs
        export_animations=True,
        export_animation_mode="ACTIONS",
        export_force_sampling=True,
        export_optimize_animation_size=False,  # every channel keeps every key: the game wants one timeline
        export_anim_slide_to_zero=True,
        export_skins=False,
        export_morph=False,
        export_extras=False,
    )
    print(f"wrote {glb}")

    model = {"mesh": f"models/{name}.glb", "scale": 1.0}
    images = find_images(scene)
    sizes = {}
    for role, suffix in (("albedo", "albedo"), ("team", "team"), ("normal", "normal")):
        imgs = images[role]
        if not imgs:
            continue
        if len(imgs) > 1:
            warn(f"{len(imgs)} {role} images ({', '.join(i.name for i in imgs)}); the game uses one per model "
                 f"(one UV layout), taking {imgs[0].name}")
        path = os.path.join(out, f"{name}_{suffix}.png")
        save_png(imgs[0], path)
        sizes[role] = tuple(imgs[0].size)
        model["team_mask" if role == "team" else role] = f"models/{name}_{suffix}.png"
        print(f"wrote {path}")
    if "team" in sizes and sizes.get("albedo") != sizes["team"]:
        warn(f"team mask {sizes['team']} and albedo {sizes.get('albedo')} differ in size: the game ignores the mask")
    if "team" in sizes and "albedo" not in sizes:
        warn("a team mask without an albedo texture is ignored; use a material named 'team' instead")
    if "albedo" in model:
        model["brightness"] = brightness

    used = sorted(a.name for a in bpy.data.actions if a.users and a.name in KNOWN)
    if used:
        print("animations: " + ", ".join(f"{a} ({KNOWN[a]})" for a in used))
    import json
    print("unit JSON:\n\"model\": " + json.dumps(model, indent=1))
    print("LR_END")


main()
