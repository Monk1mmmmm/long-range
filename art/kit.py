"""Blender modelling kit for the game's own models (art/build.py). Runs with Blender's Python (`bpy`, e.g. the
`bpy` package from PyPI in the project venv: `.venv/bin/pip install bpy`).

A model is built from simple pieces (boxes, cylinders, extruded profiles, wedges) placed in world units
(Blender: X right, -Y front, Z up; 1 unit = 1 game world unit). Pieces belong to parts: every part becomes one
object (one glTF node, one entity in the game), named as the unit JSON refers to it (Turret, Turret_Barrel...).
Surfaces get materials made of seamless tile textures (painted here with numpy, in the look of the Long Range
Tank mod's units and the Mining Town buildings) projected onto the faces; `finish()` unwraps every part into one
UV atlas and bakes the albedo and the team mask (white = owner's colour) with Cycles, then gives all parts one
textured material, which is what tools/blend_export.py exports."""
import math

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

import os

LOW = bool(os.environ.get("LR_LOW"))  # low-poly build: flat windows and doors, fewer segments, no small extras (models_themed / models_urban)
TEX = 512  # atlas size; the game copies the albedo once per unit type and player colour

# ---------------------------------------------------------------- tiles (seamless, numpy)


def _hash(ix, iy, seed):
    h = (ix * 374761393 + iy * 668265263 + seed * 1442695041) & 0xFFFFFFFF
    h = ((h ^ (h >> 13)) * 1274126177) & 0xFFFFFFFF
    return ((h ^ (h >> 16)) & 0xFFFF) / 65535.0


def pnoise(n, cells, seed=0):
    """Periodic value noise on an n x n tile with `cells` cells per side."""
    t = (np.arange(n) + 0.5) / n * cells
    x, y = np.meshgrid(t, t)
    ix, iy = np.floor(x).astype(np.int64), np.floor(y).astype(np.int64)
    fx, fy = x - ix, y - iy
    fx, fy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    a, b = _hash(ix % cells, iy % cells, seed), _hash((ix + 1) % cells, iy % cells, seed)
    c, d = _hash(ix % cells, (iy + 1) % cells, seed), _hash((ix + 1) % cells, (iy + 1) % cells, seed)
    return a + (b - a) * fx + (c - a) * fy + (a - b - c + d) * fx * fy


def fbm(n, cells, seed=0, octaves=4):
    s, amp, tot = 0.0, 1.0, 0.0
    for o in range(octaves):
        s = s + pnoise(n, cells * 2 ** o, seed + o * 31) * amp
        tot += amp
        amp *= 0.5
    return s / tot


def rgb(*c):
    return np.array(c, float) / 255.0


class Tile:
    """A seamless square of `size` world units: colour (n x n x 3, 0..1) and team mask (n x n)."""

    def __init__(self, size, n=256):
        self.size, self.n = size, n
        t = (np.arange(n) + 0.5) / n * size
        self.x, self.y = np.meshgrid(t, t[::-1])  # y up, row 0 at the top
        self.c = np.zeros((n, n, 3))
        self.team = np.zeros((n, n))

    def fill(self, col, m=None):
        if m is None:
            self.c[:] = col
        else:
            self.c[m] = col

    def mul(self, k, m=None):
        if m is None:
            self.c *= k[..., None] if np.ndim(k) == 2 else k
        else:
            self.c[m] *= k

    def grime(self, amount, cells, seed=1):
        g = fbm(self.n, cells, seed)
        self.c *= (1 - amount / 2 + g * amount)[..., None]

    def lines(self, step_x, step_y, k=0.6, w=None, ox=0.0, oy=0.0):
        """Darker lines every step (panel seams)."""
        w = w if w is not None else max(1.4 * self.size / self.n, 0.012)
        if step_x:
            self.mul(k, np.abs(((self.x + ox) % step_x) - step_x / 2) > step_x / 2 - w / 2)
        if step_y:
            self.mul(k, np.abs(((self.y + oy) % step_y) - step_y / 2) > step_y / 2 - w / 2)

    def dots(self, step_x, step_y, r, k=0.7, ox=0.0, oy=0.0, light=1.25):
        dx = ((self.x + ox) % step_x) - step_x / 2
        dy = ((self.y + oy) % step_y) - step_y / 2
        m = dx * dx + dy * dy < r * r
        self.mul(k, m)
        self.mul(light, m & (dy > r * 0.2) & (dx < 0))


STEEL = rgb(150, 154, 157)
PALETTE = {
    "steel": STEEL, "steel_d": rgb(86, 90, 94), "gun": rgb(58, 60, 62), "black": rgb(26, 27, 28),
    "rubber": rgb(30, 30, 31), "glass": rgb(48, 64, 80), "white": rgb(236, 236, 230), "red": rgb(200, 36, 28),
    "yellow": rgb(222, 178, 30), "concrete": rgb(170, 168, 160), "asphalt": rgb(70, 71, 72),
    "plaster": rgb(222, 213, 188), "plaster_w": rgb(234, 232, 224), "plaster_y": rgb(226, 194, 136),
    "plaster_g": rgb(176, 190, 168), "brick": rgb(146, 70, 54), "roof_tile": rgb(168, 52, 42), "roof_slate": rgb(74, 78, 86),
    "roof_tar": rgb(66, 66, 68), "wood": rgb(120, 86, 58), "barn": rgb(148, 46, 38), "era": rgb(132, 130, 98),
    "copper": rgb(160, 104, 60), "tank": rgb(206, 208, 202), "olive": rgb(98, 104, 76),
}


