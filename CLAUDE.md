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
cargo run --release
cargo run -- --skirmish           # straight into a game vs 3 AIs
cargo run -- --selftest           # headless 4-AI match, prints economy/tier stats
cargo run -- --combattest         # headless duels: accuracy, reversing, assisted upgrade, radar line of sight
cargo run -- --nettest            # loopback discovery + snapshot + remote command test
cargo run --bin lr-editor         # map editor
cargo run --bin lr-editor -- --generate NAME CELLS SEED   # write a generated map with towns to assets/maps
python3 tools/convert_assets.py "<mod folder>"            # re-import models/textures/portraits from the mod
python3 tools/dump_mod.py "<mod folder>"                  # print blueprint stats (source of the JSON numbers)
```

Debug environment variables: `LR_SPEED=8` (fast-forward a hosted game), `LR_WATCH=1` (an AI also plays the local
slot), `LR_REVEAL=1` (no fog of war), `LR_MAP=<name>` (premade map for `--skirmish`), `LR_DUEL=1` (stages a tank
fight at the first start).

### Testing the window without a desktop

Xvfb cannot present Vulkan frames. What works: a nested headless sway with Xwayland, xdotool for input and grim
for screenshots:
`WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 sway -c <cfg>`, then run the game with `DISPLAY` set to that
session's Xwayland and `WAYLAND_DISPLAY` unset; screenshot with `WAYLAND_DISPLAY=<its socket> grim out.png`.
The first xdotool mouse move lands at the window centre - do a throwaway move before the first click.
`swaymsg seat cursor ...` does not reach clients in headless mode.

## Conventions

- World units = SupCom units (a tank is ~0.7 wide, ranges 15-300). Y is up. Model forward is -Z.
  Unit yaw `a` means facing `(cos a, sin a)` in the XZ plane; `view::yaw_quat` converts to a rotation.
- Build/navigation grid cell = 2 world units (`map::CELL`). Structure footprints are in cells.
- Simulation: 10 ticks/s, host only, in schedule `SimTick` (run manually from `main.rs::session_update`).
  Clients never run it; they apply `Snapshot`s onto the same components (`sim::apply_snapshot`), so every
  view system works identically on host and client. Anything the view needs must be in `UnitSnap`.
- Units refer to each other by network id (`u32`), never by `Entity`.
- ECS layout: one entity per unit with `Unit`, `Body`, `Health`, `Orders`, `Intel` plus optional `Weapons`,
  `Builder`, `Factory`, `Mover` (host only), marker/state components `UnderConstruction`, `Upgrading`,
  `Hosted` (still inside its factory), `Structure`. Projectiles and wrecks are entities too.
- Systems read other units through the per-tick `Census` resource (plain copies + spatial grid) and queue
  cross-entity effects in `Effects` (damage, reclaim) instead of holding conflicting mutable queries.
- All content lives in JSON under `assets/data`. Never hard-code unit stats or unit names in Rust; the AI finds
  units by role (extractor, generator, factory...).
- Owners 250/251 are the neutral and hostile sides (teams 6/7). Neutral units are never auto-targeted.

## Source map

| Path | Contents |
|---|---|
| `src/data.rs` | JSON schema (`UnitDef`, `WeaponDef`, `Rules`...), loading, validation, data hash |
| `src/map.rs` | `V2`/`V3`, heightmap `MapData`, generator, save/load, line of sight, raycast, A* |
| `src/sim/mod.rs` | components, resources, `Order`/`Command`, spawning, snapshots |
| `src/sim/systems.rs` | tick systems: commands, visibility, economy, orders, factories, build, movement, weapons, projectiles, deaths |
| `src/ai.rs` | skirmish AI and formation assignment |
| `src/net.rs` | TCP host/client, UDP LAN discovery |
| `src/view/mod.rs` | view state, unit visuals, interpolation, turret animation |
| `src/view/hud.rs` | camera, selection, orders, overlays, HUD panels (one egui pass) |
| `src/view/intel.rs` | fog grid, terrain-shadowed radar coverage, contact memory (ghosts/blips) |
| `src/view/models.rs` | GLB/box model loading, team-colour texture baking |
| `src/view/terrain.rs` | terrain chunks, water, deposits, sun, minimap pixels |
| `src/view/fx.rs` | projectile visuals and explosions |
| `src/view/icons.rs` | procedural strategic icons |
| `src/main.rs` | menu, lobby, host/client session loop, headless tests |
| `src/bin/lr-editor.rs` | map editor and map generation CLI |

## Game rules worth knowing

- **Ballistics**: every shot is a projectile entity with 3D position/velocity under gravity
  (`rules.json: gravity`). Weapon kinds: `direct` (needs clear line of fire, leads the target), `arc`,
  `high_arc` (indirect, raise their charge to reach), `missile` (homing, limited turn rate), `bomb`.
  Hits are resolved against terrain and unit cylinders, so shots miss, fall short or hit hills.
  Guns have turret traverse speed, `aim_time` (must hold on target) and `spread`; salvos fire bursts.
- **Spotting**: weapons only engage targets their team sees or has on radar. Radar needs line of sight
  over terrain (`map.los`); vision does not.
- **Contact memory** (client side, `intel.rs`): radar blips of mobile units fade over `radar_fade_secs`;
  buildings once detected stay as ghosts until the spot is seen again and found empty. Right-clicking a
  ghost issues ground fire at its position.
- **Tiers**: `tier` on a unit, `build.tier` on a builder. Factories and pumps upgrade in place (`upgrade`).
  Assignment follows the mod's blueprint TechLevel where it differs from Basic (engineers, factories, pumps,
  gunship, solar); the mod's all-T1 custom land roster was spread out: T1 Buggy/Trooper/Overtaker/Pike/MBFAG/
  radar car, T2 Crony/Dragoon/Robin/Sparky, T3 Bohdana. Change `tier` in JSON to move a unit.
- **Engineering station** (`build.station`): immobile builder that assists anything in range by itself.
- **Movement**: land units accelerate/brake and choose between driving forward and reversing by comparing
  turn time + travel time for both (`systems.rs::locomotion`).
- **Icons**: `icon: {shape, glyphs}` plus tier pips; the loader rejects two units with the same icon.
  Glyphs: plus = direct fire, dot = artillery, arc = anti-air, chevron = missile, ring = scout, wave = radar,
  eng, mass, bolt, frame, bar, land, tri.
- Bohdana was nerfed on request: 4500 mass / 40000 energy, 40 s reload, 3 s aim, 10 deg/s traverse.
- Mobile speeds are roughly 1.25x the mod's values (user asked for slightly quicker units).

## Data files

- `assets/data/rules.json` - gravity, start resources, wreck/reclaim/repair rates, radar fade, armor table
  (armor -> damage type -> multiplier), colours.
- `assets/data/factions/coalition.json` - the playable faction (the mod roster).
  `civilian.json` - neutral/hostile map buildings (box models). A new `*.json` here with `"playable": true`
  becomes selectable in the lobby.
- `assets/maps/<name>/map.json` + `height.png` (16-bit) - premade maps; `props` are pre-placed neutral
  (owner 0) or hostile (owner 1) units of any faction.
- Host and clients must have identical data: a hash of these files is checked on join.

## Asset pipeline

`tools/convert_assets.py` reads the mod's `.scm` models (`tools/scm.py`) and `.bp` blueprints (`tools/bp.py`)
and writes `assets/models/<name>.glb` (one node per bone, so turret/barrel bones named in the weapon's
`turret` block can be rotated), `<name>_albedo.png`, `<name>_team.png` (team mask from SpecTeam alpha),
`<name>_normal.png`, and `assets/icons/units/<name>.png` portraits. The mod ships no commander or radar tower
model: the commander reuses the trooper mesh at 3x scale, the radar tower is a JSON box model.

## Not done yet / ideas

- No sound, navy, transports, air staging or shields. One playable faction (the format supports more).
- Vision is not blocked by terrain (only radar is). No friendly-fire by default (`rules.friendly_fire`).
- The hostile side only holds positions; it has no AI. Neutral buildings cannot be captured.
- Snapshots are full state every tick (fine on LAN, ~30 KB at 250 units); no delta compression.
- Balance numbers are first-pass.
