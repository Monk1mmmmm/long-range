# Changelog

Each `## vX.Y.Z` section is used as the text of the GitHub release with that tag
(`scripts/release.sh` / `scripts/release.ps1` pick it up automatically).

## Unreleased

## v0.15.2

Rules
- Control points mode: the default starting points per team are now 8000 (were 10000); the lobby field and presets still override it.

## v0.15.1

Maps
- New map: Mostyska (Lviv oblast), made from Overture Maps data and terrain tiles for the area 49.755-49.833 N,
  23.087-23.213 E. The Sichna runs through the town as a thin river (2.5-3.5 units wide) with 17 crossings: every road
  over it keeps its bridge, and fords are added where crossings would be more than 140 units apart, so no stretch of
  the river is a long wall. 4 players, ~2000 buildings, lakes and ponds, 9 control points (6 in the towns).
- Maps no longer have to be square: a map is any number of cells wide and deep (map.json `n` and `ny`). The map
  editor's "New" row has a depth next to the width ("square" by default); generated maps that are not square are for
  2 players, Flat takes any number. The lobby and game browser pictures and the minimap show such a map centred.
  `--editor --generate NAME 256x160 SEED 2` makes one from the command line. Square maps play exactly as before.

Tools
- `tools/osm_map.py`: `--river-roads` (every road over the river is a crossing, not only those mapped as bridges) and
  `--river-ford-every D` (fords so crossings are at most D apart along the river).

## v0.15.0

Transports
- The T3 Swift is now a winged cargo plane (new model, twin turboprops) that airdrops on the go: Unload (K) over a spot or a dragged circle and it flies straight through without stopping; the cargo falls spread over the circle, each unit let go as the plane passes over its spot, and lands about 2.5 s later (helpless while falling). It scoops up units it flies close over when loading.
- A dragged Unload circle (APC, Stork or Swift) sends the infantry that come out into the free buildings in it, nearest first; those left over, or all of them where there is no building, stay spread out over the circle. A plain click unloads as before.

Units and balance
- The Crony is a tier 1 unit.
- Unit categories: anti-air, artillery and transport tags.
- AT troopers fire from inside buildings (a weapon hold no longer counts them as unsettled).

Maps
- Jablonec has about twice the buildings (Overture data; `tools/graft_buildings.py`); the lobby lists an adaptive map once, as its variant with the most starts; Tokmachka's control points were rebalanced (a home point per start, one between each pair of neighbours, one in the centre).

Interface
- Camera: pan at the screen edges, a slightly faster wheel zoom.
- Icons for every unit in the mod's style (3/4 view, white glow); F1 help wraps lines to the box so long translations fit.

## v0.14.1

Performance (late game, maps with towns)
- Working out who sees whom, the costliest part of each simulation tick on town maps, is about 40% cheaper over a game
  on Liberec and Jablonec (8 players): radar sight lines skip ground they pass high above, radar sight lines between two
  ends that stay put are kept longer, and who sees a neutral building is worked out every half second instead of every
  tick (a town building can show up to 0.4 s later). Building the per-tick unit list is about 30% cheaper.
- Remembered buildings' models are only updated when something changes, not every frame; the HUD no longer checks
  every unit against every unit inside a transport or building each frame.

## v0.14.0

Maps
- Adaptive maps (SupCom style): Frankivsk, Frankivsk mini, Liberec and Jablonec are one map each, with a way to play per player count; deposits belong to a start and are only there when that slot is played. The lobby lists each variant.
- Small village houses (khata, cottage, dacha, farmhouse...), used on real-life maps, mostly Tokmachka.

Units
- The Slavuta floats and crosses water of any depth.
- The Zaporozhets and Hetman take MediumTank armor (the APC armor class is gone).
- Scavengers: wrecks of revived units, and of killed revival sites, cannot be revived again for 60 s.

Combat
- Direct-fire units with no line of fire step to a spot with a clear shot, and recheck the lane right before firing.
- Scorch marks are a solid charred core, charcoal at up to 90% opacity.

Infantry capture
- Infantry cannot capture buildings with no function (pylons, towers); turrets and empty bunkers can be taken, and field bunkers and watch towers act like map buildings.

AI
- Idle transports lift units on long walks, parked transports count as free, and captures go for turrets and empty bunkers.

Interface
- The host can change the game speed with a slider in games of AIs only.
- Benchmark / `LR_PROFILE`: no GPU pass timing on DX12 unless `LR_BENCH_GPU=1` (the device was lost on an RX 5700 XT).

## v0.13.1

Maps
- New map Tokmachka (up to 4 players): the Mala Tokmachka - Robotyne - Verbove area of the Zaporizhzhia front, from real terrain, roads and buildings, without rivers, with neutral trench lines, sandbags and four bunkers after the real positions (hostile ones made neutral, more trenches in the north). `tools/tokmachka_works.py` adds the field works.

## v0.13.0

Controls
- Pings and map drawing: with nothing selected, right double-click pings and right-drag draws on the map, in your colour, shown to your team.
- Attack circle: infantry storm every enemy-held building in it, the rest shell the area; area ground fire now covers the circle evenly.
- Enter-building orders (click and circle) also work on buildings in the fog.
- Move orders spread out planes too.
- Land transports can be queued pre-filled: a loadout picker, and the factory builds the riders into the carrier.
- Factory build menu has tabs by unit kind (troopers, tanks, AA, radar, engineers, others).

Combat
- Machine-gun vehicles (tanks too) keep moving on attack-move, and gunships and machine guns no longer freeze for the salvo after a shot; a new order cuts a weapon hold short.
- Idle armed planes keep attacking targets around them (bombers take ground targets first).

Interface
- Lobby: slot, team and colour dropdowns, move players between slots, 10000 default points, "AI reclaims trees and rocks" is a preset switch.
- Interface scale setting for menus, HUD and editor; the replay bar sits at the bottom right of the minimap.
- Anti-tank icons use a horizontal line, the Sniper Trooper a scope; neutral building icons are drawn beneath unit icons.
- Preset editor: player colours in one wrapping row.

Sound
- The game has its own audio output on a thread that reopens the device after it fails; no more ALSA error spam on Linux.

## v0.12.9