def tile_steel(base, size=1.8, panels=(0.9, 0.6), team=0.0, seed=3):
    t = Tile(size)
    t.fill(base)
    t.grime(0.16, 4, seed)
    t.grime(0.06, 32, seed + 5)
    t.lines(panels[0], panels[1], 0.66)
    t.dots(panels[0] / 4, panels[1], 0.012, 0.7, oy=panels[1] / 2 - 0.04)
    t.team[:] = team
    if team:
        w = 0.035
        edge = (np.abs((t.x % panels[0]) - panels[0] / 2) > panels[0] / 2 - w) | (np.abs((t.y % panels[1]) - panels[1] / 2) > panels[1] / 2 - w)
        t.team[edge] = 0.15
    return t


def tile_plain(col, size=1.0, grime=0.12, seed=2):
    t = Tile(size, 128)
    t.fill(col)
    t.grime(grime, 4, seed)
    return t


def tile_corrugated(col, size=2.0, rib=0.16, team=0.0, seed=4):
    t = Tile(size)
    t.fill(col)
    r = np.cos(t.x / rib * 2 * math.pi) * 0.5 + 0.5
    t.mul(0.82 + 0.26 * r)
    t.lines(1.0, 0, 0.8)
    s = pnoise(t.n, 16, seed)[:, :1]  # vertical streaks
    t.mul(1 - 0.18 * np.clip(pnoise(t.n, 24, seed + 2) * 0 + fbm(t.n, 3, seed + 9) * 0.3 + np.repeat(s, t.n, 1) * 0.7 - 0.3, 0, 1))
    t.grime(0.12, 3, seed)
    t.team[:] = team
    return t


def tile_concrete(col, size=4.0, joints=2.0, seed=5):
    t = Tile(size)
    t.fill(col)
    t.grime(0.2, 4, seed)
    t.grime(0.1, 40, seed + 1)
    t.lines(joints, joints, 0.8)
    return t


def tile_plaster(col, size=2.0, seed=6):
    t = Tile(size)
    t.fill(col)
    t.grime(0.1, 3, seed)
    t.grime(0.05, 48, seed + 1)
    return t


def tile_panel_block(col, size=3.2, seed=7):
    """Soviet prefab panels: 1.6 x 1.45 slabs with dark joints."""
    t = Tile(size)
    t.fill(col)
    t.grime(0.12, 4, seed)
    t.grime(0.06, 40, seed + 1)
    t.lines(1.6, 0, 0.7, w=0.03)
    return t


def tile_brick(col, size=1.6, seed=8):
    t = Tile(size)
    t.fill(col)
    bh, bw = 0.1, 0.25
    row = np.floor(t.y / bh)
    off = (row % 2) * bw / 2
    # per brick tint
    bx = np.floor((t.x + off) / bw).astype(np.int64) % int(round(size / bw))
    tint = _hash(bx, row.astype(np.int64) % int(round(size / bh)), seed)
    t.mul(0.85 + 0.25 * tint)
    t.mul(0.62, ((t.y % bh) < 0.014) | (((t.x + off) % bw) < 0.014))
    t.grime(0.12, 4, seed)
    return t


def tile_roof(col, size=1.2, seed=9):
    t = Tile(size)
    t.fill(col)
    rowh, w = 0.15, 0.2
    row = np.floor(t.y / rowh)
    off = (row % 2) * w / 2
    tint = _hash(np.floor((t.x + off) / w).astype(np.int64) % int(round(size / w)), row.astype(np.int64) % int(round(size / rowh)), seed)
    t.mul(0.85 + 0.22 * tint)
    t.mul(0.62, (t.y % rowh) < 0.035)
    t.mul(0.8, ((t.x + off) % w) < 0.015)
    t.grime(0.14, 3, seed)
    return t


def tile_tar(col, size=3.0, seed=10):
    t = Tile(size)
    t.fill(col)
    t.grime(0.3, 48, seed)
    t.grime(0.14, 3, seed + 1)
    return t


def tile_wood(col, size=1.6, seed=11):
    t = Tile(size)
    t.fill(col)
    t.mul(0.7, (t.x % 0.2) < 0.016)
    s = pnoise(t.n, 64, seed)[:, :1]
    t.mul(0.88 + 0.2 * np.repeat(s, t.n, 1).T)
    t.grime(0.18, 4, seed)
    return t


def tile_track(size=0.88, seed=12):
    t = Tile(size, 128)
    t.fill(PALETTE["rubber"] * 1.5)
    t.mul(0.55, (t.x % 0.11) < 0.035)
    t.mul(1.3, (t.x % 0.11) > 0.09)
    t.grime(0.25, 8, seed)
    return t


