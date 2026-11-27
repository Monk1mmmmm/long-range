#!/usr/bin/env python3
"""Make a game map of a real place from open data.

    .venv/bin/python tools/osm_map.py NAME --bbox SOUTH,WEST,NORTH,EAST [options]
    .venv/bin/python tools/osm_map.py NAME --center LAT,LON [--km 10] [options]
    options: [--cells 384] [--players 4] [--relief 0.85] [--starts "x,z;x,z;..."] [--seed 1]
             [--title "Shown name"] [--max-props 700] [--village 0.45]
             [--river "River name" --river-width 5,7]
             [--river-roads] [--river-ford-every 140]
             [--source overpass|overture] [--sea] [--drop-bridges S,W,N,E] [--drop-names REGEX]

The rectangle is given in degrees (copy it from openstreetmap.org: Export shows the box of the view,
or right-click > "Show address" for a point). Game maps are square, so a rectangle is widened on its
short side to a square around the same centre.

Elevation: AWS Terrain Tiles (Mapzen "terrarium" PNGs, zoom 13, ~12 m per pixel).
Everything else: OpenStreetMap through the Overpass API (buildings, forests and their leaf type,
scrub, parks, water, rocks, towers, power pylons, churches, roads, fields and other land use).
Downloads are cached in tools/out/osm_cache. Output: assets/maps/NAME/{map.json,height.png,layerN.png}
and a picture tools/out/NAME_preview.png (terrain shading, roads, forests, buildings, deposits, starts).

Ground textures: grass everywhere, with layers for fields and meadows, forest floor, farmyards,
industrial gravel, paving in built-up blocks, sand, mud under water and stone on rock and steep
slopes (masks `layerN.png`, one pixel per cell). Roads come from the highway ways (asphalt, or a
dirt track for tracks and unpaved roads), drawn wider than true scale like the buildings; trees
and rocks keep off them, and buildings off the main roads. Afterwards run
`longrange --editor --roads NAME`: it gives them their look (worn side streets, concrete, cobbles,
sidewalks in towns) and cuts what runs under water.

North is up (-z). Heights are true scale times
--relief. One prop is placed per real building where the game building (2-3x the size of a real
one) still fits; trees follow the forest outlines; rocks come from OSM rocks plus steep forest
slopes. Start positions are picked in open, flat ground (or given in world units with --starts),
levelled, and get deposits at the same offsets relative to the map centre, so the expansions are
roughly fair even though the land is not.

--source overture reads Overture Maps (tools/overture_src.py; a public S3 bucket, built from OpenStreetMap and
other open data) instead of the Overpass API: use it when the Overpass servers are down or not reachable.
--sea makes a coast: the sea is the data's ocean polygon at the map's water level (0 m), with a sloping sea
bed (shallows a land unit cannot enter, deep water nobody can), and the land is never below it.
Maps of the past: the data is the present day, so --drop-bridges S,W,N,E removes the road bridges (and the
ramps that led to them) inside a rectangle and --drop-names REGEX roads by name (Overture source only).

Map data (c) OpenStreetMap contributors (ODbL), Overture Maps Foundation (CDLA-Permissive-2.0 / ODbL);
elevation: Mapzen / AWS Terrain Tiles.
"""
import argparse
import io
import json
import math
import os
import random
import sys
import time
import urllib.parse
import urllib.request

import numpy as np
from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
CACHE = os.path.join(ROOT, "tools", "out", "osm_cache")
UA = "lr-mapgen/1.0"
OVERPASS = ["https://overpass-api.de/api/interpreter", "https://overpass.private.coffee/api/interpreter"]
CELL = 2.0
Z = 13
# Drawn road width (world units) per highway class: wider than true scale, like the buildings.
ROAD_WIDTH = {"motorway": 7.0, "trunk": 6.0, "primary": 5.0, "secondary": 4.5, "tertiary": 4.0, "unclassified": 3.5,
              "residential": 3.5, "living_street": 3.0, "service": 2.5, "track": 2.5}
MAIN_ROADS = {"motorway", "trunk", "primary", "secondary", "tertiary"}
# Side streets in towns (with --village or --town-streets): narrower, or the oversized buildings find no room between them.
TOWN_STREET = 2.2
# Bridges over the river (--river) by what they carry: width of the land bridge in world units.
# Footbridges are not in the road query, so they make no crossing.
BRIDGE_WIDTH = {"motorway": 7.0, "trunk": 7.0, "primary": 6.0, "secondary": 6.0, "rail": 5.0}


def fetch(url, name, data=None):
    os.makedirs(CACHE, exist_ok=True)
    path = os.path.join(CACHE, name)
    if os.path.exists(path):
        with open(path, "rb") as f:
            return f.read()
    body = urllib.parse.urlencode({"data": data}).encode() if data else None
    urls = OVERPASS if url == "overpass" else [url]
    raw, err = None, None
    for attempt in range(6):
        u = urls[attempt % len(urls)]
        try:
            req = urllib.request.Request(u, data=body, headers={"User-Agent": UA})
            with urllib.request.urlopen(req, timeout=300) as r:
                raw = r.read()
            if data and b"runtime error" in raw[:4000]:
                raise RuntimeError(raw[:300].decode(errors="replace"))
            break
        except Exception as e:  # busy servers answer 429/502/504 or a timeout remark
            raw, err = None, e
            print(f"  {u}: {e}, retrying", file=sys.stderr)
            time.sleep(5 + attempt * 5)
    if raw is None:
        sys.exit(f"download failed: {err}")
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


def overpass(geo, name, river=False):
    bb = f"{geo.s:.5f},{geo.w:.5f},{geo.n:.5f},{geo.e:.5f}"
    head = "[out:json][timeout:240];\n(\n"
    # Two queries: one big answer is more likely to time out at a busy server's gateway.
    parts = {
        "buildings": f'  way["building"]({bb});\n',
        "land": f"""  way["landuse"~"^(forest|residential|industrial|commercial|retail|farmyard|cemetery|quarry|reservoir|basin)$"]({bb});
  relation["landuse"~"^(forest|residential|industrial|reservoir)$"]({bb});
  way["natural"~"^(wood|scrub|water|rock|bare_rock|scree|heath)$"]({bb});
  relation["natural"~"^(wood|water|scrub)$"]({bb});
  way["leisure"="park"]({bb});
  way["waterway"="riverbank"]({bb});
  node["natural"~"^(rock|stone|tree|peak)$"]({bb});
  node["man_made"~"^(water_tower|tower|mast)$"]({bb});
  way["man_made"~"^(water_tower|tower)$"]({bb});
  node["power"="tower"]({bb});
""",
        "roads": f'  way["highway"~"^(motorway|trunk|primary|secondary|tertiary|unclassified|residential|living_street|service|track|motorway_link|trunk_link|primary_link|secondary_link|tertiary_link)$"]({bb});\n',
        "ground": f"""  way["landuse"~"^(farmland|meadow|grass|orchard|vineyard|allotments|construction|brownfield|railway|village_green)$"]({bb});
  relation["landuse"~"^(farmland|meadow|grass)$"]({bb});
  way["natural"~"^(sand|beach|grassland|glacier)$"]({bb});
""",
    }
    if river:
        parts["rivers"] = f'  way["waterway"="river"]({bb});\n'
        parts["railbridges"] = f'  way["bridge"]["railway"~"^(rail|tram|light_rail)$"]({bb});\n'
    els = []
    for part, body in parts.items():
        raw = fetch("overpass", f"overpass_{name}_{part}.json", data=head + body + ");\nout geom;")
        for el in json.loads(raw)["elements"]:
            el["_part"] = part
            els.append(el)
    return els


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


# ---------------------------------------------------------------- rivers

