"""Map buildings that replace the stock UEF civilian structures, in the Mining Town map's style: plaster,
brick and prefab panels, framed windows, tiled or flat tarred roofs. Ordinary East European town buildings."""
import math

from kit import Kit, model


def block(k, x, y, w, l, floors, fh=1.45, mat="panel_block", roof="roof_tar", parapet=0.25, win=(0.55, 0.62), cols=None, lit=0.12,
          faces=("front", "back", "left", "right"), door=None, seed=1):
    """Flat-roofed block: windows on the given faces, a parapet, an optional door (x offset) on the front."""
    h = floors * fh
    k.box((w, l, h), (x, y, h / 2), mat, faces={"top": roof})
    k.box((w + 0.08, l + 0.08, parapet), (x, y, h + parapet / 2 - 0.05), "concrete", faces={"top": "concrete"})
    sides = {"front": ((x, y - l / 2, h / 2), (0, -1, 0), w), "back": ((x, y + l / 2, h / 2), (0, 1, 0), w),
             "left": ((x - w / 2, y, h / 2), (-1, 0, 0), l), "right": ((x + w / 2, y, h / 2), (1, 0, 0), l)}
    for i, f in enumerate(faces):
        c, n, span = sides[f]
        nc = cols or max(int(span / 1.1), 1)
        skip = ()
        if door is not None and f == "front":
            dc = int((door + w / 2) / (w / nc))
            skip = ((0, dc),)
            k.door((x + door, y - l / 2, 0.35), (0, -1, 0), 0.6, 1.0, "metal_door")
            k.box((1.0, 0.5, 0.06), (x + door, y - l / 2 - 0.25, 1.45), "concrete")  # canopy
        k.windows(c, n, span, h, floors, nc, win=win, sill=fh * 0.32, lit=lit, skip=skip, seed=seed + i)
    return h


@model
def residential_tower():
    """Five-storey prefab apartment block, L-shaped, with balconies and stair entrances."""
    k = Kit("residential_tower")
    h = block(k, -0.6, -2.2, 6.6, 2.8, 5, door=-1.6, seed=1)
    block(k, 2.4, 1.3, 2.8, 4.4, 5, faces=("back", "left", "right"), seed=7)
    # balconies on the front
    for f in range(1, 5):
        for x in (-3.0, 0.0, 1.8):
            k.box((0.9, 0.45, 0.06), (x, -3.82, f * 1.45 + 0.2), "concrete")
            k.box((0.9, 0.04, 0.4), (x, -4.03, f * 1.45 + 0.42), "plaster_g")
    k.box((1.1, 0.9, 0.7), (-1.6, -2.0, h + 0.35), "panel_block", faces={"top": "roof_tar"})  # lift house
    for x in (-3.2, 0.8, 2.6):
        k.box((0.03, 0.03, 0.8), (x, -1.6, h + 0.4), "gun")
        k.box((0.5, 0.03, 0.03), (x, -1.6, h + 0.65), "gun")
    k.door((2.4, -0.9, 0.35), (-1, 0, 0), 0.6, 1.0, "metal_door") if False else None
    return k


@model
def science_lab():
    """Research institute: three storeys, an observatory dome and an antenna mast."""
    k = Kit("science_lab")
    h = block(k, 0, -0.4, 6.4, 3.6, 3, fh=1.3, mat="plaster_w", door=0.0, win=(0.6, 0.6), seed=3)
    k.box((1.6, 1.0, 1.4), (0, -2.5, 0.7), "glass", faces={"top": "concrete"})  # glass entrance
    k.box((1.8, 1.2, 0.15), (0, -2.5, 1.45), "concrete")
    k.box((1.4, 0.04, 0.3), (0, -3.11, 1.2), "white")
    k.cyl(1.0, 0.7, (1.6, 0.0, h), "plaster_w", seg=16, caps={1: "steel"})
    k.sphere(1.0, (1.6, 0.0, h + 0.7), "white", seg=16, rings=8, scale=(1.0, 1.0, 0.9), half=True)
    k.box((0.25, 1.8, 0.05), (1.6, -0.1, h + 1.25), "black", rot=(0, 25, 0))  # slit
    for i in range(3):  # lattice mast
        z = h + i * 1.0
        for sx in (-1, 1):
            k.box((0.04, 0.04, 1.0), (-2.2 + sx * (0.25 - i * 0.06), 0.3, z + 0.5), "red" if i % 2 else "white")
        k.box((0.5 - i * 0.12, 0.04, 0.04), (-2.2, 0.3, z + 0.95), "gun")
    k.cyl(0.25, 0.08, (-2.2, 0.0, h + 2.4), "white", axis="-y", seg=10)
    for x in (-2.8, 2.8):  # small annex labs at the back
        k.box((1.6, 2.2, 1.4), (x, 2.6, 0.7), "brick_y", faces={"top": "roof_tar"})
        k.windows((x, 3.7, 0.7), (0, 1, 0), 1.6, 1.4, 1, 2, win=(0.45, 0.5), sill=0.5)
    return k


