"""Variants of the Long Range Tank mod's models: the units that used to be another unit's model scaled up now
carry what sets them apart (reactive armour, flak guns, missile boxes, bigger radars, launchers)."""
import math

from kit import along, model
from variant import Variant
from models_vehicles import era_bricks


@model
def bastion():
    """T3 Bastion: the Crony with reactive armour bricks on the glacis, the hull sides and the turret."""
    k = Variant("bastion", "crony", 0.1925)
    k.on("UEX0105", "Bastion_ERA")
    for s in (-1, 1):
        era_bricks(k, (s * 1.0, 0.0, 0.47), "x+" if s > 0 else "x-", 2.2, 0.26, 7, 1, depth=0.06)
        era_bricks(k, (s * 0.62, -1.28, 0.5), "z+", 0.62, 0.36, 3, 2, depth=0.05, tilt=55)
    k.on("Turret", "Bastion_TurretERA")
    for s in (-1, 1):
        era_bricks(k, (s * 0.33, -0.6, 0.86), "y-", 0.36, 0.2, 2, 1, depth=0.06, tilt=-30)
        era_bricks(k, (s * 0.54, -0.15, 0.86), "x+" if s > 0 else "x-", 0.7, 0.18, 4, 1, depth=0.05)
    return k


@model
def flak_carrier():
    """T2 Hailstorm: the Dragoon with a turret-top fire-control radar, side ammo drums, armoured gun mantlets and
    two long finned flak barrels with muzzle brakes."""
    k = Variant("flak_carrier", "dragoon", 0.175)
    k.on("Turret", "Hailstorm_Radar")
    # radar mast on the turret's rear deck, dish facing forward, a small whip antenna beside it
    k.box((0.34, 0.34, 0.1), (0, 0.42, 0.97), "steel", faces={"top": "vent"}, bevel=0.02)
    k.box((0.09, 0.09, 0.42), (0, 0.42, 1.2), "steel_d")
    k.box((0.7, 0.05, 0.34), (0, 0.38, 1.5), "white", faces={"back": "steel_d"}, bevel=0.01)
    k.box((0.12, 0.08, 0.08), (0, 0.4, 1.5), "black")
    k.cyl(0.012, 0.5, (0.26, 0.5, 0.95), "black", axis="z", seg=6)
    for s in (-1, 1):
        # ammunition drums on the turret flanks, with team-coloured end plates
        k.cyl(0.19, 0.5, (s * 0.52, 0.25, 0.78), "steel", axis="y", seg=12, caps={0: "steel_d", 1: "steel_d"})
        k.box((0.06, 0.46, 0.06), (s * 0.52, 0.5, 0.97), "team")
        k.box((0.16, 0.5, 0.34), (s * 0.4, 0.1, 0.7), "steel_d", bevel=0.02)
    k.on("Turret_Barrel", "Hailstorm_Guns")
    # armoured cradle across both guns
    k.box((0.62, 0.34, 0.34), (0, -0.3, 0.72), "steel", faces={"top": "team"}, bevel=0.03)
    for s in (-1, 1):
        x = s * 0.2
        k.box((0.14, 0.3, 0.24), (x, -0.5, 0.72), "steel_d", bevel=0.02)  # mantlet
        k.cyl(0.062, 0.62, (x, -0.62, 0.72), "steel", axis="-y", seg=10)  # cooling sleeve
        k.cyl(0.036, 1.55, (x, -0.55, 0.72), "gun", axis="-y", seg=8)  # barrel
        k.cyl(0.058, 0.12, (x, -1.36, 0.72), "steel_d", axis="-y", seg=8)  # fume extractor
        k.cyl(0.062, 0.2, (x, -2.0, 0.72), "gun", axis="-y", seg=8, r2=0.05)  # muzzle brake
    return k


