"""Churches, shops and apartment blocks per theme, and the city buildings (mall, hospital, station, school,
hotel, office tower, theatre, parking garage, market hall, fire and police station, filling station).
Footprint 2 = 4 x 4 units, 4 = 8 x 8, 6 = 12 x 12. Front is -y."""
import math

from kit import Kit, model, LOW
from models_themed import gable, hip, chimney, front_windows, side_windows, balcony


def dome(k, x, y, z, r, mat="yellow", drum=0.0, drum_mat="plaster_w"):
    """Onion dome with a drum, a spire and a cross."""
    if drum:
        k.cyl(r * 0.8, drum, (x, y, z), drum_mat, axis="z", seg=10)
    z += drum
    k.sphere(r, (x, y, z + r * 0.9), mat, seg=12, rings=6, scale=(1, 1, 1.25))
    k.cyl(r * 0.28, r * 0.9, (x, y, z + r * 1.9), mat, axis="z", seg=8, r2=0.01)
    top = z + r * 2.8
    k.box((0.03, 0.03, r * 0.9), (x, y, top + r * 0.4), mat)
    k.box((r * 0.5, 0.03, 0.03), (x, y, top + r * 0.55), mat)


def pyramid(k, x, y, z, w, h, mat):
    k.box((w, w, h), (x, y, z + h / 2), mat, taper=(0.04, 0.04))


def arch_row(k, y, xs, w, h, z, mat="black", normal=-1):
    for x in xs:
        k.box((w, 0.05, h), (x, y + normal * 0.02, z + h / 2), mat)
        k.cyl(w / 2, 0.05, (x, y + normal * 0.02 + 0.025 * (1 if normal > 0 else 0), z + h), mat, axis="y", seg=10)


# ------------------------------------------------------------ churches

@model
def church_orthodox():
    """Orthodox church: whitewashed cross-plan nave under a green roof, a gold central dome and four small ones, a belfry porch."""
    k = Kit("church_orthodox")
    k.box((3.4, 3.4, 0.2), (0, 0, 0.1), "stone_d")
    k.box((3.0, 3.0, 1.9), (0, 0, 1.15), "plaster_w", faces={"top": "concrete"})
    k.box((1.5, 3.8, 1.6), (0, 0, 1.0), "plaster_w", faces={"top": "concrete"})  # transept
    k.box((3.7, 1.5, 1.6), (0, 0, 1.0), "plaster_w", faces={"top": "concrete"})
    k.box((3.04, 3.04, 0.1), (0, 0, 2.15), "plaster_sky")
    hip(k, 0, 0, 3.0, 3.0, 2.2, 0.8, "corr_green", over=0.12)
    k.box((1.5, 3.8, 0.1), (0, 0, 1.85), "plaster_sky")
    gable(k, 0, 0, 1.5, 3.8, 1.9, 0.5, "corr_green", "plaster_w", over=0.1, ridge="y")
    gable(k, 0, 0, 3.7, 1.5, 1.9, 0.5, "corr_green", "plaster_w", over=0.1, ridge="x")
    dome(k, 0, 0, 3.0, 0.55, drum=0.75)
    for sx in (-1, 1):
        for sy in (-1, 1):
            dome(k, sx * 1.25, sy * 1.25, 2.2, 0.26, drum=0.35)
    k.box((1.0, 0.9, 2.6), (0, -2.0, 1.3), "plaster_w", faces={"top": "concrete"})  # belfry porch
    dome(k, 0, -2.0, 2.6, 0.3, drum=0.3)
    k.windows((0, -1.55, 1.1), (0, -1, 0), 2.6, 1.6, 1, 3, win=(0.28, 0.7), sill=0.3, frame="plaster_sky")
    for sx in (-1, 1):
        k.windows((sx * 1.51, 0.0, 1.1), (sx, 0, 0), 2.8, 1.6, 1, 2, win=(0.26, 0.7), sill=0.3, frame="plaster_sky")
    k.door((0, -2.46, 0.2), (0, -1, 0), 0.55, 1.1, "door", frame="plaster_sky")
    return k


@model
def church_baroque():
    """Adriatic parish church: a stone nave with a front gable and a round window, a campanile with a terracotta pyramid cap."""
    k = Kit("church_baroque")
    k.box((3.6, 3.8, 0.2), (0, 0, 0.1), "stone_d")
    k.box((2.5, 3.4, 2.1), (0.3, 0.2, 1.25), "plaster_cream", faces={"top": "concrete"})
    k.box((2.6, 0.12, 0.1), (0.3, 0.2 - 1.7, 2.3), "stone")
    gable(k, 0.3, 0.2, 2.5, 3.4, 2.3, 0.9, "terracotta", "plaster_cream", over=0.14, ridge="y")
    k.box((1.1, 1.1, 3.6), (-1.15, -1.5, 2.0), "stone", faces={"top": "stone_d"})  # campanile
    k.box((1.2, 1.2, 0.1), (-1.15, -1.5, 3.85), "stone_d")
    for sx, sy in ((1, 0), (-1, 0), (0, 1), (0, -1)):  # belfry openings
        k.box((0.04 + 0.22 * (sy != 0), 0.04 + 0.22 * (sx != 0), 0.5), (-1.15 + sx * 0.56, -1.5 + sy * 0.56, 3.45), "black")
    pyramid(k, -1.15, -1.5, 3.9, 1.3, 1.4, "terracotta")
    k.box((0.04, 0.04, 0.3), (-1.15, -1.5, 5.45), "yellow")
    k.cyl(0.2, 0.05, (0.3, 0.2 - 1.72, 1.85), "glass", axis="y", seg=10)  # round window
    k.door((0.3, -1.5, 0.2), (0, -1, 0), 0.7, 1.2, "door", frame="stone")
    for i in range(3):
        k.box((1.2 + i * 0.25, 0.25, 0.1), (0.3, -1.65 - i * 0.2, 0.15 - i * 0.04), "stone")
    k.windows((0.3 + 1.26, 0.2, 1.25), (1, 0, 0), 3.0, 1.8, 1, 3, win=(0.3, 0.7), sill=0.4, frame="stone")
    return k