@model
def admin_building():
    """Two-storey town hall: plastered, a columned portico with a pediment, a tiled hip roof and a flag."""
    k = Kit("admin_building")
    w, l, h = 6.4, 4.2, 2.8
    k.box((w, l, h), (0, 0.4, h / 2), "plaster_y", faces={"top": "roof_tar"})
    k.box((w + 0.16, l + 0.16, 0.12), (0, 0.4, h - 0.06), "plaster_w")  # cornice
    roof = [(-w / 2 - 0.15, h), (w / 2 + 0.15, h), (w / 2 - 1.0, h + 0.85), (-w / 2 + 1.0, h + 0.85)]
    k.prism(roof, "y", 0.4 - l / 2 - 0.15, 0.4 + l / 2 + 0.15, "roof_tile", scale1=1.0, caps={0: "roof_tile", 1: "roof_tile"})
    for f in ("front", "back"):
        n = -1 if f == "front" else 1
        k.windows((0, 0.4 + n * l / 2, h / 2), (0, n, 0), w, h, 2, 7, win=(0.4, 0.75), sill=0.4, skip=((0, 3),) if n < 0 else (), seed=2)
    for s in (-1, 1):
        k.windows((s * w / 2, 0.4, h / 2), (s, 0, 0), l, h, 2, 4, win=(0.4, 0.75), sill=0.4, seed=4)
    # portico
    k.box((2.6, 1.2, 0.25), (0, -2.3, 0.12), "concrete")
    for x in (-1.05, -0.35, 0.35, 1.05):
        k.cyl(0.13, 2.5, (x, -2.7, 0.25), "plaster_w", seg=10)
    k.box((2.6, 1.0, 0.2), (0, -2.35, 2.82), "plaster_w")
    k.prism([(-1.35, 2.92), (1.35, 2.92), (0, 3.5)], "y", -2.85, -1.8, "plaster_w", caps={0: "plaster_w"})
    k.door((0, -1.7, 0.35), (0, -1, 0), 0.8, 1.6, "door")
    k.cyl(0.03, 2.0, (0, -2.4, 3.5), "gun", seg=6)
    k.box((0.02, 0.6, 0.38), (0, -2.1, 5.25), "team")  # flag
    return k


@model
def admin_hall():
    """Palace of culture: a wide hall behind a colonnade, a stepped central tower with a clock."""
    k = Kit("admin_hall")
    w, l, h = 11.0, 6.4, 4.4
    k.box((w, l, h), (0, 0.4, h / 2), "brick_y", faces={"top": "roof_tar"})
    k.box((w + 0.12, l + 0.12, 0.3), (0, 0.4, h + 0.1), "plaster_w")
    for s in (-1, 1):
        k.windows((s * w / 2, 0.4, h / 2), (s, 0, 0), l, h, 2, 5, win=(0.5, 1.1), sill=0.5, seed=5 + s)
        k.windows((s * 3.4, 0.4 - l / 2, h / 2), (0, -1, 0), 4.0, h, 2, 4, win=(0.5, 1.1), sill=0.5, seed=8 + s)
    k.windows((0, 0.4 + l / 2, h / 2), (0, 1, 0), w, h, 2, 10, win=(0.5, 1.1), sill=0.5, seed=9)
    # colonnade and entablature in front of the middle
    k.box((4.4, 1.6, 0.35), (0, -3.4, 0.17), "concrete")
    for i in range(6):
        k.cyl(0.18, 3.8, (-1.9 + i * 0.76, -3.8, 0.35), "plaster_w", seg=12)
    k.box((4.6, 1.5, 0.5), (0, -3.35, 4.35), "plaster_w")
    k.box((2.2, 0.05, 0.4), (0, -4.11, 4.35), "team")
    for i in range(3):
        k.door((-1.0 + i * 1.0, -2.6, 0.45), (0, -1, 0), 0.6, 1.8, "door")
    # stepped tower
    k.box((3.0, 2.6, 1.3), (0, 0.6, h + 0.65), "brick_y", faces={"top": "roof_tar"})
    k.box((1.8, 1.8, 1.2), (0, 0.6, h + 1.9), "plaster_w", faces={"top": "copper"})
    k.cyl(0.4, 0.04, (0, -0.31, h + 2.0), "white", axis="-y", seg=16)
    k.prism([(-0.9, h + 2.5), (0.9, h + 2.5), (0, h + 2.95)], "y", -0.3, 1.5, "copper", caps={0: "copper", 1: "copper"})
    return k


