#!/usr/bin/env python3
"""Packs more of the real buildings into a town centre of a finished real-life map.

    python3 tools/fill_town.py NAME --bbox S,W,N,E --center LAT,LON --radius UNITS [--nudge 3] [--road-share 0.4]

The map's own buildings stay. Every real building (Overture data cached by tools/osm_map.py --source overture, same bbox)
whose centre is within RADIUS world units of the centre and that has no game building within its reach gets one, in
the dense-town kinds (corner houses, apartment blocks, halls), biggest first: at its place or moved up to --nudge cells,
one free cell kept round it, off the map's roads (a footprint cell counts as road when --road-share of it is road),
deposits and the buildings already there. Where the big kind does not fit a little house may. Run
tools/osm_map.py --source overture with the same bbox first if the cache is missing.
The streets of these maps are drawn wide (about 60 m), so the blocks between them are small: this is what fits."""
import argparse
import importlib.util
import json
import math
import os
import random

import numpy as np
from PIL import Image, ImageDraw

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
CELL = 2.0
FINE = 4
LANDMARKS = {"pylon", "comm_tower", "water_tower"}
BIG = ["apartment", "apartment2", "apartment2", "residential_tower"]


def osm_module():
    spec = importlib.util.spec_from_file_location("osm_map", os.path.join(ROOT, "tools", "osm_map.py"))
    m = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(m)
    return m


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("name")
    ap.add_argument("--bbox", required=True)
    ap.add_argument("--center", required=True, help="LAT,LON of the town centre")
    ap.add_argument("--radius", type=float, required=True, help="world units")
    ap.add_argument("--nudge", type=int, default=3)
    ap.add_argument("--road-share", type=float, default=0.4, help="share of a cell that must be road for it to count as road")
    ap.add_argument("--seed", type=int, default=1)
    a = ap.parse_args()
    om = osm_module()
    s, w, nn, e = map(float, a.bbox.split(","))
    lat0, lon0 = (s + nn) / 2, (w + e) / 2
    km = max((nn - s) * 110574.0, (e - w) * 111320.0 * math.cos(math.radians(lat0))) / 1000.0
    d = json.load(open(os.path.join(ROOT, "assets", "maps", a.name, "map.json")))
    n = d["n"]
    geo = om.Geo(lat0, lon0, km, n * CELL)
    cx, cz = geo.to_world(*map(float, a.center.split(",")))
    cache = [f for f in os.listdir(os.path.join(ROOT, "tools", "out", "osm_cache")) if f.startswith("overture_") and f.endswith(".json")]
    els = None
    for f in sorted(cache):
        b = f[:-5].split("_")[-4:]
        if abs(float(b[0]) - geo.s) < 0.01 and abs(float(b[1]) - geo.w) < 0.01:
            els = json.load(open(os.path.join(ROOT, "tools", "out", "osm_cache", f)))
    assert els, "no cached Overture data for this bbox: run osm_map.py --source overture first"
    civ = json.load(open(os.path.join(ROOT, "assets", "data", "factions", "civilian.json")))
    foot = {u["id"]: max(1, u.get("footprint", 1)) for u in civ["units"]}

    def cells(p):
        f = foot[p["def"]]
        return int(round(p["pos"]["x"] / CELL - f / 2)), int(round(p["pos"]["y"] / CELL - f / 2)), f

    occ = np.zeros((n, n), bool)
    for q in d["deposits"]:
        occ[int(q["y"] / CELL), int(q["x"] / CELL)] = True
    for p in d["props"]:
        x0, z0, f = cells(p)
        occ[max(0, z0):z0 + f, max(0, x0):x0 + f] = True
    img = Image.new("L", (n * FINE, n * FINE), 0)
    dr = ImageDraw.Draw(img)
    sc = FINE / CELL
    for line in d["roads"]:
        t = line.split()
        wd, pts = float(t[1]), list(map(float, t[2:]))
        dr.line([(pts[i] * sc, pts[i + 1] * sc) for i in range(0, len(pts), 2)], fill=1, width=max(1, int(round(wd * sc))), joint="curve")
    road = np.array(img).reshape(n, FINE, n, FINE).sum((1, 3)) >= max(1, int(round(a.road_share * FINE * FINE)))

    cand = []
    for el in els:
        if el.get("_part") != "buildings":
            continue
        ring = [geo.to_world(g["lat"], g["lon"]) for g in el["geometry"]]
        x = sum(p[0] for p in ring) / len(ring)
        z = sum(p[1] for p in ring) / len(ring)
        if math.hypot(x - cx, z - cz) > a.radius:
            continue
        yaw, au = om.orient(list(ring))
        area = au / geo.upm ** 2
        if area >= 70:
            cand.append((area, x, z, yaw, el.get("tags", {})))
    cand.sort(key=lambda c_: -c_[0])
    rng = random.Random(a.seed)
    nudges = sorted({(dx, dz) for dx in range(-a.nudge, a.nudge + 1) for dz in range(-a.nudge, a.nudge + 1)}, key=lambda d_: d_[0] ** 2 + d_[1] ** 2)

    def snap(f, x, z):
        if f % 2:
            return int(math.floor(x / CELL)) , int(math.floor(z / CELL))
        return int(round(x / CELL)) - f // 2, int(round(z / CELL)) - f // 2

    def put(kind, x, z, yaw):
        f = foot[kind]
        x0, z0 = snap(f, x, z)
        for dx, dz in nudges:
            ax, az = x0 + dx, z0 + dz
            if ax - 1 < 1 or az - 1 < 1 or ax + f + 1 > n - 1 or az + f + 1 > n - 1:
                continue
            if occ[az - 1:az + f + 1, ax - 1:ax + f + 1].any() or road[az:az + f, ax:ax + f].any():
                continue
            occ[az:az + f, ax:ax + f] = True
            d["props"].append({"def": kind, "pos": {"x": (ax + f / 2) * CELL, "y": (az + f / 2) * CELL}, "yaw": round(yaw, 4), "owner": 0})
            return True
        return False

    # Buildings that already stand are not doubled: a real building with a game building on its footprint is done.
    placed = 0
    before = len(d["props"])
    for area, x, z, yaw, tags in cand:
        i, j = int(x / CELL), int(z / CELL)
        if occ[max(0, j - 1):j + 2, max(0, i - 1):i + 2].any():
            continue
        kind = om.building_kind(tags, area, "", True, rng)
        if kind is None or kind not in foot:
            kind = rng.choice(["corner_house1", "corner_house2", "corner_house3", "corner_house4", "row_house"]) if area > 120 else None
        if kind is None:
            continue
        if kind in ("residential_tower", "apartment", "admin_hall", "warehouse", "plant", "refinery", "farm", "science_lab", "admin_building"):
            ok = put(kind, x, z, yaw) or put(rng.choice(["apartment2", "corner_house2", "corner_house3"]), x, z, yaw)
        else:
            ok = put(kind, x, z, yaw)
        if not ok and foot[kind] > 1:
            ok = put(rng.choice(["khata", "khata_slate", "cottage"]), x, z, yaw)
        placed += ok
    with open(os.path.join(ROOT, "assets", "maps", a.name, "map.json"), "w") as fh:
        json.dump(d, fh, indent=2)
    print(f"{a.name}: {len(cand)} real buildings within {a.radius:g} of {cx:.0f},{cz:.0f}; {placed} added ({before} -> {len(d['props'])} props)")


main()