@model
def church_gothic():
    """Town church: a tower with a tall slate spire, a steep nave with buttresses and tall pointed-style windows."""
    k = Kit("church_gothic")
    k.box((3.6, 3.9, 0.2), (0, 0, 0.1), "stone_d")
    k.box((2.2, 3.4, 2.4), (0.3, 0.2, 1.4), "stone", faces={"top": "stone_d"})
    gable(k, 0.3, 0.2, 2.2, 3.4, 2.6, 1.4, "roof_slate", "stone", over=0.12, ridge="y")
    for sy in (-1.2, 0.0, 1.2):
        for sx in (-1, 1):
            k.box((0.18, 0.3, 2.0), (0.3 + sx * 1.2, 0.2 + sy, 1.1), "stone_d", taper=(1.0, 0.55))
    k.box((1.3, 1.3, 3.8), (-0.9, -1.55, 2.0), "stone", faces={"top": "stone_d"})
    k.box((1.4, 1.4, 0.1), (-0.9, -1.55, 3.95), "stone_d")
    k.box((1.3, 1.3, 2.6), (-0.9, -1.55, 5.3), "roof_slate", taper=(0.03, 0.03))
    for sx, sy in ((1, 1), (-1, 1), (1, -1), (-1, -1)):
        k.box((0.14, 0.14, 0.7), (-0.9 + sx * 0.6, -1.55 + sy * 0.6, 4.35), "roof_slate", taper=(0.1, 0.1))
    k.box((0.5, 0.05, 1.0), (-0.9, -1.55 - 0.66, 3.2), "black")
    k.box((0.35, 0.05, 0.8), (-0.9, -1.55 - 0.66, 1.7), "glass")
    k.door((-0.9, -2.2, 0.2), (0, -1, 0), 0.6, 1.1, "door", frame="stone")
    for sx in (-1, 1):
        k.windows((0.3 + sx * 1.11, 0.2, 1.5), (sx, 0, 0), 3.0, 2.2, 1, 3, win=(0.3, 1.1), sill=0.5, frame="stone", mat="canopy")
    k.box((0.5, 0.05, 1.2), (0.3, 0.2 - 1.72, 1.7), "canopy")
    return k


# ------------------------------------------------------------ shops

@model
def shop_adriatic():
    """Stone shop with an arched shopfront, a striped awning and a low terracotta roof."""
    k = Kit("shop_adriatic")
    w, l, h = 3.5, 3.0, 1.7
    k.box((w, l, h), (0, 0, h / 2), "stone", faces={"top": "stone_d"})
    hip(k, 0, 0, w, l, h + 0.02, 0.6, "terracotta", over=0.25)
    k.box((1.9, 0.05, 1.0), (-0.3, -l / 2 - 0.01, 0.8), "glass")
    k.cyl(0.95, 0.05, (-0.3, -l / 2 - 0.02, 1.3), "glass", axis="y", seg=14)
    k.box((2.1, 0.07, 0.07), (-0.3, -l / 2 - 0.03, 0.3), "stone")
    k.door((1.1, -l / 2, 0.2), (0, -1, 0), 0.55, 1.0, "door", frame="stone")
    with k.at((-0.3, -l / 2 - 0.45, 1.55), rot=(25, 0, 0)):
        for i in range(8):
            k.box((0.26, 0.9, 0.04), (-1.0 + i * 0.28, 0, 0), "red" if i % 2 == 0 else "white")
    k.box((1.6, 0.06, 0.3), (-0.3, -l / 2 - 0.04, 1.72), "shutter_g")  # shop sign
    chimney(k, 1.0, 0.6, h + 0.3, 0.5, "stone")
    k.windows((w / 2, 0, h / 2), (1, 0, 0), l - 0.8, h, 1, 2, win=(0.34, 0.5), sill=0.4, frame="stone")
    return k


@model
def shop_central():
    """Corner shop: a glazed ground floor with a sign band, a pastel upper floor and a tiled gable."""
    k = Kit("shop_central")
    w, l = 3.4, 3.1
    k.box((w, l, 1.2), (0, 0, 0.6), "plaster_cream", faces={"top": "stone"})
    k.box((w, l, 1.15), (0, 0, 1.2 + 0.575), "plaster_pink", faces={"top": "concrete"})
    k.box((w - 0.4, 0.06, 0.9), (-0.1, -l / 2 - 0.01, 0.65), "glass")
    for x in (-1.0, -0.1, 0.8):
        k.box((0.06, 0.08, 0.95), (x, -l / 2 - 0.03, 0.65), "white")
    k.box((w - 0.4, 0.1, 0.28), (-0.1, -l / 2 - 0.04, 1.28), "shutter_r")  # sign band
    k.door((1.25, -l / 2, 0.15), (0, -1, 0), 0.5, 0.95, "door", frame="white")
    gable(k, 0, 0, w, l, 2.35, 1.1, "roof_brown", "plaster_pink", over=0.2, ridge="x")
    front_windows(k, 0, -l / 2, w - 0.6, 1.3, 1, 1.1, 3, win=(0.4, 0.5), trim="white", seed=12)
    for sx in (-1, 1):
        side_windows(k, sx * w / 2, sx, l - 0.8, 1.3, 1, 1.1, 2, win=(0.36, 0.5), trim="white")
        side_windows(k, sx * w / 2, sx, l - 0.8, 0.2, 1, 1.0, 2, win=(0.5, 0.5), trim="white")
    chimney(k, -0.9, 0.3, 2.9, 0.7)
    return k


