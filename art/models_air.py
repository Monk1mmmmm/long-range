"""Aircraft of the playable faction: present-day drones, jets, helicopters and a tilt-rotor in light grey
with team-coloured tails and wing tips. Rotors and propellers are their own parts (spun by the unit JSON's
"spin"); they are left out of the hit box."""
import math

from kit import Kit, model


def fuselage(k, y0, y1, w, h, z, nose=0.3, tail_w=0.35, mat="tank", seg=8):
    """Body along y from y0 (nose) to y1: an octagonal section tapering at the tail, a cone at the nose."""
    sec = [(w / 2 * math.cos(a), z + h / 2 * math.sin(a)) for a in [math.pi / 8 + i * 2 * math.pi / seg for i in range(seg)]]
    k.prism(sec, "y", y0 + nose, y1, mat, scale1=tail_w)
    k.cyl(min(w, h) / 2 * 0.98, nose, (0, y0 + nose, z), mat, axis="-y", r2=0.02, seg=seg, smooth=False)


def wing(k, root_y, root_c, span, tip_c, sweep, z, t=0.03, mat="tank", tip_mat="team", tip=0.12, x0=0.0, side=(-1, 1)):
    """Pair of wings: root chord at y=root_y.., span from x0 to x0+span, tip moved back by `sweep`."""
    for s in side:
        plan = [(s * x0, root_y), (s * (x0 + span), root_y + sweep), (s * (x0 + span), root_y + sweep + tip_c), (s * x0, root_y + root_c)]
        k.prism(plan, "z", z, z + t, mat)
        if tip_mat:
            f = 1 - tip / span
            a = (s * (x0 + span * f), root_y + sweep * f)
            b = (s * (x0 + span * f), root_y + sweep * f + root_c + (tip_c - root_c) * f)
            plan2 = [a, (s * (x0 + span), root_y + sweep), (s * (x0 + span), root_y + sweep + tip_c), b]
            k.prism(plan2, "z", z + t, z + t + 0.004, tip_mat)


def fin(k, x, y, h, c, sweep, z, t=0.025, mat="team", cant=0.0):
    """Vertical tail fin at x, root chord c at y, height h."""
    prof = [(y, z), (y + c, z), (y + c, z + h * 0.3), (y + sweep + c * 0.4, z + h), (y + sweep, z + h)]
    with k.at((x, 0, 0), rot=(0, cant, 0)):
        k.prism(prof, "x", -t / 2, t / 2, mat)


def canopy(k, y, z, w, l, h):
    k.sphere(1.0, (0, y, z), "canopy", seg=10, rings=6, scale=(w / 2, l / 2, h))


def prop(k, name, pos, r, blades=3, axis="y", parent=None):
    """Propeller part (spins about the forward axis): hub and blades in the plane across `axis`."""
    p = k.part(name, pos, parent)
    x, y, z = pos
    with k.into(p):
        k.sphere(r * 0.18, (x, y, z), "gun", seg=8, rings=4, scale=(1, 1.4, 1))
        for i in range(blades):
            a = 360 * i / blades
            with k.at((x, y, z), rot=(0, a, 0)):
                k.box((r * 0.16, 0.015, r), (0, 0, r / 2), "black", rot=(0, 0, 0))
    k.nohit.add(p)
    return p


def rotor(k, name, pos, r, blades=4, parent=None, mat="black"):
    """Helicopter rotor part (spins about the vertical): hub and blades."""
    p = k.part(name, pos, parent)
    x, y, z = pos
    with k.into(p):
        k.cyl(r * 0.07, 0.08, (x, y, z - 0.04), "gun", seg=8)
        for i in range(blades):
            with k.at((x, y, z), rot=(0, 0, 360 * i / blades)):
                k.box((r, r * 0.07, 0.015), (r / 2, 0, 0.0), mat)
    k.nohit.add(p)
    return p


@model
def air_scout():
    """T1 recon drone: high wing, twin booms, pusher propeller, sensor ball."""
    k = Kit("air_scout")
    fuselage(k, -0.4, 0.15, 0.14, 0.14, 0.0, nose=0.12, tail_w=0.6)
    wing(k, -0.08, 0.14, 0.45, 0.1, 0.02, 0.06, t=0.02, x0=0.05)
    for s in (-1, 1):
        k.box((0.025, 0.55, 0.025), (s * 0.14, 0.3, 0.06), "tank")
        fin(k, s * 0.14, 0.48, 0.12, 0.1, 0.03, 0.06, mat="team")
    k.box((0.3, 0.08, 0.015), (0, 0.55, 0.17), "team")
    k.sphere(0.045, (0, -0.28, -0.07), "glass", seg=8, rings=4)
    prop(k, "Prop", (0, 0.18, 0.0), 0.12, blades=2)
    return k


