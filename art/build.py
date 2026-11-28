"""Builds the game's own models from the scripts in art/models_*.py with Blender's Python module.

    .venv/bin/python art/build.py                  # every model
    .venv/bin/python art/build.py pump apc_heavy3  # only these
    .venv/bin/python art/build.py --list
    .venv/bin/python art/build.py pump --blend     # also save art/blend/pump.blend to open in Blender

Setup once: python3 -m venv .venv && .venv/bin/pip install bpy numpy pillow (bpy = Blender as a Python module;
it needs the Python version its release was made for, 3.13 for bpy 5.x).
Writes assets/models/<name>.glb, <name>_albedo.png, <name>_team.png through tools/blend_export.py and prints the
model's body bounds (parts named *Barrel*/*Muzzle* and those in Kit.nohit left out), the source of hit_box.
Look at the result with tools/preview_glb.py."""
import contextlib, io, os, runpy, sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
OUT = os.environ.get("LR_MODELS_OUT") or os.path.join(HERE, "..", "assets", "models")

import bpy  # noqa: E402
from mathutils import Vector  # noqa: E402
import kit  # noqa: E402

MODELS = {}
for mod in ("models_buildings", "models_defence", "models_vehicles", "models_air", "models_naval", "models_civilian", "models_variants", "models_themed", "models_urban"):
    try:
        m = __import__(mod)
    except ModuleNotFoundError as e:
        if e.name != mod:
            raise
        continue
    for k, v in vars(m).items():
        if callable(v) and getattr(v, "is_model", False):
            MODELS[k] = v


def bounds(k):
    lo, hi = Vector((1e9,) * 3), Vector((-1e9,) * 3)
    for p in k.parts:
        if p.empty or p in k.nohit or any(s in p.name.lower() for s in ("barrel", "muzzle")):
            continue
        for v in p.obj.data.vertices:
            w = p.obj.matrix_world @ v.co
            lo = Vector(map(min, lo, w))
            hi = Vector(map(max, hi, w))
    # game axes: x, height (z), length (y)
    return [round(hi.x - lo.x, 2), round(hi.z, 2), round(hi.y - lo.y, 2)]


def build(name, save_blend=False):
    k = MODELS[name]()
    k.finish()
    hb = k.hit_box
    if save_blend:
        os.makedirs(os.path.join(HERE, "blend"), exist_ok=True)
        bpy.ops.wm.save_as_mainfile(filepath=os.path.join(HERE, "blend", name + ".blend"))
    # export (tools/blend_export.py parses sys.argv after "--")
    sys.argv = ["blender", "--", OUT, k.name, "--front", getattr(k, "front", "-y")]
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        runpy.run_path(os.path.join(HERE, "..", "tools", "blend_export.py"), run_name="__main__")
    warns = [l for l in buf.getvalue().splitlines() if l.startswith("WARNING")]
    tris = sum(len(p.obj.data.loop_triangles) if not p.empty and (p.obj.data.calc_loop_triangles() or True) else 0 for p in k.parts)
    if os.environ.get("LR_HIT_OUT"):
        import json
        p = os.environ["LR_HIT_OUT"]
        d = json.load(open(p)) if os.path.exists(p) else {}
        d[name] = hb
        json.dump(d, open(p, "w"), indent=1)
    print(f"{name:18} parts {len(k.parts):3} tris {tris:5} hit_box {hb}" + ("".join("\n  " + w for w in warns)))


def main():
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    if "--list" in sys.argv:
        print(" ".join(MODELS))
        return
    blend = "--blend" in sys.argv
    for n in args or list(MODELS):
        build(n, blend)


if __name__ == "__main__":
    main()