Performance
- Late-game lag on maps with towns: working out who sees and who has whom on radar was most of each simulation tick.
  Radar sight lines between two ends that stay put (radar towers, buildings, parked units) are now worked out once
  instead of every tick (most of them went to the neutral town buildings), and every unit only looks at the viewers that
  can reach it. That part of the tick is 40-60% cheaper on the town maps (Ivano-Frankivsk City, Liberec 8 players), with
  exactly the same results.
- Bevy's fixed timestep, which nothing in the game uses, no longer runs several times a frame when the frame rate drops.

Tools
- `longrange --bench [minutes]`: a benchmark. AIs play every slot, the game runs flat out to that minute (25 by default),
  then the camera goes to the biggest crowd of units and every graphics setting is measured in turn: frame rate, frame
  time and its slowest 1%, main and render thread time, GPU time per render pass (Vulkan / DX12). The report is written to
  `bench-*.txt` in the settings folder; the settings themselves are not changed. `LR_MAP=name` benchmarks a premade map.
- `LR_PROFILE=1` in a game writes a `profile:` line to `longrange.log` every 30 seconds (frame rate, where the frame time
  goes, the costliest simulation parts); with `--selftest` it now prints the cost of every report interval as well.

## v0.12.8

The Sudzha duel map now has its river: the Sudzha runs north to south through the town and its winding course, with the
town's three road bridges (Lenin Street, 1 May Street and the northern main road) as the only crossings, so the two
bases lie on opposite banks. The other streams (Loknya, Smerditsa) stay dry: a map has one water level.

Tools
- `tools/osm_map.py --river NAME` works with the Overture source too (rivers, streams and canals as named lines, flagged
  road and rail bridges as land crossings).
- A river that leaves the map is cut at the map edge. It used to stop at the last vertex inside, which left a land strip
  round its end that units could walk round.

## v0.12.7

New map: Dubrovnik 1991 (4 players): the city as it stood before the Franjo Tudman Bridge, from Lapad in the west to
Ploce in the east with Srdj and Fort Imperial above it. Two bases at each end, every control point in the city.

Tools
- `tools/osm_map.py --source overture` makes maps from Overture Maps data when the Overpass servers are not available
  (`tools/overture_src.py`). New options: `--sea` (coasts), `--drop-bridges` / `--drop-names` (maps of the past),
  `--cp-area`, `--cp-town`, `--outpost`.

New map: Sudzha (duel): the Russian border town on the Psel with the villages around it, made from real map data
(8 km, the whole town from its west end to its eastern quarter). Bases on the west and east sides, control points in the
town and the villages. Where the open data had streets but no buildings (the north and west), small houses were added.

Tools
- `tools/osm_map.py --infill S,W,N,E` adds made-up small houses along residential streets where no real building stands.

## v0.12.6

The last box models are gone. Hit boxes of the changed buildings follow their models, so everybody in a multiplayer
game needs the same version.

Changed
- New models for the Faust SAM site (a trainable four-canister launcher with its own search radar), the radar towers
  (lattice masts with rotating antennas, one more per level), the watch tower (timber legs, ladder, planked cabin),
  sandbags, trench pieces and the field bunker (a sandbagged concrete pillbox), and for the map's refinery, fuel tank,
  power pylon, communications mast, water tower and old bunker.

## v0.12.5

New models in the style of the Long Range Tank units and the Mining Town buildings. Hit boxes changed with them, so
everybody in a multiplayer game needs the same version.

Changed
- New models in the style of the Long Range Tank units and the Mining Town buildings replace everything the game borrowed
  from stock Supreme Commander: factories (corrugated sheds and hangars with landing pads), mass extractors (oil
  pumpjacks that nod while they work), power plant, storages, AA gun, engineers, Sparky, engineering station (a tower
  crane), all aircraft, and the city buildings (panel apartment block, research institute, town hall, palace of
  culture, office tower, farm, brick works). The armoured carriers have real models instead of boxes.
- Units that were another unit's model scaled up now look their own: the T3 factories, Phalanx, Cyclone, Harrier, Dove,
  scavenger, Bastion, Hailstorm, Skylance, UC-02R/03R, AT/Sniper/SAM Trooper and Commander.
- Reactive armour is visible: bolted bricks on the Bastion and the Hetman.
- Hit boxes follow the new models (most buildings now fill their footprint).

Added
- Map editor: buildings and units can be placed for a player slot (the owner picker shows the slot colours); they
  belong to whoever plays that slot and are left out when the slot is empty.

## v0.12.4

One download per platform: the map editor is now inside the game program. The simulation is unchanged from v0.12.3.

Changed
- The map editor is part of the game program: "Map editor" in the main menu opens it in its own window
  (or `longrange --editor`; the command-line jobs are `longrange --editor --generate/--scatter/--points ...`).
  A release now has one archive per platform instead of two, with one copy of the program and of `assets`,
  about half the download and disk space. The separate `lr-editor` program is gone.

Fixed
- The Windows release build failed while packing the archive.

## v0.12.3

Infantry now take buildings by themselves when the lobby option "Infantry capture buildings" is on. The simulation changed
(only with that option on), so everybody in a multiplayer game needs the same version.

Changed
- With "Infantry capture buildings" on, no order is needed: idle infantry, and infantry on a move, attack-move or patrol
  that are not shooting, take an unguarded capturable building within 12 by themselves (houses, bunkers and trenches keep
  working as before: stormed or entered, never captured). Attack-moving infantry no longer stop to shoot down a building
  they could take. New rule `infantry_capture_reach` (12) is in the preset form.

Fixed
- A crash when infantry riding a garrison or armoured carrier died in the same moment as the building or vehicle they were
  being thrown out of.

## v0.12.2

Your own presets and downloaded maps now survive new versions, and settings are safer against damage. The simulation is unchanged.

Changed
- Game data presets you make or keep from a host's game, and premade maps downloaded from a host, are stored in the user
  folder next to `settings.json` (`presets`, `maps`), so a new version keeps them. Presets shipped with the game stay in `assets`.
- A damaged `settings.json`, or one setting of a type another version changed, no longer resets every setting: only that
  one is dropped. Settings are written through a side file so a crash cannot leave half a file.
- Release bundles contain `uninstall.sh` (Linux) / `uninstall.bat` (Windows): they delete the user folder (settings,
  replays, presets, downloaded maps) after asking and say which game folder to delete.

## v0.12.1

Replays, a tabbed settings window, settings that survive new versions, and a fix for queued buildings. The simulation changed slightly (queued buildings), so everybody in a multiplayer game needs the same version.