@model
def dove():
    """T2 Dove: loitering munition, cruciform tail, folding wings, warhead nose, pusher propeller."""
    k = Kit("dove")
    k.cyl(0.06, 0.5, (0, -0.25, 0.0), "tank", axis="y", seg=10)
    k.cyl(0.06, 0.14, (0, -0.25, 0.0), "olive", axis="-y", r2=0.015, seg=10)
    k.cyl(0.062, 0.06, (0, -0.2, 0.0), "yellow", axis="y", seg=10)
    wing(k, -0.12, 0.09, 0.26, 0.07, 0.04, 0.04, t=0.015, x0=0.04)
    for a in (45, 135, 225, 315):
        with k.at((0, 0.18, 0), rot=(0, a, 0)):
            k.box((0.015, 0.09, 0.12), (0, 0, 0.1), "team")
    prop(k, "Prop", (0, 0.27, 0.0), 0.09, blades=2)
    return k


@model
def interceptor():
    """T1 Porter: light jet fighter, swept wing, twin tails."""
    k = Kit("interceptor")
    fuselage(k, -0.52, 0.45, 0.16, 0.14, 0.0, nose=0.22, tail_w=0.7)
    wing(k, -0.05, 0.38, 0.4, 0.1, 0.24, -0.01, x0=0.07)
    wing(k, 0.32, 0.14, 0.14, 0.07, 0.06, -0.0, t=0.02, x0=0.07, tip_mat=None)  # tailplanes
    for s in (-1, 1):
        fin(k, s * 0.07, 0.25, 0.16, 0.16, 0.1, 0.05, cant=s * 15)
        k.box((0.06, 0.22, 0.07), (s * 0.1, -0.15, -0.02), "steel")  # intakes
    canopy(k, -0.3, 0.06, 0.09, 0.2, 0.06)
    k.cyl(0.045, 0.05, (0, 0.45, 0.0), "gun", axis="y", seg=8)
    return k


@model
def interceptor3():
    """T3 Cyclone: faceted stealth fighter, diamond wing, canted tails, internal bays."""
    k = Kit("interceptor3")
    body = [(-0.08, -0.7), (0.08, -0.7), (0.24, -0.1), (0.6, 0.3), (0.62, 0.42), (0.18, 0.5), (0.1, 0.62), (-0.1, 0.62),
            (-0.18, 0.5), (-0.62, 0.42), (-0.6, 0.3), (-0.24, -0.1)]
    k.prism(body, "z", -0.05, 0.03, "tank", caps={1: "steel"})
    ridge = [(-0.14, -0.55), (0.14, -0.55), (0.2, 0.45), (-0.2, 0.45)]
    k.prism(ridge, "z", 0.03, 0.1, "steel", scale1=0.6)
    canopy(k, -0.35, 0.08, 0.1, 0.26, 0.07)
    for s in (-1, 1):
        fin(k, s * 0.16, 0.25, 0.2, 0.22, 0.14, 0.05, cant=s * 28)
        k.box((0.36, 0.015, 0.004), (s * 0.4, 0.2, 0.035), "team", rot=(0, 0, s * 33))
    k.box((0.2, 0.05, 0.03), (0, 0.6, 0.02), "black")  # flat nozzle
    return k


@model
def fighter_bomber():
    """T2 Harrier: swept-wing strike jet with bombs and fuel tanks on underwing pylons."""
    k = Kit("fighter_bomber")
    fuselage(k, -0.6, 0.55, 0.18, 0.16, 0.0, nose=0.24, tail_w=0.65)
    wing(k, -0.05, 0.38, 0.48, 0.12, 0.3, -0.02, x0=0.08)
    wing(k, 0.38, 0.16, 0.16, 0.08, 0.08, 0.0, t=0.02, x0=0.07, tip_mat=None)
    fin(k, 0, 0.3, 0.22, 0.2, 0.14, 0.06)
    canopy(k, -0.33, 0.07, 0.1, 0.24, 0.07)
    for s in (-1, 1):
        k.box((0.07, 0.3, 0.08), (s * 0.12, -0.12, -0.02), "steel")
        for x, mat, r in ((0.25, "olive", 0.035), (0.42, "white", 0.03)):
            k.box((0.015, 0.12, 0.04), (s * x, 0.1, -0.05), "steel_d")
            k.cyl(r, 0.24, (s * x, -0.03, -0.09), mat, axis="y", seg=8, caps={0: mat})
            k.cyl(r, 0.06, (s * x, -0.03, -0.09), mat, axis="-y", r2=0.005, seg=8, smooth=False)
    k.cyl(0.05, 0.05, (0, 0.55, 0.0), "gun", axis="y", seg=8)
    return k


