"""Buildings of the playable faction: industrial, like the Mining Town map's (corrugated sheds, concrete,
painted steel), team colour on cladding and panels."""
import math

from kit import Kit, model


def slab(k, w, l, h=0.12, mat="concrete", loc=(0, 0)):
    k.box((w, l, h), (loc[0], loc[1], h / 2), mat)


def pumpjack(k, s=1.0, x=0.0, y=0.0, suffix="", phase=0.0, period=60):
    """Nodding-donkey oil pump facing -y. Adds the parts Beam / Crank (+ suffix) and their 'work' keys."""
    with k.at((x, y, 0), scale=s):
        k.box((0.5, 1.5, 0.16), (0, 0.05, 0.08), "steel_d")
        for sx in (-1, 1):
            k.box((0.06, 0.07, 0.86), (sx * 0.17, -0.05, 0.58), "steel", rot=(0, sx * 8, 0))
        k.box((0.12, 0.25, 0.06), (0, -0.05, 1.0), "steel_d")
        k.box((0.42, 0.28, 0.4), (0, 0.28, 0.36), "team", faces={"top": "steel_d"}, bevel=0.02)
        k.box((0.3, 0.32, 0.24), (0, 0.62, 0.28), "steel_d", faces={"top": "vent"})
        k.cyl(0.05, 0.28, (0, -0.62, 0.16), "gun", seg=8)
        k.box((0.2, 0.06, 0.06), (0, -0.62, 0.32), "red")
    pv = (x, y - 0.05 * s, 1.05 * s)
    beam = k.part("Beam" + suffix, pv)
    with k.into(beam), k.at((x, y, 0), scale=s):
        k.box((0.1, 1.2, 0.1), (0, 0.0, 1.1), "steel", faces={"top": "team"})
        head = [(-0.56, 1.22), (-0.67, 1.1), (-0.7, 0.93), (-0.66, 0.75), (-0.52, 0.82), (-0.5, 1.1), (-0.46, 1.22)]
        k.prism(head, "x", -0.09, 0.09, "team", caps={0: "steel", 1: "steel"})
        k.box((0.02, 0.02, 0.5), (0, -0.66, 0.6), "black")
        k.box((0.38, 0.08, 0.06), (0, 0.58, 1.03), "steel_d")
    crank = k.part("Crank" + suffix, (x, y + 0.55 * s, 0.38 * s))
    with k.into(crank), k.at((x, y, 0), scale=s):
        for sx in (-1, 1):
            k.box((0.05, 0.4, 0.1), (sx * 0.24, 0.55, 0.38), "steel_d")
            k.box((0.07, 0.22, 0.26), (sx * 0.24, 0.75, 0.38), "gun")
        k.cyl(0.05, 0.46, (-0.23, 0.55, 0.38), "gun", axis="x", seg=8)
    n = 12
    k.key("work", beam, [(1 + i * period / n, None, (16 * math.sin(2 * math.pi * (i / n + phase)), 0, 0)) for i in range(n + 1)])
    k.key("work", crank, [(1 + i * period / n, None, (-360 * (i / n) - 360 * phase, 0, 0)) for i in range(n + 1)])


def upgrade_rig(k, x, y, h):
    """Scaffold below ground at rest; the 'upgrade' animation raises it with the upgrade's progress."""
    rig = k.part("UpgradeRig", (x, y, -h - 0.3))
    with k.into(rig), k.at((x, y, -h - 0.3)):
        for sx in (-1, 1):
            for sy in (-1, 1):
                k.box((0.06, 0.06, h), (sx * 0.32, sy * 0.32, h / 2), "hazard")
        z = 0.4
        while z < h:
            k.box((0.7, 0.7, 0.04), (0, 0, z), "hazard")
            z += 0.5
        k.box((0.1, 1.5, 0.08), (0, -0.4, h + 0.04), "yellow")
        k.box((0.02, 0.02, 0.7), (0, -1.1, h - 0.3), "black")
    k.key("upgrade", rig, [(1, None, None), (30, (x, y, 0), (0, 0, 0)), (60, (x, y, 0), (0, 0, 180))])
    return rig


