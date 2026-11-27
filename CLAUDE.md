# Long Range

Standalone 3D RTS in Rust/Bevy, based on *Long Range Tank* Supreme Commander: FA mod
(`~/My Games/Gas Powered Games/Supreme Commander Forged Alliance/mods/LongRangeTank`).
Core idea: unwieldy tanks, long range, slow reload, slow aim, high damage. Units out-range own sight, so scouting
and radar decide who shoots. SupCom-style flow economy and controls, BAR/Zero-K-style spread and area commands.

User (monx94) and friend make mod together; friend made its models and icons, reusable here. Do not copy stock
SupCom names or art beyond what is in mod folder.

## Stack

- Rust, **Bevy 0.20** (`default-features = false, features = ["3d"]`), **bevy_egui 0.43** (egui 0.36) for menus
  and all HUD painting, `serde_json` game data, `bincode` network, `image` (PNG), `gltf` (models).
- Bevy 0.20 and egui 0.36 are newer than most docs. Read examples in `~/.cargo/registry/src/*/bevy-0.20.1/examples`
  before using API. Differences from older versions: input events are `MessageReader<T>`, lights use
  `shadow_maps_enabled`, ambient light is `GlobalAmbientLight`, non-send resources are `insert_non_send`/`remove_non_send`,
  egui has `Panel` instead of `TopBottomPanel`, `ctx.content_rect()`, `ctx.is_pointer_over_egui()`,
  `rect_stroke(.., StrokeKind)`.
- glam serde feature not enabled; serializable vectors are our `map::V2` / `map::V3`.
- Nested system tuples are NOT ordered by outer `.chain()`: every inner tuple needs its own `.chain()`
  (real bug in `sim/systems.rs::register`).
- Dev machine: Linux, 2 cores / 8 GB, Intel HD 520. User builds release binaries on Windows. Cold Bevy build is very
  slow here; never put `target/` on `/tmp` (tmpfs).
- Game needs `assets/` next to executable (`data::asset_root`, override `LR_ASSETS`).

## Commands

```
cargo run                         # game (dev profile: our code opt 1, dependencies opt 3)
cargo run --features dev          # Bevy linked dynamically: much faster rebuilds (never for releases)
cargo run --release
cargo run -- --skirmish           # straight into a game vs 3 AIs
cargo run -- --selftest           # headless 4-AI match, prints economy/tier stats (LR_MAP=name: premade map)
cargo run -- --combattest         # headless duels: accuracy, reversing, assisted upgrade, radar line of sight
cargo run -- --nettest            # loopback netplay: 3 AIs + predicting client, state hashes must match (LR_TICKS=n,
                                  # LR_AI=zones,.. AI types, LR_CLIENT_AI=zones an AI plays the client, LR_MAP=name, LR_PLAYERS=3)
cargo run --release -- --bench [MIN]   # benchmark: AIs play every slot, game runs flat out to MIN (25), then every graphics
                                  # setting measured in turn (frame / main / render / GPU per pass); report -> bench-*.txt in config
                                  # folder (LR_BENCH_SECS=n per scenario, LR_MAP=name; GPU pass times off on DX12, where they
                                  # lost the device on an RX 5700 XT: LR_BENCH_GPU=1 forces them, or LR_BACKEND=vulkan). view/bench.rs, view/perf.rs
cargo run -- --icons out.png      # every strategic icon, order icon and cursor in one picture (check after icon changes)
cargo run -- --ground out.png     # every ground texture in one picture, plus table vs measured average colours
cargo run -- --editor             # map editor (also main menu "Map editor": game restarts itself with --editor)
cargo run -- --editor --generate NAME CELLS[xDEPTH] SEED [PLAYERS] [BIOME]   # generated map with towns, roads, trees, rocks -> assets/maps (BIOME: temperate/desert/arctic/rocky; 256x160 = not square, 2 players)
cargo run -- --editor --scatter NAME [SEED] [AMOUNT]   # add trees and rocks to a saved map
cargo run -- --editor --roads NAME ..  # restyle saved map's roads: kinds by width and town, sidewalks, cut under water (only map.json)
cargo run -- --editor --points NAME   # give a saved map without any the control points a game would place (only map.json)
scripts/release.sh [--only linux|windows] [--no-upload] [--tag vX] [--draft]   # release bundles (+ gh upload)
scripts/exe_updates.sh vX              # in-game updater files in dist/: whole exe + patch from previous release (zstd; release.sh and CI run it)
.venv/bin/python tools/convert_assets.py "<mod folder>" [--only trooper]   # re-import models/textures/portraits from the mod
python3 tools/blend.py art/NAME.blend [--watch]   # Blender model -> assets/models/NAME.glb + textures (needs Blender; tools/README.md)
.venv/bin/python tools/convert_assets.py --props "<maps/mining_town.v0001>"   # map buildings (MAP_PROPS)
.venv/bin/python art/build.py [NAME ...] [--blend]   # our own models (factories, engineers, aircraft, carriers, city buildings, variants) -> assets/models (needs bpy: .venv/bin/pip install bpy)
.venv/bin/python art/menu_background.py [--size WxH]   # main-menu backdrop (steppe, tanks on a ridge) -> assets/ui/menu_bg.png (Cycles, ~1.5 min)
.venv/bin/python art/render.py out.png NAME ...     # contact sheet of exported models rendered by Blender (--anim open:1, --yaw 35)
python3 tools/dump_mod.py "<mod folder>"                  # print blueprint stats (source of the JSON numbers)
python3 tools/relaxed_preset.py                           # remake Relaxed preset from built-in data (after data changes)
python3 tools/model_bounds.py                             # model bounds of every unit vs its JSON size / hit_box
.venv/bin/python tools/render_icons.py [id ..] [--all]   # unit portraits (assets/icons/units, 3/4 view + white glow like the mod's) rendered from our models with Blender; default: units without own icon
.venv/bin/python tools/osm_map.py NAME --bbox S,W,N,E      # map of a real place (OSM + terrain tiles) -> assets/maps/NAME, see tools/README.md
.venv/bin/python tools/scmap_map.py "<FA maps>/<map folder>" NAME [--size 512] [--variants 4,6]   # port a SupCom map (.scmap + _save.lua) -> assets/maps/NAME, NAME_<k>p
# The converters need numpy + Pillow: they live in the project venv (python3 -m venv .venv; .venv/bin/pip install numpy pillow).
```

