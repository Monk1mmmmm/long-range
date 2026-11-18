#!/usr/bin/env python3
"""Writes the "Relaxed" data preset (assets/data/presets/Relaxed) from the built-in data.

A calmer game: fewer hidden rules, more forgiving fights and economy, while keeping the core
(long range, slow aim, scouting and radar decide who shoots). Run it again after the built-in
data changes so the preset follows: python3 tools/relaxed_preset.py
"""
import json
import pathlib
import shutil

ROOT = pathlib.Path(__file__).resolve().parent.parent / "assets" / "data"
OUT = ROOT / "presets" / "Relaxed"

# Rules: (key, value). Nested keys as "a.b".
RULES = {
    "friendly_fire": False,                  # no holding fire because own units stand in the way
    "start_mass": 1000,                      # 650: a safer opening (commander storage raised to match)
    "start_energy": 6000,                    # 4000
    "power_stall_pump_factor": 0.6,          # 0.25: an energy stall hurts mass much less
    "radar_fade_secs": 25.0,                 # 10: radar blips stay longer
    "sight_blocked_by_reclaimables": False,  # trees, rocks, wrecks no longer hide units
    "sight_blocked_by_buildings": False,     # nor do buildings (hills still do)
    "near_sight": 12,                        # 6: nothing sneaks up unseen right next to a unit
    "armor_front": 1.0,                      # 0.6 / 1.0 / 1.5: no damage by hit angle
    "armor_side": 1.0,
    "armor_rear": 1.0,
    "splash_armored": 1.0,                   # 0.5: armored units take splash like any other
    "debris.land_damage": 0.0,               # falling debris hurts nobody
    "garrison_eject_damage": 0.15,           # 0.4
    "cp_bleed": 0.5,                         # 1: control point games leave time to come back
}

MOBILE_HP = 1.3      # mobile units live longer: more time to react before a group is lost
COMMANDER_HP = 1.6   # losing the commander ends the game: make sniping it much harder
RADAR_UPKEEP = 0.5   # scouting is cheaper to keep running
HOLD = {"at_trooper": 0.0, "pike": 1.5, "bohdana": 3.0}  # set-up times (built-in 3 / 3 / 5)
# No combat engineers: the units that build field works go, and so does everything only they build.
FIELD_BUILD = "field"


def rules():
    r = json.loads((ROOT / "rules.json").read_text(encoding="utf-8"))
    for key, val in RULES.items():
        d = r
        *path, last = key.split(".")
        for p in path:
            d = d[p]
        d[last] = val
    # No armor: every armor class takes every damage type in full.
    for table in r["armor"].values():
        for k in table:
            table[k] = 1.0
    r["_comment_preset"] = "Relaxed preset, made by tools/relaxed_preset.py from the built-in data."
    return r


def faction(f):
    for u in f["units"]:
        # Infantry enters no buildings: structures lose their garrison (mobile transports keep theirs).
        if not u.get("move"):
            u.pop("transport", None)
    if not f.get("playable"):
        return f
    f["units"] = [u for u in f["units"]
                  if FIELD_BUILD not in u.get("build", {}).get("categories", []) and u.get("built_by") != FIELD_BUILD]
    for u in f["units"]:
        u.pop("reactive", None)  # no reactive (dynamic) armor
        if u.get("move"):
            u["hp"] = round(u["hp"] * (COMMANDER_HP if u["id"] == f.get("commander") else MOBILE_HP))
        if u["id"] == f.get("commander"):
            # Resources are clamped to storage every tick: the larger start needs room.
            u["eco"]["store_mass"] = max(u["eco"].get("store_mass", 0), RULES["start_mass"])
            u["eco"]["store_energy"] = max(u["eco"].get("store_energy", 0), RULES["start_energy"])
        # Infantry is seen as soon as it is in view (no spotting delay).
        u.pop("spot_time", None)
        u.pop("spot_time_moving", None)
        if u.get("radar") and u.get("eco", {}).get("upkeep"):
            u["eco"]["upkeep"] = round(u["eco"]["upkeep"] * RADAR_UPKEEP, 2)
        for w in u.get("weapons", []):
            # Unlimited rounds: no rearming to keep track of (a suicide drone's one warhead stays).
            if not w.get("suicide"):
                for k in ("ammo", "rearm", "rearm_mass"):
                    w.pop(k, None)
            if u["id"] in HOLD and "hold" in w:
                if HOLD[u["id"]] > 0:
                    w["hold"] = HOLD[u["id"]]
                else:
                    del w["hold"]
    return f


def write(path, obj):
    path.parent.mkdir(parents=True, exist_ok=True)
    text = json.dumps(obj, indent=1, ensure_ascii=False) + "\n"
    path.write_bytes(text.replace("\n", "\r\n").encode("utf-8"))


def main():
    if OUT.exists():
        shutil.rmtree(OUT)
    write(OUT / "rules.json", rules())
    for fp in sorted((ROOT / "factions").glob("*.json")):
        write(OUT / "factions" / fp.name, faction(json.loads(fp.read_text(encoding="utf-8"))))
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
