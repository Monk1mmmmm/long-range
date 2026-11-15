#!/usr/bin/env python3
"""Make a game map of a real place from open data.

    .venv/bin/python tools/osm_map.py NAME LAT LON [--km 10] [--cells 384] [--players 4]
        [--relief 0.85] [--starts "x,z;x,z;..."] [--seed 1] [--title "Shown name"]

Elevation: AWS Terrain Tiles (Mapzen "terrarium" PNGs, zoom 13, ~12 m per pixel).
Everything else: OpenStreetMap through the Overpass API (buildings, forests and their leaf type,
scrub, parks, water, rocks, towers, power pylons, churches).
Downloads are cached in tools/out/osm_cache. Output: assets/maps/NAME/{map.json,height.png} and a
picture tools/out/NAME_preview.png (terrain shading, forests, buildings, deposits, starts).

The square is KM kilometres wide around LAT/LON; north is up (-z). Heights are true scale times
--relief. One prop is placed per real building where the game building (2-3x the size of a real
one) still fits; trees follow the forest outlines; rocks come from OSM rocks plus steep forest
slopes. Start positions are picked in open, flat ground (or given in world units with --starts),
levelled, and get deposits at the same offsets relative to the map centre, so the expansions are
roughly fair even though the land is not.

Map data (c) OpenStreetMap contributors (ODbL); elevation: Mapzen / AWS Terrain Tiles.
"""
import argparse
import io
import json
import math
import os
import random
import sys
import urllib.parse
import urllib.request

import numpy as np
from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CACHE = os.path.join(ROOT, "tools", "out", "osm_cache")
UA = "lr-mapgen/1.0"
CELL = 2.0
Z = 13


def fetch(url, name, data=None):
    os.makedirs(CACHE, exist_ok=True)
    path = os.path.join(CACHE, name)
    if os.path.exists(path):
        with open(path, "rb") as f:
            return f.read()
    body = urllib.parse.urlencode({"data": data}).encode() if data else None
    req = urllib.request.Request(url, data=body, headers={"User-Agent": UA})
    with urllib.request.urlopen(req, timeout=300) as r:
        raw = r.read()
    with open(path, "wb") as f:
        f.write(raw)
    return raw


# ---------------------------------------------------------------- geography

class Geo:
    def __init__(self, lat, lon, km, world):
        self.lat0, self.lon0 = lat, lon
        self.m = km * 1000.0
        self.world = world
        self.mx = 111320.0 * math.cos(math.radians(lat))  # metres per degree of longitude
        self.my = 110574.0
        self.s = lat - self.m / 2 / self.my
        self.n = lat + self.m / 2 / self.my
        self.w = lon - self.m / 2 / self.mx
        self.e = lon + self.m / 2 / self.mx
        self.upm = world / self.m  # world units per metre

    def to_world(self, lat, lon):
        x = (lon - self.w) * self.mx * self.upm
        z = (self.n - lat) * self.my * self.upm
        return x, z

    def to_latlon(self, x, z):
        return self.n - z / self.upm / self.my, self.w + x / self.upm / self.mx


def tile_xy(lat, lon, z):
    n = 2 ** z
    x = (lon + 180.0) / 360.0 * n
    y = (1.0 - math.asinh(math.tan(math.radians(lat))) / math.pi) / 2.0 * n
    return x, y


def elevation_grid(geo, n):
    """Heights in metres at the (n+1)^2 vertices."""
    x0, y0 = tile_xy(geo.n, geo.w, Z)
    x1, y1 = tile_xy(geo.s, geo.e, Z)
    tx0, ty0, tx1, ty1 = int(x0), int(y0), int(x1), int(y1)
    mos = np.zeros(((ty1 - ty0 + 1) * 256, (tx1 - tx0 + 1) * 256), np.float64)
    for ty in range(ty0, ty1 + 1):
        for tx in range(tx0, tx1 + 1):
            raw = fetch(f"https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{Z}/{tx}/{ty}.png", f"terrarium_{Z}_{tx}_{ty}.png")
            a = np.asarray(Image.open(io.BytesIO(raw)).convert("RGB")).astype(np.float64)
            h = a[..., 0] * 256.0 + a[..., 1] + a[..., 2] / 256.0 - 32768.0
            mos[(ty - ty0) * 256:(ty - ty0 + 1) * 256, (tx - tx0) * 256:(tx - tx0 + 1) * 256] = h
    side = n + 1
    out = np.zeros((side, side))
    step = geo.world / n
    for j in range(side):
        lat, _ = geo.to_latlon(0, j * step)
        for i in range(side):
            _, lon = geo.to_latlon(i * step, 0)
            px, py = tile_xy(lat, lon, Z)
            px = (px - tx0) * 256 - 0.5
            py = (py - ty0) * 256 - 0.5
            ix, iy = int(math.floor(px)), int(math.floor(py))
            fx, fy = px - ix, py - iy
            h00, h10 = mos[iy, ix], mos[iy, ix + 1]
            h01, h11 = mos[iy + 1, ix], mos[iy + 1, ix + 1]
            out[j, i] = (h00 * (1 - fx) + h10 * fx) * (1 - fy) + (h01 * (1 - fx) + h11 * fx) * fy
    return out