def tile_era(size=0.96, brick=(0.24, 0.16), seed=13):
    """Reactive armour: bolted bricks."""
    t = Tile(size, 256)
    t.fill(PALETTE["era"])
    t.grime(0.14, 6, seed)
    bw, bh = brick
    gx, gy = t.x % bw, t.y % bh
    tint = _hash(np.floor(t.x / bw).astype(np.int64) % int(round(size / bw)), np.floor(t.y / bh).astype(np.int64) % int(round(size / bh)), seed)
    t.mul(0.9 + 0.18 * tint)
    t.mul(1.12, gy > bh * 0.75)  # lit top lip
    t.mul(0.4, (gx < 0.014) | (gy < 0.014))
    t.dots(bw, bh, 0.012, 0.5, ox=bw / 2 - 0.05 if False else 0.0)
    t.dots(bw / 2, bh, 0.01, 0.55, ox=bw / 4, oy=0.0)
    return t


def tile_sandbag(size=0.8, seed=30):
    """Burlap bags laid in courses: tint per bag, dark seams, lighter tops."""
    t = Tile(size, 128)
    t.fill(rgb(160, 140, 98))
    bw, bh = 0.4, 0.16
    row = np.floor(t.y / bh)
    off = (row % 2) * bw / 2
    tint = _hash(np.floor((t.x + off) / bw).astype(np.int64) % int(round(size / bw)), row.astype(np.int64) % int(round(size / bh)), seed)
    t.mul(0.84 + 0.24 * tint)
    gy = t.y % bh
    t.mul(0.6, (gy < 0.02) | (((t.x + off) % bw) < 0.025))
    t.mul(1.12, gy > bh * 0.7)
    t.grime(0.18, 16, seed)
    return t


def tile_dirt(col, size=2.0, seed=31):
    t = Tile(size)
    t.fill(col)
    t.grime(0.35, 6, seed)
    t.grime(0.2, 48, seed + 1)
    return t


def tile_hazard(size=0.6):
    t = Tile(size, 128)
    m = np.floor((t.x + t.y) / (size / 3)) % 2 == 0
    t.fill(PALETTE["yellow"])
    t.fill(PALETTE["black"], m)
    t.grime(0.15, 4)
    return t


def tile_vent(size=0.48):
    t = Tile(size, 128)
    t.fill(PALETTE["steel_d"])
    t.mul(0.4, (t.y % 0.06) < 0.025)
    t.grime(0.12, 4)
    return t


def tile_glass(col=None, size=1.0):
    t = Tile(size, 64)
    t.fill(col if col is not None else PALETTE["glass"])
    t.mul(1.0 + 0.25 * (t.y > size * 0.5))
    return t


def tile_ashlar(col, size=2.0, seed=60):
    """Dressed limestone blocks in courses, each its own tone."""
    t = Tile(size)
    t.fill(col)
    bh, bw = 0.25, 0.5
    row = np.floor(t.y / bh).astype(np.int64)
    off = (row % 2) * bw / 2
    bx = np.floor((t.x + off) / bw).astype(np.int64) % int(round(size / bw))
    tint = _hash(bx, row % int(round(size / bh)), seed)
    t.mul(0.88 + 0.2 * tint)
    t.mul(0.7, ((t.y % bh) < 0.014) | (((t.x + off) % bw) < 0.014))
    t.grime(0.16, 4, seed)
    t.grime(0.06, 40, seed + 1)
    return t


def tile_halftimber(col, beam=None, size=2.0, seed=61):
    """Plaster panels between dark timber beams (posts every 0.66, rails every 0.5)."""
    t = Tile(size)
    t.fill(col)
    t.grime(0.1, 3, seed)
    beam = beam if beam is not None else rgb(72, 50, 36)
    w = 0.07
    post = np.abs(((t.x % 0.66) - 0.33)) > 0.33 - w / 2
    rail = np.abs(((t.y % 0.5) - 0.25)) > 0.25 - w / 2
    m = post | rail
    t.c[m] = (beam * (0.85 + 0.3 * fbm(t.n, 24, seed + 2)[..., None]))[m]
    return t


def tile_log(col, size=1.2, seed=62):
    """Horizontal logs with dark joints."""
    t = Tile(size)
    t.fill(col)
    rowh = 0.16
    row = np.floor(t.y / rowh).astype(np.int64)
    t.mul(0.8 + 0.3 * _hash(row % int(round(size / rowh)), 0, seed))
    f = (t.y % rowh) / rowh
    t.mul(0.55, (f < 0.1))
    t.mul(0.88 + 0.3 * np.abs(f - 0.5))
    t.grime(0.2, 4, seed)
    return t


TILES = {}


