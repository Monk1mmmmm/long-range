//! In-game UI in one egui pass: camera, selection and order input, world overlays
//! (icons, bars, blips) and the HUD panels. Everything is painted by hand on egui layers.
use super::icons;
use super::intel::{FogGrid, IntelMemory};
use super::terrain::{minimap_pixels, MainCamera};
use super::{Interp, LocalCommands, Mode, ViewState};
use crate::ai::assign;
use crate::data::{DefId, Defs, UnitDef};
use crate::map::*;
use crate::sim::*;
use crate::sim::Command;
use bevy::ecs::system::SystemParam;
use bevy::input::mouse::MouseWheel;
use bevy::prelude::*;
use bevy_egui::egui::{self, Align2, Color32, FontId, Pos2, Stroke, StrokeKind, TextureHandle};
use bevy_egui::EguiContexts;
use std::collections::HashMap;

pub const FOV: f32 = 0.785;
const TOP: f32 = 30.0;
const MM: f32 = 200.0;
const PANEL_H: f32 = 150.0;

#[derive(Resource, Default)]
pub struct HudAssets {
    icons: HashMap<DefId, TextureHandle>,
    portraits: HashMap<DefId, Option<TextureHandle>>,
    blip: Option<TextureHandle>,
    orders: HashMap<&'static str, TextureHandle>,
    minimap: Option<TextureHandle>,
}

impl HudAssets {
    fn icon(&mut self, ctx: &egui::Context, d: &UnitDef) -> TextureHandle {
        self.icons
            .entry(d.idx)
            .or_insert_with(|| {
                let px = icons::render(&d.icon, d.tier);
                ctx.load_texture(format!("icon_{}", d.id), egui::ColorImage::from_rgba_unmultiplied([icons::ICON, icons::ICON], &px), egui::TextureOptions::LINEAR)
            })
            .clone()
    }
    /// Textures of the order-button icons, created on first use.
    fn order_icons(&mut self, ctx: &egui::Context) -> HashMap<&'static str, TextureHandle> {
        if self.orders.is_empty() {
            for k in ["move", "attack_move", "patrol", "ground", "reclaim", "stop", "pause", "resume", "repeat", "upgrade", "assist", "mass", "energy", "time", "power", "capture", "revive", "terraform", "load", "unload", "flag"] {
                let px = icons::render_order(k);
                self.orders.insert(k, ctx.load_texture(format!("order_{k}"), egui::ColorImage::from_rgba_unmultiplied([icons::ICON, icons::ICON], &px), egui::TextureOptions::LINEAR));
            }
        }
        self.orders.clone()
    }
    fn portrait(&mut self, ctx: &egui::Context, d: &UnitDef) -> Option<TextureHandle> {
        self.portraits
            .entry(d.idx)
            .or_insert_with(|| {
                let p = d.portrait.as_ref()?;
                let img = image::open(crate::data::asset_root().join(p)).ok()?.into_rgba8();
                Some(ctx.load_texture(format!("portrait_{}", d.id), egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()), egui::TextureOptions::LINEAR))
            })
            .clone()
    }
}

/// Plain per-frame copy of a unit's state, so UI code does not juggle ECS borrows.
#[derive(Clone)]
struct Info {
    id: u32,
    def: DefId,
    owner: u8,
    pos: V2,
    y: f32,
    hp: f32,
    built: Option<f32>,
    upg: Option<f32>,
    orders: Vec<Order>,
    weapons: Vec<WeaponState>,
    factory: Option<Factory>,
    work: u32,
    /// In direct sight of the local team (not just on radar).
    seen: bool,
    paused: bool,
    /// Progress of somebody's capture of this unit.
    capture: Option<f32>,
    /// The factory or transport the unit is inside (0 = none).
    host: u32,
}

#[derive(Clone, Copy, PartialEq)]
enum Contact {
    /// In sight right now.
    Visible,
    /// On radar right now.
    Radar,
    /// Remembered position, not currently detected.
    Stale,
}

struct Seen {
    id: u32,
    def: Option<DefId>,
    owner: u8,
    pos: V2,
    sp: Pos2,
    on_screen: bool,
    rpx: f32,
    contact: Contact,
    mine: bool,
    enemy: bool,
    fade: f32,
}

#[derive(Clone, Copy, PartialEq)]
enum MoveKind {
    Move,
    AttackMove,
    Patrol,
}

#[derive(SystemParam)]
pub struct UiParams<'w, 's> {
    contexts: EguiContexts<'w, 's>,
    defs: Res<'w, Defs>,
    game: Res<'w, Game>,
    map: Res<'w, MapData>,
    view: ResMut<'w, ViewState>,
    out: ResMut<'w, LocalCommands>,
    assets: ResMut<'w, HudAssets>,
    mem: Res<'w, IntelMemory>,
    fog: Res<'w, FogGrid>,
    time: Res<'w, Time>,
    keys: Res<'w, ButtonInput<KeyCode>>,
    mouse: Res<'w, ButtonInput<MouseButton>>,
    wheel: MessageReader<'w, 's, MouseWheel>,
    gizmos: Gizmos<'w, 's>,
    #[allow(clippy::type_complexity)]
    units: Query<'w, 's, (&'static Unit, &'static Interp, &'static Health, &'static Intel, &'static Orders, Option<&'static Weapons>, Option<&'static Factory>, Option<&'static Builder>, Option<&'static UnderConstruction>, Option<&'static Upgrading>, Option<&'static Hosted>, Has<Paused>, Option<&'static Capture>)>,
    ui_sounds: ResMut<'w, super::sound::UiSounds>,
    wrecks: Query<'w, 's, &'static Wreck>,
    features: Res<'w, Features>,
    camera: Query<'w, 's, (&'static mut Transform, &'static mut Projection), With<MainCamera>>,
    settings: ResMut<'w, crate::settings::Settings>,
    started: Res<'w, crate::settings::StartSettings>,
    render_errors: Res<'w, crate::settings::RenderErrors>,
    adapter: Option<Res<'w, bevy::render::renderer::RenderAdapterInfo>>,
}

fn rgb(c: [f32; 3]) -> Color32 {
    Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
}
fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}
fn bcol(c: Color32) -> Color {
    Color::srgba_u8(c.r(), c.g(), c.b(), c.a())
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> egui::Rect {
    egui::Rect::from_min_size(Pos2::new(x, y), egui::vec2(w, h))
}
const UV: egui::Rect = egui::Rect { min: Pos2 { x: 0.0, y: 0.0 }, max: Pos2 { x: 1.0, y: 1.0 } };

fn path_len(p: &[V2]) -> f32 {
    p.windows(2).map(|w| w[0].dist(w[1])).sum()
}

/// `n` evenly spaced points (with tangents) along a polyline.
fn resample(path: &[V2], n: usize) -> Vec<(V2, V2)> {
    let total = path_len(path);
    if path.len() < 2 || total < 0.01 {
        return vec![(path[0], v2(1.0, 0.0)); n];
    }
    let mut out = vec![];
    for k in 0..n {
        let want = if n > 1 { total * k as f32 / (n - 1) as f32 } else { total * 0.5 };
        let mut acc = 0.0;
        let mut res = (*path.last().unwrap(), (path[path.len() - 1] - path[path.len() - 2]).norm());
        for w in path.windows(2) {
            let l = w[0].dist(w[1]);
            if acc + l >= want && l > 0.0 {
                res = (w[0].lerp(w[1], (want - acc) / l), (w[1] - w[0]).norm());
                break;
            }
            acc += l;
        }
        out.push(res);
    }
    out
}

/// Everything the input and drawing helpers need, bundled.
struct Frame<'a> {
    defs: &'a Defs,
    game: &'a Game,
    map: &'a MapData,
    infos: &'a HashMap<u32, Info>,
    seen: &'a [Seen],
    team: u8,
    sw: f32,
    sh: f32,
    mouse: Pos2,
}

impl Frame<'_> {
    fn selected<'b>(&'b self, view: &ViewState) -> Vec<&'b Info> {
        view.sel.iter().filter_map(|id| self.infos.get(id)).filter(|u| u.owner == view.me).collect()
    }
    fn d(&self, id: DefId) -> &UnitDef {
        self.defs.get(id)
    }

    fn formation(&self, units: &[&Info], path: &[V2]) -> Vec<V2> {
        let n = units.len();
        if n == 0 {
            return vec![];
        }
        let maxr = units.iter().map(|u| self.d(u.def).radius()).fold(0.0, f32::max);
        let spacing = maxr * 2.0 + 1.0;
        let len = path_len(path);
        let mut pts = vec![];
        if len < 2.5 || path.len() < 2 {
            let c = path[0];
            if n == 1 {
                return vec![c];
            }
            let centroid = units.iter().fold(v2(0.0, 0.0), |a, u| a + u.pos) * (1.0 / n as f32);
            let fwd = if c.dist(centroid) > 1.0 { (c - centroid).norm() } else { v2(0.0, -1.0) };
            let side = fwd.perp();
            let cols = ((n as f32).sqrt() * 1.35).ceil().max(1.0) as usize;
            let rows = (n + cols - 1) / cols;
            for k in 0..n {
                let (col, row) = (k % cols, k / cols);
                let in_row = if row == rows - 1 { n - row * cols } else { cols };
                let x = (col as f32 - (in_row as f32 - 1.0) * 0.5) * spacing;
                let y = ((rows as f32 - 1.0) * 0.5 - row as f32) * spacing;
                pts.push(self.map.clamp(c + side * x + fwd * y));
            }
        } else {
            let per_rank = ((len / spacing).floor() as usize + 1).clamp(1, n);
            let ranks = (n + per_rank - 1) / per_rank;
            let mut left = n;
            for r in 0..ranks {
                let m = left.min(per_rank);
                left -= m;
                for (p, tan) in resample(path, m) {
                    pts.push(self.map.clamp(p - tan.perp() * (r as f32 * spacing)));
                }
            }
        }
        pts
    }

    /// Move-type orders with BAR-like spreading: a drawn line distributes the units along it,
    /// a plain click arranges them in a loose block.
    fn order_move(&self, view: &ViewState, path: &[V2], kind: MoveKind, queue: bool, out: &mut Vec<Command>) {
        let units = self.selected(view);
        let mk = |p: V2| match kind {
            MoveKind::Move => Order::Move(p),
            MoveKind::AttackMove => Order::AttackMove(p),
            MoveKind::Patrol => Order::Patrol(p),
        };
        let end = *path.last().unwrap();
        let rally: Vec<u32> = units.iter().filter(|u| self.d(u.def).is_factory()).map(|u| u.id).collect();
        if !rally.is_empty() {
            out.push(Command::Orders { ids: rally, orders: vec![Order::Move(end)], queue: false });
        }
        let planes: Vec<u32> = units.iter().filter(|u| self.d(u.def).is_plane()).map(|u| u.id).collect();
        if !planes.is_empty() {
            out.push(Command::Orders { ids: planes, orders: vec![mk(end)], queue });
        }
        let movers: Vec<&Info> = units.iter().copied().filter(|u| !self.d(u.def).is_structure() && !self.d(u.def).is_plane()).collect();
        if movers.is_empty() {
            return;
        }
        let pts = self.formation(&movers, path);
        let from: Vec<V2> = movers
            .iter()
            .map(|u| {
                if queue {
                    for o in u.orders.iter().rev() {
                        if let Order::Move(p) | Order::AttackMove(p) | Order::Patrol(p) | Order::Build { pos: p, .. } = o {
                            return *p;
                        }
                    }
                }
                u.pos
            })
            .collect();
        let asg = assign(&from, &pts);
        out.push(Command::Orders { ids: movers.iter().map(|u| u.id).collect(), orders: asg.iter().map(|k| mk(pts[*k])).collect(), queue });
    }

    /// Positions for a dragged row of buildings (at any angle), a dragged block of them (`grid`),
    /// or every free deposit in a dragged circle.
    fn build_spots(&self, d: DefId, a: V2, b: V2, occ: &[u8], from: V2, grid: bool) -> Vec<V2> {
        let df = self.d(d);
        let a_s = snap(self.defs, self.map, d, a);
        if df.needs_deposit {
            let r = a.dist(b);
            if r < 3.0 {
                return vec![a_s];
            }
            // Area extractor command: all free deposits in the circle, visited nearest-first.
            let mut left: Vec<V2> = self.map.deposits.iter().copied().filter(|dp| dp.dist(a) <= r && can_place(self.defs, self.map, occ, d, *dp)).collect();
            let mut out = vec![];
            let mut cur = from;
            while !left.is_empty() {
                let (k, _) = left.iter().enumerate().min_by(|x, y| x.1.dist(cur).partial_cmp(&y.1.dist(cur)).unwrap()).unwrap();
                cur = left.swap_remove(k);
                out.push(cur);
            }
            return out;
        }
        let step = df.footprint as f32 * CELL;
        let mut out = vec![a_s];
        let delta = b - a;
        if grid {
            // A filled block from corner to corner, walked in rows that alternate direction so the
            // builder does not run back to the start of every row.
            let (nx, ny) = (((delta.x.abs() / step) as usize).min(11), ((delta.y.abs() / step) as usize).min(11));
            for j in 0..=ny {
                for i in 0..=nx {
                    let i = if j % 2 == 0 { i } else { nx - i };
                    let p = snap(self.defs, self.map, d, a_s + v2(i as f32 * step * delta.x.signum(), j as f32 * step * delta.y.signum()));
                    if !out.contains(&p) {
                        out.push(p);
                    }
                }
            }
            return out;
        }
        // A line in the dragged direction. One building per footprint along the longer axis, so
        // neighbours touch (straight) or step diagonally, and never overlap.
        let major = delta.x.abs().max(delta.y.abs());
        if major < step {
            return out;
        }
        let n = (major / step).floor() as usize;
        let per = delta * (step / major);
        for k in 1..=n.min(40) {
            let p = snap(self.defs, self.map, d, a_s + per * k as f32);
            if !out.contains(&p) {
                out.push(p);
            }
        }
        out
    }

    fn hover(&self, over_hud: bool) -> Option<usize> {
        if over_hud {
            return None;
        }
        let mut best: Option<(f32, usize)> = None;
        for (k, s) in self.seen.iter().enumerate() {
            if !s.on_screen {
                continue;
            }
            let d = s.sp.distance(self.mouse);
            if d < s.rpx.max(11.0) && best.map_or(true, |b| d < b.0) {
                best = Some((d, k));
            }
        }
        best.map(|b| b.1)
    }
}

/// FAF-style hotbuild class of a unit: which key builds it in the alternative preset.
fn hot_key(d: &UnitDef) -> Option<KeyCode> {
    use crate::data::WeaponKind as K;
    if d.is_station() {
        Some(KeyCode::KeyM)
    } else if d.is_factory() || d.is_engineer() {
        Some(KeyCode::KeyZ)
    } else if d.needs_deposit || d.eco.store_mass > 0.0 {
        Some(KeyCode::KeyX)
    } else if d.eco.energy > 0.0 || d.eco.store_energy > 0.0 {
        Some(KeyCode::KeyC)
    } else if let Some(w) = d.weapons.first() {
        Some(if w.hits_air && !w.hits_land {
            KeyCode::KeyD
        } else {
            match w.kind {
                K::Missile => KeyCode::KeyY,
                K::Arc | K::HighArc | K::Bomb => KeyCode::KeyS,
                K::Direct => KeyCode::KeyG,
            }
        })
    } else if d.radar > 0.0 || d.vision >= 45.0 {
        Some(KeyCode::KeyV)
    } else {
        None
    }
}

