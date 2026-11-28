"""Defences and sensors of the playable faction: SAM launcher, radar towers, satellite uplink, watch tower and field works
(sandbags, trench, bunker), in the same look as the other buildings."""
import math

from kit import Kit, along, model
from models_buildings import annex, hazard_edge, slab, upgrade_rig


def lattice(k, x, y, w0, w1, z0, z1, mat="steel", step=0.9, leg=0.07, brace=0.035):
    """Four-legged lattice mast tapering from w0 (bottom) to w1 (top), with cross braces."""
    def at(z):
        f = (z - z0) / (z1 - z0)
        return (w0 + (w1 - w0) * f) / 2
    for sx in (-1, 1):
        for sy in (-1, 1):
            along(k, (x + sx * at(z0), y + sy * at(z0), z0), (x + sx * at(z1), y + sy * at(z1), z1), leg, mat, seg=4)
    z = z0
    while z + step <= z1 + 1e-6:
        a, b = at(z), at(z + step)
        for s in (-1, 1):
            along(k, (x - a, y + s * a, z), (x + b, y + s * b, z + step), brace, mat, seg=4)
            along(k, (x + s * a, y - a, z), (x + s * b, y + b, z + step), brace, mat, seg=4)
            k.box((2 * b, brace * 2, brace * 2), (x, y + s * b, z + step), mat)
            k.box((brace * 2, 2 * b, brace * 2), (x + s * b, y, z + step), mat)
        z += step


def dish(k, name, pos, w, h, parent=None, mat="white"):
    """Rotating radar antenna part (spun by the unit JSON): a curved reflector on a yoke."""
    p = k.part(name, pos, parent)
    x, y, z = pos
    with k.into(p):
        k.box((0.12, 0.12, 0.25), (x, y, z + 0.12), "steel_d")
        seg = 5
        for i in range(seg):
            a = (i - (seg - 1) / 2) / seg * 1.2
            k.box((w / seg + 0.01, 0.05, h), (x + math.sin(a) * w / 2.4, y - 0.15 + (1 - math.cos(a)) * w * 0.25, z + 0.3 + h / 2), mat,
                  faces={"back": "steel_d"}, rot=(0, 0, -math.degrees(a)))
        k.box((0.04, 0.4, 0.04), (x, y - 0.35, z + 0.3 + h / 2), "gun")
    return p


@model
def sam():
    """Faust SAM site: four missile canisters on a trainable launcher, a search radar on its own mast."""
    k = Kit("sam")
    with k.nohit_():
        slab(k, 3.9, 3.9, 0.12, "concrete_d")
        hazard_edge(k, 3.9, 3.9, t=0.14)
    k.box((2.4, 2.4, 0.35), (0, 0.3, 0.3), "steel", faces={"top": "steel_d"}, bevel=0.04)
    for s in (-1, 1):
        k.box((0.35, 0.35, 0.3), (s * 1.3, 1.3, 0.27), "steel_d")  # outrigger pads
    k.box((0.8, 0.9, 0.7), (-1.3, -1.3, 0.47), "steel", faces={"top": "vent", "front": "team"})  # generator
    t = k.part("Turret", (0, 0.3, 0.47))
    with k.into(t):
        k.cyl(0.7, 0.15, (0, 0.3, 0.47), "steel_d", seg=16)
        for s in (-1, 1):
            k.box((0.15, 0.6, 0.8), (s * 0.85, 0.35, 0.95), "steel", faces={"right" if s > 0 else "left": "team"})
    b = k.part("Turret_Barrel", (0, 0.45, 1.2), t)
    with k.into(b):
        for dx in (-0.4, 0.4):
            for dz in (-0.2, 0.2):
                k.box((0.36, 2.5, 0.36), (dx, -0.4, 1.2 + dz), "olive", faces={"front": "black", "back": "steel_d"}, bevel=0.02)
                k.box((0.38, 0.1, 0.38), (dx, -1.62, 1.2 + dz), "steel_d")
        k.box((1.0, 0.3, 0.6), (0, 0.6, 1.2), "steel_d")
    k.part("Turret_Muzzle", (0, -1.7, 1.2), b)
    k.box((0.16, 0.16, 2.1), (1.25, -1.25, 1.17), "steel_d")
    dish(k, "Radar", (1.25, -1.25, 2.2), 1.6, 0.6)
    k.nohit.add(k.parts[-1])
    return k