@model
def shop_soviet():
    """Gastronom: a flat concrete box with a glazed front, a blue sign and a concrete entrance canopy."""
    k = Kit("shop_soviet")
    w, l, h = 3.7, 3.2, 1.8
    k.box((w, l, h), (0, 0, h / 2), "concrete_l", faces={"top": "roof_tar"})
    k.box((w + 0.12, l + 0.12, 0.15), (0, 0, h + 0.04), "concrete")
    k.box((w - 0.5, 0.06, 1.2), (0, -l / 2 - 0.01, 0.8), "glass")
    for i in range(5):
        k.box((0.06, 0.08, 1.25), (-1.5 + i * 0.75, -l / 2 - 0.03, 0.8), "steel_d")
    k.box((w - 0.3, 0.12, 0.4), (0, -l / 2 - 0.05, 1.55), "trim_blue")
    k.box((1.3, 0.7, 0.1), (0, -l / 2 - 0.35, 1.3), "concrete")
    for sx in (-0.55, 0.55):
        k.box((0.08, 0.08, 1.2), (sx, -l / 2 - 0.65, 0.65), "steel_d")
    k.door((0, -l / 2 - 0.02, 0.1), (0, -1, 0), 0.8, 1.0, "metal_door", frame="steel_d")
    k.box((0.9, 0.9, 0.5), (0.9, 0.6, h + 0.35), "steel_d", faces={"top": "roof_tar"})
    return k


@model
def shop_village():
    """Village shop: a whitewashed house with a green sheet roof, a porch and a painted sign."""
    k = Kit("shop_village")
    w, l, h = 3.2, 3.0, 1.35
    k.box((w, l, h + 0.2), (0, 0, (h + 0.2) / 2), "plaster_w")
    gable(k, 0, 0, w, l, 0.2 + h, 0.95, "corr_green", "plaster_w", over=0.28, ridge="x")
    front_windows(k, 0, -l / 2, w - 0.6, 0.3, 1, 1.3, 2, win=(0.7, 0.55), shutter="shutter_b", trim="trim_blue", skip=((0, 1),), seed=13)
    k.door((0.9, -l / 2, 0.2), (0, -1, 0), 0.6, 1.0, "door", frame="trim_blue")
    k.box((1.4, 0.7, 0.07), (0.9, -l / 2 - 0.35, 1.3), "corr_green")
    for sx in (0.35, 1.45):
        k.box((0.08, 0.08, 1.1), (sx, -l / 2 - 0.65, 0.75), "trim_blue")
    k.box((1.4, 0.05, 0.3), (-0.6, -l / 2 - 0.03, 1.5), "trim_blue")
    chimney(k, -0.8, 0.5, 2.0, 0.6)
    return k


# ------------------------------------------------------------ apartments

@model
def adr_apartments():
    """Seaside apartment house: four storeys of pink plaster, shutters, balconies and a low terracotta roof."""
    k = Kit("adr_apartments")
    w, l, sh = 3.6, 3.2, 1.0
    h = 4 * sh + 0.4
    k.box((w, l, h), (0, 0, h / 2), "plaster_pink", faces={"top": "concrete"})
    k.box((w, l, 0.4 + sh * 0.0), (0, 0, 0.2), "stone")
    k.box((w + 0.12, l + 0.12, 0.12), (0, 0, h + 0.04), "stone")
    hip(k, 0, 0, w, l, h + 0.1, 0.55, "terracotta", over=0.25)
    front_windows(k, 0, -l / 2, w - 0.5, 0.4, 4, sh, 3, win=(0.34, 0.5), shutter="shutter_g", trim="stone", skip=((0, 1),), seed=14)
    k.door((0, -l / 2, 0.15), (0, -1, 0), 0.6, 1.0, "door", frame="stone")
    for f in (2, 3):
        for x in (-1.1, 1.1):
            balcony(k, x, -l / 2, 0.4 + f * sh - 0.05, 0.9, d=0.4)
    for sx in (-1, 1):
        side_windows(k, sx * w / 2, sx, l - 0.8, 0.4, 4, sh, 2, win=(0.32, 0.5), trim="stone")
    return k


@model
def ce_tenement():
    """City tenement: four storeys with stucco window trim, shop windows below and a slate mansard roof."""
    k = Kit("ce_tenement")
    w, l, sh = 3.7, 3.3, 1.0
    h = 4 * sh + 0.3
    k.box((w, l, h), (0, 0, h / 2), "plaster_g", faces={"top": "roof_tar"})
    k.box((w + 0.18, l + 0.18, 0.14), (0, 0, h + 0.05), "plaster_cream")
    k.box((w + 0.08, l + 0.08, 0.06), (0, 0, 0.3 + sh), "plaster_cream")  # string courses
    k.box((w + 0.08, l + 0.08, 0.06), (0, 0, 0.3 + 3 * sh), "plaster_cream")
    k.box((w, l, 0.9), (0, 0, h + 0.5), "roof_slate", taper=(0.88, 0.86), faces={"bottom": "roof_slate"})  # mansard
    k.box((w * 0.88, l * 0.86, 0.45), (0, 0, h + 1.15), "roof_slate", taper=(0.6, 0.55))
    for x in (-1.0, 0.0, 1.0):  # dormers
        k.box((0.4, 0.4, 0.42), (x, -l * 0.43 - 0.05, h + 0.45), "plaster_cream")
        k.box((0.5, 0.5, 0.15), (x, -l * 0.43 - 0.05, h + 0.74), "roof_slate", taper=(0.8, 0.3))
        k.box((0.22, 0.04, 0.26), (x, -l * 0.43 - 0.27, h + 0.45), "glass")
    front_windows(k, 0, -l / 2, w - 0.5, 1.3, 3, sh, 3, win=(0.38, 0.55), trim="white", seed=15)
    k.box((w - 0.5, 0.05, 0.7), (0, -l / 2 - 0.01, 0.65), "glass")
    for x in (-1.2, 0.0, 1.2):
        k.box((0.07, 0.08, 0.75), (x + 0.6, -l / 2 - 0.03, 0.65), "white")
    k.door((-1.4, -l / 2, 0.3), (0, -1, 0), 0.5, 0.9, "door", frame="white")
    for sx in (-1, 1):
        side_windows(k, sx * w / 2, sx, l - 0.8, 0.3, 4, sh, 2, win=(0.34, 0.5), trim="white")
    return k


