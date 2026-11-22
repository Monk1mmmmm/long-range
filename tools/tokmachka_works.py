#!/usr/bin/env python3
"""Adds the Russian field works of the Mala Tokmachka - Robotyne - Verbove front to assets/maps/tokmachka.

Run after tools/osm_map.py tokmachka --center 47.48,35.915 --km 17 --cells 384 --players 4 --source overture --no-lakes
(see tools/README.md). The lines were traced by eye from a DeepState map picture (pixels of that picture, georeferenced
by the village labels) and are neutral (owner 0) trench pieces, sandbags at the ends and bunkers at the strongpoints.
Re-running first removes the works of an earlier run.
"""
import json, math, os, sys
import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
PATH = os.path.join(ROOT, "assets", "maps", "tokmachka", "map.json")
KM, LAT, LON, WORLD = 17.0, 47.48, 35.915, 768.0
mx, my = 111320.0 * math.cos(math.radians(LAT)), 110574.0
upm = WORLD / (KM * 1000)
north, west = LAT + KM * 500 / my, LON - KM * 500 / mx

# picture pixel <-> lat/lon, fitted to village labels
LAB = [((250, 47), (47.5341, 35.8973)), ((30, 433), (47.4465, 35.8348)), ((510, 527), (47.428, 35.9905)),
       ((500, 170), (47.5045, 35.9878)), ((50, 545), (47.411, 35.8356))]
A = np.array([[p[0], 1] for p, _ in LAB], float)
cx = np.linalg.lstsq(A, np.array([g[1] for _, g in LAB]), rcond=None)[0]
A = np.array([[p[1], 1] for p, _ in LAB], float)
cy = np.linalg.lstsq(A, np.array([g[0] for _, g in LAB]), rcond=None)[0]

def world(px, py):
    lon, lat = cx[0] * px + cx[1], cy[0] * py + cy[1]
    return (lon - west) * mx * upm, (north - lat) * my * upm

TRENCHES = [
    [(316, 230), (317, 262)], [(342, 235), (343, 252)], [(397, 268), (396, 282)],
    [(520, 272), (525, 300), (521, 328)],
    [(303, 382), (322, 398), (338, 408), (366, 418)],
    [(262, 382), (282, 381)], [(244, 366), (250, 396)], [(180, 352), (232, 351)], [(96, 356), (152, 350)],
    [(170, 428), (200, 426)], [(125, 432), (136, 440)], [(22, 478), (62, 482)],
    [(410, 436), (432, 446)], [(405, 440), (412, 456)], [(438, 466), (472, 462)],
    [(340, 470), (336, 500), (338, 548)], [(344, 460), (420, 462)],
    [(558, 395), (560, 420)], [(550, 420), (556, 440)],
]
# Northern lines (world units, not picture pixels): in front of Mala Tokmachka and along the fields up to the E-W road
NORTH = [
    [(20, 200), (70, 205), (120, 214)], [(150, 215), (210, 208), (270, 212)], [(300, 200), (350, 196), (400, 202)],
    [(430, 214), (480, 208), (540, 220)], [(560, 200), (610, 192), (660, 204)],
    [(190, 22), (240, 18), (290, 26)], [(500, 30), (550, 24), (600, 34)],
    [(60, 250), (60, 290)], [(350, 230), (352, 270)], [(520, 240), (524, 285)],
]
BUNKERS = [(377, 288), (338, 408), (546, 412), (550, 523)]

m = json.load(open(PATH, encoding="utf-8"))
WORKS = {"trench", "sandbags", "field_bunker"}
m["props"] = [p for p in m["props"] if p["def"] not in WORKS | {"bunker"}]
# the random hilltop outpost of osm_map.py is gone with its bunkers; the comm tower may stay

SIZE = {"bunker": 4.0, "trench": 2.0, "sandbags": 2.0}
def free(x, z, r):
    if not (6 < x < WORLD - 6 and 6 < z < WORLD - 6):
        return False
    for p in m["props"]:
        if math.hypot(p["pos"]["x"] - x, p["pos"]["y"] - z) < r + 3.2:
            return False
    return True

roads = []
for line in m["roads"]:
    t = line.split()
    w = float(t[1]); v = list(map(float, t[2:]))
    roads.append((w, list(zip(v[0::2], v[1::2]))))
def on_road(x, z):
    for w, pts in roads:
        for (ax, az), (bx, bz) in zip(pts, pts[1:]):
            dx, dz = bx - ax, bz - az
            L2 = dx * dx + dz * dz or 1.0
            t = max(0.0, min(1.0, ((x - ax) * dx + (z - az) * dz) / L2))
            if math.hypot(x - ax - t * dx, z - az - t * dz) < w / 2 + 1.2:
                return True
    return False

new, pieces = [], []
for line in TRENCHES + NORTH:
    pts = [world(*p) for p in line] if line not in NORTH else line
    for (ax, az), (bx, bz) in zip(pts, pts[1:]):
        L = math.hypot(bx - ax, bz - az)
        yaw = math.atan2(bz - az, bx - ax)
        k = max(1, int(L // 2))
        for i in range(k + 1):
            x, z = ax + (bx - ax) * i / k, az + (bz - az) * i / k
            x, z = round(x / 2) * 2 + 1.0, round(z / 2) * 2 + 1.0  # centre of a 2-unit cell
            if any((x, z) == q for q in pieces) or not free(x, z, 1.0) or on_road(x, z):
                continue
            pieces.append((x, z))
            new.append({"def": "trench", "pos": {"x": x, "y": z}, "yaw": round(yaw, 4), "owner": 0})
    # sandbags across both ends
    for (x, z), (ox, oz) in ((pts[0], pts[1]), (pts[-1], pts[-2])):
        yaw = math.atan2(z - oz, x - ox)
        sx, sz = round((x + math.cos(yaw) * 2) / 2) * 2 + 1.0, round((z + math.sin(yaw) * 2) / 2) * 2 + 1.0
        if (sx, sz) not in pieces and free(sx, sz, 1.0) and not on_road(sx, sz):
            pieces.append((sx, sz))
            new.append({"def": "sandbags", "pos": {"x": sx, "y": sz}, "yaw": round(yaw + math.pi / 2, 4), "owner": 0})
m["props"] += new
nb = 0
for px, py in BUNKERS:
    x, z = world(px, py)
    for dx, dz in [(0, 0), (4, 0), (-4, 0), (0, 4), (0, -4), (6, 6), (-6, -6), (6, -6), (-6, 6)]:
        bx, bz = round((x + dx) / 2) * 2, round((z + dz) / 2) * 2
        if free(bx, bz, 2.0) and not on_road(bx, bz):
            m["props"].append({"def": "bunker", "pos": {"x": float(bx), "y": float(bz)}, "yaw": 0.0, "owner": 0}); nb += 1
            break
# trees and rocks off the works
keep = []
spots = np.array([(p["pos"]["x"], p["pos"]["y"]) for p in new]) if new else np.zeros((0, 2))
for f in m["features"]:
    t = f.split()
    x, z = float(t[1]), float(t[2])
    if len(spots) and np.min(np.hypot(spots[:, 0] - x, spots[:, 1] - z)) < 1.8:
        continue
    keep.append(f)
print(len(m["features"]) - len(keep), "trees/rocks removed;", len(new), "trench/sandbag pieces,", nb, "bunkers")
m["features"] = keep
json.dump(m, open(PATH, "w", encoding="utf-8"), indent=2)
