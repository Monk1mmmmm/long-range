//! Frame-time profiling, view side. Present only when asked for: `LR_PROFILE=1` in a game (a summary
//! line goes to the log every `LOG_EVERY` seconds) and the benchmark (`--bench`, `bench.rs` in the
//! binary). Measurement only: nothing reads it back, it is not part of the simulation state.
//!
//! Where a frame's time goes:
//! - `session`: `main.rs::session_update` (simulation ticks, network, replay recording);
//! - each view system of `ViewPlugin`'s chain (`VIEW_NAMES`), timed by `vmark::<I>` between them;
//! - `hud`: the HUD's egui pass (`hud::ui_system`);
//! - main world: First to Last of the main schedule; render world: its `Render` schedule (CPU,
//!   including waiting for the swap chain); the frame: wall time between two frames;
//! - GPU time per render pass, from Bevy's `RenderDiagnosticsPlugin` (timestamp queries: Vulkan
//!   and DX12 only).
use bevy::diagnostic::DiagnosticsStore;
use bevy::prelude::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// The view systems in the order `ViewPlugin` chains them.
pub const VIEW_NAMES: [&str; 16] = [
    "tick_sync", "terrain_sync", "map_edge_sync", "flora_sync", "update_intel", "unit_visuals", "wreck_visuals", "ghost_visuals", "debris_visuals", "unit_transforms", "drive_anims", "animate_parts", "projectile_visuals", "spawn_fx", "update_fx", "light_follow",
];

/// Sections besides the view systems.
pub const SESSION: usize = VIEW_NAMES.len();
pub const HUD: usize = SESSION + 1;
const N: usize = HUD + 1;

/// Seconds between the log lines of `LR_PROFILE=1`.
pub const LOG_EVERY: f32 = 30.0;

/// Render world `Render` schedule time, nanoseconds, summed (the render world runs apart from the
/// main world: pipelined rendering).
static RENDER_NS: AtomicU64 = AtomicU64::new(0);
static RENDER_FRAMES: AtomicU64 = AtomicU64::new(0);

#[derive(Resource)]
pub struct FrameProfile {
    last: Instant,
    frame_start: Option<Instant>,
    main_start: Instant,
    /// Time per section (`VIEW_NAMES`, then `SESSION`, `HUD`).
    pub sum: [Duration; N],
    /// Main world time (First..Last) per frame, milliseconds.
    pub main_ms: Vec<f32>,
    /// Frame time (between the starts of two frames), milliseconds.
    pub frame_ms: Vec<f32>,
    render0: (u64, u64),
    /// GPU (and CPU) time of every render pass: path -> (sum ms, measurements).
    pub passes: std::collections::BTreeMap<String, (f64, u32)>,
    pass_seen: Option<Instant>,
    /// Since the last log line (`LR_PROFILE=1`).
    pub log_clock: f32,
}

impl Default for FrameProfile {
    fn default() -> Self {
        let now = Instant::now();
        FrameProfile { last: now, frame_start: None, main_start: now, sum: Default::default(), main_ms: vec![], frame_ms: vec![], render0: render_counters(), passes: Default::default(), pass_seen: None, log_clock: 0.0 }
    }
}

fn render_counters() -> (u64, u64) {
    (RENDER_NS.load(Ordering::Relaxed), RENDER_FRAMES.load(Ordering::Relaxed))
}

/// Mean and the value at fraction `q` (0.99: the slowest 1%) of `v`.
pub fn stats(v: &[f32], q: f32) -> (f32, f32) {
    if v.is_empty() {
        return (0.0, 0.0);
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.total_cmp(b));
    let mean = s.iter().sum::<f32>() / s.len() as f32;
    (mean, s[((s.len() - 1) as f32 * q).round() as usize])
}

impl FrameProfile {
    pub fn frames(&self) -> usize {
        self.frame_ms.len()
    }

    /// Adds the time of `section` (used by `session_update` and the HUD marks).
    pub fn add(&mut self, section: usize, d: Duration) {
        self.sum[section] += d;
    }

    /// Render world CPU time per frame since this profile started, ms.
    pub fn render_ms(&self) -> f32 {
        let (ns, n) = render_counters();
        let n = n.saturating_sub(self.render0.1).max(1);
        (ns.saturating_sub(self.render0.0) as f64 / 1e6 / n as f64) as f32
    }

    /// Section time per frame, ms.
    pub fn per_frame(&self, section: usize) -> f32 {
        (self.sum[section].as_secs_f64() * 1000.0 / self.frames().max(1) as f64) as f32
    }

    /// The `n` sections that took longest: (name, ms per frame).
    pub fn top(&self, n: usize) -> Vec<(&'static str, f32)> {
        let mut rows: Vec<(&'static str, f32)> = (0..N).map(|i| (section_name(i), self.per_frame(i))).collect();
        rows.sort_by(|a, b| b.1.total_cmp(&a.1));
        rows.truncate(n);
        rows
    }

    /// GPU time per frame of every pass that reported one, ms, largest first; and their sum
    /// over the top-level passes (those whose path has no parent pass among the others).
    pub fn gpu(&self) -> (f32, Vec<(String, f32)>) {
        let mut rows: Vec<(String, f32)> = self.passes.iter().filter(|(k, _)| k.ends_with("/elapsed_gpu")).map(|(k, (s, n))| (k.trim_start_matches("render/").trim_end_matches("/elapsed_gpu").to_string(), (*s / (*n).max(1) as f64) as f32)).collect();
        let total = rows.iter().filter(|(k, _)| !rows.iter().any(|(o, _)| o != k && k.starts_with(&format!("{o}/")))).map(|r| r.1).sum();
        rows.sort_by(|a, b| b.1.total_cmp(&a.1));
        (total, rows)
    }

    /// One line for the log.
    pub fn line(&self) -> String {
        let (f, f99) = stats(&self.frame_ms, 0.99);
        let (m, _) = stats(&self.main_ms, 0.5);
        let top: Vec<String> = self.top(5).iter().map(|(k, v)| format!("{k} {v:.2}")).collect();
        let (gpu, _) = self.gpu();
        let gpu = if gpu > 0.0 { format!(", gpu {gpu:.2}") } else { String::new() };
        format!("{} frames: {:.1} fps, frame {:.2} ms (1% {:.1}), main {:.2}, render cpu {:.2}{gpu}; {}", self.frames(), 1000.0 / f.max(0.001), f, f99, m, self.render_ms(), top.join(", "))
    }
}

pub fn section_name(i: usize) -> &'static str {
    match i {
        SESSION => "session",
        HUD => "hud",
        _ => VIEW_NAMES[i],
    }
}

