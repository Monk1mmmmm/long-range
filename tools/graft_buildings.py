#!/usr/bin/env python3
"""Replaces the buildings of an existing map with those of a freshly made one, keeping everything else.

    python3 tools/graft_buildings.py NAME SCRATCH [--margin 1]

NAME is a map in assets/maps (an adaptive one too) whose terrain, ground, roads, deposits, starts, control points and
variants stay as they are; SCRATCH is the same place made again by tools/osm_map.py (e.g. with a bigger --max-props,
--village 1.0 or another --source), into another folder. The scratch map's neutral buildings are taken over where they
fit: not on a deposit of NAME, not on its roads, not touching what is kept. Kept from NAME: the hostile and player
props (the outpost bunkers) and the power pylons, towers and water towers (landmarks). The scratch map's own
pylons / outpost are not used, so the landmarks stay where they were.

Buildings near the starts of other variants are fine: `MapData::variant` clears them when that variant is played.
Delete the scratch folder afterwards."""
import json
import math
import os
import sys

import numpy as np
from PIL import Image, ImageDraw

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
CELL = 2.0
LANDMARKS = {"pylon", "comm_tower", "water_tower"}
NUDGE = 2
FINE = 4  # road raster: pixels per cell


def load(name):
    return json.load(open(os.path.join(ROOT, "assets", "maps", name, "map.json")))


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    margin = int(sys.argv[sys.argv.index("--margin") + 1]) if "--margin" in sys.argv else 1
    if "--margin" in sys.argv:
        args.remove(str(margin))
    name, scratch = args[0], args[1]
    old, new = load(name), load(scratch)
    n = old["n"]
    assert new["n"] == n, "the scratch map must have the same size"
    civ = json.load(open(os.path.join(ROOT, "assets", "data", "factions", "civilian.json")))
    foot = {u["id"]: max(1, u.get("footprint", 1)) for u in civ["units"]}

    def cells(p):
        f = foot[p["def"]]
        return int(round(p["pos"]["x"] / CELL - f / 2)), int(round(p["pos"]["y"] / CELL - f / 2)), f

    # What blocks a footprint: deposits, kept props, roads.
    occ = np.zeros((n, n), bool)
    for d in old["deposits"]:
        occ[int(d["y"] / CELL), int(d["x"] / CELL)] = True
    kept = [p for p in old["props"] if p.get("owner", 0) != 0 or p["def"] in LANDMARKS]
    for p in kept:
        x0, z0, f = cells(p)
        occ[max(0, z0):z0 + f, max(0, x0):x0 + f] = True
    img = Image.new("L", (n * FINE, n * FINE), 0)
    dr = ImageDraw.Draw(img)
    s = FINE / CELL
    for line in old["roads"]:
        t = line.split()
        w, pts = float(t[1]), list(map(float, t[2:]))
        xy = [(pts[i] * s, pts[i + 1] * s) for i in range(0, len(pts), 2)]
        dr.line(xy, fill=1, width=max(1, int(round(w * s))), joint="curve")
    road = np.array(img).reshape(n, FINE, n, FINE).sum((1, 3)) >= 2

    # Shifts (in cells) tried when a building does not fit where the real one stands, nearest first, as osm_map.py does.
    nudges = sorted({(dx, dz) for dx in range(-NUDGE, NUDGE + 1) for dz in range(-NUDGE, NUDGE + 1)}, key=lambda d_: d_[0] ** 2 + d_[1] ** 2)
    taken, skipped = [], {}
    for p in new["props"]:
        if p.get("owner", 0) != 0 or p["def"] in LANDMARKS or p["def"] == "bunker" or p["def"] not in foot:
            continue
        x0, z0, f = cells(p)
        m = margin
        why = "edge"
        for dx, dz in nudges:
            a, b = x0 + dx, z0 + dz
            if a - m < 1 or b - m < 1 or a + f + m > n - 1 or b + f + m > n - 1:
                continue
            if occ[b - m:b + f + m, a - m:a + f + m].any():
                why = "kept"
            elif road[b:b + f, a:a + f].any():
                why = "road"
            else:
                q = dict(p)
                q["pos"] = {"x": p["pos"]["x"] + dx * CELL, "y": p["pos"]["y"] + dz * CELL}
                taken.append(q)
                # Later buildings keep their distance from this one too.
                occ[b:b + f, a:a + f] = True
                break
        else:
            skipped[why] = skipped.get(why, 0) + 1
    was = sum(1 for p in old["props"] if p not in kept)
    old["props"] = kept + taken
    path = os.path.join(ROOT, "assets", "maps", name, "map.json")
    with open(path, "w") as fh:
        json.dump(old, fh, indent=2)
    print(f"{name}: {was} buildings -> {len(taken)} (+ {len(kept)} kept); not taken: {skipped}")


main()