@model
def mobile_sam():
    """T3 Skylance: the Robin with a four-round missile box and a search radar mast."""
    k = Variant("mobile_sam", "robin", 0.175)
    k.on("Turret_Barrel", "Skylance_Launcher")
    k.box((0.42, 0.8, 0.38), (0.1, -0.02, 0.98), "steel", faces={"top": "team", "front": "steel_d"}, bevel=0.02)
    for dx in (-0.09, 0.09):
        for dz in (-0.08, 0.08):
            k.cyl(0.06, 0.02, (0.1 + dx, -0.42, 0.98 + dz), "red", axis="-y", seg=8)
    k.on("UEX0107", "Skylance_Radar")
    k.box((0.12, 0.12, 0.7), (-0.42, 0.62, 0.9), "steel_d")
    k.box((0.7, 0.06, 0.38), (-0.42, 0.66, 1.3), "white", faces={"back": "steel_d"})
    k.box((0.36, 0.5, 0.25), (0.35, 0.55, 0.7), "steel", faces={"top": "vent"})
    return k


def radar_mast(k, f, levels):
    """Extra dishes for the bigger radar cars: f = model scale / the UC-01R's."""
    k.box((0.36 * f, 0.4 * f, 0.22 * f), (0, 0.38 * f, 0.4 * f), "steel", faces={"top": "vent"})  # generator
    k.box((0.08 * f, 0.08 * f, 0.9 * f), (0, 0.15 * f, 1.25 * f), "steel_d")
    k.box((0.7 * f, 0.05 * f, 0.3 * f), (0, 0.18 * f, 1.62 * f), "white", faces={"back": "steel_d"})
    if levels > 2:
        k.box((0.06 * f, 0.06 * f, 0.6 * f), (0, 0.15 * f, 1.95 * f), "steel_d")
        k.sphere(0.2 * f, (0, 0.15 * f, 2.3 * f), "white", seg=12, rings=6)
        k.box((0.05 * f, 0.05 * f, 0.4 * f), (0.3 * f, 0.5 * f, 0.6 * f), "black")


@model
def radar_car2():
    """UC-02R: the radar car with a second dish on a raised mast and a generator."""
    k = Variant("radar_car2", "radar_car", 0.375)
    k.on("ULX0103", "Radar2")
    radar_mast(k, 1.2, 2)
    return k


@model
def radar_car3():
    """UC-03R: the radar car with a dish, a radome and a generator."""
    k = Variant("radar_car3", "radar_car", 0.4375)
    k.on("ULX0103", "Radar3")
    radar_mast(k, 1.4, 3)
    return k


# trooper: forearm (gun arm) from elbow to muzzle, in world units at scale 0.1562
ELBOW, MUZZLE = (-0.54, 0.04, 0.66), (-0.54, -0.40, 0.32)


def lerp(a, b, t):
    return tuple(x + (y - x) * t for x, y in zip(a, b))


@model
def at_trooper():
    """AT Trooper: a guided missile launcher over the gun arm and a pack of spare rounds."""
    k = Variant("at_trooper", "trooper", 0.1562)
    k.on("Arm_Right_B02", "Launcher")
    a, b = lerp(ELBOW, MUZZLE, -0.6), lerp(ELBOW, MUZZLE, 1.05)
    a, b = (a[0], a[1], a[2] + 0.1), (b[0], b[1], b[2] + 0.1)
    along(k, a, b, 0.06, "olive", seg=10, caps={0: "black", 1: "black"})
    k.box((0.1, 0.12, 0.08), lerp(a, b, 0.5)[:2] + (lerp(a, b, 0.5)[2] + 0.08,), "steel_d")
    k.on("Torso", "Rounds")
    for s in (-1, 1):
        along(k, (s * 0.1, 0.16, 0.65), (s * 0.1, 0.16, 1.2), 0.055, "olive", seg=8, caps={1: "black"})
    return k


@model
def sniper_trooper():
    """Sniper Trooper: a long rifle with a scope and a hooded cape."""
    k = Variant("sniper_trooper", "trooper", 0.1562)
    k.on("Arm_Right_B02", "Rifle")
    a, b = lerp(ELBOW, MUZZLE, 0.2), lerp(ELBOW, MUZZLE, 1.6)
    along(k, a, b, 0.018, "gun", seg=6)
    m = lerp(ELBOW, MUZZLE, 0.6)
    along(k, (m[0], m[1] + 0.05, m[2] + 0.1), (m[0], m[1] - 0.1, m[2] - 0.02), 0.03, "black", seg=8)
    k.on("Torso", "Cape")
    k.box((0.62, 0.08, 0.6), (0, 0.18, 0.86), "olive", rot=(10, 0, 0))
    k.box((0.24, 0.24, 0.12), (0, -0.25, 1.08), "olive")
    return k