@model
def adr_apartment_block():
    """Large Mediterranean apartment block (footprint 4): ochre plaster, shutters, loggias and a terracotta hip roof."""
    k = Kit("adr_apartment_block")
    w, l, sh = 7.4, 4.6, 1.1
    h = 4 * sh + 0.5
    k.box((w, l, h), (0, 0, h / 2), "plaster_ochre", faces={"top": "concrete"})
    k.box((w + 0.2, l + 0.2, 0.14), (0, 0, h + 0.05), "stone")
    hip(k, 0, 0, w, l, h + 0.12, 1.0, "terracotta", over=0.3)
    front_windows(k, 0, -l / 2, w - 0.8, 0.5, 4, sh, 7, win=(0.36, 0.56), shutter="shutter_g", trim="stone", skip=((0, 3),), seed=16)
    k.door((0, -l / 2, 0.3), (0, -1, 0), 0.8, 1.2, "door", frame="stone")
    for f in (1, 2, 3):
        for x in (-2.7, -1.35, 1.35, 2.7):
            balcony(k, x, -l / 2, 0.5 + f * sh - 0.05, 0.95, d=0.45)
    k.windows((0, l / 2, 0.5 + 2 * sh), (0, 1, 0), w - 0.8, 4 * sh, 4, 7, win=(0.36, 0.56), sill=0.35, frame="stone")
    for sx in (-1, 1):
        side_windows(k, sx * w / 2, sx, l - 0.8, 0.5, 4, sh, 3, win=(0.34, 0.54), trim="stone")
    for x in (-2.2, 2.0):
        chimney(k, x, 0.5, h + 0.9, 0.9, "stone")
    return k


@model
def ce_apartment_block():
    """Corner tenement block (footprint 4): five storeys, a round corner turret with a spire and a slate mansard."""
    k = Kit("ce_apartment_block")
    sh = 1.0
    h = 5 * sh + 0.4
    for (x, y, w, l) in ((0.0, -2.2, 7.4, 2.8), (-2.3, 1.4, 2.8, 4.4)):
        k.box((w, l, h), (x, y, h / 2), "plaster_cream", faces={"top": "roof_tar"})
        k.box((w + 0.16, l + 0.16, 0.14), (x, y, h + 0.04), "plaster_w")
        k.box((w + 0.06, l + 0.06, 0.07), (x, y, 0.4 + sh), "plaster_w")
        k.box((w, l, 0.8), (x, y, h + 0.45), "roof_slate", taper=(0.9, 0.86))
    k.cyl(0.9, h + 0.2, (-3.0, -2.9, 0), "plaster_pink", axis="z", seg=12)
    k.cyl(1.0, 0.12, (-3.0, -2.9, h + 0.1), "plaster_w", axis="z", seg=12)
    k.cyl(1.0, 1.6, (-3.0, -2.9, h + 0.22), "roof_slate", axis="z", seg=12, r2=0.04)
    front_windows(k, 0.5, -3.6, 5.6, 0.5, 5, sh, 6, win=(0.36, 0.55), trim="white", skip=((0, 2), (0, 3)), seed=17)
    front_windows(k, 0.5, -3.6, 5.6, 0.5, 0, sh, 1, win=(0.0, 0.0)) if False else None
    k.door((0.5, -3.6, 0.4), (0, -1, 0), 0.8, 1.2, "door", frame="white")
    for f in range(1, 5):
        for x in (-0.6, 2.4):
            balcony(k, x, -3.6, 0.4 + f * sh - 0.05, 0.9, d=0.4, mat="plaster_w")
    k.windows((-3.7, 1.4, 0.4 + 2.5 * sh), (-1, 0, 0), 3.6, 5 * sh, 5, 3, win=(0.36, 0.55), sill=0.3, frame="white")
    k.windows((3.7, -2.2, 0.4 + 2.5 * sh), (1, 0, 0), 2.2, 5 * sh, 5, 2, win=(0.36, 0.55), sill=0.3, frame="white")
    return k


# ------------------------------------------------------------ city buildings