Debug env vars: `LR_SPEED=8` (fast-forward hosted game), `LR_WATCH=1` (AI also plays local
slot, on client too: its orders go way human's do), `LR_HOST=1` (host lobby at start; starts 8 s after a
player joined) and `LR_JOIN=<ip>` (join it at start): two windows play multiplayer without clicking, `LR_REVEAL=1` (no fog of war), `LR_MAP=<name>` (premade map for `--skirmish`), `LR_DUEL=1` (stages tank
fight at first start), `LR_PLAYERS=10` (AI count for `--selftest`), `LR_SEED=n` (other map and AI seed
for `--selftest`: one game proves little, compare few), `LR_SURFACE_DEBUG=n` (ground shader shows one input
For `--selftest` only: `LR_AI=zones,overseer,simple,...` (AI type of each player in turn; default one of
each), `LR_TEAMS=2` (player i in team i % n), `LR_POINTS=1200` (control points mode), `LR_COMP=1` (lists
what every player owns), `LR_MOAT=1` (water around second start: tests transports and island play), `LR_INFCAP=0` (lobby option "infantry capture buildings" off; on by default), `LR_RUBBLE=0/1` (lobby option "rubble blocks paths", default from data), `LR_EVERY=300`
(report every that many ticks instead of 3000), `LR_AIDEBUG=1` (player 0's attacking body to stderr every 30 s: each unit's
distance to step and march centre, its order, `*` = firing). For `--combattest` with `LR_TEST=tanks`: `LR_FIGHTS=bastion:1:overtaker:4;...` (own equal-mass fights instead of built-in list), `LR_OPEN=1` (open ground only). Report prints each AI's state (`Brain::debug`): strategy, army phase, zones held.
unlit: 1 base layer, 2 scorch, 3 masks 1-3, 4 roads, 5 blended colour, 6 vertex tint), `LR_SHOT=file.png,secs,unit,dist[,shore|x:z]`
(screenshot of running game after `secs` game time, then quit; `file.png,secs,menu`: main menu after `secs`; `shore` looks at steep shore near map centre, `x:z` at that point). `LR_PROFILE=1` (`--selftest` prints time of every tick system: per report interval, then whole game; in a
windowed game `profile:` line goes to log every 30 s: fps, frame / main / render CPU / GPU ms, costliest view
systems and sim systems, `view::perf::FrameProfile`), `LR_THREADS=n` (compute threads of headless tests; 1 runs `par_map` inline - hashes must not change).

### Testing the window without a desktop

Xvfb cannot present Vulkan frames. Works: nested headless sway with Xwayland, xdotool for input, grim for screenshots:
`WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 sway -c <cfg>`, run game with `DISPLAY` set to that
session's Xwayland and `WAYLAND_DISPLAY` unset; screenshot with `WAYLAND_DISPLAY=<its socket> grim out.png`.
First xdotool mouse move lands at window centre - do throwaway move before first click.
`swaymsg seat cursor ...` does not reach clients in headless mode.

### Building

One binary, `longrange`; map editor is part of it (`src/editor.rs`, `--editor`), so releases ship
one executable and one `assets/` per platform. Compiling crate itself takes ~1 minute.

### Renderer compatibility (old Intel iGPUs)