@model
def sam_trooper():
    """SAM Trooper: a shoulder-fired missile tube with a grip stock and a battery pack."""
    k = Variant("sam_trooper", "trooper", 0.1562)
    k.on("Arm_Right_B02", "SamTube")
    a, b = lerp(ELBOW, MUZZLE, -0.8), lerp(ELBOW, MUZZLE, 1.05)
    a, b = (a[0], a[1], a[2] + 0.12), (b[0], b[1], b[2] + 0.12)
    along(k, a, b, 0.07, "steel", seg=10, caps={0: "black", 1: "red"})
    m = lerp(a, b, 0.4)
    k.box((0.04, 0.14, 0.12), (m[0] + 0.09, m[1], m[2] + 0.04), "steel_d")  # IFF antenna
    k.on("Torso", "Battery")
    k.box((0.3, 0.14, 0.32), (0, 0.2, 0.85), "steel_d", faces={"back": "team"})
    return k


@model
def commander():
    """Commander: the trooper armour with a command radio pack, whip antennas and team pauldrons."""
    f = 0.475 / 0.1562
    k = Variant("commander", "trooper", 0.475)
    k.on("Torso", "Command")
    k.box((0.46 * f, 0.2 * f, 0.4 * f), (0, 0.25 * f, 0.9 * f), "steel", faces={"back": "team", "top": "vent"}, bevel=0.02 * f)
    for s in (-1, 1):
        k.box((0.012 * f, 0.012 * f, 0.6 * f), (s * 0.15 * f, 0.3 * f, 1.4 * f), "black")
    for s in (-1, 1):
        k.box((0.22 * f, 0.34 * f, 0.08 * f), (s * 0.4 * f, -0.12 * f, 1.1 * f), "team", rot=(0, s * 20, 0), bevel=0.01 * f)
    return k


@model
def drone_operator():
    """Recon Drone Operator: a control station backpack with a tall antenna and a folded spare quadcopter."""
    k = Variant("drone_operator", "trooper", 0.1562)
    k.on("Torso", "Station")
    k.box((0.34, 0.16, 0.36), (0, 0.22, 0.86), "olive", faces={"back": "team", "top": "steel_d"}, bevel=0.015)
    k.box((0.012, 0.012, 0.7), (0.12, 0.26, 1.38), "black")  # whip antenna
    k.box((0.05, 0.05, 0.12), (-0.11, 0.26, 1.08), "black")  # link antenna stub
    k.box((0.26, 0.05, 0.26), (0, 0.33, 0.84), "tank", faces={"back": "team"})  # spare drone, arms folded
    for s in (-1, 1):
        for t in (-1, 1):
            k.cyl(0.035, 0.03, (s * 0.12, 0.36, 0.84 + t * 0.12), "black", axis="y", seg=8)
    return k


@model
def strike_operator():
    """Strike Drone Operator: FPV goggles and a backpack with a quadcopter and two spare warheads."""
    k = Variant("strike_operator", "trooper", 0.1562)
    k.on("Torso", "Rig")
    k.box((0.34, 0.15, 0.34), (0, 0.22, 0.86), "olive", faces={"back": "steel_d"}, bevel=0.015)
    for s in (-1, 1):  # spare warheads either side of the pack
        along(k, (s * 0.2, 0.22, 0.68), (s * 0.2, 0.22, 1.02), 0.045, "olive", seg=8, caps={1: "yellow"})
    k.box((0.012, 0.012, 0.4), (0.1, 0.26, 1.2), "black")  # antenna
    k.box((0.22, 0.04, 0.2), (0, 0.32, 0.86), "gun", faces={"back": "team"})  # strapped FPV drone
    k.box((0.16, 0.06, 0.06), (0, -0.1, 1.22), "black")  # goggles over the helmet's visor
    return k
