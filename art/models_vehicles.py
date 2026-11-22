"""Ground vehicles of the playable faction in the Long Range Tank mod's look: light grey steel, chamfered
plates, team-coloured panels. Carriers, engineers and the scavenger; reactive armour (`reactive` in the unit
JSON) is shown as bolted brick tiles."""
import math

from kit import Kit, model


def wheel(k, x, y, r, w, z=None):
    z = r if z is None else z
    k.cyl(r, w, (x - w / 2, y, z), "rubber", axis="x", seg=12, caps={0: "steel_d", 1: "steel_d"})
    k.cyl(r * 0.45, w + 0.02, (x - w / 2 - 0.01, y, z), "gun", axis="x", seg=8)


def wheels(k, half_track, ys, r, w):
    for y in ys:
        for s in (-1, 1):
            wheel(k, s * half_track, y, r, w)


def tracks(k, x, w, y0, y1, h, n=5, skirt=True, skirt_mat="steel"):
    """Track run centred at x, width w, from y0 (front) to y1 (back), h high: belt, road wheels, sprockets."""
    r = h * 0.45
    prof = [(y0, h * 0.55), (y0 + r * 0.8, h * 0.05), (y1 - r * 0.8, h * 0.05), (y1, h * 0.55), (y1 - r * 0.4, h * 0.95), (y0 + r * 0.4, h * 0.95)]
    k.prism(prof, "x", x - w / 2, x + w / 2, "track", caps={0: "track", 1: "track"})
    s = 1 if x > 0 else -1
    for i in range(n):
        y = y0 + r * 1.1 + (y1 - y0 - 2.2 * r) * i / max(n - 1, 1)
        k.cyl(h * 0.32, 0.06, (x + s * w / 2 - (0.0 if s > 0 else 0.06), y, h * 0.38), "steel_d", axis="x", seg=10, caps={0: "gun", 1: "gun"})
    if skirt:
        k.box((w + 0.04, (y1 - y0) * 0.9, h * 0.32), (x + s * 0.02, (y0 + y1) / 2, h * 0.8), skirt_mat, faces={"top": "steel_d"})


def era_bricks(k, center, normal, width, height, cols, rows, depth=0.07, tilt=0.0):
    """Reactive armour: a grid of bricks on a plate facing `normal` ('x+', 'x-', 'y-', 'y+', 'z+'),
    `tilt` turns the grid about the plate's horizontal axis (glacis)."""
    cx, cy, cz = center
    bw, bh = width / cols, height / rows
    for r in range(rows):
        for c in range(cols):
            u = -width / 2 + bw * (c + 0.5)
            v = -height / 2 + bh * (r + 0.5)
            if normal in ("x+", "x-"):
                s = 1 if normal == "x+" else -1
                with k.at((cx, cy, cz), rot=(tilt, 0, 0)):
                    k.box((depth, bw * 0.92, bh * 0.88), (s * depth / 2, u, v), "era", faces={"top": "era"}, bevel=0.008)
            elif normal in ("y-", "y+"):
                s = 1 if normal == "y+" else -1
                with k.at((cx, cy, cz), rot=(tilt, 0, 0)):
                    k.box((bw * 0.92, depth, bh * 0.88), (u, s * depth / 2, v), "era", bevel=0.008)
            else:
                with k.at((cx, cy, cz), rot=(tilt, 0, 0)):
                    k.box((bw * 0.92, bh * 0.88, depth), (u, v, depth / 2), "era", bevel=0.008)