/// Between two view systems: the time since the previous mark goes to view system `I - 1`.
pub fn vmark<const I: usize>(p: Option<ResMut<FrameProfile>>) {
    let Some(mut p) = p else { return };
    let now = Instant::now();
    if I > 0 {
        let d = now - p.last;
        p.sum[I - 1] += d;
    }
    p.last = now;
}

fn hud_begin(p: Option<ResMut<FrameProfile>>) {
    if let Some(mut p) = p {
        p.last = Instant::now();
    }
}

fn hud_end(p: Option<ResMut<FrameProfile>>) {
    if let Some(mut p) = p {
        let d = p.last.elapsed();
        p.sum[HUD] += d;
    }
}

fn frame_begin(p: Option<ResMut<FrameProfile>>, store: Option<Res<DiagnosticsStore>>) {
    let Some(mut p) = p else { return };
    let now = Instant::now();
    if let Some(t) = p.frame_start {
        let ms = (now - t).as_secs_f32() * 1000.0;
        p.frame_ms.push(ms);
        p.log_clock += ms / 1000.0;
    }
    p.frame_start = Some(now);
    p.main_start = now;
    // Render passes measured since the last frame (the store keeps a short history).
    if let Some(store) = store {
        let seen = p.pass_seen;
        let mut newest = seen;
        let mut add: Vec<(String, f64)> = vec![];
        for d in store.iter() {
            let path = d.path().as_str();
            if !path.starts_with("render/") {
                continue;
            }
            for m in d.measurements() {
                if seen.map_or(true, |s| m.time > s) {
                    add.push((path.to_string(), m.value));
                    newest = Some(newest.map_or(m.time, |n: Instant| n.max(m.time)));
                }
            }
        }
        // The first look only finds the time to start from.
        if seen.is_some() {
            for (k, v) in add {
                let e = p.passes.entry(k).or_default();
                e.0 += v;
                e.1 += 1;
            }
        }
        p.pass_seen = newest.or(Some(now));
    }
}

fn frame_end(p: Option<ResMut<FrameProfile>>) {
    if let Some(mut p) = p {
        let ms = p.main_start.elapsed().as_secs_f32() * 1000.0;
        p.main_ms.push(ms);
    }
}

/// `LR_PROFILE=1` in a game: a summary line in the log every `LOG_EVERY` seconds, then afresh.
fn log_profile(p: Option<ResMut<FrameProfile>>, bench: Option<Res<Benching>>, sim: Option<ResMut<crate::sim::systems::SimProfile>>) {
    let Some(mut p) = p else { return };
    if bench.is_some() || p.log_clock < LOG_EVERY {
        return;
    }
    let sim = sim.map(|mut s| {
        let line = if s.ticks > 0 { format!("; sim {}", s.short(5)) } else { String::new() };
        *s = Default::default();
        line
    });
    info!("profile: {}{}", p.line(), sim.unwrap_or_default());
    *p = FrameProfile::default();
}

/// Present while the benchmark drives the game: `log_profile` leaves the profile alone.
#[derive(Resource)]
pub struct Benching;

fn epoch_ns() -> u64 {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_nanos() as u64
}

static RENDER_START: AtomicU64 = AtomicU64::new(0);

// The render schedule runs its systems on any thread: the start time goes through an atomic.
fn render_begin() {
    RENDER_START.store(epoch_ns(), Ordering::Relaxed);
}

fn render_end() {
    let t = RENDER_START.load(Ordering::Relaxed);
    if t > 0 {
        RENDER_NS.fetch_add(epoch_ns().saturating_sub(t), Ordering::Relaxed);
        RENDER_FRAMES.fetch_add(1, Ordering::Relaxed);
    }
}

/// Adds the profiling systems (they do nothing while no `FrameProfile` exists) and, when `gpu`,
/// Bevy's render pass timings (`RenderDiagnosticsPlugin`; it costs a little, so only when asked).
pub fn add_profiling(app: &mut App, gpu: bool) {
    app.add_systems(First, frame_begin);
    app.add_systems(Last, (frame_end, log_profile).chain());
    app.add_systems(bevy_egui::EguiPrimaryContextPass, (hud_begin.before(crate::view::hud::ui_system), hud_end.after(crate::view::hud::ui_system)));
    if gpu {
        app.add_plugins(bevy::render::diagnostic::RenderDiagnosticsPlugin);
    }
    if let Some(r) = app.get_sub_app_mut(bevy::render::RenderApp) {
        use bevy::render::{Render, RenderSystems};
        r.add_systems(Render, (render_begin.before(RenderSystems::ExtractCommands), render_end.after(RenderSystems::PostCleanup)));
    }
}
