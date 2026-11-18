#!/usr/bin/env python3
"""Exports a Blender file as a game model (assets/models/NAME.glb + textures) with tools/blend_export.py.

    python3 tools/blend.py art/abbey_factory.blend                # -> assets/models/abbey_factory.glb
    python3 tools/blend.py art/abbey_factory.blend --watch        # again after every save in Blender
    python3 tools/blend.py file.blend --name other --front +y     # other asset name; model faces +Y in Blender

Options: --name NAME (default: the file name), --out DIR (default: assets/models), --front -y|+y|-x|+x
(where the model's front looks in Blender; default -y, the side Blender's Front view, numpad 1, shows),
--brightness B (albedo multiplier written into the JSON block, default 1), --watch.
Blender is found through $BLENDER, then PATH, then the usual install folders.
No packages needed beyond Python 3; the game only reads the .glb, so commit that (and the .blend if you like).
"""
import argparse
import glob
import os
import shutil
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EXPORTER = os.path.join(ROOT, "tools", "blend_export.py")


def find_blender():
    env = os.environ.get("BLENDER")
    if env:
        return env
    found = shutil.which("blender")
    if found:
        return found
    patterns = [
        r"C:\Program Files\Blender Foundation\Blender*\blender.exe",
        r"C:\Program Files (x86)\Steam\steamapps\common\Blender\blender.exe",
        os.path.expandvars(r"%LOCALAPPDATA%\Microsoft\WindowsApps\blender.exe"),
        "/Applications/Blender.app/Contents/MacOS/Blender",
        os.path.expanduser("~/Applications/Blender.app/Contents/MacOS/Blender"),
        "/snap/bin/blender",
    ]
    for p in patterns:
        # Newest version last ("Blender 4.2" < "Blender 5.2"); the Program Files glob may match several.
        hits = sorted(glob.glob(p))
        if hits:
            return hits[-1]
    sys.exit("Blender not found: install it or set BLENDER=/path/to/blender")


def export(blender, a):
    cmd = [blender, "-b", a.file, "--factory-startup", "--python-exit-code", "1", "--python", EXPORTER, "--",
           a.out, a.name, "--front", a.front, "--brightness", str(a.brightness)]
    # Blender prints UTF-8; Windows' default console code page would choke on non-ASCII paths.
    r = subprocess.run(cmd, capture_output=True, text=True, encoding="utf-8", errors="replace")
    # Blender's own chatter is long: show what the exporter printed (up to its LR_END), all of it on failure.
    lines = (r.stdout + r.stderr).splitlines()
    block = False
    for line in lines:
        if line.startswith("unit JSON"):
            block = True
        if line == "LR_END":
            block = False
            continue
        if block or line.startswith(("wrote ", "WARNING", "animations:")):
            print(line)
    if r.returncode != 0:
        print("\n".join(lines[-30:]))
        print(f"export failed (exit {r.returncode})")
    return r.returncode == 0


def main():
    # --watch output shows at once, also in a log; never fail on characters the console cannot show.
    sys.stdout.reconfigure(line_buffering=True, errors="replace")
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("file")
    p.add_argument("--name")
    p.add_argument("--out", default=os.path.join(ROOT, "assets", "models"))
    p.add_argument("--front", default="-y", choices=["-y", "+y", "-x", "+x"])
    p.add_argument("--brightness", type=float, default=1.0)
    p.add_argument("--watch", action="store_true")
    a = p.parse_args()
    a.file = os.path.abspath(a.file)
    a.name = a.name or os.path.splitext(os.path.basename(a.file))[0]
    if not os.path.exists(a.file):
        sys.exit(f"{a.file}: no such file")
    blender = find_blender()
    ok = export(blender, a)
    if not a.watch:
        sys.exit(0 if ok else 1)
    print(f"watching {a.file} (Ctrl+C stops)")
    last = os.path.getmtime(a.file)
    try:
        while True:
            time.sleep(1.0)
            try:
                m = os.path.getmtime(a.file)
            except FileNotFoundError:  # Blender replaces the file when saving
                continue
            if m != last:
                last = m
                time.sleep(0.5)
                print(time.strftime("%H:%M:%S"), "changed, exporting")
                export(blender, a)
    except KeyboardInterrupt:
        pass


if __name__ == "__main__":
    main()
