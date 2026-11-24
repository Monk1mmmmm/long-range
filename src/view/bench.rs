//! The benchmark (`longrange --bench [minutes]`): an AI plays every slot (the local one too, so the
//! view is a player's, with fog and remembered buildings), the game is run flat out to `minutes`
//! (default 25: late game, when it gets slow), then the camera goes to the biggest crowd of units the
//! local player sees and the graphics settings are changed one at a time while the frame time is
//! measured (`perf::FrameProfile`). The simulation is held during those (the same picture for every
//! setting), except in the scenarios marked as running. The report goes to stdout and
//! `bench-<time>.txt` in the user config folder (`settings::base_dir`); the settings file is not touched.
//!
//! `main.rs` starts the game, runs the ticks while `Bench.ff` and stops them while `Bench.hold`.
//! `LR_BENCH_SECS` sets the measuring time of a scenario (default 4).
use super::perf::{self, FrameProfile};
use crate::map::V2;
use crate::settings::Settings;
use crate::sim::*;
use bevy::prelude::*;
use std::fmt::Write;

#[derive(Clone)]
struct Scenario {
    name: &'static str,
    /// Camera distance: close up, battle, overview (strategic icons).
    dist: f32,
    /// The simulation runs at normal speed (else held).
    running: bool,
    set: fn(&mut Settings),
}

fn scenarios() -> Vec<Scenario> {
    let s = |name, dist, running, set| Scenario { name, dist, running, set };
    vec![
        s("current settings", 90.0, false, |_| {}),
        s("current settings, game running", 90.0, true, |_| {}),
        s("shadows off", 90.0, false, |s| s.shadows = 0),
        s("shadows low", 90.0, false, |s| s.shadows = 1),
        s("shadows high", 90.0, false, |s| s.shadows = 2),
        s("antialiasing off", 90.0, false, |s| s.antialiasing = "off".into()),
        s("antialiasing FXAA", 90.0, false, |s| s.antialiasing = "fxaa".into()),
        s("antialiasing MSAA x4", 90.0, false, |s| s.antialiasing = "msaa4".into()),
        s("trees and rocks low", 90.0, false, |s| s.flora_detail = 0),
        s("trees and rocks medium", 90.0, false, |s| s.flora_detail = 1),
        s("trees and rocks high", 90.0, false, |s| s.flora_detail = 2),
        s("effects low", 90.0, false, |s| s.effects = 0),
        s("effects medium", 90.0, false, |s| s.effects = 1),
        s("effects high", 90.0, false, |s| s.effects = 2),
        s("effects low, game running", 90.0, true, |s| s.effects = 0),
        s("effects high, game running", 90.0, true, |s| s.effects = 2),
        s("model LOD off", 90.0, false, |s| s.model_lod = false),
        s("remembered building models off", 90.0, false, |s| s.ghost_models = false),
        s("sky, haze and clouds off", 380.0, false, |s| {
            s.sky = false;
    s.terrain_detail = false;
            s.haze = false;
            s.clouds = false;
        }),
        s("overview: clouds on", 380.0, false, |s| s.clouds = true),
        s("neutral building icons off", 90.0, false, |s| s.neutral_icons = false),
        s("everything lowest", 90.0, false, lowest),
        s("everything highest", 90.0, false, highest),
        s("close up: current", 40.0, false, |_| {}),
        s("close up: everything lowest", 40.0, false, lowest),
        s("overview: current", 380.0, false, |_| {}),
        s("overview: everything lowest", 380.0, false, lowest),
        s("overview: model LOD off", 380.0, false, |s| s.model_lod = false),
        s("overview: current, game running", 380.0, true, |_| {}),
    ]
}

fn lowest(s: &mut Settings) {
    s.shadows = 0;
    s.antialiasing = "off".into();
    s.flora_detail = 0;
    s.effects = 0;
    s.model_lod = true;
    s.ghost_models = false;
    s.sky = false;
    s.haze = false;
    s.clouds = false;
}

fn highest(s: &mut Settings) {
    s.shadows = 2;
    s.antialiasing = "msaa4".into();
    s.flora_detail = 2;
    s.effects = 2;
    s.model_lod = false;
    s.ghost_models = true;
    s.sky = true;
    s.terrain_detail = true;
    s.haze = true;
    s.clouds = true;
}

#[derive(PartialEq, Clone, Copy)]
enum Phase {
    /// Waiting for the game to start.
    Start,
    /// `main.rs` runs ticks flat out until `at_tick`.
    FastForward,
    /// Settling after a change (pipelines compile, meshes rebuild), then measuring.
    Warm(usize),
    Measure(usize),
    Done,
}

