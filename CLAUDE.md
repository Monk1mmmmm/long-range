# Long Range

Standalone 3D RTS in Rust/Bevy, based on the *Long Range Tank* Supreme Commander: FA mod
(`~/My Games/Gas Powered Games/Supreme Commander Forged Alliance/mods/LongRangeTank`).
Core idea: unwieldy tanks with long range, slow reload, slow aim and high damage. Units out-range their own
sight, so scouting and radar decide who gets to shoot. SupCom-style flow economy and controls, BAR/Zero-K-style
spread and area commands.

The user (monx94) and a friend make the mod together; the friend made its models and icons, so those may be
reused here. Do not copy stock SupCom names or art beyond what is in the mod folder.

## Stack

- Rust, **Bevy 0.20** (`default-features = false, features = ["3d"]`), **bevy_egui 0.43** (egui 0.36) for menus
  and all HUD painting, `serde_json` for game data, `bincode` for the network, `image` (PNG), `gltf` (models).
- Bevy 0.20 and egui 0.36 are newer than most documentation. Read the examples in
  `~/.cargo/registry/src/*/bevy-0.20.1/examples` before using an API. Things that differ from older versions:
  input events are `MessageReader<T>`, lights use `shadow_maps_enabled`, ambient light is `GlobalAmbientLight`,
  non-send resources are `insert_non_send`/`remove_non_send`, egui has `Panel` instead of `TopBottomPanel`,
  `ctx.content_rect()`, `ctx.is_pointer_over_egui()`, `rect_stroke(.., StrokeKind)`.
- glam's serde feature is not enabled; serializable vectors are our own `map::V2` / `map::V3`.
- Nested system tuples are NOT ordered by an outer `.chain()`: every inner tuple needs its own `.chain()`
  (this caused a real bug in `sim/systems.rs::register`).
- Dev machine: Linux, 2 cores / 8 GB, Intel HD 520. The user builds release binaries on Windows. A cold Bevy
  build takes very long here; never put `target/` on `/tmp` (tmpfs).
- The game needs the `assets/` folder next to the executable (`data::asset_root`, override with `LR_ASSETS`).

## Commands

```
cargo run                         # game (dev profile: our code opt 1, dependencies opt 3)
cargo run --features dev          # same, Bevy linked dynamically: much faster rebuilds (never for releases)
cargo run --release
cargo run -- --skirmish           # straight into a game vs 3 AIs
cargo run -- --selftest           # headless 4-AI match, prints economy/tier stats
cargo run -- --combattest         # headless duels: accuracy, reversing, assisted upgrade, radar line of sight
cargo run -- --nettest            # loopback netplay: 3 AIs + a predicting client, state hashes must match (LR_TICKS=n)
cargo run -- --icons out.png      # every strategic icon, order icon and cursor in one picture (look at it after icon changes)
cargo run --bin lr-editor         # map editor
cargo run --bin lr-editor -- --generate NAME CELLS SEED [PLAYERS]   # generated map with towns, trees, rocks -> assets/maps
cargo run --bin lr-editor -- --scatter NAME [SEED] [AMOUNT]   # add trees and rocks to a saved map
scripts/release.sh [--only linux|windows] [--no-upload] [--tag vX] [--draft]   # release bundles (+ gh upload)
.venv/bin/python tools/convert_assets.py "<mod folder>"   # re-import models/textures/portraits from the mod
.venv/bin/python tools/convert_assets.py --props "<maps/mining_town.v0001>"   # map buildings (MAP_PROPS)
.venv/bin/python tools/convert_assets.py --stock "<FA>/gamedata/units.scd"    # stock UEF civilian buildings (STOCK_UNITS)
python3 tools/dump_mod.py "<mod folder>"                  # print blueprint stats (source of the JSON numbers)
# The converters need numpy + Pillow: they live in the project venv (python3 -m venv .venv; .venv/bin/pip install numpy pillow).
```

