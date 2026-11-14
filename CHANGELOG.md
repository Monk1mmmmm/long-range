# Changelog

Each `## vX.Y.Z` section is used as the text of the GitHub release with that tag
(`scripts/release.sh` / `scripts/release.ps1` pick it up automatically).

## v0.2.1

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
