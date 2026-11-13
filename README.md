# Long Range

A standalone real-time strategy game in Rust, based on the *Long Range Tank* mod:
long guns, fragile units, and scouting/radar that decides who gets to shoot.

- Flow economy (mass + energy income/drain, storage, stalling, assisting, reclaim, extractor upgrades)
- Command queues, attack-move, patrol, assist, factory queues with repeat and rally points
- Drag a line with the right mouse button to spread units along it; drag when placing a building to queue a row
- Fog of war with vision and radar (radar-only contacts show as blips and can be shelled)
- Skirmish AI and LAN multiplayer for up to 4 players (free-for-all or teams)
- Zoom from a single tank out to the whole map; units turn into strategic icons

## Building on Windows

1. Install Rust from <https://rustup.rs> (accept the default MSVC toolchain; it will offer to
   install the Visual Studio C++ Build Tools if they are missing).
2. In this folder run:

       cargo build --release

3. The game is `target\release\longrange.exe`. It is a single file with no extra DLLs or assets,
   so you can copy it to another PC as is.

`cargo run --release` builds and starts it in one step. Linux works the same way.

## LAN play

One player clicks **Host game / Skirmish vs AI**; the others see the game under **LAN games**
on the main menu, or type the host's IP (shown in the host's lobby) into **Or by IP**.
The first time, Windows Firewall asks whether to allow the game: allow it on private networks.
Ports: TCP 47800 (game) and UDP 47801 (discovery).

In the lobby the host clicks a slot to switch it between Open / AI / Closed, and clicks the team
button to set teams (same number = allies). Slots are the four map corners.

## Controls

| Input | Action |
|---|---|
| Left click / drag | Select unit / box select (double-click: all of that type on screen) |
| Shift + select | Add to selection |
| Right click | Move; on an enemy: attack; on a friendly: assist / repair / guard; on a wreck: reclaim |
| Right drag | Spread the selected units along the drawn line |
| Shift + order | Add to the command queue |
| A, then left click / drag | Attack-move (Alt + right click does the same) |
| P, then left click | Patrol |
| S | Stop (also cancels a factory queue or an upgrade) |
| U | Upgrade (Mass Pump to Mk2) |
| R | Toggle factory repeat |
| Build buttons | Builders: pick a structure, click to place, drag for a row, Shift to keep placing |
|  | Factories: left click queues (Shift = 5), right click removes |
| Ctrl + 0-9 / 0-9 | Set / recall control group (double tap centres the camera) |
| . / , | Next idle builder / select the Commander |
| Mouse wheel | Zoom toward the cursor |
| Arrow keys, middle drag, minimap | Move the camera |
| Esc | Cancel the current mode, or open the menu |
| F11 | Fullscreen |

## Command line

- `--skirmish` starts straight into a game against three AIs.
- `--selftest` runs a headless AI-vs-AI match and prints statistics.
- `--nettest` runs a loopback check of the network code.
- Environment variable `LR_SPEED=4` fast-forwards a hosted game.

(On Windows the release build has no console window; use `cargo run -- --selftest` to see the output.)

## Code map

| File | Contents |
|---|---|
| `src/defs.rs` | Units, weapons, armor table - edit this to rebalance |
| `src/sim.rs` | Simulation: economy, orders, movement, combat, vision (10 ticks/s) |
| `src/map.rs` | Map generator and pathfinding |
| `src/ai.rs` | Skirmish AI |
| `src/net.rs` | LAN host/client and discovery |
| `src/game.rs` | Camera, input, HUD, world drawing |
| `src/render.rs` | Box models, terrain texture, batching |
| `src/main.rs` | Menu, lobby, session loop |

The host runs the simulation and sends state snapshots; clients send commands.