Debug environment variables: `LR_SPEED=8` (fast-forward a hosted game), `LR_WATCH=1` (an AI also plays the local
slot), `LR_REVEAL=1` (no fog of war), `LR_MAP=<name>` (premade map for `--skirmish`), `LR_DUEL=1` (stages a tank
fight at the first start), `LR_PLAYERS=10` (AI count for `--selftest`), `LR_SEED=n` (another map and AI seed
for `--selftest`: one game proves little, compare a few).

### Testing the window without a desktop

Xvfb cannot present Vulkan frames. What works: a nested headless sway with Xwayland, xdotool for input and grim
for screenshots:
`WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 sway -c <cfg>`, then run the game with `DISPLAY` set to that
session's Xwayland and `WAYLAND_DISPLAY` unset; screenshot with `WAYLAND_DISPLAY=<its socket> grim out.png`.
The first xdotool mouse move lands at the window centre - do a throwaway move before the first click.
`swaymsg seat cursor ...` does not reach clients in headless mode.

### Building

Link one binary at a time (`cargo build --bin longrange`, then `--bin lr-editor`). `cargo build --bins` links
both in parallel, which filled 8 GB RAM + 8 GB swap and thrashed for half an hour without finishing. The release
scripts already build sequentially. Compiling the crate itself takes about a minute.

### Renderer compatibility (old Intel iGPUs)

