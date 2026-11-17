# Tools

Python scripts for bringing content from the *Long Range Tank* Supreme Commander mod into the game.
They need Python 3 with `numpy` and `Pillow` (`pip install numpy pillow`). Run them from the repository root.

## convert_assets.py - models, textures, portraits

    python3 tools/convert_assets.py "<path to the LongRangeTank mod folder>" [--only trooper,buggy]

`--only` converts just the named entries (and writes `tools/out/models_info.json` for those only).
For every entry in the `UNITS` table at the top of the script (asset name -> unit folder inside the mod) it
reads the unit's `*_LOD0.scm` model and its DDS textures and writes:

| Output | From | Notes |
|---|---|---|
| `assets/models/<name>.glb` | `*_LOD0.scm`, the blueprint's `.sca` files | one glTF node per SCM bone, same names and hierarchy; the blueprint's animations (read by `tools/sca.py`, table `ANIMS`: walk, upgrade, open, work, doors, unpack, land, water, rolloff) as named glTF animations, times / the blueprint's rate, tracks only for bones that move |
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
| OSM highways (`highway=*` up to service roads and tracks; tunnels left out) | roads: asphalt, gravel or dirt track by `surface`, drawn wider than true scale (`ROAD_WIDTH`); trees and rocks keep off them, buildings keep a cell off the main roads (tertiary and up) and off every other street except service roads and tracks |
| OSM river named by `--river` (`waterway=river`), road and railway bridges over it, culverts | water: the land is tilted along the river so it runs level (a river has one level), the bed is dug under it, and every real bridge or culvert stays a land crossing |
| OSM lakes, ponds, reservoirs (`natural=water` that is not a river, `landuse=reservoir/basin`) | lakes, each at its own water level (`lakes` + `lakes.png`): the height the terrain has across it; the bed is dug up to 1.8 under it, the ground around kept above it. `--lake-min 60` is the smallest one kept (square world units), `--no-lakes` leaves them dry |
| OSM fields/meadows, forests, farmyards and construction, industry/commerce, built-up blocks, sand, water, rock + steep slopes | ground texture layers (`layerN.png`): meadow, forest floor, dirt, gravel, paving, sand, mud, stone over grass |

Start positions (`--players`, default 4: corners) are searched in open, flat ground at equal distance from the
centre and levelled; `--starts "x,z;x,z"` sets them by hand (world units, from the preview). Every start gets
the same deposits at the same offsets turned towards the centre, plus a ring of six in the middle.
`--max-props` (600) caps the buildings: the real ones are thinned evenly (landmarks first, big ones more likely).
A building that does not fit where the real one stands is moved up to two cells (towns) or one (villages).
`--village 0.45` keeps only that share of the buildings outside towns (built-up share over ~350 m), so towns
are packed solid and villages stay loose; the default 1.0 keeps everything that fits.
`--river NAME` makes that river water, `--river-width 5,7` its width (world units, upstream,downstream).
In towns (with `--village`) residential side streets are drawn narrower and give way to the buildings: their
paint stops under a building, so they show as the lanes between them; main roads and through streets stay open.
Ground lower than the river ends up just above the water. Lakes are filled at their own levels and stay free
of trees and buildings; other rivers and streams stay dry. The preview shows the water in blue and the bridges as yellow rings.

Downloads are cached in `tools/out/osm_cache` (delete the files to fetch fresh data); the Overpass servers are
sometimes busy, the script retries. It writes a check picture `tools/out/NAME_preview.png`: terrain, roads, trees
(green), rocks (white), buildings (red), hostile (magenta), deposits (cyan), starts (yellow).
Data: (c) OpenStreetMap contributors (ODbL); elevation: Mapzen / AWS Terrain Tiles.

Example (Jablonec nad Nisou and Rychnov u Jablonce, the `jablonec` map):

    .venv/bin/python tools/osm_map.py jablonec --bbox 50.661,15.091,50.751,15.233 --title "Jablonec nad Nisou"

Liberec and Jablonec with the Lužická Nisa (the `liberec` map):

    .venv/bin/python tools/osm_map.py liberec --bbox 50.6689,14.9911,50.8078,15.2092 --cells 512 --title "Liberec and Jablonec" --river "Lužická Nisa" --river-width 4,6 --village 0.45 --max-props 9000

## scmap_map.py - port a Supreme Commander map

    .venv/bin/python tools/scmap_map.py "<FA>/maps/adaptive_ivano_frankivsk.v0002" ivano_frankivsk --title "Ivano-Frankivsk" [--variants 4,6,8,10]

Reads the map's `.scmap` (`tools/scmap.py`, FA version 56 / FAF 60; `python3 tools/scmap.py file.scmap` prints a
summary) and `_save.lua` and writes `assets/maps/NAME` (the smallest variant) plus `NAME_<k>p` for the others.
All variants share terrain, ground, roads, buildings, trees and deposits and keep the whole map; only the starts
differ (the k of the map's starts with the largest smallest distance between them).

| SupCom | Becomes |
|---|---|
| heightmap (1 unit per vertex) | 2 units per cell: tent average, the lowest point where the window touches water (narrow rivers stay whole) |
| water elevation | sea level |
| strata masks | ground layers by texture name (`STRATA`: farms -> meadow, gravel, dirt, grass -> forest floor...); a rock stratum painted as lines becomes vector roads (thinned, traced, simplified) |
| scmap props (trees, logs, rocks; `PROPS`) | trees, shrubs, stones/rocks/boulders |
| Mass markers | deposits (hydrocarbons are dropped: no such resource) |
| `ARMY_n` markers | start positions |
| civilian units (`CIVS`) | neutral buildings: big ones first where they fit (one free cell around, not on roads or deposits, not within 40 of any start), then residential towers, then a small mod house where a tower did not fit |

Check pictures: `tools/out/<variant>_preview.png`.