def wheeled_hull(k, w, l, z0, z1, nose=0.8, tail=0.3, top="team"):
    """Boat-like hull of a wheeled carrier: a side profile extruded across, with sloped flanks."""
    y0, y1 = -l / 2, l / 2
    prof = [(y0, z0 + (z1 - z0) * 0.45), (y0 + nose * 0.35, z0), (y1 - tail * 0.4, z0), (y1, z0 + (z1 - z0) * 0.25),
            (y1, z1 - 0.04), (y1 - 0.05, z1), (y0 + nose, z1), (y0 + nose * 0.12, z0 + (z1 - z0) * 0.72)]
    mats = ["steel", "steel_d", "steel", "steel", "steel", top, "team", "steel"]
    k.prism(prof, "x", -w / 2 * 0.8, w / 2 * 0.8, "steel", side_mats=mats)
    for s in (-1, 1):  # sloped side plates over the wheels
        sp = [(y0 + nose * 0.5, z0 + 0.12), (y1 - tail * 0.3, z0 + 0.12), (y1 - tail * 0.3, z1 - 0.1), (y0 + nose * 0.85, z1 - 0.1)]
        k.prism(sp, "x", s * w / 2 * 0.8, s * w / 2, "steel", scale1=1.0)
    return y0, y1


def lamps(k, xs, y, z):
    for x in xs:
        k.box((0.16, 0.05, 0.08), (x, y, z), "white")


@model
def apc():
    """T1 Tavria: light 4x4 armoured carrier."""
    k = Kit("apc")
    w, l = 1.9, 3.4
    wheeled_hull(k, w, l, 0.38, 1.2, nose=0.9, tail=0.25)
    wheels(k, w / 2 - 0.15, (-0.95, 0.95), 0.35, 0.28)
    k.box((1.1, 0.4, 0.04), (0, -0.98, 1.19), "glass", rot=(-28, 0, 0))  # windscreen slit
    k.box((0.9, 1.4, 0.12), (0, 0.55, 1.26), "steel_d", faces={"top": "vent"})
    k.cyl(0.22, 0.14, (0, -0.2, 1.2), "steel", seg=10)  # commander hatch
    lamps(k, (-0.6, 0.6), -1.68, 0.62)
    with k.nohit_():
        k.box((0.02, 0.02, 0.9), (0.75, 1.4, 1.6), "black")
    return k


@model
def apc_amph():
    """T2 Slavuta: amphibious 6x6 carrier with a trim vane and water jets."""
    k = Kit("apc_amph")
    w, l = 2.2, 4.2
    wheeled_hull(k, w, l, 0.42, 1.38, nose=1.25, tail=0.3)
    wheels(k, w / 2 - 0.16, (-1.2, 0.05, 1.3), 0.37, 0.3)
    k.box((1.7, 0.5, 0.05), (0, -1.88, 0.98), "steel_d", faces={"front": "hazard", "top": "hazard"}, rot=(-55, 0, 0))
    k.box((1.3, 0.35, 0.04), (0, -0.92, 1.4), "glass", rot=(-22, 0, 0))
    k.box((1.2, 1.8, 0.12), (0, 0.6, 1.44), "steel_d", faces={"top": "vent"})
    for s in (-1, 1):
        k.cyl(0.17, 0.14, (s * 0.55, 2.08, 0.62), "gun", axis="y", seg=10, caps={1: "black"})
        k.box((0.05, 1.0, 0.22), (s * 1.11, -0.2, 1.08), "team")
    lamps(k, (-0.7, 0.7), -2.02, 0.72)
    with k.nohit_():
        k.box((0.02, 0.02, 0.9), (0.85, 1.8, 1.8), "black")
    return k


