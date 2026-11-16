#!/usr/bin/env python3
"""Converts Long Range Tank mod assets (SCM models, DDS textures) into
game assets: GLB meshes with one node per bone, PNG albedo / team mask /
normal maps and PNG build icons.

    .venv/bin/python tools/convert_assets.py "<path to LongRangeTank mod>" [--only trooper,buggy]
    .venv/bin/python tools/convert_assets.py --props "<maps/mining_town.v0001>"
    .venv/bin/python tools/convert_assets.py --stock "<SupCom FA>/gamedata/units.scd"

--props converts the map props listed in MAP_PROPS (the Mining Town buildings, made by the mod's
author), --stock the stock UEF civilian buildings in STOCK_UNITS; both are used by
assets/data/factions/civilian.json. Needs numpy and Pillow (the project's .venv has them).

The blueprint's .sca animations become glTF animations in the GLB (see ANIMS: which blueprint key gives
which animation name; the game's view decides from the name when to play it). Key times are divided by the
blueprint's rate, so the game plays every animation at 1x. Only bones that move in an animation (or that it
holds at another angle than the rest pose, like the trooper's walking lean) get a track: a static offset of
a bone would otherwise make the model jump when the animation starts.

Output goes to assets/models and assets/icons/units. Coordinates are converted
from SupCom's left-handed space to glTF's right-handed one (forward = -Z)."""
import glob, io, json, os, re, struct, sys, zipfile
import numpy as np
from PIL import Image

sys.path.insert(0, os.path.dirname(__file__))
from scm import Scm
from sca import Sca
import bp

# asset name -> mod unit folder
UNITS = {
    "buggy": "units/UEX0101", "trooper": "units/UEX0102", "crony": "units/UEX0105", "bohdana": "units/UEX0106",
    "robin": "units/UEX0107", "dragoon": "units/UEX0108", "mbfag": "units/UEX0109", "overtaker": "units/UEX0110",
    "pike": "units/UEX0111", "radar_car": "units/ULX0103", "delivery_boy": "units/UXA0101",
    "solar": "units/UBX0107", "windmill": "units/UBX0108", "heavy_battery": "units/UBX0109",
    "hmg_turret": "units/UBX0110", "gauss_turret": "units/UBX0111",
    "air_scout": "hook/units/UEA0101", "interceptor": "hook/units/UEA0102", "bomber": "hook/units/UEA0103",
    "gunship": "hook/units/UEA0203", "land_factory": "hook/units/UEB0101", "air_factory": "hook/units/UEB0102",
    "land_factory2": "hook/units/UEB0201", "air_factory2": "hook/units/UEB0202", "power_plant": "hook/units/UEB1102",
    "pump": "hook/units/UEB1103", "pump2": "hook/units/UEB1202", "pump3": "hook/units/UEB1302",
    "energy_storage": "hook/units/UEB1105", "mass_storage": "hook/units/UEB1106", "aa_turret": "hook/units/UEB2104",
    "engineer": "hook/units/UEL0105", "engineer2": "hook/units/UEL0208", "engineer3": "hook/units/UEL0309",
    "eng_station": "hook/units/XEB0104", "sparky": "hook/units/XEL0209", "bomb": "projectiles/LRTHeavyBomb01",
    "air_transport": "hook/units/UEA0104",
}
# blueprint Display key -> (animation name, key of its rate, default rate, loops). How SupCom plays them and
# what the game does with each name: view::drive_anims.
ANIMS = {
    "AnimationWalk": ("walk", "AnimationWalkRate", 1.0, True),      # cycle while moving
    "AnimationUpgrade": ("upgrade", None, 1.0, False),              # follows the upgrade's progress
    "AnimationBuild": ("open", "AnimationBuildRate", 1.0, False),   # engineers: unfold to build, fold back
    "AnimationWater": ("water", None, 1.0, False),                  # on water
    "AnimationLand": ("land", None, 1.0, False),                    # landing legs (first frame: legs out)
    "AnimationFinishBuildLand": ("rolloff", None, 1.0, False),      # factory platform when a unit leaves
    # AnimationOpen: "work" (loops while producing) on mass extractors, else "doors" (engineering
    # station: opens at rate 2 to the middle while working, plays on to the end when done).
}
# asset name -> prop name in a map's env/Props folder (--props)
MAP_PROPS = {f"civ_house{i}": f"UEF_modern_house_00{i}" for i in range(1, 10)}
MAP_PROPS.update({f"civ_old_house{i}": f"UEF_old_house_00{i}" for i in range(1, 5)})
MAP_PROPS.update({f"civ_corner_house{i}": f"UEF_old_corner_house_00{i}" for i in range(1, 5)})
MAP_PROPS.update({"civ_apartments1": "UEF_modern_apartments_001", "civ_apartments2": "UEF_modern_apartments_002", "civ_church": "UEF_church",
                  "civ_grocery": "UEF_grocery_store", "civ_factory": "UEF_modern_factory", "civ_containers1": "UEF_Containers_7_001",
                  "civ_containers2": "UEF_Containers_7_009"})
