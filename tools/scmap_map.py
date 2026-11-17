"""Port a Supreme Commander: FA map (.scmap + _save.lua) to a Long Range map.

    .venv/bin/python tools/scmap_map.py "<FA maps>/adaptive_ivano_frankivsk.v0002" NAME [--title T]
        [--variants 4,6,8,10]

Writes assets/maps/NAME (the smallest variant) and NAME_<k>p for the others: same terrain, buildings, trees
and deposits, only the start positions differ (the best spread k of the map's starts). A check picture of each
goes to tools/out/<variant>_preview.png.

| SupCom | Long Range |
|---|---|
| heightmap (1 vertex per world unit) | heights at 2 units per cell: a tent average, but the lowest point where the window touches water, so narrow rivers stay unbroken |
| water elevation | the sea level `water` |
| stratum masks | ground layers by the stratum's texture name (`STRATA`); a stratum whose texture is rock and whose mask is thin lines (roads) becomes vector roads (thinned, traced, simplified) |
| props: trees, logs, rocks (`PROPS`) | trees and rocks, size from the blueprint name (`_s1/_s2/_s3`) |
| Mass markers | deposits (snapped to the cell centre); hydrocarbons are left out (no such resource here) |
| ARMY_n markers | start positions |
| units of NEUTRAL_CIVILIAN and the other non-player armies (`CIVS`) | neutral buildings: the large ones first where they fit (1 free cell around, none on roads, deposits or within `START_CLEAR` of a start), residential towers next, then a small house where a tower did not fit |
"""
import argparse, glob, itertools, json, math, os, re, sys
import numpy as np
from PIL import Image, ImageDraw

sys.path.insert(0, os.path.dirname(__file__))
import scmap

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CELL = 2.0
START_CLEAR = 40.0
# Texture name fragment -> ground layer kind (first match wins). "road" makes vector roads.
STRATA = [("farm", "meadow"), ("gravel", "gravel"), ("rock", "road"), ("dirt", "dirt"), ("sand", "sand"),
          ("mud", "mud"), ("snow", "snow"), ("grass", "forest_floor")]
# Prop blueprint name fragment -> feature kind.
PROPS = [("oak", "oak"), ("dc0", "birch"), ("pine", "pine"), ("fir", "fir"), ("log", "shrub"), ("bush", "shrub"),
         ("shrub", "shrub"), ("boulder", "boulder"), ("rock", "rock"), ("stone", "stone")]
# SupCom civilian unit -> our building (None: left out).
CIVS = {"uec1101": "residential_tower", "uec1201": "science_lab", "uec1301": "admin_building", "uec1401": "farm",
        "uec1501": "plant", "xec1301": "admin_hall", "xec1401": "high_command"}
SMALL = ["apartment2", "house2", "house3", "house4", "house5", "old_house2", "old_house3", "old_house4", "row_house",
         "corner_house1", "corner_house2", "house6", "house7"]


def lua_markers(text):
    out = []
    for m in re.finditer(r"\['([^']+)'\] = \{(.*?)\n\s*\},", text, re.S):
        body = m.group(2)
        t = re.search(r"\['type'\] = STRING\( '([^']*)' \)", body)
        p = re.search(r"\['position'\] = VECTOR3\( ([-\d.e]+), ([-\d.e]+), ([-\d.e]+) \)", body)
        if t and p:
            out.append((m.group(1), t.group(1), float(p.group(1)), float(p.group(3))))
    return out


def lua_units(text):
    """(army, type, x, z, yaw) of every pre-placed unit."""
    out = []
    armies = [(m.start(), m.group(1)) for m in re.finditer(r"\['(\w+)'\] = \s*\{\s*personality", text)]
    for i, (at, army) in enumerate(armies):
        end = armies[i + 1][0] if i + 1 < len(armies) else len(text)
        for u in re.finditer(r"type = '(\w+)',.*?Position = \{ ([-\d.e]+), ([-\d.e]+), ([-\d.e]+) \},\s*"
                             r"Orientation = \{ ([-\d.e]+), ([-\d.e]+), ([-\d.e]+) \}", text[at:end], re.S):
            out.append((army, u.group(1).lower(), float(u.group(2)), float(u.group(4)), float(u.group(6))))
    return out