Added
- Replays: every game is recorded (setting "Save a replay of every game", on by default) to the `replays` folder
  next to `settings.json`. Main menu - Replays lists them (date, map, players, length, result, version); Watch plays
  one back with pause, a speed slider (x0.25 to x32), a time line to jump forwards and back, and a choice of seeing everything
  or what one player saw. Replays of another game version may play back differently (shown in the list and
  during playback).
- Double-clicking an occupied house selects every occupied building on screen.
- Lobby option "Infantry capture buildings" (off by default): infantry take over enemy and neutral buildings other than
  factories, garrisons and field works (right-click, or the Capture button), free of energy, in 20 s per man (several
  men share it), but not while an armed enemy unit stands within 15 of the building. The zone AI captures unguarded
  buildings with up to 4 infantry. Rules `infantry_capture_secs` and `infantry_capture_guard` are in the preset form.

Changed
- Settings and logs are kept in your user folder (`%APPDATA%\LongRange`, `~/.config/longrange`, `~/Library/Application
  Support/LongRange`; `LR_CONFIG` overrides it) instead of next to `assets`, so they persist across versions. An old
  `settings.json` is imported on the first start.
- The settings window is split into General, Graphics, Interface and Sound tabs.

Fixed
- Engineers skip a queued building whose site was destroyed instead of rebuilding it.

## v0.12.0

New ways to order infantry and find idle units, and many balance changes. Unit data and game rules changed, so
everybody in a multiplayer game needs the same version.

Added
- Dragging a Load circle with infantry selected sends them into every building in it: own and empty buildings
  are filled, the nearest man to the nearest building, and buildings enemy infantry hold are stormed first.
- Idle factories button (next to the idle engineers one), with a list of factory types: click selects one,
  double-click selects all.
- Setting "Infantry enter nearby buildings" (off by default): idle infantry walks into an empty or own
  garrisonable building close by.

Changed
- Weapons shoot enemy engineers (and other mobile builders) before buildings: a gun shooting a building switches
  to an engineer that comes into reach.
- The hotkey preset setting is back: the default keys are the default again, the FAF alternative keymap can be
  chosen in the settings.
- Idle engineers button: click selects one, double-click selects all.
- Land transports (APCs) take infantry only. In air transports a vehicle takes 4 slots and a trooper 1, so a
  Stork lifts 2 vehicles or 8 men.
- Terraforming is off by default (in the built-in data and the Relaxed preset); presets can switch it back on.
- AT Trooper and SAM Trooper carry 6 rounds (were 3 and 4).
- Pike Mortar shells scatter much more (spread 0.6 -> 5 degrees).
- Engineers (T1-T3, Sparky) and the Buggy are bigger, about the size of their tier's tanks; their hit boxes grew
  with them.

## v0.11.3

The skirmish AIs use buildings, transports and amphibious carriers far more, and artillery can be found by radar.
The game rules changed (a new switch), so everybody in a multiplayer game needs the same version.

Added
- Counter-battery radar (rule switch, on by default): while a mortar or howitzer shell flies inside an enemy's
  radar range, the gun that fired it shows on that enemy's radar, so long guns can fire back. Mortar spam now
  gives itself away.
- Hovering a transport or a garrison building in sight shows how many of its slots are taken.

Changed
- AI infantry that is outnumbered, hurt or under artillery fire takes shelter in the nearest building.
- AI landings: a party of infantry is flown or driven to buildings near the enemy or ahead of the attack and holds
  them. On maps with rivers the AI builds an amphibious carrier and crosses the water instead of using the bridges.
  The Rusher lands most often.
- The AI runs down enemy artillery that shells its zones, rally point or army.
- The AI no longer tries to build again and again on a spot where building failed (the engineer could not build
  there, died there, or the site was destroyed); such spots are left alone for a while.
- AI factories no longer keep producing in a full mass stall.

## v0.11.2

Transports get ferry routes, and the Pike-G Gauss Turret is retuned. Unit data changed, so everybody in a
multiplayer game needs the same version.

Added
- Ferry routes for transports (SupCom style, key F): the route is a loop with a pick-up beacon and a drop point.
  Units right-clicked onto a beacon wait there, the transports load them and carry them across, over and over.

Changed
- Units keep their queued orders through a transport ride: what was queued behind boarding is carried out after
  they are set down.
- The Gauss Turret is now the Pike-G Gauss Turret (derived from the Pike Mortar), range 90 -> 105.

## v0.11.0

The skirmish AIs (every type except Simple) play with the newer game features and fight as one army. The game
rules and unit data are unchanged, but everybody in a multiplayer game still needs the same version.

Changed
- Attacks advance in bounds: the army waits at each step until most of it has arrived, so fast units no longer
  run ahead of slow ones, and spotters with binoculars walk in front. A lone scout no longer halts the march.
- In a fight the army forms a crescent round the enemy (the wings hit side armour), and direct-fire guns
  concentrate on one target at a time, only as many as it takes to kill it quickly.
- Production answers what the enemy fields: damage against its armour classes, how little of its damage the
  unit's own armour takes, reactive armour against missile users. Infantry is built in proportion to how useful
  it is against the enemy.
- The AI garrisons buildings: near its rally point, and in the towns its attack passes through (before, it
  only looked near its base and almost never garrisoned anything).
- A field engineer digs a trench line with sandbags and a bunker at the rally point; waiting infantry uses them.
- APCs and infantry fighting vehicles ferry waiting infantry up to the attacking army.
- Infantry storms enemy-held buildings and trenches it clearly outnumbers.
- Suicide drones are sent at the most valuable enemy vehicles, as many as each kill takes.
- No extractor upgrades while the enemy army is much bigger.

## v0.10.3

Balance changes to the standard game. Not compatible with v0.10.2 in multiplayer: everybody needs this version.

Changed
- Overtaker M1 moved from T1 to T2.
- Crony M1: 530 hp (was 500), reloads in 3.5 s (4), aims in 0.8 s (1), turret turns 22 deg/s (15).
- Bastion is now a Crony with reactive armor and marginally better stats: 620 mass (1300) but 6000 energy, 680 hp,
  220 damage at range 85, the Crony's speed and turret, a slightly smaller model.
