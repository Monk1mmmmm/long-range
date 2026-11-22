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
cargo run -- --selftest           # headless 4-AI match, prints economy/tier stats (LR_MAP=name: on a premade map)
cargo run -- --combattest         # headless duels: accuracy, reversing, assisted upgrade, radar line of sight
cargo run -- --nettest            # loopback netplay: 3 AIs + a predicting client, state hashes must match (LR_TICKS=n,
                                  # LR_AI=zones,.. AI types, LR_CLIENT_AI=zones an AI plays the client, LR_MAP=name, LR_PLAYERS=3)
cargo run -- --icons out.png      # every strategic icon, order icon and cursor in one picture (look at it after icon changes)
cargo run -- --ground out.png     # every ground texture in one picture, plus table vs measured average colours
cargo run -- --editor             # map editor (also the main menu's "Map editor": the game starts itself with --editor)
cargo run -- --editor --generate NAME CELLS SEED [PLAYERS] [BIOME]   # generated map with towns, roads, trees, rocks -> assets/maps (BIOME: temperate/desert/arctic/rocky)
cargo run -- --editor --scatter NAME [SEED] [AMOUNT]   # add trees and rocks to a saved map
cargo run -- --editor --points NAME   # give a saved map that has none the control points a game would place (only edits map.json)
scripts/release.sh [--only linux|windows] [--no-upload] [--tag vX] [--draft]   # release bundles (+ gh upload)
.venv/bin/python tools/convert_assets.py "<mod folder>" [--only trooper]   # re-import models/textures/portraits from the mod
python3 tools/blend.py art/NAME.blend [--watch]   # Blender model -> assets/models/NAME.glb + textures (needs Blender; tools/README.md)
.venv/bin/python tools/convert_assets.py --props "<maps/mining_town.v0001>"   # map buildings (MAP_PROPS)
.venv/bin/python tools/convert_assets.py --stock "<FA>/gamedata/units.scd"    # stock UEF civilian buildings (STOCK_UNITS)
python3 tools/dump_mod.py "<mod folder>"                  # print blueprint stats (source of the JSON numbers)
python3 tools/relaxed_preset.py                           # remake the Relaxed preset from the built-in data (after data changes)
python3 tools/model_bounds.py                             # model bounds of every unit vs its JSON size / hit_box
.venv/bin/python tools/osm_map.py NAME --bbox S,W,N,E      # map of a real place (OSM + terrain tiles) -> assets/maps/NAME, see tools/README.md
.venv/bin/python tools/scmap_map.py "<FA maps>/<map folder>" NAME [--size 512] [--variants 4,6]   # port a SupCom map (.scmap + _save.lua) -> assets/maps/NAME, NAME_<k>p
# The converters need numpy + Pillow: they live in the project venv (python3 -m venv .venv; .venv/bin/pip install numpy pillow).
```

Debug environment variables: `LR_SPEED=8` (fast-forward a hosted game), `LR_WATCH=1` (an AI also plays the local
slot, on a client too: its orders go the way a human's do), `LR_HOST=1` (host a lobby at start; it starts 8 s after a
player joined) and `LR_JOIN=<ip>` (join it at start): two windows play a multiplayer game without clicking, `LR_REVEAL=1` (no fog of war), `LR_MAP=<name>` (premade map for `--skirmish`), `LR_DUEL=1` (stages a tank
fight at the first start), `LR_PLAYERS=10` (AI count for `--selftest`), `LR_SEED=n` (another map and AI seed
for `--selftest`: one game proves little, compare a few), `LR_SURFACE_DEBUG=n` (ground shader shows one input
For `--selftest` only: `LR_AI=zones,overseer,simple,...` (the AI type of each player in turn; default one of
each), `LR_TEAMS=2` (player i plays in team i % n), `LR_POINTS=1200` (control points mode), `LR_COMP=1` (lists
what every player owns), `LR_MOAT=1` (water around the second start: tests transports and island play), `LR_INFCAP=1` (the lobby option "infantry capture buildings"), `LR_EVERY=300`
(report every that many ticks instead of 3000), `LR_AIDEBUG=1` (player 0's attacking body to stderr every 30 s: each unit's
distance to the step and to the march centre, its order, `*` = firing). For `--combattest` with `LR_TEST=tanks`: `LR_FIGHTS=bastion:1:overtaker:4;...` (own equal-mass fights instead of the built-in list), `LR_OPEN=1` (open ground only). The report prints each AI's state (`Brain::debug`): strategy, army phase, zones held.
unlit: 1 base layer, 2 scorch, 3 masks 1-3, 4 roads, 5 blended colour, 6 vertex tint), `LR_SHOT=file.png,secs,unit,dist`
(screenshot of a running game after `secs` of game time, then quit). `LR_PROFILE=1` (`--selftest` prints the time of every tick system: per tick and its slowest tick), `LR_THREADS=n` (compute threads of the headless tests; 1 runs `par_map` inline - the hashes must not change).

### Testing the window without a desktop

Xvfb cannot present Vulkan frames. What works: a nested headless sway with Xwayland, xdotool for input and grim
for screenshots:
`WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 sway -c <cfg>`, then run the game with `DISPLAY` set to that
session's Xwayland and `WAYLAND_DISPLAY` unset; screenshot with `WAYLAND_DISPLAY=<its socket> grim out.png`.
The first xdotool mouse move lands at the window centre - do a throwaway move before the first click.
`swaymsg seat cursor ...` does not reach clients in headless mode.

### Building

There is one binary, `longrange`; the map editor is part of it (`src/editor.rs`, `--editor`), so releases ship
one executable and one `assets/` per platform. (A separate `lr-editor` binary existed until v0.12.3; linking two
Bevy binaries in parallel filled 8 GB RAM + 8 GB swap here.) Compiling the crate itself takes about a minute.

### Renderer compatibility (old Intel iGPUs)

`settings::render_plugin(&Settings)` / `tolerate_render_errors()` / `apply_settings` (in `src/settings.rs`, which
also holds the `Settings` resource saved to `settings.json` in `settings::base_dir()` - the user's config folder, not next to `assets`, so it survives new versions: `%APPDATA%\LongRange`, `~/.config/longrange`, `~/Library/Application Support/LongRange`, override `LR_CONFIG`; an old `settings.json` next to `assets` is imported once; logs go there too - and the shared settings window) are used by the game and the editor:
Windows defaults to DX12 (the Intel Vulkan driver `igvk64.dll` crashes at start-up on HD 520/530),
MSAA is off, and validation errors are logged instead of quitting (Bevy 0.20's default handler exits the app).
The renderer is always Bevy's full one (GPU occlusion culling on). A "Compatible" renderer that disabled
`INDIRECT_FIRST_INSTANCE` (no occlusion culling or depth-pyramid compute passes) was removed on request;
if an old GPU fails in those passes again, that was the switch.
Do NOT zero the compute limits to force CPU preprocessing: the atmosphere plugin then fails validation at start.
Windows needs the DXC shader compiler: MSVC builds link it statically (`statically-linked-dxc`, which wgpu only
supports with MSVC - a MinGW build with that feature panics at start-up), the MinGW cross build made by
`scripts/release.sh` ships `dxcompiler.dll` next to the exe (`render_plugin` looks for it there): with FXC the 3D view stayed
black while the UI rendered. The OpenGL backend comes from a direct `wgpu` dependency with the `gles` feature.
OpenGL needs `synchronous_pipeline_compilation` (background pipeline compiles panic with "Could not lock adapter
context") and its failing SSAO shader arrives as an `Internal` error, which the handler tolerates like validation
errors; `settings::log_plugin` mutes the per-frame `wgpu_hal::gles` heuristics spam.
The Windows exe can be run straight from WSL (`dist/.../longrange.exe --skirmish`, console output is captured).
Render errors go to `render-errors.log` and the settings window. Everything logged also goes to `longrange.log` /
`lr-editor.log` (with `--editor`) next to `settings.json` (the previous run's as `*-previous.log`), panics with a backtrace:
read those first when the user reports a crash.
Alt+Tab minimises the window; Bevy then resized the swap chain to 1x1 and DX12's `ResizeBuffers` failed ("window is in
use", "Invalid surface"), losing the surface. `settings::keep_surface` (render world, before `create_surfaces`) keeps the
old size while minimised and retries configuring about once a second after a failed configure. Override: `LR_BACKEND`.
Wine's DX12 (vkd3d) panics inside wgpu's format query (`wgpu-hal dx12/adapter.rs`, CheckFeatureSupport) no matter
what we do; test Windows builds under Wine with `LR_BACKEND=vulkan`.
Linux OpenGL (`settings::linux_gl`): Bevy 0.20 makes the wgpu instance without a display handle, so EGL is
"surfaceless" and Bevy panics with "failed to choose present mode". We build the renderer ourselves
(`RenderCreation::Manual`) on our own X11 connection, force winit onto X11 (Xwayland) and repoint each window's
`RawHandleWrapper` display at that connection (`gl_window_display`). Under WSL with GPU support "auto" means
this path with Mesa's d3d12 driver (`GALLIUM_DRIVER=d3d12` and `MESA_EXTENSION_OVERRIDE=+GL_EXT_texture_shadow_lod`
are set in-process; without the extension every PBR shader fails: black 3D view). WSL's Vulkan is llvmpipe:
seconds-long frames, and WSLg's Wayland compositor then drops the window ("Broken pipe").
wgpu's OpenGL backend makes 2D textures of 6, 12... array layers cube maps: a texture array with such a layer
count samples black (the ground layer array is padded, `view::surface::albedo_image`).
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
  - parallel work only through `sim::par_map` (results in item order, applied by a sequential loop
    afterwards), and only where each result depends on its own item and on data nobody writes meanwhile
    (no `SimRng`, id allocation, budgets or effects inside). Used by `visibility` (sight lines) and
    `locomotion` (A* searches, planned in id order within `Nav.paths_left` first). Check with
    `LR_THREADS=1` and the default: `--selftest` hashes must be equal. `SimTick` runs on the single-threaded
    executor: its systems are all chained, and the multi-threaded one doubled the tick time;
  - anything that survives from one tick to the next must be in `sim::Snapshot` (`make_snapshot`/`restore`),
    which is also the state a client joins with and the repair sent when a client's state hash differs;
  - a unit component is added to the `unit_state!` list in `sim/mod.rs` (required or optional), nowhere
    else: that list makes `UnitSnap` and how snapshots read, restore and strip it (for `Limbo`). Then
    `spawn_site` does not compile until it sets the new field. `cargo test --release --lib sim::tests`
    checks the round trip; `tests::full_unit` needs the new component too;
  - new simulation objects are created with `spawn_unit` / `spawn_projectile` / `spawn_wreck` (they reuse the
    entity a rolled-back object had, so models do not flicker), never with `commands.spawn`;
  - terrain heights change only through `sim::Ground::set` (craters, terraforming): it records the vertex in
    `Ground.changed` (the snapshot's `ground` list, a diff against the map the game started on), updates
    `map.blocked` around it and tells the view which chunks to rebuild. Writing `map.heights` directly
    desyncs rollback, joining and the state hash.
- The AI runs on the host only; its commands reach clients inside the turns. Its working state (`ai::Ai`,
  `brain::Brain`) is not part of the simulation state: after a host migration the new host starts fresh AIs of
  the same types (`Player.ai_kind`) and they rebuild what they know from the world.
- **Replays** (`replay.rs`, `Settings.save_replays`, default on; `LR_NOREPLAY=1` skips recording): a replay is what a
  client joins with (map, `DataPack`, starting `Snapshot`, deflated) plus every confirmed tick's commands, written in
  chunks while the game runs (the newest `TAIL` ticks stay in memory: after host migration a client may be rewound,
  `ClientSim.rewound`) and a state fingerprint every `replay::HASH_EVERY` ticks. The host records the turns it makes,
  a client what it confirms (`ClientSim.log`, drained by the session), so every player and observer gets one.
  Files: `replays/*.lrr` next to `settings.json`, the oldest beyond `replay::KEEP` are deleted, games under 15 s
  leave none. Main menu "Replays" (`main.rs::replays_ui`) lists them; watching is `Role::Replay` (`replay_update`):
  re-simulates as `OBSERVER` with pause, a speed slider (log scale x0.25-x32, double-click = x1), time line, vision (`ViewState.replay`, `hud::replay_bar`); jumping back
  restores the nearest state kept every `HASH_EVERY` ticks. A fingerprint mismatch (another version) is shown, not
  fatal. `--nettest` writes the host's and the client's replay and plays both back.
- A player who leaves resigns (`Command::Resign`) and the game goes on. When the host leaves (connection closed, or
  silent for `HOST_SILENT_SECS`), the remaining humans take over (host migration, `main.rs::take_over`): the host
  sends `S2C::Peers` (human players + addresses) at the start and whenever someone leaves; the lowest-numbered
  remaining human becomes host from its last confirmed state (AIs restart, the old host's player resigns), the others
  reconnect with `C2S::Rejoin` and get the full state; the game waits up to `REJOIN_WAIT_SECS` for them.
- Units refer to each other by network id (`u32`), never by `Entity`.
- ECS layout: one entity per unit with `Unit`, `Body`, `Health`, `Orders`, `Intel` plus optional `Weapons`,
  `Builder`, `Factory`, `Mover`, marker/state components `UnderConstruction`, `Upgrading`,
  `Hosted` (still inside its factory), `Structure` and more: the full list is `unit_state!` in `sim/mod.rs`.
  Projectiles and wrecks are entities too. The view adds its own components (`Interp`, `Transform`, model
  children) to the same entities; they are not part of the state.
- Systems read other units through the per-tick `Census` resource (plain copies + spatial grid) and queue
  cross-entity effects in `Effects` (damage, reclaim) instead of holding conflicting mutable queries.
- **Localisation** (`src/i18n.rs`): every text a player sees is looked up by key: `tr!("lobby.watch_instead")`
  (`&str`) or `tr!("lobby.observers", a, b)` (`String`; the texts use `{}`, `{0}`, specs like `{:.1}` / `{:02}`, plural
  forms `{0|one|few|many}`). Keys in `const` tables are marked `tx!("group.key")` and shown with `tr(TABLE[i])`.
  The texts are in `assets/lang/<code>.json`: `en.json` (English, the reference) and `uk.json`, grouped by purpose in
  nested objects (the key is the path: `{"lobby": {"watch_instead": ..}}`); groups `common` (shown in several
  screens), `main_menu`, `connection`, `lobby`, `settings`, `hud_*`, `units`, `weapons`, `terrain`, `ai`, `preset_*`,
  `map_editor`. A missing text falls back to English, then to the key. Texts from the game data keep their English in
  the data: `unit_name` / `unit_desc` / `weapon_name` (and `kind_name`, `biome_name`, `ai_name`) look up
  `units.<id>.name|desc`, `weapons.<slug of the name>`... only while the data still says what `en.json` says, so a
  preset's own names show as written. `cargo test --lib i18n` fails when a key used in the code or a data text is
  missing from `en.json` (or differs from the data), when another language lacks a key or has other placeholders, or
  when a file holds unused keys: a new or changed text (also a unit name/description in the faction JSON) needs its
  `en.json` and `uk.json` lines in the same commit. Ukrainian unit nicknames are translated and in «» (type or
  designation outside: «Обганяч» M1, БРМ «Драгун», ЗРК «Небесний спис»). Never translate inside the simulation, in ids,
  or in what is sent over the network (the host sends keys, e.g. `S2C::Kicked`, the receiver translates). Language:
  `Settings.language` (empty = from `LANG`), switched live.
- All content lives in JSON under `assets/data`. Never hard-code unit stats or unit names in Rust; the AI finds
  units by role (extractor, generator, factory...).
- The `Defs` resource is whatever data set the current game uses: the built-in data, a preset the host chose,
  or the host's data received in `S2C::Start`. `main.rs::restore_defs` puts the built-in data back on the way
  to the main menu. `Defs.pack` (`data::DataPack`) holds the file texts it was built from.
- The JSON files under `assets/data` have CRLF line endings; keep them (the data hash is taken of the bytes).
- Owners 250/251 are the neutral and hostile sides (teams 14/15). Neutral units are never auto-targeted.
- 2-10 players (`sim::MAX_PLAYERS`). `Intel.seen/radar` are `u16` team bitmasks. A map's number of start
  positions decides the lobby slot count; `MapData::generate(cells, seed, players)` is k-fold symmetric.
- Hotkeys: two presets in `view/hud.rs` (`ViewState.preset`, saved to `settings.json` in the user config folder):
  default, and one modelled on FAF's "alternative" keymap (reference: FAForever/fa `lua/keymap/alternativeKeyMap.lua`)
  with hotbuild keys chosen by unit role in `hud::hot_key`.
- Releases: `scripts/release.sh` / `release.ps1` make one archive per platform in `dist/` (the game with the editor in it, + assets)
  and publish them with `gh`. They first delete everything else in `dist/` (older versions).
  `.github/workflows/release.yml` does the same on GitHub's machines (Linux + Windows MSVC) when a tag `v*` is pushed
  (or by hand from the Actions tab, "Run workflow" on main, which creates the tag): bump `Cargo.toml`, add the `## vX.Y.Z` CHANGELOG section, tag, push the tag.
  Commit every finished incremental change right away (one commit per change, short message saying what it does),
  so several sessions working in this tree at once do not pile up one big uncommitted diff. Stage only the files
  you changed (`git add <paths>`, never `git add -A` / `.`): other sessions' work may be uncommitted in the same tree.
  Do not push, tag or publish a release without being asked.
- The user wants sub-agents on weaker/cheaper models used where they help (searching and summarising code, running
  and tabulating test batches, building a baseline to compare with, self-contained pieces of work), keeping the
  design and the edits of shared files in the main session.

## Source map

| Path | Contents |
|---|---|
| `src/data.rs` | JSON schema (`UnitDef`, `WeaponDef`, `Rules`...), `DataPack` (file texts, presets on disk, assets sent along), loading, validation, data hash |
| `src/map.rs` | `V2`/`V3`, heightmap `MapData`, generator, save/load, line of sight, raycast, A* |
| `src/surface.rs` | ground texture data (view only): layer kinds (`KINDS`), layers + masks, biomes and `paint_ground`, roads (raster, keeping trees off them) |
| `src/view/surface.rs`, `surface.wesl` | the terrain material (`ExtendedMaterial` with our WESL fragment shader): procedural layer textures with mips, mask/road/scorch textures, `SurfaceView::burn`, `Tint` (average colours for minimap/preview/apron) |
| `src/sim/mod.rs` | components, resources, `Order`/`Command`, spawning, `Ground` (changed terrain), control point placement, state save/restore |
| `src/sim/systems.rs` | tick systems: commands, visibility, economy, orders (`think`), factories, build, movement, transports (`carry`), weapons, projectiles, reclaim, capture, deaths, craters/terraform (`apply_ground`), control points, victory |
| `src/ai.rs` | AI types (`AiKind`), the `Simple` AI, `ai_system` (dispatch, once a second per AI), formation assignment |
| `src/ai/brain.rs` | the zone AI behind every other type: unit roles from the JSON (`info`), per-type numbers (`Profile`), zones, enemy memory, strategy, economy (`eco`), army (`army`) |
| `src/net.rs` | TCP host/client, UDP LAN discovery, message types |
| `src/netplay.rs` | `HostSim` (turn scheduling, hash check) and `ClientSim` (prediction, rollback, resync) |
| `src/replay.rs` | replays: file format, `Recorder` (fed by `main.rs::session_update`), list/load, `Playback` (re-simulation, jumping back via kept states) |
| `src/view/mod.rs` | view state, unit visuals, interpolation, turret animation |
| `src/view/hud.rs` | camera, selection, orders, overlays, HUD panels (one egui pass) |
| `src/view/intel.rs` | fog grid, terrain-shadowed radar coverage, contact memory (ghosts/blips) |
| `src/view/models.rs` | GLB/box model loading, team-colour texture baking |
| `src/view/terrain.rs` | terrain chunks (rebuilt by `terrain_sync` where the ground changed), land beyond the border (`apron_mesh`: the map mirrored on all sides, blurred, hazy, built once), dashed map edge (`map_edge_sync` re-widens it with the camera distance), water, deposits, sun, minimap pixels |
| `src/view/sound.rs` | synthesised weapon and interface sounds, file overrides |
| `src/view/fx.rs` | projectile visuals and explosions (fewer puffs, no flash lights at the low effects level) |
| `src/view/debris.rs` | debris pieces: jagged chunk meshes, falling/tumbling, shown by the effects level |
| `src/view/flora.rs` | trees and rocks: procedural models (detail setting), one merged mesh per 16-cell chunk |
| `src/view/icons.rs` | procedural strategic icons, order icons, mouse cursors |
| `src/view/cursor.rs` | hardware mouse cursor showing the click's action (`CursorWant` set by the HUD from `hud::Frame::context` / the order mode); takes cursor control from egui |
| `src/main.rs` | menu, lobby, preset editor, host/client session loop, headless tests |
| `src/i18n.rs`, `assets/lang/` | localisation: `tr!` / `tx!` / `tr`, language files, completeness test |
| `src/preset_form.rs` | the preset editor's form view: one `Field` (JSON path, label, kind + default, tooltip) per setting of rules.json, units and weapons; edits a `serde_json::Value`, keys it does not know stay |
| `src/editor.rs` | map editor and map generation CLI (`longrange --editor`) |
| `build.rs`, `res/` | Windows exe icon (`res/longrange.ico`, made from the mod's `modicon.jpg`) embedded with `embed-resource`; `settings::add_window_icon` puts it on the running window (winit sets none) |

## Game rules worth knowing

- **Ballistics**: every shot is a projectile entity with 3D position/velocity under gravity
  (`rules.json: gravity`). Weapon kinds: `direct` (needs clear line of fire, leads the target), `arc`,
  `high_arc` (indirect, raise their charge to reach), `missile` (homing, limited turn rate), `bomb`.
  Hits are resolved against terrain and unit hit boxes, so shots miss, fall short or hit hills. A hit box
  (`hit_box: [w, h, l]` in the unit JSON, default `size`) turns with the unit and stands on its base (reaching
  `sim::HIT_BELOW` under it for slopes); the numbers are the model's bounds without gun barrels
  (`python3 tools/model_bounds.py` prints them next to the JSON; troopers, commander, watch tower and Stork were set by
  hand; aircraft have at least 0.6 in every direction, on request, so guns still hit them). Shells: `Cen::hit_seg`; blasts and missile fuzes: `Cen::hit_dist`; line-of-fire obstacles use the circle
  `Cen::hit_r` around the box.
  Guns have turret traverse speed, `aim_time` (must hold on target) and `spread`; salvos fire bursts.
- **Line of fire** (direct fire, `systems::Lane`): a gun follows the shell's real arc (with a spread allowance)
  and fires only if it reaches the target before the ground, water, a building other than the target, or an own
  (with friendly fire) or neutral unit. A hull-down target is aimed at higher up (`WeaponState.lift`); with no
  clear shot (`WeaponState.blocked`) the gun takes another enemy in the clear (not an ordered one), and units on
  Attack / attack-move whose guns are all blocked close in. `fire` adds half a tick of gravity so shells fly the
  arc they were laid for. `--combattest` "Line of fire" lines.
- **Engineers before buildings**: while an enemy engineer (any mobile builder: engineers, commander, scavengers;
  `Cen::engineer`) is in a weapon's reach, enemy buildings rank below every other enemy (`Census::pick_enemy`,
  `Lane::pick`, the bomber's line-up choice), and a gun on a building drops it every 5 ticks for one. A weapon's
  `prefer` class still comes first, an attack order still wins, and a building in the clear still beats an engineer
  with no line of fire. `--combattest` "Engineers first" line.
- **Bombers** (SupCom style, `systems.rs::bomb_drop`): a bomb weapon's reach is at least the forward
  throw (speed x fall time) + 12, whatever its JSON `range`. The stick is released so that its middle
  bomb lands on the target (moving targets are led) and every bomb gets the same small sideways push
  (`WeaponState.fix`) onto it; without an attack order a bomber drops on the target it is best lined up
  with. Locomotion keeps a bomber flying on until it has a run-in as long as its throw, then it comes
  round. A bomber must detect targets beyond its throw, hence the radar on bombers. Planes on
  attack-move go for ground targets first when they carry bombs (fighter-bomber), else aircraft first.
  Missiles lead their target; `launch_pitch` (deg) makes a SAM launch near-vertically.
- **Switches** (`rules.json`, the form's "Switches" section): `terraforming`, `craters`, `wrecks`,
  `sight_blocked_by_terrain`, `sight_blocked_by_reclaimables`, `sight_blocked_by_buildings`, `radar_blocked_by_terrain`, `counter_battery` (all default on),
  `near_sight` (6: always seen within it). The lobby lists those switched off (`Rules::switched_off`).
  A new rule needs a `Field` in `preset_form.rs` too, or it only shows in the JSON view.
- **Armor angle** (`rules.armor_front/side/rear`, 0.6 / 1.0 / 1.5): a projectile hitting an armored unit (category
  `armored`, not structures) is scaled by where it comes from against the hull's facing, blended linearly by the angle,
  and by how flat it flies (`systems::armor_angle`: the squared cosine of the descent angle, so steep mortar rounds
  count the angle little, one falling straight down not at all). Applied to `p.damage` at the hit, before the blast.
  `--combattest` infantry "Armor angle": a Crony on an Overtaker does 481 head on, 801 to the side, 1199 from behind.
- **Splash** (`rules.splash_direct` 0.4, `rules.splash_armored` 0.5, weapon `splash`): a shell with a blast radius does all
  its damage to the unit it hits directly; units it did not hit take `splash` of it at the blast centre, fading linearly to
  none at the edge. `splash` defaults to `splash_direct` for `direct` weapons, 1 for artillery, bombs and missiles.
  Armored units (not structures) take `splash_armored` of any splash, death blasts included. `--combattest` "Splash" line.
- **Spotting**: weapons only engage targets their team sees or has on radar. Radar needs line of sight
  over terrain (`map.los`).
- **Counter-battery radar** (`rules.counter_battery`): in `visibility`, an artillery shell in flight (a `PROJ_SHELL` whose
  shooter has an `arc`/`high_arc` land weapon) within a hostile team's level-1 radar range (no line of sight: shells fly
  high) sets the shooter's `radar` bit for that team that tick, so it shows as a radar contact while its shells fly and
  long guns (and the AI) can answer. No new state: worked out from the projectiles each tick. `--combattest` infantry
  "Counter-battery" line. This is the answer to mortar spam: Pikes out-range tanks (realistic: a 120 mm mortar reaches
  7 km), but every volley gives them away.
- **Binoculars** (`binoculars: {range, angle, turn, scan}` in the unit JSON; Trooper and Sniper Trooper: 140, 20 deg):
  a narrow view cone on top of the round `vision`, same line-of-sight rules. `systems::aim_binoculars` (inside
  `visibility`) keeps its direction and target in `Intel.look` / `Intel.lock` (snapshotted): it follows the
  locked enemy while it is in range and known to the team, else takes the best known mobile enemy in range
  (radar-only contacts first, ground before air, nearest), else scans to and fro +-`scan` around the body's
  facing at half the turn rate (all round from a garrison roof). Fog view: `FogGrid.cone` (`covered_cone`);
  the HUD draws the cone of selected units (brighter while locked). `--combattest` checks scan, lock and tracking
  (alone: `LR_TEST=binoculars`).
- **Spotting delay** (unit JSON `spot_time` / `spot_time_moving`, seconds; 0 = seen at once, the default: tanks,
  buildings; the three troopers: 5 / 0.5): a unit with either is only `seen` by a team once that team has had it in
  view (range + line of sight, as before) for that long. `Intel.spot[team]` (snapshotted) counts up to `sim::SPOTTED`
  in `visibility`: at the moving rate while it moves or a weapon is reloading (`Cen::firing`), at the hiding rate
  while it keeps still; out of view it drains over `rules.spot_forget` (3 s). Within `rules.near_sight` it is seen at
  once. Once spotted it stays seen while in view. `--combattest` "Spotting" line (with `LR_TEST=binoculars`).
- **Radar levels and stealth**: a unit has up to three radar ranges, `radar` (level 1), `radar2`, `radar3`, and a
  `stealth` level 0-3 (`radar_stealth: true` is the old form of 3). Radar of level L finds stealth below L, so a
  unit of stealth s shows within `UnitDef::radar_vs(s)` = the largest range of the levels above s; stealth 3
  (infantry) never shows. T2 aircraft (Harrier, Hornet, Stork) and the T3 Swift are stealth 1, the Delivery Boy 2. Radar
  towers T1 160 / T2 250+160 / T3 360+250+150 (upgrade in place, U), mobile radars UC-01R/02R/03R; build cost
  rises ~4x and upkeep ~3x per tier. The Faust SAM's own radar is level 2. Rings are drawn per level
  (blue, violet, orange); the fog shows the furthest reach. `--combattest` prints the radar-vs-stealth matrix. Vision needs it too and is also blocked by standing trees, rocks and wrecks and by finished buildings
  (`sim::sight_block` -> `map::SightBlock`: per-cell blocker height plus the building standing there; `map.sight_line`;
  the end cells never block, nor the building a viewer looks out of or the one it looks at, so garrisons see from the roof). Within 6
  units everything is seen. The fog view (`intel.rs`) uses the same blockers.
- **Contact memory** (client side, `intel.rs`): radar blips of mobile units fade over `radar_fade_secs`;
  buildings once detected stay as ghosts until the spot is seen again and found empty. A ghost of a building
  that was seen keeps its model (as last seen: type, owner, facing, construction height) out of sight
  (`view::ghost_visuals`), also after it was destroyed unseen: then, with the preset switch
  `rules.ghosts_show_destroyed` (default on), its model is drawn dark (`ModelCache::dim_mat`) and its icon faded. The graphics
  setting "Remembered buildings: Show models in the fog" (`Settings::ghost_models`, default on) turns these models off:
  out of sight only the icons remain. Right-clicking a
  ghost issues ground fire at its position.
- **Reclaimables** (the user's word for trees, rocks and wrecks together): shells do not stop at them, but a
  shell flying through one loses its `cover` share of the damage to it (cover, below), and a
  shell landing on one or a blast (radius, death blasts) wears it down (`systems::hurt_reclaimables`): trees and
  rocks have `hp` in `rules.json: features` (times the map's scale; `left` drops by damage / hp), wrecks have
  `rules.wreck_hp` x the unit's hp (armor applies) and lose mass in step. At zero they are gone.
  Unfinished buildings are ordinary targets (5% hp at placement, hp grows with progress).
- **Debris** (`rules.debris`): a single hit of at least `min_hit` of a finished building's hp, and its
  destruction, drop pieces of debris. A piece is a `Wreck` with `piece` > 0 (its size; never revived) that
  first falls (`falling`, `vel`, moved by `systems::debris_fall`; not in `WreckList` until it lands, then it
  hurts the units it lands on). Their mass comes out of the building's wreck value: the unit's `Shed`
  component (snapshot field `shed`) counts what it dropped, the wreck and reclaiming the standing building
  give that much less, so damage + repair makes no mass. Pieces have their own cap (`debris.max`), take
  shell damage like wrecks, and those at least `sight_size` big block sight (`Wreck::shape`).
  View: `Settings::effects` 0 low (pieces appear on landing, boxes without shadows, fewer explosion
  puffs, no flash lights), 1 medium, 2 high (dust where pieces land, `FX_DEBRIS`).
- **Cover** (`systems::soak_cover`, called in `projectiles` for every flight segment): each tree, rock or wreck
  a shot passes through (closest approach to its axis inside this tick's segment, so each counts once) takes
  its share of the damage: `features[].cover` (default by shape: pine 0.3, broadleaf 0.35, bush 0.15, rock
  0.65) or `rules.wreck_cover`; the shot flies on with the rest. Cover within `rules.cover_grace` of the shooter
  (`Projectile.from`) does not count, so units fire out from behind their own. Cover wears away under fire.
- **Infantry** (`"categories": ["infantry"]`, `UnitDef::is_infantry`: Trooper, AT/SAM Trooper): idle or
  engaged on attack-move/patrol, `think` sends them to `cover_spot`, behind the nearest tree/rock/wreck (at
  least 0.7 tall) on the far side from the closest known armed enemy within `cover_threat_range`, within
  `cover_search` of `Mover.anchor` (0.6x of it around themselves when fighting). Re-chosen every 5 ticks and
  kept in `Mover.cover`; a new order or Stop clears it (Stop also moves the anchor to where they stand).
  Field works are cover for every side (finished ones belong to `NEUTRAL`, `build_progress`): anybody's sandbags soak shots from anybody (`soak_works`, except within
  `cover_grace` of the shooter) and infantry hide behind any sandbags; any trench shelters whoever stands in it.
  Hovering a transport or garrison in sight shows its slots taken (`hud_map.slots_taken`).
  Right-clicking a trench piece no enemy infantry hold (`hud::Frame::trench_spots`) spreads the selected infantry
  over the whole joined trench, one per piece (`hud::spread`: evenly when fewer men than pieces, pieces our other
  men hold left to them); the rest of the selection moves to the click.
- **Garrisons**: a structure with a `transport` block (`UnitDef::is_garrison`; most civilian houses, civic and
  industrial buildings, the bunker) takes infantry only (`Order::Load(building)`, right-click a neutral or own
  building). Riders keep `Hosted`, can't be targeted, fire from the roof (`weapons` takes hosted riders of a
  garrison, origin at the building's top) and see from it (`visibility`, `intel.rs`), with vision and binocular
  range x `rules.garrison_vision` (1.3). Every hit on the building also costs each rider
  `rules.garrison_damage` (0.04) x the raw damage, their armor applied (`deaths`). `systems::garrisons`
  gives a map building to the owner of its lowest-id rider and back to `NEUTRAL` when empty (playable
  factions' buildings and `HOSTILE` ones keep their owner); garrisonable buildings can't be captured.
  `Unload` from a building sets them down at its side and walks them to the spot. Move-type orders (move,
  attack-move, patrol) given to riders of a building take them out of the side facing the goal first
  (`apply_commands`, `garrison_door`); the HUD sends a selected own building's move orders to its riders
  (`hud::Frame::garrison_riders`), so right-clicking ground with an occupied house selected empties it there. When the building dies,
  `deaths` throws them out with `rules.garrison_eject_damage` x max hp damage (none if reclaimed);
  riders of a mobile transport still die with it.
  **Storming**: `Order::Load` on a building enemy infantry hold (right-click it with infantry selected; `hud::Frame::stormers`)
  sends infantry with `assault` > 0 (unit JSON: close-quarters dps; Trooper 10, AT 3, SAM 2.5) to its door (an `Order::Attack` on such a building
  becomes that `Load` for them in `apply_commands`, `systems::storms`; other units still shoot at it), where `think`
  queues them in `Effects.assault` and `systems::assaults` trades damage: attackers' summed assault -> the lowest-id
  defender, defenders' summed assault x `rules.garrison_defense` (1.25) -> the lowest-id attacker. Defenders of a stormed
  building (`Effects.stormed`) do not fire out. When nobody is left inside, `may_board` lets them in (a map building
  whose riders all died counts as empty before `garrisons` makes it neutral); an emptied enemy bunker (playable
  faction) or `HOSTILE` building keeps its owner and cannot be entered. Riders now die inside at hp <= 0 (no wreck);
  death blasts skip hosted units. Results: `--combattest` "Storm" lines (1 trooper beats 1 AT inside, an even trooper fight goes to the defender).
  **Trenches are stormed the same way**: an attack order by such infantry on enemy infantry standing in a trench piece, or on
  the piece they hold, becomes `Load(piece)` (`systems::trench_to_storm`); `think` runs them to its edge and queues them in
  `Effects.assault`, `assaults` takes the piece's holders (`trench_holders`) as the defenders (the trench's cover does not
  soften close-quarters damage), the holders do not fire out while stormed, and once it is clear one attacker steps in.
  Before this a trench could only be shot out: 6 riflemen on attack-move lost to 4 in a trench. `--combattest` fieldworks
  "Trench fight": 6 storming riflemen take a trench 4 hold, an even fight goes to the defenders.
- **Infantry capture** (lobby option "Infantry capture buildings", `Game.infantry_capture`, off by default; Cossacks
  style): infantry may take `Order::Capture` (right-click an enemy or neutral building, or the Capture button) on any
  building `sim::seizable` (not factories, garrisons - those are stormed - or field works). At the building it queues
  `Effects.seize`; `apply_capture` adds `DT / rules.infantry_capture_secs` (20) per man, free of energy, but nothing
  while an armed finished mobile unit of the building's side is within `rules.infantry_capture_guard` (15,
  `systems::guarded`). With infantry and other units selected, the infantry capture and the rest attack. The zone AI
  sends up to 4 infantry within 40 at a known unguarded enemy building. **No order needed**: idle infantry, and infantry
  on Move, AttackMove or Patrol (not engaged), take the nearest unguarded `seizable` building within
  `rules.infantry_capture_reach` (12; `systems::seize_target`, in `think`) - they walk up to it and capture, then go on
  with the order. Garrisons (houses, bunker), factories and field works (trenches) are never taken this way, so they keep
  their own behaviour (entered, stormed, cover). `--combattest` infantry "Infantry capture" lines (ordered, idle, moving,
  attack-moving, garrison/factory refused).
- **Tiers**: `tier` on a unit, `build.tier` on a builder. Factories and pumps upgrade in place (`upgrade`).
  Assignment follows the mod's blueprint TechLevel where it differs from Basic (engineers, factories, pumps,
  gunship, solar); the mod's all-T1 custom land roster was spread out: T1 Buggy/Trooper/Pike/MBFAG/
  radar car, T2 Overtaker/Crony/Dragoon/Robin/Sparky, T3 Bohdana (the Overtaker moved from T1 on request). Change `tier` in JSON to move a unit.
  Our own additions (not in the mod; they reuse the trooper model): T2 AT Trooper (guided missile) and
  SAM Trooper (homing AA missile). Also ours: Radar Tower Mk2/Mk3 (box models), UC-02R/03R mobile radars
  (radar car mesh scaled up with box dishes), T3 Swift fast assault transport (box model, 2x Stork speed, 8 slots like the Stork; moved from T2 on request). T3 Cyclone interceptor (interceptor mesh, stealth 2, own
  T3 radar 80 so it finds stealthed aircraft) and T3 Phalanx Railgun Battery (AA turret mesh scaled up, own T3 radar 120). (A T1 Flak Trooper existed and was removed on request.)
  Mobile anti-air beyond the T1 MBFAG (requested; there is no T3 SAM trooper, only the T2 one): T2 Hailstorm
  (`flak_carrier`, Dragoon mesh, airburst flak bursts, air only) and T3 Skylance (`mobile_sam`, Robin mesh, own level-2
  radar, heavy homing missile, air only). `--combattest` air: 2 Hornets beat the MBFAG and narrowly the Hailstorm, the Skylance downs both.
  The T1 Porter interceptor was weakened on request (damage 25 -> 22, turn 85 -> 80) so that T2 Harrier fighter-bombers
  beat as many Porters (`--combattest` air: 1v1 to 4v4, all won), while 3 Porters (same mass) still beat 2 Harriers.
- **Energy stall**: upkeep and construction share energy proportionally (`build_progress`, `Player.power`);
  pumps produce at that fraction, radars cut out below 0.5. `Paused` (Tab / `Command::Pause`) switches a
  unit's build work, production, upkeep and radar off. An earlier version subtracted upkeep first, which
  dead-locked a base whose upkeep exceeded income.
- **Engineering station** (`build.station`): immobile builder that assists anything in range by itself.
- **Ammunition** (weapon `ammo`, rounds; 0 = unlimited): `WeaponState.fired` counts rounds missing (snapshotted);
  a weapon whose rounds are spent stops firing (AT Trooper 3, SAM Trooper 4). A salvo is cut to the rounds left.
  Rearming, SupCom tactical missile style (`rearm` s per round, `rearm_mass` per round; troopers 60 s, 25 / 20 mass):
  one round at a time (`WeaponState.rearm` progress), paid as a mass flow at the top of `weapons` in id order, slowed
  by a mass stall, stopped while paused. The HUD shows rounds left; `--combattest` infantry has an "Ammunition" line.
- **Hold** (weapon `hold`, seconds; Pike Mortar 3, Bohdana 5, AT Trooper 3): the weapon fires only while its unit has no movement goal
  and is stopped, its `aim_time` counts only from then (set-up time; AT Trooper 3 s), and each shot sets `Mover.hold`, during which `locomotion` keeps the land/hover unit in place
  (a move order given meanwhile starts when the hold runs out).
- **Reactive armor** (`reactive: {charges, block}` in the unit JSON; T3 Bastion: 4, 0.75): a missile that hits the
  unit directly (`projectiles`, `PROJ_MISSILE`) uses up a charge and loses `block` of its damage, with a small blast.
  Charges left are the `Reactive` component (in `unit_state!`); `build_progress` puts them all back when a repair
  brings the unit to full health. `--combattest` infantry "Reactive armor" line. The Bastion (requested T3 tank against
  AT infantry) is a Crony with reactive armor and marginally better stats (Crony mesh at 1.1x, 680 hp vs 530, 220 damage
  at 85 vs 200 at 80, same speed and turret), about the Crony's mass (620 vs 560) but 4x its energy (6000), MediumTank armor.
  The Robin's 900-damage missile kills a Crony or Overtaker head on in one hit; the Bastion's charges stop that.
- **Suicide drones** (weapon `suicide: true`, with `ammo: 1`; the T2 Dove): when the weapon fires, the unit itself is
  launched at the target as a homing missile (a straight dive from where it flies, `solve`/`fire`) and removed
  quietly (`Effects.reclaimed`: no wreck, no death blast). The Dove: 70 mass, 40 hp, speed 5.5, altitude 6, a 180
  Explosive / radius 4 warhead, range 14; the AI treats it as a bomber. `--combattest` air "Doves" lines: 4 kill an
  Overtaker unopposed; with a radar tower an MBFAG downs 4 of 6, a T1 AA turret all 6.
- **Movement**: land units accelerate/brake and choose between driving forward and reversing by comparing
  turn time + travel time for both (`systems.rs::locomotion`).
- **Game browser** (main menu "Join game" -> `Screen::Browse`, `main.rs::browser_ui`): a table of the games found by UDP
  discovery (`net::Discovery`, broadcast + localhost once a second, Refresh/F5 asks again, hosts silent 4 s drop off).
  A host answers with its version and a `net::GameAd` (host name, map folder or random size, 48x48 RGB picture + starts,
  players/AIs/slots/observers, mode, preset, data hash, started), set by `Host::advertise` whenever the lobby is dirty
  (`HostLobby::ad`); another version shows as such and cannot be joined. Joining by address is on the same screen.
  **Downloads**: the host's map and data already travel in `S2C::Start`; the client then keeps them
  (`keep_downloads`, background thread): a premade map goes to `assets/maps/<folder>` (or `<folder>_2`... when a
  different map has that name, `same_map`), a named preset to the presets (`name 2`... likewise), model/icon files via
  `store_assets` as before. The client lobby says whether the map and data are here or will be downloaded
  (`LobbyInfo.map_file/preset/data_hash`).
- **Lobby**: players on the left, observers under them (up to 8: "Watch instead" / "Take a slot", `C2S::Observe`; joiners
  find no open slot and players whose slot a smaller map removes become observers). An observer plays as `sim::OBSERVER`
  (no player: sees everything, gives no commands, not in `S2C::Peers`, so never takes over as host); on the right the map preview (`terrain::map_preview`, start numbers drawn
  on top) and the tabs Game options (mode, points, data preset, players / re-roll for random maps, "AI
  reclaims trees and rocks" -> `Ai.reclaim`, "Neutral and hostile buildings known from the start" -> `Game.reveal_props`: `intel.rs` puts
  them into the contact memory once as remembered (stale) buildings, view only) and Map (list with thumbnails). The random map is made when
  chosen (`HostLobby::ensure_map`, seed `HostLobby.seed`), so the game uses the previewed map; clients get
  the picture in `LobbyInfo.preview`.
- **Model scale**: JSON `scale` = the blueprint's `UniformScale` for stock SupCom models (factories,
  storages, pumps, air...). The mod's own models (UEX/ULX/UXA/UBX: tanks, trooper/commander,
  buggy, radar car, delivery boy, windmill, solar, heavy battery, HMG/gauss turrets) and the box-model
  APC are 2.5x that, with sizes and footprints to match. Engineers (T1 x1.6, T2 x1.5, Sparky x2.5, T3 x1.25
  over the blueprint) and the buggy (a further x1.75) were enlarged on request to about the length of their
  tier's tanks (Pike / Robin / Bohdana), with `size` and `hit_box` scaled along. Civilian map buildings were doubled
  on request (Mining Town 5x, stock UEF civilian 2x; box models, sizes and footprints too: a house takes 2x2
  cells). Towns are dense: one free cell (a street ~1.5 tanks wide) between footprints, never touching
  (footprints block movement) - the editor's `add_settlements` packs lots nearest the town centre, `osm_map.py`
  places with `margin=1`.
- **Icons**: `icon: {shape, glyphs, no_tier}` plus one vertical tick per tier under the body (SupCom style);
  the loader rejects two units with the same icon (playable factions only; map buildings share icons by kind). Shapes: land (diamond), bot, air (triangle), structure
  (square), factory (wide block), commander, civilian. Glyphs: plus = direct fire, dot = artillery,
  arc = anti-air, chevron = missile, ring = scout, wave = radar, eng (wrench; beside a second glyph it sits on the left), mass (funnel), bolt, store (tray under a
  stored resource), cargo (transports), frame, bar, land / tri (outlines of what a factory builds).
- **Reclaim**: `Order::Reclaim(id)` takes a wreck id or the id of a standing building (ids never collide);
  a building gives `wreck_mass_fraction` of its cost in step with its hit points and leaves no wreck.
  `ReclaimArea` collects wrecks, debris, trees and rocks, not buildings. Scavengers reclaim
  every reclaimable and revive whole wrecks (`Revive(wreck)` / `ReviveArea`; debris cannot come back). A revived wreck
  becomes a construction site with the `Revival(wreck id)` component (snapshot `revival`, `Cen::reviving`): only
  scavengers build it (`apply_commands`, `think`'s Assist, stations and `build_progress` keep engineers off it).
  Scavengers take `Assist`: on a revival site, or on a scavenger/engineer to revive or reclaim what it works on
  (any assister reclaims the tree/rock/wreck the assisted unit reclaims). On `ReclaimArea` / `ReviveArea` a target
  the side already works on (or an own unfinished revival in the circle) counts `systems::TEAMWORK` (12) nearer,
  so a group works together; a `Revive` whose wreck a teammate already revived helps build that site. While a
  reclaim or revive order is being given, the HUD shows what the circle (or the thing under the pointer) yields or costs.
- **Trees and rocks** (map "features"): types in `rules.json: features` (class tree/rock, shape, mass,
  energy, size, weight); a map holds `feature_kinds` + `features` (saved as one line each, "kind x z yaw
  scale"). Random lobby maps, `--generate` and the editor's Scatter call `MapData::scatter_features`
  (symmetric, keeps bases/deposits/water clear). In the sim, `sim::Features` keeps the share left of each
  one (snapshot field `features` is the diff against the map); feature `i` has id `FEATURE_ID0 + i`, so
  `Reclaim(id)` and `ReclaimArea` take them (engineers and scavengers). A new building site
  flattens what stands on its footprint. They never block movement or shots. Reclaim work =
  mass + energy / `reclaim_energy_factor`. Idle AI engineers reclaim nearby rocks/trees when low.
  Drawing: per-chunk meshes rebuilt on change; Bevy's CPU frustum culling skips off-screen chunks
  (bounds are recomputed when a mesh changes); the fog texture's RGB carries the fog brightness for them.
  LOD: at medium/high detail each chunk has a second, low-detail mesh; `VisibilityRange` crossfades to it at
  `flora::LOD_FADE` camera distance from the chunk centre (chunk meshes are built around that centre).
  Editor: Brush tool (left paints, right removes trees or rocks), Erase (held: deposits, props, trees, rocks), Scatter, Clear all.
- **Ground textures** (`surface.rs`, purely visual, never in the sim or its hash): SupCom-style strata. `MapData.layers`
  (kind + tile size; at most 9), layer 0 covers the map, each further layer is drawn over the ones below by its mask
  (`MapData.masks`, one byte per cell per layer, saved as `layerN.png`). The shader blends with the textures' height
  (alpha), a broad noise wobble and a smooth (quintic) mask step, so borders are ragged, not cell-shaped; it also
  takes a little relief from the height. Textures: `KINDS` (grass, meadow, forest floor, dirt, mud, sand, desert sand,
  cracked earth, snow, ice, stone, gravel, paving, asphalt, dirt track), made in code (`view::surface::texel`) unless
  `assets/textures/terrain/<kind>.png` exists (alpha or brightness = height). Keep big blotches out of a texture:
  they show its tiling. `KINDS.color` must match the texture's average (`--ground` prints both): the minimap, lobby
  picture and the land beyond the border use it. Maps saved without layers are painted at load
  (`paint_ground(Temperate)`); random maps get a biome (lobby "Terrain": random from the seed, or chosen) and dirt
  tracks along the lanes. **Roads** (`MapData.roads`: kind, width, points; map.json lines "kind width x z x z ...",
  at most 4 kinds per map) are a distance-like raster (`road_raster`, edge at 0.5) so edges stay crisp; trees and
  rocks are kept off them (`scatter_features`, `clear_features_on_roads`). **Scorch**: blasts and deaths near the
  ground darken a view-side map (`SurfaceView::burn` from `fx::spawn_fx`, also unseen ones), permanent, lost when the
  view restarts. Editor: Ground section (layer list, kind, tile size, Auto paint by biome; Paint tool: left adds the
  selected layer, right removes it, on layer 0 it clears all layers above; Road tool: left adds points, right finishes
  or deletes the road under the pointer). `--generate` towns get paving/gravel around the buildings and asphalt roads.
- **Water levels**: `MapData.water` is the sea; lakes above it are `MapData.lakes` (levels, at most 255) +
  `lake_of` (one byte per cell: 0 = sea, k = `lakes[k - 1]`; map.json `lakes` + `lakes.png`). A lake covers the
  cells with ground below its level (`flood_lake`; the editor's Lake tool: left fills the hollow under the pointer
  to the "lake depth" slider, right drains; `osm_map.py` makes them from OSM lakes). Never read `map.water` for
  a position: use `water_at` / `water_xz` / `water_cell`, `water_vertex` (craters), `surface(p)` (ground or water
  top: hover, markers), `water_near` (looks only). Levels are fixed during a game: a pit dug beside a lake stays
  dry. View: `terrain::lake_mesh` (one mesh, all lake cells), the sea plane as before.
- **Terrain**: ground hits of `crater_min_damage` or more dig craters; `Order::Terraform {pos, radius, height}`
  moves the circle towards `height` for energy. Vertices under buildings and on the map border are locked.
  Craters are limited so they never block a cell or dig below the water (`sim::dig_crater`), which keeps
  pathfinding and the AI out of trouble. Terraforming may build walls and pits.
- **Transports** (`transport: {capacity}` in the unit JSON; `cargo` = slots a unit takes, 0 = not carriable):
  land transports (APCs) take infantry only; air transports take any land unit, but a vehicle (engineers, commander,
  tanks, cars: `cargo` 4 in the JSON) takes 4 slots where a trooper takes 1, so a Stork (8) lifts 2 vehicles or 8 men
  (`sim::fits(host, rider)` decides who may get in, `carriable` only whether a unit can ride at all);
  riders get the `Hosted(transport)` component, the same one units inside a factory have, so every system
  already skips them. Orders: `Load(id)` (a transport fetches that unit / a unit boards that transport),
  `LoadArea`, `Unload`. Riders die with their transport (a commander inside: its side is defeated), except infantry in
  a land transport (the Tavria APC): `deaths` throws them out like a garrison's (`garrison_eject_damage`).
  Riders keep their orders: what is queued behind boarding is carried out after they are set down (`carry` only
  drops a `Load` of their own transport and a leading `Ferry`).
  **Ferry** (SupCom, `Order::Ferry { pos, pick }`, HUD key F): a mobile transport's route is a loop of `Ferry` orders
  rotated like a patrol: the `pick` one is the beacon (where the transports stood when ordered), the one before it on
  the loop the drop point (set down there with an inserted `Unload`), any between are waypoints (Shift adds points
  after the drop point, `apply_commands` inserts them before the beacon). Other units given `Ferry` (right-click a
  beacon, or F + click) wait at the beacon (`Cen::ferry_wait`); transports there load them nearest first and leave
  when full or when nobody who fits is waiting within `FERRY_PICK` or still coming within `FERRY_COMING`. A waiting
  unit gives up when no own transport has that beacon (`Cen::beacon`; `apply_commands` refreshes both fields after
  new orders). `--combattest` "Ferry" line: 6 tanks across in 3 trips, then on to their queued goal.
- **Small arms** (damage type `SmallArms`: the Trooper and Sniper rifles and every machine gun): every armored class
  takes 0.1 of it (`LightTank`, `MediumTank`, `APC`); the other classes take what they take from `Normal` (Light 1,
  Structure 0.6, Commander 0.5, Air 1, Civilian 0.3). Requested: armour shrugs off rifles and machine guns.
- **Armoured carriers** (requested; Tavria and Slavuta are ZAZ cars, `built_by` land): T1 Tavria (`apc`: 220 hp
  LightTank, fast 5.8, 4 slots, unarmed), T2 Slavuta (`apc_amph`: 300 hp LightTank, 5.2, 6 slots, amphibious, unarmed),
  T2 Zaporozhets (`apc_heavy`, an infantry fighting vehicle - БМП: 700 hp, armor class `APC` - Normal 0.1, Autocannon 0.5, AP and Explosive 1.0, SmallArms
  0.1 -, slow 3.4, 8 slots, roof machine gun ~11 dps), T3 Hetman (`apc_heavy3`, heavy БМП, its "evolution" as a separate T3 unit -
  mobile units do not upgrade in place: close to the Zaporozhets but a little dearer (330 mass / 4000 energy), 750 hp APC,
  reactive armor 4 x 0.75 like the Bastion, twin machine gun ~15 dps). The zone AI uses them (`Role::Carrier`) to ferry infantry to its attacking army. `move.amphibious: true` (land units only): it also drives along the
  bottom of water up to `map::AMPHIBIOUS_DEPTH` (4) deep, at `AMPHIBIOUS_SPEED` (0.6x). `map.blocked` marks such
  cells `map::OCC_DEEP` (other land units treat them as blocked; `passable_for`, `find_path(.., amph, ..)`);
  submerged, shells stop at the surface and guns have no line of fire. `--combattest` carrier and "River" lines
  (alone: `LR_TEST=apc`).
- **Control points** (`Game.mode == MODE_POINTS`): their positions come from the map (`MapData.points`, map.json
  `points`, placed with the editor's "Control point" tool like starts; `MapData::generate` and the premade maps have them);
  a map without any gets `MapData::auto_points` (`MapData::control_points` picks). `Game.points` / `Game.scores` (points
  left per team, start `Game.cp_start`: typed in the lobby, else `rules.cp_start_points`, default 1200). A team holding fewer points than its strongest opponent loses
  `cp_bleed` per second per point of difference, equal holdings change nothing, a team at 0 is defeated. Rules `cp_*`.
- **Model LOD** (setting `model_lod`, default on): unit and ghost models are hidden wherever the HUD draws the
  strategic icon instead (`view::as_icon`: icon zoom, or under `SPECK_PX` on screen; `view::model_lod`). Neutral
  buildings keep their models while `neutral_icons` is off. Wrecks have no icons and always keep theirs.
- **Selection**: a box takes fighting units first, then builders (`build` present), then buildings.
- **Selection sounds**: `view::sound::select_sound` picks by role, `"select_sound"` in the unit JSON overrides.
- Bohdana was nerfed on request: 40 s reload, 3 s aim, 10 deg/s traverse, 5 s hold; its cost was later cut
  from 4500 mass / 40000 energy to 1400 / 12000 (build time 4000).
- Mobile speeds are roughly 1.25x the mod's values (user asked for slightly quicker units).

## Data files

- `assets/data/rules.json` - gravity, start resources, wreck/reclaim/repair rates, radar fade, craters,
  terraforming, control points, armor table (armor -> damage type -> multiplier), colours.
- `assets/data/presets/<name>/` - a preset shipped with the game (the player's own presets, and those kept from a host's game, are in `presets/<name>/` and premade maps kept from a host's game in `maps/<name>/` of the user config folder, `data::user_presets_dir` / `user_maps_dir` / `map_dir`: they survive new versions, and a user preset hides a shipped one of the same name; the map editor still saves to `assets/maps`) - a preset: its own `rules.json` and `factions/*.json`, made and edited in the
  main menu (Game data presets: form view with tooltips, or raw JSON), chosen by the host in the lobby.
- `assets/data/factions/coalition.json` - the playable faction (the mod roster).
  `civilian.json` - neutral/hostile map buildings (box models). A new `*.json` here with `"playable": true`
  becomes selectable in the lobby.
- `assets/maps/<name>/map.json` + `height.png` (16-bit) + `layerN.png` (8-bit ground layer masks) + `lakes.png` (8-bit, lake number per cell, only with lakes) - premade maps; `props` are pre-placed neutral
  (owner 0), hostile (owner 1) or player (owner 2+k = whoever plays lobby slot k; left out when that slot is empty) units of any faction; `points` are the control points.
- Host and clients play with identical data: a client whose data hash differs gets the host's `DataPack`
  (and asset files the built-in data does not refer to) with `S2C::Start`.

## Asset pipeline

`tools/convert_assets.py` reads the mod's `.scm` models (`tools/scm.py`) and `.bp` blueprints (`tools/bp.py`)
and writes `assets/models/<name>.glb` (one node per bone, so turret/barrel bones named in the weapon's
`turret` block can be rotated), `<name>_albedo.png`, `<name>_team.png` (team mask from SpecTeam alpha),
`<name>_normal.png`, and `assets/icons/units/<name>.png` portraits. The blueprint's `.sca` animations (`tools/sca.py`,
table `ANIMS`) become named glTF animations, times divided by the blueprint's rate; only bones that move get tracks.
`models::AnimKind` (one per name, in precedence order) says what plays each, `view::drive_anims` drives the unit's
`Animator` from sim state, `AnimPart::pose` picks the first playing animation per part, `animate_parts` layers
turret aim on top: `walk` (trooper; cycles with ground speed at the mod's rate at full speed, slower where feet would
slide - the commander - backwards when reversing), `upgrade` (factories, pumps: follows `Upgrading` progress),
`work` (pumps: loops at the owner's `power`, frozen when paused), `open` (engineers: folded = first frame when idle,
unfold while `Builder.work`), `doors` (eng station: open to the middle while working, then on to the end), `unpack`
(Bohdana: target and standing), `land` (Stork: legs out when hovering still, folded when flying - it never lands),
`water` (engineers, Sparky: ground under them below the water level), `rolloff` (air factory platform: out, hold,
back when `Factory.building` changes; SupCom only does it for land units). `models::tests` loads every GLB.
Wrecks (`spawn_model` with a wreck material) get no animated, aimed or spinning parts: rest pose, still.
Models made in Blender go through `tools/blend.py` (`blend_export.py` runs inside Blender): objects are parts,
actions named like the `AnimKind`s are animations, materials named team/light/dark/metal take that flat colour
(`models::prim_mat`; other materials keep their colour when the unit has no albedo texture), an object with several
materials becomes one part per primitive. `ModelDef.brightness` multiplies the albedo (2.1 default for the mod's dark
textures, 1 for ours). The mod ships no commander or radar tower model: the commander reuses the trooper mesh at 3x scale; the radar tower
and the APC are JSON box models. The Stork uses the mod's hooked stock UEF transport (UEA0104, blueprint scale).

## Not done yet / ideas

- No music, navy, air staging or shields. One playable faction (the format supports more).
- No friendly-fire by default (`rules.friendly_fire`).
- **AI types** (`ai::AiKind`, picked per lobby slot; ideas taken from the FA AI mods M27, M28 and RNG, no code):
  Simple (the old one), Overseer (M27: switches strategy - rush, main, eco and tech, air, turtle, commander
  kill - from the balance of forces), Zones (M28: zone by zone, scouts everything, defends allies), Adaptive
  (RNG: wide expansion, raiders, production counters what the enemy fields), Rusher, Turtle. All but Simple
  share `brain.rs` and differ by `Profile`. How the engine works:
  - the map is cut into zones (starts, control points, deposit groups, filler); each keeps both sides'
    strength, its worth, when it was last seen. Armies gather at the rally zone (our safe zone nearest the
    enemy), attack the zone with the best worth / distance / defence that the force at the rally can take
    (`pick_target`), travel zone by zone as one body with reinforcements following in groups, retreat when
    outmatched (`Phase`). Waiting makes impatient (`nerve`), a stand-off without progress ends in a push or in
    leaving the zone alone (`avoid`), so do three fall-backs in a row from the same zone (`lost`), and after a
    fall-back the next attack waits 20 s (`calm`). The local fight (`enemy_near`) counts turrets only within the
    radius looked at, not every battery whose range reaches (a heavy battery covers half the map: attacks used to
    turn back at the rally point for hours). Units far from the rally standing still without firing get a plain
    Move back (attack-move would wait for ever at a target they have no line of fire to); units shelling something
    far off do not hold the next attack up;
  - zone `front` and `fwd` are worked out from the enemies still alive (`set_front`), so a defeated player's half
    of the map becomes ground to expand into;
  - it only knows what its team sees or has on radar plus `Brain::memory` (buildings until destroyed, units for
    30 s); an unscouted enemy start is assumed to hold a base with an army that grows with time (`Zone::guess`);
  - mass discipline: factories and upgrades only add spending while `spend_m` stays below a multiple of the
    income (a stall slows extractors too); planned generators count (`coming_e`), or every engineer starts one;
  - engineers that only assist or reclaim are free for real jobs (an Assist order never ends by itself);
  - the attack moves in bounds (`Brain::step`, `BOUND`): the body's formation stands round the step, and the
    next step is taken once 60% of the marching units (those not firing) are there, or after 30 s from where
    they are; so fast units wait for slow ones. Spotters (binoculars/vision well beyond their own guns) walk
    16 ahead (a muster point for zone defence was tried and made duels against Simple slower: removed). The march centre is the plain mean of the marching non-artillery units (weighted by worth, one
    idle Bohdana far behind dragged it back). It counts as a fight only when known enemies within the reach of
    most of the body's guns (75th percentile, 30-90) add up to 10% of its strength or one is within 45: a lone
    scout no longer stops the march. In a fight the body forms a crescent round the enemy's centre (every unit at
    0.8 of its range, sorted by side so nobody crosses, up to ~90 deg: the wings hit side armor; direct-fire
    tanks drive there with Move, infantry and set-up weapons stop at the first target), and direct-fire guns in
    reach focus fire (`Order::Attack`) greedily on the target they kill fastest for its worth, only as many
    guns as kill it in ~3 s;
  - production counters what the enemy fields (`Brain::counter`, from `mix`/`emix`/`missiles`, decaying
    memories of the enemy's mobile mass by armor class, its damage by type, its missile share): a type's weight
    scales with its damage against those armor classes (rules armor table), inversely with the enemy damage its
    own armor takes, and up with reactive armor while the enemy uses missiles. `Profile.infantry` is the share
    of the army that is infantry; one field engineer (`Info.works`: builds field works) digs `Profile.works`
    trench pieces across the rally point (+ sandbags at the ends, a bunker behind); idle infantry there drops into
    them by itself (`cover_spot` prefers trenches). Land carriers (`Role::Carrier`, armed or not, one per 6 infantry,
    at most 3) ferry infantry waiting at the rally point to the attacking body. The infantry share is scaled by
    how well the infantry types answer the enemy compared with the vehicles (0.2-1.5x). Direct guns up to range 110 are tanks (Bastion, Robin, AT trooper), not artillery. No extractor
    upgrades while the enemy army is far bigger than ours;
  - storming: infantry with `assault` attacks (= storms) an enemy-held building or an enemy infantry's trench
    piece within 45 when its summed assault beats the holders' x `garrison_defense` x 1.5; suicide drones get
    `Attack` on the dearest known enemy vehicles near the army or our zones, as many as kill each;
  - micro: longer guns back off from shorter ones, indirect artillery ground-fires remembered buildings, mobile radar
    follows the army;
  - scavengers (`Profile.scav`, built while whole wrecks lie in safe zones): `ReviveArea` on the dearest
    wreck, then `ReclaimArea` for what cannot come back;
  - garrisons (`Profile.garrison`): infantry waiting near the rally point (70) or the start (50) walks into
    buildings there (`Order::Load`), the body's infantry into buildings within 45 of the step it passes (towns
    stand between the bases: only looking near the rally point, the AI never garrisoned anything on most maps);
    buildings neither near the rally point nor the attack are emptied (`Unload` on the building);
  - air transports: zones carry the number of their land mass (`Zone::comp`). Jobs of an idle transport, in
    order: an engineer to a land mass with free deposits (it then builds extractors there only), army units
    from the rally point to an enemy that cannot be reached by land (they fight on their own over there,
    "abroad"), raiders to a raid zone far away. Passengers get `Load(transport)` and are left alone while in
    `Brain::boarding`. An AI cut off by water builds more air, and techs its air factory for a transport.
  - shelter: infantry outnumbered 1.5:1 near it, hurt with enemies about, or in reach of known enemy artillery walks
    into the nearest garrisonable building within 25 (`Brain::held` keeps it there at least 60 s; a building is never
    emptied while enemies or enemy guns are near it);
  - landings (`Profile.landing`, Rusher 1.0, Adaptive 0.7, Zones 0.6, Overseer 0.4 / air 0.8, Turtle 0.15): a party of
    infantry (`Brain::party`, as many as the transports lift, up to ~12) waits at the rally point; idle air transports or
    land carriers (amphibious ones count zones reachable through shallow water, `Zone::acomp/areach`) take it to a group of
    neutral or own buildings near the enemy or ahead of the attack (scored by enemy buildings near, front, the attack
    target, a river crossing for amphibious carriers; rejected when known enemies or anti-air there are too strong).
    Riders get `[Load(carrier), Load(house)]`: set down beside the town they walk in. One at a time, every 2-5 min.
    `Brain::wet` (water between the starts) makes the first carrier an amphibious one (Slavuta);
  - artillery hunts: known enemy indirect-fire units reaching our zones, the rally point or the march get a zone-defence
    detachment (up to 40% of the free force), nearest units first;
  - build spots (`ai::Tries`, also used by Simple): a build order whose engineer could not build at the spot, died there,
    did not get there within 3 min, or whose site was destroyed unfinished bans the spot for 90 s, doubling per further
    failure (`find_spot_where`); the engineer is called off;
  - `LR_AIOFF=landing,tries,...` (also infantry, works, carrier, crescent, focus, bounds, outgunned) switches parts off for tests.
  The report line ends with `landings N (air/amphibious/land) shelters N hunts N`. Premade maps have the towns
  (`LR_MAP=ivano_frankivsk_city`); generated `--selftest` maps have no buildings.
  Test a change with duels against `simple` on a few seeds (`LR_PLAYERS=2 LR_AI=zones,simple LR_SEED=n`); the
  new types should win all of them.
- The hostile side only holds positions; it has no AI.
- The AI does not terraform or flank on purpose beyond the crescent. The `Simple` type uses no scavengers,
  transports, garrisons, field works or storming at all.
- The minimap picture and the cached fog/radar shadows do not follow craters and terraforming; the
  minimap does not show trees and rocks or scorch marks. Ground layers do not change with craters (only scorch).
- Netplay sends commands only (about 20 bytes per tick). Every client holds the full game state, so fog of war
  is not cheat-proof. A player who left cannot come back (only rejoining a new host after host migration exists).
- Mispredicted effects are not undone: a muzzle flash or explosion the rollback corrected away has still been shown.
- Balance numbers are first-pass.