def overpass(geo, name):
    bb = f"{geo.s:.5f},{geo.w:.5f},{geo.n:.5f},{geo.e:.5f}"
    q = f"""[out:json][timeout:300];
(
  way["building"]({bb});
  way["landuse"~"^(forest|residential|industrial|commercial|retail|farmyard|cemetery|quarry|reservoir|basin)$"]({bb});
  relation["landuse"~"^(forest|residential|industrial|reservoir)$"]({bb});
  way["natural"~"^(wood|scrub|water|rock|bare_rock|scree|heath)$"]({bb});
  relation["natural"~"^(wood|water|scrub)$"]({bb});
  way["leisure"="park"]({bb});
  way["waterway"="riverbank"]({bb});
  node["natural"~"^(rock|stone|tree|peak)$"]({bb});
  node["man_made"~"^(water_tower|tower|mast)$"]({bb});
  way["man_made"~"^(water_tower|tower)$"]({bb});
  node["power"="tower"]({bb});
);
out geom;"""
    raw = fetch("https://overpass-api.de/api/interpreter", f"overpass_{name}.json", data=q)
    return json.loads(raw)["elements"]


def join_rings(segs):
    """Joins open way segments of a multipolygon into closed rings."""
    rings, open_ = [], [list(s) for s in segs if len(s) >= 2]
    while open_:
        cur = open_.pop()
        changed = True
        while cur[0] != cur[-1] and changed:
            changed = False
            for k, s in enumerate(open_):
                if s[0] == cur[-1]:
                    cur += s[1:]
                elif s[-1] == cur[-1]:
                    cur += s[::-1][1:]
                elif s[-1] == cur[0]:
                    cur = s[:-1] + cur
                elif s[0] == cur[0]:
                    cur = s[::-1][:-1] + cur
                else:
                    continue
                open_.pop(k)
                changed = True
                break
        if len(cur) >= 3:
            rings.append(cur)
    return rings


def shapes(el, geo):
    """(outer rings, inner rings) in world coordinates for a way or multipolygon relation."""
    conv = lambda g: [geo.to_world(p["lat"], p["lon"]) for p in g]
    if el["type"] == "way" and "geometry" in el:
        return [conv(el["geometry"])], []
    if el["type"] == "relation":
        outer = [conv(m["geometry"]) for m in el.get("members", []) if m.get("role") in ("outer", "") and "geometry" in m]
        inner = [conv(m["geometry"]) for m in el.get("members", []) if m.get("role") == "inner" and "geometry" in m]
        return [tuple(map(tuple, r)) for r in join_rings([[tuple(p) for p in r] for r in outer])], [tuple(map(tuple, r)) for r in join_rings([[tuple(p) for p in r] for r in inner])]
    return [], []


class Raster:
    """Boolean layers at RES pixels per world unit."""

    RES = 2

    def __init__(self, world):
        self.size = int(world * self.RES)
        self.layers = {}

    def img(self, key):
        if key not in self.layers:
            self.layers[key] = Image.new("L", (self.size, self.size), 0)
        return self.layers[key]

    def poly(self, key, outer, inner):
        im = self.img(key)
        d = ImageDraw.Draw(im)
        for r in outer:
            if len(r) >= 3:
                d.polygon([(x * self.RES, z * self.RES) for x, z in r], fill=255)
        for r in inner:
            if len(r) >= 3:
                d.polygon([(x * self.RES, z * self.RES) for x, z in r], fill=0)

    def arr(self, key):
        if key not in self.layers:
            return np.zeros((self.size, self.size), bool)
        return np.asarray(self.layers[key]) > 127

    @staticmethod
    def at(a, x, z):
        i, j = int(x * Raster.RES), int(z * Raster.RES)
        return 0 <= i < a.shape[1] and 0 <= j < a.shape[0] and a[j, i]