# asset name -> stock unit id in units.scd (--stock)
STOCK_UNITS = {"civ_uef_residential": "UEC1101", "civ_uef_lab": "UEC1201", "civ_uef_admin": "UEC1301", "civ_uef_farm": "UEC1401",
               "civ_uef_plant": "UEC1501", "civ_uef_hall": "XEC1301", "civ_uef_command": "XEC1401"}
TEX_MAX = 512


def find(folder, *patterns):
    for f in sorted(os.listdir(folder)):
        low = f.lower()
        if "lod1" in low:
            continue
        if any(low.endswith(p) for p in patterns):
            return os.path.join(folder, f)
    return None


def write_glb(path, nodes, meshes_data, anims=()):
    """nodes: list of dict(name, parent, matrix(4x4 col-convention), optional trs=(translation, rotation xyzw));
    meshes_data: node index -> (pos, nrm, uv, idx);
    anims: list of dict(name, times, channels: node index -> (translations Nx3, rotations Nx4 xyzw)).
    Animated nodes must have `trs` (glTF forbids animating a node given by a matrix)."""
    blob = bytearray()
    views, accessors, meshes = [], [], []

    def add(arr, target, comp, typ, minmax=False):
        while len(blob) % 4:
            blob.append(0)
        raw = arr.tobytes()
        views.append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(raw)})
        if target:
            views[-1]["target"] = target
        blob.extend(raw)
        acc = {"bufferView": len(views) - 1, "componentType": comp, "count": len(arr), "type": typ}
        if minmax:
            acc["min"] = arr.min(0).tolist()
            acc["max"] = arr.max(0).tolist()
        accessors.append(acc)
        return len(accessors) - 1

    gl_nodes = []
    for i, n in enumerate(nodes):
        if "trs" in n:
            g = {"name": n["name"], "translation": [round(float(v), 6) for v in n["trs"][0]],
                 "rotation": [round(float(v), 6) for v in n["trs"][1]]}
        else:
            g = {"name": n["name"], "matrix": n["matrix"].T.reshape(-1).round(6).tolist()}
        kids = [j for j, m in enumerate(nodes) if m["parent"] == i]
        if kids:
            g["children"] = kids
        if i in meshes_data:
            pos, nrm, uv, idx = meshes_data[i]
            prim = {"attributes": {"POSITION": add(pos.astype("<f4"), 34962, 5126, "VEC3", True),
                                   "NORMAL": add(nrm.astype("<f4"), 34962, 5126, "VEC3"),
                                   "TEXCOORD_0": add(uv.astype("<f4"), 34962, 5126, "VEC2")},
                    "indices": add(idx.astype("<u4").reshape(-1), 34963, 5125, "SCALAR"), "material": 0}
            meshes.append({"name": n["name"], "primitives": [prim]})
            g["mesh"] = len(meshes) - 1
        gl_nodes.append(g)
    gl_anims = []
    for a in anims:
        times = np.asarray(a["times"], dtype="<f4").reshape(-1, 1)
        t_acc = add(times, None, 5126, "SCALAR", True)
        samplers, channels = [], []
        for node, (tr, rot) in sorted(a["channels"].items()):
            for path_, arr, typ in (("translation", tr, "VEC3"), ("rotation", rot, "VEC4")):
                samplers.append({"input": t_acc, "output": add(np.asarray(arr, dtype="<f4"), None, 5126, typ), "interpolation": "LINEAR"})
                channels.append({"sampler": len(samplers) - 1, "target": {"node": node, "path": path_}})
        gl_anims.append({"name": a["name"], "samplers": samplers, "channels": channels})
    roots = [i for i, n in enumerate(nodes) if n["parent"] < 0]
    doc = {"asset": {"version": "2.0", "generator": "longrange convert_assets"}, "scene": 0,
           "scenes": [{"nodes": roots}], "nodes": gl_nodes, "meshes": meshes,
           "materials": [{"name": "unit", "pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0.3, "roughnessFactor": 0.6}}],
           "buffers": [{"byteLength": len(blob)}], "bufferViews": views, "accessors": accessors}
    if gl_anims:
        doc["animations"] = gl_anims
    js = json.dumps(doc, separators=(",", ":")).encode()
    js += b" " * (-len(js) % 4)
    while len(blob) % 4:
        blob.append(0)
    with open(path, "wb") as f:
        f.write(struct.pack("<4sII", b"glTF", 2, 12 + 8 + len(js) + 8 + len(blob)))
        f.write(struct.pack("<I4s", len(js), b"JSON") + js)
        f.write(struct.pack("<I4s", len(blob), b"BIN\0") + bytes(blob))


def to_gltf_local(pos, rot):
    """A SupCom bone-local transform (pos, quaternion w x y z; left handed) as glTF translation and
    rotation (x y z w): mirroring z negates the quaternion's x and y."""
    w, x, y, z = rot
    return [pos[0], pos[1], -pos[2]], [-x, -y, z, w]


def quat_angle(a, b):
    """Degrees between two rotations (w x y z)."""
    return 2 * np.degrees(np.arccos(min(1.0, abs(float(np.dot(a, b))))))


def bake_anim(m, name, sca, rate, loop):
    """Channels of an .sca animation for the bones of model `m` (by name). `loop`: frame 0 again at the end
    (for cycles whose last frame leads back to the first)."""
    index = {b["name"].lower(): i for i, b in enumerate(m.bones)}
    times = [t / rate for t in sca.times()]
    frames = sca.frames
    if loop:
        times.append(sca.duration / rate)
        frames = frames + frames[:1]
    channels = {}
    for j, bone in enumerate(sca.names):
        i = index.get(bone.lower())
        if i is None:
            continue
        ps = np.array([f["bones"][j][0] for f in sca.frames])
        qs = [f["bones"][j][1] for f in sca.frames]
        moves = np.abs(ps - ps[0]).max() > 1e-3 or max(quat_angle(q, qs[0]) for q in qs) > 0.5
        if not moves and quat_angle(qs[0], m.bones[i]["rot"]) < 1.0:
            continue
        tr, rot = [], []
        for f in frames:
            t, r = to_gltf_local(*f["bones"][j])
            if rot and np.dot(rot[-1], r) < 0:   # keep neighbouring keys on the same hemisphere
                r = [-v for v in r]
            tr.append(t)
            rot.append(r)
        channels[i] = (tr, rot)
    return {"name": name, "times": times, "channels": channels}


def convert_model(scm_path, out, anims=()):
    """`anims`: (name, Sca, rate, loop) to bake into the GLB."""
    m = Scm(scm_path)
    S = np.diag([1.0, 1.0, -1.0, 1.0])
    world = []
    for b in m.bones:
        w = np.linalg.inv(b["inv"].T)          # column-vector rest matrix, left handed
        world.append(S @ w @ S)                # right handed
    nodes, data = [], {}
    bone_of = m.verts["bone"][:, 0].astype(int)
    for i, b in enumerate(m.bones):
        local = world[i] if b["parent"] < 0 else np.linalg.inv(world[b["parent"]]) @ world[i]
        nodes.append({"name": b["name"], "parent": b["parent"], "matrix": local})
    baked = [bake_anim(m, name, sca, rate, loop) for name, sca, rate, loop in anims]
    for a in baked:
        for i in a["channels"]:
            nodes[i]["trs"] = to_gltf_local(m.bones[i]["pos"], m.bones[i]["rot"])
    pos = m.verts["pos"].astype(np.float64) * [1, 1, -1]
    nrm = m.verts["nrm"].astype(np.float64) * [1, 1, -1]
    for i in range(len(m.bones)):
        tri = m.indices[bone_of[m.indices[:, 0]] == i]
        if len(tri) == 0:
            continue
        used = np.unique(tri)
        remap = -np.ones(len(m.verts), dtype=np.int64)
        remap[used] = np.arange(len(used))
        inv = np.linalg.inv(world[i])
        p = (inv[:3, :3] @ pos[used].T).T + inv[:3, 3]
        n = (inv[:3, :3] @ nrm[used].T).T
        n /= np.maximum(np.linalg.norm(n, axis=1, keepdims=True), 1e-9)
        idx = remap[tri][:, ::-1]              # flip winding for the handedness change
        data[i] = (p, n, m.verts["uv"][used], idx)
    write_glb(out, nodes, data, baked)
    allp = pos
    return {"bones": [b["name"] for b in m.bones], "min": allp.min(0).round(3).tolist(), "max": allp.max(0).round(3).tolist(),
            "verts": len(m.verts), "tris": len(m.indices),
            "bone_pos": {b["name"]: world[i][:3, 3].round(3).tolist() for i, b in enumerate(m.bones)}}


def shrink(im):
    if max(im.size) > TEX_MAX:
        im = im.resize((TEX_MAX, TEX_MAX), Image.LANCZOS)
    return im


def save_textures(root, name, alb, spec, nor):
    """Albedo, team mask (SpecTeam alpha) and normal map; each a path, a file object or None."""
    if alb:
        shrink(Image.open(alb).convert("RGB")).save(f"{root}/models/{name}_albedo.png", optimize=True)
    if spec:
        a = shrink(Image.open(spec).convert("RGBA")).split()[3]
        arr = np.asarray(a).astype(np.float32)
        if arr.max() > 0:
            arr = np.clip(arr / max(arr.max(), 1.0) * 255.0, 0, 255)
        Image.fromarray(arr.astype(np.uint8), "L").save(f"{root}/models/{name}_team.png", optimize=True)
    if nor:
        a = np.asarray(shrink(Image.open(nor).convert("RGBA"))).astype(np.float32) / 255.0
        x, y = a[..., 3] * 2 - 1, a[..., 1] * 2 - 1
        z = np.sqrt(np.clip(1 - x * x - y * y, 0.0, 1.0))
        out = np.stack([x * 0.5 + 0.5, y * 0.5 + 0.5, z * 0.5 + 0.5], -1)
        Image.fromarray((out * 255).astype(np.uint8), "RGB").save(f"{root}/models/{name}_normal.png", optimize=True)


def print_size(info, name):
    """Size at the blueprint scale (x, height, z), for the unit JSON."""
    i, s = info[name], info[name].get("scale", 1.0)
    size = [round((b - a) * s, 3) for a, b in zip(i["min"], i["max"])]
    print(f"{name:20} scale {s:<6} size {size}  ({i['verts']} verts)")


def convert_props(folder, root, info):
    """Buildings from a map's env/Props folder (MAP_PROPS)."""
    props = os.path.join(folder, "env", "Props")
    files = {f.lower(): os.path.join(props, f) for f in os.listdir(props)}
    for name, stem in MAP_PROPS.items():
        b = bp.load(files[(stem + "_prop.bp").lower()])
        lod = b["Display"]["Mesh"]["LODs"][0]
        tex = lambda key: files.get((lod.get(key) or "").lower())
        info[name] = convert_model(files[(stem + "_lod0.scm").lower()], f"{root}/models/{name}.glb")
        info[name]["scale"] = b.get("Display", {}).get("UniformScale", 1.0)
        save_textures(root, name, tex("AlbedoName"), tex("SpecularName"), tex("NormalsName"))
        print_size(info, name)


def convert_stock(scd, root, info):
    """Stock buildings from the game's units.scd archive (STOCK_UNITS)."""
    z = zipfile.ZipFile(scd)
    names = {n.lower(): n for n in z.namelist()}
    for name, uid in STOCK_UNITS.items():
        u = uid.lower()
        get = lambda f: z.read(names[f"units/{u}/{f}"]) if f"units/{u}/{f}" in names else None
        info[name] = convert_model(get(f"{u}_lod0.scm"), f"{root}/models/{name}.glb")
        m = re.search(r"UniformScale\s*=\s*([\d.]+)", get(f"{u}_unit.bp").decode("latin1"))
        info[name]["scale"] = float(m.group(1)) if m else 1.0
        opened = lambda f: io.BytesIO(get(f)) if get(f) else None
        save_textures(root, name, opened(f"{u}_albedo.dds"), opened(f"{u}_specteam.dds"), opened(f"{u}_normalsts.dds"))
        print_size(info, name)


def blueprint_anims(b, folder, name):
    """(name, Sca, rate, loop) for every animation the blueprint names that exists in the unit folder."""
    disp = b.get("Display", {})
    cats = [c.upper() for c in b.get("Categories", [])]
    want = []
    for key, (anim, rate_key, rate, loop) in ANIMS.items():
        if disp.get(key):
            want.append((disp[key], anim, float(disp.get(rate_key, rate)) if rate_key else rate, loop))
    if disp.get("AnimationOpen"):
        if "MASSEXTRACTION" in cats:
            want.append((disp["AnimationOpen"], "work", 1.0, False))
        else:
            want.append((disp["AnimationOpen"], "doors", 2.0, False))
    for w in (b.get("Weapon") or []):
        if isinstance(w, dict) and w.get("WeaponUnpackAnimation"):
            want.append((w["WeaponUnpackAnimation"], "unpack", float(w.get("WeaponUnpackAnimationRate", 1.0)), False))
            break
    # Blueprint paths are game- or mod-absolute (/units/..., /mods/LongRangeTank/units/...): look the file
    # up by name in the unit folder.
    files = {f.lower(): os.path.join(folder, f) for f in os.listdir(folder)}
    out = []
    for path, anim, rate, loop in want:
        f = files.get(os.path.basename(path).lower())
        if f:
            out.append((anim, Sca(f), rate, loop))
        else:
            print(f"{name}: {anim} animation {path} not in the mod folder")
    return out


def main():
    root = os.path.join(os.path.dirname(__file__), "..", "assets")
    os.makedirs(root + "/models", exist_ok=True)
    os.makedirs(root + "/icons/units", exist_ok=True)
    info = {}
    if sys.argv[1] in ("--props", "--stock"):
        (convert_props if sys.argv[1] == "--props" else convert_stock)(sys.argv[2], root, info)
        os.makedirs(os.path.join(os.path.dirname(__file__), "out"), exist_ok=True)
        json.dump(info, open(os.path.join(os.path.dirname(__file__), "out", sys.argv[1][2:] + "_info.json"), "w"), indent=1)
        return
    mod = sys.argv[1]
    only = sys.argv[sys.argv.index("--only") + 1].split(",") if "--only" in sys.argv else None
    for name, rel in UNITS.items():
        if only and name not in only:
            continue
        folder = os.path.join(mod, rel)
        scm = find(folder, "lod0.scm")
        if not scm:
            print("no model for", name)
            continue
        bpf = find(folder, "_unit.bp")
        b = bp.load(bpf) if bpf else None
        anims = blueprint_anims(b or {}, folder, name)
        info[name] = convert_model(scm, f"{root}/models/{name}.glb", anims)
        if anims:
            info[name]["anims"] = [a[0] for a in anims]
        if b:
            info[name]["scale"] = b.get("Display", {}).get("UniformScale", 1.0)
            info[name]["size"] = [b.get("SizeX"), b.get("SizeY"), b.get("SizeZ")]
        save_textures(root, name, find(folder, "_albedo.dds"), find(folder, "_specteam.dds"), find(folder, "_normalsts.dds"))
        uid = os.path.basename(rel)
        ico = os.path.join(mod, "icons/units", uid + "_icon.dds")
        if os.path.exists(ico):
            Image.open(ico).convert("RGBA").save(f"{root}/icons/units/{name}.png")
        print(f"{name:16} {info[name]['verts']:5} verts  bones: {', '.join(info[name]['bones'][:10])}")
    os.makedirs(os.path.join(os.path.dirname(__file__), "out"), exist_ok=True)
    json.dump(info, open(os.path.join(os.path.dirname(__file__), "out", "models_info.json"), "w"), indent=1)


if __name__ == "__main__":
    main()