def shed(k, w, l, wall_h, roof_h, wall="corr_team", roof="corr", plinth=0.5, gable=None, loc=(0, 0), overhang=0.12):
    """Industrial shed: walls w (x) by l (y), a concrete plinth, a pitched roof with its ridge along y."""
    x, y = loc
    gable = gable or wall
    k.box((w, l, wall_h - plinth), (x, y, plinth + (wall_h - plinth) / 2), wall, faces={"top": "steel_d", "bottom": "steel_d"})
    k.box((w + 0.06, l + 0.06, plinth), (x, y, plinth / 2), "concrete")
    tri = [(x - w / 2, wall_h), (x + w / 2, wall_h), (x, wall_h + roof_h)]
    k.prism(tri, "y", y - l / 2, y + l / 2, gable, caps={0: gable, 1: gable})
    a = math.atan2(roof_h, w / 2)
    half = (w / 2 + overhang) / math.cos(a)
    for s in (-1, 1):
        cx = x + s * (w / 2 + overhang) / 2 - s * overhang / 2 + s * 0.0
        cx = x + s * ((w / 2 + overhang) / 2 - overhang / 2 + overhang / 2) - s * overhang / 2
        mid = (x + s * (w / 2 - overhang) / 2 + s * 0.0, wall_h + roof_h / 2 - overhang * math.tan(a) / 2)
        k.box((half, l + 2 * overhang, 0.06), (x + s * (w / 2 + overhang) / 2 * (1 - 0) - s * overhang / 2 * 0 + 0 * cx, y, 0),
              roof, rot=(0, 0, 0)) if False else None
        # slab from the eave (x +- (w/2 + o)) to the ridge
        ex, ez = x + s * (w / 2 + overhang), wall_h - overhang * math.tan(a)
        rx, rz = x, wall_h + roof_h
        with k.at(((ex + rx) / 2, y, (ez + rz) / 2 + 0.03), rot=(0, s * math.degrees(a), 0)):
            k.box((half, l + 2 * overhang, 0.06), (0, 0, 0), roof, faces={"bottom": "steel_d"})
    k.box((0.14, l + 2 * overhang, 0.08), (x, y, wall_h + roof_h + 0.05), "steel_d")  # ridge cap


def hazard_edge(k, w, l, loc=(0, 0), z=0.12, t=0.16):
    x, y = loc
    for s in (-1, 1):
        k.box((w, t, 0.012), (x, y + s * (l / 2 - t / 2), z + 0.006), "hazard")
        k.box((t, l - 2 * t, 0.012), (x + s * (w / 2 - t / 2), y, z + 0.006), "hazard")


def rollup_door(k, x, y, z, w, h, normal_y=-1):
    """Roll-up door on a wall facing -y (at y), with a hazard-striped frame."""
    k.box((w + 0.24, 0.05, h + 0.12), (x, y + normal_y * 0.02, z + (h + 0.12) / 2), "hazard")
    k.box((w, 0.06, h), (x, y + normal_y * 0.04, z + h / 2), "metal_door")
    k.box((w + 0.3, 0.3, 0.22), (x, y + normal_y * 0.15, z + h + 0.18), "steel_d")  # roll box


# Land factories leave units by the side facing the rally point: a roll-up door in every wall. Blender -y is the
# game's +z, so "s" is the front; the game opens the door of the exit side only (animations gate_e/w/s/n).
GATE_DIR = {"s": (0, -1), "n": (0, 1), "e": (1, 0), "w": (-1, 0)}


def gates(k, doors, speed=1):
    """doors: {side: [(x, y, w, h), ...]}, (x, y) the door's centre on the wall face. One part and one
    animation per side that rolls its panels up."""
    for side, lst in doors.items():
        nx, ny = GATE_DIR[side]
        x0, y0 = lst[0][0], lst[0][1]
        part = k.part("Gate_" + side.upper(), (x0, y0, 0.0))
        for x, y, w, h in lst:
            k.box((w + 0.24 if nx == 0 else 0.05, 0.05 if nx == 0 else w + 0.24, h + 0.12), (x + nx * 0.02, y + ny * 0.02, (h + 0.12) / 2), "hazard")
            k.box((w + 0.3 if nx == 0 else 0.3, 0.3 if nx == 0 else w + 0.3, 0.22), (x + nx * 0.15, y + ny * 0.15, h + 0.18), "steel_d")  # roll box
        with k.into(part):
            for x, y, w, h in lst:
                k.box((w if nx == 0 else 0.06, 0.06 if nx == 0 else w, h), (x + nx * 0.04, y + ny * 0.04, h / 2), "metal_door")
        up = (x0, y0, lst[0][3] * 0.9)
        k.key("gate_" + side, part, [(1, None, None), (15, up, None), (40, up, None), (55, None, None)])


def stack(k, x, y, r, h, z0=0.0, mat="steel", band=True):
    k.cyl(r, h, (x, y, z0), mat, seg=12, caps={1: "black"})
    k.cyl(r * 1.12, 0.12, (x, y, z0 + h - 0.12), "steel_d", seg=12)
    if band:
        k.cyl(r * 1.04, 0.25, (x, y, z0 + h * 0.75), "team", seg=12)


