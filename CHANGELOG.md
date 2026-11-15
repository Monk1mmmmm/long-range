# Changelog

Each `## vX.Y.Z` section is used as the text of the GitHub release with that tag
(`scripts/release.sh` / `scripts/release.ps1` pick it up automatically).

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