def ifv_turret(k, pos, r=0.56, twin=False, era=False):
    """Small MG turret: Turret (yaw) / Turret_Barrel (pitch) / Turret_Muzzle."""
    t = k.part("Turret", pos)
    x, y, z = pos
    with k.into(t):
        oct8 = [(x + r * math.cos(a), y + r * 1.1 * math.sin(a)) for a in [math.pi / 8 + i * math.pi / 4 for i in range(8)]]
        k.prism(oct8, "z", z, z + 0.34, "steel", caps={1: "team"}, scale1=0.82)
        k.box((0.2, 0.22, 0.18), (x + 0.24, y + 0.15, z + 0.42), "steel_d", faces={"front": "glass"})
        k.cyl(0.16, 0.1, (x - 0.2, y + 0.2, z + 0.32), "steel_d", seg=10)
        if era:
            era_bricks(k, (x, y - r * 1.02, z + 0.17), "y-", r * 1.5, 0.3, 4, 1, tilt=-12)
            for s in (-1, 1):
                era_bricks(k, (x + s * r * 0.98, y + 0.05, z + 0.17), "x+" if s > 0 else "x-", r * 1.6, 0.3, 4, 1)
    b = k.part("Turret_Barrel", (x, y - 0.42, z + 0.2), t)
    with k.into(b):
        k.box((0.32 if twin else 0.22, 0.32, 0.2), (x, y - 0.4, z + 0.2), "steel_d")
        for dx in ((-0.08, 0.08) if twin else (0.0,)):
            k.cyl(0.03, 0.7, (x + dx, y - 0.5, z + 0.2), "gun", axis="-y", seg=8)
            k.cyl(0.042, 0.12, (x + dx, y - 1.1, z + 0.2), "gun", axis="-y", seg=8)
    k.part("Turret_Muzzle", (x, y - 1.24, z + 0.2), b)
    return t


def ifv_hull(k, w, l, tw, th, z_deck, z_roof, glacis):
    """Tracked IFV hull: tracks, a sloped glacis, team-coloured upper plates."""
    for s in (-1, 1):
        tracks(k, s * (w / 2 - tw / 2), tw, -l / 2 + 0.1, l / 2 - 0.05, th, n=6, skirt=False)
    y0, y1 = -l / 2, l / 2
    prof = [(y0 + 0.05, z_deck - 0.3), (y0 + glacis, z_roof), (y1 - 0.5, z_roof + 0.06), (y1, z_roof), (y1, z_deck - 0.3)]
    k.prism(prof, "x", -(w / 2 - tw), w / 2 - tw, "steel", side_mats=["team", "steel", "steel", "steel_d", "steel_d"])
    for s in (-1, 1):  # fenders over the tracks
        k.box((tw + 0.04, l - glacis * 0.6, 0.06), (s * (w / 2 - tw / 2), 0.2, th + 0.05), "steel", faces={"top": "steel_d"})
    return y0, y1


@model
def apc_heavy():
    """T2 Zaporozhets: tracked infantry fighting vehicle."""
    k = Kit("apc_heavy")
    w, l = 2.6, 4.6
    ifv_hull(k, w, l, 0.5, 0.72, 0.85, 1.45, 1.1)
    for s in (-1, 1):
        k.box((0.05, 2.4, 0.5), (s * 0.78, 0.6, 1.15), "steel", faces={"right" if s > 0 else "left": "team"}, rot=(0, -s * 12, 0))
        k.box((0.3, 0.6, 0.35), (s * 1.12, 1.6, 0.95), "steel_d", faces={"top": "vent"})  # stowage boxes
    k.box((1.3, 1.2, 0.08), (0, 1.45, 1.5), "steel_d", faces={"top": "vent"})
    k.box((1.0, 0.05, 0.75), (0, 2.31, 0.95), "metal_door")
    lamps(k, (-0.8, 0.8), -2.05, 0.75)
    ifv_turret(k, (0, 0.0, 1.45), r=0.58)
    return k