@model
def bomber():
    """T1 bomber: straight-winged twin turboprop with a glazed nose and a bomb bay."""
    k = Kit("bomber")
    fuselage(k, -0.6, 0.6, 0.2, 0.22, 0.0, nose=0.16, tail_w=0.4)
    k.sphere(1.0, (0, -0.44, 0.0), "canopy", seg=10, rings=6, scale=(0.095, 0.12, 0.1))
    wing(k, -0.18, 0.28, 0.6, 0.16, 0.04, 0.02, x0=0.09)
    wing(k, 0.44, 0.15, 0.22, 0.1, 0.04, 0.05, t=0.02, x0=0.03, tip_mat=None)
    fin(k, 0, 0.42, 0.22, 0.17, 0.1, 0.08)
    k.box((0.12, 0.3, 0.02), (0, -0.05, -0.11), "black")  # bomb bay
    props = []
    for s in (-1, 1):
        k.cyl(0.06, 0.4, (s * 0.3, -0.26, 0.02), "tank", axis="y", seg=10)
        k.cyl(0.062, 0.06, (s * 0.3, -0.24, 0.02), "team", axis="y", seg=10)
        props.append(prop(k, "Prop_L" if s < 0 else "Prop_R", (s * 0.3, -0.29, 0.02), 0.2, blades=3))
    return k


@model
def gunship():
    """T2 gunship: attack helicopter, tandem cockpit, stub wings with rocket pods, chin gun."""
    k = Kit("gunship")
    fuselage(k, -0.65, 0.2, 0.3, 0.36, 0.0, nose=0.2, tail_w=0.55)
    k.cyl(0.05, 0.6, (0, 0.18, 0.06), "tank", axis="y", r2=0.035, seg=8)  # tail boom
    canopy(k, -0.38, 0.12, 0.16, 0.4, 0.12)
    fin(k, 0, 0.66, 0.26, 0.12, 0.08, 0.05)
    k.box((0.3, 0.08, 0.015), (0, 0.7, 0.08), "team")
    for s in (-1, 1):
        k.box((0.3, 0.14, 0.03), (s * 0.28, -0.05, -0.02), "steel")
        k.cyl(0.05, 0.24, (s * 0.38, -0.17, -0.07), "olive", axis="y", seg=8, caps={0: "black"})
        k.cyl(0.04, 0.2, (s * 0.24, -0.15, -0.07), "white", axis="y", seg=8)
        k.box((0.05, 0.4, 0.03), (s * 0.12, -0.1, -0.2), "gun")  # skids
    k.box((0.1, 0.1, 0.08), (0, -0.56, -0.17), "gun")
    k.cyl(0.015, 0.2, (0, -0.6, -0.18), "gun", axis="-y", seg=6)
    k.box((0.2, 0.3, 0.12), (0, -0.05, 0.22), "steel", faces={"top": "vent"})
    rotor(k, "Rotor", (0, -0.05, 0.36), 0.8, blades=4)
    p = k.part("TailRotor", (0.045, 0.75, 0.15))
    with k.into(p):
        for i in range(2):
            with k.at((0.045, 0.75, 0.15), rot=(90 * i + 45, 0, 0)):
                k.box((0.012, 0.3, 0.03), (0, 0, 0), "black")
    k.nohit.add(p)
    return k


