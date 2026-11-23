#!/usr/bin/env python3
"""Merge maps that are the same place with different start positions (player counts) into one adaptive map.

    python3 tools/merge_variants.py OUT_NAME --title "Title" DIR_A DIR_B [DIR_C ...] [--attach-radius 26]

Each DIR is a premade map folder (4 players, 6 players...). The result (assets/maps/OUT_NAME) holds the union of
their starts, a `variants` entry per input (its starts, in lobby slot order, and its own control points), and
`attached` entries that make deposits conditional (see `MapData::variant` / `MapData::adapt`):
  * a deposit within --attach-radius of a start of its variant is attached to that start: it is only there when
    somebody plays on that start (SupCom adaptive maps: the extractors beside a spawn);
  * a deposit only some variants have is attached to that variant.
Terrain: the start levelling of every variant is applied (median of the variants = the natural ground, then every
variant's changes on top). Trees, rocks and buildings come from the first input; where it had cleared the ground
for its own starts they are filled in from another variant. Roads, ground layers, lakes come from the first input.
"""
import argparse, json, math, os, shutil, sys
import numpy as np
from PIL import Image

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "maps")
CLEAR_FEATURES, CLEAR_PROPS = 26.0, 40.0


def load(d):
    j = json.load(open(os.path.join(d, "map.json")))
    px = np.array(Image.open(os.path.join(d, "height.png"))).astype(np.float64)
    h = j["min_height"] + px / 65535.0 * (j["max_height"] - j["min_height"])
    return j, h


def key(p):
    return (round(p["x"], 2), round(p["y"], 2))


def dist(a, b):
    return math.hypot(a[0] - b[0], a[1] - b[1])


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("out")
    ap.add_argument("dirs", nargs="+")
    ap.add_argument("--title", default="")
    ap.add_argument("--attach-radius", type=float, default=26.0)
    a = ap.parse_args()
    dirs = [d if os.path.isdir(d) else os.path.join(ROOT, d) for d in a.dirs]
    maps = [load(d) for d in dirs]
    js = [m[0] for m in maps]
    base = js[0]
    for j in js:
        assert j["n"] == base["n"] and j["water"] == base["water"], "the maps must be the same place"

    # ---- starts and variants
    starts, variants = [], []
    for j in js:
        idx = []
        for s in j["starts"]:
            k = next((i for i, t in enumerate(starts) if dist((t["x"], t["y"]), (s["x"], s["y"])) < 0.5), None)
            if k is None:
                starts.append(s)
                k = len(starts) - 1
            idx.append(k)
        v = {"starts": idx}
        if j.get("points"):
            v["points"] = j["points"]
        variants.append(v)
    spos = [(s["x"], s["y"]) for s in starts]

    # ---- deposits
    members = {}
    order = []
    for vi, j in enumerate(js):
        for d in j["deposits"]:
            k = key(d)
            if k not in members:
                members[k] = (d, set())
                order.append(k)
            members[k][1].add(vi)
    deposits, attached = [], []
    n_start = n_var = n_shared = 0
    for k in order:
        d, ms = members[k]
        deposits.append({"x": d["x"], "y": d["y"]})
        own = sorted({i for vi in ms for i in variants[vi]["starts"]})
        near = min(own, key=lambda i: dist(spos[i], k), default=None)
        if near is not None and dist(spos[near], k) <= a.attach_radius:
            attached.append({"pos": {"x": d["x"], "y": d["y"]}, "start": near})
            n_start += 1
        elif len(ms) < len(js):
            attached.append({"pos": {"x": d["x"], "y": d["y"]}, "variant": min(ms)})
            n_var += 1
        else:
            n_shared += 1

    # ---- terrain: every variant's levelling on the natural ground
    hs = [m[1] for m in maps]
    if len(hs) > 1 and any(np.abs(h - hs[0]).max() > 0.02 for h in hs[1:]):
        nat = np.median(np.stack(hs), axis=0) if len(hs) > 2 else hs[0]
        if len(hs) == 2:
            print("warning: two variants with different terrain, the first one's terrain is kept where they differ")
        comp = nat.copy()
        for h in hs:
            m = np.abs(h - nat) > 0.02
            comp[m] = h[m]
    else:
        comp = hs[0]
    mn, mx = float(comp.min()), float(comp.max())
    mx = max(mx, mn + 0.01)
    px = np.round((comp - mn) / (mx - mn) * 65535.0).astype(np.uint16)

    # ---- trees, rocks, buildings: the first input's, filled in round its own starts
    def fill(items, pos, radius, extra):
        out = list(items)
        for b in js[0]["starts"]:
            bp = (b["x"], b["y"])
            src = next((i for i in range(1, len(js)) if all(dist(bp, (s["x"], s["y"])) >= radius * 2 for s in js[i]["starts"])), None)
            if src is None:
                continue
            out += [x for x in extra(src) if dist(pos(x), bp) < radius]
        return out

    fpos = lambda line: tuple(map(float, line.split()[1:3]))
    features = fill(base["features"], fpos, CLEAR_FEATURES, lambda i: js[i]["features"])
    props = fill(base["props"], lambda p: (p["pos"]["x"], p["pos"]["y"]), CLEAR_PROPS, lambda i: [p for p in js[i]["props"] if p.get("owner", 0) == 0])

    out = os.path.join(ROOT, a.out)
    os.makedirs(out, exist_ok=True)
    Image.fromarray(px).save(os.path.join(out, "height.png"))
    for f in os.listdir(dirs[0]):
        if f.startswith("layer") or f == "lakes.png":
            shutil.copy(os.path.join(dirs[0], f), os.path.join(out, f))
    res = dict(base)
    res.update(name=a.title or base["name"], min_height=mn, max_height=mx, deposits=deposits, starts=starts, props=props, features=features, variants=variants, attached=attached)
    res.pop("points", None)
    if not any(v.get("points") for v in variants) and base.get("points"):
        res["points"] = base["points"]
    json.dump(res, open(os.path.join(out, "map.json"), "w"), indent=2)
    print(f"{a.out}: {len(starts)} starts, {len(variants)} variants {[len(v['starts']) for v in variants]}, "
          f"{len(deposits)} deposits ({n_start} attached to a start, {n_var} to a variant, {n_shared} shared), "
          f"{len(props)} props, {len(features)} trees and rocks")


main()