@model
def mall():
    """Shopping mall (footprint 6): a two-storey box with a glass curtain wall, a tall entrance atrium, a sign band and roof plant."""
    k = Kit("mall")
    with k.nohit_():
        for i in range(0 if LOW else 8):  # parked cars
            k.box((0.9, 0.5, 0.35), (-4.5 + i * 1.3, -5.0, 0.25), ["red", "white", "steel", "black", "yellow"][i % 5])
    w, l, h = 10.6, 7.0, 3.6
    k.box((w, l, h), (0, 1.8, h / 2), "concrete_l", faces={"top": "roof_tar"})
    k.box((w + 0.2, l + 0.2, 0.2), (0, 1.8, h + 0.05), "concrete")
    yf = 1.8 - l / 2
    k.box((w - 0.6, 0.08, 2.4), (0, yf - 0.02, 1.4), "glass")
    for i in range(0 if LOW else 11):
        k.box((0.07, 0.1, 2.5), (-w / 2 + 0.5 + i * (w - 1.0) / 10, yf - 0.04, 1.4), "steel_d")
    k.box((w - 0.6, 0.1, 0.1), (0, yf - 0.04, 1.4), "steel_d")
    k.box((w - 0.2, 0.18, 0.5), (0, yf - 0.1, 3.0), "red")  # sign band
    k.box((3.0, 1.0, 4.6), (0, yf - 0.4, 2.3), "glass_lit", faces={"top": "concrete"})  # atrium
    k.box((3.2, 1.2, 0.2), (0, yf - 0.4, 4.7), "concrete")
    k.box((2.4, 0.06, 1.6), (0, yf - 0.95, 0.9), "glass")
    for sx in (-1, 1):
        k.box((0.1, 0.1, 2.2), (sx * 1.2, yf - 0.95, 1.1), "steel_d")
    k.box((3.4, 1.4, 0.12), (0, yf - 1.3, 2.3), "concrete")  # entrance canopy
    for i in range(2 if LOW else 5):
        k.box((1.0, 1.0, 0.7), (-3.8 + i * 1.9, 2.8, h + 0.45), "steel_d", faces={"top": "roof_tar"})
    for sx in (-1, 1):
        k.box((0.5, 5.0, 2.0), (sx * (w / 2 + 0.25), 2.0, 1.0), "concrete_l")
    k.box((0.2, 0.2, 4.0), (4.6, yf - 1.5, 2.0), "steel_d")  # pylon sign
    k.box((1.0, 0.1, 1.6), (4.6, yf - 1.5, 4.0), "red")
    return k


@model
def hospital():
    """Hospital (footprint 6): a white four-storey central block with two wings, a red cross, an ambulance bay and a rooftop helipad."""
    k = Kit("hospital")
    sh = 1.1
    h = 4 * sh
    k.box((4.8, 4.2, h + 0.8), (0, 0.8, (h + 0.8) / 2), "plaster_w", faces={"top": "roof_tar"})
    for sx in (-1, 1):
        k.box((3.2, 6.0, h - 1.1), (sx * 4.0, 0.2, (h - 1.1) / 2), "plaster_w", faces={"top": "roof_tar"})
        k.box((3.3, 6.1, 0.1), (sx * 4.0, 0.2, h - 1.1), "concrete")
    k.box((4.9, 4.3, 0.12), (0, 0.8, h + 0.8), "concrete")
    k.box((1.6, 0.06, 1.6), (0, 0.8 - 2.12, h - 0.2), "red")  # cross
    k.box((0.5, 0.07, 1.6), (0, 0.8 - 2.13, h - 0.2), "white")
    k.box((1.6, 0.07, 0.5), (0, 0.8 - 2.13, h - 0.2), "white")
    front_windows(k, 0, 0.8 - 2.1, 4.2, 0.2, 3, sh, 5, win=(0.4, 0.52), trim="steel", skip=((0, 2),), lit=0.1, seed=18)
    for sx in (-1, 1):
        k.windows((sx * 4.0, 0.2 - 3.0, (h - 1.1) / 2), (0, -1, 0), 3.0, h - 1.1, 3, 3, win=(0.4, 0.5), sill=0.3, frame="steel")
        k.windows((sx * 5.61, 0.2, (h - 1.1) / 2), (sx, 0, 0), 5.0, h - 1.1, 3, 4, win=(0.4, 0.5), sill=0.3, frame="steel")
    k.box((3.4, 1.8, 0.12), (0, -2.2, 1.7), "concrete")  # ambulance canopy
    for sx in (-1.5, 1.5):
        k.box((0.12, 0.12, 1.5), (sx, -2.9, 0.85), "steel_d")
    k.door((0, -1.3, 0.1), (0, -1, 0), 1.2, 1.3, "glass", frame="steel")
    k.box((1.0, 0.5, 0.5), (-4.0, -2.6, 0.3), "white")  # ambulance
    k.box((0.4, 0.4, 0.05), (-4.0, -2.6, 0.58), "red")
    k.cyl(1.0, 0.06, (0, 0.8, h + 0.92), "concrete_d", axis="z", seg=16)  # helipad
    k.box((0.14, 0.9, 0.02), (0, 0.8, h + 1.0), "yellow")
    k.box((0.14, 0.9, 0.02), (0.4, 0.8, h + 1.0), "yellow", rot=(0, 0, 90)) if False else None
    k.box((0.7, 0.12, 0.02), (0, 0.8, h + 1.0), "yellow")
    return k


