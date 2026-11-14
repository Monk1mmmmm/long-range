# Long Range

A standalone 3D real-time strategy game in Rust (Bevy), based on the *Long Range Tank* mod:
unwieldy tanks with long guns, slow turrets and long reloads, where scouting and radar decide who gets to shoot.

- Real 3D terrain with hills, cliffs and water; the mod's models with rotating turrets and team colours
- Shots are simulated projectiles: they arc, miss moving targets, fall short and hit hills
- Flow economy (mass + energy), assisting, repair, reclaim, engineering stations
- Three tiers of factories, engineers and extractors (upgrade in place)
- Fog of war, radar blocked by terrain, radar blips that fade, remembered enemy buildings
- Command queues, attack-move, patrol, ground fire, area reclaim, area extractor placement,
  drag a line to spread units along it
- Skirmish AI and LAN multiplayer for up to 4 players (free-for-all or teams)
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

A Vulkan/DirectX 12 capable GPU is required. F6 turns shadows off if the game is slow.

## LAN play

One player clicks **Host game / Skirmish vs AI**; the others see it under **LAN games** in the main menu or
type the host's IP (shown in the lobby) into **Or by IP**. Allow the game through Windows Firewall on private
networks (TCP 47800, UDP 47801). Everyone needs the same `assets/data` files - the game checks this on join.

In the lobby the host clicks a slot to switch it between Open / AI / Closed, sets teams (same number = allies),
factions and the map (random in three sizes, or any map from `assets/maps`).

## Controls

| Input | Action |
|---|---|
| Left click / drag | Select / box select. Double-click: all of that type on screen. Shift: add |
| Right click | Move; enemy: attack; friend: assist / repair / guard; wreck: reclaim |
| Right-drag a line | Spread the selected units along the line (Alt: attack-move) |
| Shift + order | Queue |
| A | Attack-move (click, or drag a line) |
| P | Patrol |
| G | Ground fire: click a spot, or drag a circle to shell an area |
| E | Reclaim: click a wreck, or drag a circle to collect everything inside |
| S | Stop (also cancels factory queues and upgrades) |
| U | Upgrade (pumps and factories go up a tier) |
| R | Factory repeat on/off |
| Build menu | Click a structure, then the map. Drag for a row. Pumps: drag a circle to queue every deposit in it |
| Factory menu | Left click queues (Shift = 5), right click removes |
| Selection panel | Click a unit type to keep only that type, Shift-click to drop it |
| Ctrl+0-9 / 0-9 | Set / recall control group (double tap centres the camera) |
| . / , | Next idle builder / Commander |
| Wheel, arrows, middle-drag, minimap | Camera |
| F1 / F6 / F11 / Esc | Help / shadows / fullscreen / menu |

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
- `assets/maps/<name>/` - `map.json` (deposits, start positions, pre-placed neutral/hostile objects, water
  level) and `height.png` (16-bit greyscale). Make them with `lr-editor`.

The editor: `cargo run --release --bin lr-editor`. Raise/lower/smooth/flatten terrain, set the water level,
place deposits, start positions and buildings or units for the neutral or hostile side, then Save.
`lr-editor --generate NAME CELLS SEED` writes a generated map with towns without opening a window.

## Command line

`--skirmish` (straight into a game against three AIs), `--selftest`, `--combattest`, `--nettest`
(headless checks that print their results; on Windows run them through `cargo run --` to see the output).
See `CLAUDE.md` for the architecture and the debug environment variables.
