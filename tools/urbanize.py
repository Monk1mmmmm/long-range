#!/usr/bin/env python3
"""Swaps some neutral buildings of the real-life maps for city buildings of the same footprint (mall, hospital,
station, school, hotel, office tower, theatre, parking garage, market hall, fire and police station, filling station).
The footprint is the same, so the map's layout, paths and deposits do not change; only `def` of those props does.
Central types (mall, office tower, hotel, theatre, station) go to the densest spots, the rest anywhere; the same type
keeps a minimum distance from itself. Run once on a map (it replaces what it finds, so a second run finds fewer).

    python3 tools/urbanize.py [map ...]
"""
import json, math, random, sys, os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")
FP = {u["id"]: u["footprint"] for u in json.load(open(os.path.join(ROOT, "data/factions/civilian.json")))["units"]}
SKIP = {"church", "pylon", "trench", "sandbags", "bunker", "comm_tower", "fuel_tank", "refinery", "water_tower", "containers", "containers2",
        "farm", "farmhouse", "plant", "outbuilding"}
# what each size may replace
POOL = {2: {"grocery", "apartment2", "row_house", "corner_house1", "corner_house2", "corner_house3", "corner_house4",
            "old_house2", "old_house3", "old_house4"} | {f"house{i}" for i in range(2, 10)} | {"house"},
        4: {"apartment", "admin_building", "science_lab", "residential_tower"},
        6: {"admin_hall", "warehouse"}}
CENTRAL = {"mall", "office_tower", "hotel", "theatre", "train_station"}
SIZE_OF = {"mall": 6, "hospital": 6, "train_station": 6, "school": 4, "hotel": 4, "office_tower": 4, "theatre": 4, "parking_garage": 4,
           "market_hall": 4, "fire_station": 4, "police_station": 2, "gas_station": 2}
SPACING = {"mall": 110, "hospital": 90, "train_station": 120, "school": 55, "hotel": 60, "office_tower": 45, "theatre": 120, "parking_garage": 45,
           "market_hall": 70, "fire_station": 80, "police_station": 55, "gas_station": 50}
# map -> {type: count}
PLAN = {
    "dubrovnik_1991": dict(hospital=1, mall=1, train_station=1, hotel=6, theatre=1, market_hall=2, school=3, parking_garage=2, fire_station=2, police_station=4, gas_station=3),
    "liberec": dict(mall=5, hospital=3, train_station=2, hotel=5, office_tower=6, theatre=2, school=6, market_hall=3, fire_station=4, parking_garage=5, police_station=8, gas_station=8),
    "jablonec": dict(mall=1, hospital=1, train_station=1, hotel=2, office_tower=2, theatre=1, school=3, market_hall=2, fire_station=2, parking_garage=2, police_station=4, gas_station=4),
    "mostyska": dict(school=3, market_hall=2, fire_station=2, hotel=1, police_station=3, gas_station=4),
    "sudzha_duel": dict(hospital=1, train_station=1, school=2, market_hall=1, fire_station=1, hotel=1, police_station=2, gas_station=3),
    "tokmachka": dict(hospital=1, school=1, fire_station=1, market_hall=1, police_station=1, gas_station=2),
    "frankivsk": dict(hospital=1, office_tower=5, hotel=3, school=4, theatre=1, parking_garage=3, market_hall=2, fire_station=3, police_station=3, gas_station=3),
    "frankivsk_mini": dict(hospital=1, office_tower=3, hotel=2, school=3, theatre=1, parking_garage=2, fire_station=2, police_station=2, gas_station=2),
}


def run(name):
    p = os.path.join(ROOT, "maps", name, "map.json")
    j = json.load(open(p, encoding="utf-8"))
    props = j["props"]
    rng = random.Random(name)
    cands = {s: [i for i, q in enumerate(props) if q.get("owner") == 0 and q["def"] in POOL[s] and FP.get(q["def"]) == s] for s in POOL}
    # density: neighbours within 30 units
    cell = 30.0
    grid = {}
    for q in props:
        grid.setdefault((int(q["pos"]["x"] // cell), int(q["pos"]["y"] // cell)), 0)
        grid[(int(q["pos"]["x"] // cell), int(q["pos"]["y"] // cell))] += 1

    def dens(i):
        gx, gy = int(props[i]["pos"]["x"] // cell), int(props[i]["pos"]["y"] // cell)
        return sum(grid.get((gx + dx, gy + dy), 0) for dx in (-1, 0, 1) for dy in (-1, 0, 1))
    placed = {}
    done = {}
    # biggest first so they get the best sites
    for t in sorted(PLAN[name], key=lambda t: -SIZE_OF[t]):
        s = SIZE_OF[t]
        pool = [i for i in cands[s] if props[i]["def"] in POOL[s]]
        rng.shuffle(pool)
        if t in CENTRAL:
            pool.sort(key=lambda i: -dens(i))
        got = 0
        for i in pool:
            if got >= PLAN[name][t]:
                break
            x, y = props[i]["pos"]["x"], props[i]["pos"]["y"]
            if any(math.hypot(x - a, y - b) < SPACING[t] for a, b in placed.get(t, [])):
                continue
            props[i]["def"] = t
            cands[s].remove(i)
            placed.setdefault(t, []).append((x, y))
            got += 1
        done[t] = got
    open(p, "w", encoding="utf-8", newline="").write(json.dumps(j, indent=2, ensure_ascii=False))
    print(name, {t: f"{done[t]}/{n}" for t, n in PLAN[name].items()})


if __name__ == "__main__":
    for n in (sys.argv[1:] or PLAN):
        run(n)