@model
def train_station():
    """Railway station (footprint 6): a long hall with a glazed roof, a clock tower, platform canopies and tracks."""
    k = Kit("train_station")
    with k.nohit_():
        for i in range(4):
            k.box((11.8, 0.1, 0.06), (0, 0.6 + i * 1.6, 0.08), "steel_d")
            for j in range(0 if LOW else 23):
                k.box((0.12, 0.8, 0.04), (-5.8 + j * 0.5, 0.6 + i * 1.6, 0.06), "wood")
    k.box((10.8, 3.2, 2.6), (0, -3.6, 1.3), "plaster_cream", faces={"top": "roof_tar"})
    k.box((10.9, 3.3, 0.2), (0, -3.6, 0.1), "stone_d")
    gable(k, 0, -3.6, 10.8, 3.2, 2.6, 0.9, "roof_slate", "plaster_cream", over=0.3, ridge="x")
    k.box((1.8, 1.8, 4.8), (0, -3.6, 2.4), "plaster_cream", faces={"top": "stone"})  # clock tower
    k.box((1.9, 1.9, 0.14), (0, -3.6, 4.85), "stone")
    k.box((1.9, 1.9, 1.1), (0, -3.6, 5.5), "roof_slate", taper=(0.1, 0.1))
    for sx, sy in ((0, -1), (0, 1), (1, 0), (-1, 0)):
        k.cyl(0.4, 0.05, (sx * 0.93, -3.6 + sy * 0.93, 4.0), "white", axis="y" if sx == 0 else "x", seg=14)
    k.box((10.4, 0.08, 1.7), (0, -5.22, 1.2), "glass")
    for i in range(9):
        k.box((0.4, 0.06, 1.3), (-4.6 + i * 1.15, -5.24, 1.1), "black") if i % 2 else None
    k.box((2.0, 0.1, 1.3), (0, -5.25, 0.8), "glass_lit")
    k.box((10.4, 3.0, 0.12), (0, 2.8, 2.3), "corr_white")  # platform canopy
    for i in range(7):
        k.box((0.1, 0.1, 2.3), (-4.8 + i * 1.6, 2.0, 1.15), "steel_d")
    k.box((10.8, 1.2, 0.3), (0, 1.8, 0.15), "concrete")
    return k


@model
def school():
    """School (footprint 4): a yellow three-storey main block with a lower wing, a flagpole and a sports field."""
    k = Kit("school")
    with k.nohit_():
        k.box((3.0, 2.0, 0.04), (2.3, 2.9, 0.06), "shutter_g")
    sh = 1.15
    k.box((7.0, 2.8, 3 * sh), (0, -1.2, 3 * sh / 2), "plaster_y", faces={"top": "roof_tar"})
    k.box((2.8, 4.2, 2 * sh), (-2.1, 1.7, sh), "plaster_y", faces={"top": "roof_tar"})
    k.box((7.1, 2.9, 0.16), (0, -1.2, 3 * sh), "concrete")
    k.box((2.9, 4.3, 0.16), (-2.1, 1.7, 2 * sh), "concrete")
    front_windows(k, 0, -2.6, 6.4, 0.3, 3, sh, 7, win=(0.5, 0.6), trim="white", skip=((0, 3),), seed=19)
    k.windows((-2.1, 3.8, sh), (0, 1, 0), 2.4, 2 * sh, 2, 3, win=(0.5, 0.6), sill=0.3, frame="white")
    k.windows((-3.5, 1.7, sh), (-1, 0, 0), 3.6, 2 * sh, 2, 3, win=(0.5, 0.6), sill=0.3, frame="white")
    k.door((0, -2.6, 0.2), (0, -1, 0), 1.0, 1.2, "glass", frame="white")
    k.box((1.8, 0.8, 0.12), (0, -3.0, 1.7), "concrete")
    k.box((0.04, 0.04, 2.4), (3.4, -3.1, 1.2), "steel")
    k.box((0.5, 0.02, 0.3), (3.65, -3.1, 2.25), "yellow")
    return k


@model
def hotel():
    """Hotel (footprint 4): a six-storey slab with lit rooms, balconies, a columned entrance canopy and a rooftop sign."""
    k = Kit("hotel")
    sh = 1.05
    h = 6 * sh
    k.box((5.6, 3.4, h + 0.9), (0, 0.0, (h + 0.9) / 2), "plaster_w", faces={"top": "roof_tar"})
    k.box((5.7, 3.5, 0.14), (0, 0, h + 0.97), "concrete")
    front_windows(k, 0, -1.7, 4.8, 1.0, 6, sh, 5, win=(0.4, 0.58), trim="steel", lit=0.25, seed=20)
    for f in range(1, 6):
        for x in (-1.9, 0.0, 1.9):
            balcony(k, x, -1.7, 1.0 + f * sh - 0.05, 1.0, d=0.35)
    k.box((4.0, 1.8, 0.14), (0, -2.8, 1.8), "concrete")
    for x in (-1.7, -0.6, 0.6, 1.7):
        k.cyl(0.1, 1.7, (x, -3.5, 0.1), "white", axis="z", seg=8)
    k.door((0, -1.7, 0.0), (0, -1, 0), 1.4, 1.6, "glass", frame="steel")
    k.box((3.0, 0.2, 0.8), (0, 0.0, h + 1.45), "red")
    k.windows((0, 1.7, 3.8), (0, 1, 0), 4.8, 6 * sh, 6, 5, win=(0.4, 0.58), sill=0.3, frame="steel", lit=0.2, seed=21)
    return k


@model
def office_tower():
    """Office tower (footprint 4): a glass and concrete tower of twelve floors on a low podium with a plaza."""
    k = Kit("office_tower")
    k.box((7.0, 6.0, 1.5), (0, 0.2, 0.75), "concrete_l", faces={"top": "concrete"})
    k.box((7.0, 0.08, 1.1), (0, 0.2 - 3.02, 0.8), "glass")
    k.box((3.8, 3.8, 11.0), (0, 0.4, 7.0), "glass", faces={"top": "roof_tar"})
    for i in range(0 if LOW else 6):
        k.box((0.14, 3.9, 11.0), (-1.9 + i * 0.76, 0.4, 7.0), "steel_d")
        k.box((3.9, 0.14, 11.0), (0, 0.4 - 1.9 + i * 0.76, 7.0), "steel_d")
    for f in range(0 if LOW else 12):
        k.box((3.9, 3.9, 0.1), (0, 0.4, 1.7 + f * 0.9), "concrete")
    k.box((3.9, 3.9, 0.2), (0, 0.4, 12.55), "concrete")
    k.box((1.2, 1.2, 0.8), (0.6, 0.4, 13.0), "steel_d", faces={"top": "roof_tar"})
    k.box((0.04, 0.04, 1.4), (-0.8, 0.4, 13.5), "steel")
    k.box((2.4, 0.6, 0.1), (0, -2.6, 1.4), "steel_d")
    k.door((0, 0.2 - 3.0, 0.1), (0, -1, 0), 1.6, 1.2, "glass", frame="steel")
    return k


