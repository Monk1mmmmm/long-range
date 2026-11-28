"""Ships of the playable faction: small grey boats with a team-coloured stripe. Origin at the waterline centre
(z = 0), front -y; the hull reaches below the water. Turrets follow the vehicles' naming: Turret (yaw) /
Turret_Barrel (pitch) / Turret_Muzzle."""
import math

from kit import Kit, model, along


def hull(k, l, beam, deck=0.8, keel=-0.5, bow=0.3, stern=0.0):
    """Pointed hull from a plan: a tapering underwater part, the deck-level part above, a team stripe."""
    f, b, h = -l / 2, l / 2, beam / 2
    plan = [(0, f), (h * 0.62, f + l * bow * 0.55), (h, f + l * bow * 1.25), (h, b - stern), (-h, b - stern),
            (-h, f + l * bow * 1.25), (-h * 0.62, f + l * bow * 0.55)]
    k.prism(plan, "z", 0.1, keel, "steel_d", scale1=0.55)
    k.prism(plan, "z", 0.1, deck, "steel", caps={1: "steel_d"}, scale1=0.94)
    k.prism([(x * 0.94, y) for x, y in plan], "z", deck, deck + 0.06, "steel_d", scale1=0.98)
    # team stripe along the topsides, bulwark rails
    for s in (-1, 1):
        k.box((0.05, l * 0.62, 0.16), (s * h * 0.935, 0.12 * l, deck * 0.55), "team")
        k.box((0.05, l * 0.6, 0.14), (s * h * 0.9, 0.12 * l, deck + 0.1), "steel", faces={"top": "steel_d"})


def gun_turret(k, pos, r, twin, barrel_len, bore, gap=0.1, mount_z=0.3):
    """Turret on a pedestal: shield, barrel part with the gun(s), muzzle marker at the tip."""
    x, y, z = pos
    t = k.part("Turret", pos)
    with k.into(t):
        k.cyl(r, 0.14, (x, y, z), "steel_d", seg=12)
        oct8 = [(x + r * 0.9 * math.cos(a), y + r * 1.0 * math.sin(a)) for a in [math.pi / 8 + i * math.pi / 4 for i in range(8)]]
        k.prism(oct8, "z", z + 0.14, z + mount_z + 0.1, "steel", caps={1: "team"}, scale1=0.85)
    b = k.part("Turret_Barrel", (x, y - r * 0.8, z + mount_z), t)
    with k.into(b):
        k.box((gap * 2 + bore * 4 + 0.05 if twin else bore * 4, 0.34, bore * 5), (x, y - r * 0.85, z + mount_z), "steel_d", bevel=0.01)
        for dx in ((-gap, gap) if twin else (0.0,)):
            along(k, (x + dx, y - r * 0.85, z + mount_z), (x + dx, y - r * 0.85 - barrel_len, z + mount_z), bore, "gun", seg=8)
            along(k, (x + dx, y - r * 0.85 - barrel_len, z + mount_z), (x + dx, y - r * 0.85 - barrel_len - 0.12, z + mount_z), bore * 1.5, "gun", seg=8)
    k.part("Turret_Muzzle", (x, y - r * 0.85 - barrel_len - 0.12, z + mount_z), b)
    return t


@model
def patrol_boat():
    """T1 patrol boat: small fast hull, wheelhouse aft of the bow turret, twin heavy machine guns."""
    k = Kit("patrol_boat")
    hull(k, 6.0, 2.2, deck=0.8, bow=0.32)
    # wheelhouse, with a team roof, and a low aft deck box
    prof = [(-0.75, 0.86), (-0.75, 1.25), (-0.4, 1.6), (0.7, 1.6), (0.7, 0.86)]
    k.prism(prof, "x", -0.6, 0.6, "steel", side_mats=["glass", "steel", "steel", "steel_d", "steel_d"], caps={0: "glass", 1: "glass"})
    k.box((1.3, 1.25, 0.06), (0, 0.17, 1.62), "team", bevel=0.01)
    k.box((0.9, 0.9, 0.3), (0, 1.9, 0.98), "steel_d", faces={"top": "vent"}, bevel=0.02)  # engine hatch
    for s in (-1, 1):
        k.cyl(0.16, 0.3, (s * 0.45, 2.55, 0.86), "gun", seg=8)  # exhausts
    with k.nohit_():
        k.cyl(0.015, 1.0, (0.3, 0.9, 1.62), "black", seg=6)  # mast
        k.box((0.3, 0.03, 0.1), (0.3, 0.9, 2.55), "gun")
        k.box((0.04, 0.04, 1.0), (-0.4, 2.2, 0.9), "black")
    gun_turret(k, (0, -1.55, 0.86), 0.46, True, 0.85, 0.035, gap=0.12)
    return k


@model
def gunboat():
    """T2 gunboat: longer hull, two-deck superstructure with a mast and radar, 30 mm autocannon forward."""
    k = Kit("gunboat")
    hull(k, 8.5, 3.0, deck=1.0, keel=-0.7, bow=0.3)
    # superstructure amidships: lower deck, bridge, funnel
    k.box((2.1, 3.0, 0.85), (0, 0.9, 1.5), "steel", faces={"top": "steel_d", "front": "glass"}, bevel=0.03)
    k.box((2.1, 0.06, 0.2), (0, -0.62, 1.7), "team")
    k.box((1.6, 1.5, 0.7), (0, 0.5, 2.2), "tank", faces={"top": "team", "front": "glass"}, bevel=0.03)
    for s in (-1, 1):
        k.box((0.06, 1.2, 0.3), (s * 0.83, 0.5, 2.25), "glass")
    k.cyl(0.32, 0.9, (0, 1.9, 1.9), "steel_d", seg=10, caps={1: "black"})  # funnel
    k.cyl(0.34, 0.14, (0, 1.9, 2.45), "team", seg=10)
    # aft gun deck: a boat crane-free stern with life raft and a small mount
    k.cyl(0.4, 0.35, (0.0, 3.1, 1.0), "tank", seg=10)
    k.box((0.9, 0.7, 0.3), (0, 3.2, 1.06), "steel_d", faces={"top": "vent"})
    with k.nohit_():
        k.box((0.08, 0.08, 1.6), (0, 0.2, 2.9), "steel_d")  # mast
        k.box((1.3, 0.06, 0.26), (0, 0.2, 3.65), "white", faces={"back": "steel_d"})  # radar bar
        k.box((0.14, 0.14, 0.1), (0, 0.2, 3.58), "black")
        k.box((0.04, 0.04, 0.9), (0.5, 0.0, 3.0), "black")
        k.cyl(0.015, 1.2, (-0.5, 1.4, 2.5), "black", seg=6)
    gun_turret(k, (0, -2.25, 1.06), 0.62, False, 1.75, 0.05, mount_z=0.34)
    return k