def annex(k, x, y, w, l, h, floors=2, mat="plaster", face=-1):
    """Small office block with windows on its front (face -1: -y) and sides, flat roof."""
    k.box((w, l, h), (x, y, h / 2), mat, faces={"top": "roof_tar"})
    k.box((w + 0.1, l + 0.1, 0.12), (x, y, h + 0.04), "concrete")
    fh = h / floors
    k.windows((x, y + face * l / 2, h / 2), (0, face, 0), w, h, floors, max(int(w / 0.8), 1), win=(0.42, fh * 0.48), sill=fh * 0.3, lit=0.15)
    for s in (-1, 1):
        k.windows((x + s * w / 2, y, h / 2), (s, 0, 0), l, h, floors, max(int(l / 0.9), 1), win=(0.42, fh * 0.48), sill=fh * 0.3, seed=3 + s)


@model
def pump():
    k = Kit("pump")
    with k.nohit_():
        slab(k, 1.9, 1.9, 0.1)
    k.box((1.9, 0.18, 0.02), (0, 0.86, 0.11), "hazard")
    pumpjack(k)
    k.cyl(0.22, 0.55, (0.62, -0.55, 0.1), "white", seg=12, caps={1: "steel"})
    k.box((0.46, 0.05, 0.08), (0.62, -0.55, 0.45), "team")
    upgrade_rig(k, -0.62, -0.55, 1.5)
    return k


@model
def pump2():
    k = Kit("pump2")
    with k.nohit_():
        slab(k, 1.95, 1.95, 0.12)
        hazard_edge(k, 1.95, 1.95, t=0.1)
    pumpjack(k, 1.12, -0.25, 0.08)
    k.cyl(0.3, 0.95, (0.6, -0.42, 0.12), "white", seg=14, caps={1: "steel"})
    k.cyl(0.31, 0.18, (0.6, -0.42, 0.72), "team", seg=14)
    k.box((0.04, 0.04, 0.95), (0.9, -0.42, 0.6), "steel_d")  # ladder rail
    k.cyl(0.18, 0.55, (0.62, 0.48, 0.12), "steel", seg=12, caps={1: "steel_d"})
    k.box((0.75, 0.06, 0.06), (0.22, -0.72, 0.2), "gun")
    k.box((0.06, 0.9, 0.06), (0.62, 0.02, 0.98), "gun")
    upgrade_rig(k, 0.55, 0.45, 1.8)
    return k


@model
def pump3():
    k = Kit("pump3")
    with k.nohit_():
        slab(k, 1.98, 1.98, 0.14, "concrete_d")
        hazard_edge(k, 1.98, 1.98, z=0.14, t=0.1)
    pumpjack(k, 0.95, -0.45, -0.05, "_L")
    pumpjack(k, 0.95, 0.45, -0.05, "_R", phase=0.5)
    # derrick mast at the back
    for sx in (-1, 1):
        k.box((0.05, 0.05, 1.7), (sx * 0.22, 0.78, 0.98), "hazard", rot=(0, -sx * 5, 0))
    for z in (0.55, 0.95, 1.35):
        k.box((0.46 - z * 0.12, 0.04, 0.04), (0, 0.78, z), "yellow")
    k.box((0.26, 0.22, 0.12), (0, 0.78, 1.86), "team")
    k.box((1.7, 0.36, 0.3), (0, 0.78, 0.3), "steel", faces={"top": "vent"})
    k.box((1.72, 0.37, 0.08), (0, 0.78, 0.38), "team")
    return k


@model
def energy_storage():
    """Electrical substation transformer with radiators, bushings and a conservator tank."""
    k = Kit("energy_storage")
    with k.nohit_():
        slab(k, 1.9, 1.9, 0.1)
        k.box((1.9, 1.9, 0.01), (0, 0, 0.105), "concrete_d") if False else None
    k.box((0.9, 0.7, 0.85), (0, 0, 0.53), "team", faces={"top": "steel"}, bevel=0.02)
    for s in (-1, 1):
        for i in range(5):
            k.box((0.22, 0.03, 0.62), (s * 0.58, -0.24 + i * 0.12, 0.5), "steel_d")
        k.box((0.04, 0.6, 0.05), (s * 0.68, 0, 0.84), "steel_d")
    for i, x in enumerate((-0.28, 0, 0.28)):
        k.cyl(0.05, 0.12, (x, -0.15, 0.95), "steel_d", seg=8)
        for j in range(4):
            k.cyl(0.075, 0.03, (x, -0.15, 1.07 + j * 0.07), "copper", seg=10)
            k.cyl(0.045, 0.04, (x, -0.15, 1.1 + j * 0.07), "copper", seg=8)
        k.cyl(0.02, 0.1, (x, -0.15, 1.36), "gun", seg=6)
    k.cyl(0.13, 0.65, (-0.32, 0.22, 1.13), "steel", axis="x", seg=12)
    for x in (-0.25, 0.25):
        k.box((0.05, 0.05, 0.2), (x, 0.22, 1.0), "steel_d")
    # fence posts and battery cabinets
    for s in (-1, 1):
        k.box((0.5, 0.36, 0.5), (s * 0.55, 0.68, 0.35), "steel", faces={"front": "vent"})
        k.box((0.52, 0.38, 0.05), (s * 0.55, 0.68, 0.62), "team")
    for x in (-0.88, 0.88):
        for y in (-0.88, 0.88):
            k.box((0.04, 0.04, 0.5), (x, y, 0.35), "gun")
    k.box((0.04, 1.76, 0.03), (-0.88, 0, 0.56), "gun")
    k.box((0.04, 1.76, 0.03), (0.88, 0, 0.56), "gun")
    k.box((1.76, 0.04, 0.03), (0, -0.88, 0.56), "gun")
    k.box((0.3, 0.02, 0.22), (0, -0.37, 0.6), "yellow")  # warning sign
    return k


