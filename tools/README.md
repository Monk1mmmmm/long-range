# Tools

Python scripts for bringing content from the *Long Range Tank* Supreme Commander mod into the game.
They need Python 3 with `numpy` and `Pillow` (`pip install numpy pillow`). Run them from the repository root.

## convert_assets.py - models, textures, portraits

    python3 tools/convert_assets.py "<path to the LongRangeTank mod folder>"

For every entry in the `UNITS` table at the top of the script (asset name -> unit folder inside the mod) it
reads the unit's `*_LOD0.scm` model and its DDS textures and writes:

| Output | From | Notes |
|---|---|---|
| `assets/models/<name>.glb` | `*_LOD0.scm` | geometry only; one glTF node per SCM bone, same names and hierarchy |
| `assets/models/<name>_albedo.png` | `*_Albedo.dds` | resized to at most 512 px |
| `assets/models/<name>_team.png` | alpha of `*_SpecTeam.dds` | greyscale mask of the team-coloured areas |
| `assets/models/<name>_normal.png` | `*_NormalsTS.dds` | converted from SupCom's packed format to a normal RGB map |
| `assets/icons/units/<name>.png` | `icons/units/<ID>_icon.dds` | build-menu portrait, if the mod has one |
| `tools/out/models_info.json` | - | bone names, bone positions, bounding box and blueprint scale per model |

It overwrites existing files of the same name. To add a unit: add a line to `UNITS`, run the script, then
reference the files from the unit's `model` block in `assets/data/factions/*.json`:

```json
"model": {
  "mesh": "models/crony.glb",
  "albedo": "models/crony_albedo.png",
  "team_mask": "models/crony_team.png",
  "normal": "models/crony_normal.png",
  "scale": 0.07
}
```

`scale` is the blueprint's `Display.UniformScale` (written to `models_info.json`). Turrets work by bone name:
a weapon's `turret` block names the `yaw_bone`, `pitch_bone` and `muzzle_bone`, which are the SCM bone names
(usually `Turret`, `Turret_Barrel`, `Turret_Muzzle`); look them up in `models_info.json`.
`"spin": [{"bone": "RotorBone", "axis": [0, 0, 1], "speed": 140}]` makes a bone rotate continuously.

Coordinates are converted from SupCom's left-handed space to glTF's right-handed one: Z is flipped, so a model
that faces +Z in SupCom faces -Z in the `.glb` (the game's forward direction), and triangle winding is reversed.
Only LOD0 is used; animations (`.sca`) are not converted.

## scm.py - SCM reader

Library used by the converter; also a quick inspector:

    python3 tools/scm.py "<mod>/units/UEX0105/UEX0105_LOD0.scm"

prints the vertex and triangle counts, every bone with its parent, vertex count and rest position, the
bounding box and the UV range. Use it to find bone names or to check a model before converting it.
Supports SCM version 5 (one bone per vertex, 68-byte vertices, 16-bit indices).

## bp.py - blueprint reader

Parses `.bp` files (Lua table literals) into Python dicts: `bp.load(path)`.

## dump_mod.py - blueprint stats

    python3 tools/dump_mod.py "<path to the LongRangeTank mod folder>"

prints one block per unit: tech level, size, health, armor, movement, costs, intel and every weapon with its
damage, range, rate of fire, muzzle velocity, turret speeds and bones. This is where the numbers in
`assets/data/factions/coalition.json` came from.