@model
def apc_heavy3():
    """T3 Hetman: heavy IFV, reactive armour bricks on the glacis, side skirts and turret."""
    k = Kit("apc_heavy3")
    w, l = 2.8, 5.0
    ifv_hull(k, w, l, 0.55, 0.78, 0.92, 1.58, 1.2)
    for s in (-1, 1):
        k.box((0.05, 2.6, 0.55), (s * 0.85, 0.6, 1.25), "steel", rot=(0, -s * 12, 0))
        # armoured skirt covered in bricks
        k.box((0.06, l - 0.7, 0.5), (s * 1.42, 0.05, 0.66), "steel_d")
        era_bricks(k, (s * 1.45, 0.05, 0.66), "x+" if s > 0 else "x-", l - 0.8, 0.46, 9, 2, depth=0.09)
    # glacis: two rows following the slope
    ang = math.degrees(math.atan2(1.58 - 0.62, 1.15))
    era_bricks(k, (0, -l / 2 + 0.66, 1.12), "z+", 1.6, 1.42, 4, 3, depth=0.09, tilt=ang)
    k.box((1.4, 1.3, 0.08), (0, 1.55, 1.64), "steel_d", faces={"top": "vent"})
    k.box((1.1, 0.05, 0.8), (0, 2.51, 1.0), "metal_door")
    lamps(k, (-0.9, 0.9), -2.42, 0.78)
    ifv_turret(k, (0, 0.1, 1.58), r=0.62, twin=True, era=True)
    return k


def cab(k, x, y, z, w, l, h, glass_front=True, mat="team"):
    """Truck cab: body with a sloped windscreen and side windows."""
    prof = [(y - l / 2, z), (y - l / 2, z + h * 0.55), (y - l / 2 + l * 0.3, z + h), (y + l / 2, z + h), (y + l / 2, z)]
    k.prism(prof, "x", x - w / 2, x + w / 2, mat, side_mats=["steel", "glass" if glass_front else mat, "steel", "steel_d", "steel_d"])
    for s in (-1, 1):
        k.box((0.02, l * 0.4, h * 0.3), (x + s * (w / 2 + 0.005), y + l * 0.05, z + h * 0.7), "glass")


@model
def engineer():
    """T1 engineer: compact tracked construction vehicle with a folding crane arm."""
    k = Kit("engineer")
    for s in (-1, 1):
        tracks(k, s * 0.62, 0.34, -0.85, 0.85, 0.52, n=4, skirt=False)
    k.box((0.92, 1.6, 0.42), (0, 0.0, 0.6), "steel", faces={"top": "steel_d", "front": "team"}, bevel=0.04)
    cab(k, -0.22, -0.35, 0.8, 0.5, 0.75, 0.42)
    k.box((0.38, 0.7, 0.3), (0.26, 0.35, 0.95), "team", faces={"top": "vent"}, bevel=0.03)  # engine
    with k.nohit_():
        k.cyl(0.04, 0.3, (0.38, 0.62, 1.05), "black", seg=6)  # exhaust
    k.box((1.5, 0.12, 0.22), (0, -0.95, 0.32), "steel", faces={"front": "hazard"})  # small blade
    k.cyl(0.05, 0.08, (-0.3, 0.1, 1.2), "yellow", seg=8)  # beacon
    base = (0.28, -0.05, 1.1)
    arm = k.part("Arm", base)
    with k.into(arm):
        k.box((0.22, 0.22, 0.18), (0.28, -0.05, 1.1), "steel_d")
        k.box((0.12, 0.95, 0.12), (0.28, -0.5, 1.2), "yellow", faces={"top": "hazard"})
    tip = (0.28, -0.95, 1.2)
    arm2 = k.part("Arm2", tip, arm)
    with k.into(arm2):
        k.box((0.1, 0.5, 0.1), (0.28, -1.12, 1.2), "yellow")
        k.box((0.16, 0.12, 0.14), (0.28, -1.38, 1.16), "gun")
        k.box((0.04, 0.04, 0.12), (0.28, -1.38, 1.04), "white")
    k.nohit.update((arm, arm2))
    k.key("open", arm, [(1, None, (0, 0, 0)), (20, None, (-45, 0, 0))])
    k.key("open", arm2, [(1, None, (0, 0, 0)), (20, None, (75, 0, 0))])
    return k