@model
def mass_storage():
    """Two horizontal storage tanks on saddles, a ladder and a walkway."""
    k = Kit("mass_storage")
    with k.nohit_():
        slab(k, 1.9, 1.9, 0.08)
    for x in (-0.45, 0.45):
        k.cyl(0.36, 1.5, (x, -0.75, 0.48), "white", axis="y", seg=16, caps={0: "steel", 1: "steel"})
        k.cyl(0.365, 0.16, (x, -0.1, 0.48), "team", axis="y", seg=16)
        k.cyl(0.365, 0.16, (x, 0.35, 0.48), "team", axis="y", seg=16)
        for y in (-0.5, 0.5):
            k.box((0.6, 0.12, 0.26), (x, y, 0.21), "steel_d")
        k.cyl(0.07, 0.1, (x, 0.0, 0.83), "steel_d", seg=8)
    k.box((0.24, 1.5, 0.04), (0, 0, 0.88), "steel_d")  # walkway
    for y in (-0.7, 0.7):
        k.box((0.04, 0.04, 0.85), (0, y, 0.47), "gun")
    k.box((0.04, 1.5, 0.03), (0.11, 0, 1.05), "yellow")
    k.box((0.04, 1.5, 0.03), (-0.11, 0, 1.05), "yellow")
    return k


@model
def power_plant():
    """Gas turbine power station: turbine hall, two stacks, cooling fans and transformers."""
    k = Kit("power_plant")
    with k.nohit_():
        slab(k, 5.8, 5.8, 0.12)
        hazard_edge(k, 5.8, 5.8, t=0.14)
    shed(k, 2.6, 3.4, 1.4, 0.4, wall="corr_team", roof="corr", plinth=0.35, loc=(-0.9, 0.3))
    rollup_door(k, -0.9, -1.4, 0.12, 1.0, 0.9)
    for i, y in enumerate((-0.5, 0.6)):
        stack(k, 0.75, y, 0.28, 3.0, 0.12)
        k.box((0.9, 0.45, 0.35), (0.25, y, 0.6), "steel_d")  # exhaust duct
    # cooling fan bank
    k.box((1.0, 2.4, 0.55), (2.0, 0.6, 0.39), "steel", faces={"top": "steel_d"})
    for y in (-0.15, 0.6, 1.35):
        k.cyl(0.33, 0.06, (2.0, y, 0.66), "black", seg=16)
        k.cyl(0.36, 0.08, (2.0, y, 0.66), "steel", seg=16) if False else None
        k.cyl(0.06, 0.07, (2.0, y, 0.66), "steel_d", seg=8)
    # transformers
    for x in (-2.1, -1.2, -0.3):
        k.box((0.6, 0.5, 0.6), (x, -2.15, 0.42), "team", faces={"top": "steel"})
        for j in range(3):
            k.cyl(0.04, 0.3, (x - 0.18 + j * 0.18, -2.15, 0.72), "copper", seg=8)
    k.box((0.04, 2.2, 0.04), (-1.2, -2.15, 1.25), "gun")
    for x in (-2.3, -0.1):
        k.box((0.06, 0.06, 1.15), (x, -2.15, 0.7), "gun")
    # pipe rack
    k.box((0.1, 2.0, 0.08), (0.75, 1.9, 0.9), "gun")
    k.cyl(0.08, 2.0, (0.75, 0.9, 1.0), "steel", axis="y", seg=8)
    annex(k, 1.9, -1.9, 1.3, 1.0, 1.0, floors=1, mat="plaster_w")
    return k