`settings::render_plugin(&Settings)` / `tolerate_render_errors()` / `apply_settings` (in `src/settings.rs`, which
also holds `Settings` resource saved to `settings.json` in `settings::base_dir()` - user config folder, not next to `assets`, so it survives new versions: `%APPDATA%\LongRange`, `~/.config/longrange`, `~/Library/Application Support/LongRange`, override `LR_CONFIG`; old `settings.json` next to `assets` is imported once; logs go there too - and shared settings window) are used by game and editor:
Windows defaults to DX12 (Intel Vulkan driver `igvk64.dll` crashes at start-up on HD 520/530),
MSAA off, validation errors logged instead of quitting (Bevy 0.20's default handler exits app).
Renderer is always Bevy's full one (GPU occlusion culling on). A "Compatible" renderer that disabled
`INDIRECT_FIRST_INSTANCE` (no occlusion culling or depth-pyramid compute passes) was removed on request;
if old GPU fails in those passes again, that was switch.
Do NOT zero compute limits to force CPU preprocessing: atmosphere plugin then fails validation at start.
Windows needs DXC shader compiler: MSVC builds link it statically (`statically-linked-dxc`, wgpu supports it
only with MSVC - MinGW build with that feature panics at start-up); MinGW cross build made by
`scripts/release.sh` ships `dxcompiler.dll` next to exe (`render_plugin` looks for it there): with FXC 3D view stayed
black while UI rendered. OpenGL backend comes from direct `wgpu` dependency with `gles` feature.
OpenGL needs `synchronous_pipeline_compilation` (background pipeline compiles panic with "Could not lock adapter
context") and its failing SSAO shader arrives as `Internal` error, tolerated like validation
errors; `settings::log_plugin` mutes per-frame `wgpu_hal::gles` heuristics spam.
Windows exe runs straight from WSL (`dist/.../longrange.exe --skirmish`, console output captured).
Render errors go to `render-errors.log` and settings window. Everything logged also goes to `longrange.log` /
`lr-editor.log` (with `--editor`) next to `settings.json` (previous run's as `*-previous.log`), panics with backtrace:
read those first on crash report.
Alt+Tab minimises window; Bevy then resized swap chain to 1x1 and DX12 `ResizeBuffers` failed ("window is in
use", "Invalid surface"), losing surface. `settings::keep_surface` (render world, before `create_surfaces`) keeps the
old size while minimised and retries configuring ~once second after failed configure. Override: `LR_BACKEND`.
Wine's DX12 (vkd3d) panics inside wgpu's format query (`wgpu-hal dx12/adapter.rs`, CheckFeatureSupport) regardless;
test Windows builds under Wine with `LR_BACKEND=vulkan`.
Linux OpenGL (`settings::linux_gl`): Bevy 0.20 makes wgpu instance without display handle, so EGL is
"surfaceless" and Bevy panics with "failed to choose present mode". We build renderer ourselves
(`RenderCreation::Manual`) on our own X11 connection, force winit onto X11 (Xwayland) and repoint each window's
`RawHandleWrapper` display at that connection (`gl_window_display`). Under WSL with GPU support "auto" means
this path with Mesa's d3d12 driver (`GALLIUM_DRIVER=d3d12` and `MESA_EXTENSION_OVERRIDE=+GL_EXT_texture_shadow_lod`
set in-process; without extension every PBR shader fails: black 3D view). WSL's Vulkan is llvmpipe:
seconds-long frames, and WSLg's Wayland compositor then drops window ("Broken pipe").
wgpu's OpenGL backend makes 2D textures of 6, 12... array layers cube maps: such texture array samples black
(ground layer array is padded, `view::surface::albedo_image`).
Never replace camera `Projection` wholesale each frame: it resets `aspect_ratio` and frustum culling then
drops terrain at screen sides (edit `fov/near/far` in place).

## Conventions

- World units = SupCom units (tank ~0.7 wide, ranges 15-300). Y up. Model forward is -Z.
  Unit yaw `a` = facing `(cos a, sin a)` in XZ plane; `view::yaw_quat` converts to rotation.
- Build/navigation grid cell = 2 world units (`map::CELL`). Structure footprints in cells.
- **Maps may be rectangular**: `MapData.nx` cells across (x) by `ny` deep (z); heightmap has (nx+1) x (ny+1)
  vertices, every per-cell grid (`blocked`, `masks`, `lake_of`, nav `occ`, fog, sight blockers) is `nx` wide, row by
  row. Never assume square: use `world_x()` / `world_z()` / `size()`, `dims()` (`map::Dims`, what `find_path` /
  `nearest_free` / `line_clear` take), `idx(cx, cy)`, `cells()`, `vx()` (vertices per row), `inside`, `on_map`, `clamp`;
  `extent()` (side of same-area square) for "share of map" distances, `world_max()` for camera limits.
  map.json keeps `n` (width), adds `ny` only when different. Square pictures (lobby, browser, thumbnails) centre a
  rectangle (`MapData::pic_uv`, `terrain::map_preview`); minimap letterboxes it. `MapData::generate_rect`
  makes 2-player maps only (half turn is one rotation mapping rectangle onto itself), `scatter_features` /
  editor's towns use no symmetry on rectangle with more starts. Square maps simulate exactly as before.
- Simulation: 10 ticks/s in schedule `SimTick`, run on EVERY machine (`netplay.rs`, called from
  `main.rs::session_update`). Only commands cross network: host bundles each tick's commands (own,
  AI's, clients') into `Turn`; clients show prediction `lead` ticks ahead, apply own commands
  at once, and on every turn roll back to last confirmed state (`sim::restore`) and re-simulate.
- Simulation must stay deterministic, Linux and Windows alike (`--selftest` prints state hashes that
  must be equal on both; `--nettest` compares host and client):
  - query loops that allocate ids, use `SimRng`, spend budget or queue effects iterate with
    `.sort_by_key::<&Unit, _>(|u| u.id)` - plain ECS order differs per machine and after rollback;
  - `sin/cos/atan2/acos/atan/powf...` only through `map::fm` (libm); `sqrt`, `+ - * /`, `floor` are fine;
  - never iterate `HashMap`, never read `Entity` values, wall-clock time or view state in sim system;
  - parallel work only through `sim::par_map` (results in item order, applied by sequential loop
    afterwards), only where each result depends on its own item and on data nobody writes meanwhile
    (no `SimRng`, id allocation, budgets or effects inside). Used by `visibility` (sight lines) and
    `locomotion` (A* searches, planned in id order within `Nav.paths_left` first). Check with
    `LR_THREADS=1` and default: `--selftest` hashes must be equal. `SimTick` runs on single-threaded
    executor: its systems are all chained; multi-threaded one doubled tick time;
  - anything surviving from one tick to next must be in `sim::Snapshot` (`make_snapshot`/`restore`),
    which is also state client joins with and repair sent when client's state hash differs;
  - unit component is added to `unit_state!` list in `sim/mod.rs` (required or optional), nowhere
    else: that list makes `UnitSnap` and how snapshots read, restore and strip it (for `Limbo`). Then
    `spawn_site` does not compile until it sets new field. `cargo test --release --lib sim::tests`
    checks round trip; `tests::full_unit` needs new component too;
  - new simulation objects are created with `spawn_unit` / `spawn_projectile` / `spawn_wreck` (they reuse the
    entity rolled-back object had, so models do not flicker), never `commands.spawn`;
  - terrain heights change only through `sim::Ground::set` (craters, terraforming): it records vertex in
    `Ground.changed` (snapshot's `ground` list, diff against map game started on), updates
    `map.blocked` around it and tells view which chunks to rebuild. Writing `map.heights` directly
    desyncs rollback, joining and state hash.
- AI runs on host only; its commands reach clients inside turns. Its working state (`ai::Ai`,
  `brain::Brain`) is not simulation state: after host migration new host starts fresh AIs of
  same types (`Player.ai_kind`) that rebuild what they know from world.
- **Replays** (`replay.rs`, `Settings.save_replays`, default on; `LR_NOREPLAY=1` skips recording): replay is what a
  client joins with (map, `DataPack`, starting `Snapshot`, deflated) plus every confirmed tick's commands, written in
  chunks while game runs (newest `TAIL` ticks stay in memory: after host migration client may be rewound,
  `ClientSim.rewound`) and state fingerprint every `replay::HASH_EVERY` ticks. Host records turns it makes,
  client what it confirms (`ClientSim.log`, drained by session), so every player and observer gets one.
  Files: `replays/*.lrr` next to `settings.json`, oldest beyond `replay::KEEP` deleted, games under 15 s
  leave none. Main menu "Replays" (`main.rs::replays_ui`) lists them; watching is `Role::Replay` (`replay_update`):
  re-simulates as `OBSERVER` with pause, speed slider (log scale x0.25-x32, double-click = x1), time line, vision (`ViewState.replay`, `hud::replay_bar`); jumping back
  restores nearest state kept every `HASH_EVERY` ticks. A fingerprint mismatch (another version) is shown, not
  fatal. `--nettest` writes host's and client's replay and plays both back.
- A player who leaves resigns (`Command::Resign`), game goes on. When host leaves (connection closed, or
  silent for `HOST_SILENT_SECS`), remaining humans take over (host migration, `main.rs::take_over`): host
  sends `S2C::Peers` (human players + addresses) at start and whenever someone leaves; lowest-numbered
  remaining human becomes host from its last confirmed state (AIs restart, old host's player resigns), others
  reconnect with `C2S::Rejoin` and get full state; game waits up to `REJOIN_WAIT_SECS` for them.
- Network writes (`net.rs`): frames go out in `CHUNK` (16 KB) pieces with per-piece limit `STALL_SECS` (30), so a
  multi-MB `S2C::Start` over slow VPN (Radmin) does not time out - whole-frame 5 s limit dropped players at start,
  and on Windows timed-out send leaves socket unusable. Client writes go through writer thread (`Client::broken()`),
  host repairs diverged client at most once per `netplay::REPAIR_GAP` ticks, `LobbyInfo.preview_same` leaves out
  unchanged map picture.
- Units refer to each other by network id (`u32`), never `Entity`.
- ECS layout: one entity per unit with `Unit`, `Body`, `Health`, `Orders`, `Intel` plus optional `Weapons`,
  `Builder`, `Factory`, `Mover`, marker/state components `UnderConstruction`, `Upgrading`,
  `Hosted` (still inside its factory), `Structure` and more: full list is `unit_state!` in `sim/mod.rs`.
  Projectiles and wrecks are entities too. View adds its own components (`Interp`, `Transform`, model
  children) to same entities; not part of state.
- Systems read other units through per-tick `Census` resource (plain copies + spatial grid) and queue
  cross-entity effects in `Effects` (damage, reclaim) instead of holding conflicting mutable queries.
- **Localisation** (`src/i18n.rs`): every player-visible text is looked up by key: `tr!("lobby.watch_instead")`
  (`&str`) or `tr!("lobby.observers", a, b)` (`String`; texts use `{}`, `{0}`, specs like `{:.1}` / `{:02}`, plural
  forms `{0|one|few|many}`). Keys in `const` tables are marked `tx!("group.key")` and shown with `tr(TABLE[i])`.
  Texts are in `assets/lang/<code>.json`: `en.json` (English, reference) and `uk.json`, grouped in
  nested objects (key = path: `{"lobby": {"watch_instead": ..}}`); groups `common` (shown in several
  screens), `main_menu`, `connection`, `lobby`, `settings`, `hud_*`, `units`, `weapons`, `terrain`, `ai`, `preset_*`,
  `map_editor`. A missing text falls back to English, then key. Texts from game data keep their English in
  data: `unit_name` / `unit_desc` / `weapon_name` (and `kind_name`, `biome_name`, `ai_name`) look up
  `units.<id>.name|desc`, `weapons.<slug of the name>`... only while data still says what `en.json` says, so a
  preset's own names show as written. `cargo test --lib i18n` fails when key used in code or data text is
  missing from `en.json` (or differs from data), when another language lacks key or has other placeholders, or
  when file holds unused keys: new or changed text (also unit name/description in faction JSON) needs its
  `en.json` and `uk.json` lines in same commit. Ukrainian unit nicknames are translated and in «» (type or
  designation outside: «Обганяч» M1, БРМ «Драгун», ЗРК «Небесний спис»). Never translate inside simulation, in ids,
  or in what is sent over network (host sends keys, e.g. `S2C::Kicked`, receiver translates). Language:
  `Settings.language` (empty = from `LANG`), switched live.
- All content lives in JSON under `assets/data`. Never hard-code unit stats or names in Rust; AI finds
  units by role (extractor, generator, factory...).
- `Defs` resource is data set current game uses: built-in data, host-chosen preset,
  or host's data received in `S2C::Start`. `main.rs::restore_defs` puts built-in data back on way
  to main menu. `Defs.pack` (`data::DataPack`) holds file texts it was built from.
- JSON files under `assets/data` have CRLF line endings; keep them (data hash is taken of bytes).
- Owners 250/251 are neutral and hostile sides (teams 14/15). Neutral units are never auto-targeted.
- 2-10 players (`sim::MAX_PLAYERS`). `Intel.seen/radar` are `u16` team bitmasks. A map's number of start
  positions decides lobby slot count; `MapData::generate(cells, seed, players)` is k-fold symmetric.
- Hotkeys: two presets in `view/hud.rs` (`ViewState.preset`, saved to `settings.json` in user config folder):
  default, and one modelled on FAF's "alternative" keymap (reference: FAForever/fa `lua/keymap/alternativeKeyMap.lua`)
  with hotbuild keys chosen by unit role in `hud::hot_key`.
- Releases: `scripts/release.sh` / `release.ps1` make one archive per platform in `dist/` (game with editor + assets)
  and publish with `gh`. Each release also carries in-game updater's `longrange-<v>-<platform>-exe.zst` and
  `-from-<previous v>.zst` (`scripts/exe_updates.sh`, needs zstd: `release.ps1` makes none). **In-game updater**
  (`src/update.rs`, main menu "Check for updates", manual only): compares local files' git blob hashes with tag's git
  tree (GitHub API), fetches changed assets from raw.githubusercontent.com and exe patch (or whole exe) with the
  `curl` program into `.update/`, checks them, moves them into place (running exe -> `*.old`) and restarts. Assets must stay
  byte for byte what git holds (they do: bundle copies checkout). Off under `cargo run` / `LR_ASSETS`. They first delete everything else in `dist/` (older versions).
  `.github/workflows/release.yml` does same on GitHub's machines (Linux + Windows MSVC) when tag `v*` is pushed
  (or by hand from Actions tab, "Run workflow" on main, which creates tag): bump `Cargo.toml`, add `## vX.Y.Z` CHANGELOG section, tag, push tag.
  Commit every finished incremental change right away (one commit per change, short message saying what it does),
  so several sessions in this tree do not pile up one big uncommitted diff. Stage only files
  you changed (`git add <paths>`, never `git add -A` / `.`): other sessions' work may be uncommitted in same tree.
  Do not tag or publish release without being asked. Pushing is allowed to separate branch (session's
  own `claude/...` branch, never main): push finished commits there without asking. Do not push to main.
- User wants sub-agents on weaker/cheaper models used where they help (searching and summarising code, running
  and tabulating test batches, building baseline to compare with, self-contained pieces of work), keeping
  design and edits of shared files in main session.

## Source map

| Path | Contents |
|---|---|
| `src/data.rs` | JSON schema (`UnitDef`, `WeaponDef`, `Rules`...), `DataPack` (file texts, presets on disk, assets sent along), loading, validation, data hash |
| `src/map.rs` | `V2`/`V3`, heightmap `MapData`, generator, save/load, line of sight, raycast, A* |
| `src/surface.rs` | ground texture data (view only): layer kinds (`KINDS`), layers + masks, biomes, `paint_ground`, roads (raster, keeping trees off) |
| `src/view/surface.rs`, `surface.wesl` | terrain material (`ExtendedMaterial`, WESL fragment shader): procedural layer textures with mips, mask/road/scorch textures, `SurfaceView::burn`, `Tint` (average colours for minimap/preview/apron) |
| `src/sim/mod.rs` | components, resources, `Order`/`Command`, spawning, `Ground` (changed terrain), control point placement, state save/restore |
| `src/sim/systems.rs` | tick systems: commands, visibility, economy, orders (`think`), factories, build, movement, transports (`carry`), weapons, projectiles, reclaim, capture, deaths, craters/terraform (`apply_ground`), control points, victory |
| `src/ai.rs` | AI types (`AiKind`), `Simple` AI, `ai_system` (dispatch, once a second per AI), formation assignment |
| `src/ai/brain.rs` | zone AI behind every other type: unit roles from JSON (`info`), per-type numbers (`Profile`), zones, enemy memory, strategy, economy (`eco`), army (`army`) |
| `src/net.rs` | TCP host/client, UDP LAN discovery, message types |
| `src/netplay.rs` | `HostSim` (turn scheduling, hash check), `ClientSim` (prediction, rollback, resync) |
| `src/replay.rs` | replays: file format, `Recorder` (fed by `main.rs::session_update`), list/load, `Playback` (re-simulation, jumping back via kept states) |
| `src/view/mod.rs` | view state, unit visuals, interpolation, turret animation |
| `src/view/hud.rs` | camera, selection, orders, overlays, HUD panels (one egui pass) |
| `src/view/intel.rs` | fog grid, terrain-shadowed radar coverage, contact memory (ghosts/blips) |
| `src/view/models.rs` | GLB/box model loading, team-colour texture baking |
| `src/view/terrain.rs` | terrain chunks (rebuilt by `terrain_sync` where ground changed), land beyond border (`apron_mesh`: map mirrored on all sides, blurred, hazy, built once), dashed map edge (`map_edge_sync` re-widens it with camera distance), water, deposits, sun, minimap pixels |
| `src/view/water.rs`, `water.wesl` | water material (`WaterMaterial`, sea and lakes): shore field (`shore_field`: depth + signed distance to waterline on a quarter-cell grid, 64 beyond border, rebuilt at most once a second when `Ground.hash` or editor's map changes) for smooth shoreline, shallows, lapping foam at every level; wave normals at high; ultra (`water_detail` 3): planar reflection camera (`reflection_sync`, half window size, mirrored at the water level the camera looks at, clipped at surface by `MirrorProjection` - Bevy's `near_clip_plane` leaves no depth range when looking down at water); gizmos on own render layer so they are not mirrored |
| `src/view/sky.rs` | skies (`MapData.sky`, map.json `"sky"`; `SKIES`: clear, hazy, overcast, sunset, night, arctic): procedural cubemap skybox on camera, haze colour, sun and ambient light, cloud layer (`Settings::clouds`, visible only with camera above ~320 units, fades in to 700; 3D, so never over HUD icons); `sky_sync` runs in game and editor; at ultra building detail also specular-only `EnvironmentMapLight` (`env_light`: CPU-prefiltered mips, black diffuse map, lower half a dimmed horizon) for glass and metal; `--skies out.png` previews; unknown/empty id = clear day; `paint_ground` gives an unset map its biome's sky |
| `src/view/sound.rs` | synthesised weapon and interface sounds, file overrides |
| `src/view/fx.rs` | projectile visuals, explosions (fewer puffs, no flash lights at low effects level) |
| `src/view/debris.rs` | debris pieces: jagged chunk meshes, falling/tumbling, shown by effects level |
| `src/view/grass.rs`, `grass.wesl` | grass (`Settings::grass_detail`): clumped blades per 8-cell chunk near view point, `GrassMaterial` vertex shader thins by distance (blade rank vs density; far chunks hold only low ranks), sways, hides where scorch map is set; rebuilt only where `GrassTouched` (from `terrain_sync`) says ground changed |
| `src/view/flora.rs` | trees and rocks: procedural models (detail setting), one merged mesh per 16-cell chunk |
| `src/view/marks.rs` | pings and map drawings (`Marks`: own ones to send, shown ones), team filter, sound |
| `src/view/icons.rs` | procedural strategic icons, order icons, mouse cursors |
| `src/view/cursor.rs` | hardware mouse cursor showing the click's action (`CursorWant` set by HUD from `hud::Frame::context` / order mode); takes cursor control from egui |
| `src/main.rs` | menu, lobby, preset editor, host/client session loop, headless tests |
| `src/update.rs`, `scripts/exe_updates.sh` | in-game updater: changed assets via tag's git tree + raw downloads, exe via zstd patch, curl, install + restart |
| `src/theme.rs`, `assets/fonts/` | interface look (game, menus, editor): Play (all text, Cyrillic) and Michroma (title only, Latin) embedded, flat egui style (square, 1 px lines, no shadows, gradients or glows: requested), menu plates/buttons with cut corners, backdrop `assets/ui/menu_bg.png` |
| `src/i18n.rs`, `assets/lang/` | localisation: `tr!` / `tx!` / `tr`, language files, completeness test |
| `src/preset_form.rs` | preset editor's form view: one `Field` (JSON path, label, kind + default, tooltip) per setting of rules.json, units, weapons; edits a `serde_json::Value`, unknown keys stay |
| `src/editor.rs` | map editor and map generation CLI (`longrange --editor`) |
| `build.rs`, `res/` | Windows exe icon (`res/longrange.ico`, from mod's `modicon.jpg`) embedded with `embed-resource`; `settings::add_window_icon` puts it on the running window (winit sets none) |

## Game rules worth knowing

- **Ballistics**: every shot is a projectile entity (3D, gravity `rules.gravity`); kinds `direct`, `arc`, `high_arc`, `missile`, `bomb`; hits resolve against terrain and turning unit hit boxes (`hit_box`, `Cen::hit_seg` / `hit_dist`). Details: comment above `systems::projectiles`.
- **Line of fire** (direct fire, `systems::Lane`): a gun fires only if the shell's real arc reaches the target before ground, water, buildings, own/neutral units or cover >= `COVER_BLOCK`; blocked ground units step aside (`firing_spot`). Details at `Lane`.
- **Engineers before buildings**: while an enemy mobile builder is in a weapon's reach, enemy buildings rank last as targets (`Census::pick_enemy`, `Lane::pick`). Details at `pick_enemy`.
- **Bombers** (`systems.rs::bomb_drop`): SupCom-style stick bombing, reach >= throw + 12, locomotion keeps them flying through a run-in. Details at `bomb_drop`.
- **Switches** (`rules.json`, the form's "Switches" section): rule flags (`terraforming`, `craters`, `wrecks`, `sight_blocked_by_*`, `counter_battery`, `near_sight`...); the lobby lists those switched off (`Rules::switched_off`, list in its comment).
  A new rule needs a `Field` in `preset_form.rs` too, or it only shows in the JSON view.
- **Armor angle** (`rules.armor_front/side/rear`): damage on armored units scales with approach angle vs hull facing and flatness of flight (`systems::armor_angle`).
- **Splash** (`rules.splash_direct`, `splash_armored`, weapon `splash`): the direct hit takes all damage, nearby units a fading share; armored units take `splash_armored` of splash. Details at `Rules.splash_direct`.
- **Spotting**: weapons only engage targets their team sees or has on radar. Radar needs line of sight
  over terrain (`map.los`).
- **Counter-battery radar** (`rules.counter_battery`): shells in flight inside enemy radar reveal their artillery shooter as a radar contact (worked out in `visibility`, no state).
- **Binoculars** (unit JSON `binoculars`): a narrow view cone that locks, tracks or scans (`systems::aim_binoculars`, `Intel.look/lock`, `FogGrid.cone`).
- **Spotting delay** (unit JSON `spot_time` / `spot_time_moving`): a unit counts as `seen` only after being in view that long (`Intel.spot`, `visibility`).
- **Radar levels and stealth**: `radar` / `radar2` / `radar3` ranges vs unit `stealth` 0-3 (`UnitDef::radar_vs`); vision is also blocked by trees, rocks, wrecks and buildings (`sim::sight_block`, `map.sight_line`). Details at `radar_vs`.
- **Visibility cost** (`systems::visibility`, costliest tick system on town maps): viewer grid, `RadarLosCache`, `map.los_top`, staggered neutral checks; `Census` hash maps are lookup-only.
- **Contact memory** (client side, `intel.rs`): radar blips fade, detected buildings stay as ghosts (models: `view::ghost_visuals`); right-click on a ghost = ground fire.
- **Reclaimables** (trees, rocks, wrecks together): **weapons never destroy them**; shots through them lose a `cover` share, blasts fell trees, shatter rocks and shove wrecks (`hurt_reclaimables`, `push_wrecks`, `rubble_motion`). Felling: `Features::fell`.
- **Debris** (`rules.debris`): big hits and destruction drop `Wreck` pieces (`piece` > 0); their mass comes out of the building's wreck value (`Shed`). Details at `Wreck`.
- **Cover** (`systems::soak_cover`): each tree/rock/wreck a shot passes through takes its `cover` share of the damage; never used up.
- **Rubble blocks paths** (lobby option `Game.rubble_blocks`): wrecks, ruins and big debris mark `map::OCC_RUBBLE` in `Nav.occ` (`rubble_cells`, derived, not state).
- **Infantry** (`UnitDef::is_infantry`): idle or engaged infantry hide behind cover (`cover_spot`, `Mover.cover`); field works are cover for every side. Details at `cover_spot`.
- **Garrisons** (`UnitDef::is_garrison`, `Order::Load(building)`): riders fire and see from the roof; `systems::garrisons` hands out ownership; enemy infantry storm them (`assaults`, `Effects.assault`), trenches likewise (`trench_to_storm`). Details at those functions.
- **Infantry capture** (lobby option `Game.infantry_capture`): infantry take `seizable` buildings (`Order::Capture`, `systems::seize_target`, `apply_capture`). Details at `apply_capture`.
- **Tiers**: `tier` on a unit, `build.tier` on a builder; factories and pumps upgrade in place (`upgrade`); change `tier` in JSON to move a unit. Roster history and our own additions: comment at `UnitDef.tier`.
- **Energy stall**: upkeep and construction share energy proportionally (`build_progress`, `Player.power`); `Paused` (Tab) switches a unit's work off. Details at `build_progress`.
- **Engineering station** (`build.station`): immobile builder that assists anything in range by itself.
- **Ammunition** (weapon `ammo`, `rearm`, `rearm_mass`): rounds missing in `WeaponState.fired`, rearmed as a mass flow in `weapons`. Details at `WeaponState`.
- **Hold** (weapon `hold`): weapon fires only while stopped; each shot sets `Mover.hold`. Details at `Mover`.
- **Reactive armor** (unit JSON `reactive {charges, block}`): missile hits spend a charge (`Reactive` component, `projectiles`). Details (and the Bastion) at `Reactive`.
- **Suicide drones** (weapon `suicide: true`, `ammo: 1`; T2 Dove): the unit itself becomes a homing missile (`fire`). Details at `fire`.
- **Movement**: land units accelerate/brake and choose forward or reverse by turn + travel time (`systems::locomotion`); Ctrl-held orders add `Command::Pace`. Details at `locomotion`.
- **Adaptive maps** (`frankivsk_mini`, `frankivsk`, `liberec`; `tools/merge_variants.py`): `map.json` holds the union of starts + `variants` + `attached`; the lobby lists one entry (`folder#k`), `MapData::adapt` (host, `start_game`) trims to the slots played. Details at `adapt`.
- **Game browser** (`Screen::Browse`, `main.rs::browser_ui`): UDP discovery (`net::Discovery`) + `net::GameAd`; joining by address too. Downloads of a host's map/data/assets: `main.rs::keep_downloads`. Details at `GameAd` / `keep_downloads`.
- **Lobby** (`main.rs`): players, observers (`sim::OBSERVER`, `C2S::Observe`, never host), map preview, option tabs; a random map is made when chosen (`HostLobby::ensure_map`). Details at `ensure_map`.
- **Pings and drawings** (`view/marks.rs`): not commands: `C2S::Mark` -> host -> `S2C::Mark`, so they skip sim, hash and replay. Details at `Marks`.
- **Small village houses** (`khata`, `cottage`, `dacha`, `farmhouse`...; models `art/models_civilian.py`): `tools/small_houses.py` swaps a share of houses on real-life maps; `osm_map.py` places them. Details in that script.
- **Model scale**: the mod's own models use 2.5x the blueprint's `UniformScale` (sizes, footprints too); our `art/` models are in world units at scale 1; civilian map buildings doubled; towns dense (1-cell streets). Details at `ModelDef`.
- **Icons**: `icon: {shape, glyphs, no_tier}` + tier ticks; the loader rejects duplicate icons among playable factions. Details at `IconDef`.
- **Reclaim**: `Order::Reclaim(id)` (wreck, building, tree, rock; ids never collide), `ReclaimArea`, scavenger `Revive` / `ReviveArea` / `Assist` with the `Revival` component. Details at `systems::TEAMWORK`.
- **Trees and rocks** (map "features", types in `rules.json: features`): `sim::Features` keeps the share left (snapshot = diff vs map), id `FEATURE_ID0 + i`, reclaimed by id/area, never block movement or shots; `MapData::scatter_features`. Details at `Features`; drawing/LOD/streaming at `flora::stream_near`.
- **Ground textures** (`surface.rs`, visual only, never in sim/hash): `MapData.layers` + per-layer masks, `KINDS` textures (`KINDS.color` must match the texture's average: `--ground`), roads (`road_raster`, `centre_raster`, `--editor --roads` = `editor::restyle_roads`), scorch (`SurfaceView::burn`). Trees/rocks are kept off roads (`clear_features_on_roads`). Details at `KINDS`, `road_raster`, `burn`.
- **Glass and metal reflections** (ultra building detail, close up only): `models::gloss_map` + `gloss_sync` swap in glossy materials by camera distance. Details at `gloss_map`.
- **Water levels**: `MapData.water` is the sea, `lakes` + `lake_of` the lakes (levels fixed during a game). Never read `map.water` for a position: use `water_at` / `water_xz` / `water_cell`, `water_vertex` (craters), `surface(p)` (ground or water top), `water_near` (looks only). Details at `water_at`.
- **Terrain**: ground hits dig craters (`sim::dig_crater`), `Order::Terraform` reshapes ground; heights change only through `sim::Ground::set`. Details at `dig_crater`.
- **Transports** (`transport: {capacity}`, `cargo` slots): `sim::fits(host, rider)` decides who may board; riders get `Hosted`; orders `Load` / `LoadArea` / `Unload` / `Ferry`. Details at `fits`, `loadout_ok` (loaded carriers), `carry` (ferry).
- **Airdrops** (Swift, `MoveKind::Plane` + `transport`): the plane never lands, releases riders over `drop_spot`s (`Mover.drop`) and scoops up loads in flight. Details at `drop_spot`.
- **Small arms** (damage type `SmallArms`: Trooper and Sniper rifles and every machine gun): every armored class
  takes 0.1 of it (`LightTank`, `MediumTank`); other classes take what they take from `Normal` (Light 1,
  Structure 0.6, Commander 0.5, Air 1, Civilian 0.3). Requested: armour shrugs off rifles and machine guns.
- **Armoured carriers** (`apc`, `apc_amph`, `apc_heavy`, `apc_heavy3`; roster at `amphibious`): `move.amphibious` units float over deep water (`map::OCC_DEEP`, `passable_for`); the zone AI uses `Role::Carrier`.
- **Control points** (`Game.mode == MODE_POINTS`): from `MapData.points` (or `auto_points`); a team holding fewer points than its strongest opponent bleeds (`systems::control_points`).
- **Far meshes** (`view/decimate.rs`, `model_lod_swap`): parts with >= 60 triangles get a vertex-clustered mesh shown at distance.
- **Model LOD** (`model_lod` setting): models are hidden where the HUD draws the strategic icon (`view::as_icon`, `view::model_lod`).
- **Selection**: box takes fighting units first, then builders (`build` present), then buildings.
- **Selection sounds**: `view::sound::select_sound` picks by role, `"select_sound"` in unit JSON overrides.
- Bohdana nerfed on request: 40 s reload, 3 s aim, 10 deg/s traverse, 5 s hold; cost later cut
  from 4500 mass / 40000 energy to 1400 / 12000 (build time 4000).
- Mobile speeds are ~1.25x mod's values (user asked for slightly quicker units).

## Data files

- `assets/data/rules.json` - gravity, start resources, wreck/reclaim/repair rates, radar fade, craters,
  terraforming, control points, armor table (armor -> damage type -> multiplier), colours.
- `assets/data/presets/<name>/` - preset shipped with game (player's own presets, and those kept from host's game, are in `presets/<name>/` and premade maps kept from host's game in `maps/<name>/` of user config folder, `data::user_presets_dir` / `user_maps_dir` / `map_dir`: they survive new versions, and user preset hides shipped one of same name; map editor still saves to `assets/maps`) - own `rules.json` and `factions/*.json`, made and edited in the
  main menu (Game data presets: form view with tooltips, or raw JSON), chosen by host in lobby.
- `assets/data/factions/coalition.json` - playable faction (mod roster).
  `civilian.json` - neutral/hostile map buildings. A new `*.json` here with `"playable": true`
  becomes selectable in lobby.
- `assets/maps/<name>/map.json` + `height.png` (16-bit) + `layerN.png` (8-bit ground layer masks) + `lakes.png` (8-bit, lake number per cell, only with lakes) - premade maps; `props` are pre-placed neutral
  (owner 0), hostile (owner 1) or player (owner 2+k = whoever plays lobby slot k; left out when that slot is empty) units of any faction; `points` are control points.
- Host and clients play with identical data: client whose data hash differs gets host's `DataPack`
  (and asset files built-in data does not refer to) with `S2C::Start`.

## Asset pipeline

`tools/convert_assets.py` reads the mod's `.scm` / `.bp` and writes `assets/models/<name>.glb` (one node per bone, so turret bones can turn), `_albedo` / `_team` / `_normal` PNGs and unit portraits; `.sca` animations become named glTF animations (table `ANIMS`). `models::AnimKind` + `view::drive_anims` play them (`walk`, `upgrade`, `work`, `open`, `doors`, `unpack`, `land`, `water`, `rolloff`; details at `AnimKind`). Wrecks get no animated, aimed or spinning parts. Blender models: `tools/blend.py` (materials team/light/dark/metal: `models::prim_mat`); `ModelDef.brightness` 2.1 default (mod's dark textures), 1 for ours. Our own models: `art/` scripts on `bpy` (`.venv/bin/python art/build.py`; kit, files and animations in `art/kit.py`'s header comment).


## Not done yet / ideas

- No music, navy, air staging or shields. One playable faction (format supports more).
- No friendly-fire by default (`rules.friendly_fire`).
- **AI types** (`ai::AiKind`, per lobby slot): Simple (old), Overseer, Zones, Adaptive, Rusher, Turtle; all but Simple share `ai/brain.rs` and differ by `Profile`. The engine design (zones, bounds, counters, storming, scavengers, garrisons, lifts, landings, build spots, `LR_AIOFF`) is in the comment above `Brain`. Test a change with duels against `simple` on a few seeds (`LR_PLAYERS=2 LR_AI=zones,simple LR_SEED=n`); new types should win all.
- Hostile side only holds positions; it has no AI.
- AI does not terraform or flank on purpose beyond crescent. `Simple` type uses no scavengers,
  transports, garrisons, field works or storming at all.
- Minimap picture and cached fog/radar shadows do not follow craters and terraforming; minimap does not show trees and rocks or scorch marks. Ground layers do not change with craters (only scorch).
- Netplay sends commands only (~20 bytes per tick). Every client holds full game state, so fog of war
  is not cheat-proof. A player who left cannot come back (only rejoining new host after host migration exists).
- Mispredicted effects are not undone: muzzle flash or explosion rollback corrected away has still been shown.
- Balance numbers are first-pass.