@model
def high_command():
    """Office tower: a glass curtain wall in a concrete frame on a low podium, an antenna mast on top."""
    k = Kit("high_command")
    k.box((6.6, 5.6, 1.2), (0, 0.3, 0.6), "plaster_w", faces={"top": "roof_tar"})
    k.windows((0, 0.3 - 2.8, 0.6), (0, -1, 0), 6.6, 1.2, 1, 6, win=(0.8, 0.7), sill=0.25, skip=((0, 2), (0, 3)))
    k.box((1.8, 0.8, 1.0), (0, -2.75, 0.5), "glass", faces={"top": "concrete"})
    tw, tl, th = 4.0, 3.6, 7.6
    k.box((tw, tl, th - 1.2), (0, 0.3, 1.2 + (th - 1.2) / 2), "concrete", faces={"top": "roof_tar"})
    floors = 6
    fh = (th - 1.2) / floors
    for f, n, span in (((0, 0.3 - tl / 2), (0, -1, 0), tw), ((0, 0.3 + tl / 2), (0, 1, 0), tw), ((-tw / 2, 0.3), (-1, 0, 0), tl), ((tw / 2, 0.3), (1, 0, 0), tl)):
        for i in range(floors):  # ribbon windows
            z = 1.2 + i * fh + fh * 0.55
            nx, ny, _ = n
            size = (span - 0.3, 0.04, fh * 0.6) if nx == 0 else (0.04, span - 0.3, fh * 0.6)
            k.box(size, (f[0] + nx * 0.01, f[1] + ny * 0.01, z), "glass")
    k.box((tw + 0.1, tl + 0.1, 0.3), (0, 0.3, th + 0.1), "concrete")
    k.box((1.2, 1.0, 0.6), (0.9, 0.6, th + 0.4), "steel", faces={"top": "vent"})
    for i in range(3):
        k.box((0.05, 0.05, 0.7), (-1.0, 0.3, th + 0.25 + 0.7 * i + 0.35), "red" if i % 2 else "white")
    k.cyl(0.3, 0.06, (-1.0, 0.0, th + 1.6), "white", axis="-y", seg=10)
    k.box((0.02, 0.5, 0.32), (-1.0, 0.3, th + 2.4), "team")
    return k