@model
def air_transport():
    """T2 Stork: tandem-rotor heavy transport helicopter with landing gear that folds in flight."""
    k = Kit("air_transport")
    w, h = 1.3, 1.25
    sec = [(-w / 2, 0.15), (w / 2, 0.15), (w / 2 + 0.05, 0.75), (w / 2 - 0.05, h), (-w / 2 + 0.05, h), (-w / 2 - 0.05, 0.75)]
    k.prism(sec, "y", -2.2, 2.0, "tank", side_mats=["steel_d", "tank", "tank", "team", "tank", "tank"])
    nose = [(-w / 2, 0.15), (w / 2, 0.15), (w / 2 + 0.05, 0.75), (w / 2 - 0.05, h), (-w / 2 + 0.05, h), (-w / 2 - 0.05, 0.75)]
    k.prism(nose, "y", -2.2, -2.75, "tank", scale1=0.55)
    for s in (-1, 1):
        k.box((0.03, 0.45, 0.25), (s * 0.5, -2.5, 0.95), "glass", rot=(0, 0, s * 20))
        k.box((0.3, 2.8, 0.45), (s * 0.78, -0.2, 0.4), "tank", faces={"top": "steel"}, bevel=0.04)  # sponsons
        for y in (-1.3, -0.4, 0.5, 1.4):
            k.box((0.02, 0.22, 0.22), (s * (w / 2 + 0.035), y, 0.95), "glass")
    k.box((0.5, 0.4, 0.2), (0, -2.55, 1.0), "glass", rot=(-30, 0, 0))
    # rear pylon and front rotor mast
    k.prism([(1.7, h), (2.4, h), (2.4, 2.05), (1.95, 2.05)], "x", -0.35, 0.35, "tank", side_mats=["tank", "team", "steel", "tank"])
    k.box((0.5, 0.6, 0.25), (0, -1.9, h + 0.12), "steel", faces={"top": "vent"})
    k.box((0.6, 2.5, 0.2), (0, 0.2, h + 0.1), "steel", faces={"top": "vent"})  # engine fairing
    k.box((1.1, 0.04, 0.9), (0, 2.38, 0.65), "metal_door")  # rear ramp
    rotor(k, "Rotor_F", (0, -1.9, h + 0.32), 2.1, blades=3)
    rotor(k, "Rotor_B", (0, 2.15, 2.12), 2.1, blades=3)
    for name, x, y in (("Gear_FL", -0.45, -1.8), ("Gear_FR", 0.45, -1.8), ("Gear_BL", -0.8, 1.2), ("Gear_BR", 0.8, 1.2)):
        g = k.part(name, (x, y, 0.3))
        with k.into(g):
            k.box((0.06, 0.06, 0.3), (x, y, 0.15), "gun")
            k.cyl(0.12, 0.1, (x - 0.05, y, 0.1), "rubber", axis="x", seg=10)
        side = -1 if x < 0 else 1
        k.key("land", g, [(1, None, (0, 0, 0)), (20, None, (0, side * 80, 0))])
    return k


@model
def fast_transport():
    """T3 Swift: high-winged airdrop transport, twin turboprops, T-tail, rear cargo ramp."""
    k = Kit("fast_transport")
    w, h = 0.95, 1.0
    sec = [(-w / 2, 0.1), (w / 2, 0.1), (w / 2 + 0.04, 0.6), (w / 2 - 0.08, h), (-w / 2 + 0.08, h), (-w / 2 - 0.04, 0.6)]
    k.prism(sec, "y", -1.5, 1.5, "tank", side_mats=["steel_d", "tank", "tank", "team", "tank", "tank"])
    k.prism(sec, "y", -1.5, -2.2, "tank", scale1=0.45)
    k.prism([(1.5, 0.15), (2.5, 0.75), (2.5, 0.95), (1.5, h)], "x", -0.3, 0.3, "tank")  # upswept tail cone
    k.box((0.5, 0.35, 0.2), (0, -1.85, 0.75), "glass", rot=(-35, 0, 0))
    wing(k, -0.5, 1.0, 1.9, 0.55, 0.25, h, t=0.09, x0=0.35)
    k.box((0.7, 2.6, 0.12), (0, -0.1, h + 0.04), "steel", faces={"top": "vent"})  # wing-root fairing
    fin(k, 0, 1.75, 1.15, 0.75, 0.45, h - 0.05)
    wing(k, 2.05, 0.45, 0.85, 0.3, 0.15, 2.0, t=0.05, x0=0.0, tip_mat=None)  # T-tailplane
    for s in (-1, 1):
        k.box((0.04, 1.6, 0.4), (s * (w / 2 + 0.03), 0.1, 0.5), "team")
        k.box((0.3, 1.5, 0.4), (s * (w / 2 + 0.08), 0.35, 0.25), "steel")  # gear pods
        x = s * 1.15
        k.cyl(0.19, 1.3, (x, -0.95, 0.78), "tank", axis="y", seg=10, caps={0: "steel_d"})
        k.cyl(0.195, 0.14, (x, -0.55, 0.78), "team", axis="y", seg=10)
        k.box((0.05, 0.7, 0.2), (x, -0.4, 0.92), "steel_d")  # pylon
        prop(k, "Prop_L" if s < 0 else "Prop_R", (x, -0.98, 0.78), 0.55, blades=4)
    k.box((0.85, 0.04, 0.75), (0, 1.62, 0.5), "metal_door", rot=(-8, 0, 0))  # rear ramp
    return k
