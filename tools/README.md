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

## art/build.py - our own models (no Blender install needed)

    python3 -m venv .venv && .venv/bin/pip install bpy numpy pillow   # once; bpy = Blender 5 as a Python module (Python 3.13)
    .venv/bin/python art/build.py                      # every model in art/models_*.py -> assets/models
    .venv/bin/python art/build.py apc_heavy3 pump      # only these
    .venv/bin/python art/build.py pump --blend         # also art/blend/pump.blend, to open and change in Blender
    .venv/bin/python art/render.py out.png pump apc_heavy3 [--anim work:0.5]   # rendered contact sheet
    python3 tools/preview_glb.py out.png --unit bastion crony                  # quick look without Blender

The models that replace the stock Supreme Commander ones (factories, pumpjack extractors, engineers, aircraft,
carriers, city buildings) and the variants of the mod's models (Bastion with reactive armour, Hailstorm, Skylance,
...) are Python: `art/kit.py` builds parts from boxes, extrusions and cylinders, paints them with seamless tile
textures, bakes one 512 atlas (albedo + team mask) with Cycles and exports through `blend_export.py`. Each model is a
function marked `@model`; `build.py` prints the hit box to put into the unit JSON. Without Linux EGL libraries the
renderer needs `apt-get install libegl1 libgl1-mesa-dri` (Workbench/Eevee); baking uses Cycles on the CPU.

## blend.py - models made in Blender

    python3 tools/blend.py art/abbey_factory.blend            # -> assets/models/abbey_factory.glb (+ textures)
    python3 tools/blend.py art/abbey_factory.blend --watch    # exports again every time the file is saved

Needs Blender (found through `$BLENDER`, PATH, then the usual install folders; tested with 5.2) and nothing else.
It runs Blender in the background with `blend_export.py`, which exports the scene with the settings the game's
loader (`src/view/models.rs::load_glb`) needs and prints the unit JSON `model` block to paste. The `.blend` file
is never changed. Options: `--name` (asset name, default the file name), `--out` (default `assets/models`),
`--front -y|+y|-x|+x` (where the model's front looks in Blender, default `-y`: the side the Front view, numpad 1,
shows; the game's forward is -Z), `--brightness` (albedo multiplier in the JSON block, default 1; the mod's
dark textures use 2.1, the default when the JSON leaves it out).

How to build a model for it:

- **Size**: 1 Blender unit = 1 world unit (a tank is ~0.7 wide, a build cell is 2 units), origin on the ground
  at the middle of the footprint, Z up as usual in Blender. Keep `"scale": 1` in the JSON.
- **Parts**: every object is a part of the model; moving parts (turret, barrel, factory platform, doors) are
  separate objects in a parent hierarchy. Their names are what the weapon's `turret` block refers to:
  `yaw_bone` (turns sideways), `pitch_bone` (child of it, raises the barrel), `muzzle_bone` (an empty at the
  muzzle), and `spin` (radar dishes). Modifiers are applied on export.
- **No skinning**: an Armature modifier (vertex weights) is not supported, the game shows the rest pose. Animate
  objects, or parent objects to bones (Ctrl+P > Bone) and animate the bones. No shape keys.
- **Colours without textures** (low poly): a material named `team` is drawn in the owner's colour, `light` in a
  pale team colour, `dark` and `metal` in fixed greys; any other material keeps its Base Color. An object may have
  several materials.
- **Textures**: one set per model (one UV layout): the image linked to Base Color becomes `NAME_albedo.png`, the
  image of a Normal Map node `NAME_normal.png`, and an Image Texture node with "team" in its name or label
  (white = team colour, same size as the albedo; it need not be connected) `NAME_team.png`. With an albedo
  texture all materials use it, except those named team / light / dark / metal. Images that are files on disk
  (PNG) are copied; packed or painted ones are written from their pixels.
- **Animations**: Blender actions, named after what the game plays them for: `walk` (cycle while moving),
  `open` (engineers unfold to build; first frame folded), `upgrade`, `work` (pumps), `doors`, `unpack`,
  `land`, `water`, `rolloff` (factory platform when a unit leaves). Other names are reported and ignored.
  One action may move several objects (action slots). Every frame of the action is sampled, starting at 0 s.

The script warns about skinning, unknown action names and textures it cannot use. The game loads models at
start, so restart it (or the editor) to see a new export.

### Step by step: a new building

1. **Model.** Start Blender, delete the default cube, model at real size (a 6x6-cell factory is 12 x 12 units),
   origin at the ground in the middle, front towards -Y (numpad 1 looks at it). Name the moving objects
   (`Platform`, `Turret`, ...). Save as `art/abbey_factory.blend` (any folder works; `art/` keeps sources together).
2. **Colours.** Either give materials the names `team`, `light`, `dark`, `metal` or any other name with the
   colour you want, or unwrap and paint one texture set (see above).