@model
def engineer2():
    """T2 engineer: 6x6 truck crane with a telescopic boom."""
    k = Kit("engineer2")
    wheels(k, 0.72, (-0.72, 0.25, 0.75), 0.27, 0.24)
    k.box((1.5, 2.0, 0.25), (0, 0.05, 0.42), "steel_d")
    cab(k, 0, -0.62, 0.55, 1.4, 0.7, 0.62)
    k.box((1.45, 1.2, 0.18), (0, 0.45, 0.62), "steel", faces={"top": "steel_d"})
    for s in (-1, 1):  # outriggers
        k.box((0.3, 0.14, 0.12), (s * 0.82, 0.95, 0.4), "hazard")
        k.box((0.06, 0.06, 0.35), (s * 0.95, 0.95, 0.25), "gun")
    k.box((0.05, 1.0, 0.25), (0.74, 0.5, 0.82), "team")
    k.box((0.05, 1.0, 0.25), (-0.74, 0.5, 0.82), "team")
    k.box((0.7, 0.18, 0.12), (0, -1.0, 0.45), "white")
    rot = k.part("Crane", (0, 0.65, 0.75))
    with k.into(rot):
        k.cyl(0.36, 0.12, (0, 0.65, 0.72), "steel_d", seg=12)
        k.box((0.5, 0.6, 0.36), (0.12, 0.75, 1.0), "team", faces={"top": "steel"}, bevel=0.03)
        k.box((0.24, 0.22, 0.34), (-0.25, 0.6, 1.0), "glass")
    boom = k.part("Boom", (0.0, 0.85, 1.12), rot)
    with k.into(boom):
        k.box((0.22, 1.5, 0.2), (0.0, 0.15, 1.15), "yellow", faces={"top": "hazard"})
    ext = k.part("BoomTip", (0.0, -0.55, 1.15), boom)
    with k.into(ext):
        k.box((0.15, 0.8, 0.14), (0.0, -0.6, 1.15), "yellow")
        k.box((0.03, 0.03, 0.25), (0.0, -1.0, 1.0), "black")
        k.box((0.1, 0.1, 0.08), (0.0, -1.0, 0.86), "gun")
    k.nohit.update((boom, ext))
    k.key("open", rot, [(1, None, (0, 0, 0)), (24, None, (0, 0, 35))])
    k.key("open", boom, [(1, None, (0, 0, 0)), (24, None, (-35, 0, 0))])
    k.key("open", ext, [(1, None, None), (24, (0.0, -1.0, 1.15), None)])
    return k


@model
def engineer3():
    """T3 engineer: heavy armoured engineering vehicle with a dozer blade and a big excavator arm."""
    k = Kit("engineer3")
    for s in (-1, 1):
        tracks(k, s * 0.85, 0.45, -1.15, 1.2, 0.7, n=5)
    prof = [(-1.05, 0.55), (-0.6, 1.25), (1.2, 1.3), (1.25, 0.55)]
    k.prism(prof, "x", -0.62, 0.62, "steel", side_mats=["team", "steel", "steel_d", "steel_d"])
    k.box((0.6, 0.7, 0.35), (-0.25, -0.15, 1.45), "steel", faces={"front": "glass", "top": "team"}, bevel=0.03)
    k.box((0.5, 0.6, 0.3), (0.35, 0.8, 1.42), "steel_d", faces={"top": "vent"})
    k.cyl(0.05, 0.4, (0.5, 1.05, 1.5), "black", seg=6)
    blade = k.part("Blade", (0, -1.25, 0.55))
    with k.into(blade):
        k.box((2.1, 0.14, 0.55), (0, -1.38, 0.42), "steel", faces={"front": "hazard"}, rot=(-10, 0, 0))
        for s in (-1, 1):
            k.box((0.1, 0.5, 0.1), (s * 0.7, -1.15, 0.55), "steel_d")
    arm = k.part("Arm", (0.35, 0.3, 1.35))
    with k.into(arm):
        k.box((0.3, 0.3, 0.3), (0.35, 0.3, 1.35), "steel_d")
        k.box((0.18, 1.4, 0.2), (0.35, -0.35, 1.55), "yellow", faces={"top": "hazard"}, rot=(-8, 0, 0))
    stick = k.part("Arm2", (0.35, -1.0, 1.7), arm)
    with k.into(stick):
        k.box((0.14, 0.14, 0.7), (0.35, -1.05, 1.4), "yellow")
        k.box((0.3, 0.2, 0.18), (0.35, -1.1, 1.03), "gun")
        for s in (-1, 1):
            k.box((0.04, 0.12, 0.16), (0.35 + s * 0.1, -1.2, 0.92), "gun")
    k.nohit.update((arm, stick))
    k.key("open", arm, [(1, None, (0, 0, 0)), (24, None, (-30, 0, 0))])
    k.key("open", stick, [(1, None, (0, 0, 0)), (24, None, (40, 0, 0))])
    k.key("open", blade, [(1, None, (0, 0, 0)), (24, None, (12, 0, 0))])
    return k