def blur(a, r):
    """Box blur (twice: close to a Gaussian) without scipy."""
    a = a.astype(np.float64)
    for _ in range(2):
        for axis in (0, 1):
            c = np.cumsum(np.pad(a, [(r + 1, r) if k == axis else (0, 0) for k in range(2)], mode="edge"), axis=axis)
            hi = np.take(c, np.arange(2 * r + 1, c.shape[axis]), axis=axis)
            lo = np.take(c, np.arange(0, c.shape[axis] - 2 * r - 1), axis=axis)
            a = (hi - lo) / (2 * r + 1)
    return a


# ---------------------------------------------------------------- buildings

HOUSES = ["house", "house2", "house3", "house4", "house5", "house6", "house7", "house8", "house9"]
OLD = ["old_house2", "old_house3", "old_house4", "row_house"]
CORNER = ["corner_house1", "corner_house2", "corner_house3", "corner_house4", "row_house"]


def building_kind(tags, area, landuse, dense, rng):
    """Game building for an OSM building of `area` m^2, or None (sheds, garages)."""
    b = tags.get("building", "yes")
    if b in ("church", "chapel", "cathedral") or tags.get("amenity") == "place_of_worship":
        return "church" if area > 120 else None
    if b in ("garage", "garages", "shed", "carport", "hut", "roof", "kiosk", "toilets", "greenhouse", "container", "bunker", "ruins", "construction", "transformer_tower", "service") or area < 70:
        return None
    if tags.get("man_made") == "water_tower" or b == "water_tower":
        return "water_tower"
    if b in ("hospital", "university", "college"):
        return "science_lab"
    if b in ("industrial", "warehouse", "manufacture", "factory", "hangar") or landuse == "industrial":
        if area > 4000:
            return rng.choice(["warehouse", "warehouse", "plant", "admin_hall", "refinery"])
        if area > 1200:
            return rng.choice(["warehouse", "plant", "containers", "containers2"])
        if area > 300:
            return rng.choice(["containers", "containers2", "fuel_tank", "plant"])
        return None
    if b in ("farm_auxiliary", "barn", "agricultural", "farm", "stable", "cowshed", "sty") or landuse == "farmyard":
        return "farm" if area > 600 else rng.choice(OLD) if area > 150 else None
    if b in ("school", "public", "civic", "government", "office", "townhall", "kindergarten", "train_station", "transportation", "sports_hall", "hotel"):
        return "admin_hall" if area > 2500 else "admin_building" if area > 400 else "grocery"
    if b in ("retail", "commercial", "supermarket"):
        return "warehouse" if area > 3000 else "admin_building" if area > 800 else "grocery"
    if b in ("apartments", "residential", "dormitory") or area > 350:
        if area > 1100:
            return rng.choice(["residential_tower", "apartment", "apartment"])
        if area > 350:
            return rng.choice(["apartment", "apartment2"]) if not dense else rng.choice(["apartment", "apartment2"] + CORNER)
        return rng.choice(CORNER) if dense else rng.choice(HOUSES + OLD)
    if dense:
        return rng.choice(CORNER + ["apartment2"])
    return rng.choice(HOUSES + HOUSES + OLD)


def orient(ring):
    """Yaw (radians, world x/z) of a polygon's longest edge, and its area in world units^2."""
    best, yaw, area = 0.0, 0.0, 0.0
    for (x0, z0), (x1, z1) in zip(ring, ring[1:] + ring[:1]):
        l2 = (x1 - x0) ** 2 + (z1 - z0) ** 2
        if l2 > best:
            best, yaw = l2, math.atan2(z1 - z0, x1 - x0)
        area += x0 * z1 - x1 * z0
    return yaw, abs(area) * 0.5


# ---------------------------------------------------------------- main

