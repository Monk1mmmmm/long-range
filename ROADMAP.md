# Roadmap to v0.4

Working list for the v0.4 update. Milestones are done in order; each one ends with the crate compiling,
`--nettest` and `--combattest` passing, and a short checklist of things to try by hand.
Status: `[ ]` open, `[x]` done (code written and headless-tested), `[~]` partly done, see note.

## M1 - Fixes and small features

- [x] Nickname is saved in `settings.json` and restored at the next start.
- [x] Anti-air weapon range rings are blue, not red.
- [x] Factory hit points rebalanced (T3 at 16k is far too much).
- [x] Strategic icons no longer flicker when they overlap (stable draw order).
- [x] An extractor order in the fog goes through even if the deposit turns out to be taken.
- [x] Setting: bigger strategic icons (like FAF's "Bigger strategic icons").
- [x] Hovering an enemy unit shows what it is and its hit points.
- [~] Units fire on their own at radar blips outside vision (reported bug). Could not reproduce: in the new
  `--combattest` scenario every unit that out-ranges its sight shoots at a radar-only target. Likely what was
  seen: a remembered (faded) blip that is no longer on radar, a target beyond radar range, or a direct-fire gun
  with a hill in the way. Needs a concrete case.
- [x] Infantry does not show on radar (`radar_stealth` in the unit JSON).
- [x] Vision is blocked by terrain, like radar.

## M2 - HUD

- [x] Icons for mass, energy, time and build power, used everywhere the HUD mentions them.
- [x] Idle engineers button (top right) with a count; click selects them, like SupCom.
- [x] Factory queue as a row of unit icons (FAF style) that can be reordered by dragging.

## M3 - Building and reclaim

- [x] Shift-drag places a line of buildings at any angle, not only along the grid axes.
- [x] Ctrl-drag while placing fills a block of buildings (like BAR); Shift still means "queue".
- [x] Wrecks hold a share of the mass that actually went into the unit (partly built = less).
- [x] Ctrl+Shift with no building selected shows how much mass lies around, summed per spot.

## M4 - Capture and scavengers

- [x] Engineers can capture neutral and enemy units (SupCom style, takes time and energy).
- [x] Scavenger unit: can only reclaim wrecks and revive them as units for its owner.
  The AI does not build or use scavengers yet.

## M5 - Presentation

- [x] Camera: hold Space and move the mouse to rotate/tilt, like SupCom.
- [x] Map edge: something nicer than the void beyond the map border.
- [x] Sound effects for shots (different per weapon kind), with volume settings.
  Sounds are synthesised at start-up; `assets/sounds/<name>.wav` or `.ogg` replaces one (names: cannon, mg,
  artillery, missile, bomb, blast, death), and a weapon can pick one with a `sound` entry in its JSON.
  Linux builds need `sudo apt install libasound2-dev` once; without it `release.sh` builds Linux without sound.

## M6 - Terrain changes

- [ ] Shell impacts leave craters (Zero-K style), part of the simulation state.
- [ ] Terraforming orders for engineers (raise, lower, level).

## M7 - Transports

- [ ] Transport plane and transport APC.
- [ ] Load/unload UI with a draggable circle, like BAR/Zero-K.

## M8 - Game data presets

- [ ] Presets: named copies of the game data that override the built-in values, stored in an `assets/data` folder.
- [ ] Menu to add, edit and delete presets; the host's preset and relevant assets is sent to the clients at game start.

## M9 - Control points mode

- [ ] Alternative game mode: capture positions, hold them to gain points, first to the limit wins.

## Open questions

- Ctrl+Shift is asked for twice (building grid, mass overlay). Plan: grid while a building is being
  placed, mass overlay otherwise.

## Test checklist

Filled in as milestones are finished.

### M1
- [ ] Change the name in the main menu, restart the game: the name is still there.
- [ ] Select an AA turret or Sparky: its range ring is blue; a tank's is red.
- [ ] Zoom out over a crowded base: overlapping strategic icons no longer flicker.
- [ ] Settings -> Strategic icons -> Bigger: icons on the map grow.
- [ ] Hover an enemy unit in sight: name and hit points appear under it; a radar blip says "Unknown contact".
- [ ] Queue an extractor on a deposit in the fog that the enemy already took: the order is accepted.
- [ ] Stand a unit behind a hill: it does not reveal what is on the other side, and the fog overlay has a
      shadow there. Planes see over hills.
- [ ] Enemy Troopers do not appear as radar blips; other units do.
- [ ] T3 factories die noticeably faster than before (6000 hp instead of 16000).

### M2
- [ ] Top bar shows a hexagon for mass and a bolt for energy; build buttons, tooltips and the unit info line
      use the same icons (plus a clock for build time and a wrench for build power).
- [ ] With an engineer standing idle, a button with a count appears under the player list (top right).
      Left-click selects all idle engineers, right-click jumps to the next one.
- [ ] Select a factory and queue several different units: a strip of icons appears above the panel in build
      order, with a progress bar on the first. Drag an icon to another place: the order changes.
      Right-click an icon: one unit is removed. Dragging something else to the front scraps the unit in progress.
- [ ] Settings -> Strategic icons -> Bigger also enlarges the small icon in the corner of build buttons.

### M3
- [ ] Pick a building and drag diagonally: the row follows the drag direction instead of snapping to an axis.
- [ ] Hold Ctrl while dragging: a filled block of buildings is previewed and queued.
- [ ] Kill a half-built structure or unit: it leaves a smaller wreck; reclaiming it gives mass.
- [ ] Hold Ctrl+Shift with nothing being placed: mass totals appear over groups of wrecks.

### M4
- [ ] Select an engineer, right-click a neutral building (maps with towns): a purple bar fills on the building
      and it becomes yours. The Capture button (flag icon) does the same for enemy units; commanders and
      aircraft cannot be captured.
- [ ] Stop capturing half-way: the purple bar slowly drains.
- [ ] Build a Scavenger in the land factory. Right-click a wreck: it turns into a construction site and comes
      back as a working unit of yours (costs the mass the wreck was missing).
- [ ] A Scavenger can reclaim (E / Q) but cannot build, assist or capture.

### M5
- [ ] Hold Space and move the mouse: the camera turns and tilts. Tap Space: it returns to the default angle.
      Arrow keys and middle-mouse panning still follow the screen after turning.
- [ ] Zoom out at the map border: the land continues, dimmed, and slopes into a sea; no more black void.
- [ ] Shots and explosions make sounds (Windows build): machine guns, cannons, artillery and missiles differ,
      and distant fights are quieter. Settings -> Sound effects changes the volume; 0 is silent.
- [ ] Put a `cannon.wav` into `assets/sounds/`: tank guns use it after a restart.