@model
def sparky():
    """T2 combat engineer: armoured 6x6 with a roof machine gun and a folded crane."""
    k = Kit("sparky")
    w, l = 1.6, 2.0
    wheeled_hull(k, w, l, 0.3, 0.95, nose=0.55, tail=0.2)
    wheels(k, w / 2 - 0.13, (-0.62, 0.05, 0.68), 0.26, 0.22)
    k.box((0.9, 0.25, 0.03), (0, -0.62, 0.95), "glass", rot=(-25, 0, 0))
    k.box((0.25, 1.1, 0.14), (0.38, 0.35, 1.02), "yellow", faces={"top": "hazard"})  # folded crane
    k.box((0.3, 0.3, 0.2), (0.38, 0.85, 1.0), "steel_d")
    with k.nohit_():
        k.box((0.02, 0.02, 0.6), (-0.55, 0.85, 1.25), "black")
    lamps(k, (-0.5, 0.5), -0.98, 0.55)
    t = k.part("Gun_Turret", (-0.25, -0.15, 0.97))
    with k.into(t):
        k.cyl(0.24, 0.1, (-0.25, -0.15, 0.95), "steel_d", seg=10)
        k.box((0.36, 0.3, 0.2), (-0.25, -0.12, 1.12), "team", bevel=0.02, taper=(0.85, 0.85))
    b = k.part("Gun_Barrel", (-0.25, -0.3, 1.15), t)
    with k.into(b):
        k.box((0.12, 0.25, 0.1), (-0.25, -0.32, 1.15), "gun")
        k.cyl(0.025, 0.45, (-0.25, -0.42, 1.15), "gun", axis="-y", seg=8)
    k.part("Gun_Muzzle", (-0.25, -0.88, 1.15), b)
    return k


@model
def scavenger():
    """T1 scavenger: small tracked salvage tractor with a grab arm and a scrap bin."""
    k = Kit("scavenger")
    for s in (-1, 1):
        tracks(k, s * 0.24, 0.14, -0.3, 0.3, 0.22, n=3, skirt=False)
    k.box((0.36, 0.55, 0.16), (0, 0.0, 0.25), "steel", faces={"top": "steel_d"})
    k.box((0.3, 0.26, 0.2), (0, 0.15, 0.42), "steel_d", faces={"top": "vent"})  # scrap bin
    for i in range(3):
        k.box((0.06, 0.05, 0.05), (-0.08 + i * 0.08, 0.15, 0.55), "copper" if i % 2 else "gun", rot=(0, 0, 30 * i))
    k.box((0.2, 0.18, 0.18), (-0.04, -0.16, 0.42), "team", faces={"front": "glass"}, bevel=0.015)
    arm = k.part("Arm", (0.12, -0.12, 0.4))
    with k.into(arm):
        k.box((0.05, 0.3, 0.05), (0.12, -0.28, 0.44), "yellow")
        for s in (-1, 1):
            k.box((0.015, 0.08, 0.08), (0.12 + s * 0.03, -0.45, 0.4), "gun")
    k.nohit.add(arm)
    k.key("open", arm, [(1, None, (0, 0, 0)), (16, None, (30, 0, 0))])
    return k
