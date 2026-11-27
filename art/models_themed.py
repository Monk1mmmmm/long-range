"""Houses made for the map themes (assets/data/themes.json): Adriatic stone and plaster, Central European
plaster houses and gabled townhouses, village log and whitewashed houses, Soviet blocks, industrial flat-roofed
houses. Footprint 2 (4 x 4 units) unless noted; front is -y."""
import math

from kit import Kit, model, LOW


def gable(k, x, y, w, l, z, rise, roof, gable_mat, over=0.18, ridge="y"):
    """Pitched roof on walls w (x) by l (y) topping out at z: the ridge runs along y (or x)."""
    if ridge == "x":
        with k.at((x, y, 0), rot=(0, 0, 90)):
            gable(k, 0, 0, l, w, z, rise, roof, gable_mat, over, "y")
        return
    tri = [(x - w / 2, z), (x + w / 2, z), (x, z + rise)]
    k.prism(tri, "y", y - l / 2, y + l / 2, gable_mat, caps={0: gable_mat, 1: gable_mat})
    a = math.atan2(rise, w / 2)
    half = (w / 2 + over) / math.cos(a)
    for s in (-1, 1):
        ex, ez = x + s * (w / 2 + over), z - over * math.tan(a)
        with k.at(((ex + x) / 2, y, (ez + z + rise) / 2 + 0.03), rot=(0, s * math.degrees(a), 0)):
            k.box((half, l + 2 * over, 0.07), (0, 0, 0), roof, faces={"bottom": "steel_d"})
    k.box((0.16, l + 2 * over, 0.09), (x, y, z + rise + 0.05), "steel_d")


def hip(k, x, y, w, l, z, rise, roof, over=0.2):
    tx, ty = ((w - l) / (w + 2 * over), 0.05) if w >= l else (0.05, (l - w) / (l + 2 * over))
    k.box((w + 2 * over, l + 2 * over, rise), (x, y, z + rise / 2), roof, taper=(max(tx, 0.05), max(ty, 0.05)), faces={"bottom": "steel_d"})


def chimney(k, x, y, z, h, mat="brick"):
    k.box((0.3, 0.3, h), (x, y, z + h / 2), mat)
    k.box((0.4, 0.4, 0.06), (x, y, z + h), "concrete")


def front_windows(k, cx, yf, span, z0, storeys, sh, cols, win=(0.36, 0.5), shutter=None, trim="white", skip=(), lit=0.0, seed=1):
    """Windows in a grid on a -y face at y=yf, with optional shutters either side."""
    shutter = None if LOW else shutter
    k.windows((cx, yf, z0 + storeys * sh / 2), (0, -1, 0), span, storeys * sh, storeys, cols, win=win, sill=sh * 0.3, frame=trim, skip=skip, lit=lit, seed=seed)
    if shutter:
        for r in range(storeys):
            for c in range(cols):
                if (r, c) in skip:
                    continue
                x = cx - span / 2 + span / cols * (c + 0.5)
                z = z0 + r * sh + sh * 0.3 + win[1] / 2
                for s in (-1, 1):
                    k.box((0.13, 0.04, win[1] + 0.08), (x + s * (win[0] / 2 + 0.15), yf - 0.03, z), shutter)


def side_windows(k, x_face, sx, span, z0, storeys, sh, cols, win=(0.36, 0.5), trim="white", skip=(), yc=0.0):
    k.windows((x_face, yc, z0 + storeys * sh / 2), (sx, 0, 0), span, storeys * sh, storeys, cols, win=win, sill=sh * 0.3, frame=trim, skip=skip)


def balcony(k, x, y, z, w, d=0.4, mat="concrete"):
    if LOW:
        return
    k.box((w, d, 0.07), (x, y - d / 2, z), mat)
    k.box((w, 0.03, 0.04), (x, y - d, z + 0.34), "black")
    for i in range(int(w / 0.18) + 1):
        k.box((0.025, 0.025, 0.34), (x - w / 2 + i * w / int(w / 0.18), y - d, z + 0.17), "black")


# ---------------------------------------------------------------- Adriatic

