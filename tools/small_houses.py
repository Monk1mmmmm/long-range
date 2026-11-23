#!/usr/bin/env python3
"""Swaps some buildings of real-life maps for the small village houses (khata, khata_slate, cottage, dacha, farmhouse,
outbuilding; art/models_civilian.py).

    python3 tools/small_houses.py                 # the built-in list of maps
    python3 tools/small_houses.py MAP [SHARE] ... # MAP[:SHARE] pairs, share = fraction of the matching buildings (default 0.5)

Detached houses (house*, old_house*) are replaced by SHARE of them; with --all-small (tokmachka) the town houses
(corner_house*, row_house) and half of the little apartment buildings go too. Positions and yaw stay; the choice is a
seeded draw, so running it twice on a fresh map gives the same result (and nothing happens to a map twice: the
replaced buildings are not in the lists any more)."""
import json, os, random, sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets", "maps")
DETACHED = {"house", "house2", "house3", "house4", "house5", "house6", "house7", "house8", "house9", "old_house2", "old_house3", "old_house4"}
TOWN = {"corner_house1", "corner_house2", "corner_house3", "corner_house4", "row_house"}
SMALL = [("khata", 25), ("khata_slate", 20), ("cottage", 20), ("dacha", 10), ("farmhouse", 12), ("outbuilding", 6)]
# map: (share of detached houses, everything small)
DEFAULT = {"tokmachka": (0.95, True), "sudzha_duel": (0.7, False), "frankivsk": (0.55, False), "frankivsk_mini": (0.55, False),
           "liberec": (0.5, False), "jablonec": (0.5, False), "dubrovnik_1991": (0.4, False)}


def run(name, share, everything):
    path = os.path.join(ROOT, name, "map.json")
    d = json.load(open(path))
    rng = random.Random(sum(map(ord, name)))
    kinds, weights = zip(*SMALL)
    n = 0
    for p in d["props"]:
        c = p["def"]
        if p.get("owner", 0) != 0:
            continue
        hit = c in DETACHED and rng.random() < share
        if everything and (c in TOWN or c in DETACHED):
            hit = rng.random() < share
        elif everything and c == "apartment2":
            hit = rng.random() < 0.6
        if hit:
            p["def"] = rng.choices(kinds, weights)[0]
            n += 1
    json.dump(d, open(path, "w"), indent=2)
    print(f"{name}: {n} of {len(d['props'])} buildings are small houses now")


def main():
    args = sys.argv[1:]
    if not args:
        for n, (s, e) in DEFAULT.items():
            run(n, s, e)
        return
    for a in args:
        n, _, s = a.partition(":")
        run(n, float(s or 0.5), n == "tokmachka")


main()