def radar_base(k, w, h):
    with k.nohit_():
        slab(k, 1.9, 1.9, 0.12, "concrete_d")
    k.box((w, w, h), (0, 0, 0.12 + h / 2), "steel", faces={"top": "steel_d", "front": "team"}, bevel=0.03)
    k.box((0.36, 0.04, 0.6), (0, -w / 2 - 0.02, 0.45), "metal_door")


@model
def radar_tower():
    """Radar Tower: a lattice mast with a rotating antenna on an equipment cabin."""
    k = Kit("radar_tower")
    radar_base(k, 1.1, 0.7)
    lattice(k, 0, 0, 0.6, 0.3, 0.82, 4.0)
    k.box((0.5, 0.5, 0.1), (0, 0, 4.05), "steel_d")
    k.cyl(0.04, 0.4, (0.2, 0.2, 4.1), "red", seg=6)
    dish(k, "Radar", (0, 0, 4.1), 1.8, 0.55)
    upgrade_rig(k, 0.7, 0.7, 2.5)
    return k


@model
def radar_tower2():
    """Radar Tower Mk2: a taller mast, a second antenna lower down, a bigger cabin."""
    k = Kit("radar_tower2")
    radar_base(k, 1.4, 0.9)
    lattice(k, 0, 0, 0.8, 0.35, 1.02, 5.4)
    k.box((0.6, 0.6, 0.1), (0, 0, 5.45), "steel_d")
    k.box((1.0, 1.0, 0.08), (0, 0, 3.4), "steel_d")  # service platform
    dish(k, "Radar", (0, 0, 5.5), 2.6, 0.7)
    dish(k, "Radar2", (0, 0, 3.45), 1.4, 0.45)
    upgrade_rig(k, 0.75, 0.75, 3.0)
    return k


@model
def radar_tower3():
    """Radar Tower Mk3: a heavy mast, a radome-capped top and three antennas."""
    k = Kit("radar_tower3")
    radar_base(k, 1.8, 1.1)
    lattice(k, 0, 0, 1.0, 0.45, 1.22, 7.0, step=1.0, leg=0.09)
    for z in (4.5, 6.0):
        k.box((1.0, 1.0, 0.08), (0, 0, z), "steel_d")
    k.box((0.7, 0.7, 0.1), (0, 0, 7.05), "steel_d")
    dish(k, "Radar", (0, 0, 7.1), 3.2, 0.8)
    dish(k, "Radar2", (0, 0, 6.05), 2.2, 0.55)
    dish(k, "Radar3", (0, 0, 4.55), 1.4, 0.45)
    k.cyl(0.04, 0.5, (0.4, 0.4, 7.1), "red", seg=6)
    return k