def chain(lines):
    """Joins polylines (lists of hashable points) that share end points; longest first."""
    out = [list(l) for l in lines if len(l) >= 2]
    merged = True
    while merged:
        merged = False
        for i in range(len(out)):
            for k in range(len(out)):
                if i == k or not out[i] or not out[k]:
                    continue
                a, b = out[i], out[k]
                if a[-1] == b[0]:
                    out[i] = a + b[1:]
                elif a[-1] == b[-1]:
                    out[i] = a + b[::-1][1:]
                elif a[0] == b[-1]:
                    out[i] = b + a[1:]
                elif a[0] == b[0]:
                    out[i] = b[::-1] + a[1:]
                else:
                    continue
                out[k] = []
                merged = True
        out = [l for l in out if l]
    return sorted(out, key=len, reverse=True)


def simplify(pts, tol):
    """Douglas-Peucker."""
    if len(pts) < 3:
        return list(pts)
    (ax, az), (bx, bz) = pts[0], pts[-1]
    l2 = max((bx - ax) ** 2 + (bz - az) ** 2, 1e-9)
    best, idx = -1.0, 0
    for i in range(1, len(pts) - 1):
        px, pz = pts[i]
        t = min(max(((px - ax) * (bx - ax) + (pz - az) * (bz - az)) / l2, 0.0), 1.0)
        dd = math.hypot(px - ax - (bx - ax) * t, pz - az - (bz - az) * t)
        if dd > best:
            best, idx = dd, i
    if best <= tol:
        return [pts[0], pts[-1]]
    return simplify(pts[:idx + 1], tol)[:-1] + simplify(pts[idx:], tol)


def resample(pts, step):
    """Points about every `step` along a polyline."""
    out = []
    for (x0, z0), (x1, z1) in zip(pts, pts[1:]):
        k = max(1, int(math.ceil(math.hypot(x1 - x0, z1 - z0) / step)))
        out += [(x0 + (x1 - x0) * j / k, z0 + (z1 - z0) * j / k) for j in range(k)]
    out.append(tuple(pts[-1]))
    return np.array(out)


def seg_cross(p, q, a, b):
    """Intersection point of segments p-q and a-b, or None."""
    r = (q[0] - p[0], q[1] - p[1])
    s = (b[0] - a[0], b[1] - a[1])
    den = r[0] * s[1] - r[1] * s[0]
    if abs(den) < 1e-12:
        return None
    t = ((a[0] - p[0]) * s[1] - (a[1] - p[1]) * s[0]) / den
    u = ((a[0] - p[0]) * r[1] - (a[1] - p[1]) * r[0]) / den
    return (p[0] + r[0] * t, p[1] + r[1] * t) if 0 <= t <= 1 and 0 <= u <= 1 else None


def nearest(px, pz, pts, chunk=6000):
    """Distance from each point (flat arrays px, pz) to the nearest of `pts`, and that one's index."""
    d = np.empty(len(px))
    idx = np.empty(len(px), np.int64)
    for i in range(0, len(px), chunk):
        d2 = (px[i:i + chunk, None] - pts[None, :, 0]) ** 2 + (pz[i:i + chunk, None] - pts[None, :, 1]) ** 2
        k = d2.argmin(1)
        idx[i:i + chunk] = k
        d[i:i + chunk] = np.sqrt(d2[np.arange(len(k)), k])
    return d, idx


# ---------------------------------------------------------------- buildings