@model
def theatre():
    """Theatre (footprint 4): a classical hall with a columned portico and pediment, a stage house behind and a copper dome."""
    k = Kit("theatre")
    k.box((6.6, 6.0, 0.4), (0, 0, 0.2), "stone")
    k.box((5.0, 4.4, 3.2), (0, 0.8, 2.0), "plaster_cream", faces={"top": "roof_tar"})
    k.box((5.2, 4.6, 0.2), (0, 0.8, 3.7), "stone")
    k.box((3.4, 3.4, 2.4), (0, 1.8, 4.9), "plaster_cream", faces={"top": "roof_tar"})  # stage house
    k.sphere(1.5, (0, 0.2, 3.6), "copper", seg=14, rings=6, scale=(1, 1, 0.7), half=True)
    k.box((4.6, 1.6, 0.18), (0, -2.3, 3.2), "stone")
    k.prism([(-2.3, 0.0), (2.3, 0.0), (0, 0.8)], "y", -3.0, -1.5, "plaster_cream", caps={0: "plaster_cream", 1: "plaster_cream"})
    with k.at((0, 0, 3.28)):
        k.prism([(-2.3, 0.0), (2.3, 0.0), (0, 0.8)], "y", -3.1, -1.5, "stone") if False else None
    for i in range(6):
        k.cyl(0.14, 2.8, (-2.0 + i * 0.8, -2.8, 0.4), "plaster_w", axis="z", seg=10)
    k.box((4.6, 0.2, 0.3), (0, -3.05, 3.2), "stone")
    for sx in (-1, 1):
        k.windows((sx * 2.51, 0.8, 2.0), (sx, 0, 0), 3.8, 2.4, 1, 3, win=(0.4, 1.1), sill=0.5, frame="stone")
    k.windows((0, -1.4, 1.9), (0, -1, 0), 4.0, 2.3, 1, 3, win=(0.5, 1.2), sill=0.4, frame="stone") if False else None
    k.door((0, -1.4, 0.4), (0, -1, 0), 1.4, 1.6, "door", frame="stone")
    return k


@model
def parking_garage():
    """Multi-storey car park (footprint 4): open concrete decks on pillars with parapets, a ramp and parked cars."""
    k = Kit("parking_garage")
    cars = ["red", "white", "steel", "black", "yellow", "olive", "white"]
    for d in range(4):
        z = 0.12 + d * 1.5
        k.box((7.4, 7.4, 0.22), (0, 0, z), "concrete")
        k.box((7.4, 0.15, 0.55), (0, -3.62, z + 0.38), "concrete_l")
        k.box((7.4, 0.15, 0.55), (0, 3.62, z + 0.38), "concrete_l")
        k.box((0.15, 7.0, 0.55), (-3.62, 0, z + 0.38), "concrete_l")
        k.box((0.15, 7.0, 0.55), (3.62, 0, z + 0.38), "concrete_l")
        for i in range(0 if LOW else 5):
            for j in range(2):
                if (i + j + d) % 3:
                    k.box((0.5, 0.95, 0.3), (-2.8 + i * 1.4, -2.6 + j * 5.0, z + 0.28), cars[(i + 2 * j + d) % len(cars)])
        if d:
            for sx in (-3.4, 0.0, 3.4):
                for sy in (-3.4, 0.0, 3.4):
                    k.box((0.3, 0.3, 1.3), (sx, sy, z - 0.8), "concrete")
    for sx in (-3.4, 0.0, 3.4):
        for sy in (-3.4, 0.0, 3.4):
            k.box((0.3, 0.3, 1.3), (sx, sy, 0.7), "concrete")
    with k.at((3.0, -1.0, 0.8), rot=(0, 0, 0)):
        k.box((1.2, 4.6, 0.15), (0, 0, 0), "concrete_d", rot=(14, 0, 0))
    k.box((1.0, 0.1, 0.8), (-3.4, -3.7, 6.5), "red")
    return k


@model
def market_hall():
    """Covered market (footprint 4): a brick hall with arched openings, a gable roof with a glazed lantern and stalls out front."""
    k = Kit("market_hall")
    k.box((6.2, 5.6, 2.4), (0, 0.4, 1.2), "brick", faces={"top": "steel_d"})
    k.box((6.3, 5.7, 0.3), (0, 0.4, 0.15), "stone_d")
    gable(k, 0, 0.4, 6.2, 5.6, 2.4, 1.6, "corr_green", "brick", over=0.25, ridge="y")
    k.box((1.4, 4.4, 0.6), (0, 0.4, 4.3), "glass", faces={"top": "corr_green"})  # lantern
    k.box((1.5, 4.5, 0.1), (0, 0.4, 4.65), "corr_green")
    for x in (-1.9, -0.65, 0.65, 1.9):
        k.box((0.8, 0.06, 1.7), (x, 0.4 - 2.83, 1.15), "black")
        k.cyl(0.4, 0.06, (x, 0.4 - 2.83, 2.0), "black", axis="y", seg=10)
    for sx in (-1, 1):
        k.windows((sx * 3.11, 0.4, 1.3), (sx, 0, 0), 4.6, 1.6, 1, 4, win=(0.5, 0.9), sill=0.4, frame="stone")
    for i in range(0 if LOW else 4):  # stalls
        k.box((1.1, 0.8, 0.5), (-2.6 + i * 1.7, -3.3, 0.3), "wood")
        k.box((1.3, 1.0, 0.06), (-2.6 + i * 1.7, -3.3, 1.5), ["red", "white", "shutter_g", "yellow"][i])
        for sx in (-0.5, 0.5):
            k.box((0.05, 0.05, 1.2), (-2.6 + i * 1.7 + sx, -3.7, 0.7), "wood")
    return k