def factory_apron(k, w, l, y):
    with k.nohit_():
        k.box((w, l, 0.1), (0, y, 0.05), "concrete", faces={"top": "concrete"})
        hazard_edge(k, w, l, (0, y), z=0.1, t=0.18)


@model
def land_factory():
    """Vehicle plant: a corrugated assembly shed in team colours with a roll-up door, an office annex and a
    concrete apron in front."""
    k = Kit("land_factory")
    factory_apron(k, 7.6, 2.4, -2.7)
    shed(k, 5.0, 5.4, 2.5, 0.8, loc=(0.4, 1.0))
    # a door in every wall (the office stands in the front corner, clear of the side doors)
    gates(k, {"s": [(0.4, -1.7, 2.6, 1.9)], "n": [(0.4, 3.7, 2.6, 1.9)], "w": [(-2.1, 0.9, 2.2, 1.9)], "e": [(2.9, 0.9, 2.2, 1.9)]})
    annex(k, -3.0, -2.0, 1.5, 2.2, 1.6, floors=2)
    k.box((0.7, 0.04, 0.22), (-3.0, -3.13, 1.35), "team")  # sign
    for y in (0.0, 2.2):
        stack(k, 2.1, y, 0.14, 1.0, 3.0, "steel_d", band=False)
    k.box((1.0, 1.2, 0.5), (-2.9, 2.4, 0.25), "steel", faces={"top": "vent"})  # generator
    k.box((0.3, 0.3, 0.9), (-3.4, 2.9, 0.45), "steel_d") if False else None
    upgrade_rig(k, -3.0, 2.6, 3.4)
    return k


@model
def land_factory2():
    """T2 vehicle plant: two bays under twin roofs, a paint shop stack and a bigger office."""
    k = Kit("land_factory2")
    factory_apron(k, 7.8, 2.2, -2.85)
    shed(k, 3.0, 5.6, 2.8, 0.7, loc=(-0.6, 0.9))
    shed(k, 3.0, 5.6, 2.8, 0.7, loc=(2.4, 0.9), wall="corr")
    gates(k, {"s": [(-0.6, -1.9, 2.2, 2.1), (2.4, -1.9, 2.2, 2.1)], "n": [(-0.6, 3.7, 2.2, 2.1), (2.4, 3.7, 2.2, 2.1)],
              "w": [(-2.1, 1.9, 2.2, 2.1)], "e": [(3.9, 1.9, 2.2, 2.1)]})
    annex(k, -3.15, -2.2, 1.5, 2.8, 2.4, floors=3)
    stack(k, -2.6, 4.4, 0.22, 3.6)
    k.box((1.4, 1.0, 0.7), (-3.1, 3.0, 0.35), "corr_d", faces={"top": "vent"}) if False else None
    for x in (-0.6, 2.4):
        k.box((0.8, 0.8, 0.3), (x, 1.6, 3.1), "steel", faces={"top": "vent"})
    upgrade_rig(k, -3.6, 4.4, 3.8)
    return k


@model
def land_factory3():
    """T3 vehicle plant: the T2 plant with a third bay, an overhead gantry crane and a test ramp."""
    k = Kit("land_factory3")
    factory_apron(k, 7.9, 2.0, -2.95)
    for x, wall in ((-2.4, "corr_team"), (0.3, "corr"), (3.0, "corr_team")):
        shed(k, 2.6, 5.6, 3.2, 0.6, loc=(x, 1.0), wall=wall)
    gates(k, {"s": [(x, -1.8, 2.0, 2.4) for x in (-2.4, 0.3, 3.0)], "n": [(x, 3.8, 2.0, 2.4) for x in (-2.4, 0.3, 3.0)],
              "w": [(-3.7, 1.0, 2.2, 2.4)], "e": [(4.3, 1.0, 2.2, 2.4)]})
    # gantry crane over the yard
    for x in (-3.8, 3.8):
        k.box((0.18, 0.18, 3.0), (x, -2.6, 1.5), "yellow")
    k.box((7.8, 0.3, 0.3), (0, -2.6, 3.0), "yellow", faces={"front": "hazard"})
    k.box((0.6, 0.5, 0.35), (-1.0, -2.6, 2.75), "steel_d")
    k.box((0.02, 0.02, 1.2), (-1.0, -2.6, 2.0), "black")
    stack(k, -3.3, 3.4, 0.22, 4.2)
    stack(k, 3.3, 3.4, 0.22, 4.2)
    for x in (-2.4, 0.3, 3.0):
        k.box((0.8, 0.8, 0.3), (x, 1.8, 3.5), "steel", faces={"top": "vent"})
    return k