fn key_letter(k: KeyCode) -> &'static str {
    match k {
        KeyCode::KeyZ => "Z",
        KeyCode::KeyX => "X",
        KeyCode::KeyC => "C",
        KeyCode::KeyV => "V",
        KeyCode::KeyD => "D",
        KeyCode::KeyS => "S",
        KeyCode::KeyG => "G",
        KeyCode::KeyM => "M",
        KeyCode::KeyY => "Y",
        _ => "",
    }
}

/// Local occupancy grid for placement previews (works on clients too, which have no `Nav`).
/// Where the local player believes buildings stand: their team's own, and enemy ones they have
/// detected. A spot the enemy has taken unseen still looks free, so the order can be given.
fn local_occ(map: &MapData, defs: &Defs, infos: &HashMap<u32, Info>, game: &Game, team: u8, mem: &IntelMemory, reveal: bool) -> Vec<u8> {
    let mut occ = map.blocked.clone();
    let own = infos.values().filter(|u| reveal || game.team_of(u.owner) == team).map(|u| (u.def, u.pos));
    let known = mem.ghosts.values().filter(|g| g.structure).filter_map(|g| g.def.map(|d| (d, g.pos)));
    for (def, pos) in own.chain(known) {
        let f = defs.get(def).footprint;
        if f > 0 {
            let (x0, y0) = foot_origin(pos, f);
            for y in y0..y0 + f {
                for x in x0..x0 + f {
                    if map.inside(x, y) {
                        occ[y as usize * map.n + x as usize] = 2;
                    }
                }
            }
        }
    }
    occ
}

/// Weapon range circles of a unit: red for the longest gun that hits ground targets, blue for the
/// longest that only hits aircraft.
fn range_rings(d: &UnitDef, alpha: f32) -> Vec<(f32, Color)> {
    let longest = |air_only: bool| d.weapons.iter().filter(|w| (w.hits_air && !w.hits_land) == air_only).map(|w| w.range).fold(0.0, f32::max);
    let mut out = vec![];
    if longest(false) > 0.0 {
        out.push((longest(false), Color::srgba(1.0, 0.35, 0.3, alpha)));
    }
    if longest(true) > 0.0 {
        out.push((longest(true), Color::srgba(0.25, 0.45, 1.0, alpha)));
    }
    out
}

/// A production queue as runs of the same unit: (unit, how many in a row).
fn queue_runs(queue: &[DefId]) -> Vec<(DefId, usize)> {
    let mut runs: Vec<(DefId, usize)> = vec![];
    for d in queue {
        match runs.last_mut() {
            Some(r) if r.0 == *d => r.1 += 1,
            _ => runs.push((*d, 1)),
        }
    }
    runs
}

/// Width of one entry in the factory queue strip.
const QUEUE_W: f32 = 46.0;
/// Height of the factory queue strip above the selection panel.
const QUEUE_H: f32 = 46.0;
/// Placeholder line in a tooltip where the cost icons go.
const COST_ROW: &str = "\u{1}cost";

/// A terrain-hugging circle drawn with gizmo lines.
fn ring(g: &mut Gizmos, map: &MapData, c: V2, r: f32, col: Color) {
    let n = (20.0 + r * 0.8).min(120.0) as usize;
    let pt = |k: usize| {
        let a = k as f32 / n as f32 * std::f32::consts::TAU;
        let p = map.clamp(c + V2::from_angle(a) * r);
        p.at(map.h(p).max(map.water) + 0.25)
    };
    g.linestrip((0..=n).map(pt), col);
}

fn ground_line(g: &mut Gizmos, map: &MapData, a: V2, b: V2, col: Color) {
    let n = ((a.dist(b) / 6.0).ceil() as usize).clamp(1, 60);
    g.linestrip(
        (0..=n).map(|k| {
            let p = a.lerp(b, k as f32 / n as f32);
            p.at(map.h(p).max(map.water) + 0.3)
        }),
        col,
    );
}