3. **Animation** (optional). Select the platform, open the Action editor (Dope Sheet > Action Editor), click
   New and name the action `rolloff`, key Location at the first frame, move it out, key, back, key.
4. **Export.** From the repository root:

       python3 tools/blend.py art/abbey_factory.blend --watch          # Linux / macOS
       py tools\blend.py art\abbey_factory.blend --watch               # Windows (or: python tools\blend.py ...)

   It prints what it wrote, any warnings and the `model` block. Leave it running: every Ctrl+S in Blender
   exports again.
5. **Unit JSON.** Paste the `model` block into the unit in `assets/data/factions/<faction>.json` (or the preset
   editor) and set `footprint` / `size` to match the model:

       "model": { "mesh": "models/abbey_factory.glb", "scale": 1.0 },

6. **Look at it.** Start the game (`cargo run -- --skirmish`) or the map editor and build the unit; restart after
   each export. If it faces the wrong way, export with `--front +y` (or `-x` / `+x`); if a textured model looks
   washed out or dark, change `brightness`.

### Windows

The script works the same on Windows; only the commands differ:

- Python: `py tools\blend.py ...` (the Python launcher) or `python tools\blend.py ...`; no packages needed.
- Blender is looked for on PATH, in `C:\Program Files\Blender Foundation\Blender*` (the newest version there),
  in Steam's `steamapps\common\Blender` and as the Microsoft Store app. Anywhere else, set the path once per
  console: `set BLENDER=D:\Apps\Blender\blender.exe` (cmd) or `$env:BLENDER="D:\Apps\Blender\blender.exe"`
  (PowerShell).
- Paths with spaces or Cyrillic letters are fine; quote paths with spaces.

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
| OSM highways (`highway=*` up to service roads and tracks; tunnels left out) | roads: asphalt, gravel or dirt track by `surface`, drawn wider than true scale (`ROAD_WIDTH`); trees and rocks keep off them, buildings keep a cell off the main roads (tertiary and up) and off every other street except service roads and tracks. Then run `longrange --editor --roads NAME` for their look: main roads with lane markings, worn side streets, concrete service roads, cobbles in old centres, sidewalks in towns, nothing under water |
| OSM river named by `--river` (`waterway=river`), road and railway bridges over it, culverts | water: the land is tilted along the river so it runs level (a river has one level), the bed is dug under it, and every real bridge or culvert stays a land crossing ; `--river-roads`: every road over it crosses it, `--river-ford-every D`: fords so crossings are at most D apart along it |
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

**Other source (`--source overture`).** When the Overpass servers are down or blocked, the same map can be made from
Overture Maps (`tools/overture_src.py`; Parquet files on a public S3 bucket, built from OpenStreetMap and other open
data; needs `pip install pyarrow shapely` too). It gives buildings, roads, forests, scrub, rock, beaches, land use, water
and the ocean as the same elements the Overpass query returns, so everything above works unchanged. Differences:
`--river NAME` works (rivers, streams and canals come as named lines, in the local language: `--river "Суджа"`; flagged road and rail bridges make the land crossings, footbridges none), `pedestrian` streets count as living streets (the lanes of an old town),
footways and steps are left out, forests have no leaf type (mixed), and the data is always the present day.
`LR_OVERTURE_RELEASE` picks another monthly release than the one in the script.

- `--sea` makes a coast (Overture source): the ocean polygon becomes the sea at the map's water level
  (`--sea-height`, 6 world units), land is never below it, and the sea bed slopes away from the shore (shallows only
  amphibious units can wade, then deep water). Buildings, trees, roads and bases keep off the water.
- A map of the past: `--drop-bridges S,W,N,E` leaves out the road bridges of primary roads and better inside the
  rectangle, with the ramps and viaducts that only led to them; `--drop-names REGEX` leaves out roads by name.
  Overture has no construction dates, so buildings are the present ones.
- `--infill S,W,N,E` (repeatable) adds made-up small houses on both sides of the residential streets inside a rectangle,
  wherever no real building stands within `--infill-gap` (14 world units; `--infill-step` 9 is the spacing along the street,
  `--infill-max` 700): for villages where the open data has the roads but not the buildings. They come after the real
  buildings, so the real ones are placed exactly as without the option.
- `--cp-area S,W,N,E` keeps the control points inside a rectangle (the city, when the map also shows its surroundings),
  `--dep-area` does the same for the resource deposits (Dubrovnik: none beyond the river), `--cp-town 1.0` takes all control points from the densest places, `--outpost LAT,LON` puts the hostile hilltop outpost
  on a real landmark instead of the highest hill.

