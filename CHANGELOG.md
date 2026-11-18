# Changelog

Each `## vX.Y.Z` section is used as the text of the GitHub release with that tag
(`scripts/release.sh` / `scripts/release.ps1` pick it up automatically).

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