def tiles():
    if TILES:
        return TILES
    P = PALETTE
    TILES.update({
        "steel": tile_steel(P["steel"]), "steel_d": tile_steel(P["steel_d"], seed=4), "tank": tile_steel(P["tank"], seed=9),
        "team": tile_steel(rgb(150, 150, 150), team=1.0, seed=5), "team_plain": tile_plain(rgb(150, 150, 150), 1.0, 0.1),
        "olive": tile_steel(P["olive"], seed=6),
        "gun": tile_plain(P["gun"], 0.5, 0.2), "black": tile_plain(P["black"], 0.5), "rubber": tile_plain(P["rubber"], 0.5, 0.2),
        "white": tile_plain(P["white"], 1.0, 0.08), "red": tile_plain(P["red"], 1.0, 0.08), "yellow": tile_plain(P["yellow"], 1.0),
        "glass": tile_glass(), "glass_lit": tile_glass(rgb(214, 196, 130)), "canopy": tile_glass(rgb(72, 98, 120)),
        "concrete": tile_concrete(P["concrete"]), "concrete_d": tile_concrete(P["concrete"] * 0.75, seed=15),
        "asphalt": tile_tar(P["asphalt"], seed=16),
        "corr": tile_corrugated(rgb(178, 180, 178)), "corr_d": tile_corrugated(rgb(118, 122, 124), seed=17),
        "corr_team": tile_corrugated(rgb(150, 150, 150), team=1.0, seed=18), "corr_green": tile_corrugated(rgb(96, 122, 100), seed=19),
        "corr_white": tile_corrugated(rgb(212, 212, 206), seed=20),
        "plaster": tile_plaster(P["plaster"]), "plaster_w": tile_plaster(P["plaster_w"], seed=21), "plaster_y": tile_plaster(P["plaster_y"], seed=22),
        "plaster_g": tile_plaster(P["plaster_g"], seed=23),
        "panel_block": tile_panel_block(rgb(196, 194, 184)), "brick": tile_brick(P["brick"]), "brick_y": tile_brick(rgb(190, 160, 110), seed=24),
        "roof_tile": tile_roof(P["roof_tile"]), "roof_slate": tile_roof(P["roof_slate"], seed=25), "roof_tar": tile_tar(P["roof_tar"]),
        "wood": tile_wood(P["wood"]), "barn": tile_wood(P["barn"], seed=26), "track": tile_track(), "era": tile_era(),
        "hazard": tile_hazard(), "vent": tile_vent(), "copper": tile_plain(P["copper"], 1.0), "door": tile_wood(rgb(96, 64, 44), 1.0, seed=27),
        "sandbag": tile_sandbag(), "dirt": tile_dirt(rgb(104, 84, 60)), "mud": tile_dirt(rgb(70, 56, 42), seed=32),
        "timber": tile_wood(rgb(110, 84, 58), 1.2, seed=33), "thatch": tile_wood(rgb(176, 146, 84), 0.8, seed=40), "plaster_b": tile_plaster(rgb(120, 160, 190), seed=41), "concrete_old": tile_concrete(rgb(150, 150, 140), 3.0, 1.5, seed=34),
        "red_white": tile_plain(rgb(200, 50, 40), 1.0, 0.08), "rust": tile_dirt(rgb(120, 70, 44), 1.5, seed=35),
        "metal_door": tile_steel(rgb(110, 116, 120), 1.0, (1.0, 0.5), seed=28),
        "stone": tile_ashlar(rgb(214, 202, 178)), "stone_d": tile_ashlar(rgb(160, 150, 132), seed=63),
        "terracotta": tile_roof(rgb(190, 90, 56), seed=64), "roof_brown": tile_roof(rgb(112, 74, 54), seed=65), "roof_green": tile_roof(rgb(66, 104, 78), seed=66),
        "plaster_ochre": tile_plaster(rgb(228, 190, 118), seed=67), "plaster_pink": tile_plaster(rgb(226, 178, 152), seed=68),
        "plaster_sky": tile_plaster(rgb(172, 192, 208), seed=69), "plaster_cream": tile_plaster(rgb(238, 226, 196), seed=70),
        "halftimber": tile_halftimber(rgb(236, 226, 200)), "halftimber_y": tile_halftimber(rgb(232, 206, 150), seed=71),
        "log": tile_log(rgb(150, 108, 66)), "log_d": tile_log(rgb(112, 80, 52), seed=72),
        "shutter_g": tile_plain(rgb(62, 108, 80), 1.0, 0.12), "shutter_b": tile_plain(rgb(52, 84, 140), 1.0, 0.12),
        "shutter_r": tile_plain(rgb(130, 44, 38), 1.0, 0.12), "trim_blue": tile_plain(rgb(70, 130, 190), 1.0, 0.1),
        "panel_grey": tile_panel_block(rgb(176, 176, 168), seed=73), "panel_beige": tile_panel_block(rgb(204, 192, 164), seed=74),
        "concrete_l": tile_concrete(rgb(196, 194, 186), 3.0, 1.5, seed=75),
    })
    return TILES


# ---------------------------------------------------------------- materials

_MATS = {}


def _image(name, arr, alpha=None):
    n = arr.shape[0]
    img = bpy.data.images.get(name) or bpy.data.images.new(name, n, n, alpha=False)
    px = np.ones((n, n, 4), np.float32)
    px[..., :3] = arr[::-1]  # Blender rows start at the bottom
    img.pixels.foreach_set(px.ravel())
    img.pack()
    return img