def hangar(k, w, l, h, loc=(0, 0), wall="corr_team", roof="corr", seg=10):
    """Arched hangar, open side facing -y; returns the door's world centre bottom."""
    x, y = loc
    arc = [(x + w / 2 * math.cos(math.pi * i / seg), h * 0.35 + (h * 0.65) * math.sin(math.pi * i / seg)) for i in range(seg + 1)]
    prof = [(x + w / 2, 0.0)] + arc + [(x - w / 2, 0.0)]
    sides = ["concrete"] + [roof] * seg + ["concrete", "hidden"]
    sides = [roof] * (seg + 2) + ["concrete_d"]
    k.prism(prof, "y", y - l / 2, y + l / 2, roof, caps={0: wall, 1: wall}, side_mats=sides)
    for s in (-1, 1):
        k.box((0.15, l, h * 0.3), (x + s * (w / 2 + 0.02), y, h * 0.15), "concrete")
    return (x, y - l / 2, 0.0)


def landing_pad(k, cx, cy, r):
    with k.nohit_():
        k.cyl(r, 0.1, (cx, cy, 0), "concrete", seg=24, smooth=False)
        k.cyl(r * 0.82, 0.012, (cx, cy, 0.1), "yellow", seg=24, smooth=False)
        k.cyl(r * 0.76, 0.014, (cx, cy, 0.1), "concrete", seg=24, smooth=False)
        k.box((r * 0.14, r * 0.9, 0.02), (cx - r * 0.3, cy, 0.11), "white")
        k.box((r * 0.14, r * 0.9, 0.02), (cx + r * 0.3, cy, 0.11), "white")
        k.box((r * 0.6, r * 0.14, 0.02), (cx, cy, 0.11), "white")


def control_tower(k, x, y, h, radar_name="Radar", cab=0.9):
    k.box((0.7, 0.7, h), (x, y, h / 2), "plaster_w", faces={"top": "roof_tar"})
    k.windows((x, y - 0.35, h / 2), (0, -1, 0), 0.7, h - 0.4, max(int(h / 0.9), 1), 1, win=(0.3, 0.4), sill=0.3)
    k.cyl(cab * 0.55, 0.12, (x, y, h), "steel_d", seg=8, smooth=False)
    k.cyl(cab * 0.5, 0.5, (x, y, h + 0.12), "glass", seg=8, r2=cab * 0.56, smooth=False)
    k.cyl(cab * 0.6, 0.1, (x, y, h + 0.62), "team", seg=8, smooth=False)
    k.cyl(0.05, 0.35, (x, y, h + 0.72), "gun", seg=6)
    rad = k.part(radar_name, (x, y, h + 1.07))
    with k.into(rad):
        k.box((0.9, 0.06, 0.22), (x, y + 0.04, h + 1.07), "white", faces={"back": "steel_d"})
        k.box((0.08, 0.1, 0.1), (x, y - 0.02, h + 1.07), "gun")
    return rad


@model
def air_factory():
    """Airfield: arched hangar in team colours with a sliding door, a landing pad and a control tower."""
    k = Kit("air_factory")
    hangar(k, 4.6, 4.4, 3.0, loc=(0.6, 1.5))
    door = k.part("Door", (0.6, -0.72, 0.0))
    with k.into(door):
        k.box((3.4, 0.08, 2.0), (0.6, -0.76, 1.0), "metal_door")
        k.box((3.5, 0.1, 0.1), (0.6, -0.78, 2.0), "hazard")
    k.key("rolloff", door, [(1, None, None), (15, (0.6, -0.72, 1.7), None), (40, (0.6, -0.72, 1.7), None), (55, None, None)])
    landing_pad(k, 0.6, -2.5, 1.45)
    control_tower(k, -3.1, 1.2, 2.2)
    k.nohit.add(k.parts[-1])
    annex(k, -3.1, 2.9, 1.4, 1.4, 1.0, floors=1, mat="plaster_w")
    k.box((0.5, 0.5, 0.6), (3.4, -1.3, 0.3), "steel", faces={"top": "vent"})  # fuel pump
    k.cyl(0.32, 1.2, (3.4, 0.2, 0.42), "white", axis="y", seg=12, caps={0: "steel", 1: "steel"})  # fuel tank
    upgrade_rig(k, 3.3, 3.0, 3.6)
    return k