pub fn ui_system(mut p: UiParams) -> Result {
    let ctx = p.contexts.ctx_mut()?.clone();
    let (defs, game, map, features) = (&*p.defs, &*p.game, &*p.map, &*p.features);
    let view = &mut *p.view;
    let me = view.me;
    let Some(player) = game.players.get(me as usize) else { return Ok(()) };
    let team = player.team;
    let reveal = super::intel::reveal(game, me);
    let screen = ctx.content_rect();
    let (sw, sh) = (screen.width().max(1.0), screen.height().max(1.0));
    let mouse = ctx.pointer_latest_pos().unwrap_or(Pos2::new(sw * 0.5, sh * 0.5));
    let now = p.time.elapsed_secs_f64();
    let dt = p.time.delta_secs().min(0.1);
    let shift = p.keys.pressed(KeyCode::ShiftLeft) || p.keys.pressed(KeyCode::ShiftRight);
    let ctrl = p.keys.pressed(KeyCode::ControlLeft) || p.keys.pressed(KeyCode::ControlRight);
    let alt = p.keys.pressed(KeyCode::AltLeft) || p.keys.pressed(KeyCode::AltRight);

    // ---------------------------------------------------------------- per-frame unit data
    let mut infos: HashMap<u32, Info> = HashMap::with_capacity(512);
    let mut hosted: Vec<u32> = vec![];
    for (u, i, h, intel, orders, w, f, b, uc, up, host, paused, cap) in &p.units {
        let known = reveal || game.team_of(u.owner) == team || (intel.seen | intel.radar) & (1 << team) != 0;
        let is_hosted = host.is_some();
        if is_hosted {
            hosted.push(u.id);
        }
        if !known && !is_hosted {
            continue;
        }
        let own = u.owner == me;
        infos.insert(
            u.id,
            Info {
                id: u.id,
                def: u.def,
                owner: u.owner,
                pos: i.pos,
                y: i.y,
                hp: h.0,
                built: uc.map(|x| x.0),
                upg: up.map(|x| x.0),
                orders: if own { orders.0.clone() } else { vec![] },
                weapons: w.map(|w| w.list.clone()).unwrap_or_default(),
                factory: if own { f.cloned() } else { None },
                work: b.map_or(0, |b| b.work),
                seen: reveal || intel.seen & (1 << team) != 0,
                paused,
                capture: cap.map(|c| c.progress),
                host: host.map_or(0, |h| h.0),
            },
        );
    }
    // Units inside a transport cannot be selected until they are set down.
    view.sel.retain(|id| infos.get(id).map_or(false, |u| u.owner == me && (u.host == 0 || u.built.is_some())));
    let wrecks: Vec<(u32, V2, f32)> = p.wrecks.iter().map(|w| (w.id, w.pos, w.mass)).collect();

    // ---------------------------------------------------------------- HUD geometry
    let mm = rect(8.0, sh - MM - 8.0, MM, MM);
    let panel = rect(MM + 18.0, sh - PANEL_H - 8.0, sw - MM - 26.0, PANEL_H);
    // Idle engineers (button in the top right corner; the commander is not counted).
    let idle: Vec<(u32, V2)> = {
        let com = game.players.get(me as usize).and_then(|pl| defs.factions[pl.faction as usize].commander);
        let mut v: Vec<(u32, V2)> = infos.values().filter(|u| u.owner == me && u.built.is_none() && u.host == 0 && u.orders.is_empty() && u.work == 0 && Some(u.def) != com && defs.get(u.def).is_engineer()).map(|u| (u.id, u.pos)).collect();
        v.sort_by_key(|x| x.0);
        v
    };
    let idle_r = rect(sw - 56.0, TOP + 12.0 + game.players.len() as f32 * 16.0, 48.0, 48.0);
    // The production queue of a selected factory is a strip of icons above the panel.
    let has_queue = view.sel.iter().filter_map(|id| infos.get(id)).any(|u| u.factory.as_ref().map_or(false, |f| !f.queue.is_empty()));
    let queue_band = has_queue && mouse.x >= panel.min.x && mouse.y >= panel.min.y - QUEUE_H - 4.0 && mouse.y < panel.min.y;
    let over_hud = mouse.y < TOP || mm.contains(mouse) || (!view.sel.is_empty() && panel.contains(mouse)) || view.menu_open
        || (!idle.is_empty() && idle_r.contains(mouse))
        || queue_band
        || view.queue_drag.is_some()
        || view.settings_open
        || ctx.is_pointer_over_egui();

    // ---------------------------------------------------------------- camera
    let world = map.world();
    let set_matrices = |view: &mut ViewState| {
        let d = view.cam_dist;
        let t = ((d - 50.0) / 260.0).clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        // The zoom sets the default pitch; holding Space turns and tilts the camera on top of it.
        let pitch = (50.0 + 34.0 * t + view.cam_tilt).clamp(12.0, 88.0).to_radians();
        let target = view.cam_target.at(map.h(view.cam_target).max(map.water));
        view.cam_pos = target + Vec3::new(d * pitch.cos() * view.cam_yaw.sin(), d * pitch.sin(), d * pitch.cos() * view.cam_yaw.cos());
        view.vp = view_proj(FOV, sw / sh, view.cam_pos, target);
        view.inv = view.vp.inverse();
        target
    };
    let ray_ground = |view: &ViewState, m: Pos2| -> V2 {
        let ndc = Vec3::new(2.0 * m.x / sw - 1.0, 1.0 - 2.0 * m.y / sh, 0.5);
        let far = view.inv.project_point3(ndc);
        match map.raycast(view.cam_pos, far - view.cam_pos) {
            Some(hit) => map.clamp(flat(hit)),
            None => view.cam_target,
        }
    };
    set_matrices(view);
    if !view.menu_open {
        // Hold Space and move the mouse: turn (left/right) and tilt (up/down), like SupCom.
        // A tap on Space without moving puts the camera back to its default angle.
        if p.keys.pressed(KeyCode::Space) {
            if let Some(last) = view.space_last {
                let (dx, dy) = (mouse.x - last.x, mouse.y - last.y);
                if dx.abs() + dy.abs() > 0.5 {
                    view.space_moved = true;
                    view.cam_yaw = wrap(view.cam_yaw - dx * 0.006);
                    view.cam_tilt = (view.cam_tilt + dy * 0.25).clamp(-70.0, 40.0);
                }
            } else {
                view.space_moved = false;
            }
            view.space_last = Some(Vec2::new(mouse.x, mouse.y));
        } else if view.space_last.take().is_some() && !view.space_moved {
            view.cam_yaw = 0.0;
            view.cam_tilt = 0.0;
        }
        // Arrow keys pan relative to the screen, whichever way the camera is turned.
        let pan = view.cam_dist * 0.9 * dt;
        let right = v2(view.cam_yaw.cos(), -view.cam_yaw.sin());
        let ahead = v2(-view.cam_yaw.sin(), -view.cam_yaw.cos());
        if p.keys.pressed(KeyCode::ArrowLeft) {
            view.cam_target = view.cam_target - right * pan;
        }
        if p.keys.pressed(KeyCode::ArrowRight) {
            view.cam_target = view.cam_target + right * pan;
        }
        if p.keys.pressed(KeyCode::ArrowUp) {
            view.cam_target = view.cam_target + ahead * pan;
        }
        if p.keys.pressed(KeyCode::ArrowDown) {
            view.cam_target = view.cam_target - ahead * pan;
        }
        if p.mouse.pressed(MouseButton::Middle) {
            if let Some(last) = view.mmb_last {
                let (a, b) = (ray_ground(view, Pos2::new(last.x, last.y)), ray_ground(view, mouse));
                view.cam_target = view.cam_target + (a - b);
            }
            view.mmb_last = Some(Vec2::new(mouse.x, mouse.y));
        } else {
            view.mmb_last = None;
        }
        let wheel: f32 = p.wheel.read().map(|w| w.y.signum()).sum();
        if wheel != 0.0 && !over_hud && view.mode == Mode::Terraform {
            // While terraforming the wheel sets how far to raise or lower the ground.
            view.terra_delta = (view.terra_delta + wheel).clamp(-10.0, 10.0);
        } else if wheel != 0.0 && !over_hud {
            let before = ray_ground(view, mouse);
            view.cam_dist = (view.cam_dist * if wheel > 0.0 { 0.85 } else { 1.0 / 0.85 }).clamp(6.0, world * 1.4);
            set_matrices(view);
            if wheel > 0.0 {
                let after = ray_ground(view, mouse);
                view.cam_target = view.cam_target + (before - after);
            }
        }
    } else {
        p.wheel.clear();
    }
    view.cam_target = v2(view.cam_target.x.clamp(0.0, world), view.cam_target.y.clamp(0.0, world));
    let target3 = set_matrices(view);
    if let Ok((mut t, mut proj)) = p.camera.single_mut() {
        *t = Transform::from_translation(view.cam_pos).looking_at(target3, Vec3::Y);
        // Adjust the existing projection in place: replacing it would reset the aspect ratio
        // to 1:1 until Bevy recomputes it, and frustum culling would drop everything left and
        // right of a square view (missing terrain chunks at the screen edges).
        let (near, far) = ((view.cam_dist * 0.02).max(0.3), view.cam_dist * 8.0 + 3000.0);
        if let Projection::Perspective(pp) = &mut *proj {
            if pp.fov != FOV || (pp.near - near).abs() > 0.01 || (pp.far - far).abs() > 1.0 {
                pp.fov = FOV;
                pp.near = near;
                pp.far = far;
            }
        }
    }
    let w2s = |view: &ViewState, pt: Vec3| -> Option<(Pos2, f32)> {
        let c = view.vp * pt.extend(1.0);
        if c.w <= 0.05 {
            return None;
        }
        Some((Pos2::new((c.x / c.w * 0.5 + 0.5) * sw, (0.5 - c.y / c.w * 0.5) * sh), c.w))
    };
    let px_per_unit = |depth: f32| sh / (2.0 * (FOV * 0.5).tan() * depth);
    let ground = ray_ground(view, mouse);

    // ---------------------------------------------------------------- what we can see
    let mut seen: Vec<Seen> = Vec::with_capacity(infos.len());
    let project = |pos: V2, y: f32, d: Option<&UnitDef>| -> (Pos2, bool, f32) {
        let h = d.map_or(0.5, |d| d.height() * 0.5);
        match w2s(view, pos.at(y + h)) {
            Some((sp, depth)) => (sp, sp.x > -30.0 && sp.y > -30.0 && sp.x < sw + 30.0 && sp.y < sh + 30.0, d.map_or(0.6, |d| d.radius()) * px_per_unit(depth)),
            None => (Pos2::ZERO, false, 0.0),
        }
    };
    for u in infos.values() {
        if hosted.contains(&u.id) {
            continue;
        }
        let ut = game.team_of(u.owner);
        let d = defs.get(u.def);
        if ut == team || reveal {
            let (sp, on_screen, rpx) = project(u.pos, u.y, Some(d));
            seen.push(Seen { id: u.id, def: Some(u.def), owner: u.owner, pos: u.pos, sp, on_screen, rpx, contact: Contact::Visible, mine: u.owner == me, enemy: hostile(team, ut), fade: 1.0 });
        }
    }
    if !reveal {
        let fade_ticks = defs.rules.radar_fade_secs / DT;
        for g in p.mem.ghosts.values() {
            let d = g.def.map(|d| defs.get(d));
            let visible = g.live && infos.get(&g.id).map_or(false, |u| u.seen);
            // Use the interpolated position for live contacts.
            let (pos, y) = if g.live { infos.get(&g.id).map_or((g.pos, g.y), |u| (u.pos, u.y)) } else { (g.pos, g.y) };
            let contact = if visible {
                Contact::Visible
            } else if g.live {
                Contact::Radar
            } else {
                Contact::Stale
            };
            let fade = if g.live || g.structure { 1.0 } else { (1.0 - (game.tick.saturating_sub(g.last_tick)) as f32 / fade_ticks).clamp(0.0, 1.0) };
            let (sp, on_screen, rpx) = project(pos, y, d);
            let shown_def = if contact == Contact::Visible { infos.get(&g.id).map(|u| u.def).or(g.def) } else { g.def };
            seen.push(Seen { id: g.id, def: shown_def, owner: g.owner, pos, sp, on_screen, rpx, contact, mine: false, enemy: hostile(team, game.team_of(g.owner)), fade });
        }
    }

    // A fixed draw order (far to near, then by id): overlapping icons must not swap places from
    // frame to frame, which they did when this list came straight out of a hash map.
    seen.sort_by(|a, b| a.sp.y.total_cmp(&b.sp.y).then(a.id.cmp(&b.id)));
    let icon_scale = if p.settings.big_icons { 1.5 } else { 1.0 };
    let fr = Frame { defs, game, map, infos: &infos, seen: &seen, team, sw, sh, mouse };
    let _ = (fr.team, fr.sw, fr.sh, fr.game);
    let hover = fr.hover(over_hud);
    let out = &mut p.out.0;
    let out_before = out.len();
    let occ = if matches!(view.mode, Mode::Place(_)) { local_occ(map, defs, &infos, game, team, &p.mem, reveal) } else { vec![] };
    let lmb_down = p.mouse.just_pressed(MouseButton::Left);
    let lmb_up = p.mouse.just_released(MouseButton::Left);
    let lmb_held = p.mouse.pressed(MouseButton::Left);
    let rmb_down = p.mouse.just_pressed(MouseButton::Right);
    let rmb_up = p.mouse.just_released(MouseButton::Right);

    // ---------------------------------------------------------------- world input
    if !view.menu_open {
        if lmb_down && !over_hud {
            match view.mode {
                Mode::Place(_) | Mode::GroundAttack | Mode::Reclaim | Mode::Assist | Mode::Capture | Mode::Revive | Mode::Terraform | Mode::Load | Mode::Unload => view.drag_start = Some(ground),
                Mode::AttackMove | Mode::Patrol | Mode::Move => {
                    view.path = vec![ground];
                    view.pathing = Some(0);
                }
                Mode::None => view.box_start = Some(Vec2::new(mouse.x, mouse.y)),
            }
        }
        if lmb_up {
            if let Some(start) = view.drag_start.take() {
                let radius = start.dist(ground);
                match view.mode {
                    Mode::Place(d) => {
                        let builders: Vec<&Info> = fr.selected(view).into_iter().filter(|u| fr.d(u.def).is_engineer() && fr.d(u.def).builds.contains(&d)).collect();
                        if !builders.is_empty() {
                            let from = builders.iter().fold(v2(0.0, 0.0), |a, u| a + u.pos) * (1.0 / builders.len() as f32);
                            let ids: Vec<u32> = builders.iter().map(|u| u.id).collect();
                            let mut first = true;
                            for pt in fr.build_spots(d, start, ground, &occ, from, ctrl) {
                                if can_place(defs, map, &occ, d, pt) {
                                    out.push(Command::Orders { ids: ids.clone(), orders: vec![Order::Build { def: d, pos: pt }], queue: shift || !first });
                                    first = false;
                                }
                            }
                        }
                    }
                    Mode::Assist => {
                        if let Some(h) = hover.filter(|h| !seen[*h].enemy && seen[*h].contact == Contact::Visible) {
                            let ids: Vec<u32> = view.sel.iter().copied().filter(|id| *id != seen[h].id).collect();
                            if !ids.is_empty() {
                                out.push(Command::Orders { ids, orders: vec![Order::Assist(seen[h].id)], queue: shift });
                            }
                        }
                    }
                    Mode::Capture => {
                        if let Some(h) = hover.filter(|h| game.team_of(seen[*h].owner) != team && seen[*h].contact != Contact::Stale) {
                            let ids: Vec<u32> = fr.selected(view).iter().filter(|u| fr.d(u.def).is_engineer()).map(|u| u.id).collect();
                            if !ids.is_empty() {
                                out.push(Command::Orders { ids, orders: vec![Order::Capture(seen[h].id)], queue: shift });
                            }
                        }
                    }
                    Mode::GroundAttack => {
                        let enemy = hover.filter(|h| seen[*h].enemy && seen[*h].contact != Contact::Stale && radius < 1.5);
                        let order = match enemy {
                            Some(h) => Order::Attack(seen[h].id),
                            None => Order::AttackGround { pos: start, radius: if radius < 1.5 { 0.0 } else { radius } },
                        };
                        out.push(Command::Orders { ids: view.sel.clone(), orders: vec![order], queue: shift });
                    }
                    Mode::Reclaim => {
                        let mut ids: Vec<u32> = fr.selected(view).iter().filter(|u| fr.d(u.def).can_assist() || fr.d(u.def).is_scavenger()).map(|u| u.id).collect();
                        // A wreck, else a tree or rock (engineers only), under the click.
                        let single = wrecks.iter().filter(|w| w.1.dist(start) < 2.0).min_by(|a, b| a.1.dist(start).partial_cmp(&b.1.dist(start)).unwrap()).map(|w| (w.0, w.1)).or_else(|| {
                            let f = features.nearest(start, 1.6, start)?;
                            ids.retain(|id| fr.selected(view).iter().any(|u| u.id == *id && fr.d(u.def).can_assist()));
                            Some(f)
                        });
                        // A click straight on a standing building takes that building apart
                        // (never a dragged circle: that only collects wrecks, trees and rocks).
                        let building = hover.filter(|h| seen[*h].contact == Contact::Visible && seen[*h].def.map_or(false, |d| fr.d(d).is_structure()) && (seen[*h].mine || game.team_of(seen[*h].owner) != team));
                        let order = match (single, building, radius < 1.5) {
                            (Some(w), _, true) => Some(Order::Reclaim(w.0)),
                            (None, Some(h), true) => {
                                ids = fr.selected(view).iter().filter(|u| fr.d(u.def).can_assist() && u.id != seen[h].id).map(|u| u.id).collect();
                                Some(Order::Reclaim(seen[h].id))
                            }
                            (None, None, true) => None,
                            _ => Some(Order::ReclaimArea { pos: start, radius }),
                        };
                        if let (Some(o), false) = (order, ids.is_empty()) {
                            out.push(Command::Orders { ids, orders: vec![o], queue: shift });
                        }
                    }
                    Mode::Revive => {
                        let ids: Vec<u32> = fr.selected(view).iter().filter(|u| fr.d(u.def).is_scavenger()).map(|u| u.id).collect();
                        let single = wrecks.iter().filter(|w| w.1.dist(start) < 2.0).min_by(|a, b| a.1.dist(start).partial_cmp(&b.1.dist(start)).unwrap());
                        let order = match (single, radius < 1.5) {
                            (Some(w), true) => Some(Order::Revive(w.0)),
                            (None, true) => None,
                            _ => Some(Order::ReviveArea { pos: start, radius }),
                        };
                        if let (Some(o), false) = (order, ids.is_empty()) {
                            out.push(Command::Orders { ids, orders: vec![o], queue: shift });
                        }
                    }
                    Mode::Terraform => {
                        let ids: Vec<u32> = fr.selected(view).iter().filter(|u| fr.d(u.def).is_engineer()).map(|u| u.id).collect();
                        if !ids.is_empty() {
                            let height = map.h(start) + view.terra_delta;
                            out.push(Command::Orders { ids, orders: vec![Order::Terraform { pos: start, radius: radius.max(3.0), height }], queue: shift });
                        }
                    }
                    Mode::Load => {
                        let sel = fr.selected(view);
                        let carriers: Vec<u32> = sel.iter().filter(|u| fr.d(u.def).transport.is_some()).map(|u| u.id).collect();
                        let riders: Vec<u32> = sel.iter().filter(|u| carriable(fr.d(u.def))).map(|u| u.id).collect();
                        let target = hover.filter(|h| radius < 1.5 && seen[*h].contact == Contact::Visible && game.team_of(seen[*h].owner) == team).map(|h| &seen[h]);
                        match target.and_then(|s| s.def.map(|d| (s, fr.d(d)))) {
                            // Click on a transport: the selected units get in.
                            Some((s, d)) if d.transport.is_some() && !riders.is_empty() => out.push(Command::Orders { ids: riders, orders: vec![Order::Load(s.id)], queue: shift }),
                            // Click on a unit: the selected transports fetch it.
                            Some((s, d)) if carriable(d) && s.mine && !carriers.is_empty() => out.push(Command::Orders { ids: carriers, orders: vec![Order::Load(s.id)], queue: shift }),
                            Some(_) => {}
                            None if !carriers.is_empty() && radius >= 1.5 => out.push(Command::Orders { ids: carriers, orders: vec![Order::LoadArea { pos: start, radius }], queue: shift }),
                            None => {}
                        }
                    }
                    Mode::Unload => {
                        let ids: Vec<u32> = fr.selected(view).iter().filter(|u| fr.d(u.def).transport.is_some()).map(|u| u.id).collect();
                        if !ids.is_empty() {
                            out.push(Command::Orders { ids, orders: vec![Order::Unload { pos: start, radius: radius.max(3.0) }], queue: shift });
                        }
                    }
                    _ => {}
                }
                if !shift {
                    view.mode = Mode::None;
                }
            } else if view.pathing == Some(0) {
                view.pathing = None;
                let kind = match view.mode {
                    Mode::Patrol => MoveKind::Patrol,
                    Mode::Move => MoveKind::Move,
                    _ => MoveKind::AttackMove,
                };
                let path = std::mem::take(&mut view.path);
                let enemy = hover.filter(|h| seen[*h].enemy && seen[*h].contact != Contact::Stale && path_len(&path) < 2.5);
                if let (Some(h), MoveKind::AttackMove) = (enemy, kind) {
                    out.push(Command::Orders { ids: view.sel.clone(), orders: vec![Order::Attack(seen[h].id)], queue: shift });
                } else {
                    fr.order_move(view, &path, kind, shift, out);
                }
                if !shift {
                    view.mode = Mode::None;
                }
            } else if let Some(b) = view.box_start.take() {
                let b = Pos2::new(b.x, b.y);
                if b.distance(mouse) < 6.0 {
                    match hover.filter(|h| seen[*h].mine) {
                        Some(h) => {
                            let (id, d) = (seen[h].id, seen[h].def.unwrap_or(0));
                            if now - view.last_click.0 < 0.35 && view.last_click.1 == d {
                                let same: Vec<u32> = seen.iter().filter(|s| s.mine && s.on_screen && s.def == Some(d)).map(|s| s.id).collect();
                                if shift {
                                    for s in same {
                                        if !view.sel.contains(&s) {
                                            view.sel.push(s);
                                        }
                                    }
                                } else {
                                    view.sel = same;
                                }
                            } else if shift {
                                if let Some(k) = view.sel.iter().position(|x| *x == id) {
                                    view.sel.remove(k);
                                } else {
                                    view.sel.push(id);
                                }
                            } else {
                                view.sel = vec![id];
                            }
                            view.last_click = (now, d);
                        }
                        None => {
                            if !shift {
                                view.sel.clear();
                            }
                        }
                    }
                } else {
                    let r = egui::Rect::from_two_pos(b, mouse);
                    let inside: Vec<&Seen> = seen.iter().filter(|s| s.mine && s.on_screen && r.contains(s.sp)).collect();
                    // A box picks the army first: fighting units and their support, without the
                    // commander, engineers and scavengers standing among them. Without any army
                    // unit in the box it takes the builders, and only then buildings.
                    let rank = |s: &Seen| -> u8 {
                        match s.def.map(|d| defs.get(d)) {
                            Some(d) if d.is_structure() => 0,
                            Some(d) if d.build.is_some() => 1,
                            _ => 2,
                        }
                    };
                    let top = inside.iter().map(|s| rank(s)).max().unwrap_or(0);
                    let ids: Vec<u32> = inside.iter().filter(|s| rank(s) == top).map(|s| s.id).collect();
                    if shift {
                        for id in ids {
                            if !view.sel.contains(&id) {
                                view.sel.push(id);
                            }
                        }
                    } else {
                        view.sel = ids;
                    }
                }
            }
        }
        if rmb_down && !over_hud {
            if view.mode != Mode::None {
                view.mode = Mode::None;
                view.drag_start = None;
                view.pathing = None;
            } else if !view.sel.is_empty() {
                view.path = vec![ground];
                view.pathing = Some(1);
            }
        }
        if view.pathing.is_some() && view.path.len() < 500 && view.path.last().map_or(false, |l| l.dist(ground) > 0.8) {
            view.path.push(ground);
        }
        if rmb_up && view.pathing == Some(1) {
            view.pathing = None;
            let path = std::mem::take(&mut view.path);
            let units = fr.selected(view);
            if path_len(&path) >= 2.5 {
                fr.order_move(view, &path, if alt { MoveKind::AttackMove } else { MoveKind::Move }, shift, out);
            } else if !units.is_empty() {
                // Context order.
                let mut done = false;
                if let Some(h) = hover {
                    let s = &seen[h];
                    if s.enemy {
                        // A remembered building can still be shelled where it stood.
                        let order = if s.contact == Contact::Stale { Order::AttackGround { pos: s.pos, radius: 0.0 } } else { Order::Attack(s.id) };
                        out.push(Command::Orders { ids: view.sel.clone(), orders: vec![order], queue: shift });
                        done = true;
                    } else if game.team_of(s.owner) != team {
                        // Neutral: engineers take it over.
                        let ids: Vec<u32> = units.iter().filter(|u| fr.d(u.def).is_engineer()).map(|u| u.id).collect();
                        if !ids.is_empty() && s.contact != Contact::Stale {
                            out.push(Command::Orders { ids, orders: vec![Order::Capture(s.id)], queue: shift });
                            done = true;
                        }
                    } else if s.contact == Contact::Visible {
                        let sd = s.def.map(|d| fr.d(d));
                        // A transport: units that can ride get in. A unit, with transports selected: they fetch it.
                        let riders: Vec<u32> = units.iter().filter(|u| carriable(fr.d(u.def))).map(|u| u.id).collect();
                        let carriers: Vec<u32> = units.iter().filter(|u| fr.d(u.def).transport.is_some() && u.id != s.id).map(|u| u.id).collect();
                        if sd.map_or(false, |d| d.transport.is_some()) && !riders.is_empty() {
                            out.push(Command::Orders { ids: riders, orders: vec![Order::Load(s.id)], queue: shift });
                            done = true;
                        } else if sd.map_or(false, carriable) && s.mine && !carriers.is_empty() {
                            out.push(Command::Orders { ids: carriers, orders: vec![Order::Load(s.id)], queue: shift });
                            done = true;
                        } else {
                            let ids: Vec<u32> = view.sel.iter().copied().filter(|id| *id != s.id).collect();
                            if !ids.is_empty() {
                                out.push(Command::Orders { ids, orders: vec![Order::Assist(s.id)], queue: shift });
                                done = true;
                            }
                        }
                    }
                }
                if !done {
                    // A wreck: engineers reclaim it, scavengers bring it back to life.
                    let builders: Vec<u32> = units.iter().filter(|u| fr.d(u.def).can_assist()).map(|u| u.id).collect();
                    let scavs: Vec<u32> = units.iter().filter(|u| fr.d(u.def).is_scavenger()).map(|u| u.id).collect();
                    if let (false, Some(w)) = (builders.is_empty() && scavs.is_empty(), wrecks.iter().filter(|w| w.1.dist(ground) < 1.5).min_by(|a, b| a.1.dist(ground).partial_cmp(&b.1.dist(ground)).unwrap())) {
                        if !builders.is_empty() {
                            out.push(Command::Orders { ids: builders, orders: vec![Order::Reclaim(w.0)], queue: shift });
                        }
                        if !scavs.is_empty() {
                            out.push(Command::Orders { ids: scavs, orders: vec![Order::Revive(w.0)], queue: shift });
                        }
                        done = true;
                    } else if builders.len() == units.len() && !builders.is_empty() {
                        // A tree or rock: reclaimed when only builders are selected (a mixed
                        // group just drives through the forest).
                        if let Some((fid, _)) = features.nearest(ground, 1.4, ground) {
                            out.push(Command::Orders { ids: builders, orders: vec![Order::Reclaim(fid)], queue: shift });
                            done = true;
                        }
                    }
                }
                if !done {
                    fr.order_move(view, &[ground], if alt { MoveKind::AttackMove } else { MoveKind::Move }, shift, out);
                }
            }
        }
    }

    // ---------------------------------------------------------------- keyboard
    let has_sel = !view.sel.is_empty();
    let mut do_stop = false;
    let mut do_upgrade = false;
    let mut do_repeat = false;
    view.preset = (p.settings.hotkeys == "faf_alternative") as u8;
    let faf = view.preset == 1;
    if p.keys.just_pressed(KeyCode::Escape) {
        if view.mode != Mode::None || view.pathing.is_some() || view.drag_start.is_some() {
            view.mode = Mode::None;
            view.pathing = None;
            view.drag_start = None;
        } else if faf && has_sel && !view.menu_open {
            // FAF alternative preset: Esc is "stop"; the menu is on F10.
            do_stop = true;
        } else {
            view.menu_open = !view.menu_open;
        }
    }
    if p.keys.just_pressed(KeyCode::F10) {
        view.menu_open = !view.menu_open;
    }
    if p.keys.just_pressed(KeyCode::F1) {
        view.show_help = !view.show_help;
    }
    // Pausing the whole game: Pause key or Ctrl+P (both presets), the top bar button, the menu.
    if p.keys.just_pressed(KeyCode::Pause) || (ctrl && p.keys.just_pressed(KeyCode::KeyP) && !ctx.egui_wants_keyboard_input()) {
        view.pause_toggle = true;
    }
    let mut do_pause = !view.menu_open && has_sel && p.keys.just_pressed(KeyCode::Tab);
    if p.keys.just_pressed(KeyCode::F6) {
        p.settings.shadows = if p.settings.shadows > 0 { 0 } else { 2 };
        p.settings.save();
    }
    if !view.menu_open && !ctx.egui_wants_keyboard_input() {
        let jp = |k: KeyCode| p.keys.just_pressed(k);
        // --- selection shortcuts (same in both presets, FAF bindings)
        let pick = |f: &dyn Fn(&UnitDef) -> bool, on_screen: bool| -> Vec<u32> {
            seen.iter().filter(|s| s.mine && (!on_screen || s.on_screen) && s.def.map_or(false, |d| f(defs.get(d)))).map(|s| s.id).collect()
        };
        let mobile = |d: &UnitDef| !d.is_structure();
        if ctrl && !shift {
            if jp(KeyCode::KeyA) {
                view.sel = pick(&|d| d.is_air(), false);
            }
            if jp(KeyCode::KeyD) {
                view.sel = pick(&|d| mobile(d) && !d.is_air() && d.build.is_none(), false);
            }
            if jp(KeyCode::KeyB) {
                view.sel = pick(&|d| d.is_engineer(), false);
            }
            // "All units" means the army: no commander, engineers or scavengers.
            if jp(KeyCode::KeyX) {
                view.sel = pick(&|d| mobile(d) && d.build.is_none(), false);
            }
            if jp(KeyCode::KeyC) {
                view.sel = pick(&|d| mobile(d) && d.build.is_none(), true);
            }
            if jp(KeyCode::KeyH) {
                view.sel = pick(&|d| d.is_factory(), true);
            }
            if jp(KeyCode::Period) {
                view.sel = pick(&|d| d.is_engineer(), true);
            }
            if jp(KeyCode::KeyZ) && has_sel {
                let types: Vec<DefId> = fr.selected(view).iter().map(|u| u.def).collect();
                view.sel = pick(&|d| types.contains(&d.idx), false);
            }
            if jp(KeyCode::KeyK) && has_sel {
                out.push(Command::SelfDestruct { ids: view.sel.clone() });
            }
        }
        if ctrl && shift {
            for (key, cat) in [(KeyCode::KeyD, "land"), (KeyCode::KeyA, "air")] {
                if jp(key) {
                    let near = seen
                        .iter()
                        .filter(|s| s.mine && s.def.map_or(false, |d| defs.get(d).is_factory() && defs.get(d).build.as_ref().map_or(false, |b| b.categories.iter().any(|c| c == cat))))
                        .min_by(|a, b| a.pos.dist(view.cam_target).partial_cmp(&b.pos.dist(view.cam_target)).unwrap());
                    if let Some(f) = near {
                        view.sel = vec![f.id];
                    }
                }
            }
        }
        // --- commands
        if has_sel && !ctrl {
            // Same keys in both presets.
            if jp(KeyCode::KeyL) {
                view.mode = Mode::Load;
            }
            if jp(KeyCode::KeyK) {
                view.mode = Mode::Unload;
            }
            if jp(KeyCode::KeyJ) {
                view.mode = Mode::Revive;
            }
            if faf {
                if jp(KeyCode::KeyQ) {
                    view.mode = Mode::Reclaim;
                }
                if jp(KeyCode::KeyW) {
                    view.mode = if alt { Mode::Patrol } else { Mode::Move };
                }
                if jp(KeyCode::KeyI) || jp(KeyCode::KeyT) {
                    view.mode = Mode::Assist;
                }
                if jp(KeyCode::KeyR) {
                    view.mode = Mode::GroundAttack;
                }
                do_repeat = jp(KeyCode::Backquote);
                do_upgrade = jp(KeyCode::KeyO);
                // Hotbuild: one key per building/unit class, repeated presses cycle through the class.
                for key in [KeyCode::KeyZ, KeyCode::KeyX, KeyCode::KeyC, KeyCode::KeyV, KeyCode::KeyD, KeyCode::KeyS, KeyCode::KeyG, KeyCode::KeyM, KeyCode::KeyY] {
                    if !jp(key) {
                        continue;
                    }
                    let units = fr.selected(view);
                    let Some(lead) = units.iter().find(|u| !fr.d(u.def).builds.is_empty()).copied() else { continue };
                    let ld = fr.d(lead.def);
                    let mut cands: Vec<DefId> = ld.builds.iter().copied().filter(|d| hot_key(fr.d(*d)) == Some(key)).collect();
                    cands.sort_by_key(|d| std::cmp::Reverse(fr.d(*d).tier));
                    if cands.is_empty() {
                        continue;
                    }
                    let cycling = view.hot.0 == Some(key) && now - view.hot.2 < 1.2;
                    let idx = if cycling { (view.hot.1 + 1) % cands.len() } else { 0 };
                    let d = cands[idx];
                    if ld.is_factory() {
                        let n: i16 = if shift { 5 } else { 1 };
                        for f in units.iter().filter(|u| u.def == lead.def) {
                            if cycling && view.hot.3 != d {
                                // Swap the unit queued by the previous press for the next one in the class.
                                out.push(Command::Produce { factory: f.id, def: view.hot.3, count: -n });
                            }
                            out.push(Command::Produce { factory: f.id, def: d, count: n });
                        }
                    } else {
                        view.mode = Mode::Place(d);
                    }
                    view.hot = (Some(key), idx, now, d);
                }
            } else {
                if jp(KeyCode::KeyA) {
                    view.mode = Mode::AttackMove;
                }
                if jp(KeyCode::KeyP) {
                    view.mode = Mode::Patrol;
                }
                if jp(KeyCode::KeyG) {
                    view.mode = Mode::GroundAttack;
                }
                if jp(KeyCode::KeyE) {
                    view.mode = Mode::Reclaim;
                }
                do_stop |= jp(KeyCode::KeyS);
                do_upgrade = jp(KeyCode::KeyU);
                do_repeat = jp(KeyCode::KeyR);
            }
        }
        if jp(KeyCode::Period) && !ctrl {
            let mut ids: Vec<(u32, V2)> = infos.values().filter(|u| u.owner == me && fr.d(u.def).is_engineer() && u.built.is_none() && u.host == 0 && u.orders.is_empty()).map(|u| (u.id, u.pos)).collect();
            if !ids.is_empty() {
                ids.sort_by_key(|x| x.0);
                view.idle_cycle = (view.idle_cycle + 1) % ids.len();
                view.sel = vec![ids[view.idle_cycle].0];
                if !alt {
                    view.cam_target = ids[view.idle_cycle].1;
                }
            }
        }
        if jp(KeyCode::Comma) || jp(KeyCode::Home) {
            let com = defs.factions[player.faction as usize].commander;
            if let Some(c) = infos.values().find(|u| u.owner == me && Some(u.def) == com) {
                view.sel = vec![c.id];
                if !alt {
                    view.cam_target = c.pos;
                }
            }
        }
        let digits = [KeyCode::Digit0, KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4, KeyCode::Digit5, KeyCode::Digit6, KeyCode::Digit7, KeyCode::Digit8, KeyCode::Digit9];
        for (g, k) in digits.iter().enumerate() {
            if jp(*k) {
                if ctrl {
                    view.groups[g] = view.sel.clone();
                } else if !view.groups[g].is_empty() {
                    view.groups[g].retain(|id| infos.contains_key(id));
                    if shift {
                        // Shift: add the group to the current selection.
                        for id in view.groups[g].clone() {
                            if !view.sel.contains(&id) {
                                view.sel.push(id);
                            }
                        }
                    } else {
                        view.sel = view.groups[g].clone();
                    }
                    if view.last_group.1 == g && now - view.last_group.0 < 0.4 {
                        let us = fr.selected(view);
                        if !us.is_empty() {
                            view.cam_target = us.iter().fold(v2(0.0, 0.0), |a, u| a + u.pos) * (1.0 / us.len() as f32);
                        }
                    }
                    view.last_group = (now, g);
                }
            }
        }
    }

    // ---------------------------------------------------------------- 3D helpers (gizmos)
    let g = &mut p.gizmos;
    let selected = fr.selected(view);
    let sel_col = Color::srgba(0.4, 1.0, 0.5, 0.9);
    for (k, u) in selected.iter().enumerate() {
        let d = fr.d(u.def);
        if k < 250 {
            ring(g, map, u.pos, d.radius() + 0.25, sel_col);
        }
        if selected.len() <= 12 || k < 3 {
            if d.armed() {
                for (r, c) in range_rings(d, 0.6) {
                    ring(g, map, u.pos, r, c);
                }
                let minr = d.weapons[0].min_range;
                if minr > 0.0 {
                    ring(g, map, u.pos, minr, Color::srgba(1.0, 0.6, 0.2, 0.4));
                }
            }
            if d.radar > 0.0 {
                ring(g, map, u.pos, d.radar, Color::srgba(0.4, 0.7, 1.0, 0.6));
            }
            if d.is_station() {
                ring(g, map, u.pos, d.build_range(), Color::srgba(0.5, 1.0, 0.6, 0.6));
            }
        }
        if k < 60 {
            let mut last = u.pos;
            if let Some(r) = u.factory.as_ref().and_then(|f| f.rally) {
                ground_line(g, map, last, r, Color::srgba(0.3, 0.8, 1.0, 0.8));
                ring(g, map, r, 0.5, Color::srgba(0.3, 0.8, 1.0, 0.8));
            }
            for o in &u.orders {
                let (pt, c) = match o {
                    Order::Move(pt) => (Some(*pt), Color::srgba(0.3, 0.8, 1.0, 0.7)),
                    Order::AttackMove(pt) => (Some(*pt), Color::srgba(1.0, 0.4, 0.3, 0.7)),
                    Order::Patrol(pt) => (Some(*pt), Color::srgba(0.4, 0.9, 0.9, 0.7)),
                    Order::Build { def, pos } => {
                        let h = fr.d(*def).footprint as f32 * CELL * 0.5;
                        let c = Color::srgba(1.0, 0.9, 0.3, 0.8);
                        for (a, b) in [((-h, -h), (h, -h)), ((h, -h), (h, h)), ((h, h), (-h, h)), ((-h, h), (-h, -h))] {
                            ground_line(g, map, *pos + v2(a.0, a.1), *pos + v2(b.0, b.1), c);
                        }
                        (Some(*pos), c)
                    }
                    Order::Attack(id) => (infos.get(id).map(|t| t.pos), Color::srgba(1.0, 0.2, 0.2, 0.8)),
                    Order::AttackGround { pos, radius } => {
                        ring(g, map, *pos, radius.max(0.6), Color::srgba(1.0, 0.3, 0.2, 0.8));
                        (Some(*pos), Color::srgba(1.0, 0.3, 0.2, 0.8))
                    }
                    Order::Assist(id) => (infos.get(id).map(|t| t.pos), Color::srgba(0.9, 0.9, 0.3, 0.7)),
                    Order::Reclaim(id) => (wrecks.iter().find(|w| w.0 == *id).map(|w| w.1).or_else(|| features.get(*id).map(|f| f.pos)).or_else(|| infos.get(id).map(|t| t.pos)), Color::srgba(0.6, 1.0, 0.4, 0.7)),
                    Order::ReviveArea { pos, radius } => {
                        ring(g, map, *pos, *radius, Color::srgba(0.5, 0.9, 1.0, 0.8));
                        (Some(*pos), Color::srgba(0.5, 0.9, 1.0, 0.8))
                    }
                    Order::Terraform { pos, radius, .. } => {
                        ring(g, map, *pos, *radius, Color::srgba(0.85, 0.65, 0.35, 0.9));
                        (Some(*pos), Color::srgba(0.85, 0.65, 0.35, 0.9))
                    }
                    Order::Load(id) => (infos.get(id).map(|t| t.pos), Color::srgba(0.4, 0.8, 1.0, 0.8)),
                    Order::LoadArea { pos, radius } => {
                        ring(g, map, *pos, radius.max(0.6), Color::srgba(0.4, 0.8, 1.0, 0.8));
                        (Some(*pos), Color::srgba(0.4, 0.8, 1.0, 0.8))
                    }
                    Order::Unload { pos, radius } => {
                        ring(g, map, *pos, radius.max(0.6), Color::srgba(1.0, 0.75, 0.3, 0.8));
                        (Some(*pos), Color::srgba(1.0, 0.75, 0.3, 0.8))
                    }
                    Order::Revive(id) => (wrecks.iter().find(|w| w.0 == *id).map(|w| w.1), Color::srgba(0.5, 0.9, 1.0, 0.8)),
                    Order::Capture(id) => (infos.get(id).map(|t| t.pos), Color::srgba(0.8, 0.5, 1.0, 0.8)),
                    Order::ReclaimArea { pos, radius } => {
                        ring(g, map, *pos, *radius, Color::srgba(0.6, 1.0, 0.4, 0.7));
                        (Some(*pos), Color::srgba(0.6, 1.0, 0.4, 0.7))
                    }
                };
                if let Some(pt) = pt {
                    ground_line(g, map, last, pt, c);
                    last = pt;
                }
            }
        }
    }
    // Build and reclaim beams.
    for u in infos.values() {
        if u.work == 0 || !(fr.d(u.def).can_assist() || fr.d(u.def).is_scavenger()) || !p.fog.visible(u.pos) && game.team_of(u.owner) != team {
            continue;
        }
        let target = infos.get(&u.work).map(|t| t.pos.at(t.y + 0.4)).or_else(|| wrecks.iter().find(|w| w.0 == u.work).map(|w| w.1.at(map.h(w.1) + 0.3))).or_else(|| features.get(u.work).map(|f| f.pos.at(map.h(f.pos) + 0.8)));
        if let Some(t) = target {
            let flick = 0.5 + 0.5 * ((now * 22.0 + u.id as f64).sin() as f32);
            let a = u.pos.at(u.y + fr.d(u.def).height() * 0.9);
            g.line(a, t, Color::srgba(0.4, 1.0, 0.55, 0.4 + 0.6 * flick));
            g.line(a, t + Vec3::new(0.3, -0.2, 0.2), Color::srgba(0.8, 1.0, 0.5, 0.3 + 0.4 * flick));
        }
    }
    // Control points: a ring in the holder's colour, and inside it one that grows with a capture.
    let team_rgb = |t: u8| -> [f32; 3] { game.players.iter().find(|pl| pl.team == t).map_or([0.75, 0.75, 0.75], |pl| defs.rules.player_colors[pl.color as usize % defs.rules.player_colors.len()]) };
    for cp in &game.points {
        let c = team_rgb(cp.owner);
        ring(g, map, cp.pos, defs.rules.cp_radius, Color::srgba(c[0], c[1], c[2], 0.9));
        ring(g, map, cp.pos, defs.rules.cp_radius - 0.4, Color::srgba(c[0], c[1], c[2], 0.5));
        if cp.progress > 0.0 {
            let b = team_rgb(cp.by);
            ring(g, map, cp.pos, (defs.rules.cp_radius * cp.progress).max(0.5), Color::srgba(b[0], b[1], b[2], 1.0));
        }
        let base = cp.pos.at(map.h(cp.pos).max(map.water));
        g.line(base, base + Vec3::Y * 7.0, Color::srgba(c[0], c[1], c[2], 0.9));
    }
    // Previews for the active command.
    let preview = Color::srgba(1.0, 0.9, 0.4, 0.95);
    match view.mode {
        Mode::Place(d) if !over_hud => {
            let df = fr.d(d);
            let from = selected.first().map_or(ground, |u| u.pos);
            let spots = match view.drag_start {
                Some(a) => fr.build_spots(d, a, ground, &occ, from, ctrl),
                None => vec![snap(defs, map, d, ground)],
            };
            if let (Some(a), true) = (view.drag_start, df.needs_deposit) {
                ring(g, map, a, a.dist(ground), preview);
            }
            for pt in &spots {
                let ok = can_place(defs, map, &occ, d, *pt);
                let c = if ok { Color::srgba(0.3, 1.0, 0.4, 1.0) } else { Color::srgba(1.0, 0.25, 0.2, 1.0) };
                let h = df.footprint as f32 * CELL * 0.5;
                let base = map.h(*pt).max(map.water);
                for (a, b) in [((-h, -h), (h, -h)), ((h, -h), (h, h)), ((h, h), (-h, h)), ((-h, h), (-h, -h))] {
                    let (pa, pb) = (*pt + v2(a.0, a.1), *pt + v2(b.0, b.1));
                    ground_line(g, map, pa, pb, c);
                    g.line(pa.at(base + df.height()), pb.at(base + df.height()), c);
                    g.line(pa.at(base + 0.2), pa.at(base + df.height()), c);
                }
            }
            if let Some(pt) = spots.first() {
                for (r, c) in range_rings(df, 0.8) {
                    ring(g, map, *pt, r, c);
                }
                if df.radar > 0.0 {
                    ring(g, map, *pt, df.radar, Color::srgba(0.4, 0.7, 1.0, 0.8));
                }
                if df.is_station() {
                    ring(g, map, *pt, df.build_range(), Color::srgba(0.5, 1.0, 0.6, 0.8));
                }
            }
        }
        Mode::GroundAttack | Mode::Reclaim | Mode::Revive | Mode::Load | Mode::Unload | Mode::Terraform => {
            let c = match view.mode {
                Mode::Reclaim => Color::srgba(0.6, 1.0, 0.4, 1.0),
                Mode::Revive => Color::srgba(0.5, 0.9, 1.0, 1.0),
                Mode::Load => Color::srgba(0.4, 0.8, 1.0, 1.0),
                Mode::Unload => Color::srgba(1.0, 0.75, 0.3, 1.0),
                Mode::Terraform => Color::srgba(0.85, 0.65, 0.35, 1.0),
                _ => Color::srgba(1.0, 0.3, 0.2, 1.0),
            };
            // Unloading and terraforming always cover an area, even on a plain click.
            let least = if matches!(view.mode, Mode::Unload | Mode::Terraform) { 3.0 } else { 0.5 };
            match view.drag_start {
                Some(a) => {
                    ring(g, map, a, a.dist(ground).max(least), c);
                    ground_line(g, map, a, ground, c);
                }
                None if least > 1.0 && !over_hud => ring(g, map, ground, least, c),
                None => {}
            }
        }
        _ => {}
    }
    if view.pathing.is_some() && path_len(&view.path) >= 2.5 {
        let c = if view.pathing == Some(0) || alt { Color::srgba(1.0, 0.45, 0.3, 1.0) } else { Color::srgba(0.3, 0.9, 1.0, 1.0) };
        g.linestrip(view.path.iter().map(|pt| pt.at(map.h(*pt).max(map.water) + 0.35)), c);
        let movers: Vec<&Info> = selected.iter().copied().filter(|u| !fr.d(u.def).is_structure() && !fr.d(u.def).is_plane()).collect();
        for pt in fr.formation(&movers, &view.path) {
            ring(g, map, pt, 0.45, c);
        }
    }

    // ---------------------------------------------------------------- 2D overlay: icons, blips, bars
    let bg = ctx.layer_painter(egui::LayerId::background());
    let blip = p.assets.blip.get_or_insert_with(|| ctx.load_texture("blip", egui::ColorImage::from_rgba_unmultiplied([icons::ICON, icons::ICON], &icons::render_blip()), egui::TextureOptions::LINEAR)).clone();
    if view.cam_dist > 110.0 {
        for dep in &map.deposits {
            if let Some((sp, _)) = w2s(view, dep.at(map.h(*dep))) {
                bg.circle_filled(sp, 3.2, rgba(0, 0, 0, 160));
                bg.circle_filled(sp, 2.2, rgba(205, 215, 230, 230));
            }
        }
    }
    for (k, s) in seen.iter().enumerate() {
        if !s.on_screen {
            continue;
        }
        let (_, col) = super::owner_color(defs, game, s.owner);
        let col = rgb(col);
        let selected_here = s.mine && view.sel.contains(&s.id);
        let d = s.def.map(|d| defs.get(d));
        match (s.contact, d) {
            (Contact::Visible, Some(d)) => {
                let size = icons::icon_size(d) * icon_scale;
                // Strategic icon once the model gets small on screen.
                if s.rpx * 2.0 < size * 0.8 {
                    let tex = p.assets.icon(&ctx, d);
                    if selected_here {
                        bg.circle_stroke(s.sp, size * 0.62, Stroke::new(1.5, Color32::WHITE));
                    }
                    bg.image(tex.id(), egui::Rect::from_center_size(s.sp, egui::vec2(size, size)), UV, col);
                }
            }
            (_, Some(d)) => {
                // Known type, not in sight: dimmed icon (radar contact or remembered building).
                let size = icons::icon_size(d) * icon_scale;
                let a = if s.contact == Contact::Stale { 0.5 * s.fade } else { 0.85 };
                let tex = p.assets.icon(&ctx, d);
                bg.image(tex.id(), egui::Rect::from_center_size(s.sp, egui::vec2(size, size)), UV, col.gamma_multiply(a));
                if s.contact == Contact::Stale {
                    bg.circle_stroke(s.sp, size * 0.6, Stroke::new(1.0, rgba(255, 255, 255, 60)));
                }
            }
            (_, None) => {
                let a = if s.contact == Contact::Stale { 0.8 * s.fade } else { 1.0 };
                bg.image(blip.id(), egui::Rect::from_center_size(s.sp, egui::vec2(15.0 * icon_scale, 15.0 * icon_scale)), UV, col.gamma_multiply(a));
            }
        }
        if hover == Some(k) {
            bg.circle_stroke(s.sp, s.rpx.max(11.0) + 2.0, Stroke::new(1.0, rgba(255, 255, 255, 150)));
            // Other players' units: say what it is, and how much of it is left if we can see it.
            if !s.mine {
                let text = match (d, infos.get(&s.id).filter(|_| s.contact == Contact::Visible)) {
                    (Some(d), Some(u)) => format!("{}  {:.0} / {:.0}", d.name, u.hp.max(0.0), d.hp),
                    (Some(d), None) => format!("{} (not in sight)", d.name),
                    (None, _) => "Unknown contact".to_string(),
                };
                let at = Pos2::new(s.sp.x, s.sp.y + s.rpx.max(11.0) + 6.0);
                let galley = ctx.fonts_mut(|f| f.layout_no_wrap(text, FontId::proportional(13.0), Color32::WHITE));
                let r = egui::Rect::from_center_size(Pos2::new(at.x, at.y + galley.size().y * 0.5), galley.size() + egui::vec2(10.0, 4.0));
                bg.rect_filled(r, 3.0, rgba(0, 0, 0, 190));
                bg.galley(r.min + egui::vec2(5.0, 2.0), galley, Color32::WHITE);
            }
        }
        if s.contact != Contact::Visible {
            continue;
        }
        let (Some(d), Some(u)) = (d, infos.get(&s.id)) else { continue };
        // Bars above the unit.
        let w = (s.rpx * 2.2).clamp(22.0, 70.0);
        let (bx, mut by) = (s.sp.x - w * 0.5, s.sp.y - s.rpx.max(9.0) - 9.0);
        let mut bar = |frac: f32, c: Color32, h: f32| {
            bg.rect_filled(rect(bx - 1.0, by - 1.0, w + 2.0, h + 2.0), 0.0, rgba(0, 0, 0, 190));
            bg.rect_filled(rect(bx, by, w * frac.clamp(0.0, 1.0), h), 0.0, c);
            by -= h + 3.0;
        };
        if let Some(c) = u.capture {
            bar(c, rgba(200, 120, 255, 255), 3.0);
        }
        let hp = (u.hp / d.hp).clamp(0.0, 1.0);
        if (hp < 0.999 && u.built.is_none()) || selected_here {
            bar(hp, if hp > 0.6 { rgba(60, 230, 80, 255) } else if hp > 0.3 { rgba(240, 205, 50, 255) } else { rgba(240, 55, 50, 255) }, 3.0);
        }
        // Reload of the main gun: the whole point of slow, heavy weapons.
        if s.mine {
            if let (Some(wd), Some(ws)) = (d.weapons.first(), u.weapons.first()) {
                if wd.reload >= 2.5 && (ws.cd > 0.0 || selected_here) {
                    let ready = ws.cd <= 0.0;
                    bar(1.0 - ws.cd / wd.reload, if ready { rgba(120, 220, 255, 255) } else { rgba(255, 170, 40, 255) }, 2.0);
                }
            }
        }
        if u.paused && s.mine {
            bg.text(Pos2::new(s.sp.x, s.sp.y + s.rpx.max(9.0) + 4.0), Align2::CENTER_TOP, "PAUSED", FontId::proportional(11.0), rgba(255, 210, 90, 255));
        }
        let prog = u.built.or(u.upg).or_else(|| u.factory.as_ref().and_then(|f| infos.get(&f.building)).and_then(|b| b.built));
        if let Some(pr) = prog {
            bar(pr, rgba(100, 200, 255, 255), 3.0);
        }
    }
    // Control points: a flag in the holder's colour, with the capture progress under it.
    if !game.points.is_empty() {
        let tex = p.assets.order_icons(&ctx);
        for cp in &game.points {
            let Some((sp, _)) = w2s(view, cp.pos.at(map.h(cp.pos).max(map.water) + 7.0)) else { continue };
            bg.circle_filled(sp, 13.0, rgba(0, 0, 0, 150));
            if let Some(tx) = tex.get("flag") {
                bg.image(tx.id(), egui::Rect::from_center_size(sp, egui::vec2(20.0, 20.0)), UV, rgb(team_rgb(cp.owner)));
            }
            if cp.progress > 0.0 {
                bg.rect_filled(rect(sp.x - 16.0, sp.y + 15.0, 32.0, 5.0), 0.0, rgba(0, 0, 0, 200));
                bg.rect_filled(rect(sp.x - 15.0, sp.y + 16.0, 30.0 * cp.progress.clamp(0.0, 1.0), 3.0), 0.0, rgb(team_rgb(cp.by)));
            }
        }
    }
    // Ctrl+Shift: how much mass lies around, summed per spot.
    if ctrl && shift && !matches!(view.mode, Mode::Place(_)) {
        let mut piles: HashMap<(i32, i32), (f32, V2)> = HashMap::new();
        // Rocks count too (trees hold mostly energy).
        let rocks = features.list.iter().filter(|f| f.left > 0.0).filter_map(|f| defs.rules.features.get(f.def as usize).map(|d| (f.pos, d.mass * f.left))).filter(|r| r.1 >= 1.0);
        for (pos, mass) in wrecks.iter().map(|w| (w.1, w.2)).chain(rocks) {
            let e = piles.entry(((pos.x / 12.0) as i32, (pos.y / 12.0) as i32)).or_insert((0.0, v2(0.0, 0.0)));
            e.0 += mass;
            e.1 = e.1 + pos * mass;
        }
        let tex = p.assets.order_icons(&ctx);
        for (mass, at) in piles.values() {
            let pos = *at * (1.0 / mass.max(0.001));
            if let Some((sp, _)) = w2s(view, pos.at(map.h(pos) + 1.0)) {
                let t = format!("{mass:.0}");
                let wd = t.len() as f32 * 7.5 + 24.0;
                bg.rect_filled(egui::Rect::from_center_size(sp, egui::vec2(wd, 18.0)), 4.0, rgba(0, 0, 0, 170));
                if let Some(tx) = tex.get("mass") {
                    bg.image(tx.id(), egui::Rect::from_center_size(Pos2::new(sp.x - wd * 0.5 + 10.0, sp.y), egui::vec2(14.0, 14.0)), UV, rgba(190, 215, 240, 255));
                }
                bg.text(Pos2::new(sp.x + 8.0, sp.y), Align2::CENTER_CENTER, t, FontId::proportional(13.0), rgba(210, 230, 250, 255));
            }
        }
    }
    if let Some(b) = view.box_start {
        let b = Pos2::new(b.x, b.y);
        if b.distance(mouse) >= 6.0 {
            let r = egui::Rect::from_two_pos(b, mouse);
            bg.rect_filled(r, 0.0, rgba(80, 255, 110, 30));
            bg.rect_stroke(r, 0.0, Stroke::new(1.5, rgba(100, 255, 130, 230)), StrokeKind::Inside);
        }
    }

    // ---------------------------------------------------------------- HUD panels
    let hud = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("hud")));
    let font = |s: f32| FontId::proportional(s);
    let text = |x: f32, y: f32, t: &str, size: f32, c: Color32| hud.text(Pos2::new(x, y), Align2::LEFT_TOP, t, font(size), c);
    let text_c = |x: f32, y: f32, t: &str, size: f32, c: Color32| hud.text(Pos2::new(x, y), Align2::CENTER_CENTER, t, font(size), c);
    let panel_bg = rgba(18, 21, 27, 235);
    let line_c = rgba(90, 100, 115, 255);
    let button = |r: egui::Rect, active: bool| -> bool {
        let hov = r.contains(mouse);
        hud.rect_filled(r, 3.0, if active { rgba(60, 105, 75, 250) } else if hov { rgba(64, 76, 94, 250) } else { rgba(34, 40, 50, 250) });
        hud.rect_stroke(r, 3.0, Stroke::new(1.0, if hov { Color32::WHITE } else { line_c }), StrokeKind::Inside);
        hov
    };
    let (_, my_rgb) = super::owner_color(defs, game, me);
    let my_col = rgb(my_rgb);

    // Resource symbols: the same four icons wherever the HUD talks about mass, energy, time or
    // build power. `stat` draws icon + text and returns where the next item can start.
    let res_tex = p.assets.order_icons(&ctx);
    let stat_col = |kind: &str| match kind {
        "mass" => rgba(190, 215, 240, 255),
        "energy" => rgba(255, 215, 65, 255),
        "power" => rgba(120, 230, 140, 255),
        _ => rgba(205, 205, 205, 255),
    };
    let stat = |pt: &egui::Painter, x: f32, y: f32, size: f32, kind: &str, t: &str, col: Color32| -> f32 {
        if let Some(tx) = res_tex.get(kind) {
            pt.image(tx.id(), rect(x, y - 1.0, size + 4.0, size + 4.0), UV, stat_col(kind));
        }
        pt.text(Pos2::new(x + size + 6.0, y), Align2::LEFT_TOP, t, font(size), col).max.x + 10.0
    };

    // --- economy bar
    hud.rect_filled(rect(0.0, 0.0, sw, TOP), 0.0, panel_bg);
    let res = |x: f32, name: &str, cur: f32, max: f32, inc: f32, exp: f32, col: Color32| {
        if let Some(tx) = res_tex.get(name) {
            hud.image(tx.id(), rect(x + 30.0, 3.0, 24.0, 24.0), UV, col);
        }
        let bx = x + 62.0;
        hud.rect_filled(rect(bx, 7.0, 150.0, 16.0), 2.0, rgba(0, 0, 0, 220));
        hud.rect_filled(rect(bx, 7.0, 150.0 * (cur / max.max(1.0)).clamp(0.0, 1.0), 16.0), 2.0, col.gamma_multiply(0.7));
        text_c(bx + 75.0, 15.0, &format!("{:.0} / {:.0}", cur, max), 12.0, Color32::WHITE);
        text(bx + 158.0, 7.0, &format!("+{:.1}", inc), 14.0, rgba(110, 255, 130, 255));
        text(bx + 220.0, 7.0, &format!("-{:.1}", exp), 14.0, if exp > inc + 0.01 { rgba(255, 105, 90, 255) } else { rgba(215, 215, 215, 255) });
    };
    res(12.0, "mass", player.mass, player.max_mass, player.income_m, player.spend_m, rgba(190, 215, 240, 255));
    res(360.0, "energy", player.energy, player.max_energy, player.income_e, player.spend_e, rgba(255, 215, 65, 255));
    if player.power_out {
        text(700.0, 7.0, "POWER STALL", 14.0, rgba(255, 80, 80, 255));
    }
    let secs = game.tick / 10;
    let clock = hud.text(Pos2::new(sw - 10.0, 7.0), Align2::RIGHT_TOP, format!("{:02}:{:02}   F1 help", secs / 60, secs % 60), font(14.0), rgba(205, 205, 205, 255));
    {
        let r = rect(clock.min.x - 100.0, 4.0, 86.0, TOP - 8.0);
        let hov = r.contains(mouse) && !view.menu_open && !view.settings_open;
        let on = view.paused.is_some();
        let fill = if on { rgba(150, 110, 30, 255) } else if hov { rgba(64, 76, 94, 255) } else { rgba(34, 40, 50, 255) };
        hud.rect_filled(r, 3.0, fill);
        hud.rect_stroke(r, 3.0, Stroke::new(1.0, if hov { Color32::WHITE } else { line_c }), StrokeKind::Inside);
        // Pause bars or a play triangle, then the word.
        let (ix, iy) = (r.min.x + 14.0, r.center().y);
        if on {
            hud.add(egui::Shape::convex_polygon(vec![Pos2::new(ix - 4.0, iy - 6.0), Pos2::new(ix + 6.0, iy), Pos2::new(ix - 4.0, iy + 6.0)], Color32::WHITE, Stroke::NONE));
        } else {
            hud.rect_filled(rect(ix - 5.0, iy - 6.0, 3.5, 12.0), 0.0, Color32::WHITE);
            hud.rect_filled(rect(ix + 1.5, iy - 6.0, 3.5, 12.0), 0.0, Color32::WHITE);
        }
        hud.text(Pos2::new(ix + 12.0, iy), Align2::LEFT_CENTER, if on { "Resume" } else { "Pause" }, font(14.0), Color32::WHITE);
        if hov && lmb_down {
            view.pause_toggle = true;
        }
    }
    if game.mode == MODE_POINTS {
        // One bar per team (ours first): the points it has left, how many control points it
        // holds and how fast it is losing points to a team that holds more.
        let mut teams: Vec<u8> = game.players.iter().map(|pl| pl.team).filter(|&t| t < 14).collect();
        teams.sort_by_key(|&t| (t != team, t));
        teams.dedup();
        let alive = |t: u8| game.players.iter().any(|pl| pl.team == t && pl.alive);
        let held = |t: u8| game.points.iter().filter(|cp| cp.owner == t).count() as f32;
        let n = teams.len().max(1) as f32;
        let gap = 8.0;
        let w = ((sw * 0.56 - gap * (n - 1.0)) / n).clamp(60.0, 230.0);
        let x0 = sw * 0.5 - (w * n + gap * (n - 1.0)) * 0.5;
        let start = game.cp_start.max(1.0);
        for (k, &t) in teams.iter().enumerate() {
            let x = x0 + k as f32 * (w + gap);
            let y = TOP + 8.0;
            let live = alive(t);
            let col = if live { rgb(team_rgb(t)) } else { rgba(120, 120, 120, 255) };
            let score = game.scores.get(t as usize).copied().unwrap_or(0.0);
            let score = if live { score } else { score.min(0.0) };
            let r = rect(x, y, w, 18.0);
            hud.rect_filled(r.expand(2.0), 2.0, rgba(0, 0, 0, 215));
            hud.rect_filled(rect(x, y, w * (score / start).clamp(0.0, 1.0), 18.0), 1.0, col.gamma_multiply(0.75));
            hud.rect_stroke(r.expand(2.0), 2.0, Stroke::new(if t == team { 2.0 } else { 1.0 }, if t == team { Color32::WHITE } else { line_c }), StrokeKind::Outside);
            let label = if w >= 120.0 { format!("{} {}:  {:.0}", if t == team { "Your team" } else { "Team" }, t + 1, score.ceil()) } else { format!("{:.0}", score.ceil()) };
            hud.text(Pos2::new(x + w * 0.5 + 1.0, y + 10.0), Align2::CENTER_CENTER, &label, font(13.0), rgba(0, 0, 0, 255));
            hud.text(Pos2::new(x + w * 0.5, y + 9.0), Align2::CENTER_CENTER, &label, font(13.0), Color32::WHITE);
            if !live {
                hud.text(Pos2::new(x + w * 0.5, y + 30.0), Align2::CENTER_CENTER, "defeated", font(12.0), rgba(150, 150, 150, 255));
                continue;
            }
            let lead = teams.iter().filter(|&&o| o != t && alive(o)).map(|&o| held(o)).fold(0.0, f32::max);
            let loss = (lead - held(t)).max(0.0) * defs.rules.cp_bleed.max(0.0);
            let (line, c) = if loss > 0.0 {
                (format!("holds {:.0} of {}   -{:.1}/s", held(t), game.points.len(), loss), rgba(255, 105, 90, 255))
            } else {
                (format!("holds {:.0} of {}", held(t), game.points.len()), rgba(215, 215, 215, 255))
            };
            let line = if w >= 120.0 { line } else if loss > 0.0 { format!("{:.0}  -{:.1}/s", held(t), loss) } else { format!("{:.0}", held(t)) };
            hud.text(Pos2::new(x + w * 0.5 + 1.0, y + 31.0), Align2::CENTER_CENTER, &line, font(12.0), rgba(0, 0, 0, 255));
            hud.text(Pos2::new(x + w * 0.5, y + 30.0), Align2::CENTER_CENTER, &line, font(12.0), c);
        }
    }
    for (k, pl) in game.players.iter().enumerate() {
        let c = rgb(defs.rules.player_colors[pl.color as usize % defs.rules.player_colors.len()]);
        let score = if game.mode == MODE_POINTS { format!("   {:.0} pts", game.scores.get(pl.team as usize).copied().unwrap_or(0.0)) } else { String::new() };
        let t = format!("{}{} (team {}){score}", pl.name, if pl.alive { "" } else { " - defeated" }, pl.team + 1);
        hud.text(Pos2::new(sw - 10.0, TOP + 6.0 + k as f32 * 16.0), Align2::RIGHT_TOP, t, font(13.0), if pl.alive { c } else { rgba(130, 130, 130, 255) });
    }

    // --- minimap
    let mini = p.assets.minimap.get_or_insert_with(|| ctx.load_texture("minimap", egui::ColorImage::from_rgba_unmultiplied([map.n, map.n], &minimap_pixels(map)), egui::TextureOptions::LINEAR)).clone();
    hud.rect_filled(mm.expand(2.0), 0.0, rgba(0, 0, 0, 240));
    hud.image(mini.id(), mm, UV, Color32::WHITE);
    // Fog of war on the minimap: a coarse vertex-coloured grid. (A texture that was re-uploaded
    // every few ticks made the whole minimap flicker.)
    if !reveal {
        const G: usize = 48;
        let n = map.n;
        let mut mesh = egui::Mesh::default();
        for gy in 0..=G {
            for gx in 0..=G {
                let (cx, cy) = ((gx * (n - 1) / G).min(n - 1), (gy * (n - 1) / G).min(n - 1));
                let a = p.fog.alpha[cy * n + cx];
                mesh.colored_vertex(Pos2::new(mm.min.x + gx as f32 / G as f32 * MM, mm.min.y + gy as f32 / G as f32 * MM), rgba(0, 0, 0, a));
            }
        }
        for gy in 0..G {
            for gx in 0..G {
                let i = (gy * (G + 1) + gx) as u32;
                let w = G as u32 + 1;
                mesh.add_triangle(i, i + 1, i + w);
                mesh.add_triangle(i + 1, i + w + 1, i + w);
            }
        }
        hud.add(egui::Shape::mesh(mesh));
    }
    let to_mm = |pt: V2| Pos2::new(mm.min.x + pt.x / world * MM, mm.min.y + pt.y / world * MM);
    for s in &seen {
        let (_, c) = super::owner_color(defs, game, s.owner);
        let c = if s.contact == Contact::Visible { rgb(c) } else { rgb(c).gamma_multiply(0.6 * s.fade.max(0.3)) };
        let d = s.def.map(|d| defs.get(d));
        let sz = if d.map_or(false, |d| d.icon.shape == "commander") { 5.0 } else if d.map_or(false, |d| d.is_structure()) { 3.0 } else { 2.0 };
        hud.rect_filled(egui::Rect::from_center_size(to_mm(s.pos), egui::vec2(sz, sz)), 0.0, c);
    }
    for cp in &game.points {
        let at = to_mm(cp.pos);
        hud.circle_filled(at, 4.5, rgba(0, 0, 0, 220));
        hud.circle_filled(at, 3.2, rgb(team_rgb(cp.owner)));
    }
    let corners = [Pos2::new(0.0, TOP), Pos2::new(sw, TOP), Pos2::new(sw, sh), Pos2::new(0.0, sh)].map(|c| {
        let q = to_mm(ray_ground(view, c));
        Pos2::new(q.x.clamp(mm.min.x, mm.max.x), q.y.clamp(mm.min.y, mm.max.y))
    });
    for k in 0..4 {
        hud.line_segment([corners[k], corners[(k + 1) % 4]], Stroke::new(1.0, Color32::WHITE));
    }
    if mm.contains(mouse) && !view.menu_open {
        let wp = v2((mouse.x - mm.min.x) / MM * world, (mouse.y - mm.min.y) / MM * world);
        if p.mouse.pressed(MouseButton::Left) && view.box_start.is_none() && view.pathing.is_none() && view.drag_start.is_none() {
            view.cam_target = wp;
        }
        if rmb_down && !view.sel.is_empty() {
            fr.order_move(view, &[wp], if alt { MoveKind::AttackMove } else { MoveKind::Move }, shift, out);
        }
    }

    // --- selection panel
    let units = fr.selected(view);
    let mut tip: Option<(DefId, f32)> = None;
    let mut new_sel: Option<Vec<u32>> = None;
    let mut new_mode: Option<Mode> = None;
    if !idle.is_empty() {
        let hov = button(idle_r, false);
        if let Some(u) = infos.get(&idle[0].0) {
            hud.image(p.assets.icon(&ctx, fr.d(u.def)).id(), egui::Rect::from_center_size(idle_r.center() - egui::vec2(0.0, 5.0), egui::vec2(30.0, 30.0)), UV, my_col);
        }
        hud.text(Pos2::new(idle_r.max.x - 4.0, idle_r.max.y - 2.0), Align2::RIGHT_BOTTOM, idle.len().to_string(), font(15.0), rgba(255, 230, 90, 255));
        if hov {
            hud.text(Pos2::new(idle_r.min.x - 6.0, idle_r.center().y), Align2::RIGHT_CENTER, "Idle engineers: click selects all, right-click goes to the next one", font(12.0), Color32::WHITE);
            if lmb_down {
                new_sel = Some(idle.iter().map(|x| x.0).collect());
            } else if rmb_down {
                view.idle_cycle = (view.idle_cycle + 1) % idle.len();
                new_sel = Some(vec![idle[view.idle_cycle].0]);
                view.cam_target = idle[view.idle_cycle].1;
            }
        }
    }
    if !lmb_held {
        if let (Some((fid, from_run)), true) = (view.queue_drag.take(), lmb_up) {
            // Dropped: move that run of the queue to the gap under the mouse.
            if let Some(f) = infos.get(&fid).and_then(|u| u.factory.as_ref()) {
                let runs = queue_runs(&f.queue);
                if from_run < runs.len() {
                    let gap = (((mouse.x - panel.min.x - 4.0) / QUEUE_W).round().max(0.0) as usize).min(runs.len());
                    if gap != from_run && gap != from_run + 1 {
                        let from: usize = runs[..from_run].iter().map(|r| r.1).sum();
                        let to: usize = runs[..gap].iter().enumerate().filter(|(k, _)| *k != from_run).map(|(_, r)| r.1).sum();
                        out.push(Command::QueueMove { factory: fid, from: from as u16, len: runs[from_run].1 as u16, to: to as u16 });
                    }
                }
            }
        }
    }
    if !units.is_empty() {
        hud.rect_filled(panel, 4.0, panel_bg);
        hud.rect_stroke(panel, 4.0, Stroke::new(1.0, line_c), StrokeKind::Inside);
        let lead = units.iter().find(|u| !fr.d(u.def).builds.is_empty()).copied().unwrap_or(units[0]);
        let ld = fr.d(lead.def);
        let ix = panel.min.x + 10.0;
        let py = panel.min.y;
        if units.len() == 1 {
            let u = lead;
            let r = rect(ix, py + 8.0, 52.0, 52.0);
            hud.rect_filled(r, 3.0, rgba(8, 10, 13, 255));
            match p.assets.portrait(&ctx, ld) {
                Some(t) => {
                    hud.image(t.id(), r, UV, Color32::WHITE);
                    let ic = p.assets.icon(&ctx, ld);
                    hud.image(ic.id(), rect(r.max.x - 18.0, r.max.y - 18.0, 20.0, 20.0), UV, my_col);
                }
                None => {
                    let ic = p.assets.icon(&ctx, ld);
                    hud.image(ic.id(), r.shrink(6.0), UV, my_col);
                }
            };
            text(ix + 60.0, py + 6.0, &ld.name, 17.0, Color32::WHITE);
            text(ix + 60.0, py + 27.0, &format!("Tier {}", ld.tier.max(1)), 12.0, rgba(170, 180, 195, 255));
            // Health bar with numbers.
            let hb = rect(ix + 60.0, py + 44.0, 220.0, 14.0);
            let hp = (u.hp / ld.hp).clamp(0.0, 1.0);
            hud.rect_filled(hb, 2.0, rgba(0, 0, 0, 230));
            hud.rect_filled(rect(hb.min.x, hb.min.y, hb.width() * hp, hb.height()), 2.0, if hp > 0.6 { rgba(50, 190, 70, 255) } else if hp > 0.3 { rgba(215, 180, 40, 255) } else { rgba(215, 50, 45, 255) });
            text_c(hb.center().x, hb.center().y, &format!("{:.0} / {:.0}", u.hp.max(0.0), ld.hp), 11.0, Color32::WHITE);
            let mut y = py + 66.0;
            // One reload bar per weapon.
            for (wd, ws) in ld.weapons.iter().zip(u.weapons.iter()) {
                let rb = rect(ix, y, 280.0, 13.0);
                let frac = (1.0 - ws.cd / wd.reload.max(0.01)).clamp(0.0, 1.0);
                hud.rect_filled(rb, 2.0, rgba(0, 0, 0, 230));
                hud.rect_filled(rect(rb.min.x, rb.min.y, rb.width() * frac, rb.height()), 2.0, if frac >= 1.0 { rgba(55, 130, 175, 255) } else { rgba(190, 120, 30, 255) });
                let state = if ws.cd > 0.0 { format!("{:.1}s", ws.cd) } else if ws.target != 0 && ws.aim < wd.aim_time { "aiming".into() } else { "ready".into() };
                text(rb.min.x + 4.0, rb.min.y, &format!("{}  {:.0} dmg  r{:.0}", wd.name, wd.damage * wd.salvo as f32, wd.range), 10.5, Color32::WHITE);
                hud.text(Pos2::new(rb.max.x - 4.0, rb.min.y), Align2::RIGHT_TOP, state, font(10.5), Color32::WHITE);
                y += 16.0;
            }
            let e = &ld.eco;
            if e.mass > 0.0 || e.energy > 0.0 || e.upkeep > 0.0 || ld.build_rate() > 0.0 {
                let c = rgba(180, 225, 255, 255);
                let mut x = ix;
                if e.mass > 0.0 {
                    x = stat(&hud, x, y, 12.0, "mass", &format!("+{:.0}", e.mass), c);
                }
                if e.energy > 0.0 {
                    x = stat(&hud, x, y, 12.0, "energy", &format!("+{:.0}", e.energy), c);
                }
                if e.upkeep > 0.0 {
                    x = stat(&hud, x, y, 12.0, "energy", &format!("-{:.0} upkeep", e.upkeep), rgba(255, 190, 150, 255));
                }
                if ld.build_rate() > 0.0 {
                    stat(&hud, x, y, 12.0, "power", &format!("{:.0}", ld.build_rate()), c);
                }
                y += 17.0;
            }
            if let Some(tr) = &ld.transport {
                let aboard: Vec<&Info> = infos.values().filter(|c| c.host == u.id).collect();
                let used: u32 = aboard.iter().map(|c| fr.d(c.def).cargo as u32).sum();
                text(ix, y, &format!("Cargo: {} / {}", used, tr.capacity), 12.0, rgba(180, 225, 255, 255));
                y += 17.0;
            }
            if let Some(pr) = u.upg {
                text(ix, y, &format!("Upgrading: {:.0}%", pr * 100.0), 12.0, rgba(130, 225, 255, 255));
            } else if let Some(b) = u.factory.as_ref().and_then(|f| infos.get(&f.building)) {
                text(ix, y, &format!("Building {}: {:.0}%", fr.d(b.def).name, b.built.unwrap_or(1.0) * 100.0), 12.0, rgba(130, 225, 255, 255));
            }
        } else {
            // Selection by type: one icon per unit type with a count, like SupCom.
            let mut counts: Vec<(DefId, usize, f32)> = vec![];
            for u in &units {
                let frac = u.hp / fr.d(u.def).hp;
                match counts.iter_mut().find(|c| c.0 == u.def) {
                    Some(c) => {
                        c.1 += 1;
                        c.2 += frac;
                    }
                    None => counts.push((u.def, 1, frac)),
                }
            }
            counts.sort_by_key(|c| c.0);
            for (k, (d, n, hp_sum)) in counts.iter().take(12).enumerate() {
                let r = rect(ix + (k % 6) as f32 * 48.0, py + 8.0 + (k / 6) as f32 * 52.0, 44.0, 48.0);
                let hov = button(r, false);
                let dd = fr.d(*d);
                let ir = rect(r.min.x + 6.0, r.min.y + 2.0, 32.0, 32.0);
                match p.assets.portrait(&ctx, dd) {
                    Some(t) => hud.image(t.id(), ir, UV, Color32::WHITE),
                    None => hud.image(p.assets.icon(&ctx, dd).id(), ir, UV, my_col),
                };
                hud.text(Pos2::new(r.max.x - 3.0, r.min.y + 1.0), Align2::RIGHT_TOP, n.to_string(), font(13.0), rgba(255, 230, 90, 255));
                let hp = (hp_sum / *n as f32).clamp(0.0, 1.0);
                hud.rect_filled(rect(r.min.x + 4.0, r.max.y - 8.0, 36.0, 4.0), 0.0, rgba(0, 0, 0, 230));
                hud.rect_filled(rect(r.min.x + 4.0, r.max.y - 8.0, 36.0 * hp, 4.0), 0.0, if hp > 0.6 { rgba(60, 220, 80, 255) } else { rgba(235, 180, 50, 255) });
                if hov {
                    tip = Some((*d, 0.0));
                    if lmb_down {
                        // Click: keep only this type. Shift-click: drop this type.
                        new_sel = Some(units.iter().filter(|u| (u.def == *d) != shift).map(|u| u.id).collect());
                    }
                }
            }
        }

        // Production queue: one icon per run of the same unit, in build order. Drag to reorder,
        // right-click takes one off.
        if let Some(f) = lead.factory.as_ref().filter(|f| !f.queue.is_empty()) {
            let runs = queue_runs(&f.queue);
            let qy = panel.min.y - QUEUE_H - 2.0;
            let shown = runs.len().min((((panel.width() - 8.0) / QUEUE_W) as usize).max(1));
            hud.rect_filled(rect(panel.min.x, qy - 2.0, shown as f32 * QUEUE_W + 8.0, QUEUE_H + 2.0), 4.0, panel_bg);
            let mut start = 0usize;
            for (k, (d, n)) in runs.iter().take(shown).enumerate() {
                let r = rect(panel.min.x + 4.0 + k as f32 * QUEUE_W, qy, QUEUE_W - 4.0, QUEUE_H - 4.0);
                let dragged = view.queue_drag == Some((lead.id, k));
                let hov = button(r, dragged);
                let dd = fr.d(*d);
                let ir = egui::Rect::from_center_size(r.center(), egui::vec2(34.0, 34.0));
                let tint = if dragged { rgba(255, 255, 255, 90) } else { Color32::WHITE };
                match p.assets.portrait(&ctx, dd) {
                    Some(t) => hud.image(t.id(), ir, UV, tint),
                    None => hud.image(p.assets.icon(&ctx, dd).id(), ir, UV, my_col),
                };
                if *n > 1 {
                    hud.text(Pos2::new(r.max.x - 3.0, r.max.y - 1.0), Align2::RIGHT_BOTTOM, n.to_string(), font(14.0), rgba(255, 230, 80, 255));
                }
                if k == 0 {
                    if let Some(pr) = infos.get(&f.building).and_then(|b| b.built) {
                        hud.rect_filled(rect(r.min.x + 2.0, r.min.y + 2.0, (r.width() - 4.0) * pr.clamp(0.0, 1.0), 3.0), 0.0, rgba(100, 200, 255, 255));
                    }
                }
                if hov && view.queue_drag.is_none() {
                    tip = Some((*d, 0.0));
                    if lmb_down {
                        view.queue_drag = Some((lead.id, k));
                    } else if rmb_down {
                        out.push(Command::Dequeue { factory: lead.id, index: (start + n - 1) as u16 });
                    }
                }
                start += n;
            }
            if let Some((_, from_run)) = view.queue_drag.filter(|d| d.0 == lead.id) {
                // Where it would land, and the icon under the cursor.
                let gap = (((mouse.x - panel.min.x - 4.0) / QUEUE_W).round().max(0.0) as usize).min(runs.len());
                let gx = panel.min.x + 2.0 + gap as f32 * QUEUE_W;
                hud.line_segment([Pos2::new(gx, qy - 2.0), Pos2::new(gx, qy + QUEUE_H - 2.0)], Stroke::new(3.0, rgba(120, 255, 150, 255)));
                if let Some((d, _)) = runs.get(from_run) {
                    let dd = fr.d(*d);
                    let ir = egui::Rect::from_center_size(mouse, egui::vec2(34.0, 34.0));
                    match p.assets.portrait(&ctx, dd) {
                        Some(t) => hud.image(t.id(), ir, UV, Color32::WHITE),
                        None => hud.image(p.assets.icon(&ctx, dd).id(), ir, UV, my_col),
                    };
                }
            }
        }

        // Build menu.
        let bx0 = panel.min.x + 310.0;
        let (bw, bh) = (52.0, 60.0);
        let per_row = (((panel.width() - 318.0) / (bw + 4.0)).floor() as usize).max(1);
        let is_factory = ld.is_factory();
        let builders = ld.is_engineer() || is_factory;
        if builders {
            for (k, d) in ld.builds.iter().enumerate() {
                let r = rect(bx0 + (k % per_row) as f32 * (bw + 4.0), py + 6.0 + (k / per_row) as f32 * (bh + 4.0), bw, bh);
                let bd = fr.d(*d);
                let hov = button(r, view.mode == Mode::Place(*d));
                let ir = rect(r.min.x + 6.0, r.min.y + 3.0, 40.0, 40.0);
                match p.assets.portrait(&ctx, bd) {
                    Some(t) => {
                        hud.image(t.id(), ir, UV, Color32::WHITE);
                        let s = if p.settings.big_icons { 27.0 } else { 17.0 };
                        hud.image(p.assets.icon(&ctx, bd).id(), rect(r.min.x + 1.0, r.min.y + 1.0, s, s), UV, my_col);
                    }
                    None => {
                        hud.image(p.assets.icon(&ctx, bd).id(), ir, UV, my_col);
                    }
                };
                stat(&hud, r.min.x + 1.0, r.max.y - 15.0, 10.5, "mass", &format!("{:.0}", bd.cost.mass), rgba(190, 215, 240, 255));
                if let (true, Some(k)) = (faf, hot_key(bd)) {
                    hud.text(Pos2::new(r.max.x - 3.0, r.max.y - 30.0), Align2::RIGHT_TOP, key_letter(k), font(10.5), rgba(150, 230, 255, 255));
                }
                if bd.tier > 1 {
                    hud.text(Pos2::new(r.max.x - 3.0, r.min.y + 1.0), Align2::RIGHT_TOP, format!("T{}", bd.tier), font(10.5), rgba(255, 200, 120, 255));
                }
                if is_factory {
                    let n = lead.factory.as_ref().map_or(0, |f| f.queue.iter().filter(|q| **q == *d).count());
                    if n > 0 {
                        hud.text(Pos2::new(r.max.x - 3.0, r.max.y - 17.0), Align2::RIGHT_TOP, n.to_string(), font(15.0), rgba(255, 230, 80, 255));
                    }
                }
                if hov {
                    tip = Some((*d, ld.build_rate().max(1.0)));
                    if is_factory {
                        let n: i16 = if shift { 5 } else { 1 };
                        if lmb_down || rmb_down {
                            for f in units.iter().filter(|u| u.def == lead.def) {
                                out.push(Command::Produce { factory: f.id, def: *d, count: if lmb_down { n } else { -n } });
                            }
                        }
                    } else if lmb_down {
                        new_mode = Some(Mode::Place(*d));
                    }
                }
            }
        }
        // Order buttons: icon, hotkey letter in the corner, name on hover.
        let order_tex = p.assets.order_icons(&ctx);
        let mut ax = panel.min.x + 10.0;
        let ay = panel.max.y - 36.0;
        let mut action = |icon: &str, label: &str, key: &str, active: bool| -> bool {
            let r = rect(ax, ay, 28.0, 30.0);
            ax += 30.0;
            let hov = button(r, active);
            if let Some(t) = order_tex.get(icon) {
                hud.image(t.id(), egui::Rect::from_center_size(r.center(), egui::vec2(22.0, 22.0)), UV, if active { rgba(170, 255, 190, 255) } else { Color32::WHITE });
            }
            if !key.is_empty() {
                hud.text(Pos2::new(r.max.x - 2.0, r.max.y - 1.0), Align2::RIGHT_BOTTOM, key, font(9.5), rgba(150, 230, 255, 255));
            }
            if hov {
                let tipl = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("order_tip")));
                let full = if key.is_empty() { label.to_string() } else { format!("{label}  [{key}]") };
                let w = full.len() as f32 * 6.6 + 14.0;
                let tr = rect(r.min.x, r.min.y - 24.0, w, 20.0);
                tipl.rect_filled(tr, 3.0, rgba(12, 15, 20, 245));
                tipl.text(tr.center(), Align2::CENTER_CENTER, full, font(12.0), Color32::WHITE);
            }
            hov && lmb_down
        };
        let any_mobile = units.iter().any(|u| !fr.d(u.def).is_structure());
        let any_armed = units.iter().any(|u| fr.d(u.def).armed());
        let can_assist = units.iter().any(|u| fr.d(u.def).can_assist());
        let can_reclaim = can_assist || units.iter().any(|u| fr.d(u.def).is_scavenger());
        let can_capture = units.iter().any(|u| fr.d(u.def).is_engineer());
        if any_mobile {
            if faf && action("move", "Move", "W", view.mode == Mode::Move) {
                new_mode = Some(Mode::Move);
            }
            if action("attack_move", "Attack-move", if faf { "Alt+RMB" } else { "A" }, view.mode == Mode::AttackMove) {
                new_mode = Some(Mode::AttackMove);
            }
            if action("patrol", "Patrol", if faf { "Alt+W" } else { "P" }, view.mode == Mode::Patrol) {
                new_mode = Some(Mode::Patrol);
            }
        }
        if any_armed && action("ground", if faf { "Attack / ground fire" } else { "Ground fire" }, if faf { "R" } else { "G" }, view.mode == Mode::GroundAttack) {
            new_mode = Some(Mode::GroundAttack);
        }
        if can_capture && action("capture", "Capture a neutral or enemy unit", "", view.mode == Mode::Capture) {
            new_mode = Some(Mode::Capture);
        }
        if can_reclaim && action("reclaim", "Reclaim wrecks, trees and rocks (drag = area); click a building to take it apart", if faf { "Q" } else { "E" }, view.mode == Mode::Reclaim) {
            new_mode = Some(Mode::Reclaim);
        }
        if units.iter().any(|u| fr.d(u.def).is_scavenger()) && action("revive", "Revive wrecks as your own units (drag = area)", "J", view.mode == Mode::Revive) {
            new_mode = Some(Mode::Revive);
        }
        if can_capture && action("terraform", "Terraform: level, raise or lower the ground", "", view.mode == Mode::Terraform) {
            new_mode = Some(Mode::Terraform);
        }
        let any_transport = units.iter().any(|u| fr.d(u.def).transport.is_some());
        if (any_transport || units.iter().any(|u| carriable(fr.d(u.def)))) && action("load", if any_transport { "Load units (drag = area)" } else { "Board a transport" }, "L", view.mode == Mode::Load) {
            new_mode = Some(Mode::Load);
        }
        if any_transport && action("unload", "Unload (drag = area)", "K", view.mode == Mode::Unload) {
            new_mode = Some(Mode::Unload);
        }
        if can_assist && faf && action("assist", "Assist / repair", "I", view.mode == Mode::Assist) {
            new_mode = Some(Mode::Assist);
        }
        if action("stop", "Stop", if faf { "Esc" } else { "S" }, false) {
            do_stop = true;
        }
        // Pause: builders and factories stop working, pumps and radars switch off and cost no energy.
        let pausable = units.iter().any(|u| {
            let d = fr.d(u.def);
            d.build.is_some() || d.eco.upkeep > 0.0 || d.radar > 0.0
        });
        let all_paused = units.iter().all(|u| u.paused);
        if pausable && action(if all_paused { "resume" } else { "pause" }, if all_paused { "Resume" } else { "Pause" }, "Tab", all_paused) {
            do_pause = true;
        }
        if is_factory {
            let on = lead.factory.as_ref().map_or(false, |f| f.repeat);
            if action("repeat", if on { "Repeat build: on" } else { "Repeat build: off" }, if faf { "`" } else { "R" }, on) {
                do_repeat = true;
            }
        }
        if units.iter().any(|u| fr.d(u.def).upgrade_id.is_some() && u.upg.is_none()) && action("upgrade", "Upgrade", if faf { "O" } else { "U" }, false) {
            do_upgrade = true;
        }
    } else {
        text(MM + 22.0, sh - 22.0, "LMB select    RMB order    RMB-drag line: spread units    wheel zoom    arrows / MMB pan    F1 help    Esc menu", 12.0, rgba(200, 200, 200, 230));
    }
    if do_stop {
        out.push(Command::Stop { ids: view.sel.clone() });
    }
    if do_pause && !units.is_empty() {
        let on = !units.iter().all(|u| u.paused);
        out.push(Command::Pause { ids: view.sel.clone(), on });
    }
    if do_upgrade {
        out.push(Command::Upgrade { ids: view.sel.clone() });
    }
    if do_repeat {
        let facs: Vec<&Info> = units.iter().copied().filter(|u| u.factory.is_some()).collect();
        if let Some(f) = facs.first() {
            let on = !f.factory.as_ref().map_or(false, |f| f.repeat);
            out.push(Command::Repeat { ids: facs.iter().map(|u| u.id).collect(), on });
        }
    }

    // Tooltip above the panel.
    if let Some((d, rate)) = tip {
        let bd = fr.d(d);
        let mut lines = vec![format!("{}   (tier {})", bd.name, bd.tier.max(1))];
        if rate > 0.0 {
            // Drawn as a row of icons below (the marker keeps its place in the list).
            lines.push(COST_ROW.into());
            lines.push(format!("HP {:.0}   Armor {}{}", bd.hp, bd.armor, if bd.radar_stealth { "   Invisible to radar" } else { "" }));
        } else {
            lines.push(format!("HP {:.0}   Armor {}", bd.hp, bd.armor));
        }
        lines.push(bd.desc.clone());
        for w in &bd.weapons {
            let mut t = format!("{}: {:.0} {} dmg", w.name, w.damage, w.damage_type);
            if w.salvo > 1 {
                t += &format!(" x{}", w.salvo);
            }
            t += &format!(" every {:.1}s, range {:.0}", w.reload, w.range);
            if w.min_range > 0.0 {
                t += &format!(" (min {:.0})", w.min_range);
            }
            if w.radius > 0.0 {
                t += &format!(", blast {:.0}", w.radius);
            }
            if let Some(tu) = &w.turret {
                t += &format!(", traverse {:.0} deg/s", tu.yaw_speed);
            }
            if w.aim_time > 0.0 {
                t += &format!(", aim {:.1}s", w.aim_time);
            }
            if w.hits_air && !w.hits_land {
                t += ", anti-air";
            }
            lines.push(t);
        }
        if bd.mv.speed > 0.0 {
            lines.push(format!("Speed {:.1} (reverse {:.1})   Sight {:.0}{}", bd.mv.speed, bd.mv.reverse, bd.vision, if bd.radar > 0.0 { format!("   Radar {:.0}", bd.radar) } else { String::new() }));
        }
        if rate > 0.0 && ld_is_factory(&units, &fr) {
            lines.push("Left-click: queue (+Shift x5).  Right-click: remove.  Drag the queue strip to reorder.".into());
        }
        let w = lines.iter().map(|l| l.len() as f32 * 6.4).fold(330.0, f32::max) + 24.0;
        let h = lines.len() as f32 * 17.0 + 12.0;
        let (tx, ty) = ((mouse.x - 20.0).min(sw - w - 8.0).max(8.0), panel.min.y - h - 6.0);
        let top = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("tip")));
        top.rect_filled(rect(tx, ty, w, h), 3.0, rgba(12, 15, 20, 245));
        top.rect_stroke(rect(tx, ty, w, h), 3.0, Stroke::new(1.0, line_c), StrokeKind::Inside);
        for (k, l) in lines.iter().enumerate() {
            let (x, y) = (tx + 10.0, ty + 6.0 + k as f32 * 17.0);
            if l == COST_ROW {
                let mut x = stat(&top, x, y, 12.5, "mass", &format!("{:.0}", bd.cost.mass), Color32::WHITE);
                x = stat(&top, x, y, 12.5, "energy", &format!("{:.0}", bd.cost.energy), Color32::WHITE);
                x = stat(&top, x, y, 12.5, "time", &format!("{:.0}s", bd.cost.time / rate), Color32::WHITE);
                if bd.build_rate() > 0.0 {
                    stat(&top, x, y, 12.5, "power", &format!("{:.0}", bd.build_rate()), Color32::WHITE);
                }
                continue;
            }
            top.text(Pos2::new(x, y), Align2::LEFT_TOP, l, font(12.5), if k == 0 { rgba(255, 230, 130, 255) } else { Color32::WHITE });
        }
    }
    if let Some(s) = new_sel {
        view.sel = s;
    }
    if let Some(m) = new_mode {
        if m == Mode::Terraform && view.mode != Mode::Terraform {
            view.terra_delta = 0.0;
        }
        view.mode = m;
    }
    // Sounds: the unit answers when it is newly selected, and an order gets a short blip.
    if view.sel != view.last_sel {
        if let Some(u) = view.sel.iter().find(|id| !view.last_sel.contains(id)).and_then(|id| infos.get(id)) {
            let com = defs.factions[player.faction as usize].commander == Some(u.def);
            p.ui_sounds.0.push((super::sound::select_sound(fr.d(u.def), com), 0.55));
        }
        view.last_sel = view.sel.clone();
    }
    if out[out_before.min(out.len())..].iter().any(|c| matches!(c, Command::Orders { .. })) {
        p.ui_sounds.0.push(("ack", 0.4));
    }

    // --- mode hint, banners
    let label = match view.mode {
        Mode::AttackMove => "ATTACK-MOVE: left-click, or drag a line to spread out   (right-click cancels)".to_string(),
        Mode::Patrol => "PATROL: left-click a point".to_string(),
        Mode::GroundAttack => "GROUND FIRE: click a spot or a unit, or drag a circle to shell an area".to_string(),
        Mode::Reclaim => "RECLAIM: click a wreck, tree, rock or building, or drag a circle to collect every wreck, tree and rock inside".to_string(),
        Mode::Move => "MOVE: left-click, or drag a line to spread out".to_string(),
        Mode::Assist => "ASSIST / REPAIR: left-click a friendly unit or building".to_string(),
        Mode::Capture => "CAPTURE: left-click a neutral or enemy unit (not commanders or aircraft)".to_string(),
        Mode::Revive => "REVIVE: click a wreck, or drag a circle to bring back everything inside".to_string(),
        Mode::Terraform if view.terra_delta == 0.0 => "TERRAFORM: drag a circle to level it to the height at its centre.  Mouse wheel: raise or lower instead".to_string(),
        Mode::Terraform => format!("TERRAFORM: drag a circle to {} it by {:.0}.  Mouse wheel changes the amount (0 = level)", if view.terra_delta > 0.0 { "raise" } else { "lower" }, view.terra_delta.abs()),
        Mode::Load if selected.iter().any(|u| fr.d(u.def).transport.is_some()) => "LOAD: click a unit, or drag a circle to pick up everything inside".to_string(),
        Mode::Load => "LOAD: click the transport to board".to_string(),
        Mode::Unload => "UNLOAD: click or drag a circle; the cargo is set down spread over it".to_string(),
        Mode::Place(d) if fr.d(d).needs_deposit => format!("BUILD {}: click a deposit, or drag a circle to queue every deposit inside", fr.d(d).name),
        Mode::Place(d) => format!("BUILD {}: click, or drag for a row at any angle (hold Ctrl for a filled block). Shift keeps placing.", fr.d(d).name),
        Mode::None => String::new(),
    };
    if !label.is_empty() {
        text_c(sw * 0.5, TOP + 20.0, &label, 15.0, rgba(255, 230, 130, 255));
    }
    if let Some(by) = view.paused {
        let who = game.players.get(by as usize).map_or(String::new(), |pl| if by == view.me { " by you".to_string() } else { format!(" by {}", pl.name) });
        text_c(sw * 0.5, sh * 0.22, "PAUSED", 56.0, rgba(255, 215, 90, 255));
        text_c(sw * 0.5, sh * 0.22 + 40.0, &format!("Paused{who}.  Pause, Ctrl+P or the button at the top resumes."), 16.0, Color32::WHITE);
    }
    if let Some(w) = game.winner {
        let (t, c) = if w == team { ("VICTORY", rgba(110, 255, 130, 255)) } else { ("DEFEAT", rgba(255, 95, 80, 255)) };
        text_c(sw * 0.5, sh * 0.3, t, 72.0, c);
        text_c(sw * 0.5, sh * 0.3 + 50.0, "Press Esc for the menu", 18.0, Color32::WHITE);
    } else if !player.alive {
        text_c(sw * 0.5, TOP + 44.0, "Your commander was destroyed - spectating", 20.0, rgba(255, 130, 105, 255));
    }

    let top = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("top")));
    if view.show_help {
        let lines = [
            "CONTROLS",
            "Left click / drag: select, box select.  Double-click: all of that type on screen.  Shift: add.",
            "Right click: move / attack enemy / assist or repair friend / reclaim wreck (engineers alone: also trees and rocks).  Shift: queue.",
            "Right-drag a line: spread the selected units along it (Alt: attack-move).",
            if faf { "FAF alternative keys:  W move   Alt+W patrol   R attack / ground fire (drag = area)   Q reclaim (drag = area)" } else { "A attack-move    P patrol    S stop    G ground fire (drag = area)    E reclaim (drag = area)" },
            if faf { "I / T assist, repair   Esc stop   O upgrade   ` factory repeat   F10 menu   Alt+right-click attack-move" } else { "U upgrade (pumps, factories)    R factory repeat    . idle builder    , commander" },
            if faf { "Hotbuild (press again to cycle): Z builders/factories  X mass  C power  V radar/scouts  G direct fire" } else { "Hotkey preset can be switched to FAF alternative in the Esc menu." },
            if faf { "                                 S artillery  D anti-air  Y missiles  M engineering station" } else { "Ctrl+K self-destruct.  Shift+0-9 adds a group to the selection." },
            "Select: Ctrl+A air  Ctrl+D land army  Ctrl+B engineers  Ctrl+X all  Ctrl+C all on screen  Ctrl+Z same type  Ctrl+H factories",
            "        Ctrl+Shift+D / A nearest land / air factory   . next idle builder   , commander   Ctrl+K self-destruct",
            "Build menu: click, then click the map; drag for a row of buildings; pumps: drag a circle.",
            "Factory menu: left-click queues (Shift x5), right-click removes.",
            "Ctrl+0-9 set group, 0-9 recall (double tap: centre).  Selection panel: click a type to keep only it.",
            "Tab pauses / resumes the selection: builders and factories stop, pumps and radars switch off (no upkeep).",
            "Wheel zoom, arrows or middle-drag pan, minimap click.  F6 shadows on/off.  F11 fullscreen.  Pause / Ctrl+P pauses the game.",
            "Radar is blocked by hills.  Seen buildings stay marked until you look again.",
            "L load / board a transport   K unload (drag = area)   J revive wrecks (scavengers, drag = area)",
            "Reclaim: click a wreck, tree, rock or building, drag = every wreck, tree and rock in the circle.  Terraform button: drag a circle, wheel = raise / lower.",
            "A box selects the army; engineers and the commander only when no fighting unit is inside (or use Ctrl+B / ,).",
        ];
        let (w, h) = (820.0, lines.len() as f32 * 20.0 + 20.0);
        let r = rect(sw * 0.5 - w * 0.5, TOP + 50.0, w, h);
        top.rect_filled(r, 4.0, rgba(12, 15, 20, 240));
        for (k, l) in lines.iter().enumerate() {
            top.text(Pos2::new(r.min.x + 14.0, r.min.y + 10.0 + k as f32 * 20.0), Align2::LEFT_TOP, *l, font(13.5), if k == 0 { rgba(255, 230, 130, 255) } else { Color32::WHITE });
        }
    }
    if view.menu_open {
        top.rect_filled(rect(0.0, 0.0, sw, sh), 0.0, rgba(0, 0, 0, 130));
        let (w, h) = (300.0, 292.0);
        let (x, y) = (sw * 0.5 - w * 0.5, sh * 0.5 - h * 0.5);
        top.rect_filled(rect(x, y, w, h), 5.0, rgba(20, 24, 31, 252));
        top.rect_stroke(rect(x, y, w, h), 5.0, Stroke::new(1.5, line_c), StrokeKind::Inside);
        top.text(Pos2::new(sw * 0.5, y + 26.0), Align2::CENTER_CENTER, "MENU", font(22.0), Color32::WHITE);
        let pause_l = if view.paused.is_some() { "Resume game" } else { "Pause game" };
        for (k, l) in ["Back to game", pause_l, "Settings", "Resign", "Quit to main menu"].iter().enumerate() {
            let r = rect(x + 30.0, y + 52.0 + k as f32 * 46.0, w - 60.0, 36.0);
            let hov = r.contains(mouse);
            top.rect_filled(r, 3.0, if hov { rgba(64, 76, 94, 255) } else { rgba(34, 40, 50, 255) });
            top.rect_stroke(r, 3.0, Stroke::new(1.0, if hov { Color32::WHITE } else { line_c }), StrokeKind::Inside);
            top.text(r.center(), Align2::CENTER_CENTER, *l, font(16.0), Color32::WHITE);
            if hov && lmb_down {
                match k {
                    0 => view.menu_open = false,
                    1 => {
                        view.pause_toggle = true;
                        view.menu_open = false;
                    }
                    2 => {
                        view.settings_open = true;
                        view.menu_open = false;
                    }
                    3 => {
                        out.push(Command::Resign);
                        view.menu_open = false;
                    }
                    _ => view.quit = true,
                }
            }
        }
    }
    // Settings window (opened from the Esc menu).
    if view.settings_open {
        let adapter = p.adapter.as_ref().map_or("unknown".to_string(), |a| format!("{} ({:?})", a.name, a.backend));
        let mut close = false;
        egui::Window::new("settings").title_bar(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(&ctx, |ui| {
            close = crate::settings::settings_ui(ui, &mut p.settings, &p.started.0, &adapter, &p.render_errors.0, true);
        });
        if close || p.keys.just_pressed(KeyCode::Escape) {
            view.settings_open = false;
        }
    }
    let _ = bcol;
    Ok(())
}

fn ld_is_factory(units: &[&Info], fr: &Frame) -> bool {
    units.iter().find(|u| !fr.d(u.def).builds.is_empty()).map_or(false, |u| fr.d(u.def).is_factory())
}