@model
def adr_stone_house():
    """Dalmatian stone house: dressed limestone, green shutters, an iron balcony, a hipped terracotta roof."""
    k = Kit("adr_stone_house")
    w, l, h, sh = 3.5, 3.0, 2.5, 1.25
    k.box((w, l, h), (0, 0, h / 2), "stone", faces={"top": "stone_d"})
    k.box((w + 0.16, l + 0.16, 0.1), (0, 0, h + 0.04), "stone")  # cornice
    hip(k, 0, 0, w, l, h + 0.05, 1.0, "terracotta", over=0.28)
    chimney(k, 0.9, 0.4, h + 0.6, 0.7, "stone")
    front_windows(k, 0, -l / 2, w - 0.5, 0.25, 2, sh, 3, win=(0.34, 0.52), shutter="shutter_g", trim="stone", skip=((0, 1),), seed=2)
    k.door((0, -l / 2, 0.22), (0, -1, 0), 0.55, 1.0, "door", frame="stone")
    k.box((0.85, 0.05, 0.1), (0, -l / 2 - 0.03, 1.3), "stone")  # lintel
    balcony(k, 0, -l / 2, 1.55, 1.1)
    for sx in (-1, 1):
        side_windows(k, sx * w / 2, sx, l - 0.7, 0.25, 2, sh, 2, win=(0.32, 0.5), trim="stone")
    k.windows((0, l / 2, 1.5), (0, 1, 0), w - 0.8, 2 * sh, 2, 2, win=(0.34, 0.5), sill=0.35, frame="stone")
    return k


@model
def adr_townhouse():
    """Tall narrow town house of ochre or pink plaster with blue shutters and a ridge roof along the street."""
    k = Kit("adr_townhouse")
    w, l, sh = 3.4, 2.9, 1.2
    h = 3 * sh
    k.box((w, l, h), (0, 0, h / 2), "plaster_ochre", faces={"top": "stone_d"})
    for sx in (-1, 1):  # stone quoins
        for i in range(7):
            k.box((0.22 + 0.08 * (i % 2), 0.22, 0.22), (sx * (w / 2 - 0.06), -l / 2 + 0.01, 0.4 + i * 0.5), "stone")
    k.box((w + 0.2, l + 0.2, 0.1), (0, 0, h + 0.04), "stone")
    gable(k, 0, 0, w, l, h + 0.09, 1.0, "terracotta", "plaster_ochre", over=0.26, ridge="x")
    chimney(k, -1.0, 0.0, h + 0.5, 0.8, "stone")
    front_windows(k, 0, -l / 2, w - 0.4, 0.3, 3, sh, 3, win=(0.3, 0.5), shutter="shutter_b", trim="stone", skip=((0, 1),), seed=4)
    k.door((0, -l / 2, 0.25), (0, -1, 0), 0.55, 1.0, "door", frame="stone")
    balcony(k, 0.0, -l / 2, 1.5 + 0.0, 0.9)
    for sx in (-1, 1):
        side_windows(k, sx * w / 2, sx, l - 0.8, 0.3, 3, sh, 2, win=(0.3, 0.46), trim="stone")
    return k


# ---------------------------------------------------------------- Central European

@model
def ce_plaster_house():
    """Plain two-storey plaster house: a string course, framed windows, a small canopy over the door, a tiled gable roof."""
    k = Kit("ce_plaster_house")
    w, l, sh = 3.4, 3.1, 1.2
    h = 2 * sh + 0.2
    k.box((w, l, h), (0, 0, h / 2), "plaster_cream", faces={"top": "concrete"})
    k.box((w + 0.06, l + 0.06, 0.08), (0, 0, 0.2 + sh), "plaster_w")  # string course
    k.box((w + 0.14, l + 0.14, 0.1), (0, 0, h + 0.03), "plaster_w")  # eaves cornice
    gable(k, 0, 0, w, l, h + 0.08, 1.15, "roof_tile", "plaster_cream", over=0.24, ridge="x")
    chimney(k, 0.9, 0.2, h + 0.6, 0.9)
    front_windows(k, 0, -l / 2, w - 0.6, 0.3, 2, sh, 3, win=(0.38, 0.54), trim="white", skip=((0, 1),), seed=5)
    k.door((0, -l / 2, 0.24), (0, -1, 0), 0.55, 1.0, "door", frame="white")
    k.box((1.0, 0.45, 0.07), (0, -l / 2 - 0.22, 1.35), "concrete")  # canopy
    for sx in (-1, 1):
        side_windows(k, sx * w / 2, sx, l - 0.8, 0.3, 2, sh, 2, win=(0.34, 0.5), trim="white")
    k.windows((0, l / 2, 0.3 + sh), (0, 1, 0), w - 0.6, 2 * sh, 2, 3, win=(0.36, 0.5), sill=0.35, frame="white")
    return k