@model
def air_factory2():
    """T2 airfield: a bigger hangar with a second pad, a taller tower and a fuel farm."""
    k = Kit("air_factory2")
    hangar(k, 5.0, 4.6, 3.4, loc=(0.9, 1.6))
    door = k.part("Door", (0.9, -0.72, 0.0))
    with k.into(door):
        k.box((3.8, 0.08, 2.3), (0.9, -0.76, 1.15), "metal_door")
        k.box((3.9, 0.1, 0.1), (0.9, -0.78, 2.3), "hazard")
    k.key("rolloff", door, [(1, None, None), (15, (0.9, -0.72, 2.0), None), (40, (0.9, -0.72, 2.0), None), (55, None, None)])
    landing_pad(k, -1.6, -2.6, 1.25)
    landing_pad(k, 1.6, -2.6, 1.25)
    control_tower(k, -3.2, 1.5, 3.0)
    k.nohit.add(k.parts[-1])
    for y in (2.9, 3.6):
        k.cyl(0.3, 1.2, (-3.2, y - 0.6, 0.4), "white", axis="y", seg=12, caps={0: "steel", 1: "steel"}) if False else None
    for x in (-3.5, -2.8):
        k.cyl(0.3, 0.9, (x, 3.4, 0.0), "white", seg=12, caps={1: "steel"})
        k.cyl(0.31, 0.15, (x, 3.4, 0.6), "team", seg=12)
    upgrade_rig(k, 3.6, 3.4, 4.2)
    return k


@model
def air_factory3():
    """T3 airbase: twin hangars, a radome tower and three pads."""
    k = Kit("air_factory3")
    hangar(k, 3.7, 4.6, 3.6, loc=(-1.75, 1.6))
    hangar(k, 3.7, 4.6, 3.6, loc=(2.05, 1.6), wall="corr")
    door = k.part("Door", (0.15, -0.72, 0.0))
    with k.into(door):
        for x in (-1.75, 2.05):
            k.box((2.9, 0.08, 2.4), (x, -0.76, 1.2), "metal_door")
            k.box((3.0, 0.1, 0.1), (x, -0.78, 2.4), "hazard")
    k.key("rolloff", door, [(1, None, None), (15, (0.15, -0.72, 2.1), None), (40, (0.15, -0.72, 2.1), None), (55, None, None)])
    for x in (-2.6, 0.0, 2.6):
        landing_pad(k, x, -2.75, 1.1)
    k.box((0.8, 0.8, 4.2), (3.55, -1.1, 2.1), "plaster_w") if False else None
    k.cyl(0.35, 4.3, (-3.6, -1.0, 0), "white", seg=10)
    k.cyl(0.36, 0.2, (-3.6, -1.0, 3.2), "team", seg=10)
    k.sphere(0.65, (-3.6, -1.0, 4.6), "white", seg=14, rings=8)
    return k


@model
def aa_turret():
    """Twin 35 mm anti-air gun on a sandbagged pad."""
    k = Kit("aa_turret")
    with k.nohit_():
        slab(k, 1.9, 1.9, 0.1, "concrete_d")
    oct8 = [(0.62 * math.cos(a), 0.62 * math.sin(a)) for a in [math.pi / 8 + i * math.pi / 4 for i in range(8)]]
    k.prism(oct8, "z", 0.1, 0.42, "concrete", caps={1: "steel_d"}, scale1=0.85)
    for i in range(12):  # sandbag ring
        a = i / 12 * 2 * math.pi
        k.box((0.3, 0.16, 0.14), (0.82 * math.cos(a), 0.82 * math.sin(a), 0.17), "olive", rot=(0, 0, math.degrees(a) + 90), bevel=0.03)
    t = k.part("Turret", (0, 0, 0.42))
    with k.into(t):
        k.cyl(0.36, 0.12, (0, 0, 0.42), "steel_d", seg=12)
        k.box((0.62, 0.7, 0.36), (0, 0.06, 0.72), "steel", faces={"top": "team", "front": "team"}, bevel=0.03, taper=(0.9, 0.85))
        k.box((0.2, 0.2, 0.32), (0, 0.45, 0.82), "steel_d")  # radar mast
        k.box((0.5, 0.05, 0.28), (0, 0.5, 1.08), "white", faces={"back": "steel_d"})
        for s in (-1, 1):
            k.box((0.1, 0.36, 0.3), (s * 0.38, 0.0, 0.72), "steel_d")  # ammo drums
    b = k.part("Turret_Barrel", (0, -0.05, 0.75), t)
    with k.into(b):
        for s in (-1, 1):
            k.box((0.1, 0.4, 0.12), (s * 0.43, -0.15, 0.75), "steel")
            k.cyl(0.03, 0.8, (s * 0.43, -0.3, 0.75), "gun", axis="-y", seg=8)
            k.cyl(0.045, 0.12, (s * 0.43, -1.0, 0.75), "gun", axis="-y", seg=8)
    k.part("Turret_Muzzle_01", (0.43, -1.15, 0.75), b)
    k.part("Turret_Muzzle_02", (-0.43, -1.15, 0.75), b)
    return k