#[derive(Resource)]
pub struct Bench {
    pub at_tick: u32,
    /// main.rs: run ticks without waiting for the clock.
    pub ff: bool,
    /// main.rs: do not advance the simulation.
    pub hold: bool,
    phase: Phase,
    clock: f32,
    measure: f32,
    base: Option<Settings>,
    focus: V2,
    report: String,
    rows: Vec<(String, FrameProfile, String)>,
    /// Fast-forward: sim profile lines every 5 game minutes.
    pub ff_lines: Vec<String>,
    started: Option<std::time::Instant>,
}

impl Bench {
    pub fn new(minutes: f32) -> Bench {
        let measure = std::env::var("LR_BENCH_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(4.0f32).max(1.0);
        Bench { at_tick: (minutes.max(0.0) * 600.0) as u32, ff: false, hold: false, phase: Phase::Start, clock: 0.0, measure, base: None, focus: V2::default(), report: String::new(), rows: vec![], ff_lines: vec![], started: None }
    }
}

/// Where most units the local player sees stand (cells of 48).
fn crowd(me: u8, game: &Game, units: &Query<(&Unit, &Body, &Intel)>) -> V2 {
    let team = game.players.get(me as usize).map(|p| p.team);
    let mut cells: std::collections::BTreeMap<(i32, i32), (u32, V2)> = Default::default();
    for (u, b, i) in units.iter() {
        if !team.map_or(true, |t| u.owner == me || i.seen & (1 << t) != 0) {
            continue;
        }
        let c = cells.entry(((b.pos.x / 48.0).floor() as i32, (b.pos.y / 48.0).floor() as i32)).or_insert((0, V2::default()));
        c.0 += 1;
        c.1 = c.1 + b.pos;
    }
    cells.values().max_by_key(|c| c.0).map_or(V2::default(), |c| c.1 * (1.0 / c.0 as f32))
}

#[allow(clippy::too_many_arguments)]
pub fn bench_system(
    mut bench: ResMut<Bench>,
    mut settings: ResMut<Settings>,
    view: Option<ResMut<super::ViewState>>,
    game: Option<Res<Game>>,
    mut profile: ResMut<FrameProfile>,
    sim: Option<ResMut<crate::sim::systems::SimProfile>>,
    time: Res<Time>,
    units: Query<(&Unit, &Body, &Intel)>,
    wrecks: Query<&Wreck>,
    shells: Query<&Projectile>,
    entities: Query<Entity>,
    windows: Query<&Window>,
    adapter: Option<Res<bevy::render::renderer::RenderAdapterInfo>>,
    mut exit: MessageWriter<AppExit>,
) {
    let (Some(mut view), Some(game)) = (view, game) else { return };
    let dt = time.delta_secs();
    bench.clock += dt;
    let list = scenarios();
    let mut sim = sim;
    match bench.phase {
        Phase::Start => {
            bench.started = Some(std::time::Instant::now());
            bench.ff = true;
            bench.phase = Phase::FastForward;
            println!("bench: running the game to {} min", bench.at_tick / 600);
        }
        Phase::FastForward => {
            if game.tick >= bench.at_tick || game.winner.is_some() {
                bench.ff = false;
                bench.hold = true;
                let mut base = settings.clone();
                base.vsync = false;
                bench.base = Some(base);
                if !game.players.get(view.me as usize).map_or(false, |p| p.alive) {
                    view.me = OBSERVER;
                }
                bench.focus = crowd(view.me, &game, &units);
                let wall = bench.started.map_or(0.0, |t| t.elapsed().as_secs_f32());
                let mut r = String::new();
                let _ = writeln!(r, "Long Range benchmark, version {}", env!("CARGO_PKG_VERSION"));
                let _ = writeln!(r, "renderer: {}", adapter.as_ref().map_or("unknown".to_string(), |a| format!("{} ({:?}, driver {} {})", a.name, a.backend, a.driver, a.driver_info)));
                let _ = writeln!(r, "window: {}", windows.iter().next().map_or("?".to_string(), |w| format!("{}x{} (scale {})", w.physical_width(), w.physical_height(), w.scale_factor())));
                let _ = writeln!(r, "game time {}:{:02} (run in {wall:.0} s), {} players alive, view of {}", game.tick / 600, game.tick / 10 % 60, game.players.iter().filter(|p| p.alive).count(), if view.me == OBSERVER { "an observer".to_string() } else { format!("player {}", view.me) });
                let _ = writeln!(r, "units {}, wrecks and debris {}, projectiles {}, entities {}", units.iter().count(), wrecks.iter().count(), shells.iter().count(), entities.iter().count());
                let _ = writeln!(r, "camera at {:.0}, {:.0}; vsync off for the test; each scenario: {} s measured", bench.focus.x, bench.focus.y, bench.measure);
                if !bench.ff_lines.is_empty() {
                    let _ = writeln!(r, "\nsimulation while running the game flat out (ms per tick, the systems that took longest):");
                    for l in &bench.ff_lines {
                        let _ = writeln!(r, "  {l}");
                    }
                }
                bench.report = r;
                start(&mut bench, &list, 0, &mut settings, &mut view);
            }
        }
        Phase::Warm(i) => {
            view.cam_target = bench.focus;
            view.cam_dist = list[i].dist;
            // The first scenario also waits for the pipelines and meshes of the late-game picture.
            let warm = if i == 0 { 6.0 } else if list[i].name.contains("antialiasing") { 3.0 } else { 1.5 };
            if bench.clock >= warm {
                bench.clock = 0.0;
                bench.phase = Phase::Measure(i);
                *profile = FrameProfile::default();
                if let Some(s) = sim.as_mut() {
                    **s = Default::default();
                }
            }
        }
        Phase::Measure(i) => {
            view.cam_target = bench.focus;
            if bench.clock >= bench.measure {
                let sim_line = sim.as_mut().map_or(String::new(), |s| {
                    let l = if s.ticks > 0 { s.short(4) } else { String::new() };
                    **s = Default::default();
                    l
                });
                let p = std::mem::take(&mut *profile);
                println!("bench: {:<34} {}", list[i].name, p.line());
                bench.rows.push((list[i].name.to_string(), p, sim_line));
                if i + 1 < list.len() {
                    start(&mut bench, &list, i + 1, &mut settings, &mut view);
                } else {
                    if let Some(b) = bench.base.clone() {
                        *settings = b;
                    }
                    finish(&mut bench);
                    bench.phase = Phase::Done;
                    exit.write(AppExit::Success);
                }
            }
        }
        Phase::Done => {}
    }
}

fn start(bench: &mut Bench, list: &[Scenario], i: usize, settings: &mut Settings, view: &mut super::ViewState) {
    let mut s = bench.base.clone().unwrap_or_default();
    (list[i].set)(&mut s);
    // Only what differs, so unchanged settings do not trigger change detection.
    if *settings != s {
        *settings = s;
    }
    bench.hold = !list[i].running;
    bench.clock = 0.0;
    bench.phase = Phase::Warm(i);
    view.cam_target = bench.focus;
    view.cam_dist = list[i].dist;
}

fn finish(bench: &mut Bench) {
    let mut r = std::mem::take(&mut bench.report);
    let _ = writeln!(r, "\nframe times in ms (1% = the slowest 1% of frames); main = main world (simulation, view, HUD),");
    let _ = writeln!(r, "render = render world CPU time (includes waiting for the GPU / swap chain), gpu = sum of the timed passes\n");
    let _ = writeln!(r, "{:<34} {:>6} {:>7} {:>6} {:>6} {:>7} {:>6}", "scenario", "fps", "frame", "1%", "main", "render", "gpu");
    for (name, p, _) in &bench.rows {
        let (f, f99) = perf::stats(&p.frame_ms, 0.99);
        let (m, _) = perf::stats(&p.main_ms, 0.5);
        let (gpu, _) = p.gpu();
        let _ = writeln!(r, "{:<34} {:>6.1} {:>7.2} {:>6.1} {:>6.2} {:>7.2} {:>6.2}", name, 1000.0 / f.max(0.001), f, f99, m, p.render_ms(), gpu);
    }
    let _ = writeln!(r, "\nmain world, ms per frame by section (largest five):");
    for (name, p, sim) in &bench.rows {
        let top: Vec<String> = p.top(5).iter().map(|(k, v)| format!("{k} {v:.2}")).collect();
        let _ = writeln!(r, "  {:<34} {}", name, top.join(", "));
        if !sim.is_empty() {
            let _ = writeln!(r, "  {:<34}   sim {}", "", sim);
        }
    }
    let _ = writeln!(r, "\nGPU time of the render passes, ms per frame (Vulkan; on DirectX 12 only with LR_BENCH_GPU=1):");
    for (name, p, _) in &bench.rows {
        let (_, passes) = p.gpu();
        if passes.is_empty() {
            continue;
        }
        let top: Vec<String> = passes.iter().take(7).map(|(k, v)| format!("{k} {v:.2}")).collect();
        let _ = writeln!(r, "  {:<34} {}", name, top.join(", "));
    }
    print!("\n{r}");
    let path = crate::settings::base_dir().join(format!("bench-{}.txt", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())));
    match std::fs::write(&path, &r) {
        Ok(()) => println!("bench: report written to {}", path.display()),
        Err(e) => eprintln!("bench: cannot write {}: {e}", path.display()),
    }
}