- Robin: its guided missile does 900 damage (500), enough to kill a Crony or an Overtaker head on in one hit.
- Dragoon ARV: armour-piercing autocannon.
- Faust SAM: reloads in 12 s (25).
- Hetman: close to the Zaporozhets but a little dearer, 330 mass / 4000 energy (650 / 6500), 750 hp (1000), a
  lighter twin machine gun; keeps its reactive armor.
- Pike Mortar: 300 mass / 1500 energy (220 / 1200), blast radius 4 (5).
- Power Plant: 300 mass / 2400 energy (450 / 3600), Solar Field: 800 / 6000 (1200 / 9000): T2 and T3 power now give
  clearly more energy for the mass than windmills.
- Finished sandbags and trench pieces belong to nobody: cover for every side.
- Crony and Bastion icons carry a bar (heavy tank), the Overtaker keeps the plain one.

## v0.10.1

Only the Relaxed data preset changes; the standard game is the same as v0.10.0. Everybody in a multiplayer
game still needs the same version.

Changed (Relaxed preset)
- No armour: every unit takes every damage type in full, armoured vehicles take splash like anything else, and
  where a shot hits the hull (front, side, rear) no longer matters.
- No reactive armour on the Bastion and the Hetman.
- No combat engineer: the Sparky is gone, and with it everything only it built (sandbags, trenches, the field
  bunker and the watch tower).
- Infantry can no longer enter buildings (houses, civic and industrial buildings, bunkers). Carriers and air
  transports still take them.

## v0.10.0

Not compatible with v0.9.0 in multiplayer: everybody needs this version.

New
- Three more armoured carriers alongside the Tavria, built at the land factory:
  - Tavria (T1): now a light, fast, cheap carrier for 4 units, 220 hp;
  - Slavuta (T2): amphibious carrier for 6 units - drives along the bottom of rivers and lakes up to 4 deep,
    where shells and guns cannot reach it (slower under water);
  - Zaporozhets (T2): infantry fighting vehicle for 8 units, 700 hp of heavy armour, with a roof machine gun;
  - Hetman (T3): heavy infantry fighting vehicle for 10 units, 1000 hp, reactive armor like the Bastion's
    (its first four missile hits lose three quarters of their damage) and a twin machine gun.
- Infantry riding a land carrier bail out alive (hurt) when it is destroyed.
- Unit tooltips show "Amphibious"; the preset editor has an "Amphibious" switch for land units.

Changed
- Armour shrugs off small arms: rifles and machine guns (new damage type SmallArms) do a tenth of their damage to
  every armoured vehicle. They also do less to buildings (0.6) and the commander (0.5), like machine guns did before.

## v0.9.0

Not compatible with v0.8.0 in multiplayer: everybody needs this version.

New
- T2 Dove suicide drone: a cheap flying bomb (70 mass, 40 hp) that dives into its target with a 180-damage
  warhead. Four kill an Overtaker; anti-air with radar cover shoots most of them down on the way in.
- Sandbags and trenches can be built along a line of any shape: draw it with the right mouse button.
- Right-clicking a trench with infantry selected spreads them over all of it.
- Troopers heal by themselves, back to full health in about two minutes.
- "Relaxed" data preset, for a calmer game (pick it in the lobby under the data preset). It keeps the core -
  long range, slow aim, scouting and radar decide who shoots - but drops the hidden rules and softens the rest:
  - no friendly fire; trees, rocks, wrecks and buildings no longer block sight (hills still do), and units see
    everything within 12 (was 6); troopers are seen as soon as they are in view (no spotting delay);
  - mobile units have 1.3x hp, the commander 1.6x;
  - armor angle matters less (front 0.75, rear 1.25 instead of 0.6 / 1.5);
  - unlimited missiles for the AT and SAM Troopers (no rearming); shorter set-up times: AT Trooper none, Pike
    Mortar 1.5 s, Bohdana 3 s;
  - start with 1000 mass / 6000 energy (was 650 / 4000); an energy stall slows mass much less; radar costs half
    the upkeep and blips stay 25 s (was 10);
  - falling debris hurts nobody, units thrown out of a destroyed building take less damage, and control points
    drain at half the rate.
- Hotkeys: Ctrl+R shares the selection's queued orders out among the selected units (as in FAF); `/` selects
  the nearest extractor of the lowest tier, Shift+`/` adds the next one.

Changed
- Direct-fire guns follow the shell's real arc: they aim higher at hull-down targets and hold fire instead of
  shooting into a hill, a building, a neutral unit or (with friendly fire) an own unit in the way; they then
  pick another target in the clear. Units on an attack order or attack-move close in when none of their guns
  has a clear shot. Shells also fly the arc they were laid for (they used to drop a little short).
- Splash damage: a shell does full damage only to the unit it hits. Others in the blast take less, fading to
  nothing at its edge: 40% at the centre for direct-fire guns, all of it for artillery, bombs and missiles.
  Armored units take half of any splash, death blasts included.
- Hit boxes follow the models: every unit is hit as a box of its model's shape (without gun barrels) instead of
  a padded cylinder, so low, long tanks are harder to hit from the front. Aircraft are at least 0.6 in every
  direction.
- Sandbags: 1000 hp (was 250). They now shelter from shots coming from any side, and infantry take cover
  behind anybody's sandbags.
- Bastion: 800 hp (was 1400), 12000 energy (was 6000).
- Porter interceptor: damage 22 (was 25), turn rate 80 (was 85), so Harrier fighter-bombers beat as many
  Porters; three Porters (the same mass) still beat two Harriers.

Under the hood
- `tools/blend.py` exports Blender models for the game (parts, animations, team/light/dark/metal materials,
  `--watch` re-exports on every save); see `tools/README.md`.
- Releases are built and published by GitHub Actions when a version tag is pushed (`.github/workflows/release.yml`).
- `tools/relaxed_preset.py` makes the Relaxed preset from the built-in data; run it again after the data changes.
- `tools/model_bounds.py` compares model bounds with the units' JSON size and hit box.
- Preset editor: new unit setting "Regeneration (hp/s)", rules `splash_direct` / `splash_armored`, weapon
  settings `splash` and `suicide`.

## v0.8.0

Not compatible with v0.7.1 in multiplayer: everybody needs this version. The game list does not show games of
older versions, and older versions do not see 0.8.0 games.

New
- Game browser: "Join game" lists the games on your network in a table with the map picture, players, preset,
  host and ping. A game of another version shows as such. Joining by address is on the same screen.