@model
def aa_turret3():
    """T3 Phalanx railgun battery: armoured base, a turret with four rail barrels and its own radar."""
    k = Kit("aa_turret3")
    with k.nohit_():
        slab(k, 3.9, 3.9, 0.12, "concrete_d")
        hazard_edge(k, 3.9, 3.9, t=0.14)
    oct8 = [(1.3 * math.cos(a), 1.3 * math.sin(a)) for a in [math.pi / 8 + i * math.pi / 4 for i in range(8)]]
    k.prism(oct8, "z", 0.12, 0.75, "steel", caps={1: "steel_d"}, scale1=0.82, side_mats=["steel", "team"] * 4)
    for s in (-1, 1):
        k.box((0.7, 0.9, 0.6), (s * 1.35, 1.2, 0.42), "steel_d", faces={"top": "vent"})  # capacitor banks
        k.box((0.72, 0.92, 0.1), (s * 1.35, 1.2, 0.65), "team")
    t = k.part("Turret", (0, 0, 0.75))
    with k.into(t):
        k.cyl(0.8, 0.15, (0, 0, 0.75), "steel_d", seg=16)
        k.box((1.3, 1.5, 0.8), (0, 0.15, 1.3), "steel", faces={"top": "team", "front": "team"}, bevel=0.06, taper=(0.85, 0.8))
        k.box((0.25, 0.25, 0.9), (0, 0.75, 1.9), "steel_d")
        k.box((0.12, 0.6, 0.2), (0, 0.75, 2.3), "steel_d")
    rad = k.part("Radar", (0, 0.75, 2.5), t)
    with k.into(rad):
        k.box((1.1, 0.08, 0.5), (0, 0.8, 2.6), "white", faces={"back": "steel_d"})
    b = k.part("Turret_Barrel", (0, -0.3, 1.35), t)
    with k.into(b):
        k.box((0.9, 0.6, 0.55), (0, -0.35, 1.35), "steel_d", bevel=0.03)
        for dx in (-0.22, 0.22):
            for dz in (-0.13, 0.13):
                k.box((0.1, 1.9, 0.08), (dx, -1.5, 1.35 + dz), "gun")
                k.box((0.04, 1.9, 0.04), (dx, -1.5, 1.35 + dz + 0.06), "copper")
        k.box((0.95, 0.12, 0.6), (0, -1.6, 1.35), "steel")  # rail clamp
    k.part("Turret_Muzzle_01", (0, -2.5, 1.35), b)
    return k


@model
def eng_station():
    """Engineering station: a tower crane on a concrete base; the jib slews while it works ("doors")."""
    k = Kit("eng_station")
    with k.nohit_():
        slab(k, 1.9, 1.9, 0.12, "concrete_d")
        hazard_edge(k, 1.9, 1.9, t=0.1)
    k.box((0.9, 0.9, 0.35), (0, 0, 0.3), "concrete", faces={"top": "steel_d"})
    k.box((0.6, 0.5, 0.45), (0.6, 0.55, 0.35), "team", faces={"top": "vent"})  # generator
    mast_h = 2.2
    for sx in (-1, 1):
        for sy in (-1, 1):
            k.box((0.05, 0.05, mast_h), (sx * 0.2, sy * 0.2, 0.47 + mast_h / 2), "yellow")
    z = 0.6
    while z < 0.47 + mast_h:
        for r in (0, 90):
            with k.at((0, 0, z), rot=(0, 0, r)):
                k.box((0.42, 0.03, 0.03), (0, 0.2, 0), "yellow")
                k.box((0.42, 0.03, 0.03), (0, -0.2, 0), "yellow")
        z += 0.35
    top = 0.47 + mast_h
    jib = k.part("Jib", (0, 0, top))
    with k.into(jib), k.nohit_():
        k.cyl(0.24, 0.1, (0, 0, top), "steel_d", seg=10)
        k.box((0.28, 0.3, 0.3), (0.28, 0.0, top + 0.25), "team", faces={"front": "glass"})  # cab
        k.box((0.16, 2.2, 0.12), (0, -0.85, top + 0.18), "yellow", faces={"top": "hazard"})
        k.box((0.16, 0.8, 0.12), (0, 0.55, top + 0.18), "yellow")
        k.box((0.3, 0.3, 0.35), (0, 0.85, top + 0.05), "concrete")  # counterweight
        k.box((0.06, 0.06, 0.6), (0, 0.0, top + 0.5), "yellow")
        k.box((0.02, 1.9, 0.02), (0, -0.85, top + 0.52), "gun", rot=(-15, 0, 0))
        k.box((0.015, 0.015, 0.9), (0, -1.7, top - 0.3), "black")
        k.box((0.1, 0.1, 0.08), (0, -1.7, top - 0.78), "gun")
    k.key("doors", jib, [(1, None, (0, 0, 0)), (30, None, (0, 0, 150)), (60, None, (0, 0, 360))])
    return k