@model
def farm():
    """Collective farm yard: a red barn with a slate roof, two silos, a machine shed and hay bales."""
    k = Kit("farm")
    bw, bl, bh = 4.4, 7.0, 2.4
    k.box((bw, bl, bh), (-2.6, 0.6, bh / 2), "barn")
    roof = [(-2.6 - bw / 2 - 0.2, bh), (-2.6 - bw / 2 + 0.3, bh + 0.9), (-2.6, bh + 1.75), (-2.6 + bw / 2 - 0.3, bh + 0.9), (-2.6 + bw / 2 + 0.2, bh)]
    k.prism(roof, "y", 0.6 - bl / 2 - 0.15, 0.6 + bl / 2 + 0.15, "roof_slate", caps={0: "barn", 1: "barn"})
    k.box((1.6, 0.06, 1.9), (-2.6, 0.6 - bl / 2 - 0.03, 0.95), "wood")
    k.box((1.65, 0.07, 0.08), (-2.6, 0.6 - bl / 2 - 0.04, 0.95), "white", rot=(0, 49, 0))
    k.box((1.65, 0.07, 0.08), (-2.6, 0.6 - bl / 2 - 0.04, 0.95), "white", rot=(0, -49, 0))
    k.box((0.7, 0.06, 0.6), (-2.6, 0.6 - bl / 2 - 0.03, 2.9), "wood")
    for x in (2.2, 4.3):
        k.cyl(0.85, 3.9, (x, 2.8, 0), "corr_white", seg=16)
        k.sphere(0.85, (x, 2.8, 3.9), "steel", seg=16, rings=6, scale=(1, 1, 0.45), half=True)
        k.box((0.06, 0.06, 4.0), (x - 0.88, 2.8, 2.0), "gun")
    # open machine shed
    for xx in (1.2, 3.0, 4.8):
        for yy in (-2.4, -4.6):
            k.box((0.12, 0.12, 2.0), (xx, yy, 1.0), "wood")
    k.box((4.2, 2.8, 0.08), (3.0, -3.5, 2.05), "corr", rot=(6, 0, 0))
    k.box((1.0, 1.6, 0.7), (2.1, -3.5, 0.45), "olive", faces={"top": "steel_d"})  # tractor
    k.cyl(0.3, 0.2, (1.5, -3.0, 0.3), "rubber", axis="x", seg=10)
    for i in range(4):  # hay bales
        k.cyl(0.35, 0.6, (-0.3 + (i % 2) * 0.65, -4.5 + (i // 2) * 0.8, 0.35), "plaster_y", axis="x", seg=10)
    k.box((0.04, 4.0, 0.05), (-5.7, 0.0, 0.7), "wood")
    k.box((0.04, 4.0, 0.05), (-5.7, 0.0, 0.4), "wood")
    return k


@model
def plant():
    """Small brick works: sawtooth north-light roofs, loading doors and a tall brick chimney."""
    k = Kit("plant")
    w, l, h = 5.6, 4.6, 2.4
    k.box((w, l, h), (-0.6, 0.4, h / 2), "brick", faces={"top": "roof_tar"})
    for i in range(4):  # sawtooth
        x0 = -0.6 - w / 2 + i * w / 4
        k.prism([(x0, h), (x0 + w / 4, h), (x0 + w / 4, h + 0.9)], "y", 0.4 - l / 2, 0.4 + l / 2, "roof_slate",
                side_mats=["roof_slate", "glass", "roof_slate"], caps={0: "brick", 1: "brick"})
    k.windows((-0.6, 0.4 - l / 2, h / 2), (0, -1, 0), w, h, 1, 5, win=(0.6, 0.9), sill=0.9, skip=((0, 1), (0, 3)))
    k.windows((-0.6 - w / 2, 0.4, h / 2), (-1, 0, 0), l, h, 1, 4, win=(0.6, 0.9), sill=0.9)
    for x in (-2.0, 0.8):
        k.box((1.1, 0.06, 1.5), (x, 0.4 - l / 2 - 0.03, 0.75), "metal_door")
    k.cyl(0.42, 6.0, (2.9, 2.2, 0), "brick", seg=12, r2=0.3)
    k.cyl(0.34, 0.25, (2.9, 2.2, 5.85), "black", seg=12)
    k.box((1.4, 1.6, 1.6), (2.8, -1.6, 0.8), "plaster", faces={"top": "roof_tar"})  # office
    k.windows((2.8, -2.4, 0.8), (0, -1, 0), 1.4, 1.6, 1, 2, win=(0.4, 0.6), sill=0.6)
    k.cyl(0.5, 1.6, (2.8, 0.6, 0), "steel", seg=12, caps={1: "steel_d"})  # tank
    return k


@model
def refinery():
    """Oil refinery: distillation columns, a furnace block, storage spheres, pipe racks and a flare stack."""
    from models_defence import lattice
    k = Kit("refinery")
    k.box((5.0, 3.0, 2.6), (-2.6, 2.6, 1.3), "brick", faces={"top": "roof_tar"})  # furnace house
    k.windows((-2.6, 1.1, 1.3), (0, -1, 0), 5.0, 2.6, 1, 5, win=(0.5, 0.8), sill=1.0)
    for x, r, h in ((-3.8, 0.55, 8.8), (-2.4, 0.45, 7.0), (-1.2, 0.38, 6.0)):
        k.cyl(r, h, (x, -1.2, 0), "white", seg=14, caps={1: "steel"})
        for z in range(2, int(h), 2):
            k.cyl(r + 0.08, 0.1, (x, -1.2, z), "steel_d", seg=14)
            k.cyl(r + 0.5, 0.05, (x, -1.2, z + 0.02), "steel_d", seg=14) if False else None
    k.box((3.4, 0.5, 0.1), (-2.4, -1.2, 4.0), "steel_d")  # walkway between columns
    for x, y in ((2.6, 2.8), (2.6, -0.6)):
        k.sphere(1.3, (x, y, 2.2), "white", seg=16, rings=8)
        for sx in (-1, 1):
            for sy in (-1, 1):
                k.box((0.14, 0.14, 1.6), (x + sx * 0.8, y + sy * 0.8, 0.8), "steel_d")
    k.box((10.0, 0.6, 0.12), (0, -3.6, 2.4), "steel_d")  # pipe rack
    for x in (-4.5, -1.5, 1.5, 4.5):
        k.box((0.14, 0.6, 2.4), (x, -3.6, 1.2), "steel_d")
    for dy in (-0.15, 0.15):
        k.cyl(0.12, 10.0, (-5.0, -3.6 + dy, 2.6), "steel", axis="x", seg=8)
    lattice(k, 4.6, -4.4, 0.8, 0.3, 0, 6.0, mat="steel_d")
    k.cyl(0.14, 6.6, (4.6, -4.4, 0), "rust", seg=8, caps={1: "black"})
    k.cyl(0.25, 0.3, (4.6, -4.4, 6.6), "yellow", seg=8)
    return k


@model
def fuel_tank():
    """Fuel storage tank with a spiral-less stair, a bund wall and a pump."""
    k = Kit("fuel_tank")
    for s in (-1, 1):
        k.box((3.8, 0.12, 0.35), (0, s * 1.85, 0.2), "concrete_d")
        k.box((0.12, 3.8, 0.35), (s * 1.85, 0, 0.2), "concrete_d")
    k.cyl(1.4, 2.6, (0, 0.2, 0.08), "white", seg=20, caps={1: "steel"})
    k.cyl(1.42, 0.3, (0, 0.2, 1.9), "red", seg=20)
    k.cyl(1.42, 0.12, (0, 0.2, 2.68), "steel", seg=20, r2=0.6)
    for i in range(9):  # stair on the front
        k.box((0.3, 0.06, 0.04), (-0.9 + i * 0.12, -1.25 + i * 0.03, 0.3 + i * 0.28), "steel_d")
    k.box((0.04, 0.04, 2.7), (-0.75, -1.3, 1.4), "yellow")
    k.box((0.5, 0.4, 0.4), (1.3, -1.4, 0.28), "steel", faces={"top": "vent"})
    k.cyl(0.08, 1.0, (0.8, -1.2, 0.35), "gun", axis="x", seg=6)
    return k


@model
def pylon():
    """Power line pylon: a tapering steel lattice with two cross arms and hanging insulators."""
    from models_defence import lattice
    k = Kit("pylon")
    for sx in (-1, 1):
        for sy in (-1, 1):
            k.box((0.4, 0.4, 0.3), (sx * 0.9, sy * 0.25, 0.15), "concrete")
    lattice(k, 0, 0, 1.8, 0.4, 0.3, 8.6, mat="steel_d", step=1.2, leg=0.06, brace=0.025)
    for z, w in ((7.8, 3.8), (6.4, 2.6)):
        k.box((w, 0.3, 0.16), (0, 0, z), "steel_d")
        for s in (-1, 1):
            for j in range(4):
                k.cyl(0.07, 0.05, (s * (w / 2 - 0.1), 0, z - 0.15 - j * 0.1), "glass", seg=8)
    return k


@model
def comm_tower():
    """Communications mast: a red and white lattice with dishes, a rotating antenna and an equipment hut."""
    from models_defence import lattice, dish
    k = Kit("comm_tower")
    k.box((2.2, 1.6, 1.4), (0, 1.0, 0.7), "plaster_w", faces={"top": "roof_tar"})
    k.door((0, 0.2, 0.0), (0, -1, 0), 0.6, 1.0, "metal_door")
    for i in range(5):
        z0 = i * 2.0
        lattice(k, 0, -0.6, 1.4 - i * 0.2, 1.2 - i * 0.2, z0, z0 + 2.0, mat="red_white" if i % 2 == 0 else "white", step=1.0, leg=0.05, brace=0.025)
    for z, a in ((6.0, 0), (7.5, 120)):
        k.cyl(0.35, 0.12, (0.4 * math.cos(math.radians(a)), -0.6 + 0.4 * math.sin(math.radians(a)), z), "white", axis="-y", seg=12)
    k.cyl(0.04, 1.5, (0, -0.6, 10.0), "white", seg=6)
    k.cyl(0.06, 0.1, (0, -0.6, 11.5), "red", seg=6)
    dish(k, "Radar", (0, -0.6, 10.2), 1.6, 0.5)
    return k


@model
def water_tower():
    """Water tower of the Rozhnovsky kind: a brick shaft with a riveted steel tank on top."""
    k = Kit("water_tower")
    k.cyl(0.85, 4.4, (0, 0, 0), "brick", seg=14, r2=0.75)
    k.cyl(0.95, 0.3, (0, 0, 0), "concrete_d", seg=14)
    k.door((0, -0.82, 0.1), (0, -1, 0), 0.5, 1.0, "metal_door")
    for z in (1.6, 3.0):
        k.box((0.04, 0.3, 0.5), (0.0, -0.8, z), "glass", rot=(0, 0, 0))
    k.cyl(0.9, 0.3, (0, 0, 4.4), "concrete", seg=14, r2=1.4)
    k.cyl(1.5, 2.0, (0, 0, 4.7), "steel", seg=18, caps={1: "steel_d"})
    for z in (5.2, 5.9, 6.5):
        k.cyl(1.52, 0.06, (0, 0, z), "steel_d", seg=18)
    k.cyl(1.55, 0.5, (0, 0, 6.7), "steel_d", seg=18, r2=0.2)
    k.box((0.03, 0.03, 2.6), (1.5, 0, 5.8), "gun")
    return k


@model
def bunker():
    """Old concrete pillbox with a wide firing slit and an entrance at the back."""
    k = Kit("bunker")
    k.box((3.5, 3.5, 1.3), (0, 0, 0.65), "concrete_old", faces={"top": "dirt"}, bevel=0.15, taper=(0.9, 0.9))
    k.box((3.7, 3.7, 0.3), (0, 0, 1.4), "concrete_old", faces={"top": "dirt"}, bevel=0.1)
    k.box((1.8, 0.1, 0.3), (0, -1.66, 1.0), "black")
    for s in (-1, 1):
        k.box((0.1, 1.0, 0.25), (s * 1.66, -0.3, 1.0), "black")
    k.box((0.8, 0.4, 1.0), (0, 1.9, 0.5), "concrete_old")
    k.box((0.6, 0.04, 0.9), (0, 2.1, 0.45), "black")
    for i in range(5):
        k.box((0.5, 0.4, 0.25), (-1.4 + i * 0.7, -2.0, 0.14), "sandbag", bevel=0.06)
    k.box((1.0, 1.2, 0.3), (0.5, 0.2, 1.7), "grass" if False else "dirt", bevel=0.1, taper=(0.5, 0.5))
    return k


# ---------------------------------------------------------------- small village houses (one cell, 2 x 2)

def small_house(k, w, l, wall_h, roof_h, wall, roof, door_x=0.0, chimney=None, win_n=2, overhang=0.14, trim="white"):
    """Gable-roofed cottage, ridge along y: the door in the front gable, windows along both long sides."""
    from models_buildings import shed
    shed(k, w, l, wall_h, roof_h, wall=wall, roof=roof, gable=wall, overhang=overhang)
    k.door((door_x, -l / 2, 0.0), (0, -1, 0), 0.42, 0.9, "door", frame=trim)
    k.box((0.7, 0.3, 0.05), (door_x, -l / 2 - 0.15, 0.95), "wood")  # little porch roof
    for sx in (-1, 1):
        k.windows((sx * w / 2, 0.0, wall_h / 2), (sx, 0, 0), l - 0.5, wall_h - 0.05, 1, win_n, win=(0.34, 0.4), sill=0.22, frame=trim, lit=0.0)
    k.windows((0, l / 2, wall_h / 2), (0, 1, 0), w, wall_h, 1, 1, win=(0.34, 0.4), sill=0.22, frame=trim)
    if chimney is not None:
        cx, cy = chimney
        k.box((0.26, 0.26, roof_h + 0.35), (cx, cy, wall_h + (roof_h + 0.35) / 2 - 0.05), "brick")
        k.box((0.34, 0.34, 0.06), (cx, cy, wall_h + roof_h + 0.28), "concrete")


@model
def khata():
    """Whitewashed village hut with a thatched roof."""
    k = Kit("khata")
    small_house(k, 1.5, 1.9, 0.95, 0.75, "plaster_w", "thatch", door_x=0.0, chimney=(0.35, 0.4), overhang=0.18, trim="plaster_b")
    return k


@model
def khata_slate():
    """Village hut, yellow-washed, slate roof, a chimney."""
    k = Kit("khata_slate")
    small_house(k, 1.6, 1.8, 0.95, 0.65, "plaster_y", "roof_slate", door_x=0.3, chimney=(-0.35, 0.3), trim="plaster_b")
    return k


@model
def cottage():
    """Small brick house with a tiled roof and a chimney."""
    k = Kit("cottage")
    small_house(k, 1.7, 1.95, 1.05, 0.7, "brick", "roof_tile", door_x=-0.35, chimney=(0.4, 0.3), win_n=2)
    return k


@model
def dacha():
    """Timber summer cabin with a green corrugated roof and a veranda."""
    k = Kit("dacha")
    small_house(k, 1.5, 1.6, 0.95, 0.6, "timber", "corr_green", door_x=0.3, trim="white")
    k.box((1.5, 0.45, 0.06), (0, -1.05, 0.28), "wood")  # veranda deck
    k.box((1.6, 0.5, 0.05), (0, -1.05, 1.0), "corr_green")
    for sx in (-0.7, 0.7):
        k.box((0.05, 0.05, 0.72), (sx, -1.25, 0.64), "white")
    return k


@model
def outbuilding():
    """Barn-like outbuilding of boards and corrugated sheet."""
    k = Kit("outbuilding")
    from models_buildings import shed
    shed(k, 1.6, 1.8, 0.8, 0.5, wall="barn", roof="corr_d", gable="barn", overhang=0.1)
    k.box((0.7, 0.04, 0.65), (0, -0.92, 0.45), "wood")
    k.box((0.5, 0.04, 0.5), (0, 0.92, 0.5), "black")
    return k


@model
def farmhouse():
    """Larger country house: two brick wings under tiled roofs, a chimney and a fence post gateway."""
    k = Kit("farmhouse")
    from models_buildings import shed
    shed(k, 2.4, 3.4, 1.15, 0.9, wall="brick_y", roof="roof_tile", gable="brick_y", loc=(-0.5, 0.0), overhang=0.16)
    shed(k, 1.5, 1.8, 0.95, 0.6, wall="plaster_w", roof="roof_tile", gable="plaster_w", loc=(1.1, 0.7), overhang=0.12)
    k.door((-0.5, -1.7, 0.25), (0, -1, 0), 0.5, 0.85, "door")
    for sx in (-1, 1):
        k.windows((-0.5 + sx * 1.2, 0.0, 0.575), (sx, 0, 0), 2.8, 1.1, 1, 3, win=(0.42, 0.48), sill=0.3)
    k.windows((-0.5, -1.7, 0.575), (0, -1, 0), 2.4, 1.1, 1, 2, win=(0.4, 0.46), sill=0.3, skip=((0, 0),))
    k.box((0.3, 0.3, 1.5), (-1.0, 0.4, 1.7), "brick")
    k.box((0.38, 0.38, 0.06), (-1.0, 0.4, 2.46), "concrete")
    return k
