#!/usr/bin/env python3
"""Converts Long Range Tank mod assets (SCM models, DDS textures) into
game assets: GLB meshes with one node per bone, PNG albedo / team mask /
normal maps and PNG build icons.

    .venv/bin/python tools/convert_assets.py "<path to LongRangeTank mod>"
    .venv/bin/python tools/convert_assets.py --props "<maps/mining_town.v0001>"
    .venv/bin/python tools/convert_assets.py --stock "<SupCom FA>/gamedata/units.scd"

--props converts the map props listed in MAP_PROPS (the Mining Town buildings, made by the mod's
author), --stock the stock UEF civilian buildings in STOCK_UNITS; both are used by
assets/data/factions/civilian.json. Needs numpy and Pillow (the project's .venv has them).

Output goes to assets/models and assets/icons/units. Coordinates are converted
from SupCom's left-handed space to glTF's right-handed one (forward = -Z)."""
import glob, io, json, os, re, struct, sys, zipfile
import numpy as np
from PIL import Image

sys.path.insert(0, os.path.dirname(__file__))
from scm import Scm
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


def write_glb(path, nodes, meshes_data):
    """nodes: list of dict(name, parent, matrix(4x4 col-convention)); meshes_data: node index -> (pos, nrm, uv, idx)."""
    blob = bytearray()
    views, accessors, meshes = [], [], []

    def add(arr, target, comp, typ, minmax=False):
        while len(blob) % 4:
            blob.append(0)
        raw = arr.tobytes()
        views.append({"buffer": 0, "byteOffset": len(blob), "byteLength": len(raw), "target": target})
        blob.extend(raw)
        acc = {"bufferView": len(views) - 1, "componentType": comp, "count": len(arr), "type": typ}
        if minmax:
            acc["min"] = arr.min(0).tolist()
            acc["max"] = arr.max(0).tolist()
        accessors.append(acc)
        return len(accessors) - 1

    gl_nodes = []
    for i, n in enumerate(nodes):
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
    roots = [i for i, n in enumerate(nodes) if n["parent"] < 0]
    doc = {"asset": {"version": "2.0", "generator": "longrange convert_assets"}, "scene": 0,
           "scenes": [{"nodes": roots}], "nodes": gl_nodes, "meshes": meshes,
           "materials": [{"name": "unit", "pbrMetallicRoughness": {"baseColorFactor": [1, 1, 1, 1], "metallicFactor": 0.3, "roughnessFactor": 0.6}}],
           "buffers": [{"byteLength": len(blob)}], "bufferViews": views, "accessors": accessors}
    js = json.dumps(doc, separators=(",", ":")).encode()
    js += b" " * (-len(js) % 4)
    while len(blob) % 4:
        blob.append(0)
    with open(path, "wb") as f:
        f.write(struct.pack("<4sII", b"glTF", 2, 12 + 8 + len(js) + 8 + len(blob)))
        f.write(struct.pack("<I4s", len(js), b"JSON") + js)
        f.write(struct.pack("<I4s", len(blob), b"BIN\0") + bytes(blob))


def convert_model(scm_path, out):
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
    write_glb(out, nodes, data)
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
    for name, rel in UNITS.items():
        folder = os.path.join(mod, rel)
        scm = find(folder, "lod0.scm")
        if not scm:
            print("no model for", name)
            continue
        info[name] = convert_model(scm, f"{root}/models/{name}.glb")
        bpf = find(folder, "_unit.bp")
        if bpf:
            b = bp.load(bpf)
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