def material(name):
    """A material showing tile `name` box-projected in object space. Keeps both images (colour, team) so
    `finish` can bake either."""
    if name in _MATS:
        return _MATS[name]
    t = tiles()[name]
    m = bpy.data.materials.new("m_" + name)
    m.use_nodes = True
    nt = m.node_tree
    nt.nodes.clear()
    out = nt.nodes.new("ShaderNodeOutputMaterial")
    bsdf = nt.nodes.new("ShaderNodeBsdfPrincipled")
    tc = nt.nodes.new("ShaderNodeTexCoord")
    mp = nt.nodes.new("ShaderNodeMapping")
    mp.inputs["Scale"].default_value = (1 / t.size, 1 / t.size, 1 / t.size)
    nt.links.new(tc.outputs["Object"], mp.inputs["Vector"])
    col = nt.nodes.new("ShaderNodeTexImage")
    col.name = "tile_color"
    col.image = _image("tile_" + name, t.c)
    col.projection = "BOX"
    col.projection_blend = 0.15
    team = nt.nodes.new("ShaderNodeTexImage")
    team.name = "tile_mask"
    team.image = _image("mask_" + name, np.repeat(t.team[..., None], 3, 2))
    team.image.colorspace_settings.name = "Non-Color"
    team.projection = "BOX"
    team.projection_blend = 0.15
    for n in (col, team):
        nt.links.new(mp.outputs["Vector"], n.inputs["Vector"])
    nt.links.new(col.outputs["Color"], bsdf.inputs["Base Color"])
    nt.links.new(bsdf.outputs["BSDF"], out.inputs["Surface"])
    _MATS[name] = m
    return m


# ---------------------------------------------------------------- pieces and parts

class Part:
    """A named moving piece of the model. `pivot` (world) is its origin, `parent` another Part."""

    def __init__(self, kit, name, pivot=(0, 0, 0), parent=None):
        self.kit, self.name, self.pivot, self.parent = kit, name, Vector(pivot), parent
        self.pieces = []
        self.obj = None
        self.empty = False