@model
def ce_gabled_townhouse():
    """Pastel three-storey townhouse behind a stepped front gable."""
    k = Kit("ce_gabled_townhouse")
    w, l, sh = 3.0, 3.4, 1.15
    h = 3 * sh
    k.box((w, l, h), (0, 0, h / 2), "plaster_sky", faces={"top": "roof_tar"})
    for i, fw in enumerate((w, w * 0.78, w * 0.56, w * 0.34)):  # stepped gable on the front
        k.box((fw, 0.32, 0.3), (0, -l / 2 + 0.16, h + 0.15 + i * 0.3), "plaster_sky")
        k.box((fw + 0.08, 0.36, 0.06), (0, -l / 2 + 0.16, h + 0.3 + i * 0.3), "stone")
    gable(k, 0, 0.1, w, l - 0.4, h, 1.3, "roof_brown", "plaster_sky", over=0.12, ridge="y")
    chimney(k, 0.6, 0.9, h + 0.8, 0.8)
    front_windows(k, 0, -l / 2, w - 0.4, 0.3, 3, sh, 2, win=(0.4, 0.55), trim="white", skip=((0, 0),), seed=6)
    k.door((-0.52, -l / 2, 0.25), (0, -1, 0), 0.55, 1.0, "door", frame="white")
    k.box((1.0, 0.2, 0.08), (-0.52, -l / 2 - 0.1, 1.35), "stone")
    for sx in (-1, 1):
        side_windows(k, sx * w / 2, sx, l - 0.8, 0.3, 3, sh, 2, win=(0.32, 0.48), trim="white")
    return k


# ---------------------------------------------------------------- village

@model
def vil_log_house():
    """Log house: horizontal timber walls, blue-framed windows, a wooden porch and a green sheet roof."""
    k = Kit("vil_log_house")
    w, l, h = 3.4, 3.0, 1.45
    k.box((w, l, h + 0.18), (0, 0, (h + 0.18) / 2), "log", faces={"top": "log_d"})
    for sx in (-1, 1):  # log ends sticking out at the corners
        for sy in (-1, 1):
            for i in range(5):
                k.box((0.16, 0.16, 0.12), (sx * (w / 2 + 0.02), sy * (l / 2 + 0.02), 0.3 + i * 0.28), "log_d")
    gable(k, 0, 0, w, l, 0.18 + h, 1.1, "corr_green", "log", over=0.3, ridge="x")
    chimney(k, -0.9, 0.3, 0.18 + h + 0.6, 0.9, "brick")
    front_windows(k, 0, -l / 2, w - 0.6, 0.25, 1, 1.3, 3, win=(0.4, 0.5), trim="trim_blue", skip=((0, 1),), seed=7)
    for x in (-0.95, 0.95):  # carved shutters
        for s in (-1, 1):
            k.box((0.12, 0.04, 0.58), (x + s * 0.34, -l / 2 - 0.03, 0.18 + 0.4 + 0.25), "trim_blue")
    k.door((0, -l / 2, 0.2), (0, -1, 0), 0.55, 0.95, "door", frame="trim_blue")
    k.box((1.3, 0.8, 0.07), (0, -l / 2 - 0.4, 1.25), "corr_green")  # porch roof
    for sx in (-0.6, 0.6):
        k.box((0.09, 0.09, 1.05), (sx, -l / 2 - 0.75, 0.72), "wood")
    k.box((1.3, 0.8, 0.07), (0, -l / 2 - 0.4, 0.22), "wood")
    for sx in (-1, 1):
        side_windows(k, sx * w / 2, sx, l - 0.8, 0.25, 1, 1.3, 2, win=(0.36, 0.46), trim="trim_blue")
    return k


@model
def vil_white_house():
    """Whitewashed village house under a hipped thatch roof, blue trim and a veranda."""
    k = Kit("vil_white_house")
    w, l, h = 3.3, 3.1, 1.5
    k.box((w, l, h + 0.2), (0, 0, (h + 0.2) / 2), "plaster_w", faces={"top": "plaster_w"})
    hip(k, 0, 0, w, l, 0.2 + h, 1.3, "thatch", over=0.42)
    chimney(k, 0.7, 0.2, 0.2 + h + 0.8, 0.8, "brick")
    front_windows(k, 0, -l / 2, w - 0.5, 0.3, 1, 1.3, 3, win=(0.4, 0.52), shutter="shutter_b", trim="trim_blue", skip=((0, 1),), seed=8)
    k.door((0, -l / 2, 0.2), (0, -1, 0), 0.55, 0.95, "door", frame="trim_blue")
    k.box((1.5, 0.7, 0.07), (0, -l / 2 - 0.35, 1.3), "thatch")
    for sx in (-0.65, 0.65):
        k.box((0.09, 0.09, 1.1), (sx, -l / 2 - 0.65, 0.72), "trim_blue")
    k.box((1.5, 0.7, 0.07), (0, -l / 2 - 0.35, 0.22), "wood")
    for sx in (-1, 1):
        side_windows(k, sx * w / 2, sx, l - 0.8, 0.3, 1, 1.3, 2, win=(0.36, 0.48), trim="trim_blue")
    return k


# ---------------------------------------------------------------- Soviet

