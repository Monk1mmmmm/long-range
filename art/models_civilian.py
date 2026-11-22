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
    k.box((w + 0.1, l + 0.1, 0.35), (x, y, 0.17), "concrete_d")  # plinth
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
    with k.nohit_():
        k.box((7.9, 7.9, 0.06), (0, 0, 0.03), "asphalt")
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
    with k.nohit_():
        k.box((7.9, 7.9, 0.06), (0, 0, 0.03), "asphalt")
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
    with k.nohit_():
        k.box((7.9, 7.9, 0.06), (0, 0, 0.03), "concrete")
    w, l, h = 6.4, 4.2, 2.8
    k.box((w, l, h), (0, 0.4, h / 2), "plaster_y", faces={"top": "roof_tar"})
    k.box((w + 0.1, l + 0.1, 0.35), (0, 0.4, 0.17), "concrete_d")
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
    with k.nohit_():
        k.box((11.9, 11.9, 0.06), (0, 0, 0.03), "concrete")
    w, l, h = 11.0, 6.4, 4.4
    k.box((w, l, h), (0, 0.4, h / 2), "brick_y", faces={"top": "roof_tar"})
    k.box((w + 0.12, l + 0.12, 0.3), (0, 0.4, h + 0.1), "plaster_w")
    k.box((w + 0.12, l + 0.12, 0.45), (0, 0.4, 0.22), "concrete_d")
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
    with k.nohit_():
        k.box((7.9, 7.9, 0.06), (0, 0, 0.03), "concrete")
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
    with k.nohit_():
        k.box((11.9, 11.9, 0.04), (0, 0, 0.02), "asphalt") if False else None
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
    with k.nohit_():
        k.box((7.9, 7.9, 0.06), (0, 0, 0.03), "concrete")
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