class Kit:
    def __init__(self, name):
        self.name = name + "_low" if LOW else name
        bpy.ops.wm.read_factory_settings(use_empty=True)
        _MATS.clear()
        TILES_USED.clear()
        self.parts = []
        self.root = self.part(name.capitalize())
        self.cur = self.root
        self.xf = [Matrix.Identity(4)]
        self.anims = []  # (action name, {part: [(frame, loc or None, rot_euler or None)]})
        self.nohit = set()  # parts left out of the hit box
        self.hit = True  # pieces added while False are left out of the hit box too (aprons, rotors)
        self.nohit_pieces = set()
        self.hit_box = None

    def part(self, name, pivot=(0, 0, 0), parent=None):
        p = Part(self, name, pivot, parent if parent is not None else (self.root if self.parts else None))
        self.parts.append(p)
        return p

    def into(self, part):
        """Context: pieces added inside go to `part`."""
        kit = self

        class C:
            def __enter__(s):
                s.prev = kit.cur
                kit.cur = part
                return part

            def __exit__(s, *a):
                kit.cur = s.prev
        return C()

    def nohit_(self):
        """Context: pieces added inside do not count for the hit box."""
        kit = self

        class C:
            def __enter__(s):
                s.prev = kit.hit
                kit.hit = False

            def __exit__(s, *a):
                kit.hit = s.prev
        return C()

    def at(self, loc=(0, 0, 0), rot=(0, 0, 0), scale=1.0):
        """Context: pieces added inside are moved/turned (degrees, XYZ euler) relative to the current frame."""
        kit = self

        class C:
            def __enter__(s):
                from mathutils import Euler
                m = Matrix.Translation(Vector(loc)) @ Euler([math.radians(a) for a in rot], "XYZ").to_matrix().to_4x4() @ Matrix.Scale(scale, 4)
                kit.xf.append(kit.xf[-1] @ m)

            def __exit__(s, *a):
                kit.xf.pop()
        return C()

    # -- making pieces
    def _add(self, bm, mats, smooth=False, part=None):
        me = bpy.data.meshes.new("piece")
        bm.transform(self.xf[-1])
        bm.to_mesh(me)
        bm.free()
        for m in mats:
            me.materials.append(material(m))
            TILES_USED.add(m)
        for p in me.polygons:
            p.use_smooth = smooth
        ob = bpy.data.objects.new("piece", me)
        bpy.context.scene.collection.objects.link(ob)
        if not self.hit:
            self.nohit_pieces.add(ob.name)
        (part or self.cur).pieces.append(ob)
        return ob

    def box(self, size, loc=(0, 0, 0), mat="steel", faces=None, bevel=0.0, rot=(0, 0, 0), taper=None):
        """Box of `size` (x, y, z) centred at loc. `faces`: {'top'|'bottom'|'front'(-Y)|'back'|'left'(-X)|'right':
        material}. `taper` (tx, ty): scale of the top face (wedges, sloped hulls). `bevel`: chamfer width."""
        bm = bmesh.new()
        bmesh.ops.create_cube(bm, size=1.0)
        if taper:
            for v in bm.verts:
                if v.co.z > 0:
                    v.co.x *= taper[0]
                    v.co.y *= taper[1]
        bmesh.ops.scale(bm, vec=Vector(size), verts=bm.verts)
        mats = [mat]
        faces = faces or {}
        names = {"top": (0, 0, 1), "bottom": (0, 0, -1), "front": (0, -1, 0), "back": (0, 1, 0), "left": (-1, 0, 0), "right": (1, 0, 0)}
        bm.normal_update()
        for f in bm.faces:
            for k, n in names.items():
                if k in faces and f.normal.dot(Vector(n)) > 0.7:
                    m = faces[k]
                    if m not in mats:
                        mats.append(m)
                    f.material_index = mats.index(m)
        if bevel > 0:
            bmesh.ops.bevel(bm, geom=list(bm.edges), offset=bevel, segments=1, profile=0.5, affect="EDGES", clamp_overlap=True)
        from mathutils import Euler
        bm.transform(Matrix.Translation(Vector(loc)) @ Euler([math.radians(a) for a in rot], "XYZ").to_matrix().to_4x4())
        return self._add(bm, mats)

    def prism(self, prof, axis, a0, a1, mat="steel", caps=None, side_mats=None, bevel=0.0, smooth=False, scale1=1.0):
        """Extrudes a 2D profile along `axis` from a0 to a1. Profile coordinates: axis 'x' -> (y, z) (a side
        profile, front is -y), 'y' -> (x, z) (a cross section), 'z' -> (x, y) (a plan)."""
        bm = bmesh.new()
        ax = "xyz".index(axis)

        def lift(p, a):
            if ax == 0:
                return Vector((a, p[0], p[1]))
            if ax == 1:
                return Vector((p[0], a, p[1]))
            return Vector((p[0], p[1], a))
        prof = [tuple(p) for p in prof]
        cx = sum(p[0] for p in prof) / len(prof)
        cy = sum(p[1] for p in prof) / len(prof)
        A = [bm.verts.new(lift(p, a0)) for p in prof]
        B = [bm.verts.new(lift(((p[0] - cx) * scale1 + cx, (p[1] - cy) * scale1 + cy), a1)) for p in prof]
        mats = [mat]

        def mi(m):
            if m not in mats:
                mats.append(m)
            return mats.index(m)
        k = len(prof)
        for i in range(k):
            j = (i + 1) % k
            f = bm.faces.new((A[i], A[j], B[j], B[i]))
            if side_mats and side_mats[i]:
                f.material_index = mi(side_mats[i])
        caps = caps or {}
        fa = bm.faces.new(A[::-1])
        fb = bm.faces.new(B)
        fa.material_index = mi(caps.get(0, mat))
        fb.material_index = mi(caps.get(1, mat))
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        # keep n-gon caps but make sure they are planar-valid
        if bevel > 0:
            bmesh.ops.bevel(bm, geom=list(bm.edges), offset=bevel, segments=1, affect="EDGES", clamp_overlap=True)
        ob = self._add(bm, mats, smooth=smooth)
        return ob

    def cyl(self, r, depth, loc=(0, 0, 0), mat="steel", axis="z", seg=12, r2=None, caps=None, smooth=True):
        """Cylinder (or frustum, top radius r2) from loc along +axis by depth."""
        bm = bmesh.new()
        r2 = r if r2 is None else r2
        if LOW:
            seg = min(seg, 6)
        bmesh.ops.create_cone(bm, cap_ends=True, cap_tris=False, segments=seg, radius1=r, radius2=r2, depth=depth)
        bmesh.ops.translate(bm, vec=Vector((0, 0, depth / 2)), verts=bm.verts)
        mats = [mat]
        caps = caps or {}
        bm.normal_update()
        for f in bm.faces:
            if abs(f.normal.z) > 0.99:
                m = caps.get(1 if f.normal.z > 0 else 0)
                if m:
                    if m not in mats:
                        mats.append(m)
                    f.material_index = mats.index(m)
        rot = {"z": Matrix.Identity(4), "x": Matrix.Rotation(math.pi / 2, 4, "Y"), "y": Matrix.Rotation(-math.pi / 2, 4, "X"),
               "-z": Matrix.Rotation(math.pi, 4, "X"), "-x": Matrix.Rotation(-math.pi / 2, 4, "Y"), "-y": Matrix.Rotation(math.pi / 2, 4, "X")}[axis]
        bm.transform(Matrix.Translation(Vector(loc)) @ rot)
        ob = self._add(bm, mats, smooth=False)
        if smooth:
            for p in ob.data.polygons:
                p.use_smooth = abs(p.normal.dot(rot.to_3x3() @ Vector((0, 0, 1)))) < 0.99
        return ob

    def sphere(self, r, loc=(0, 0, 0), mat="steel", seg=12, rings=6, scale=(1, 1, 1), half=False):
        bm = bmesh.new()
        if LOW:
            seg, rings = min(seg, 8), min(rings, 4)
        bmesh.ops.create_uvsphere(bm, u_segments=seg, v_segments=rings, radius=r)
        if half:
            bmesh.ops.delete(bm, geom=[v for v in bm.verts if v.co.z < -1e-4], context="VERTS")
            bm.faces.new([v for v in sorted((v for v in bm.verts if abs(v.co.z) < 1e-4), key=lambda v: math.atan2(v.co.y, v.co.x))][::-1]) if False else None
        bmesh.ops.scale(bm, vec=Vector(scale), verts=bm.verts)
        bm.transform(Matrix.Translation(Vector(loc)))
        return self._add(bm, [mat], smooth=True)

    def poly(self, pts, mat="steel", thick=0.0):
        """A flat polygon (list of 3D points), optionally given thickness along its normal."""
        bm = bmesh.new()
        vs = [bm.verts.new(Vector(p)) for p in pts]
        f = bm.faces.new(vs)
        if thick:
            r = bmesh.ops.extrude_face_region(bm, geom=[f])
            n = f.normal.copy()
            bmesh.ops.translate(bm, vec=-n * thick, verts=[e for e in r["geom"] if isinstance(e, bmesh.types.BMVert)])
            bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        return self._add(bm, [mat])

    def windows(self, face_center, normal, width, height, rows, cols, win=(0.5, 0.7), mat="glass", frame="white", depth=0.06,
                lit=0.0, skip=(), sill=0.25, seed=1):
        """Window panes on a wall: a grid on the rectangle (width x height) centred at face_center facing
        `normal` (a world axis like (0,-1,0)). Each window is a framed pane set slightly into the wall."""
        n = Vector(normal).normalized()
        u = Vector((0, 0, 1)).cross(n).normalized() if abs(n.z) < 0.9 else Vector((1, 0, 0))
        v = n.cross(u)
        rng = np.random.default_rng(seed)
        cw, ch = width / cols, height / rows
        for r in range(rows):
            for c in range(cols):
                if (r, c) in skip:
                    continue
                cen = Vector(face_center) + u * (-width / 2 + cw * (c + 0.5)) + v * (-height / 2 + ch * r + sill + win[1] / 2)
                m = "glass_lit" if lit and rng.random() < lit else mat
                self._pane(cen, n, u, v, win[0], win[1], m, frame, depth)

    def _pane(self, cen, n, u, v, w, h, mat, frame, depth):
        f = 0.05
        rot = Matrix((u, v, n)).transposed().to_4x4()
        with self.at(tuple(cen)):
            self.xf[-1] = self.xf[-1] @ rot
            if LOW:
                self.box((w + 0.06, h + 0.06, 0.03), (0, 0, 0.012), mat)
                return
            self.box((w + 2 * f, h + 2 * f, 0.03), (0, 0, 0.005), frame)
            self.box((w, h, 0.02), (0, 0, 0.022), mat)
            self.box((w + 2 * f + 0.06, 0.05, 0.08), (0, -h / 2 - f - 0.02, 0.03), frame)  # sill
            if w > 0.35:
                self.box((0.035, h, 0.03), (0, 0, 0.03), frame)

    def door(self, cen, normal, w, h, mat="door", frame="white"):
        n = Vector(normal).normalized()
        u = Vector((0, 0, 1)).cross(n).normalized()
        v = n.cross(u)
        rot = Matrix((u, v, n)).transposed().to_4x4()
        with self.at(tuple(Vector(cen) + v * h / 2)):
            self.xf[-1] = self.xf[-1] @ rot
            if LOW:
                self.box((w, h, 0.03), (0, 0, 0.015), mat)
                return
            self.box((w + 0.1, h + 0.05, 0.03), (0, 0.025, 0.005), frame)
            self.box((w, h, 0.025), (0, 0, 0.02), mat)

    # -- animation
    def key(self, action, part, frames):
        """frames: list of (frame, location or None (rest), euler degrees or None (rest))."""
        self.anims.append((action, part, frames))

    # -- finishing
    def finish(self, smart_angle=40.0):
        scene = bpy.context.scene
        # 0. hit box: every piece in the rest pose but aprons, rotors, barrels and muzzles (game axes x, height, length)
        lo, hi = Vector((1e9,) * 3), Vector((-1e9,) * 3)
        for p in self.parts:
            if p in self.nohit or any(w in p.name.lower() for w in ("barrel", "muzzle", "rig")):
                continue
            for o in p.pieces:
                if o.name in self.nohit_pieces:
                    continue
                for v in o.data.vertices:
                    lo = Vector(map(min, lo, v.co))
                    hi = Vector(map(max, hi, v.co))
        if hi.x > lo.x:
            self.hit_box = [round(max(hi.x, -lo.x) * 2, 2), round(hi.z, 2), round(max(hi.y, -lo.y) * 2, 2)]
        # 1. join each part's pieces into one object, origin at the pivot, parented
        for p in self.parts:
            if not p.pieces:
                me = bpy.data.meshes.new(p.name)
                ob = bpy.data.objects.new(p.name, None)
                scene.collection.objects.link(ob)
                ob.empty_display_size = 0.1
                p.empty = True
            else:
                bpy.ops.object.select_all(action="DESELECT")
                for o in p.pieces:
                    o.select_set(True)
                bpy.context.view_layer.objects.active = p.pieces[0]
                if len(p.pieces) > 1:
                    bpy.ops.object.join()
                ob = bpy.context.view_layer.objects.active
                ob.name = p.name
                ob.data.name = p.name
                ob.data.transform(Matrix.Translation(-p.pivot))
                bm = bmesh.new()
                bm.from_mesh(ob.data)
                bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)
                bm.to_mesh(ob.data)
                bm.free()
            ob.location = p.pivot
            p.obj = ob
        for p in self.parts:
            if p.parent is not None:
                p.obj.parent = p.parent.obj
                p.obj.matrix_parent_inverse = Matrix.Translation(-p.parent.pivot)
        bpy.context.view_layer.update()
        meshes = [p.obj for p in self.parts if not p.empty]
        # 2. one UV atlas over all parts
        bpy.ops.object.select_all(action="DESELECT")
        for o in meshes:
            o.select_set(True)
        bpy.context.view_layer.objects.active = meshes[0]
        bpy.ops.object.mode_set(mode="EDIT")
        bpy.ops.mesh.select_all(action="SELECT")
        bpy.ops.uv.smart_project(angle_limit=math.radians(smart_angle), island_margin=0.0, area_weight=0.0, scale_to_bounds=False)
        bpy.ops.uv.pack_islands(margin=3.0 / TEX, rotate=True, shape_method="AABB", margin_method="FRACTION")
        bpy.ops.object.mode_set(mode="OBJECT")
        # 3. bake albedo and team mask
        albedo = bpy.data.images.new(self.name + "_albedo", TEX, TEX, alpha=False)
        team = bpy.data.images.new(self.name + "_team", TEX, TEX, alpha=False)
        team.colorspace_settings.name = "Non-Color"
        scene.render.engine = "CYCLES"
        scene.cycles.device = "CPU"
        scene.cycles.samples = 4
        scene.render.bake.use_pass_direct = False
        scene.render.bake.use_pass_indirect = False
        scene.render.bake.use_pass_color = True
        scene.render.bake.margin = 6
        mats = {m for o in meshes for m in o.data.materials}
        for img, src in ((albedo, "tile_color"), (team, "tile_mask")):
            for m in mats:
                nt = m.node_tree
                bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
                nt.links.new(nt.nodes[src].outputs["Color"], bsdf.inputs["Base Color"])
                tgt = nt.nodes.get("bake_target") or nt.nodes.new("ShaderNodeTexImage")
                tgt.name = "bake_target"
                tgt.image = img
                nt.nodes.active = tgt
            bpy.ops.object.select_all(action="DESELECT")
            for o in meshes:
                o.select_set(True)
            bpy.ops.object.bake(type="DIFFUSE", pass_filter={"COLOR"}, use_clear=True, margin=6)
        # 4. one material for everything
        unit = bpy.data.materials.new("unit")
        unit.use_nodes = True
        nt = unit.node_tree
        bsdf = next(n for n in nt.nodes if n.type == "BSDF_PRINCIPLED")
        a = nt.nodes.new("ShaderNodeTexImage")
        a.image = albedo
        nt.links.new(a.outputs["Color"], bsdf.inputs["Base Color"])
        tnode = nt.nodes.new("ShaderNodeTexImage")
        tnode.name = tnode.label = "team"
        tnode.image = team
        for o in meshes:
            o.data.materials.clear()
            o.data.materials.append(unit)
            for poly in o.data.polygons:
                poly.material_index = 0
        # 5. animations
        self._actions()
        return albedo, team

    def _actions(self):
        acts = {}
        for name, part, frames in self.anims:
            act = acts.get(name) or bpy.data.actions.new(name)
            acts[name] = act
            ob = part.obj
            ad = ob.animation_data or ob.animation_data_create()
            ad.action = act
            rest_loc, rest_rot = ob.location.copy(), ob.rotation_euler.copy()
            for f, loc, rot in frames:
                ob.location = Vector(loc) if loc is not None else rest_loc
                ob.rotation_euler = [math.radians(a) for a in rot] if rot is not None else rest_rot
                ob.keyframe_insert("location", frame=f)
                ob.keyframe_insert("rotation_euler", frame=f)
            ob.location, ob.rotation_euler = rest_loc, rest_rot
            # stash on an NLA track so the object can take part in several actions; none stays active
            slot = ad.action_slot
            tr = ad.nla_tracks.new()
            tr.name = name
            strip = tr.strips.new(name, int(frames[0][0]), act)
            try:
                strip.action_slot = slot
            except Exception:
                pass
            ad.action = None


TILES_USED = set()


def model(fn):
    """Marks a function in art/models_*.py that returns a finished-to-be Kit."""
    fn.is_model = True
    return fn


def along(k, a, b, r, mat="gun", seg=8, r2=None, caps=None):
    """Cylinder from point a to point b (world, current frame)."""
    a, b = Vector(a), Vector(b)
    d = b - a
    q = Vector((0, 0, 1)).rotation_difference(d.normalized())
    with k.at(tuple(a)):
        k.xf[-1] = k.xf[-1] @ q.to_matrix().to_4x4()
        k.cyl(r, d.length, (0, 0, 0), mat, seg=seg, r2=r2, caps=caps)