def downsample(h, water):
    """(2n+1)^2 vertices at 1 unit -> (n+1)^2 at 2 units."""
    p = np.pad(h, 1, mode="edge")
    k = np.array([0.25, 0.5, 0.25])
    rows = p[:-2] * k[0] + p[1:-1] * k[1] + p[2:] * k[2]
    tent = rows[:, :-2] * k[0] + rows[:, 1:-1] * k[1] + rows[:, 2:] * k[2]
    mn = np.min([p[1 + dz:p.shape[0] - 1 + dz, 1 + dx:p.shape[1] - 1 + dx] for dz in (-1, 0, 1) for dx in (-1, 0, 1)], 0)
    out = np.where(mn < water, mn, tent) if water is not None else tent
    return out[::2, ::2].astype(np.float32)


def thin(img):
    """Zhang-Suen thinning of a boolean image."""
    a = np.pad(img.astype(np.uint8), 1)
    while True:
        changed = False
        for step in (0, 1):
            P = [a[1:-1, 1:-1], a[:-2, 1:-1], a[:-2, 2:], a[1:-1, 2:], a[2:, 2:], a[2:, 1:-1], a[2:, :-2], a[1:-1, :-2], a[:-2, :-2]]
            nb = sum(P[1:]).astype(np.int32)
            seq = P[1:] + [P[1]]
            trans = sum(((seq[i] == 0) & (seq[i + 1] == 1)).astype(np.int32) for i in range(8))
            if step == 0:
                c = (P[1] * P[3] * P[5] == 0) & (P[3] * P[5] * P[7] == 0)
            else:
                c = (P[1] * P[3] * P[7] == 0) & (P[1] * P[5] * P[7] == 0)
            kill = (P[0] == 1) & (nb >= 2) & (nb <= 6) & (trans == 1) & c
            if kill.any():
                a[1:-1, 1:-1][kill] = 0
                changed = True
        if not changed:
            return a[1:-1, 1:-1].astype(bool)


def trace(sk):
    """Skeleton pixels -> polylines between ends and junctions (pixel coordinates x, z)."""
    pts = set(zip(*np.nonzero(sk)))
    N = [(-1, -1), (-1, 0), (-1, 1), (0, -1), (0, 1), (1, -1), (1, 0), (1, 1)]

    def nbrs(p):
        return [(p[0] + a, p[1] + b) for a, b in N if (p[0] + a, p[1] + b) in pts]

    def near(p, q):
        return max(abs(p[0] - q[0]), abs(p[1] - q[1])) <= 1

    # Staircase corners: a pixel whose two 4-neighbours touch diagonally is redundant when every other
    # neighbour touches one of them; left in, each corner looks like a junction.
    changed = True
    while changed:
        changed = False
        for p in sorted(pts):
            for a, b in (((-1, 0), (0, 1)), ((0, 1), (1, 0)), ((1, 0), (0, -1)), ((0, -1), (-1, 0))):
                q1, q2 = (p[0] + a[0], p[1] + a[1]), (p[0] + b[0], p[1] + b[1])
                if q1 in pts and q2 in pts and all(near(o, q1) or near(o, q2) for o in nbrs(p)):
                    pts.discard(p)
                    changed = True
                    break

    nodes = {p for p in pts if len(nbrs(p)) != 2}
    used = set()
    lines = []

    def walk(a, b):
        line = [a, b]
        prev, cur = a, b
        while cur not in nodes:
            nx = [q for q in nbrs(cur) if q != prev and (min(cur, q), max(cur, q)) not in used]
            if not nx:
                break
            used.add((min(cur, nx[0]), max(cur, nx[0])))
            prev, cur = cur, nx[0]
            line.append(cur)
        return line

    for a in sorted(nodes):
        for b in nbrs(a):
            e = (min(a, b), max(a, b))
            if e in used:
                continue
            used.add(e)
            lines.append(walk(a, b))
    # Closed loops without any node.
    for p in sorted(pts):
        for b in nbrs(p):
            e = (min(p, b), max(p, b))
            if e not in used:
                used.add(e)
                nodes.add(p)
                lines.append(walk(p, b))
    # Short spurs (a free end) are thinning noise; short pieces between junctions stay.
    deg = {p: len(nbrs(p)) for p in nodes}
    return [[(c + 0.5, r + 0.5) for r, c in ln] for ln in lines
            if len(ln) >= 6 or (deg.get(ln[0], 2) != 1 and deg.get(ln[-1], 2) != 1)]


