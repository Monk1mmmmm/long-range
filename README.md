# Long Range

A standalone 3D real-time strategy game in Rust (Bevy), based on the *Long Range Tank* mod:
unwieldy tanks with long guns, slow turrets and long reloads, where scouting and radar decide who gets to shoot.

- Real 3D terrain with hills, cliffs and water; the mod's models with rotating turrets and team colours
- Shots are simulated projectiles: they arc, miss moving targets, fall short and hit hills
- Flow economy (mass + energy), assisting, repair, reclaim (wrecks and standing buildings), engineering stations
- Shells leave craters; engineers level, raise and lower the ground
- Transport aircraft and APCs; capturing; scavengers that revive wrecks
- Two game modes: annihilation, and control points (every team has a pool of points; holding fewer positions than the enemy drains it)
- Three tiers of factories, engineers and extractors (upgrade in place)
- Fog of war, radar blocked by terrain, radar blips that fade, remembered enemy buildings
- Command queues, attack-move, patrol, ground fire, area reclaim, area extractor placement,
  drag a line to spread units along it
- Skirmish AI and LAN multiplayer for 2-10 players (free-for-all or teams)
- Units, weapons, factions and rules are JSON files; maps are a JSON file plus a heightmap PNG
- Map editor (`lr-editor`) with neutral and hostile pre-placed buildings

## Building

Install Rust from <https://rustup.rs> (on Windows accept the default MSVC toolchain and the Visual Studio
C++ Build Tools it offers), then in this folder:

    cargo build --release

The first build compiles the Bevy engine and takes a while; later builds are quick.
You get `target/release/longrange` and `target/release/lr-editor` (`.exe` on Windows).
`cargo run --release` builds and starts the game in one step.

**The executables need the `assets` folder next to them** (or run them through `cargo run`).
To give the game to someone else, copy the exe together with `assets/`.

A Vulkan, DirectX 12 or OpenGL capable GPU is required. **Settings** (main menu or Esc menu) lets you pick
the graphics API, shadows, anti-aliasing, v-sync and fullscreen; if the 3D view is black or broken, try
another graphics API there. Graphics errors are written to `render-errors.log` next to the game.

## LAN play

One player clicks **Host game / Skirmish vs AI**; the others see it under **LAN games** in the main menu or
type the host's IP (shown in the lobby) into **Or by IP**. Allow the game through Windows Firewall on private
networks (TCP 47800, UDP 47801). The game data (`assets/data`) is the host's: a player whose files differ
receives the host's rules and units for that game.

In the lobby the host clicks a slot to switch it between Open / AI / Closed, sets teams (same number = allies),
factions and the map: random (three sizes, 2-10 players) or any map from `assets/maps`, which brings its own
number of start positions. **Mode** switches between annihilation and control points; **Game data** picks the
built-in values or one of your presets.

## Controls