`settings::render_plugin(&Settings)` / `tolerate_render_errors()` / `apply_settings` (in `src/settings.rs`, which
also holds the `Settings` resource saved to `settings.json` and the shared settings window) are used by both binaries:
Windows defaults to DX12 (the Intel Vulkan driver `igvk64.dll` crashes at start-up on HD 520/530),
`INDIRECT_FIRST_INSTANCE` is disabled so Bevy skips GPU occlusion culling and its depth-pyramid compute passes,
MSAA is off, and validation errors are logged instead of quitting (Bevy 0.20's default handler exits the app).
Do NOT zero the compute limits to force CPU preprocessing: the atmosphere plugin then fails validation at start.
Windows needs the DXC shader compiler: MSVC builds link it statically (`statically-linked-dxc`, which wgpu only
supports with MSVC - a MinGW build with that feature panics at start-up), the MinGW cross build made by
`scripts/release.sh` ships `dxcompiler.dll` next to the exe (`render_plugin` looks for it there): with FXC the 3D view stayed
black while the UI rendered. The OpenGL backend comes from a direct `wgpu` dependency with the `gles` feature.
OpenGL needs `synchronous_pipeline_compilation` (background pipeline compiles panic with "Could not lock adapter
context") and its failing SSAO shader arrives as an `Internal` error, which the handler tolerates like validation
errors; `settings::log_plugin` mutes the per-frame `wgpu_hal::gles` heuristics spam.
The Windows exe can be run straight from WSL (`dist/.../longrange.exe --skirmish`, console output is captured).
Render errors go to `render-errors.log` and the settings window. Overrides: `LR_BACKEND`, `LR_RENDERER=full`.
Wine's DX12 (vkd3d) panics inside wgpu's format query (`wgpu-hal dx12/adapter.rs`, CheckFeatureSupport) no matter
what we do; test Windows builds under Wine with `LR_BACKEND=vulkan`.
Never replace the camera `Projection` wholesale each frame: it resets `aspect_ratio` and frustum culling then
drops terrain at the screen sides (edit `fov/near/far` in place).

## Conventions

- World units = SupCom units (a tank is ~0.7 wide, ranges 15-300). Y is up. Model forward is -Z.
  Unit yaw `a` means facing `(cos a, sin a)` in the XZ plane; `view::yaw_quat` converts to a rotation.
- Build/navigation grid cell = 2 world units (`map::CELL`). Structure footprints are in cells.
- Simulation: 10 ticks/s in schedule `SimTick`, run on EVERY machine (`netplay.rs`, called from
  `main.rs::session_update`). Only commands cross the network: the host bundles each tick's commands (its own,
  the AI's, the clients') into a `Turn`; clients show a prediction `lead` ticks ahead, apply their own commands
  at once, and on every turn roll back to the last confirmed state (`sim::restore`) and re-simulate.
- The simulation must stay deterministic, on Linux and Windows alike (`--selftest` prints state hashes that
  must be equal on both; `--nettest` compares host and client):
  - query loops that allocate ids, use `SimRng`, spend a budget or queue effects iterate with
    `.sort_by_key::<&Unit, _>(|u| u.id)` - plain ECS order differs per machine and after a rollback;
  - `sin/cos/atan2/acos/atan/powf...` only through `map::fm` (libm); `sqrt`, `+ - * /`, `floor` are fine;
  - never iterate a `HashMap`, never read `Entity` values, wall-clock time or view state in a sim system;
  - anything that survives from one tick to the next must be in `sim::Snapshot` (`make_snapshot`/`restore`),
    which is also the state a client joins with and the repair sent when a client's state hash differs;
  - new simulation objects are created with `spawn_unit` / `spawn_projectile` / `spawn_wreck` (they reuse the
    entity a rolled-back object had, so models do not flicker), never with `commands.spawn`;
  - terrain heights change only through `sim::Ground::set` (craters, terraforming): it records the vertex in
    `Ground.changed` (the snapshot's `ground` list, a diff against the map the game started on), updates
    `map.blocked` around it and tells the view which chunks to rebuild. Writing `map.heights` directly
    desyncs rollback, joining and the state hash.
- The AI runs on the host only; its commands reach clients inside the turns.
- Units refer to each other by network id (`u32`), never by `Entity`.
- ECS layout: one entity per unit with `Unit`, `Body`, `Health`, `Orders`, `Intel` plus optional `Weapons`,
  `Builder`, `Factory`, `Mover`, marker/state components `UnderConstruction`, `Upgrading`,
  `Hosted` (still inside its factory), `Structure`. Projectiles and wrecks are entities too.
- Systems read other units through the per-tick `Census` resource (plain copies + spatial grid) and queue
  cross-entity effects in `Effects` (damage, reclaim) instead of holding conflicting mutable queries.
- All content lives in JSON under `assets/data`. Never hard-code unit stats or unit names in Rust; the AI finds
  units by role (extractor, generator, factory...).
- The `Defs` resource is whatever data set the current game uses: the built-in data, a preset the host chose,
  or the host's data received in `S2C::Start`. `main.rs::restore_defs` puts the built-in data back on the way
  to the main menu. `Defs.pack` (`data::DataPack`) holds the file texts it was built from.
- The JSON files under `assets/data` have CRLF line endings; keep them (the data hash is taken of the bytes).
- Owners 250/251 are the neutral and hostile sides (teams 14/15). Neutral units are never auto-targeted.
- 2-10 players (`sim::MAX_PLAYERS`). `Intel.seen/radar` are `u16` team bitmasks. A map's number of start
  positions decides the lobby slot count; `MapData::generate(cells, seed, players)` is k-fold symmetric.
- Hotkeys: two presets in `view/hud.rs` (`ViewState.preset`, saved to `settings.json` next to `assets/`):
  default, and one modelled on FAF's "alternative" keymap (reference: FAForever/fa `lua/keymap/alternativeKeyMap.lua`)
  with hotbuild keys chosen by unit role in `hud::hot_key`.
- Releases: `scripts/release.sh` / `release.ps1` make two archives per platform in `dist/` (game + assets,
  editor + assets) and publish them with `gh`. They first delete everything else in `dist/` (older versions).
  The user commits and pushes themselves; do not commit, push or publish without being asked.

## Source map

| Path | Contents |
|---|---|
| `src/data.rs` | JSON schema (`UnitDef`, `WeaponDef`, `Rules`...), `DataPack` (file texts, presets on disk, assets sent along), loading, validation, data hash |
| `src/map.rs` | `V2`/`V3`, heightmap `MapData`, generator, save/load, line of sight, raycast, A* |
| `src/sim/mod.rs` | components, resources, `Order`/`Command`, spawning, `Ground` (changed terrain), control point placement, state save/restore |
| `src/sim/systems.rs` | tick systems: commands, visibility, economy, orders (`think`), factories, build, movement, transports (`carry`), weapons, projectiles, reclaim, capture, deaths, craters/terraform (`apply_ground`), control points, victory |
| `src/ai.rs` | skirmish AI and formation assignment |
| `src/net.rs` | TCP host/client, UDP LAN discovery, message types |
| `src/netplay.rs` | `HostSim` (turn scheduling, hash check) and `ClientSim` (prediction, rollback, resync) |
| `src/view/mod.rs` | view state, unit visuals, interpolation, turret animation |
| `src/view/hud.rs` | camera, selection, orders, overlays, HUD panels (one egui pass) |
| `src/view/intel.rs` | fog grid, terrain-shadowed radar coverage, contact memory (ghosts/blips) |
| `src/view/models.rs` | GLB/box model loading, team-colour texture baking |
| `src/view/terrain.rs` | terrain chunks (rebuilt by `terrain_sync` where the ground changed), land beyond the border (`apron_mesh`: the map mirrored on all sides, blurred, hazy, built once), dashed map edge (`map_edge_sync` re-widens it with the camera distance), water, deposits, sun, minimap pixels |
| `src/view/sound.rs` | synthesised weapon and interface sounds, file overrides |
| `src/view/fx.rs` | projectile visuals and explosions |
| `src/view/flora.rs` | trees and rocks: procedural models (detail setting), one merged mesh per 16-cell chunk |
| `src/view/icons.rs` | procedural strategic icons, order icons, mouse cursors |
| `src/view/cursor.rs` | hardware mouse cursor showing the click's action (`CursorWant` set by the HUD from `hud::Frame::context` / the order mode); takes cursor control from egui |
| `src/main.rs` | menu, lobby, preset editor, host/client session loop, headless tests |
| `src/bin/lr-editor.rs` | map editor and map generation CLI |
| `build.rs`, `res/` | Windows exe icon (`res/longrange.ico`, made from the mod's `modicon.jpg`) embedded with `embed-resource`; `settings::add_window_icon` puts it on the running window (winit sets none) |

## Game rules worth knowing

- **Ballistics**: every shot is a projectile entity with 3D position/velocity under gravity
  (`rules.json: gravity`). Weapon kinds: `direct` (needs clear line of fire, leads the target), `arc`,
  `high_arc` (indirect, raise their charge to reach), `missile` (homing, limited turn rate), `bomb`.
  Hits are resolved against terrain and unit cylinders, so shots miss, fall short or hit hills.
  Guns have turret traverse speed, `aim_time` (must hold on target) and `spread`; salvos fire bursts.
- **Spotting**: weapons only engage targets their team sees or has on radar. Radar needs line of sight
  over terrain (`map.los`). Vision needs it too and is also blocked by standing trees, rocks and wrecks
  (`sim::sight_block`: per-cell blocker height, `map.los_blocked`; the end cells never block). Within 6
  units everything is seen. The fog view (`intel.rs`) uses the same blockers.
- **Contact memory** (client side, `intel.rs`): radar blips of mobile units fade over `radar_fade_secs`;
  buildings once detected stay as ghosts until the spot is seen again and found empty. Right-clicking a
  ghost issues ground fire at its position.
- **Reclaimables** (the user's word for trees, rocks and wrecks together): shells do not stop at them, but a
  shell landing on one or a blast (radius, death blasts) wears it down (`systems::hurt_reclaimables`): trees and
  rocks have `hp` in `rules.json: features` (times the map's scale; `left` drops by damage / hp), wrecks have
  `rules.wreck_hp` x the unit's hp (armor applies) and lose mass in step. At zero they are gone.
  Unfinished buildings are ordinary targets (5% hp at placement, hp grows with progress).
- **Tiers**: `tier` on a unit, `build.tier` on a builder. Factories and pumps upgrade in place (`upgrade`).
  Assignment follows the mod's blueprint TechLevel where it differs from Basic (engineers, factories, pumps,
  gunship, solar); the mod's all-T1 custom land roster was spread out: T1 Buggy/Trooper/Overtaker/Pike/MBFAG/
  radar car, T2 Crony/Dragoon/Robin/Sparky, T3 Bohdana. Change `tier` in JSON to move a unit.
- **Energy stall**: upkeep and construction share energy proportionally (`build_progress`, `Player.power`);
  pumps produce at that fraction, radars cut out below 0.5. `Paused` (Tab / `Command::Pause`) switches a
  unit's build work, production, upkeep and radar off. An earlier version subtracted upkeep first, which
  dead-locked a base whose upkeep exceeded income.
- **Engineering station** (`build.station`): immobile builder that assists anything in range by itself.
- **Movement**: land units accelerate/brake and choose between driving forward and reversing by comparing
  turn time + travel time for both (`systems.rs::locomotion`).
- **Lobby**: players on the left; on the right the map preview (`terrain::map_preview`, start numbers drawn
  on top) and the tabs Game options (mode, points, data preset, players / re-roll for random maps, "AI
  reclaims trees and rocks" -> `Ai.reclaim`) and Map (list with thumbnails). The random map is made when
  chosen (`HostLobby::ensure_map`, seed `HostLobby.seed`), so the game uses the previewed map; clients get
  the picture in `LobbyInfo.preview`.
- **Model scale**: JSON `scale` = the blueprint's `UniformScale` for stock SupCom models (factories,
  storages, pumps, air, engineers...). The mod's own models (UEX/ULX/UXA/UBX: tanks, trooper/commander,
  buggy, radar car, delivery boy, windmill, solar, heavy battery, HMG/gauss turrets) and the box-model
  APC/air transport are 2.5x that, with sizes and footprints to match; Mining Town buildings are 2.5x too,
  stock UEF civilian buildings 1x.
- **Icons**: `icon: {shape, glyphs, no_tier}` plus one vertical tick per tier under the body (SupCom style);
  the loader rejects two units with the same icon (playable factions only; map buildings share icons by kind). Shapes: land (diamond), bot, air (triangle), structure
  (square), factory (wide block), commander, civilian. Glyphs: plus = direct fire, dot = artillery,
  arc = anti-air, chevron = missile, ring = scout, wave = radar, eng (wrench; beside a second glyph it sits on the left), mass (funnel), bolt, store (tray under a
  stored resource), cargo (transports), frame, bar, land / tri (outlines of what a factory builds).
- **Reclaim**: `Order::Reclaim(id)` takes a wreck id or the id of a standing building (ids never collide);
  a building gives `wreck_mass_fraction` of its cost in step with its hit points and leaves no wreck.
  `ReclaimArea` only collects wrecks. Scavengers: `Revive(wreck)` / `ReviveArea`.
- **Trees and rocks** (map "features"): types in `rules.json: features` (class tree/rock, shape, mass,
  energy, size, weight); a map holds `feature_kinds` + `features` (saved as one line each, "kind x z yaw
  scale"). Random lobby maps, `--generate` and the editor's Scatter call `MapData::scatter_features`
  (symmetric, keeps bases/deposits/water clear). In the sim, `sim::Features` keeps the share left of each
  one (snapshot field `features` is the diff against the map); feature `i` has id `FEATURE_ID0 + i`, so
  `Reclaim(id)` and `ReclaimArea` take them (engineers only, not scavengers). A new building site
  flattens what stands on its footprint. They never block movement or shots. Reclaim work =
  mass + energy / `reclaim_energy_factor`. Idle AI engineers reclaim nearby rocks/trees when low.
  Drawing: per-chunk meshes rebuilt on change; Bevy's CPU frustum culling skips off-screen chunks
  (bounds are recomputed when a mesh changes); the fog texture's RGB carries the fog brightness for them.
  Editor: Brush tool (left paints, right removes trees or rocks), Erase (held: deposits, props, trees, rocks), Scatter, Clear all.
- **Terrain**: ground hits of `crater_min_damage` or more dig craters; `Order::Terraform {pos, radius, height}`
  moves the circle towards `height` for energy. Vertices under buildings and on the map border are locked.
  Craters are limited so they never block a cell or dig below the water (`sim::dig_crater`), which keeps
  pathfinding and the AI out of trouble. Terraforming may build walls and pits.
- **Transports** (`transport: {capacity}` in the unit JSON; `cargo` = slots a unit takes, 0 = not carriable):
  riders get the `Hosted(transport)` component, the same one units inside a factory have, so every system
  already skips them. Orders: `Load(id)` (a transport fetches that unit / a unit boards that transport),
  `LoadArea`, `Unload`. Riders die with their transport.
- **Control points** (`Game.mode == MODE_POINTS`): `Game.points` / `Game.scores` (points left per team, start `Game.cp_start`,
  picked in the lobby or `rules.cp_start_points`). A team holding fewer points than its strongest opponent loses
  `cp_bleed` per second per point of difference, equal holdings change nothing, a team at 0 is defeated. Rules `cp_*`.
- **Selection**: a box takes fighting units first, then builders (`build` present), then buildings.
- **Selection sounds**: `view::sound::select_sound` picks by role, `"select_sound"` in the unit JSON overrides.
- Bohdana was nerfed on request: 4500 mass / 40000 energy, 40 s reload, 3 s aim, 10 deg/s traverse.
- Mobile speeds are roughly 1.25x the mod's values (user asked for slightly quicker units).

## Data files

- `assets/data/rules.json` - gravity, start resources, wreck/reclaim/repair rates, radar fade, craters,
  terraforming, control points, armor table (armor -> damage type -> multiplier), colours.
- `assets/data/presets/<name>/` - a preset: its own `rules.json` and `factions/*.json`, made and edited in the
  main menu (Game data presets), chosen by the host in the lobby.
- `assets/data/factions/coalition.json` - the playable faction (the mod roster).
  `civilian.json` - neutral/hostile map buildings (box models). A new `*.json` here with `"playable": true`
  becomes selectable in the lobby.
- `assets/maps/<name>/map.json` + `height.png` (16-bit) - premade maps; `props` are pre-placed neutral
  (owner 0) or hostile (owner 1) units of any faction.
- Host and clients play with identical data: a client whose data hash differs gets the host's `DataPack`
  (and asset files the built-in data does not refer to) with `S2C::Start`.

## Asset pipeline

`tools/convert_assets.py` reads the mod's `.scm` models (`tools/scm.py`) and `.bp` blueprints (`tools/bp.py`)
and writes `assets/models/<name>.glb` (one node per bone, so turret/barrel bones named in the weapon's
`turret` block can be rotated), `<name>_albedo.png`, `<name>_team.png` (team mask from SpecTeam alpha),
`<name>_normal.png`, and `assets/icons/units/<name>.png` portraits. The mod ships no commander, radar tower or
transport model: the commander reuses the trooper mesh at 3x scale; the radar tower, the APC and the
transport aircraft are JSON box models.

## Not done yet / ideas

- No music, navy, air staging or shields. One playable faction (the format supports more).
- No friendly-fire by default (`rules.friendly_fire`).
- The hostile side only holds positions; it has no AI.
- The AI does not use scavengers, transports or terraforming. Maps cannot place their own control points.
- The minimap picture and the cached fog/radar shadows do not follow craters and terraforming; the
  minimap does not show trees and rocks.
- The preset editor is JSON text per unit; there are no per-field widgets.
- Netplay sends commands only (about 20 bytes per tick). Every client holds the full game state, so fog of war
  is not cheat-proof. No reconnect or joining a running game yet (the full-state message would allow it).
- Mispredicted effects are not undone: a muzzle flash or explosion the rollback corrected away has still been shown.
- Balance numbers are first-pass.