@model
def sov_stalinka():
    """Post-war three-storey block: yellow plaster, a columned entrance, a cornice and a hipped sheet roof."""
    k = Kit("sov_stalinka")
    w, l, sh = 3.8, 3.4, 1.15
    h = 3 * sh
    k.box((w, l, h), (0, 0, h / 2), "plaster_y", faces={"top": "concrete"})
    k.box((w + 0.25, l + 0.25, 0.16), (0, 0, h + 0.05), "plaster_w")  # cornice
    hip(k, 0, 0, w, l, h + 0.1, 0.75, "corr_green", over=0.2)
    front_windows(k, 0, -l / 2, w - 0.5, 0.3, 3, sh, 4, win=(0.36, 0.56), trim="white", skip=((0, 1), (0, 2)), seed=9)
    # entrance portico
    k.box((1.0, 0.55, 0.12), (0, -l / 2 - 0.28, 1.45), "concrete")
    for sx in (-0.42, 0.42):
        k.cyl(0.08, 1.2, (sx, -l / 2 - 0.5, 0.3), "plaster_w", axis="z", seg=10)
    k.door((0, -l / 2, 0.3), (0, -1, 0), 0.7, 1.1, "metal_door", frame="white")
    k.box((1.2, 0.5, 0.08), (0, -l / 2 - 0.25, 0.28), "concrete")
    for sx in (-1, 1):
        side_windows(k, sx * w / 2, sx, l - 0.6, 0.3, 3, sh, 3, win=(0.34, 0.54), trim="white")
    return k


@model
def sov_block5():
    """Five-storey prefab slab (footprint 4): three stairwell entrances, a balcony strip, a flat tarred roof."""
    k = Kit("sov_block5")
    w, l, sh = 7.4, 3.4, 1.2
    h = 5 * sh
    k.box((w, l, h + 0.3), (0, 0, (h + 0.3) / 2), "panel_grey", faces={"top": "roof_tar"})
    k.box((w + 0.12, l + 0.12, 0.2), (0, 0, 0.3 + h + 0.05), "concrete")
    front_windows(k, 0, -l / 2, w - 0.6, 0.55, 5, sh, 9, win=(0.3, 0.5), trim="white", skip=tuple((0, c) for c in (1, 4, 7)), lit=0.1, seed=10)
    for x in (-2.4, 0.0, 2.4):
        k.box((1.3, 0.55, 0.1), (x, -l / 2 - 0.27, 1.7), "concrete")  # entrance canopy
        k.door((x, -l / 2, 0.3), (0, -1, 0), 0.7, 1.15, "metal_door", frame="white")
    for f in range(1, 5):
        for x in (-3.0, -1.2, 1.2, 3.0):
            balcony(k, x, -l / 2, 0.3 + f * sh, 0.9, d=0.4)
    k.windows((0, l / 2, 0.3 + h / 2), (0, 1, 0), w - 0.6, h, 5, 9, win=(0.3, 0.5), sill=0.35, skip=tuple((0, c) for c in (1, 4, 7)))
    for sx in (-1, 1):
        k.box((0.9, 0.9, 0.5), (sx * 2.2, 0.0, 0.3 + h + 0.35), "concrete", faces={"top": "roof_tar"})  # roof access
    return k


# ---------------------------------------------------------------- industrial

@model
def ind_flat_house():
    """Flat-roofed utilitarian house of block and brick: ribbon windows, a steel door, roof vents and a tank."""
    k = Kit("ind_flat_house")
    w, l, h = 3.5, 3.4, 2.4
    k.box((w, l, h * 0.4), (0, 0, h * 0.2), "brick_y")
    k.box((w, l, h * 0.6), (0, 0, h * 0.4 + h * 0.3), "concrete_l", faces={"top": "roof_tar"})
    k.box((w + 0.2, l + 0.2, 0.14), (0, 0, h + 0.07), "concrete")
    front_windows(k, 0, -l / 2, w - 0.4, 0.2, 2, 1.1, 3, win=(0.5, 0.42), trim="steel_d", skip=((0, 1),), seed=11)
    k.door((0, -l / 2, 0.1), (0, -1, 0), 0.7, 1.1, "metal_door", frame="steel_d")
    k.box((1.0, 0.45, 0.07), (0, -l / 2 - 0.22, 1.35), "steel_d")
    for sx in (-1, 1):
        side_windows(k, sx * w / 2, sx, l - 0.8, 0.2, 2, 1.1, 2, win=(0.44, 0.4), trim="steel_d")
    for x in (-1.0, 0.1):
        k.box((0.5, 0.5, 0.4), (x, 0.7, h + 0.34), "steel_d", faces={"top": "roof_tar"})  # vents
    k.cyl(0.4, 0.8, (1.0, 0.9, h + 0.14), "corr", axis="z", seg=12)  # tank
    k.box((0.05, 1.2, 0.04), (1.0, 0.3, h + 0.5), "gun")
    return k
