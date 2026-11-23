"""Unit portraits in the style of the Long Range Tank mod's icons: a 3/4 view of the model, no background, soft white
glow round it, 64x64 (assets/icons/units/<id>.png, the `portrait` of the unit JSON).
Renders with Blender (art/render.py, bpy in .venv) the units whose portrait is missing or borrowed from another unit.
    .venv/bin/python tools/render_icons.py [id ...] [--all]     (--all: also redo units that have their own icon)
The `portrait` lines of the unit JSON are edited by hand (CRLF files)."""
import json, os, sys, tempfile
import numpy as np
from PIL import Image, ImageFilter

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SIZE, FIT, RENDER = 64, 58, 384


def glow(img):
    """White halo round the shape, like the mod's icons."""
    a = np.array(img)[..., 3]
    m = Image.fromarray(a).filter(ImageFilter.MaxFilter(3))
    halo = np.array(m.filter(ImageFilter.GaussianBlur(1.6)), np.float32) / 255
    halo = np.clip(halo * 1.5, 0, 1) * 0.85
    out = np.zeros(a.shape + (4,), np.float32)
    out[..., :3] = 235
    out[..., 3] = halo * 255
    base = Image.fromarray(out.astype(np.uint8))
    base.alpha_composite(img)
    return base


def to_icon(path):
    im = Image.open(path).convert("RGBA")
    bb = im.getchannel("A").point(lambda v: 255 if v > 8 else 0).getbbox()
    im = im.crop(bb)
    k = FIT / max(im.size)
    im = im.resize((max(1, round(im.width * k)), max(1, round(im.height * k))), Image.LANCZOS)
    cv = Image.new("RGBA", (SIZE, SIZE), (0, 0, 0, 0))
    cv.alpha_composite(im, ((SIZE - im.width) // 2, (SIZE - im.height) // 2))
    return glow(cv)


def main():
    args = sys.argv[1:]
    d = json.load(open(os.path.join(ROOT, "assets/data/factions/coalition.json")))
    units = d["units"] if isinstance(d, dict) else d
    own = {u["id"] for u in units}
    todo = []
    for u in units:
        p = u.get("portrait")
        mine = p == f"icons/units/{u['id']}.png"
        if args and not args[0].startswith("--"):
            if u["id"] in args:
                todo.append(u)
        elif "--all" in args or not mine:
            todo.append(u)
    tmp = tempfile.mkdtemp()
    os.environ["LR_TRANSPARENT"] = "1"
    os.environ.setdefault("LR_TEAM", "0.45,0.52,0.62")
    sys.path.insert(0, os.path.join(ROOT, "art"))
    import importlib.util
    spec = importlib.util.spec_from_file_location("lr_render", os.path.join(ROOT, "art/render.py"))
    r = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(r)
    os.environ["TMPDIR"] = tmp
    for u in todo:
        mesh = (u.get("model") or {}).get("mesh") or ""
        name = os.path.splitext(os.path.basename(mesh))[0] or u["id"]
        if not os.path.exists(os.path.join(ROOT, "assets/models", name + ".glb")):
            print("no model", u["id"], name)
            continue
        out = os.path.join(ROOT, "assets/icons/units", u["id"] + ".png")
        src = r.render(name, RENDER, None, 35.0)
        to_icon(src).save(out)
        print(u["id"], "<-", name)


main()