- After joining a game you keep the host's map and preset: the map goes to your maps, the preset to your presets
  (with a number added when you already have a different one of that name). The lobby says whether the map and
  data are already here or will be downloaded.
- Ukrainian translation. Pick the language in Settings; it switches at once. Every text in the game and the
  map editor now comes from `assets/lang/en.json` / `uk.json`, so more languages can be added.
- New units (thanks to Sadsaltan):
  - T3 Bastion heavy tank with explosive reactive armor: its first four missile hits lose three quarters of
    their damage, and a full repair replaces the tiles.
  - T2 Hailstorm (flak, aircraft only) and T3 Skylance (SAM with its own radar, aircraft only): mobile
    anti-air beyond the T1 MBFAG.
- Armor depends on the angle a shot hits from (thanks to Sadsaltan). Armored units take 0.6x damage head on,
  1x from the side and 1.5x from behind. The steeper a shell falls, the less the angle counts.
- Limited ammunition (thanks to Sadsaltan): the AT Trooper carries 3 missiles, the SAM Trooper 4. Spent rounds
  come back one a minute and cost mass (25 / 20 per round). The selection panel shows the rounds left.
- Storming (thanks to Sadsaltan): an attack order on a building held by enemy infantry storms it, like a
  right-click does. Trenches held by enemy infantry can be stormed the same way.
- Buildings block sight (new switch "Buildings block sight", on by default). Infantry inside a building still
  look out from its roof.
- Holding Shift shows the order queues of all your units.
- Right-click the ground with an occupied building selected: its infantry leave and move there.
- Mass overflow makes the mass bar blink, with a warning sound.
- Graphics setting "Units under icons": where a unit shows as its strategic icon, its model is not drawn.
  Faster with big armies. Trees and rocks now switch to a simpler model in the distance.
- Ivano-Frankivsk maps, ported from the Forged Alliance adaptive map: 8 and 10 players, and the city map for
  4 and 6 players.

Changed
- Longer tank guns: Overtaker 60 (was 45), Crony 80 (was 60), Bastion 90 (was 70). AT Trooper 95 (was 70).
- The AT Trooper sets up before firing (3 s) and holds still after each shot.
- Swift assault transport moves to T3 and carries 8, like the Stork (thanks to Sadsaltan).
- The Hornet gunship shoots infantry first (thanks to Sadsaltan).
- Renamed units: Mule APC -> Tavria, Cyclone-L -> Porter, Cyclone-H -> Cyclone, DA1 Railgun -> Dart Railgun,
  Skyspear SAM -> Faust SAM, DA3 Railgun Battery -> Phalanx Railgun Battery, M1 Heavy Battery -> Bertha
  Heavy Battery.
- The "Compatible" renderer option is gone; the full renderer is always used (thanks to Sadsaltan).

Bug fixes
- A building can no longer be placed on a queued build site.

Under the hood
- `tools/scmap_map.py` ports Supreme Commander maps (`.scmap` + `_save.lua`).

## v0.7.1

Not compatible with v0.7.0 in multiplayer: everybody needs this version.

New
- Weapon target priority. Cannons and anti-tank missiles shoot armored units (tanks and other heavy vehicles)
  first. Machine guns, rifles and riot guns shoot infantry first. If nothing of that kind is in range, they
  take the nearest enemy as before. Presets can set this per weapon ("Target priority") and give units the
  new `armored` category.
- Lobby observer slots (up to 8). Use "Watch instead" to watch, and "Take a slot" to play again. A player who
  loses their slot when the host picks a smaller map now becomes an observer instead of being kicked.
- Graphics setting "Neutral buildings: Show icons". Off, neutral map buildings show only their models at any
  zoom. A building someone garrisons still shows that player's icon.

Changed
- AT Trooper and SAM Trooper no longer have binoculars (the Trooper and Sniper Trooper keep theirs).
- Infantry hitbox is back to the size of the model (0.625 x 1.5 instead of 0.3 x 1.3), so infantry is
  easier to hit.

Bug fixes
- Shots and blasts never hurt the unit that fired them or its garrison. With friendly fire on, every tank
  shell used to hit its own tank, because the muzzle sits inside the tank's hitbox. Shots also pass
  through your own units close to the muzzle.
- AI: attacks no longer turn back at the rally point when a long-range battery covers it. Units stuck on
  attack-move far from the rally point are called back.
- AI: when an enemy is defeated, the AI now expands into that enemy's half of the map.

Under the hood
- `--combattest LR_TEST=tanks`: equal-mass duels of tanks against their counters, in the open and in woods.
- Log files are no longer tracked by git (thanks to Sadsaltan).

## v0.6.1

Bug fixes:

- Host no longer resigns when a client left (main.rs, session_update). Commands left over when no tick was due were put back into LocalCommands, which made them the host’s own. A leaver’s Resign then killed the host’s commander — most frames have no tick due. They are now held in Session.held with their player.
- Capturing engineer no longer repairs its target (systems.rs, build_progress). Builder.work is also the capture target, so a damaged enemy got repaired, and an upgrading enemy building got its upgrade paid for. Builders now only work for their own team; Assist on another team’s unit pops.
- Missiles no longer follow targets indoors. A missile with no blast radius still hit a unit that had boarded a transport or building. Missiles now lose such a target, and an Attack order on it ends.
- Fixed platform-dependent maths. MapData::generate used std sin_cos (while scatter_features already used fm for this reason), and brain.rs used std powf. Both now go through libm, so a seed gives the same map and AI choices on Linux and Windows.
- Simple AI no longer iterates in ECS order (ai.rs) while drawing from its RNG. It now goes by unit id like everything else.
- Fixed data loader gaps (data.rs). The loader now rejects:
 - a playable faction without a commander (it could never be defeated, and Resign did nothing);
 - an upgrade to a different footprint or a non-structure (the host’s occupancy grid would differ from a client’s after rollback);
 - hp or cost.time of 0 or less.

## v0.6

Not compatible with v0.5.4 in multiplayer: everybody needs this version.

New
- Five new AI types, built on ideas from the Forged Alliance AI mods M27, M28 and RNG. Click an AI slot in
  the lobby to go through them (hover it to read what the type does):
  - Zones (after M28): takes and holds the map zone by zone, keeps scouts on every zone, sends help to
    allies' zones.
  - Overseer (after M27): changes its strategy with the situation - early rush, eco and tech, air, turtle,
    all-in on a hurt commander.
  - Adaptive (after RNG): expands wide, raids extractors, builds what counters the army it has seen.
  - Rusher: many factories, cheap units, early and constant attacks.
  - Turtle: turrets, radar and long range guns; techs up and attacks late.
  The old AI is still there as "Simple".
