#!/usr/bin/env python3
"""Software preview of game models (no GPU, no game build): renders GLBs from assets/models with their
albedo, team mask (in a blue team colour) and simple sun shading, from a 3/4 view, into one contact sheet.

    python3 tools/preview_glb.py out.png crony bastion_overlay ...      # names of assets/models/<name>.glb
    python3 tools/preview_glb.py out.png --unit bastion apc_heavy3      # unit ids: model block from the faction JSON
    python3 tools/preview_glb.py out.png --all-units                    # every unit of every faction

Options: --size N (pixels per cell, default 320), --yaw DEG (view direction, default 35), --anim NAME:T
(pose of an animation at fraction T of its length, e.g. open:1). Needs numpy and Pillow."""
import json, math, os, struct, sys
import numpy as np
from PIL import Image, ImageDraw

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")
TEAM = np.array([0.25, 0.45, 0.95])


def quat_mat(q):
    x, y, z, w = q
    return np.array([[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
                     [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
                     [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]])


def trs(t, r, s=(1, 1, 1)):
    m = np.eye(4)
    m[:3, :3] = quat_mat(r) * np.array(s)
    m[:3, 3] = t
    return m


def load_glb(path, anim=None):
    data = open(path, "rb").read()
    jl = struct.unpack_from("<I", data, 12)[0]
    doc = json.loads(data[20:20 + jl])
    blob = data[20 + jl + 8:]

    def acc(i):
        a = doc["accessors"][i]
        v = doc["bufferViews"][a["bufferView"]]
        dt = {5126: "<f4", 5125: "<u4", 5123: "<u2", 5121: "u1"}[a["componentType"]]
        n = {"SCALAR": 1, "VEC2": 2, "VEC3": 3, "VEC4": 4}[a["type"]]
        arr = np.frombuffer(blob, dt, a["count"] * n, v.get("byteOffset", 0) + a.get("byteOffset", 0))
        return arr.reshape(a["count"], n) if n > 1 else arr

    nodes = doc["nodes"]
    pose = {}
    if anim:
        name, frac = anim
        for a in doc.get("animations", []):
            if a["name"] != name:
                continue
            for ch in a["channels"]:
                s = a["samplers"][ch["sampler"]]
                t = acc(s["input"])
                out = acc(s["output"])
                tt = t[0] + (t[-1] - t[0]) * frac
                k = min(int(np.searchsorted(t, tt, side="right")) - 1, len(t) - 1)
                k = max(k, 0)
                pose.setdefault(ch["target"]["node"], {})[ch["target"]["path"]] = out[k]
    tris = []

    def walk(i, parent):
        n = nodes[i]
        if "matrix" in n:
            local = np.array(n["matrix"]).reshape(4, 4).T
        else:
            p = pose.get(i, {})
            local = trs(p.get("translation", n.get("translation", [0, 0, 0])), p.get("rotation", n.get("rotation", [0, 0, 0, 1])),
                        n.get("scale", [1, 1, 1]))
        m = parent @ local
        if "mesh" in n:
            for prim in doc["meshes"][n["mesh"]]["primitives"]:
                pos = acc(prim["attributes"]["POSITION"]).astype(float)
                uv = acc(prim["attributes"]["TEXCOORD_0"]).astype(float) if "TEXCOORD_0" in prim["attributes"] else np.zeros((len(pos), 2))
                idx = acc(prim["indices"]).astype(int).reshape(-1, 3) if "indices" in prim else np.arange(len(pos)).reshape(-1, 3)
                mat = doc["materials"][prim["material"]] if "material" in prim else {}
                col = mat.get("pbrMetallicRoughness", {}).get("baseColorFactor")
                flat = mat.get("name", "").lower()
                w = (np.c_[pos, np.ones(len(pos))] @ m.T)[:, :3]
                tris.append((w[idx], uv[idx], flat, col))
        for c in n.get("children", []):
            walk(c, m)

    for r in doc["scenes"][doc.get("scene", 0)]["nodes"]:
        walk(r, np.eye(4))
    return tris


def render(tris, albedo, team, brightness, scale, size, yaw_deg, label=""):
    yaw = math.radians(yaw_deg)
    pitch = math.radians(32)
    # camera basis: looking from front-right-above; model forward is -Z
    fwd = np.array([-math.sin(yaw) * math.cos(pitch), -math.sin(pitch), math.cos(yaw) * math.cos(pitch)])
    fwd = -fwd
    right = np.cross(fwd, [0, 1, 0]); right /= np.linalg.norm(right)
    up = np.cross(right, fwd)
    sun = np.array([0.45, 0.8, 0.35]); sun /= np.linalg.norm(sun)
    allp = np.concatenate([t[0].reshape(-1, 3) for t in tris]) * scale if tris else np.zeros((1, 3))
    sx, sy = allp @ right, allp @ up
    ext = max(sx.max() - sx.min(), sy.max() - sy.min(), 1e-3) * 1.12
    cx, cy = (sx.max() + sx.min()) / 2, (sy.max() + sy.min()) / 2
    img = np.zeros((size, size, 3)) + np.array([0.62, 0.66, 0.7])
    zb = np.full((size, size), -1e9)
    alb = np.asarray(albedo, float)[..., :3] / 255 * brightness if albedo is not None else None
    tm = np.asarray(team, float)[..., 0] / 255 if team is not None else None
    for w, uv, flat, col in tris:
        w = w * scale
        n = np.cross(w[:, 1] - w[:, 0], w[:, 2] - w[:, 0])
        ln = np.linalg.norm(n, axis=1)
        ok = ln > 1e-12
        n[ok] /= ln[ok, None]
        px = ((w @ right) - cx) / ext * size + size / 2
        py = size / 2 - ((w @ up) - cy) / ext * size
        pz = w @ (-fwd)
        for k in np.nonzero(ok)[0]:
            if n[k] @ fwd < 0:  # back face
                continue
            xs, ys = px[k], py[k]
            x0, x1 = max(int(xs.min()), 0), min(int(xs.max()) + 1, size)
            y0, y1 = max(int(ys.min()), 0), min(int(ys.max()) + 1, size)
            if x0 >= x1 or y0 >= y1:
                continue
            gx, gy = np.meshgrid(np.arange(x0, x1) + 0.5, np.arange(y0, y1) + 0.5)
            d = (ys[1] - ys[2]) * (xs[0] - xs[2]) + (xs[2] - xs[1]) * (ys[0] - ys[2])
            if abs(d) < 1e-9:
                continue
            a = ((ys[1] - ys[2]) * (gx - xs[2]) + (xs[2] - xs[1]) * (gy - ys[2])) / d
            b = ((ys[2] - ys[0]) * (gx - xs[2]) + (xs[0] - xs[2]) * (gy - ys[2])) / d
            c = 1 - a - b
            inside = (a >= -1e-4) & (b >= -1e-4) & (c >= -1e-4)
            if not inside.any():
                continue
            z = a * pz[k, 0] + b * pz[k, 1] + c * pz[k, 2]
            sub = zb[y0:y1, x0:x1]
            vis = inside & (z > sub)
            if not vis.any():
                continue
            sub[vis] = z[vis]
            shade = 0.45 + 0.65 * max(float(n[k] @ sun), 0.0)
            if flat in ("team", "light", "dark", "metal") or alb is None:
                base = {"team": TEAM, "light": TEAM * 0.45 + 0.5, "dark": np.array([0.14, 0.14, 0.16]),
                        "metal": np.array([0.5, 0.52, 0.56])}.get(flat, np.array(col[:3]) if col else np.array([0.6, 0.6, 0.6]))
                img[y0:y1, x0:x1][vis] = np.clip(base * shade, 0, 1)
            else:
                u = a * uv[k, 0, 0] + b * uv[k, 1, 0] + c * uv[k, 2, 0]
                v = a * uv[k, 0, 1] + b * uv[k, 1, 1] + c * uv[k, 2, 1]
                H, W = alb.shape[:2]
                tx = np.clip((u % 1.0) * W, 0, W - 1).astype(int)
                ty = np.clip((v % 1.0) * H, 0, H - 1).astype(int)
                colr = alb[ty, tx]
                if tm is not None:
                    km = tm[ty, tx][..., None]
                    lum = colr.mean(-1, keepdims=True)
                    colr = colr + (TEAM * (0.45 + lum * 1.3) - colr) * km
                img[y0:y1, x0:x1][vis] = np.clip(colr * shade, 0, 1)[vis]
    out = Image.fromarray((img * 255).astype(np.uint8))
    if label:
        ImageDraw.Draw(out).text((4, 4), label, fill=(0, 0, 0))
    return out


def unit_models():
    res = {}
    fdir = os.path.join(ROOT, "data", "factions")
    for f in sorted(os.listdir(fdir)):
        if f.endswith(".json"):
            for u in json.load(open(os.path.join(fdir, f), encoding="utf-8"))["units"]:
                res[u["id"]] = u.get("model", {})
    return res


def box_tris(boxes):
    tris = []
    for b in boxes:
        p, s = np.array(b["pos"]), np.array(b["size"]) / 2
        col = b["color"]
        flat = col if isinstance(col, str) else ""
        rgb = None if isinstance(col, str) else list(col) + [1]
        c = np.array([[x, y, z] for x in (-1, 1) for y in (-1, 1) for z in (-1, 1)]) * s + p
        faces = [(0, 1, 3, 2), (4, 6, 7, 5), (0, 4, 5, 1), (2, 3, 7, 6), (0, 2, 6, 4), (1, 5, 7, 3)]
        for f in faces:
            for t in ((f[0], f[1], f[2]), (f[0], f[2], f[3])):
                tris.append((c[list(t)][None], np.zeros((1, 3, 2)), flat, rgb))
    return tris


def main():
    args = sys.argv[1:]
    out, size, yaw, anim, names, units = args[0], 320, 35.0, None, [], False
    i = 1
    allu = False
    mdir = os.path.join(ROOT, "models")
    while i < len(args):
        a = args[i]
        if a == "--size":
            size = int(args[i + 1]); i += 1
        elif a == "--yaw":
            yaw = float(args[i + 1]); i += 1
        elif a == "--anim":
            n, t = args[i + 1].split(":"); anim = (n, float(t)); i += 1
        elif a == "--dir":
            mdir = os.path.abspath(args[i + 1]); i += 1
        elif a == "--unit":
            units = True
        elif a == "--all-units":
            allu = True
        else:
            names.append(a)
        i += 1
    models = unit_models()
    if allu:
        names, units = list(models), True
    cells = []
    for n in names:
        if units:
            md = models[n]
        else:
            md = {"mesh": f"{mdir}/{n}.glb", "albedo": f"{mdir}/{n}_albedo.png", "team_mask": f"{mdir}/{n}_team.png", "scale": 1, "brightness": 1}
            if not os.path.exists(os.path.join(ROOT, md["albedo"])):
                md.pop("albedo")
        tris = load_glb(os.path.join(ROOT, md["mesh"]), anim) if md.get("mesh") else []
        tris += box_tris(md.get("boxes", []))
        alb = Image.open(os.path.join(ROOT, md["albedo"])).convert("RGB") if md.get("albedo") else None
        team = Image.open(os.path.join(ROOT, md["team_mask"])).convert("RGB") if md.get("team_mask") and os.path.exists(os.path.join(ROOT, md["team_mask"])) else None
        if alb is not None and team is not None and team.size != alb.size:
            team = None
        cells.append(render(tris, alb, team, md.get("brightness", 2.1), md.get("scale", 1), size, yaw, n))
    cols = min(len(cells), 6)
    rows = (len(cells) + cols - 1) // cols
    sheet = Image.new("RGB", (cols * size, rows * size), (40, 40, 40))
    for k, c in enumerate(cells):
        sheet.paste(c, ((k % cols) * size, (k // cols) * size))
    sheet.save(out)
    print(out, len(cells), "models")


if __name__ == "__main__":
    main()