@model
def fire_station():
    """Fire station (footprint 4): a brick building with three red engine doors, a hose tower and a flat roof."""
    k = Kit("fire_station")
    k.box((6.6, 4.4, 2.8), (0, 0.4, 1.4), "brick", faces={"top": "roof_tar"})
    k.box((6.7, 4.5, 0.14), (0, 0.4, 2.85), "concrete")
    for x in (-2.0, 0.0, 2.0):
        k.box((1.5, 0.08, 1.8), (x, 0.4 - 2.23, 1.0), "red")
        for i in range(0 if LOW else 5):
            k.box((1.4, 0.03, 0.04), (x, 0.4 - 2.28, 0.4 + i * 0.35), "white")
        k.box((1.7, 0.06, 0.12), (x, 0.4 - 2.25, 1.95), "stone")
    k.box((1.5, 1.5, 5.6), (-2.9, 2.1, 2.8), "brick", faces={"top": "concrete"})  # hose tower
    k.box((1.6, 1.6, 0.14), (-2.9, 2.1, 5.65), "concrete")
    k.box((0.9, 0.06, 1.2), (-2.9, 2.1 - 0.77, 4.8), "black")
    k.windows((0, 0.4 - 2.2, 2.5), (0, -1, 0), 6.0, 0.9, 1, 4, win=(0.5, 0.4), sill=0.15, frame="white") if False else None
    k.box((6.0, 0.06, 0.4), (0, 0.4 - 2.25, 2.55), "white")
    for sx in (-1, 1):
        side_windows(k, sx * 3.3, sx, 3.6, 0.5, 1, 2.0, 3, win=(0.5, 0.7), trim="white", yc=0.4)
    k.box((0.7, 0.7, 0.4), (2.0, 0.4, 3.2), "steel_d")
    k.cyl(0.04, 1.6, (-2.9, 2.1, 5.7), "steel", axis="z", seg=6)
    return k


@model
def police_station():
    """Police station (footprint 2): a boxy grey-blue two-storey building with a blue sign, a canopy, a mast and a flag."""
    k = Kit("police_station")
    w, l, sh = 3.5, 3.2, 1.2
    h = 2 * sh + 0.2
    k.box((w, l, h), (0, 0, h / 2), "plaster_sky", faces={"top": "roof_tar"})
    k.box((w + 0.14, l + 0.14, 0.12), (0, 0, h + 0.03), "concrete")
    front_windows(k, 0, -l / 2, w - 0.6, 0.3, 2, sh, 3, win=(0.4, 0.5), trim="white", skip=((0, 1),), seed=22)
    k.box((1.8, 0.06, 0.3), (0, -l / 2 - 0.04, 1.45), "trim_blue")
    k.door((0, -l / 2, 0.25), (0, -1, 0), 0.65, 1.0, "metal_door", frame="white")
    k.box((1.5, 0.6, 0.08), (0, -l / 2 - 0.3, 1.25), "concrete")
    for sx in (-1, 1):
        side_windows(k, sx * w / 2, sx, l - 0.8, 0.3, 2, sh, 2, win=(0.36, 0.5), trim="white")
    k.box((0.04, 0.04, 1.6), (-1.2, 0.3, h + 0.8), "steel")
    k.box((0.04, 0.04, 2.2), (1.3, -0.5, h + 1.1), "steel")
    k.box((0.5, 0.02, 0.32), (1.55, -0.5, h + 2.0), "trim_blue")
    k.box((0.7, 0.4, 0.3), (1.0, 0.8, h + 0.3), "steel_d")
    return k


@model
def gas_station():
    """Filling station (footprint 2): a canopy on four posts over two pumps, a small shop and a price sign."""
    k = Kit("gas_station")
    k.box((3.2, 1.9, 0.14), (0, -0.5, 2.0), "white")
    k.box((3.24, 1.94, 0.1), (0, -0.5, 2.1), "red")
    for sx in (-1.4, 1.4):
        for sy in (-1.3, 0.3):
            k.box((0.12, 0.12, 2.0), (sx, sy, 1.0), "steel_d")
    for x in (-0.7, 0.7):
        k.box((0.34, 0.24, 0.9), (x, -0.5, 0.5), "red")
        k.box((0.3, 0.02, 0.2), (x, -0.5 - 0.13, 0.75), "glass_lit")
        k.box((0.5, 0.45, 0.06), (x, -0.5, 0.08), "concrete")
    k.box((3.0, 1.1, 1.3), (0, 1.4, 0.65), "plaster_w", faces={"top": "roof_tar"})
    k.box((3.1, 1.2, 0.1), (0, 1.4, 1.35), "red")
    k.box((2.2, 0.05, 0.7), (0, 1.4 - 0.58, 0.7), "glass")
    k.door((1.2, 1.4 - 0.55, 0.1), (0, -1, 0), 0.5, 0.9, "metal_door", frame="white")
    k.box((0.12, 0.12, 3.0), (-1.75, -1.6, 1.5), "steel_d")
    k.box((0.7, 0.1, 1.0), (-1.75, -1.6, 3.0), "red")
    k.box((0.55, 0.12, 0.5), (-1.75, -1.6, 2.95), "yellow")
    return k