HOUSES = ["house", "house2", "house3", "house4", "house5", "house6", "house7", "house8", "house9", "khata", "khata_slate", "cottage", "dacha"]
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
    ap.add_argument("--bbox", default="", help="SOUTH,WEST,NORTH,EAST in degrees")
    ap.add_argument("--center", default="", help="LAT,LON (with --km)")
    ap.add_argument("--km", type=float, default=10.0, help="side of the square with --center")
    ap.add_argument("--cells", type=int, default=384)
    ap.add_argument("--players", type=int, default=4)
    ap.add_argument("--relief", type=float, default=0.85, help="vertical scale against true scale")
    ap.add_argument("--starts", default="", help="'x,z;x,z;...' in world units (default: picked automatically)")
    ap.add_argument("--seed", type=int, default=1)
    ap.add_argument("--title", default="")
    ap.add_argument("--max-props", type=int, default=600, help="buildings (pylons and the outpost come on top)")
    ap.add_argument("--river", default="", help="OSM name of a river to make water: the land is tilted along it so it runs level")
    ap.add_argument("--river-width", default="5,7", help="width of the river in world units, upstream,downstream")
    ap.add_argument("--river-roads", action="store_true", help="every road over the river crosses it, not only those mapped as bridges")
    ap.add_argument("--river-ford-every", type=float, default=0.0, help="add fords so crossings are at most this many world units apart along the river")
    ap.add_argument("--points", type=int, default=-1, help="control points (default: by the number of players; 0: none, the game places its own)")
    ap.add_argument("--no-lakes", action="store_true", help="leave lakes and ponds dry (mud), as before the game had lakes")
    ap.add_argument("--lake-min", type=float, default=60.0, help="smallest lake kept, in square world units")
    ap.add_argument("--village", type=float, default=1.0, help="share of the real buildings kept outside the towns (towns keep all that fit)")
    ap.add_argument("--town-streets", action="store_true", help="narrow side streets in towns and cut the residential ones where buildings stand, as --village below 1 does, without thinning the villages")
    ap.add_argument("--cp-town", type=float, default=0.6, help="share of the control points placed in the towns (the densest places), the rest around them")
    ap.add_argument("--cp-area", default="", help="S,W,N,E in degrees: control points only inside this rectangle (e.g. the city of a map that also shows its surroundings)")
    ap.add_argument("--infill", action="append", default=[], metavar="S,W,N,E",
                    help="made-up small houses along residential streets inside this rectangle (degrees; repeat for several) "
                         "wherever no real building stands near: for places where the open data has roads but no buildings")
    ap.add_argument("--infill-gap", type=float, default=14.0, help="no made-up house within this many world units of a real one")
    ap.add_argument("--infill-step", type=float, default=9.0, help="distance between made-up houses along a street (world units)")
    ap.add_argument("--infill-max", type=int, default=700, help="most made-up houses")
    ap.add_argument("--dep-area", default="", help="S,W,N,E in degrees: resource deposits only inside this rectangle (the part of the map that is played over)")
    ap.add_argument("--outpost", default="", help="LAT,LON of the hostile hilltop outpost (default: the highest open hilltop)")
    ap.add_argument("--source", choices=("overpass", "overture"), default="overpass", help="where the map features come from")
    ap.add_argument("--sea", action="store_true", help="a coast: the ocean polygon becomes the sea at 0 m (needs --source overture)")
    ap.add_argument("--sea-height", type=float, default=6.0, help="water level in world units (the sea bed goes down from it)")
    ap.add_argument("--drop-bridges", default="", help="S,W,N,E: leave out road bridges and their ramps inside this rectangle (a map of the past)")
    ap.add_argument("--drop-names", default="", help="regex: leave out roads whose name matches (a map of the past)")
    a = ap.parse_args()
    if a.sea and a.source != "overture":
        sys.exit("--sea needs --source overture (the ocean polygon)")

    n = a.cells
    world = n * CELL
    if a.bbox:
        s_, w_, n_, e_ = map(float, a.bbox.split(","))
        lat, lon = (s_ + n_) / 2, (w_ + e_) / 2
        km = max((n_ - s_) * 110.574, (e_ - w_) * 111.320 * math.cos(math.radians(lat)))
    elif a.center:
        lat, lon = map(float, a.center.split(","))
        km = a.km
    else:
        sys.exit("give --bbox SOUTH,WEST,NORTH,EAST or --center LAT,LON")
    geo = Geo(lat, lon, km, world)
    rng = random.Random(a.seed)
    print(f"square of {km:.2f} km: lat {geo.s:.4f}..{geo.n:.4f} lon {geo.w:.4f}..{geo.e:.4f}, {1 / geo.upm:.1f} m per world unit")

    # Heights.
    elev = elevation_grid(geo, n)
    print(f"elevation {elev.min():.0f}..{elev.max():.0f} m")
    heights = (elev - elev.min()) * geo.upm * a.relief + 1.0

    # OSM layers.
    if a.source == "overture":
        import overture_src
        els = overture_src.fetch(geo, CACHE, a.name)
        drop = tuple(map(float, a.drop_bridges.split(","))) if a.drop_bridges else None
        els = overture_src.drop_future(els, a.drop_names or None, drop)
    else:
        els = overpass(geo, a.name, river=bool(a.river))
    ras = Raster(world)
    sea_polys = []
    buildings, points, roads, lake_polys = [], [], [], []
    river_ways, bridge_ways, road_ways = [], [], []
    for el in els:
        t = el.get("tags", {})
        if el["_part"] in ("rivers", "railbridges"):
            if el["type"] == "way" and "geometry" in el:
                if el["_part"] == "rivers":
                    river_ways.append((t, el["geometry"]))
                else:
                    bridge_ways.append((BRIDGE_WIDTH["rail"], [geo.to_world(p["lat"], p["lon"]) for p in el["geometry"]]))
            continue
        if el["type"] == "node":
            points.append((t, geo.to_world(el["lat"], el["lon"])))
            continue
        if "_sea" in t:
            outer, inner = shapes(el, geo)
            sea_polys.append((outer, inner))
            ras.poly("water", outer, inner)
            continue
        if "highway" in t and el["type"] == "way" and "geometry" in el and "building" not in t:
            if t.get("tunnel") not in (None, "no") or t.get("area") == "yes":
                continue
            hw = t["highway"].replace("_link", "")
            if t.get("bridge", "no") != "no":
                bridge_ways.append((BRIDGE_WIDTH.get(hw, 5.0), [geo.to_world(p["lat"], p["lon"]) for p in el["geometry"]]))
            elif a.river_roads and hw in ROAD_WIDTH:
                # --river-roads: every road over the river crosses it (a stream's small bridges and culverts are
                # often not mapped as such).
                road_ways.append((max(4.0, ROAD_WIDTH[hw] + 1.0), [geo.to_world(p["lat"], p["lon"]) for p in el["geometry"]]))
            unpaved = t.get("surface") in ("unpaved", "dirt", "earth", "ground", "grass", "mud", "sand", "compacted", "fine_gravel", "gravel")
            kind = "dirt_road" if hw == "track" or unpaved else "asphalt"
            if t.get("surface") in ("gravel", "fine_gravel", "compacted") and hw != "track":
                kind = "gravel"
            width = ROAD_WIDTH.get(hw, 3.0)
            # 2 main road (buildings keep a cell off), 1 street (buildings keep off it), 3 side street
            # (a street, except in towns: there buildings may cover it), 0 service road or track
            level = 2 if hw in MAIN_ROADS else 3 if hw in ("residential", "living_street") else 1 if hw not in ("service", "track") else 0
            roads.append((kind, width, level, [geo.to_world(p["lat"], p["lon"]) for p in el["geometry"]]))
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
            # Standing water becomes a lake at its own level (rivers slope: they are left to --river).
            if t.get("waterway") != "riverbank" and t.get("water") not in ("river", "stream", "canal", "ditch", "drain"):
                lake_polys.append((outer, inner))
        elif nat in ("bare_rock", "scree", "rock"):
            ras.poly("rock", outer, inner)
        elif t.get("leisure") == "park" or lu == "cemetery":
            ras.poly("park", outer, inner)
        elif lu in ("industrial", "residential", "commercial", "retail", "farmyard", "quarry"):
            ras.poly(lu, outer, inner)
        elif lu in ("farmland", "meadow", "grass", "orchard", "vineyard", "village_green") or nat == "grassland":
            ras.poly("field", outer, inner)
        elif lu in ("allotments", "construction", "brownfield"):
            ras.poly("dirt", outer, inner)
        elif lu == "railway":
            ras.poly("industrial", outer, inner)
        elif nat in ("sand", "beach"):
            ras.poly("sand", outer, inner)
        elif nat == "glacier":
            ras.poly("snow", outer, inner)
    print(f"{len(buildings)} buildings, {len(points)} points of interest, {len(roads)} roads")

    # ---------------------------------------------------------------- the sea (--sea)
    # Land never lies below the water level; the sea bed slopes down from the shore, 0.95 per ring of
    # vertices (a land unit cannot enter any cell with water over it; amphibious ones wade the shallows,
    # `map::AMPHIBIOUS_DEPTH`), to about 7 units deep.
    sea_level, sea_mask = None, None
    if a.sea:
        side = n + 1
        SS = 4
        im = Image.new("L", (side * SS, side * SS), 0)
        dr = ImageDraw.Draw(im)
        for outer, inner in sea_polys:
            for r_ in outer:
                if len(r_) >= 3:
                    dr.polygon([(x / CELL * SS + SS / 2, z / CELL * SS + SS / 2) for x, z in r_], fill=255)
            for r_ in inner:
                if len(r_) >= 3:
                    dr.polygon([(x / CELL * SS + SS / 2, z / CELL * SS + SS / 2) for x, z in r_], fill=0)
        sea_mask = np.asarray(im)[SS // 2::SS, SS // 2::SS][:side, :side] > 127
        sea_level = a.sea_height
        land_h = np.maximum(elev, 0.0) * geo.upm * a.relief
        depth, core = np.zeros((side, side)), sea_mask.copy()
        for k in range(9):
            depth += core * (0.5 if k == 0 else 0.95)
            g_ = core.copy()
            g_[1:, :] &= core[:-1, :]
            g_[:-1, :] &= core[1:, :]
            g_[:, 1:] &= core[:, :-1]
            g_[:, :-1] &= core[:, 1:]
            core = g_
            if not core.any():
                break
        heights = np.where(sea_mask, sea_level - depth, sea_level + 0.35 + land_h)
        print(f"sea: {sea_mask.mean() * 100:.0f}% of the map, land {elev[~sea_mask].min():.0f}..{elev[~sea_mask].max():.0f} m")

    def sea_at(x, z):
        if sea_mask is None:
            return False
        i, j = int(round(x / CELL)), int(round(z / CELL))
        return 0 <= i < sea_mask.shape[1] and 0 <= j < sea_mask.shape[0] and bool(sea_mask[j, i])
    # Roads cut to the map (with a little overhang, so they run off the edge), points thinned to
    # about one per 1.5 units; a road leaving and re-entering the map becomes two.
    # Town or village: built-up share over ~350 m. Towns keep every building that fits (dense
    # blocks, lanes a cell wide) and narrower side streets, villages --village of the buildings.
    dens_big = blur(ras.arr("building"), int(350 * geo.upm * Raster.RES / 2) + 1)

    def town_at(x, z):
        v = dens_big[min(max(int(z * Raster.RES), 0), ras.size - 1), min(max(int(x * Raster.RES), 0), ras.size - 1)]
        return min(max((v - 0.07) / 0.08, 0.0), 1.0)

    town_streets = a.village < 1.0 or a.town_streets
    if a.village < 1.0:
        bpx = ras.arr("building")
        print(f"town: {(dens_big[bpx] > 0.11).mean() * 100:.0f}% of the built area, {(dens_big > 0.11).mean() * 100:.0f}% of the map")
    road_lines = []
    for kind, width, main_road, pts in roads:
        if main_road in (1, 3) and town_streets:
            # Towns: side streets narrower, and the residential ones give way to the buildings
            # (the real street grid does not fit between buildings 2-3x the real size).
            t_ = town_at(*pts[len(pts) // 2])
            width = width + (TOWN_STREET - width) * t_
            if t_ <= 0.5:
                main_road = min(main_road, 1)
        elif main_road == 3:
            main_road = 1
        cur = []
        for x, z in pts:
            inside = -8 < x < world + 8 and -8 < z < world + 8 and not sea_at(x, z)
            if inside:
                if not cur or math.hypot(x - cur[-1][0], z - cur[-1][1]) >= 1.5:
                    cur.append((x, z))
            elif len(cur) >= 2:
                road_lines.append((kind, width, main_road, cur))
                cur = []
            else:
                cur = []
        if len(cur) >= 2:
            road_lines.append((kind, width, main_road, cur))
    # Kinds in use, at most four (the game shows four road kinds per map).
    road_kinds = []
    for kind, *_ in road_lines:
        if kind not in road_kinds:
            road_kinds.append(kind)
    if len(road_kinds) > 4:
        road_lines = [r for r in road_lines if r[0] in road_kinds[:4]]
    # Road raster: where trees and rocks must not stand (crowns reach ~1.2 over the edge), and
    # main roads, where buildings must not stand.
    road_img = Image.new("L", (ras.size, ras.size), 0)
    main_img = Image.new("L", (ras.size, ras.size), 0)
    street_img = Image.new("L", (ras.size, ras.size), 0)
    rd, md, sd = ImageDraw.Draw(road_img), ImageDraw.Draw(main_img), ImageDraw.Draw(street_img)
    for kind, width, main_road, pts in road_lines:
        xy = [(x * Raster.RES, z * Raster.RES) for x, z in pts]
        rd.line(xy, fill=255, width=max(1, int((width + 2.4) * Raster.RES)), joint="curve")
        if main_road == 2:
            md.line(xy, fill=255, width=max(1, int(width * Raster.RES)), joint="curve")
        elif main_road == 1:
            sd.line(xy, fill=255, width=max(1, int(width * Raster.RES)), joint="curve")
    on_road = np.asarray(road_img) > 127
    on_main = np.asarray(main_img) > 127
    on_street = np.asarray(street_img) > 127

    # ---------------------------------------------------------------- the river (--river)
    # The game has one water level, so the land is tilted along the river until the river runs
    # level: a smooth field (the river's own height, spread out from it) is taken off every vertex,
    # and hills keep their height above the valley floor. The bed is then dug under the water,
    # except where a real road or railway bridge (or a culvert) crosses: the ground stays there as a
    # land bridge.
    water_level, bridges = sea_level, []
    if a.river:
        def sample(grid, x, z):
            fx = np.clip(np.asarray(x, float) / CELL, 0, n - 0.001)
            fz = np.clip(np.asarray(z, float) / CELL, 0, n - 0.001)
            i, j = fx.astype(int), fz.astype(int)
            u, v = fx - i, fz - j
            return (grid[j, i] * (1 - u) + grid[j, i + 1] * u) * (1 - v) + (grid[j + 1, i] * (1 - u) + grid[j + 1, i + 1] * u) * v

        segs, covered = [], []
        for t, g in river_ways:
            if t.get("name") != a.river:
                continue
            segs.append([(p["lat"], p["lon"]) for p in g])
            if t.get("tunnel", "no") != "no":
                covered.append([geo.to_world(p["lat"], p["lon"]) for p in g])
        if not segs:
            sys.exit(f"no river named {a.river!r} in the area")
        line = [geo.to_world(*p) for p in chain(segs)[0]]
        inside = [i for i, (x, z) in enumerate(line) if -4 < x < world + 4 and -4 < z < world + 4]

        def to_edge(p_in, p_out):
            """Where the segment from a point inside the map to one outside crosses the map's edge (+4)."""
            lo_, hi_ = -4.0, world + 4.0
            t_ = 1.0
            for k_ in (0, 1):
                d_ = p_out[k_] - p_in[k_]
                if d_ > 0 and p_out[k_] > hi_:
                    t_ = min(t_, (hi_ - p_in[k_]) / d_)
                elif d_ < 0 and p_out[k_] < lo_:
                    t_ = min(t_, (lo_ - p_in[k_]) / d_)
            return (p_in[0] + (p_out[0] - p_in[0]) * t_, p_in[1] + (p_out[1] - p_in[1]) * t_)

        # The river runs off the map: the last vertices can be far apart, so the line is cut at the edge, not at
        # the last vertex inside (a gap there would let land units walk round the end of the river).
        head = [to_edge(line[inside[0]], line[inside[0] - 1])] if inside[0] > 0 else []
        tail = [to_edge(line[inside[-1]], line[inside[-1] + 1])] if inside[-1] < len(line) - 1 else []
        line = head + line[inside[0]:inside[-1] + 1] + tail
        if sample(heights, *line[0]) < sample(heights, *line[-1]):
            line = line[::-1]  # upstream first
        R = resample(line, 2.0)
        L = len(R)
        hr = sample(heights, R[:, 0], R[:, 1])
        # The river's height: median over ~30 units, never rising downstream.
        hs = np.minimum.accumulate(np.array([np.median(hr[max(0, i - 7):i + 8]) for i in range(L)]))
        side = n + 1
        vz, vx = np.mgrid[0:side, 0:side] * CELL
        px, pz = vx.ravel().astype(float), vz.ravel().astype(float)
        # Spread it over the map: Gaussian weights by distance to the river (sigma 90 units).
        sub, hsub = R[::4], hs[::4]
        D = np.empty(len(px))
        for i in range(0, len(px), 4000):
            d2 = (px[i:i + 4000, None] - sub[None, :, 0]) ** 2 + (pz[i:i + 4000, None] - sub[None, :, 1]) ** 2
            w_ = np.exp(-(d2 - d2.min(1, keepdims=True)) / (2 * 90.0 ** 2))
            D[i:i + 4000] = (w_ * hsub[None, :]).sum(1) / w_.sum(1)
        level = float(hs.min())
        heights = heights - (D.reshape(side, side) - level)
        # Other valleys must not end up under the water.
        low = heights < level + 0.3
        heights = np.maximum(heights, level + 0.3)
        water_level = level
        print(f"river {a.river}: {L * 2} units on the map, falls {hs[0] - hs[-1]:.1f} units; {low.mean() * 100:.1f}% of the land raised to stay dry")
        dry = heights.copy()
        # The bed: 1.2 under the water across the river's width, banks rising 0.9 per unit.
        w_up, w_dn = map(float, a.river_width.split(","))
        hw = (w_up + (w_dn - w_up) * np.arange(L) / max(1, L - 1)) * 0.5
        d, idx = nearest(px, pz, R)
        d, idx = d.reshape(side, side), idx.reshape(side, side)
        hwv = hw[idx]
        carved = np.minimum(heights, level - 1.2 + np.maximum(0, d - hwv) * 0.9)
        # Land across the river, per river point (0..1): culverts, where the river runs underground,
        # and bridges, each as wide (along the river) as its road, square to the river.
        land_s = np.zeros(L)
        if covered:
            cov = np.concatenate([resample(c, 1.0) for c in covered if len(c) >= 2])
            dc, _ = nearest(R[:, 0], R[:, 1], cov)
            land_s = np.maximum(land_s, (dc < 1.5).astype(float))
        for bw, pts in bridge_ways + road_ways:
            for p, q in zip(pts, pts[1:]):
                bx0, bx1, bz0, bz1 = min(p[0], q[0]), max(p[0], q[0]), min(p[1], q[1]), max(p[1], q[1])
                for r0, r1 in zip(line, line[1:]):
                    if max(r0[0], r1[0]) < bx0 or min(r0[0], r1[0]) > bx1 or max(r0[1], r1[1]) < bz0 or min(r0[1], r1[1]) > bz1:
                        continue
                    c_ = seg_cross(p, q, r0, r1)
                    if c_ is None:
                        continue
                    bridges.append((c_[0], c_[1], bw))
        arc = np.arange(L) * 2.0
        if a.river_ford_every > 0:
            # Fords: where the crossings are further apart along the river than --river-ford-every (or from either end
            # of the river), land is added evenly in between.
            at = sorted(float(arc[int(np.argmin((R[:, 0] - cx) ** 2 + (R[:, 1] - cz) ** 2))]) for cx, cz, _ in bridges)
            marks = [-a.river_ford_every * 0.5] + at + [arc[-1] + a.river_ford_every * 0.5]
            fords = 0
            for s0, s1 in zip(marks, marks[1:]):
                k_ = int((s1 - s0) // a.river_ford_every)
                for j in range(1, k_ + 1):
                    sp = s0 + (s1 - s0) * j / (k_ + 1)
                    i_ = int(np.clip(round(sp / 2.0), 0, L - 1))
                    bridges.append((float(R[i_, 0]), float(R[i_, 1]), 5.0))
                    fords += 1
            print(f"  {fords} fords added (crossings at most {a.river_ford_every:.0f} apart along the river)")
        for cx, cz, bw in bridges:
            ic = int(np.argmin((R[:, 0] - cx) ** 2 + (R[:, 1] - cz) ** 2))
            land_s = np.maximum(land_s, np.clip((bw / 2 + 1.0 - np.abs(arc - arc[ic])) / 1.0, 0, 1))
        land = land_s[idx] * np.clip(hwv + 5.0 - d, 0, 1)
        places = []
        for cx, cz, _ in bridges:
            if all(math.hypot(cx - sx, cz - sz) > 6 for sx, sz in places):
                places.append((cx, cz))
        print(f"  water level {level:.2f}, {len(places)} bridges ({len(bridges)} crossing ways), {len(covered)} culverts")
        heights = carved * (1 - land) + np.maximum(dry, level + 0.45) * land
        # Trees, buildings and bases keep off the water (the ground layer puts mud under it).
        dr = ImageDraw.Draw(ras.img("water"))
        for i in range(L - 1):
            dr.line([(R[i, 0] * Raster.RES, R[i, 1] * Raster.RES), (R[i + 1, 0] * Raster.RES, R[i + 1, 1] * Raster.RES)],
                    fill=255, width=int(round((hw[i] * 2 + 1.0) * Raster.RES)))

    needle, broad, mixed = ras.arr("needle"), ras.arr("broad"), ras.arr("mixed")
    industrial, farmyard = ras.arr("industrial"), ras.arr("farmyard")
    forest = needle | broad | mixed
    water = ras.arr("water")
    built = ras.arr("building")
    # Built-up density over ~60 m: dense blocks get town houses instead of detached ones.
    dens = blur(built, int(60 * geo.upm * Raster.RES / 2) + 1)
    print(f"forest {forest.mean() * 100:.0f}% of the map, water {water.mean() * 100:.1f}%")

    # Start positions: open (no forest, buildings, water), flat ground, one per sector around the centre.
    rs = Raster.RES
    open_ = blur(forest | water | built | industrial | ras.arr("residential"), int(30 * rs))
    hv = heights
    slope = np.zeros((n + 1, n + 1))
    slope[:-1, :-1] = np.abs(np.diff(hv, axis=0))[:, :-1] + np.abs(np.diff(hv, axis=1))[:-1, :]
    slope_b = blur(slope, 12)
    c = world / 2
    if a.starts:
        starts = [tuple(map(float, s_.split(","))) for s_ in a.starts.split(";")]
    else:
        # Each player gets open, flat, dry ground outside the towns; the places are spread as far
        # from each other as the land allows, wherever that is (nothing is mirrored).
        step, edge = 6.0, 60.0
        gx, gz = np.meshgrid(np.arange(edge, world - edge, step), np.arange(edge, world - edge, step))
        gx, gz = gx.ravel(), gz.ravel()
        pi, pj = (gx * rs).astype(int), (gz * rs).astype(int)
        town_c = dens_big[pj, pi]
        cost = open_[pj, pi] * 3.0 + slope_b[(gz / CELL).astype(int), (gx / CELL).astype(int)] * 4.0 + np.maximum(town_c - 0.02, 0) * 30.0
        if sea_mask is not None:
            # A coast: no base in the water or at the shore.
            near_sea = blur(water, int(20 * rs))[pj, pi]
            cost = cost + near_sea * 40.0
        k = a.players
        ok = np.zeros(len(gx), bool)
        for town_max, share in ((0.05, 0.5), (0.07, 0.6), (0.11, 0.75), (9.0, 1.0)):
            out_of_town = (town_c < town_max) & ((blur(water, int(20 * rs))[pj, pi] < 0.02) if sea_mask is not None else True)
            if out_of_town.sum() >= k * 8:
                ok = out_of_town & (cost <= np.quantile(cost[out_of_town], share))
                # Enough room for everyone at a fair distance?
                if ok.sum() >= k * 8:
                    break
        cx_, cz_, cc = gx[ok], gz[ok], cost[ok]
        cc = (cc - cc.min()) / max(cc.max() - cc.min(), 1e-6)
        # Farthest-point start (from a random good place), then swaps while the closest pair gets further apart.
        chosen = [int(rng.choice(np.argsort(cc)[:max(1, len(cc) // 10)].tolist()))]
        dmin = np.hypot(cx_ - cx_[chosen[0]], cz_ - cz_[chosen[0]])
        while len(chosen) < k:
            nxt = int(np.argmax(dmin * (1.0 - 0.25 * cc)))
            chosen.append(nxt)
            dmin = np.minimum(dmin, np.hypot(cx_ - cx_[nxt], cz_ - cz_[nxt]))
        for _ in range(4 * k):
            moved = False
            for m_ in range(k):
                others = [q_ for q_ in chosen if q_ != chosen[m_]]
                d_o = np.min([np.hypot(cx_ - cx_[q_], cz_ - cz_[q_]) for q_ in others], axis=0) if others else np.full(len(cx_), world)
                val = d_o * (1.0 - 0.25 * cc)
                best_ = int(np.argmax(val))
                if val[best_] > val[chosen[m_]] * 1.02:
                    chosen[m_], moved = best_, True
            if not moved:
                break
        # That layout is the widest one, and close to a ring along the edges. Keep 3/4 of its
        # spacing as the least distance and draw layouts at random from the best ground instead;
        # the winner has good sites and nobody much more crowded than the others.
        def nn_dist(sel):
            return [min(math.hypot(cx_[p_] - cx_[q_], cz_[p_] - cz_[q_]) for q_ in sel if q_ != p_) for p_ in sel]

        if k > 1:
            least = min(nn_dist(chosen)) * 0.7
            good = np.argsort(cc)[:max(k * 8, len(cc) * 2 // 3)]
            land_x, land_z = gx[out_of_town], gz[out_of_town]
            best_layout = (-1e9, chosen)
            for _ in range(600):
                # Each next player goes to one of the emptier places (not the emptiest: that is the ring again).
                sel, dg = [int(good[rng.randrange(len(good))])], None
                while len(sel) < k:
                    d_ = np.hypot(cx_[good] - cx_[sel[-1]], cz_[good] - cz_[sel[-1]])
                    dg = d_ if dg is None else np.minimum(dg, d_)
                    room = np.flatnonzero(dg >= max(least, dg.max() * 0.7))
                    if not len(room):
                        break
                    sel.append(int(good[rng.choice(room.tolist())]))
                if len(sel) == k:
                    nn = nn_dist(sel)
                    # No big stretch of good land left without a player either.
                    empty = np.min([np.hypot(land_x - cx_[q_], land_z - cz_[q_]) for q_ in sel], axis=0).max()
                    score = 2.0 * min(nn) / max(nn) - sum(cc[q_] for q_ in sel) / k - 6.0 * empty / world
                    if score > best_layout[0]:
                        best_layout = (score, sel)
            chosen = best_layout[1]
        # Numbered clockwise from the north-west, so neighbours in the lobby are neighbours on the map.
        chosen.sort(key=lambda q_: math.atan2(cz_[q_] - c, cx_[q_] - c))
        starts = [(round(float(cx_[q_]), 1), round(float(cz_[q_]), 1)) for q_ in chosen]
        near = min(math.hypot(p_[0] - q_[0], p_[1] - q_[1]) for ii, p_ in enumerate(starts) for q_ in starts[ii + 1:]) if k > 1 else 0
        print(f"closest starts {near:.0f} apart; built-up share around them {[round(float(dens_big[int(z_ * rs), int(x_ * rs)]), 3) for x_, z_ in starts]}")
    print("starts", starts)

    # Level the bases: blend to the mean height within 26 units, fading out by 46.
    for sx, sz in starts:
        ys, xs = np.mgrid[0:n + 1, 0:n + 1] * CELL
        d = np.hypot(xs - sx, ys - sz)
        mean = heights[d < 26].mean()
        t = np.clip((46 - d) / 20, 0, 1)
        t = t * t * (3 - 2 * t)
        heights = heights * (1 - t) + mean * t

    # ---------------------------------------------------------------- lakes
    # Every lake or pond of the real place keeps its own water level (the game's `lakes`): the
    # level is the height the terrain model has across it, the bed is dug under it (deeper away
    # from the shore) and the ground just around it is kept above it.
    side = n + 1
    lake_levels, lake_lab = [], np.zeros((side, side), np.int32)
    if lake_polys and not a.no_lakes:
        im = Image.new("I", (side, side), 0)
        dr = ImageDraw.Draw(im)
        for k, (outer, inner) in enumerate(lake_polys):
            for r in outer:
                if len(r) >= 3:
                    dr.polygon([(x / CELL, z / CELL) for x, z in r], fill=k + 1)
            for r in inner:
                if len(r) >= 3:
                    dr.polygon([(x / CELL, z / CELL) for x, z in r], fill=0)
        lab = np.asarray(im).astype(np.int32)
        # Polygons that touch are one lake.
        parent = list(range(len(lake_polys) + 1))

        def find(i):
            while parent[i] != i:
                parent[i] = parent[parent[i]]
                i = parent[i]
            return i

        for u, v in ((lab[:, :-1], lab[:, 1:]), (lab[:-1, :], lab[1:, :])):
            m_ = (u > 0) & (v > 0) & (u != v)
            for i, j in set(zip(u[m_].tolist(), v[m_].tolist())):
                parent[find(i)] = find(j)
        lab = np.array([find(i) for i in range(len(parent))], np.int32)[lab]
        sea = water_level if water_level is not None else float(heights.min()) - 2.0
        found = []
        for k in np.unique(lab[lab > 0]):
            m_ = lab == k
            level = float(np.median(heights[m_]))
            if m_.sum() * CELL * CELL >= a.lake_min and level > sea + 0.3:
                found.append((int(m_.sum()), int(k), level))
        found.sort(reverse=True)
        for _, k, level in found[:255]:
            lake_levels.append(round(level, 3))
            lake_lab[lab == k] = len(lake_levels)

        def grow(g):
            out = g.copy()
            for dj, di in ((0, 1), (1, 0), (0, -1), (-1, 0), (1, 1), (1, -1), (-1, 1), (-1, -1)):
                sh = np.zeros_like(g)
                sh[max(dj, 0):side + min(dj, 0), max(di, 0):side + min(di, 0)] = g[max(-dj, 0):side + min(-dj, 0), max(-di, 0):side + min(-di, 0)]
                out = np.maximum(out, sh)
            return out

        if lake_levels:
            lv = np.array([0.0] + lake_levels)
            inside = lake_lab > 0
            # Steps in from the shore (up to 3): 0.6 deep at the edge, 1.8 from the third vertex on.
            depth, core = np.zeros((side, side)), inside
            for _ in range(3):
                depth += core * 0.6
                core = ~grow(~core)
            bed = lv[lake_lab] - depth
            heights = np.where(inside, np.minimum(heights, bed), heights)
            ring = grow(grow(lake_lab))
            rim = (ring > 0) & ~inside
            heights = np.where(rim, np.maximum(heights, lv[ring] + 0.25), heights)
            print(f"lakes: {len(lake_levels)}, levels {min(lake_levels):.1f}..{max(lake_levels):.1f}, {inside.mean() * 100:.1f}% of the map")

    lake_cells_early = np.stack([lake_lab[:-1, :-1], lake_lab[:-1, 1:], lake_lab[1:, :-1], lake_lab[1:, 1:]]).max(0) > 0

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
    # Main roads: cells whose centre is on one take no building.
    for j in range(n):
        for i in range(n):
            if on_main[int((j + 0.5) * CELL * rs), int((i + 0.5) * CELL * rs)]:
                occ[j, i] = max(occ[j, i], 4)
    # Streets: a building may line one but not stand on it (no gap needed, unlike the main roads).
    # Any cell the street touches (not just its centre), so a diagonal street stays one
    # side-by-side chain of free cells (units do not cut corners).
    q4 = int(CELL * rs)
    roadc = on_street[:n * q4, :n * q4].reshape(n, q4, n, q4).sum((1, 3)) >= 2
    # Shifts (in cells) tried when a building does not fit where the real one stands: the game's
    # buildings are 2-3x the real size, so in a town most of them only fit when moved a little.
    nudges = sorted({(dx, dz) for dx in range(-2, 3) for dz in range(-2, 3)}, key=lambda d_: d_[0] ** 2 + d_[1] ** 2)
    props = []

    def snap(kind, x, z):
        f = foot[kind]
        if f % 2 == 1:
            return (math.floor(x / CELL) + 0.5) * CELL, (math.floor(z / CELL) + 0.5) * CELL
        return round(x / CELL) * CELL, round(z / CELL) * CELL

    def fits(kind, x, z, margin, near_start):
        f = foot[kind]
        x0, z0 = int(round(x / CELL - f / 2)), int(round(z / CELL - f / 2))
        if x0 - margin < 1 or z0 - margin < 1 or x0 + f + margin > n - 1 or z0 + f + margin > n - 1:
            return None
        if occ[z0 - margin:z0 + f + margin, x0 - margin:x0 + f + margin].any() or roadc[z0:z0 + f, x0:x0 + f].any():
            return None
        if any(math.hypot(x - sx, z - sz) < near_start for sx, sz in starts):
            return None
        return x0, z0

    def place(kind, x, z, yaw, owner=0, margin=1, near_start=60.0, nudge=0):
        if kind not in foot:
            return False
        x, z = snap(kind, x, z)
        f = foot[kind]
        for dx, dz in nudges:
            if dx * dx + dz * dz > nudge * nudge:
                break
            at = fits(kind, x + dx * CELL, z + dz * CELL, margin, near_start)
            if at:
                x0, z0 = at
                occ[z0:z0 + f, x0:x0 + f] = 2
                props.append({"def": kind, "pos": {"x": round(x + dx * CELL, 2), "y": round(z + dz * CELL, 2)}, "yaw": round(yaw, 4), "owner": owner})
                return True
        return False

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
        lu = "industrial" if Raster.at(industrial, x, z) else "farmyard" if Raster.at(farmyard, x, z) else ""
        dense = dens[min(int(z * rs), ras.size - 1), min(int(x * rs), ras.size - 1)] > 0.22
        kind = building_kind(t, area, lu, dense, rng)
        if kind:
            prio = 3 if kind == "church" else 2 if kind in ("science_lab", "admin_hall", "water_tower") else 1
            cand.append((prio, area, x, z, yaw, kind))
    # Landmarks first, then a random order weighted by size (u^(1/w) sampling): the prop budget
    # thins every town evenly, but big blocks and factories tend to get their place first.
    cand.sort(key=lambda c_: (-c_[0], -rng.random() ** (1.0 / (1.0 + c_[1] / 300.0))))
    for _, _, x, z, yaw, kind in cand:
        if len(props) >= a.max_props:
            break
        t_ = town_at(x, z)
        if rng.random() > a.village + (1 - a.village) * t_:
            continue
        place(kind, x, z, yaw, nudge=2 if t_ > 0.5 else 1)
    # Made-up houses (--infill): along residential streets inside the given rectangles, on both sides, small
    # houses where no real building stands within --infill-gap. Open data is sparse in some villages.
    if a.infill:
        boxes = [tuple(map(float, b_.split(","))) for b_ in a.infill]
        real = occ == 2
        gap = max(1, int(round(a.infill_gap / CELL)))
        made = 0
        irng = random.Random(a.seed * 7919 + 13)
        for kind_, width_, main_, pts_ in road_lines:
            if main_ not in (1, 3) or made >= a.infill_max:
                continue
            la_, lo_ = geo.to_latlon(*pts_[len(pts_) // 2])
            if not any(s_ <= la_ <= n_ and w_ <= lo_ <= e_ for s_, w_, n_, e_ in boxes):
                continue
            side = irng.choice((-1, 1))
            dist = irng.uniform(0.0, a.infill_step)
            for (x0_, z0_), (x1_, z1_) in zip(pts_, pts_[1:]):
                seg = math.hypot(x1_ - x0_, z1_ - z0_)
                if seg < 1e-6:
                    continue
                tx, tz = (x1_ - x0_) / seg, (z1_ - z0_) / seg
                while dist < seg and made < a.infill_max:
                    px, pz = x0_ + tx * dist, z0_ + tz * dist
                    dist += a.infill_step * irng.uniform(0.85, 1.35)
                    side = -side if irng.random() < 0.8 else side
                    if irng.random() < 0.15:
                        continue
                    off = width_ / 2 + 3.2 + irng.uniform(0.0, 2.0)
                    hx, hz = px - tz * off * side, pz + tx * off * side
                    ci, cj = int(hx / CELL), int(hz / CELL)
                    if not (gap < ci < n - gap and gap < cj < n - gap) or real[cj - gap:cj + gap + 1, ci - gap:ci + gap + 1].any():
                        continue
                    if place(irng.choice(HOUSES), hx, hz, math.atan2(tz, tx), nudge=1):
                        made += 1
        print(f"{made} made-up houses along the streets of {len(boxes)} area(s)")
    # Power pylons along the real lines (sparse).
    pyl = 0
    for t, (x, z) in points:
        if t.get("power") == "tower" and pyl < 40:
            if place("pylon", x, z, 0.0, margin=2):
                pyl += 1
    counts = {}
    for p in props:
        counts[p["def"]] = counts.get(p["def"], 0) + 1
    print(f"{len(props)} props ({pyl} pylons):", dict(sorted(counts.items(), key=lambda kv: -kv[1])))

    # Hostile outpost on the highest open hilltop (the best lookout on the map).
    hi = None
    if a.outpost:
        ox, oz = geo.to_world(*map(float, a.outpost.split(",")))
        hi = (0.0, ox, oz)
    for j in range(20, n - 20, 2):
        for i in range(20, n - 20, 2):
            if a.outpost:
                break
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
        if a.dep_area:
            la_, lo_ = geo.to_latlon(x, z)
            s_, w_, n_, e_ = map(float, a.dep_area.split(","))
            if not (s_ <= la_ <= n_ and w_ <= lo_ <= e_):
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
    ex_s = max(1.0, world / 768.0)  # larger maps spread the expansions out
    expansions = [(50, -32), (34, 52)]
    while len(expansions) < 13:
        fwd, side = exp_rng.uniform(-40, 190) * ex_s, exp_rng.uniform(-170, 170) * ex_s
        if math.hypot(fwd, side) < 45 or any(math.hypot(fwd - e[0], side - e[1]) < 30 for e in expansions):
            continue
        expansions.append((fwd, side))
    for sx, sz in starts:
        dx, dz = c - sx, c - sz
        L = math.hypot(dx, dz) or 1
        fx, fz = dx / L, dz / L
        # The generator's home deposits, turned with the start: they are laid out for a start whose
        # centre direction is (1, 1), and leave the AI's factory spot (towards the centre) free.
        th = math.atan2(fz, fx) - math.pi / 4
        for ox, oz in base:
            rx, rz = ox * math.cos(th) - oz * math.sin(th), ox * math.sin(th) + oz * math.cos(th)
            add_dep(sx + rx, sz + rz, search=6, keep=4)
        for fwd, side in expansions[:max(6, 52 // len(starts))]:
            add_dep(sx + fx * fwd - fz * side, sz + fz * fwd + fx * side)
    # ---------------------------------------------------------------- control points
    # Points of interest: town centres first (the most built-up places, on a road so units can stand
    # there), the rest around the towns where the map has no point yet. Each has two deposits.
    cp = []
    if a.points != 0:
        want = a.points if a.points > 0 else max(5, round(len(starts) * 1.5) + 3)
        spacing = world * 0.13
        cand = []
        for j in range(15, n - 15, 2):
            for i in range(15, n - 15, 2):
                x, z = (i + 0.5) * CELL, (j + 0.5) * CELL
                pj_, pi_ = int(z * rs), int(x * rs)
                if blocked[j, i] or wcell[pj_, pi_] or not (on_main[pj_, pi_] or on_street[pj_, pi_]):
                    continue
                if a.cp_area:
                    la_, lo_ = geo.to_latlon(x, z)
                    s_, w_, n_, e_ = map(float, a.cp_area.split(","))
                    if not (s_ <= la_ <= n_ and w_ <= lo_ <= e_):
                        continue
                if lake_cells_early[j - 1:j + 2, i - 1:i + 2].any() or min(math.hypot(x - sx, z - sz) for sx, sz in starts) < max(90.0, spacing):
                    continue
                cand.append((x, z, float(dens_big[pj_, pi_]), bool(on_main[pj_, pi_])))

        def far(x, z, others):
            return min([math.hypot(x - q_[0], z - q_[1]) for q_ in others] or [world])

        # In the towns: the densest places, main roads first, well apart.
        for x, z, t_, main_ in sorted(cand, key=lambda q_: -(q_[2] + 0.02 * q_[3])):
            if len(cp) >= math.ceil(want * a.cp_town) or t_ < 0.07:
                break
            if far(x, z, cp) >= spacing:
                cp.append((x, z))
        # Around them: villages and town edges, each time the place furthest from the points so far.
        fringe = [q_ for q_ in cand if 0.015 < q_[2] < 0.09] or cand
        while len(cp) < want and fringe:
            x, z, *_ = max(fringe, key=lambda q_: far(q_[0], q_[1], cp))
            if far(x, z, cp) < spacing * 0.6:
                break
            cp.append((x, z))
        for x, z in cp:
            add_dep(x + 5, z + 3)
            add_dep(x - 5, z - 3)
        print(f"{len(cp)} control points: {sum(1 for x, z in cp if town_at(x, z) > 0)} in towns")
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
        if blocked[j, i] or Raster.at(water, x, z) or Raster.at(built, x, z) or Raster.at(on_road, x, z):
            return False
        # Every base gets the same clearing (forest around one base and fields around another
        # made a big difference in AI test games): none within 55 units, thinning out to 75.
        for sx, sz in starts:
            dd = math.hypot(x - sx, z - sz)
            if dd < 55 or (dd < 75 and rng.random() < (75 - dd) / 20):
                return False
        if any(math.hypot(x - dx_, z - dz_) < 3.5 + r for dx_, dz_ in deposits):
            return False
        gx, gz = int(x // 16), int(z // 16)
        for b_ in (-1, 0, 1):
            for a_ in (-1, 0, 1):
                for fx_, fz_, half in fgrid.get((gx + a_, gz + b_), ()):
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

    # Town side streets: the paint stops where a building stands on them, so what is left are
    # the lanes between the buildings.
    if any(r_[2] == 3 for r_ in road_lines):
        kept = []
        for kind, width, level, pts in road_lines:
            if level != 3:
                kept.append((kind, width, level, pts))
                continue
            cur = []
            for p in resample(pts, 1.0):
                i, j = int(p[0] / CELL), int(p[1] / CELL)
                if 0 <= i < n and 0 <= j < n and occ[j, i] == 2:
                    if len(cur) >= 3:
                        kept.append((kind, width, level, simplify(cur, 0.2)))
                    cur = []
                else:
                    cur.append((float(p[0]), float(p[1])))
            if len(cur) >= 3:
                kept.append((kind, width, level, simplify(cur, 0.2)))
        print(f"town side streets cut by buildings: {len(road_lines)} -> {len(kept)} road lines")
        road_lines = kept

    # ---------------------------------------------------------------- ground textures
    # Each layer's mask: its area averaged over each cell, softened a little (the game blends
    # the edges with the textures' height, so they come out ragged, not blurry).
    def cells(a_, soft=1):
        f = a_.astype(np.float64)
        k = int(CELL * rs)
        f = f[:n * k, :n * k].reshape(n, k, n, k).mean(axis=(1, 3))
        return blur(f, soft) if soft else f

    slope_c = slope[:n, :n] / CELL
    ground = [
        ("grass", None),
        ("meadow", cells(ras.arr("field"))),
        ("forest_floor", cells(forest) * 0.95),
        ("dirt", np.maximum(cells(farmyard | ras.arr("dirt")), cells(ras.arr("quarry")) * 0.6)),
        ("gravel", cells(industrial | ras.arr("commercial") | ras.arr("retail"))),
        ("paving", np.clip((cells(dens, 0) - 0.12) * 4.0, 0, 1) * cells(ras.arr("residential") | ras.arr("commercial") | ras.arr("retail") | industrial)),
        ("sand", cells(ras.arr("sand"))),
        ("mud", cells(water)),
        ("stone", np.maximum(cells(rockarea), np.clip((slope_c - 0.25) / 0.25, 0, 1))),
    ]
    if ras.arr("snow").any():
        ground[6] = ("snow", cells(ras.arr("snow")))
    # Layers nobody uses are left out.
    ground = [g for g in ground if g[1] is None or g[1].max() > 0.05]
    layer_list = [{"kind": k, "scale": 0.0} for k, _ in ground]

    # ---------------------------------------------------------------- write
    out = os.path.join(ROOT, "assets", "maps", a.name)
    os.makedirs(out, exist_ok=True)
    for i in range(1, 9):
        f_ = os.path.join(out, f"layer{i}.png")
        if i < len(ground):
            Image.fromarray(np.clip(np.round(ground[i][1] * 255), 0, 255).astype(np.uint8)).save(f_)
        elif os.path.exists(f_):
            os.remove(f_)
    mn, mx = float(heights.min()), float(heights.max())
    px = np.round((heights - mn) / (mx - mn) * 65535).astype(np.uint16)
    Image.fromarray(px).save(os.path.join(out, "height.png"))
    # A cell belongs to a lake when one of its corners lies in it (the ground there is below the
    # lake's level, and no lower ground outside the rim gets flooded).
    lake_file = os.path.join(out, "lakes.png")
    lake_cells = np.zeros((n, n), np.uint8)
    if lake_levels:
        lc = np.stack([lake_lab[:-1, :-1], lake_lab[:-1, 1:], lake_lab[1:, :-1], lake_lab[1:, 1:]]).max(0)
        low = np.stack([heights[:-1, :-1], heights[:-1, 1:], heights[1:, :-1], heights[1:, 1:]]).min(0)
        lake_cells = np.where(low < np.array([-1e9] + lake_levels)[lc], lc, 0).astype(np.uint8)
        Image.fromarray(lake_cells).save(lake_file)
    elif os.path.exists(lake_file):
        os.remove(lake_file)
    m = {
        "name": a.title or a.name,
        "n": n,
        "water": round(water_level, 3) if water_level is not None else round(mn - 2.0, 3),
        "min_height": mn,
        "max_height": mx,
        "deposits": [{"x": d[0], "y": d[1]} for d in deposits],
        "starts": [{"x": s[0], "y": s[1]} for s in starts],
        "points": [{"x": p_[0], "y": p_[1]} for p_ in cp],
        "props": props,
        "seed": a.seed,
        "features": feats,
        "layers": layer_list,
        "roads": [f"{kind} {width:.2f} " + " ".join(f"{x:.2f} {z:.2f}" for x, z in pts) for kind, width, _, pts in road_lines],
    }
    if lake_levels:
        m["lakes"] = lake_levels
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
    if water_level is not None:
        wet = Image.fromarray(((heights < water_level) * 255).astype(np.uint8)).resize(img.size)
        img.paste((40, 90, 200), mask=wet)
    if lake_levels:
        img.paste((40, 90, 200), mask=Image.fromarray(((lake_cells > 0) * 255).astype(np.uint8)).resize(img.size))
    for kind, width, _, pts in road_lines:
        d.line([(x * k, z * k) for x, z in pts], fill=(60, 60, 60) if kind == "asphalt" else (140, 110, 70), width=max(1, int(width * k)))
    for f in feats:
        kind, x, z = f.split()[:3]
        x, z = float(x) * k, float(z) * k
        if fdefs[kind]["class"] == "rock":
            d.rectangle([x - 1, z - 1, x + 1, z + 1], fill=(235, 235, 225))
        else:
            d.rectangle([x - 1, z - 1, x, z], fill=(25, 70, 30))
    for p in props:
        x, z = p["pos"]["x"] * k, p["pos"]["y"] * k
        h = foot[p["def"]] * CELL * k / 2
        d.rectangle([x - h, z - h, x + h, z + h], fill=(200, 40, 40) if p["owner"] == 0 else (255, 0, 255))
    for x, z in deposits:
        d.ellipse([x * k - 2, z * k - 2, x * k + 2, z * k + 2], fill=(40, 220, 255))
    for cx, cz, *_ in bridges:
        d.ellipse([cx * k - 3, cz * k - 3, cx * k + 3, cz * k + 3], outline=(255, 220, 0), width=2)
    for x, z in cp:
        d.rectangle([x * k - 6, z * k - 6, x * k + 6, z * k + 6], outline=(255, 255, 255), width=2)
    for i, (x, z) in enumerate(starts):
        d.ellipse([x * k - 8, z * k - 8, x * k + 8, z * k + 8], outline=(255, 255, 0), width=3)
        d.text((x * k + 10, z * k - 6), str(i + 1), fill=(255, 255, 0))
    os.makedirs(os.path.join(ROOT, "tools", "out"), exist_ok=True)
    img.save(os.path.join(ROOT, "tools", "out", f"{a.name}_preview.png"))


if __name__ == "__main__":
    main()