def simplify(pts, tol):
    if len(pts) < 3:
        return pts
    a, b = np.array(pts[0]), np.array(pts[-1])
    d = b - a
    L = math.hypot(*d)
    best, bi = 0.0, 0
    for i in range(1, len(pts) - 1):
        p = np.array(pts[i]) - a
        dist = abs(d[0] * p[1] - d[1] * p[0]) / L if L > 1e-9 else math.hypot(*p)
        if dist > best:
            best, bi = dist, i
    if best <= tol:
        return [pts[0], pts[-1]]
    return simplify(pts[:bi + 1], tol)[:-1] + simplify(pts[bi:], tol)


def pick_starts(starts, k, size):
    """The k starts with the largest smallest distance between them; ties: the most even spacing."""
    S = np.array(starts)
    D = np.sqrt(((S[:, None] - S[None]) ** 2).sum(-1))
    best, choice = None, None
    for c in itertools.combinations(range(len(S)), k):
        sub = D[np.ix_(c, c)] + np.eye(k) * 1e9
        nn = sub.min(1)
        key = (round(float(nn.min()), 1), -float(nn.std()))
        if best is None or key > best:
            best, choice = key, c
    return list(choice)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("src")
    ap.add_argument("name")
    ap.add_argument("--title", default="")
    ap.add_argument("--variants", default="4,6,8,10")
    ap.add_argument("--size", type=int, default=0, help="cut a square of this many world units out of the map")
    ap.add_argument("--origin", default="", help="its top-left corner 'x,z' (default: centred on the town)")
    a = ap.parse_args()
    src = a.src
    sc_file = glob.glob(os.path.join(src, "*.scmap"))[0]
    save = open(glob.glob(os.path.join(src, "*_save.lua"))[0], encoding="latin1").read()
    m = scmap.Map(sc_file)
    units = lua_units(save)
    marks = lua_markers(save)
    ox = oz = 0
    if a.size:
        if a.origin:
            ox, oz = (int(v) for v in a.origin.split(","))
        else:
            # Centre on the residential buildings (the town core), not on farms spread over the map.
            res = np.array([(x, z) for _, t, x, z, _ in units if t == "uec1101"] or [(x, z) for _, t, x, z, _ in units if t in CIVS])
            cx, cz = np.median(res, 0)
            ox, oz = (int(round((v - a.size / 2) / 2)) * 2 for v in (cx, cz))
        full = int(m.size[0])
        ox, oz = (max(0, min(full - a.size, v)) for v in (ox, oz))
        m.heights = m.heights[oz:oz + a.size + 1, ox:ox + a.size + 1]
        for attr in ("masks_low", "masks_high"):
            mk = getattr(m, attr)
            q = mk.shape[0] / full
            setattr(m, attr, mk[int(oz * q):int((oz + a.size) * q), int(ox * q):int((ox + a.size) * q)])
        for p in m.props:
            p["pos"] = (p["pos"][0] - ox, p["pos"][1], p["pos"][2] - oz)
        inside = lambda x, z: 0 <= x < a.size and 0 <= z < a.size
        units = [(ar, t, x - ox, z - oz, y) for ar, t, x, z, y in units if inside(x - ox, z - oz)]
        marks = [(nm, t, x - ox, z - oz) for nm, t, x, z in marks if inside(x - ox, z - oz)]
        m.size = (a.size, a.size)
        print(f"cut {a.size}x{a.size} at {ox},{oz}")
    size = int(m.size[0])
    n = size // 2
    water = m.water
    print(f"{os.path.basename(sc_file)}: {size} units, water {water}, {len(m.props)} props")

    heights = downsample(m.heights, water)
    assert heights.shape == (n + 1, n + 1)
    wl = water if water is not None else float(heights.min()) - 2.0
    corners = np.stack([heights[:-1, :-1], heights[:-1, 1:], heights[1:, :-1], heights[1:, 1:]])
    blocked = ((corners.max(0) - corners.min(0)) / CELL > 0.85) | (corners.mean(0) < wl - 0.3)
    print(f"blocked cells {blocked.mean() * 100:.1f}%")

    # ------------------------------------------------------------ ground layers and roads
    masks = np.concatenate([m.masks_low, m.masks_high], -1).astype(np.float32) / 255.0  # strata 1..8
    layers = [("grass", None)]
    road_lines = []
    road_mask = None
    for s in range(8):
        tex = m.albedo[s + 1][0].lower()
        mk = masks[..., s]
        if not tex or mk.max() < 0.2:
            continue
        name = os.path.basename(tex)
        kind = next((k for frag, k in STRATA if frag in name), None)
        if kind is None:
            print("  stratum", s + 1, name, "left out")
            continue
        if kind == "road":
            road_mask = mk
            continue
        q = mk.shape[0] // n
        cells = mk[:n * q, :n * q].reshape(n, q, n, q).mean((1, 3))
        print(f"  stratum {s + 1} {name} -> {kind} (cover {cells.mean():.2f})")
        layers.append((kind, cells))
    if road_mask is not None:
        scale = size / road_mask.shape[0]
        on = road_mask > 0.5
        sk = thin(on)
        width = max(3.0, on.sum() / max(1, sk.sum()) * scale * 1.3)
        for ln in trace(sk):
            pts = simplify([(x * scale, z * scale) for x, z in ln], 0.6 * scale)
            road_lines.append(("asphalt", width, pts))
        print(f"  roads: {len(road_lines)} lines, width {width:.1f}")

    # ------------------------------------------------------------ markers
    deposits = []
    for name, typ, x, z in marks:
        if typ == "Mass":
            d = ((math.floor(x / CELL) + 0.5) * CELL, (math.floor(z / CELL) + 0.5) * CELL)
            if d not in deposits:
                deposits.append(d)
    deposits.sort()
    starts_all = sorted(((int(nm.split("_")[1]), x, z) for nm, typ, x, z in marks if re.fullmatch(r"ARMY_\d+", nm)))
    army_of = [k for k, _, _ in starts_all]
    starts_all = [(x, z) for _, x, z in starts_all]
    print(f"{len(deposits)} deposits, {len(starts_all)} starts, "
          f"{sum(1 for _, t, _, _ in marks if t == 'Hydrocarbon')} hydrocarbons left out")

    # ------------------------------------------------------------ trees and rocks
    fdefs = {f["id"]: f for f in json.load(open(os.path.join(ROOT, "assets/data/rules.json"), encoding="utf-8"))["features"]}
    feats = []
    skipped = {}
    for p in m.props:
        x, y, z = p["pos"]
        if y < -100 or not (0 < x < size and 0 < z < size):
            continue
        name = os.path.basename(p["path"]).lower()
        kind = next((k for frag, k in PROPS if frag in name), None)
        if kind is None:
            skipped[name] = skipped.get(name, 0) + 1
            continue
        sz = {"_s1": 1.0, "_s2": 1.15, "_s3": 0.85}
        sc = next((v for k_, v in sz.items() if k_ in name), 1.0)
        h = (hash(round(x * 37 + z * 101, 2)) % 1000) / 1000.0
        if kind == "rock":
            kind = "stone" if h < 0.55 else ("rock" if h < 0.93 else "boulder")
        sc *= 0.9 + 0.2 * ((int(x * 131 + z * 17) % 100) / 100.0)
        yaw = math.atan2(p["rx"][2], p["rx"][0])
        feats.append(f"{kind} {x:.2f} {z:.2f} {yaw:.2f} {sc:.2f}")
    if skipped:
        print("props left out:", skipped)
    print(f"{len(feats)} trees and rocks")

    # ------------------------------------------------------------ buildings
    civ = json.load(open(os.path.join(ROOT, "assets/data/factions/civilian.json"), encoding="utf-8"))
    foot = {u["id"]: max(1, u.get("footprint", 1)) for u in civ["units"]}
    occ = blocked.astype(np.uint8)
    for x, z in deposits:
        i, j = int(x / CELL), int(z / CELL)
        occ[max(0, j - 1):j + 2, max(0, i - 1):i + 2] = 1
    roadc = np.zeros((n, n), bool)
    if road_mask is not None:
        q = road_mask.shape[0] // n
        roadc = (road_mask[:n * q, :n * q] > 0.5).reshape(n, q, n, q).sum((1, 3)) >= 2
    props = []

    def snap(kind, x, z):
        if foot[kind] % 2 == 1:
            return (math.floor(x / CELL) + 0.5) * CELL, (math.floor(z / CELL) + 0.5) * CELL
        return round(x / CELL) * CELL, round(z / CELL) * CELL

    def place(kind, x, z, yaw, nudge):
        x, z = snap(kind, x, z)
        f = foot[kind]
        for dx, dz in sorted(((dx, dz) for dx in range(-nudge, nudge + 1) for dz in range(-nudge, nudge + 1)),
                             key=lambda d: d[0] ** 2 + d[1] ** 2):
            px, pz = x + dx * CELL, z + dz * CELL
            x0, z0 = int(round(px / CELL - f / 2)), int(round(pz / CELL - f / 2))
            if x0 < 2 or z0 < 2 or x0 + f > n - 2 or z0 + f > n - 2:
                continue
            if occ[z0 - 1:z0 + f + 1, x0 - 1:x0 + f + 1].any() or roadc[z0:z0 + f, x0:x0 + f].any():
                continue
            if any(math.hypot(px - sx, pz - sz) < START_CLEAR + f for sx, sz in starts_all):
                continue
            occ[z0:z0 + f, x0:x0 + f] = 2
            props.append({"def": kind, "pos": {"x": round(px, 2), "y": round(pz, 2)}, "yaw": round(yaw, 4), "owner": 0})
            return True
        return False

    civs = [u for u in units if u[1] in CIVS]
    others = {}
    for u in units:
        if u[1] not in CIVS:
            others[u[1]] = others.get(u[1], 0) + 1
    print(f"{len(civs)} civilian buildings; left out: {others}")
    civs.sort(key=lambda u: (-foot[CIVS[u[1]]], u[1] == "uec1101", u[3], u[2]))
    missed = []
    for army, typ, x, z, ry in civs:
        yaw = math.pi / 2 - ry
        if not place(CIVS[typ], x, z, yaw, 1):
            missed.append((typ, x, z, yaw))
    big = len(props)
    rng = np.random.default_rng(7)
    for typ, x, z, yaw in missed:
        if typ == "uec1101":
            place(SMALL[rng.integers(len(SMALL))], x, z, yaw, 1)
    print(f"buildings: {big} stock ones, {len(props) - big} small houses in the gaps, {len(civs) - len(props)} dropped")

    # ------------------------------------------------------------ write the variants
    mn, mx = float(heights.min()), float(heights.max())
    px16 = np.round((heights - mn) / (mx - mn) * 65535).astype(np.uint16)
    ks = [int(k) for k in a.variants.split(",")]
    title = a.title or a.name
    for k in ks:
        chosen = pick_starts(starts_all, k, size)
        starts = [starts_all[i] for i in chosen]
        vname = a.name if k == min(ks) else f"{a.name}_{k}p"
        out = os.path.join(ROOT, "assets", "maps", vname)
        os.makedirs(out, exist_ok=True)
        for i in range(1, 9):
            f_ = os.path.join(out, f"layer{i}.png")
            if i < len(layers):
                Image.fromarray(np.clip(np.round(layers[i][1] * 255), 0, 255).astype(np.uint8)).save(f_)
            elif os.path.exists(f_):
                os.remove(f_)
        Image.fromarray(px16).save(os.path.join(out, "height.png"))
        mj = {
            "name": title if k == min(ks) else f"{title} ({k} players)",
            "n": n,
            "water": round(wl, 3),
            "min_height": mn,
            "max_height": mx,
            "deposits": [{"x": d[0], "y": d[1]} for d in deposits],
            "starts": [{"x": s[0], "y": s[1]} for s in starts],
            "props": props,
            "seed": 1,
            "features": feats,
            "layers": [{"kind": kd, "scale": 0.0} for kd, _ in layers],
            "roads": [f"{kd} {w:.2f} " + " ".join(f"{x:.2f} {z:.2f}" for x, z in pts) for kd, w, pts in road_lines],
        }
        with open(os.path.join(out, "map.json"), "w", encoding="utf-8") as f:
            json.dump(mj, f, indent=2)
        D = [min(math.hypot(s[0] - t[0], s[1] - t[1]) for t in starts if t != s) for s in starts]
        print(f"wrote {out}: starts ARMY_{[army_of[i] for i in chosen]}, nearest neighbour {min(D):.0f}-{max(D):.0f}")
        preview(vname, heights, wl, road_lines, feats, fdefs, props, foot, deposits, starts)


