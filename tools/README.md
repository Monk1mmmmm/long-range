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

## osm_map.py - a map of a real place

    .venv/bin/python tools/osm_map.py NAME --bbox SOUTH,WEST,NORTH,EAST [options]
    .venv/bin/python tools/osm_map.py NAME --center LAT,LON --km 10 [options]

Turns a rectangle of the real world into `assets/maps/NAME` (playable at once, also in the editor).
The rectangle is in degrees: on openstreetmap.org, *Export* shows the box of the current view, or right-click >
*Show address* gives a point. Maps are square, so a rectangle is widened on its short side.

| Source | Becomes |
|---|---|
| AWS Terrain Tiles (Mapzen terrarium, zoom 13, ~12 m) | heightmap, true scale x `--relief` (0.85) |
| OSM buildings (`building=*`, with landuse) | neutral props: one game building per real one where it fits, chosen by kind and size (houses, town houses in dense blocks, apartments, churches, schools/offices, factories, farms), turned like the real one |
| OSM forests (`leaf_type` decides fir/pine vs oak/birch), scrub, parks, lone trees | trees and shrubs |
| OSM rocks, bare rock; steep or high forest ground | stones, rocks, boulders |
| OSM water towers, towers/masts, power pylons | `water_tower`, `comm_tower`, `pylon` |
| highest hilltop away from the bases | neutral comm tower + three hostile bunkers |

Start positions (`--players`, default 4: corners) are searched in open, flat ground at equal distance from the
centre and levelled; `--starts "x,z;x,z"` sets them by hand (world units, from the preview). Every start gets
the same deposits at the same offsets turned towards the centre, plus a ring of six in the middle.
`--max-props` (600) caps the buildings: the real ones are thinned evenly (landmarks first, big ones more likely).
Water is not flooded (the game has one water level for the whole map); lakes stay free of trees and buildings.

Downloads are cached in `tools/out/osm_cache` (delete the files to fetch fresh data); the Overpass servers are
sometimes busy, the script retries. It writes a check picture `tools/out/NAME_preview.png`: terrain, trees
(green), rocks (white), buildings (red), hostile (magenta), deposits (cyan), starts (yellow).
Data: (c) OpenStreetMap contributors (ODbL); elevation: Mapzen / AWS Terrain Tiles.

Example (Jablonec nad Nisou and Rychnov u Jablonce, the `jablonec` map):

    .venv/bin/python tools/osm_map.py jablonec --bbox 50.661,15.091,50.751,15.233 --title "Jablonec nad Nisou"