- The new AIs play by what their team sees and has on radar (they remember buildings like your ghosts), scout,
  build radar and bring mobile radar with the army, gather before attacking and fall back from fights they
  are losing, keep longer guns out of reach of shorter ones, shell remembered buildings with artillery, raid
  with fast units, defend zones under attack, answer aircraft with anti-air, upgrade extractors and radars,
  and reclaim wrecks.
- They also use scavengers (reviving the wrecks in the zones they hold), put waiting infantry into buildings
  near their front and their base, and fly air transports: engineers to islands with free deposits, army
  units to an enemy they cannot reach by land, raiders to far-away extractors. Cut off by water, they switch
  to aircraft.
- `--skirmish` starts against three different AI types.
- Graphics setting "Remembered buildings: Show models in the fog". Off, buildings you have lost sight of
  show only their icons, no models.

Changed
- The "Map icons from zoom" slider goes much further out (camera distance 100 to 2500, steps of 100).

Performance
- The simulation tick is much faster: a 10-AI test game went from 2.2 ms to 0.8 ms per tick, and the
  slowest tick from 17 ms to 10-12 ms (sight lines and path searches now use several cores, with the same
  results on any number of threads).

Under the hood
- A unit's components are now listed once (`unit_state!`) and snapshots, rollback and joining are generated
  from that list, so a new component can no longer be forgotten in one place and desync netplay. (Thanks to
  Sadsaltan for this and the speed-up.)
- `--selftest` knows `LR_AI`, `LR_TEAMS`, `LR_POINTS`, `LR_COMP`, `LR_MOAT` and `LR_PROFILE=1` (time per
  system).

## v0.5.4

Not compatible with v0.5.3 in multiplayer: everybody needs this version.

New
- Control points are part of the map now, like the start positions. The map editor has a "Control point"
  tool (left click places one, right click removes it, "Auto place" puts them where a game would) and
  `lr-editor --points NAME` gives a saved map its points. All premade maps come with their own points;
  a map that has none still gets them placed automatically.
- The lobby map picture shows the control points in control points mode.

Changed
- The starting points of a control points game are typed in the lobby (any number) instead of picked
  from a list. Default raised from 400 to 1200.

## v0.5.3

Not compatible with v0.5.2 in multiplayer: everybody needs this version.

New
- Scavengers can assist: they help another scavenger revive or reclaim, and help build a revived wreck. On
  area reclaim and revive orders they gang up on nearby targets a teammate is already working on.
- A remembered enemy building that was destroyed while nobody on your team could see it looks dim, with a
  faded icon. Preset switch "Dim destroyed remembered buildings" (on by default).

Changed
- Engineers can no longer help build a revived wreck (only scavengers can).

## v0.5.2

Compatible with v0.5.1 in multiplayer (the host sends the map, so v0.5.1 players can join a game on the
new map).

New
- Enemy buildings you have seen keep their 3D model when they go out of sight, shown as last seen.
- New map: Liberec and Jablonec, the two towns and the land between them made from real map data, for 4
  players (512 cells). The towns' streets, the river with its bridges and culverts, and the villages
  around them.
- `tools/osm_map.py` (maps of real places): `--river`, bridges and culverts where roads cross water,
  `--village`, and better town streets.

## v0.5.1

Not compatible with v0.5.0 in multiplayer: everybody needs this version.

New
- Radar levels and stealth. Radars come in three levels and units have a stealth level: T2 aircraft
  (Harrier, Hornet, Stork, Swift) are found only by T2 radar or better, the Delivery Boy and the Cyclone-H
  only by T3 radar, and infantry never show on radar. Radar Tower Mk2 and Mk3 (build them, or upgrade a
  tower with U) and the UC-02R and UC-03R mobile radars reach further and find stealthier units, at a
  much higher running cost. The Skyspear SAM's own radar is T2. Range rings are drawn per level (blue,
  violet, orange).