def preview(vname, heights, wl, road_lines, feats, fdefs, props, foot, deposits, starts):
    n = heights.shape[0] - 1
    S = 2
    mn, mx = float(heights.min()), float(heights.max())
    shade = np.zeros_like(heights)
    shade[1:, 1:] = (heights[1:, 1:] - heights[:-1, :-1]) * 1.5
    base = 0.55 + 0.45 * (heights - mn) / (mx - mn)
    rgb = np.stack([base * 150, base * 170, base * 120], -1) * np.clip(1 - shade[..., None] * 0.35, 0.5, 1.4)
    img = Image.fromarray(np.clip(rgb, 0, 255).astype(np.uint8)).resize(((n + 1) * S, (n + 1) * S))
    img.paste((40, 90, 200), mask=Image.fromarray(((heights < wl) * 255).astype(np.uint8)).resize(img.size))
    d = ImageDraw.Draw(img)
    k = S / CELL
    for kind, width, pts in road_lines:
        d.line([(x * k, z * k) for x, z in pts], fill=(60, 60, 60), width=max(1, int(width * k)))
    for f in feats:
        kind, x, z = f.split()[:3]
        x, z = float(x) * k, float(z) * k
        d.point((x, z), fill=(235, 235, 225) if fdefs[kind]["class"] == "rock" else (25, 70, 30))
    for p in props:
        x, z = p["pos"]["x"] * k, p["pos"]["y"] * k
        h = foot[p["def"]] * CELL * k / 2
        d.rectangle([x - h, z - h, x + h, z + h], fill=(200, 40, 40))
    for x, z in deposits:
        d.ellipse([x * k - 2, z * k - 2, x * k + 2, z * k + 2], fill=(40, 220, 255))
    for i, (x, z) in enumerate(starts):
        d.ellipse([x * k - 8, z * k - 8, x * k + 8, z * k + 8], outline=(255, 255, 0), width=3)
        d.text((x * k + 10, z * k - 6), str(i + 1), fill=(255, 255, 0))
    os.makedirs(os.path.join(ROOT, "tools", "out"), exist_ok=True)
    img.save(os.path.join(ROOT, "tools", "out", f"{vname}_preview.png"))


if __name__ == "__main__":
    main()