@model
def watch_tower():
    """Wooden watch tower: four braced timber legs, a ladder, a planked cabin under a tin roof."""
    k = Kit("watch_tower")
    top = 5.0
    for sx in (-1, 1):
        for sy in (-1, 1):
            along(k, (sx * 0.85, sy * 0.85, 0), (sx * 0.6, sy * 0.6, top), 0.09, "timber", seg=6)
    for z0 in (0.3, 1.9, 3.4):
        z1 = z0 + 1.4
        f0, f1 = 0.85 - z0 / top * 0.25, 0.85 - z1 / top * 0.25
        for s in (-1, 1):
            along(k, (-f0, s * f0, z0), (f1, s * f1, z1), 0.05, "timber", seg=4)
            along(k, (s * f0, -f0, z0), (s * f1, f1, z1), 0.05, "timber", seg=4)
    for s in (-1, 1):  # ladder
        along(k, (s * 0.15, -0.95, 0), (s * 0.15, -0.65, top), 0.03, "timber", seg=4)
    for i in range(12):
        z = 0.4 + i * 0.4
        k.box((0.3, 0.04, 0.04), (0, -0.95 + 0.3 * z / top, z), "timber")
    k.box((1.6, 1.6, 0.12), (0, 0, top + 0.06), "timber")
    for s in (-1, 1):  # planked walls, open above
        k.box((1.6, 0.06, 0.8), (0, s * 0.77, top + 0.52), "wood")
        k.box((0.06, 1.6, 0.8), (s * 0.77, 0, top + 0.52), "wood")
    for sx in (-1, 1):
        for sy in (-1, 1):
            k.box((0.08, 0.08, 0.7), (sx * 0.75, sy * 0.75, top + 1.25), "timber")
    k.prism([(-1.0, top + 1.6), (1.0, top + 1.6), (0, top + 2.0)], "y", -1.0, 1.0, "corr_d", caps={0: "wood", 1: "wood"})
    k.box((0.6, 0.03, 0.3), (0, -0.81, top + 0.6), "team")
    return k


@model
def sandbags():
    """Sandbag wall piece: courses of bags filling the cell, lower at the edges so pieces join."""
    k = Kit("sandbags")
    for i, (w, z) in enumerate(((1.98, 0.0), (1.8, 0.28), (1.55, 0.56))):
        h = 0.3 if i < 2 else 0.32
        k.box((w, w, h), (0, 0, z + h / 2), "sandbag", faces={"top": "sandbag"}, bevel=0.06)
    for i in range(3):  # a few loose bags on top
        k.box((0.4, 0.22, 0.14), (-0.45 + i * 0.45, 0.1 * (i - 1), 0.95), "sandbag", bevel=0.05, rot=(0, 0, 15 * (i - 1)))
    return k


@model
def trench():
    """Trench piece: a dug floor, timber revetment posts and low earth spoil at the corners."""
    k = Kit("trench")
    k.box((1.96, 1.96, 0.06), (0, 0, 0.03), "mud")
    for sx in (-1, 1):
        for sy in (-1, 1):
            k.box((0.55, 0.55, 0.3), (sx * 0.72, sy * 0.72, 0.15), "dirt", taper=(0.6, 0.6))
            k.cyl(0.07, 0.75, (sx * 0.88, sy * 0.88, 0), "timber", seg=6)
    for s in (-1, 1):
        k.box((1.2, 0.08, 0.06), (0, s * 0.9, 0.62), "timber")
    k.box((0.6, 1.2, 0.04), (0.3, 0, 0.07), "timber")  # duckboards
    k.box((0.04, 0.04, 0.5), (0.88, 0.88, 0.9), "timber")
    k.box((0.02, 0.24, 0.16), (0.88, 0.75, 1.05), "team")  # pennant
    return k


@model
def field_bunker():
    """Field bunker: a squat concrete pillbox with firing slits all round, sandbagged at the base, a periscope."""
    k = Kit("field_bunker")
    oct8 = [(1.8 * math.cos(a), 1.8 * math.sin(a)) for a in [math.pi / 8 + i * math.pi / 4 for i in range(8)]]
    k.prism(oct8, "z", 0, 1.45, "concrete_old", caps={1: "concrete_old"}, scale1=0.88)
    k.prism([(x * 0.86, y * 0.86) for x, y in oct8], "z", 1.45, 1.7, "concrete_old", scale1=0.85)
    for i in range(8):
        a = math.pi / 8 + i * math.pi / 4 + math.pi / 8
        r = 1.62
        k.box((0.06, 0.8, 0.18), (r * math.cos(a), r * math.sin(a), 1.05), "black", rot=(0, 0, math.degrees(a)))
    for i in range(16):
        a = i / 16 * 2 * math.pi
        k.box((0.45, 0.28, 0.2), (1.85 * math.cos(a), 1.85 * math.sin(a), 0.1), "sandbag", bevel=0.05, rot=(0, 0, math.degrees(a) + 90))
    k.cyl(0.1, 0.35, (0.5, 0.3, 1.7), "gun", seg=8)
    k.box((0.5, 0.5, 0.04), (0, 0, 1.72), "team")
    k.box((0.7, 0.05, 0.9), (0, -1.7, 0.45), "metal_door", rot=(0, 0, 0))
    return k