| Input | Action |
|---|---|
| Left click / drag | Select / box select (a box takes the army; engineers and the commander only when no fighting unit is inside). Double-click: all of that type on screen. Shift: add |
| Right click | Move; enemy: attack; friend: assist / repair / guard; wreck: reclaim |
| Right-drag a line | Spread the selected units along the line (Alt: attack-move) |
| Shift + order | Queue |
| A | Attack-move (click, or drag a line) |
| P | Patrol |
| G | Ground fire: click a spot, or drag a circle to shell an area |
| E | Reclaim: click a wreck or a building (yours or the enemy's), or drag a circle to collect every wreck inside |
| J | Revive (scavengers): click a wreck, or drag a circle to bring back everything inside |
| L / K | Load / unload a transport: click, or drag a circle. Right-clicking a transport boards it |
| Terraform button | Engineers: drag a circle to level it; the mouse wheel sets how far to raise or lower instead |
| S | Stop (also cancels factory queues and upgrades) |
| Tab | Pause / resume: builders and factories stop working, pumps and radars switch off and cost no energy |
| U | Upgrade (pumps and factories go up a tier) |
| R | Factory repeat on/off |
| Build menu | Click a structure, then the map. Drag for a row. Pumps: drag a circle to queue every deposit in it |
| Factory menu | Left click queues (Shift = 5), right click removes |
| Selection panel | Click a unit type to keep only that type, Shift-click to drop it |
| Ctrl+0-9 / 0-9 | Set / recall control group (double tap centres the camera) |
| . / , | Next idle builder / Commander |
| Wheel, arrows, middle-drag, minimap | Camera |
| F1 / F6 / F11 / Esc | Help / shadows / fullscreen / menu |

### FAF alternative hotkeys

Esc menu -> **Hotkeys** switches between the default keys above and a preset modelled on FAF's
"alternative" layout (saved in `settings.json`). In that preset:

| Key | Action |
|---|---|
| W / Alt+W | Move / patrol |
| R | Attack: click a unit, click the ground to shell it, drag a circle for an area |
| Q | Reclaim (drag for an area) |
| I, T | Assist / repair |
| Esc | Stop (cancels a pending command first; the menu moves to F10) |
| O / ` | Upgrade / factory repeat |
| Z X C V | Hotbuild: builders and factories / mass / power / radar and scouts |
| G S D Y M | Hotbuild: direct fire / artillery / anti-air / missiles / engineering station |

Hotbuild keys work on a selected engineer (starts placing that building) or factory (queues that unit, Shift = 5).
Pressing the same key again within a second cycles to the next unit of that class; the letter is shown on each
build button.

Both presets: J revive, L load, K unload.
Ctrl+A air, Ctrl+D land army, Ctrl+B engineers, Ctrl+X the whole army, Ctrl+C the army on screen,
Ctrl+Z all of the selected types, Ctrl+H factories on screen, Ctrl+Shift+D / A nearest land / air factory,
Shift+0-9 add a group to the selection, Ctrl+K self-destruct.

Energy is shared: when you cannot pay for everything, construction, pumps and radars all slow down together
(radars drop out below half power) - pause what you can spare to get the rest moving.

Things to know: most guns out-range their unit's sight, so they only fire at what a scout or radar reveals.
Radar does not see through hills. Enemy buildings you have seen stay marked on the map until you look at the
spot again. The bars over a unit are health (green), main gun reload (orange, blue when ready) and
construction (cyan).

## Modding

- `assets/data/rules.json` - gravity, economy constants, armor table, colours.
- `assets/data/factions/*.json` - one file per faction. Each unit has stats, `tier`, `built_by` (build
  category), `build` (what it can construct, up to which tier), `weapons` (kind `direct`, `arc`, `high_arc`,
  `missile` or `bomb`, turret speeds, aim time, spread, salvo), a `model` (a `.glb` with optional textures,
  or a list of coloured `boxes`) and an `icon` (shape + glyphs; every unit needs a unique one).
  Set `"playable": true` on a new faction file to make it selectable in the lobby.
- Presets: **Game data presets** in the main menu makes named copies of all of the above in
  `assets/data/presets/<name>/` and edits them (JSON text, checked before saving). The host chooses one in the
  lobby and the other players receive it, so nobody has to copy files around.
- Sounds are synthesised; `assets/sounds/<name>.wav` or `.ogg` replaces one (weapons: cannon, mg, artillery,
  missile, bomb, blast, death; interface: select_tank, select_vehicle, select_infantry, select_air,
  select_radar, select_building, select_engineer, select_commander, ack).
- `assets/maps/<name>/` - `map.json` (deposits, start positions, pre-placed neutral/hostile objects, water
  level) and `height.png` (16-bit greyscale). Make them with `lr-editor`.

The editor: `cargo run --release --bin lr-editor`. Raise/lower/smooth/flatten terrain, set the water level,
place deposits, start positions and buildings or units for the neutral or hostile side, then Save.
Start positions are placed with the **Start** tool (2 to 10 per map; pick the next number to add one).
`lr-editor --generate NAME CELLS SEED [PLAYERS]` writes a generated map with towns without opening a window.

## Releases

`scripts/release.sh` (Linux, also Git Bash) and `scripts/release.ps1` (Windows PowerShell) build the game and
the editor in release mode and publish them as a GitHub release with `gh`. Each platform gets two archives,
`longrange-<version>-<platform>` (the game) and `longrange-editor-<version>-<platform>` (the map editor), each
with its own copy of `assets/`:

    scripts/release.sh                 # Linux + Windows bundles, upload both to release v<Cargo version>
    scripts/release.sh --only linux    # just one platform (linux | windows)
    scripts/release.sh --no-upload     # build only; archives stay in dist/
    scripts/release.sh --tag v0.3.0 --draft

    scripts\release.ps1 -Only windows -NoUpload

On Linux the Windows bundle is cross-compiled (`rustup target add x86_64-pc-windows-gnu` plus MinGW-w64).
On Windows the Linux bundle is built through WSL if it is installed. Uploading needs a clean, pushed commit;
running again for the same tag replaces the files.

## Command line

`--skirmish` (straight into a game against three AIs), `--selftest`, `--combattest`, `--nettest`
(headless checks that print their results; on Windows run them through `cargo run --` to see the output),
`--icons FILE.png` (draws every strategic and order icon into one picture).
See `CLAUDE.md` for the architecture and the debug environment variables.