Downloads are cached in `tools/out/osm_cache` (delete the files to fetch fresh data); the Overpass servers are
sometimes busy, the script retries. It writes a check picture `tools/out/NAME_preview.png`: terrain, roads, trees
(green), rocks (white), buildings (red), hostile (magenta), deposits (cyan), starts (yellow).
Data: (c) OpenStreetMap contributors (ODbL); elevation: Mapzen / AWS Terrain Tiles.

Example (Jablonec nad Nisou and Rychnov u Jablonce, the `jablonec` map):

    .venv/bin/python tools/osm_map.py jablonec --bbox 50.661,15.091,50.751,15.233 --title "Jablonec nad Nisou"

Sudzha and the villages round it, a duel (the `sudzha_duel` map, made with the Overture source):

    .venv/bin/python tools/osm_map.py sudzha_duel --source overture --center 51.1917,35.280 --km 8 --cells 512 --players 2 --title "Sudzha" --village 0.6 --max-props 1800 --seed 1 --infill 51.196,35.20,51.24,35.36 --infill 51.17,35.20,51.196,35.262 --infill-step 6.5 --infill-gap 12 --river "Суджа" --river-width 3,4

Mostyska with the Sichna (OSM: «Січня», a stream) running through it (the `mostyska` map). The stream is thin; most of
its bridges are not mapped as such in Overture, so `--river-roads` makes every road over it a crossing and
`--river-ford-every 140` adds fords where crossings are further apart along it (17 crossings on 1700 units):

    .venv/bin/python tools/osm_map.py mostyska --source overture --bbox 49.754956,23.087311,49.833239,23.212999 --cells 512 --players 4 --title "Mostyska" --river "Січня" --river-width 2.5,3.5 --river-roads --river-ford-every 140 --village 0.6 --max-props 2500 --seed 1

Liberec and Jablonec with the Lužická Nisa (the `liberec` map):

    .venv/bin/python tools/osm_map.py liberec --bbox 50.6689,14.9911,50.8078,15.2092 --cells 512 --title "Liberec and Jablonec" --river "Lužická Nisa" --river-width 4,6 --village 0.45 --max-props 9000

Dubrovnik in 1991 (the `dubrovnik_1991` map): the Old Town, Lapad, Gruz harbour, Ploce and Srdj above them, with
the Franjo Tudman Bridge (2002) and its approach viaducts left out, so the Rijeka Dubrovacka is crossed only far
round its head. Two bases at the west end of the city (Lapad), two at the east end (Ploce), all control points
in the city, the hostile outpost on Fort Imperial:

    .venv/bin/python tools/osm_map.py dubrovnik_1991 --center 42.649,18.099 --km 6 --cells 512 --players 4 --relief 0.8 --source overture --sea --drop-bridges 42.660,18.060,42.675,18.095 --title "Dubrovnik 1991" --max-props 1400 --village 0.5 --points 7 --cp-town 1.0 --cp-area 42.60,18.05,42.6655,18.14 --dep-area 42.60,18.05,42.6685,18.15 --starts "83,293;114,499;880,450;985,700" --outpost 42.6505,18.1135

## graft_buildings.py, fill_town.py - more buildings (and narrower streets) for a finished map

    .venv/bin/python tools/osm_map.py NAME_new --source overture --bbox S,W,N,E --cells N --starts "..." --max-props 9000 --village 1.0 --town-streets
    python3 tools/graft_buildings.py NAME NAME_new --roads      # then delete assets/maps/NAME_new
    python3 tools/fill_town.py NAME --bbox S,W,N,E --center LAT,LON --radius UNITS   # pack a town centre (needs the Overture cache of the run above)

`--town-streets` (osm_map.py) narrows the side streets in towns and cuts the residential ones where buildings stand, like
`--village` below 1 does, but keeps every village building. `graft_buildings.py` replaces the neutral buildings of an
existing map (terrain, ground, deposits, starts, points and variants stay) with those of the scratch map, nudged up to 2 cells
off deposits and kept props (pylons, towers, outpost); `--roads` takes the scratch map's roads too and removes trees and rocks
now standing on them. `fill_town.py` then adds the real buildings of a centre that still did not fit, biggest first, nudged up
to 3 cells, in dense-town kinds. Used for `jablonec` (bbox 50.661,15.091,50.751,15.233, the starts of its 8-player variant,
downtown 50.7243,15.1709 radius 60): 600 -> about 1830 buildings.

## merge_variants.py - one adaptive map from per-player-count maps

    python3 tools/merge_variants.py liberec --title "Liberec and Jablonec" liberec_4p liberec_6p liberec_8p [--attach-radius 26]

Merges maps of the same place that differ in their starts into one map with `variants` and deposits attached to starts
(see "Adaptive maps" in CLAUDE.md). Deposits within the radius of a start exist only when that start is played; deposits
only some variants have belong to that variant. The terrain's start levelling of every variant is kept.

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