@model
def satellite_uplink():
    """T4 Satellite Uplink: a big dish (part Dish, turns slowly about the vertical) on a pedestal, an operations
    block, generator, fuel tank and antenna masts on a concrete pad."""
    k = Kit("satellite_uplink")
    slab(k, 10.0, 10.0, 0.2)
    hazard_edge(k, 10.0, 10.0, z=0.2)
    # pedestal
    k.cyl(1.4, 0.5, (0, 0, 0.2), "concrete_d", seg=16)
    k.cyl(0.85, 1.8, (0, 0, 0.7), "steel", seg=16, caps={1: "steel_d"})
    k.cyl(1.0, 0.2, (0, 0, 2.3), "team", seg=16)
    # operations block, generator, fuel tank
    annex(k, -3.0, 3.1, 3.4, 3.0, 2.6, floors=2, mat="plaster_w")
    k.box((3.4, 0.04, 0.3), (-3.0, 1.58, 2.3), "team")
    k.box((1.6, 1.6, 1.0), (3.2, 3.6, 0.7), "steel", faces={"top": "vent"})
    k.cyl(0.55, 1.4, (3.4, -3.3, 0.2), "tank", seg=12, caps={1: "steel_d"})
    with k.nohit_():
        for x, y, h in ((4.3, 1.0, 5.0), (-4.3, -2.0, 4.2), (4.3, -1.6, 3.4)):
            k.box((0.1, 0.1, h), (x, y, 0.2 + h / 2), "steel_d")
            k.box((0.3, 0.3, 0.04), (x, y, 0.2 + h * 0.6), "steel_d")
            k.cyl(0.05, 0.25, (x, y, 0.2 + h), "red", seg=6)
        k.cyl(0.02, 1.2, (-1.6, 2.2, 2.7), "black", seg=6)
    # the dish on a turning yoke
    d = k.part("Dish", (0, 0, 2.5))
    with k.into(d):
        k.cyl(1.2, 0.25, (0, 0, 2.5), "steel_d", seg=16)  # turntable ring
        k.box((3.2, 0.8, 0.35), (0, 0, 2.9), "steel_d")
        for s in (-1, 1):  # yoke arms
            k.box((0.32, 0.9, 2.4), (s * 1.45, 0, 4.1), "steel", faces={"left": "team", "right": "team"}, bevel=0.03)
        with k.at((0, 0, 5.0), rot=(35, 0, 0)):
            k.cyl(0.45, 0.8, (0, 0, -0.9), "steel_d", seg=10)  # hub behind the reflector
            k.cyl(0.7, 0.7, (0, 0, -0.2), "white", seg=24, r2=3.4, caps={1: "steel"})  # reflector
            k.cyl(2.4, 0.03, (0, 0, 0.5), "white", seg=24)
            for a in (0, 120, 240):  # feed struts
                with k.at((0, 0, 0), rot=(0, 0, a)):
                    along(k, (3.0, 0, 0.5), (0, 0, 2.8), 0.04, "steel_d", seg=6)
            k.cyl(0.16, 0.45, (0, 0, 2.6), "black", seg=8)  # feed horn
    return k