def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("name")
    ap.add_argument("lat", type=float)
    ap.add_argument("lon", type=float)
    ap.add_argument("--km", type=float, default=10.0)
    ap.add_argument("--cells", type=int, default=384)
    ap.add_argument("--players", type=int, default=4)
    ap.add_argument("--relief", type=float, default=0.85, help="vertical scale against true scale")
    ap.add_argument("--starts", default="", help="'x,z;x,z;...' in world units (default: picked automatically)")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--title", default="")
    ap.add_argument("--max-props", type=int, default=700)
    a = ap.parse_args()

    n = a.cells
    world = n * CELL
    geo = Geo(a.lat, a.lon, a.km, world)
    rng = random.Random(a.seed)
    print(f"area lat {geo.s:.4f}..{geo.n:.4f} lon {geo.w:.4f}..{geo.e:.4f}, {1 / geo.upm:.1f} m per world unit")

    # Heights.
    elev = elevation_grid(geo, n)
    print(f"elevation {elev.min():.0f}..{elev.max():.0f} m")
    heights = (elev - elev.min()) * geo.upm * a.relief + 1.0

    # OSM layers.
    els = overpass(geo, a.name)
    ras = Raster(world)
    buildings, points = [], []
    for el in els:
        t = el.get("tags", {})
        if el["type"] == "node":
            points.append((t, geo.to_world(el["lat"], el["lon"])))
            continue
        outer, inner = shapes(el, geo)
        if not outer:
            continue
        if "building" in t:
            buildings.append((t, outer[0]))
            ras.poly("building", outer, inner)
            continue
        lu, nat = t.get("landuse"), t.get("natural")
        if lu == "forest" or nat == "wood":
            leaf = t.get("leaf_type", "mixed")
            ras.poly({"needleleaved": "needle", "broadleaved": "broad"}.get(leaf, "mixed"), outer, inner)
        elif nat == "scrub" or nat == "heath":
            ras.poly("scrub", outer, inner)
        elif nat == "water" or lu in ("reservoir", "basin") or t.get("waterway") == "riverbank":
            ras.poly("water", outer, inner)
        elif nat in ("bare_rock", "scree", "rock"):
            ras.poly("rock", outer, inner)
        elif t.get("leisure") == "park" or lu == "cemetery":
            ras.poly("park", outer, inner)
        elif lu in ("industrial", "residential", "commercial", "retail", "farmyard", "quarry"):
            ras.poly(lu, outer, inner)
    print(f"{len(buildings)} buildings, {len(points)} points of interest")

    needle, broad, mixed = ras.arr("needle"), ras.arr("broad"), ras.arr("mixed")
    forest = needle | broad | mixed
    water = ras.arr("water")
    built = ras.arr("building")
    # Built-up density over ~60 m: dense blocks get town houses instead of detached ones.
    dens = blur(built, int(60 * geo.upm * Raster.RES / 2) + 1)
    print(f"forest {forest.mean() * 100:.0f}% of the map, water {water.mean() * 100:.1f}%")

    # Start positions: open (no forest, buildings, water), flat ground, one per sector around the centre.
    rs = Raster.RES
    open_ = blur(forest | water | built | ras.arr("industrial") | ras.arr("residential"), int(30 * rs))
    hv = heights
    slope = np.zeros((n + 1, n + 1))
    slope[:-1, :-1] = np.abs(np.diff(hv, axis=0))[:, :-1] + np.abs(np.diff(hv, axis=1))[:-1, :]
    slope_b = blur(slope, 12)
    c = world / 2
    if a.starts:
        starts = [tuple(map(float, s.split(","))) for s in a.starts.split(";")]
    else:
        starts = []
        k = a.players
        for p in range(k):
            ang = -3 * math.pi / 4 + p * 2 * math.pi / k
            best = None
            for rad in np.arange(world * 0.26, world * 0.44, 6.0):
                for da in np.linspace(-math.pi / k * 0.6, math.pi / k * 0.6, 25):
                    x, z = c + math.cos(ang + da) * rad, c + math.sin(ang + da) * rad
                    if not (70 < x < world - 70 and 70 < z < world - 70):
                        continue
                    o = open_[int(z * rs), int(x * rs)]
                    s = slope_b[int(z / CELL), int(x / CELL)]
                    score = o * 3.0 + s * 4.0 + abs(rad - world * 0.36) / world * 2.0 + abs(da) * 0.3
                    if best is None or score < best[0]:
                        best = (score, x, z)
            starts.append((round(best[1], 1), round(best[2], 1)))
    print("starts", starts)

    # Level the bases: blend to the mean height within 26 units, fading out by 46.
    for sx, sz in starts:
        ys, xs = np.mgrid[0:n + 1, 0:n + 1] * CELL
        d = np.hypot(xs - sx, ys - sz)
        mean = heights[d < 26].mean()
        t = np.clip((46 - d) / 20, 0, 1)
        t = t * t * (3 - 2 * t)
        heights = heights * (1 - t) + mean * t

    def h_at(x, z):
        fx, fz = min(max(x / CELL, 0), n - 0.001), min(max(z / CELL, 0), n - 0.001)
        i, j = int(fx), int(fz)
        u, v = fx - i, fz - j
        return (heights[j, i] * (1 - u) + heights[j, i + 1] * u) * (1 - v) + (heights[j + 1, i] * (1 - u) + heights[j + 1, i + 1] * u) * v

    # Cells too steep to drive on (same rule as map.rs::cell_blocked).
    q = np.stack([heights[:-1, :-1], heights[:-1, 1:], heights[1:, :-1], heights[1:, 1:]])
    blocked = (q.max(0) - q.min(0)) / CELL > 0.85
    print(f"blocked cells {blocked.mean() * 100:.1f}%")

    # ---------------------------------------------------------------- props
    civ = json.load(open(os.path.join(ROOT, "assets/data/factions/civilian.json"), encoding="utf-8"))
    foot = {u["id"]: max(1, u.get("footprint", 1)) for u in civ["units"]}
    occ = np.zeros((n, n), np.uint8)
    occ[blocked] = 1
    wcell = blur(water, rs) > 0.3
    for j in range(n):
        for i in range(n):
            if wcell[int((j + 0.5) * CELL * rs), int((i + 0.5) * CELL * rs)]:
                occ[j, i] = 1
    props = []

    def snap(kind, x, z):
        f = foot[kind]
        if f % 2 == 1:
            return (math.floor(x / CELL) + 0.5) * CELL, (math.floor(z / CELL) + 0.5) * CELL
        return round(x / CELL) * CELL, round(z / CELL) * CELL

    def place(kind, x, z, yaw, owner=0, margin=1, near_start=60.0):
        if kind not in foot:
            return False
        x, z = snap(kind, x, z)
        f = foot[kind]
        x0, z0 = int(round(x / CELL - f / 2)), int(round(z / CELL - f / 2))
        if x0 - margin < 1 or z0 - margin < 1 or x0 + f + margin > n - 1 or z0 + f + margin > n - 1:
            return False
        if occ[z0 - margin:z0 + f + margin, x0 - margin:x0 + f + margin].any():
            return False
        if any(math.hypot(x - sx, z - sz) < near_start for sx, sz in starts):
            return False
        occ[z0:z0 + f, x0:x0 + f] = 2
        props.append({"def": kind, "pos": {"x": round(x, 2), "y": round(z, 2)}, "yaw": round(yaw, 4), "owner": owner})
        return True

    # Landmarks first: towers on hills, churches, water towers.
    for t, (x, z) in points:
        mm = t.get("man_made")
        if mm == "water_tower":
            place("water_tower", x, z, 0.0)
        elif mm in ("tower", "mast") and t.get("tower:type") in ("observation", "communication", None):
            place("comm_tower", x, z, 0.0)
    cand = []
    for t, ring in buildings:
        yaw, area_u = orient(list(ring))
        area = area_u / geo.upm ** 2
        x = sum(p[0] for p in ring) / len(ring)
        z = sum(p[1] for p in ring) / len(ring)
        if not (0 < x < world and 0 < z < world):
            continue
        lu = "industrial" if Raster.at(ras.arr("industrial"), x, z) else "farmyard" if Raster.at(ras.arr("farmyard"), x, z) else ""
        dense = dens[min(int(z * rs), ras.size - 1), min(int(x * rs), ras.size - 1)] > 0.22
        kind = building_kind(t, area, lu, dense, rng)
        if kind:
            prio = 3 if kind == "church" else 2 if kind in ("science_lab", "admin_hall", "water_tower") else 1
            cand.append((prio, area, x, z, yaw, kind))
    # Important and big buildings first; houses in random order so thinning is even.
    big = sorted([c_ for c_ in cand if c_[0] > 1 or c_[1] > 350], key=lambda c_: (-c_[0], -c_[1]))
    small = [c_ for c_ in cand if not (c_[0] > 1 or c_[1] > 350)]
    rng.shuffle(small)
    for _, _, x, z, yaw, kind in big + small:
        if len(props) >= a.max_props:
            break
        place(kind, x, z, yaw)
    # Power pylons along the real lines (sparse).
    pyl = 0
    for t, (x, z) in points:
        if t.get("power") == "tower" and len(props) < a.max_props + 60:
            if place("pylon", x, z, 0.0, margin=2):
                pyl += 1
    counts = {}
    for p in props:
        counts[p["def"]] = counts.get(p["def"], 0) + 1
    print(f"{len(props)} props ({pyl} pylons):", dict(sorted(counts.items(), key=lambda kv: -kv[1])))

    # Hostile outpost on the highest open hilltop (the best lookout on the map).
    hi = None
    for j in range(20, n - 20, 2):
        for i in range(20, n - 20, 2):
            x, z = (i + 0.5) * CELL, (j + 0.5) * CELL
            if any(math.hypot(x - sx, z - sz) < 140 for sx, sz in starts):
                continue
            if hi is None or heights[j, i] > hi[0]:
                hi = (heights[j, i], x, z)
    if hi:
        _, x, z = hi
        for dx, dz, kind, own in [(0, 0, "comm_tower", 0), (-9, 5, "bunker", 1), (8, -6, "bunker", 1), (3, 10, "bunker", 1)]:
            for r in range(6):
                if place(kind, x + dx + r * 2, z + dz, 0.0, owner=own, margin=1):
                    break
        print(f"hilltop outpost at {x:.0f},{z:.0f}")

    # ---------------------------------------------------------------- deposits
    deposits = []

    def dep_ok(x, z, keep=7.0):
        i, j = int(x / CELL), int(z / CELL)
        if not (2 <= i < n - 2 and 2 <= j < n - 2) or occ[j - 1:j + 2, i - 1:i + 2].any():
            return False
        return all(math.hypot(x - d[0], z - d[1]) >= keep for d in deposits)

    def add_dep(x, z, search=14.0, keep=7.0):
        for r in np.arange(0, search, 1.0):
            for k in range(max(1, int(r * 3))):
                ang = k / max(1, int(r * 3)) * 2 * math.pi
                px, pz = x + math.cos(ang) * r, z + math.sin(ang) * r
                cx, cz = (math.floor(px / CELL) + 0.5) * CELL, (math.floor(pz / CELL) + 0.5) * CELL
                if dep_ok(cx, cz, keep):
                    deposits.append((cx, cz))
                    occ[int(cz / CELL), int(cx / CELL)] = 3
                    return True
        return False

    # Same offsets for every start, turned towards the map centre.
    base = [(7, 3), (-3, 7), (-7, -5), (5, -7)]
    exp_rng = random.Random(a.seed * 7 + 3)
    expansions = []
    while len(expansions) < 12:
        fwd, side = exp_rng.uniform(35, 230), exp_rng.uniform(-150, 150)
        if math.hypot(fwd, side) < 45 or any(math.hypot(fwd - e[0], side - e[1]) < 26 for e in expansions):
            continue
        expansions.append((fwd, side))
    for sx, sz in starts:
        dx, dz = c - sx, c - sz
        L = math.hypot(dx, dz) or 1
        fx, fz = dx / L, dz / L
        for ox, oz in base:
            add_dep(sx + ox, sz + oz, search=6, keep=4)
        for fwd, side in expansions:
            add_dep(sx + fx * fwd - fz * side, sz + fz * fwd + fx * side)
    for ang in range(0, 360, 60):
        add_dep(c + math.cos(math.radians(ang)) * 28, c + math.sin(math.radians(ang)) * 28)
    print(f"{len(deposits)} deposits")

    # ---------------------------------------------------------------- trees and rocks
    rules = json.load(open(os.path.join(ROOT, "assets/data/rules.json"), encoding="utf-8"))
    fdefs = {f["id"]: f for f in rules["features"]}
    shape_r = {"pine": 0.8, "broadleaf": 1.1, "bush": 0.6, "rock": 0.9}
    radius = {k: f["size"] * shape_r.get(f["shape"], 1.0) for k, f in fdefs.items()}
    G = 4.0
    gn = int(world / G) + 1
    grid = [[] for _ in range(gn * gn)]
    feats = []
    foots = [(p["pos"]["x"], p["pos"]["y"], foot[p["def"]] * CELL * 0.5 + 0.8) for p in props]
    fgrid = {}
    for fx_, fz_, half in foots:
        fgrid.setdefault((int(fx_ // 16), int(fz_ // 16)), []).append((fx_, fz_, half))

    def free(x, z, r, gap):
        if not (3 < x < world - 3 and 3 < z < world - 3):
            return False
        gx, gz = int(x / G), int(z / G)
        for yy in range(max(0, gz - 2), min(gn, gz + 3)):
            for xx in range(max(0, gx - 2), min(gn, gx + 3)):
                for qx, qz, qr in grid[yy * gn + xx]:
                    if (qx - x) ** 2 + (qz - z) ** 2 < (max(gap, (r + qr) * 0.85)) ** 2:
                        return False
        return True

    def ok_spot(x, z, r):
        i, j = int(x / CELL), int(z / CELL)
        if blocked[j, i] or Raster.at(water, x, z) or Raster.at(built, x, z):
            return False
        if any(math.hypot(x - sx, z - sz) < 30 for sx, sz in starts):
            return False
        if any(math.hypot(x - dx_, z - dz_) < 3.5 + r for dx_, dz_ in deposits):
            return False
        for fx_, fz_, half in fgrid.get((int(x // 16), int(z // 16)), []) + [f for k in [(int(x // 16) + a_, int(z // 16) + b_) for a_ in (-1, 0, 1) for b_ in (-1, 0, 1) if (a_, b_) != (0, 0)] for f in fgrid.get(k, [])]:
            if abs(x - fx_) < half + r and abs(z - fz_) < half + r:
                return False
        return True

    def put(kind, x, z, gap, scale=None):
        s = scale if scale is not None else rng.uniform(0.8, 1.2)
        r = radius[kind] * s
        if not free(x, z, r, gap) or not ok_spot(x, z, r):
            return False
        grid[int(z / G) * gn + int(x / G)].append((x, z, r))
        feats.append(f"{kind} {x:.2f} {z:.2f} {rng.uniform(-math.pi, math.pi):.2f} {s:.2f}")
        return True

    def pick(table):
        t = rng.random() * sum(w for _, w in table)
        for k, w in table:
            if t < w:
                return k
            t -= w
        return table[-1][0]

    mixes = {
        "needle": [("fir", 5), ("pine", 4), ("birch", 0.5)],
        "broad": [("oak", 4), ("birch", 2), ("pine", 0.5)],
        "mixed": [("fir", 3), ("pine", 2), ("oak", 2), ("birch", 1)],
    }
    layers = {k: ras.arr(k) for k in mixes}
    edge = blur(forest, 3 * rs)
    # OSM points: rocks and lone trees.
    for t, (x, z) in points:
        nat = t.get("natural")
        if nat in ("rock", "stone"):
            put("boulder" if nat == "rock" else "rock", x, z, 2.0)
        elif nat == "tree":
            put(rng.choice(["oak", "oak", "birch"]), x, z, 3.0)
    # Forests: dart throwing at ~3 units apart, edges a little sparser with shrubs.
    darts = int(world * world / 2.2)
    rockgap = 0
    for _ in range(darts):
        x, z = rng.uniform(3, world - 3), rng.uniform(3, world - 3)
        pi_, pj = int(x * rs), int(z * rs)
        if forest[pj, pi_]:
            layer = "needle" if layers["needle"][pj, pi_] else "broad" if layers["broad"][pj, pi_] else "mixed"
            e = edge[pj, pi_]
            if e < 0.75 and rng.random() < 0.3:
                put("shrub", x, z, 2.2)
            else:
                put(pick(mixes[layer]), x, z, 3.0)
        elif ras.arr("scrub")[pj, pi_] if False else False:
            pass
    scrub = ras.arr("scrub")
    park = ras.arr("park")
    rockarea = ras.arr("rock")
    for _ in range(int(world * world / 8)):
        x, z = rng.uniform(3, world - 3), rng.uniform(3, world - 3)
        pi_, pj = int(x * rs), int(z * rs)
        if scrub[pj, pi_]:
            put(pick([("shrub", 3), ("birch", 1)]), x, z, 2.5)
        elif park[pj, pi_] and rng.random() < 0.35:
            put(pick([("oak", 3), ("birch", 2)]), x, z, 4.5)
        elif rockarea[pj, pi_]:
            put(pick([("rock", 2), ("stone", 3), ("boulder", 0.5)]), x, z, 2.0)
    # Rocks on steep or high forest ground, a few stones in the fields, lone trees and hedges.
    hmax, hmin = heights.max(), heights.min()
    for _ in range(int(world * world / 6)):
        x, z = rng.uniform(3, world - 3), rng.uniform(3, world - 3)
        pi_, pj = int(x * rs), int(z * rs)
        i, j = int(x / CELL), int(z / CELL)
        s = slope[j, i]
        hn = (heights[j, i] - hmin) / (hmax - hmin)
        if forest[pj, pi_]:
            if rng.random() < 0.02 * (s * 2.5 + hn * hn * 1.5):
                if put(pick([("stone", 4), ("rock", 3), ("boulder", 1)]), x, z, 2.0):
                    rockgap += 1
        elif not built[pj, pi_]:
            r_ = rng.random()
            if r_ < 0.0012:
                put(pick([("stone", 3), ("rock", 1)]), x, z, 3.0)
            elif r_ < 0.006 and edge[pj, pi_] < 0.05 and dens[pj, pi_] < 0.05:
                put(pick([("oak", 3), ("birch", 2), ("shrub", 2), ("pine", 1)]), x, z, 3.5)
            elif r_ < 0.012 and dens[pj, pi_] > 0.03:
                put(pick([("oak", 2), ("birch", 2), ("shrub", 2)]), x, z, 3.5)
    kinds = {}
    for f in feats:
        k = f.split()[0]
        kinds[k] = kinds.get(k, 0) + 1
    print(f"{len(feats)} trees and rocks:", kinds)

    # ---------------------------------------------------------------- write
    out = os.path.join(ROOT, "assets", "maps", a.name)
    os.makedirs(out, exist_ok=True)
    mn, mx = float(heights.min()), float(heights.max())
    px = np.round((heights - mn) / (mx - mn) * 65535).astype(np.uint16)
    Image.fromarray(px).save(os.path.join(out, "height.png"))
    m = {
        "name": a.title or a.name,
        "n": n,
        "water": round(mn - 2.0, 3),
        "min_height": mn,
        "max_height": mx,
        "deposits": [{"x": d[0], "y": d[1]} for d in deposits],
        "starts": [{"x": s[0], "y": s[1]} for s in starts],
        "props": props,
        "seed": a.seed,
        "features": feats,
    }
    with open(os.path.join(out, "map.json"), "w", encoding="utf-8") as f:
        json.dump(m, f, indent=2)
    print("wrote", out)

    # Preview picture.
    S = 2
    shade = np.zeros((n + 1, n + 1))
    shade[1:, 1:] = (heights[1:, 1:] - heights[:-1, :-1]) * 1.5
    base = 0.55 + 0.45 * (heights - mn) / (mx - mn)
    rgb = np.stack([base * 150, base * 170, base * 120], -1) * np.clip(1 - shade[..., None] * 0.35, 0.5, 1.4)
    img = Image.fromarray(np.clip(rgb, 0, 255).astype(np.uint8)).resize(((n + 1) * S // 1, (n + 1) * S // 1))
    d = ImageDraw.Draw(img)
    k = S / CELL
    for f in feats:
        kind, x, z = f.split()[:3]
        x, z = float(x) * k, float(z) * k
        col = (120, 120, 120) if fdefs[kind]["class"] == "rock" else (30, 80, 35)
        d.point((x, z), fill=col)
    for p in props:
        x, z = p["pos"]["x"] * k, p["pos"]["y"] * k
        h = foot[p["def"]] * CELL * k / 2
        d.rectangle([x - h, z - h, x + h, z + h], fill=(200, 40, 40) if p["owner"] == 0 else (255, 0, 255))
    for x, z in deposits:
        d.ellipse([x * k - 2, z * k - 2, x * k + 2, z * k + 2], fill=(40, 220, 255))
    for i, (x, z) in enumerate(starts):
        d.ellipse([x * k - 8, z * k - 8, x * k + 8, z * k + 8], outline=(255, 255, 0), width=3)
        d.text((x * k + 10, z * k - 6), str(i + 1), fill=(255, 255, 0))
    os.makedirs(os.path.join(ROOT, "tools", "out"), exist_ok=True)
    img.save(os.path.join(ROOT, "tools", "out", f"{a.name}_preview.png"))


if __name__ == "__main__":
    main()