- New units: Cyclone-H (T3 heavy interceptor with its own T3 radar), DA3 Railgun Battery (T3 anti-air
  battery with its own T3 radar) and Swift (T2 fast assault transport: twice the Stork's speed, four slots).
- Binoculars: troopers, AT and SAM troopers see 140 far in a narrow 20-degree cone beyond their round
  vision. The cone scans the front, turns to a radar contact in range, and locks onto an enemy and follows
  it. Selected units show their cone (brighter while locked).
- Storming buildings: infantry sent into a building held by enemy infantry fight them at the door and move
  in once it is cleared. Riflemen beat AT and SAM troopers inside; in an even fight the defenders win.
- Garrisoned infantry see 1.3x further and all round from the roof, and take 4% of every hit on their
  building.
- Ground textures: maps are painted in layers (grass, meadow, forest floor, dirt, mud, sand, snow, ice,
  stone, gravel, paving, asphalt and more) with ragged borders, plus roads. Random maps get a terrain type
  (lobby option "Terrain") and dirt tracks; generated towns get paving and asphalt roads. Explosions scorch
  the ground. The map editor has a Ground section (layers, painting, auto paint) and a Road tool.
- Host migration: when the host leaves, the game goes on. The next player takes over as host and the
  others reconnect to them. A player who leaves resigns and the others play on.
- Model animations from the mod: troopers and the commander walk, engineers unfold to build, factories and
  pumps play their upgrade, pumps work, the engineering station opens its doors, the Bohdana unpacks, the
  Stork folds its legs, the air factory platform rolls off.
- The Stork uses the mod's transport model.
- Everything the game and the editor log goes to `longrange.log` / `lr-editor.log` next to
  `settings.json` (the run before as `*-previous.log`), crashes with a backtrace: send it along with a
  crash report.

Changed
- The Pike Mortar and the Bohdana fire only when standing still, and stay put for 3 s / 5 s after each
  shot.
- The Bohdana costs 1400 mass / 12000 energy (was 4500 / 40000).
- Mass Pump Mk2 and Mk3 can be built directly by T2 / T3 engineers (upgrading still works).
- The Flak Trooper is gone.
- The AI leaves out units and buildings whose upkeep its income cannot carry.

Fixed
- Windows: Alt+Tab out of the game could stop it drawing or crash it (DirectX 12).

## v0.5.0

Not compatible with v0.4.1 in multiplayer: everybody needs this version.

New
- Debris: buildings that take a hard hit drop pieces of debris next to their walls, and a destroyed
  building spills more around its wreck. Pieces fall, hurt units they land on, can be reclaimed like wrecks
  and are worn down by shells; big ones block sight. Their mass comes out of the building's wreck, so
  nothing is gained by damaging and repairing a building.
- Settings: Effects (Low / Medium / High). Low is for slow graphics cards: debris appears where it lands
  instead of falling, explosions are smaller and give no light.
- Cover: trees, rocks and wrecks soak up part of every shot that passes through them, and wear away under
  fire. Infantry move into cover on their own, on the far side from the enemy.
- Garrisons: infantry can occupy most civilian buildings and the bunker (right-click it), fire from the
  roof and see from it. A map building belongs to whoever holds it; when it falls, the troops are thrown
  out hurt.
- New units: Flak Trooper (T1, anti-air), AT Trooper and SAM Trooper (T2), the Harrier fighter-bomber and
  the Skyspear SAM site.
- Bombers aim like in SupCom: the stick is released so it lands on the target, moving targets are led,
  and a bomber flies on and comes round for another run.
- New map: Jablonec, made from a real town (`tools/osm_map.py` makes maps of real places).
- Mouse cursors show what a click will do (move, attack, reclaim, capture, load...).
- While giving a reclaim or revive order, the HUD shows what the circle or the thing under the pointer yields.
- Lobby: option "Neutral and hostile buildings known from the start"; the lobby lists the rules a preset
  switches off.
- Game data presets: a form with a field and a tooltip for every setting (the raw JSON is still there),
  and switches to turn off terraforming, craters, wrecks and the sight and radar blocking.
- Settings: camera distance at which units turn into strategic icons.

Changed
- Scavengers also reclaim trees and rocks. Area reclaim collects wrecks, debris, trees and rocks.
- Civilian map buildings are twice as large, and towns are packed with narrow streets.
- Shells stay visible from far away (a yellow square of a size you set in the settings).
- The land beyond the map edge is a blurred mirror of the map; the map edge is a dashed line.
- A commander riding in a transport dies with it, and its side is defeated.
- Engineer icons show a wrench.
- Balance changes for bombers and anti-air.

Fixed
- Water looked uneven.
- A blank map could crash the HUD.
- Lobby and preset editor layout at small window sizes.
- Linux: OpenGL works (also under WSL with GPU support).

## v0.4.1

Not compatible with v0.4.0 in multiplayer: everybody needs this version.

New
- Trees and rocks. Random maps and all premade maps now have forests and rock fields. Engineers reclaim
  them (click one, or drag a circle): rocks give mass, trees energy. They block sight, not radar, and
  never block movement or shots. A building placed on them clears its footprint.
- Wrecks block sight as well, up to the height of the wreck.
- Pause: any player can pause the whole game (Pause key, Ctrl+P, or the menu) and anyone resumes it.
- Lobby redesigned: players on the left; the map preview with numbered start positions, a Game options tab
  and a Map tab with thumbnails on the right. A random map is made when you pick it, so the game is played
  on the map the preview shows ("New random map" rolls another). Clients see the preview too.
- Lobby: click a slot to switch it between Open, AI and Closed; the host can remove a player; option
  "AI reclaims trees and rocks".
- New neutral town buildings: houses, apartments, a church, shops, containers, a factory and more.
- Settings: detail level for trees and rocks (Low turns off their shadows).
- Map editor: Brush tool for trees and rocks (left paints, right removes), Scatter for a whole map, Erase
  works while held, Clear all. From the command line: `lr-editor --scatter NAME [SEED] [AMOUNT]`.
- Windows: the game and the editor have their own program icon (the mod's icon), also in the title bar
  and the taskbar while running.

Changed
- Control points: every team starts with a pool of points (400, or what the host picks in the lobby). A team
  holding fewer control points than its strongest opponent loses points; a team at 0 is defeated.
- The mod's own models (tanks, trooper and commander, buggy, radar car, windmill, solar, heavy battery,
  turrets...) are 2.5 times bigger, as in the mod. Some buildings take more ground because of it:
  Windmill 3x3, Solar Field 6x6, M1 Heavy Battery 3x3, HMG and Gauss Turret 2x2.
- The map editor is now a separate download (`longrange-editor-...`), each archive with its own assets.

Good to know
- Game data presets made with v0.4.0 have no tree and rock types: maps then show no trees or rocks.
  Make a new preset to get them.
- The minimap does not show trees and rocks.

## v0.4.0

Not compatible with earlier versions in multiplayer: everybody needs this version.

New
- Control points mode (lobby: Mode). Hold the marked positions with ground units to score; the first team
  to 400 wins. Destroying all enemy commanders still wins as well.
- Shells and bombs leave craters, and engineers can terraform: drag a circle to level it, or turn the mouse
  wheel to raise or lower it instead. Craters never block the way; earthworks can.
- Transports: the Mule (APC, 6 units) and the Stork (aircraft, 8 units). L loads (click a unit or drag a
  circle), K unloads over a circle, right-clicking a transport boards it.
- Engineers capture neutral and enemy units. The Scavenger reclaims wrecks or revives them as your own
  units; Revive (J) is its own button and takes a dragged circle.
- Reclaim, clicked on a standing building (yours or the enemy's), takes it apart for its mass.
- Game data presets (main menu): named copies of all unit stats and rules that you can edit in the game.
  The host picks one in the lobby and everybody else receives it. Different data files no longer keep a
  player out of a game: the host's are used.
- Sound: weapons and explosions, and units that answer when selected (tracks, engines, radar beeps...).
  Volume in the settings; `assets/sounds/<name>.wav` replaces any of them.
- Strategic icons redrawn in SupCom's manner: wide factory blocks showing what they build, a funnel for
  mass extractors, storages with their resource in a tray, tiers as ticks under the icon. Setting for
  bigger icons.
- HUD: icons for mass, energy, time and build power; an idle-engineers button; the factory queue as a row
  of icons that can be reordered by dragging; hovering an enemy shows what it is and its hit points.
- Building: Shift-drag a row at any angle, Ctrl-drag a filled block; Ctrl+Shift shows the mass lying around.
- Camera: hold Space and move the mouse to turn and tilt (tap Space to reset). The land continues beyond
  the map border instead of ending in a void.

Changed
- A selection box takes the army only: the commander, engineers and scavengers are selected only when no
  fighting unit is in the box. Ctrl+X and Ctrl+C leave them out too.
- Sight is blocked by hills, like radar. Infantry does not show on radar.
- Wrecks hold a share of the mass that actually went into the unit (half-built = less).
- Factories have fewer hit points (T3: 6000 instead of 16000). Anti-air range rings are blue.
- The player name is remembered. Overlapping strategic icons no longer flicker.

Good to know
- The AI does not use scavengers, transports or terraforming.
- The minimap picture does not show craters and earthworks.
- Linux: building with sound needs `libasound2-dev`.

## v0.3.0

Netcode
- Multiplayer now sends only commands instead of the whole game state ten times a second (about 20 bytes per
  tick instead of about 30 KB with 250 units). Every machine runs the same simulation.
- Rollback: your own orders take effect immediately on your screen. The game shows a prediction a few ticks
  ahead of the host and corrects it when the other players' orders arrive.
- If a machine ever computes a different result, the host notices within two seconds and sends it the full
  state, so games repair themselves instead of drifting apart.
- Not compatible with earlier versions in multiplayer: everybody needs this version.

Good to know
- An effect shown from a wrong prediction (a muzzle flash, an explosion) is not taken back when the game
  corrects itself.
- Every player's machine now holds the whole game state, so fog of war is not cheat-proof.
- Rejoining a running game is not possible yet.

Internals
- The simulation is deterministic across Linux and Windows (`--selftest` prints state hashes to compare,
  `--nettest` plays a host against a predicting client and checks that they agree).

## v0.2.3

Fixes
- Black 3D view (UI and minimap visible, no terrain or units) on Windows with DirectX 12: the game fell
  back to the old FXC shader compiler when `dxcompiler.dll` was not present. The DXC compiler now always
  comes with the game: built into the executables, or as `dxcompiler.dll` next to them (keep that file).
- Minimap flickering in game (its fog overlay was a texture re-uploaded twice a second).
- The whole map flickering in the editor while sculpting terrain: brushes now update only the touched
  terrain pieces in place.

New
- Settings window (main menu and Esc menu): graphics API (Automatic / DirectX 12 / Vulkan / OpenGL),
  renderer (Compatible / Full), shadows, anti-aliasing (Off / FXAA / MSAA 4x), v-sync, fullscreen and
  hotkey preset. API and renderer changes apply after a restart ("Restart now" button).
- Graphics errors are written to `render-errors.log` next to the game and shown in the settings window.
  If the 3D view is black or broken, try another graphics API and send that file.

## v0.2.2

Fixes
- Start-up crash on older Intel integrated graphics (HD 520 / HD 530 class) on Windows. The fix shipped in
  v0.2.1 did not work - that build closed right after starting - and this one replaces it:
  - Windows uses DirectX 12 instead of Vulkan (the Intel Vulkan driver crashed in `igvk64.dll`).
  - Only GPU occlusion culling is switched off, so the depth-pyramid passes these GPUs reject are never
    created. Everything else in the renderer is left as it was.
  - No MSAA (multisampled depth was the other thing the drivers rejected).
  - A render validation error is now written to the log and the game keeps running, instead of the window
    going black and closing.
- Terrain disappearing towards the left and right of the screen when zoomed out. The camera's aspect ratio
  was reset every frame, so visibility culling used a square view.
- Running the Windows build under Wine: it now picks Vulkan there, because Wine's DirectX 12 layer fails
  inside the graphics library.

Changes
- `longrange.exe` keeps a console window. It carries the log, which is what you need to send if the game
  does not start on your machine (`longrange.exe > log.txt 2>&1`).
- Graphics overrides in `settings.json` next to the `assets` folder, or as environment variables:
  `"backend"` / `LR_BACKEND` = `dx12`, `vulkan`, `gl` or `auto`; `"renderer"` / `LR_RENDERER` = `full` turns
  occlusion culling and MSAA back on for hardware that handles them.

Other
- The release scripts build the game and the editor one after the other; linking both at once ran an
  8 GB machine out of memory.

Known issues
- Not yet confirmed on a native Windows machine with Intel HD 530; it was tested on Linux and under Wine on
  an HD 520. If it still fails, please attach the console log.
- AI-only matches tend to end in a stalemate.

## v0.2.1

Note: the Intel graphics fix in this version did not work (the game still closed at start-up); it is
fixed in v0.2.2.

Fixes
- Start-up crash or black window on older Intel integrated graphics (HD 520 / HD 530 class) on Windows.
  The game and the editor now use DirectX 12 instead of Vulkan on Windows, no longer use GPU
  occlusion culling (its depth-pyramid compute passes failed on these GPUs) and render without MSAA. Set `"renderer": "full"` or `"backend": "vulkan"` in `settings.json` (or `LR_RENDERER` /
  `LR_BACKEND`) to get the previous behaviour back on hardware that handles it.
- An economy whose energy upkeep exceeded its income could no longer build anything. Construction, pumps
  and radars now share the available energy proportionally, so a stalled base slows down instead of
  locking up.
- Assisted pump and factory upgrades no longer repeat.
- Dark polygons and rectangular seams around the map edges and on water.

New
- Pause / resume (Tab): builders and factories stop working, pumps and radars switch off and stop drawing energy.
- Hotkey preset modelled on FAF's "alternative" layout, with hotbuild keys (Esc menu -> Hotkeys).
- More selection shortcuts: Ctrl+A/D/B/X/C/Z/H, Ctrl+Shift+D/A, Shift+0-9, Ctrl+K self-destruct.
- 2-10 players. Maps carry their own start positions; random maps take a player count; the editor can add
  and remove starts. New maps: `duel` (2 players) and `ring_of_ten` (10 players).
- Order buttons with icons and hotkey hints instead of text buttons.
- Mortars and howitzers pick a charge for a steep arc instead of always firing at full power.

Other
- Release scripts (`scripts/release.sh`, `scripts/release.ps1`) and this changelog.
- `tools/README.md` documents the model converter.

## v0.2.0

- Rewrite on the Bevy engine: 3D terrain, the mod's models with working turrets, simulated projectiles,
  three tiers, engineering stations, fog of war with terrain-blocked radar and remembered contacts,
  JSON-defined units and rules, map editor, LAN multiplayer.
