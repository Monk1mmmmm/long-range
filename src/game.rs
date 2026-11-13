//! The in-game screen: camera, selection, orders, HUD and world rendering.
//! It only reads the simulation and returns `Command`s, so it works the same
//! for the host and for network clients.
use crate::defs::*;
use crate::map::*;
use crate::render::*;
use crate::sim::*;
use macroquad::prelude::*;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    None,
    AttackMove,
    Patrol,
    Place(DefId),
}

#[derive(Clone, Copy, PartialEq)]
enum MoveKind {
    Move,
    AttackMove,
    Patrol,
}

/// A unit the local player can currently see (or has on radar), with interpolated transform.
struct Seen {
    id: u32,
    idx: usize,
    pos: V2,
    heading: f32,
    aim: f32,
    sp: Vec2,
    on_screen: bool,
    rpx: f32,
    radar_only: bool,
    mine: bool,
    enemy: bool,
}

const TOP: f32 = 30.0;
const MM: f32 = 190.0;
const PANEL_H: f32 = 126.0;
const FOV: f32 = 0.785;

pub struct GameView {
    pub me: u8,
    pub quit: bool,
    cam_target: V2,
    cam_dist: f32,
    vp: Mat4,
    inv: Mat4,
    sel: Vec<u32>,
    groups: Vec<Vec<u32>>,
    mode: Mode,
    box_start: Option<Vec2>,
    place_start: Option<V2>,
    pathing: Option<MouseButton>,
    path: Vec<V2>,
    last_click: (f64, DefId),
    last_group: (f64, usize),
    idle_cycle: usize,
    prev: HashMap<u32, (V2, f32, f32)>,
    cur: HashMap<u32, (V2, f32, f32)>,
    last_tick: u32,
    tick_time: f64,
    fx: Vec<(V2, f32, f32, f64)>,
    ground_tex: Texture2D,
    fog_tex: Texture2D,
    fog_img: Image,
    rocks: Vec<(V2, f32, Mesh)>,
    models: Vec<Vec<Part>>,
    batch: Batch,
    menu_open: bool,
    mmb_last: Option<Vec2>,
}

fn ctrl() -> bool {
    is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl)
}
fn shift() -> bool {
    is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift)
}
fn alt() -> bool {
    is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt)
}

fn text_c(t: &str, cx: f32, y: f32, size: f32, c: Color) {
    let m = measure_text(t, None, size as u16, 1.0);
    draw_text(t, cx - m.width * 0.5, y, size, c);
}

fn path_len(p: &[V2]) -> f32 {
    p.windows(2).map(|w| w[0].dist(w[1])).sum()
}

/// `n` evenly spaced points (with tangents) along a polyline.
fn resample(path: &[V2], n: usize) -> Vec<(V2, V2)> {
    let total = path_len(path);
    let mut out = vec![];
    if path.len() < 2 || total < 0.01 {
        return vec![(path[0], v2(1.0, 0.0)); n];
    }
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

impl GameView {
    pub fn new(sim: &Sim, me: u8) -> GameView {
        let px = if sim.map.n <= 256 { 4 } else { 2 };
        let ground_tex = Texture2D::from_image(&ground_image(&sim.map, px));
        ground_tex.set_filter(FilterMode::Linear);
        let n = sim.map.n as u16;
        let fog_img = Image::gen_image_color(n, n, Color::new(0.0, 0.0, 0.0, 0.0));
        let fog_tex = Texture2D::from_image(&fog_img);
        fog_tex.set_filter(FilterMode::Linear);
        let start = sim.s.players.get(me as usize).map(|p| p.start).unwrap_or(v2(100.0, 100.0));
        GameView {
            me,
            quit: false,
            cam_target: start,
            cam_dist: 75.0,
            vp: Mat4::IDENTITY,
            inv: Mat4::IDENTITY,
            sel: vec![],
            groups: vec![vec![]; 10],
            mode: Mode::None,
            box_start: None,
            place_start: None,
            pathing: None,
            path: vec![],
            last_click: (0.0, 255),
            last_group: (0.0, 99),
            idle_cycle: 0,
            prev: HashMap::new(),
            cur: HashMap::new(),
            last_tick: u32::MAX,
            tick_time: 0.0,
            fx: vec![],
            ground_tex,
            fog_tex,
            fog_img,
            rocks: rock_chunks(&sim.map),
            models: DEFS.iter().map(model).collect(),
            batch: Batch::new(),
            menu_open: false,
            mmb_last: None,
        }
    }

    fn my_team(&self, sim: &Sim) -> u8 {
        sim.s.players[self.me as usize].team
    }
    fn reveal(&self, sim: &Sim) -> bool {
        !sim.s.players[self.me as usize].alive || sim.s.winner.is_some()
    }

    /// Call after the simulation state changed (new tick or new snapshot).
    pub fn sync(&mut self, sim: &Sim) {
        if sim.s.tick == self.last_tick {
            return;
        }
        self.last_tick = sim.s.tick;
        self.tick_time = get_time();
        self.prev = std::mem::take(&mut self.cur);
        for u in &sim.s.units {
            self.cur.insert(u.id, (u.pos, u.heading, u.aim));
        }
        let team = self.my_team(sim);
        let reveal = self.reveal(sim);
        for f in &sim.s.fx {
            // Only show explosions in places we can see.
            let visible = reveal
                || sim.s.units.iter().any(|u| {
                    sim.team_of(u.owner) == team && u.done() && u.pos.dist(f.pos) < u.d().vision.max(u.d().radar) + f.r
                });
            if visible {
                self.fx.push((f.pos, f.r, f.h, self.tick_time));
            }
        }
        let me = self.me;
        self.sel.retain(|id| sim.unit(*id).map_or(false, |u| u.owner == me));
        // Fog of war texture: dark = unseen, dim = radar only, clear = vision.
        let n = sim.map.n as i32;
        for px in self.fog_img.bytes.chunks_exact_mut(4) {
            px[3] = if reveal { 0 } else { 135 };
        }
        if !reveal {
            let mut stamp = |pos: V2, r: f32, a: u8| {
                let (cx, cy, rc) = (pos.x / CELL, pos.y / CELL, r / CELL);
                for y in ((cy - rc) as i32).max(0)..=((cy + rc) as i32).min(n - 1) {
                    for x in ((cx - rc) as i32).max(0)..=((cx + rc) as i32).min(n - 1) {
                        let d2 = (x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2);
                        if d2 <= rc * rc {
                            let b = &mut self.fog_img.bytes[((y * n + x) * 4 + 3) as usize];
                            *b = (*b).min(a);
                        }
                    }
                }
            };
            let power_out: Vec<bool> = sim.s.players.iter().map(|p| p.power_out).collect();
            for u in &sim.s.units {
                if sim.team_of(u.owner) != team || !u.done() || u.host != 0 {
                    continue;
                }
                let d = u.d();
                if d.radar > 0.0 && !power_out[u.owner as usize] {
                    stamp(u.pos, d.radar, 75);
                }
                stamp(u.pos, d.vision, 0);
            }
        }
        self.fog_tex.update(&self.fog_img);
    }

    fn camera(&self) -> Camera3D {
        let d = self.cam_dist;
        let t = ((d - 120.0) / 400.0).clamp(0.0, 1.0);
        let t = t * t * (3.0 - 2.0 * t);
        let pitch = (52.0 + 32.0 * t).to_radians();
        Camera3D {
            position: vec3(self.cam_target.x, d * pitch.sin(), self.cam_target.y + d * pitch.cos()),
            target: vec3(self.cam_target.x, 0.0, self.cam_target.y),
            up: vec3(0.0, 1.0, 0.0),
            fovy: FOV,
            z_near: (d * 0.05).max(0.5),
            z_far: d * 6.0 + 800.0,
            ..Default::default()
        }
    }

    fn set_matrices(&mut self) {
        let cam = self.camera();
        self.vp = cam.matrix();
        self.inv = self.vp.inverse();
    }

    /// Screen position to a point on the ground plane.
    fn s2g(&self, m: Vec2) -> V2 {
        let (x, y) = (2.0 * m.x / screen_width() - 1.0, 1.0 - 2.0 * m.y / screen_height());
        let a = self.inv.project_point3(vec3(x, y, -1.0));
        let b = self.inv.project_point3(vec3(x, y, 1.0));
        let d = b - a;
        if d.y.abs() < 1e-6 {
            return self.cam_target;
        }
        let h = a + d * (-a.y / d.y);
        v2(h.x, h.z)
    }

    /// World point to screen position plus view depth.
    fn w2s(&self, p: Vec3) -> Option<(Vec2, f32)> {
        let c = self.vp * vec4(p.x, p.y, p.z, 1.0);
        if c.w <= 0.05 {
            return None;
        }
        Some((vec2((c.x / c.w * 0.5 + 0.5) * screen_width(), (0.5 - c.y / c.w * 0.5) * screen_height()), c.w))
    }

    fn px_per_unit(&self, depth: f32) -> f32 {
        screen_height() / (2.0 * (FOV * 0.5).tan() * depth)
    }

    fn mm_rect(&self) -> Rect {
        Rect::new(8.0, screen_height() - MM - 8.0, MM, MM)
    }
    fn panel_rect(&self) -> Rect {
        let x = MM + 18.0;
        Rect::new(x, screen_height() - PANEL_H - 8.0, screen_width() - x - 8.0, PANEL_H)
    }

    fn camera_input(&mut self, sim: &Sim, mouse: Vec2, over_hud: bool) {
        let dt = get_frame_time().min(0.1);
        let world = sim.map.world();
        let pan = self.cam_dist * 0.9 * dt;
        if is_key_down(KeyCode::Left) {
            self.cam_target.x -= pan;
        }
        if is_key_down(KeyCode::Right) {
            self.cam_target.x += pan;
        }
        if is_key_down(KeyCode::Up) {
            self.cam_target.y -= pan;
        }
        if is_key_down(KeyCode::Down) {
            self.cam_target.y += pan;
        }
        // Middle mouse drag pans.
        if is_mouse_button_down(MouseButton::Middle) {
            if let Some(last) = self.mmb_last {
                let (a, b) = (self.s2g(last), self.s2g(mouse));
                self.cam_target = self.cam_target + (a - b);
                self.set_matrices();
            }
            self.mmb_last = Some(mouse);
        } else {
            self.mmb_last = None;
        }
        // Wheel zooms toward the cursor, strategic-zoom style.
        let wheel = mouse_wheel().1;
        if wheel != 0.0 && !over_hud {
            let before = self.s2g(mouse);
            let f = if wheel > 0.0 { 0.85 } else { 1.0 / 0.85 };
            self.cam_dist = (self.cam_dist * f).clamp(22.0, world * 1.35);
            self.set_matrices();
            // Zooming in homes in on the cursor; zooming out keeps the view where it is.
            if wheel > 0.0 {
                let after = self.s2g(mouse);
                self.cam_target = self.cam_target + (before - after);
            }
        }
        self.cam_target = v2(self.cam_target.x.clamp(0.0, world), self.cam_target.y.clamp(0.0, world));
        self.set_matrices();
    }

    fn own_selected<'a>(&self, sim: &'a Sim) -> Vec<&'a Unit> {
        self.sel.iter().filter_map(|id| sim.unit(*id)).filter(|u| u.owner == self.me).collect()
    }

    /// Issues move-type orders with BAR-like spreading: a drawn line distributes the
    /// units along it, a plain click arranges them in a loose block.
    fn order_move(&self, sim: &Sim, path: &[V2], kind: MoveKind, queue: bool, cmds: &mut Vec<Command>) {
        let units = self.own_selected(sim);
        let mk = |p: V2| match kind {
            MoveKind::Move => Order::Move(p),
            MoveKind::AttackMove => Order::AttackMove(p),
            MoveKind::Patrol => Order::Patrol(p),
        };
        let end = *path.last().unwrap();
        let rally: Vec<u32> = units.iter().filter(|u| u.d().kind == Kind::Factory).map(|u| u.id).collect();
        if !rally.is_empty() {
            cmds.push(Command::Orders { ids: rally, orders: vec![Order::Move(end)], queue: false });
        }
        let planes: Vec<u32> = units.iter().filter(|u| u.d().layer == Layer::Plane).map(|u| u.id).collect();
        if !planes.is_empty() {
            cmds.push(Command::Orders { ids: planes, orders: vec![mk(end)], queue });
        }
        let movers: Vec<&&Unit> = units.iter().filter(|u| !u.d().is_structure() && u.d().layer != Layer::Plane).collect();
        if movers.is_empty() {
            return;
        }
        let pts = self.formation(sim, &movers.iter().map(|u| **u).collect::<Vec<_>>(), path);
        // When queueing, measure from where each unit will be at the end of its queue.
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
        cmds.push(Command::Orders {
            ids: movers.iter().map(|u| u.id).collect(),
            orders: asg.iter().map(|k| mk(pts[*k])).collect(),
            queue,
        });
    }

    fn formation(&self, sim: &Sim, units: &[&Unit], path: &[V2]) -> Vec<V2> {
        let n = units.len();
        if n == 0 {
            return vec![];
        }
        let maxr = units.iter().map(|u| u.d().radius).fold(0.0, f32::max);
        let spacing = maxr * 2.0 + 2.2;
        let len = path_len(path);
        let mut pts = vec![];
        if len < 4.0 || path.len() < 2 {
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
                pts.push(sim.map.clamp(c + side * x + fwd * y));
            }
        } else {
            // Several ranks if the line is too short for everybody.
            let per_rank = ((len / spacing).floor() as usize + 1).clamp(1, n);
            let ranks = (n + per_rank - 1) / per_rank;
            let mut left = n;
            for r in 0..ranks {
                let m = left.min(per_rank);
                left -= m;
                for (p, tan) in resample(path, m) {
                    pts.push(sim.map.clamp(p - tan.perp() * (r as f32 * spacing)));
                }
            }
        }
        pts
    }

    fn build_line(&self, sim: &Sim, d: DefId, a: V2, b: V2) -> Vec<V2> {
        let df = def(d);
        let step = df.foot as f32 * CELL;
        let a = sim.snap(d, a);
        let mut out = vec![a];
        if df.needs_deposit {
            // Dragging over deposits queues an extractor on each of them.
            for dep in &sim.map.deposits {
                let ab = b - a;
                let t = ((*dep - a).dot(ab) / ab.len2().max(0.01)).clamp(0.0, 1.0);
                if (a + ab * t).dist(*dep) < 6.0 && !out.contains(dep) {
                    out.push(*dep);
                }
            }
            return out;
        }
        let dist = a.dist(b);
        let n = (dist / step).floor() as usize;
        let dir = (b - a).norm();
        // Keep lines axis aligned so buildings tile cleanly.
        let dir = if dir.x.abs() > dir.y.abs() { v2(dir.x.signum(), 0.0) } else { v2(0.0, dir.y.signum()) };
        for k in 1..=n.min(40) {
            let p = sim.snap(d, a + dir * (k as f32 * step));
            if !out.contains(&p) {
                out.push(p);
            }
        }
        out
    }

    fn select_idle_engineer(&mut self, sim: &Sim) {
        let idle: Vec<&Unit> =
            sim.s.units.iter().filter(|u| u.owner == self.me && u.d().is_builder() && u.done() && u.orders.is_empty()).collect();
        if idle.is_empty() {
            return;
        }
        self.idle_cycle = (self.idle_cycle + 1) % idle.len();
        let u = idle[self.idle_cycle];
        self.sel = vec![u.id];
        self.cam_target = u.pos;
    }

    fn keyboard(&mut self, sim: &Sim, cmds: &mut Vec<Command>) {
        if is_key_pressed(KeyCode::Escape) {
            if self.mode != Mode::None || self.pathing.is_some() {
                self.mode = Mode::None;
                self.pathing = None;
                self.place_start = None;
            } else {
                self.menu_open = !self.menu_open;
            }
        }
        if self.menu_open {
            return;
        }
        let has_sel = !self.sel.is_empty();
        if is_key_pressed(KeyCode::A) && has_sel {
            self.mode = Mode::AttackMove;
        }
        if is_key_pressed(KeyCode::P) && has_sel {
            self.mode = Mode::Patrol;
        }
        if is_key_pressed(KeyCode::S) && has_sel {
            cmds.push(Command::Stop { ids: self.sel.clone() });
        }
        if is_key_pressed(KeyCode::U) && has_sel {
            cmds.push(Command::Upgrade { ids: self.sel.clone() });
        }
        if is_key_pressed(KeyCode::R) && has_sel {
            let fac: Vec<&Unit> = self.own_selected(sim).into_iter().filter(|u| u.d().kind == Kind::Factory).collect();
            if let Some(f) = fac.first() {
                cmds.push(Command::Repeat { ids: fac.iter().map(|u| u.id).collect(), on: !f.repeat });
            }
        }
        if is_key_pressed(KeyCode::Period) {
            self.select_idle_engineer(sim);
        }
        if is_key_pressed(KeyCode::Comma) || is_key_pressed(KeyCode::Home) {
            if let Some(c) = sim.s.units.iter().find(|u| u.owner == self.me && u.def == COMMANDER) {
                self.sel = vec![c.id];
                self.cam_target = c.pos;
            }
        }
        let digits = [
            KeyCode::Key0,
            KeyCode::Key1,
            KeyCode::Key2,
            KeyCode::Key3,
            KeyCode::Key4,
            KeyCode::Key5,
            KeyCode::Key6,
            KeyCode::Key7,
            KeyCode::Key8,
            KeyCode::Key9,
        ];
        for (g, k) in digits.iter().enumerate() {
            if is_key_pressed(*k) {
                if ctrl() {
                    self.groups[g] = self.sel.clone();
                } else if !self.groups[g].is_empty() {
                    self.groups[g].retain(|id| sim.unit(*id).is_some());
                    self.sel = self.groups[g].clone();
                    let now = get_time();
                    if self.last_group.1 == g && now - self.last_group.0 < 0.4 && !self.sel.is_empty() {
                        let us = self.own_selected(sim);
                        if !us.is_empty() {
                            self.cam_target = us.iter().fold(v2(0.0, 0.0), |a, u| a + u.pos) * (1.0 / us.len() as f32);
                        }
                    }
                    self.last_group = (now, g);
                }
            }
        }
    }

    fn world_input(&mut self, sim: &Sim, seen: &[Seen], mouse: Vec2, over_hud: bool, cmds: &mut Vec<Command>) {
        let ground = sim.map.clamp(self.s2g(mouse));
        let hover = self.hover(seen, mouse, over_hud);
        let sh = shift();

        if is_mouse_button_pressed(MouseButton::Left) && !over_hud {
            match self.mode {
                Mode::Place(_) => self.place_start = Some(ground),
                Mode::AttackMove | Mode::Patrol => {
                    self.path = vec![ground];
                    self.pathing = Some(MouseButton::Left);
                }
                Mode::None => self.box_start = Some(mouse),
            }
        }
        if is_mouse_button_released(MouseButton::Left) {
            if let Some(start) = self.place_start.take() {
                if let Mode::Place(d) = self.mode {
                    let builders: Vec<u32> =
                        self.own_selected(sim).iter().filter(|u| u.d().builds.contains(&d) && u.d().is_builder()).map(|u| u.id).collect();
                    let mut first = true;
                    for p in self.build_line(sim, d, start, ground) {
                        if sim.can_place(d, p) && !builders.is_empty() {
                            cmds.push(Command::Orders {
                                ids: builders.clone(),
                                orders: vec![Order::Build { def: d, pos: p }],
                                queue: sh || !first,
                            });
                            first = false;
                        }
                    }
                    if !sh {
                        self.mode = Mode::None;
                    }
                }
            } else if self.pathing == Some(MouseButton::Left) {
                self.pathing = None;
                let kind = if self.mode == Mode::Patrol { MoveKind::Patrol } else { MoveKind::AttackMove };
                // Attack-move clicked directly on an enemy becomes a focused attack.
                let enemy = hover.filter(|h| seen[*h].enemy && path_len(&self.path) < 4.0);
                if let (Some(h), MoveKind::AttackMove) = (enemy, kind) {
                    cmds.push(Command::Orders { ids: self.sel.clone(), orders: vec![Order::Attack(seen[h].id)], queue: sh });
                } else {
                    let path = std::mem::take(&mut self.path);
                    self.order_move(sim, &path, kind, sh, cmds);
                }
                if !sh {
                    self.mode = Mode::None;
                }
            } else if let Some(b) = self.box_start.take() {
                if b.distance(mouse) < 6.0 {
                    let pick = hover.filter(|h| seen[*h].mine);
                    match pick {
                        Some(h) => {
                            let id = seen[h].id;
                            let d = sim.s.units[seen[h].idx].def;
                            let now = get_time();
                            if now - self.last_click.0 < 0.35 && self.last_click.1 == d {
                                let same: Vec<u32> = seen
                                    .iter()
                                    .filter(|s| s.mine && s.on_screen && sim.s.units[s.idx].def == d)
                                    .map(|s| s.id)
                                    .collect();
                                if sh {
                                    for s in same {
                                        if !self.sel.contains(&s) {
                                            self.sel.push(s);
                                        }
                                    }
                                } else {
                                    self.sel = same;
                                }
                            } else if sh {
                                if let Some(k) = self.sel.iter().position(|x| *x == id) {
                                    self.sel.remove(k);
                                } else {
                                    self.sel.push(id);
                                }
                            } else {
                                self.sel = vec![id];
                            }
                            self.last_click = (now, d);
                        }
                        None => {
                            if !sh {
                                self.sel.clear();
                            }
                        }
                    }
                } else {
                    let r = Rect::new(b.x.min(mouse.x), b.y.min(mouse.y), (b.x - mouse.x).abs(), (b.y - mouse.y).abs());
                    let inside: Vec<&Seen> = seen.iter().filter(|s| s.mine && s.on_screen && r.contains(s.sp)).collect();
                    let any_mobile = inside.iter().any(|s| !sim.s.units[s.idx].d().is_structure());
                    let ids: Vec<u32> = inside
                        .iter()
                        .filter(|s| !any_mobile || !sim.s.units[s.idx].d().is_structure())
                        .map(|s| s.id)
                        .collect();
                    if sh {
                        for id in ids {
                            if !self.sel.contains(&id) {
                                self.sel.push(id);
                            }
                        }
                    } else {
                        self.sel = ids;
                    }
                }
            }
        }

        if is_mouse_button_pressed(MouseButton::Right) && !over_hud {
            if self.mode != Mode::None {
                self.mode = Mode::None;
                self.place_start = None;
                self.pathing = None;
            } else if !self.sel.is_empty() {
                self.path = vec![ground];
                self.pathing = Some(MouseButton::Right);
            }
        }
        if self.pathing.is_some() && self.path.len() < 500 && self.path.last().map_or(false, |l| l.dist(ground) > 1.5) {
            self.path.push(ground);
        }
        if is_mouse_button_released(MouseButton::Right) && self.pathing == Some(MouseButton::Right) {
            self.pathing = None;
            let path = std::mem::take(&mut self.path);
            if path_len(&path) >= 4.0 {
                let kind = if alt() { MoveKind::AttackMove } else { MoveKind::Move };
                self.order_move(sim, &path, kind, sh, cmds);
            } else {
                self.context_order(sim, seen, hover, ground, sh, cmds);
            }
        }
    }

    fn context_order(&self, sim: &Sim, seen: &[Seen], hover: Option<usize>, ground: V2, queue: bool, cmds: &mut Vec<Command>) {
        let units = self.own_selected(sim);
        if units.is_empty() {
            return;
        }
        if let Some(h) = hover {
            let s = &seen[h];
            if s.enemy {
                cmds.push(Command::Orders { ids: self.sel.clone(), orders: vec![Order::Attack(s.id)], queue });
                return;
            }
            if !s.radar_only {
                let ids: Vec<u32> = self.sel.iter().copied().filter(|id| *id != s.id).collect();
                if !ids.is_empty() {
                    cmds.push(Command::Orders { ids, orders: vec![Order::Assist(s.id)], queue });
                    return;
                }
            }
        }
        let builders: Vec<u32> = units.iter().filter(|u| u.d().is_builder()).map(|u| u.id).collect();
        if !builders.is_empty() {
            if let Some(w) = sim.s.wrecks.iter().filter(|w| w.pos.dist(ground) < 3.0).min_by(|a, b| {
                a.pos.dist(ground).partial_cmp(&b.pos.dist(ground)).unwrap()
            }) {
                cmds.push(Command::Orders { ids: builders, orders: vec![Order::Reclaim(w.id)], queue });
                return;
            }
        }
        let kind = if alt() { MoveKind::AttackMove } else { MoveKind::Move };
        self.order_move(sim, &[ground], kind, queue, cmds);
    }

    fn hover(&self, seen: &[Seen], mouse: Vec2, over_hud: bool) -> Option<usize> {
        if over_hud {
            return None;
        }
        let mut best: Option<(f32, usize)> = None;
        for (k, s) in seen.iter().enumerate() {
            if !s.on_screen {
                continue;
            }
            let d = s.sp.distance(mouse);
            if d < s.rpx.max(9.0) && best.map_or(true, |b| d < b.0) {
                best = Some((d, k));
            }
        }
        best.map(|b| b.1)
    }

    // ------------------------------------------------------------------ frame

    pub fn frame(&mut self, sim: &Sim) -> Vec<Command> {
        let mut cmds = vec![];
        self.sync(sim);
        let now = get_time();
        let alpha = (((now - self.tick_time) / DT as f64) as f32).clamp(0.0, 1.0);
        let mouse: Vec2 = mouse_position().into();
        let (sw, shh) = (screen_width(), screen_height());
        let over_hud = mouse.y < TOP
            || self.mm_rect().contains(mouse)
            || (!self.sel.is_empty() && self.panel_rect().contains(mouse))
            || self.menu_open;
        self.set_matrices();
        self.camera_input(sim, mouse, over_hud);

        // Everything the local player may see this frame.
        let team = self.my_team(sim);
        let bit = 1u8 << team;
        let reveal = self.reveal(sim);
        let mut seen: Vec<Seen> = Vec::with_capacity(sim.s.units.len());
        for (idx, u) in sim.s.units.iter().enumerate() {
            if u.host != 0 {
                continue;
            }
            let vis = reveal || u.seen & bit != 0;
            if !vis && u.radar & bit == 0 {
                continue;
            }
            let c = self.cur.get(&u.id).copied().unwrap_or((u.pos, u.heading, u.aim));
            let p = self.prev.get(&u.id).copied().unwrap_or(c);
            let pos = p.0.lerp(c.0, alpha);
            let heading = p.1 + wrap(c.1 - p.1) * alpha;
            let aim = p.2 + wrap(c.2 - p.2) * alpha;
            let d = u.d();
            let (sp, on_screen, rpx) = match self.w2s(vec3(pos.x, d.alt + 0.6, pos.y)) {
                Some((sp, depth)) => {
                    let on = sp.x > -30.0 && sp.y > -30.0 && sp.x < sw + 30.0 && sp.y < shh + 30.0;
                    (sp, on, u.radius() * self.px_per_unit(depth))
                }
                None => (Vec2::ZERO, false, 0.0),
            };
            seen.push(Seen {
                id: u.id,
                idx,
                pos,
                heading,
                aim,
                sp,
                on_screen,
                rpx,
                radar_only: !vis,
                mine: u.owner == self.me,
                enemy: sim.team_of(u.owner) != team,
            });
        }

        if !self.menu_open {
            self.world_input(sim, &seen, mouse, over_hud, &mut cmds);
        }
        self.keyboard(sim, &mut cmds);

        clear_background(Color::new(0.05, 0.06, 0.08, 1.0));
        let cam = self.camera();
        set_camera(&cam);
        self.draw_world(sim, &seen, alpha, mouse, over_hud);
        set_default_camera();
        self.draw_overlay(sim, &seen, mouse, over_hud);
        self.hud(sim, &seen, mouse, &mut cmds);
        cmds
    }

    fn ring(&self, c: V2, r: f32, y: f32, col: Color) {
        let n = (16.0 + r * 0.5).min(96.0) as usize;
        let mut last = vec3(c.x + r, y, c.y);
        for k in 1..=n {
            let a = k as f32 / n as f32 * std::f32::consts::TAU;
            let p = vec3(c.x + r * a.cos(), y, c.y + r * a.sin());
            draw_line_3d(last, p, col);
            last = p;
        }
    }

    fn draw_model(&mut self, d: DefId, pos: V2, heading: f32, aim: f32, built: f32, col: Color, alpha: f32) {
        let df = def(d);
        let origin = vec3(pos.x, df.alt, pos.y);
        let spin = get_time() as f32 * 1.6;
        let grow = built.clamp(0.12, 1.0);
        for k in 0..self.models[d as usize].len() {
            let part = self.models[d as usize][k];
            let yaw = match part.rot {
                0 => heading,
                1 => aim,
                _ => spin,
            };
            let mut c = part_color(part.col, col);
            if built < 1.0 {
                c = mix(c, Color::new(0.75, 0.8, 0.55, 1.0), 0.55);
            }
            c.a = alpha;
            let (mut pc, mut ph) = (part.c, part.h);
            pc.y *= grow;
            ph.y *= grow;
            self.batch.boxy(origin, yaw, pc, ph, c);
        }
    }

    fn draw_world(&mut self, sim: &Sim, seen: &[Seen], alpha: f32, mouse: Vec2, over_hud: bool) {
        let world = sim.map.world();
        let quad = |y: f32, tex: &Texture2D| Mesh {
            vertices: vec![
                Vertex::new2(vec3(0.0, y, 0.0), vec2(0.0, 0.0), WHITE),
                Vertex::new2(vec3(world, y, 0.0), vec2(1.0, 0.0), WHITE),
                Vertex::new2(vec3(world, y, world), vec2(1.0, 1.0), WHITE),
                Vertex::new2(vec3(0.0, y, world), vec2(0.0, 1.0), WHITE),
            ],
            indices: vec![0, 1, 2, 0, 2, 3],
            texture: Some(tex.clone()),
        };
        draw_mesh(&quad(0.0, &self.ground_tex));
        let view_r = self.cam_dist * 2.2 + 60.0;
        for (c, r, m) in &self.rocks {
            if c.dist(self.cam_target) < view_r + r {
                draw_mesh(m);
            }
        }
        // Blob shadows.
        let shadow = Color::new(0.0, 0.0, 0.0, 0.28);
        for s in seen {
            let u = &sim.s.units[s.idx];
            let d = u.d();
            if s.radar_only || !s.on_screen || s.rpx < 4.0 || d.is_structure() {
                continue;
            }
            let r = d.radius * 1.15;
            let o = d.alt * 0.25;
            let (x, z) = (s.pos.x + o, s.pos.y + o);
            self.batch.quad([vec3(x - r, 0.05, z - r), vec3(x + r, 0.05, z - r), vec3(x + r, 0.05, z + r), vec3(x - r, 0.05, z + r)], shadow);
        }
        self.batch.flush();
        if !self.reveal(sim) {
            draw_mesh(&quad(0.09, &self.fog_tex));
        }
        // Wrecks.
        let bit = 1u8 << self.my_team(sim);
        for w in &sim.s.wrecks {
            if w.pos.dist(self.cam_target) > view_r {
                continue;
            }
            let r = def(w.def).radius.min(3.0);
            let k = (w.mass / (def(w.def).mass * 0.6)).clamp(0.3, 1.0);
            self.batch.boxy(vec3(w.pos.x, 0.0, w.pos.y), w.heading, vec3(0.0, 0.25 * r * k, 0.0), vec3(r * 0.9, 0.25 * r * k, r * 0.5), Color::new(0.2, 0.17, 0.15, 1.0));
            self.batch.boxy(vec3(w.pos.x, 0.0, w.pos.y), w.heading + 0.5, vec3(-0.2 * r, 0.5 * r * k, 0.1 * r), vec3(r * 0.35, 0.15 * r, r * 0.3), Color::new(0.13, 0.12, 0.12, 1.0));
        }
        // Units.
        for s in seen {
            if s.radar_only || !s.on_screen {
                continue;
            }
            let u = &sim.s.units[s.idx];
            if s.rpx < 2.5 {
                continue;
            }
            let col = PLAYER_COLORS[sim.s.players[u.owner as usize].color as usize % 4];
            self.draw_model(u.def, s.pos, s.heading, s.aim, u.built, col, 1.0);
        }
        // Shells, bombs and missiles as small bright boxes.
        let _ = bit;
        let mut tracers: Vec<(Vec3, Vec3, Color)> = vec![];
        for p in &sim.s.projs {
            let back = (alpha - 1.0) * DT;
            let pos = p.pos + p.vel * back;
            if pos.dist(self.cam_target) > view_r {
                continue;
            }
            let t = (1.0 - (p.life - back) / p.total.max(0.01)).clamp(0.0, 1.0);
            let h = p.z0 + (p.z1 - p.z0) * t + p.arc * 4.0 * t * (1.0 - t);
            let at = vec3(pos.x, h, pos.y);
            let dir = p.vel.norm();
            if p.kind == PK::Direct as u8 {
                let len = (p.vel.len() * 0.035).clamp(0.8, 3.5);
                let c = if p.dmg > 40.0 { Color::new(1.0, 0.8, 0.4, 1.0) } else { Color::new(1.0, 0.95, 0.5, 0.9) };
                tracers.push((at, at - vec3(dir.x, 0.0, dir.y) * len, c));
                if p.dmg > 40.0 {
                    self.batch.boxy(at, dir.angle(), Vec3::ZERO, vec3(0.5, 0.12, 0.12), Color::new(1.0, 0.85, 0.5, 1.0));
                }
            } else {
                let size = (0.2 + p.dmg.sqrt() * 0.018).min(0.7);
                let c = if p.kind == PK::Homing as u8 { Color::new(0.95, 0.95, 0.95, 1.0) } else { Color::new(1.0, 0.6, 0.2, 1.0) };
                self.batch.boxy(at, dir.angle(), Vec3::ZERO, vec3(size * 2.0, size, size), c);
                tracers.push((at, at - vec3(dir.x, 0.0, dir.y) * 2.5, Color::new(0.8, 0.8, 0.8, 0.6)));
            }
        }
        self.batch.flush();

        // Translucent: queued build ghosts and the placement preview.
        let my_col = PLAYER_COLORS[sim.s.players[self.me as usize].color as usize % 4];
        let selected = self.own_selected(sim);
        for u in selected.iter().take(40) {
            for o in &u.orders {
                if let Order::Build { def: d, pos } = o {
                    if !sim.s.units.iter().any(|s| s.pos == *pos && s.d().foot > 0) {
                        self.draw_model(*d, *pos, 1.5708, 1.5708, 1.0, my_col, 0.35);
                    }
                }
            }
        }
        let ground = sim.map.clamp(self.s2g(mouse));
        if let Mode::Place(d) = self.mode {
            if !over_hud {
                let line = match self.place_start {
                    Some(a) => self.build_line(sim, d, a, ground),
                    None => vec![sim.snap(d, ground)],
                };
                for p in line {
                    let ok = sim.can_place(d, p);
                    let c = if ok { Color::new(0.3, 1.0, 0.4, 1.0) } else { Color::new(1.0, 0.25, 0.2, 1.0) };
                    self.draw_model(d, p, 1.5708, 1.5708, 1.0, c, 0.5);
                    let h = def(d).foot as f32 * CELL * 0.5;
                    self.batch.quad(
                        [vec3(p.x - h, 0.12, p.y - h), vec3(p.x + h, 0.12, p.y - h), vec3(p.x + h, 0.12, p.y + h), vec3(p.x - h, 0.12, p.y + h)],
                        Color::new(c.r, c.g, c.b, 0.25),
                    );
                }
            }
        }
        self.batch.flush();

        // Lines: tracers, selection rings, ranges, order queues, build beams.
        for (a, b, c) in tracers {
            draw_line_3d(a, b, c);
        }
        if let Mode::Place(d) = self.mode {
            let df = def(d);
            let p = sim.snap(d, ground);
            if df.armed() {
                self.ring(p, df.max_range(), 0.2, Color::new(1.0, 0.35, 0.3, 0.8));
            }
            if df.radar > 0.0 {
                self.ring(p, df.radar, 0.2, Color::new(0.4, 0.7, 1.0, 0.8));
            }
        }
        let pos_of: HashMap<u32, V2> = seen.iter().map(|s| (s.id, s.pos)).collect();
        for (k, u) in selected.iter().enumerate() {
            let Some(&pos) = pos_of.get(&u.id) else { continue };
            let d = u.d();
            if k < 200 {
                self.ring(pos, u.radius() + 0.6, 0.15, Color::new(0.4, 1.0, 0.5, 0.9));
            }
            if selected.len() <= 12 || k < 3 {
                if d.armed() {
                    self.ring(pos, d.max_range(), 0.2, Color::new(1.0, 0.35, 0.3, 0.55));
                    let minr = d.weapons[0].min_range;
                    if minr > 0.0 {
                        self.ring(pos, minr, 0.2, Color::new(1.0, 0.6, 0.2, 0.35));
                    }
                }
                if d.radar > 0.0 {
                    self.ring(pos, d.radar, 0.2, Color::new(0.4, 0.7, 1.0, 0.55));
                }
            }
            if k < 60 {
                let mut last = vec3(pos.x, 0.3, pos.y);
                if let Some(r) = u.rally {
                    draw_line_3d(last, vec3(r.x, 0.3, r.y), Color::new(0.3, 0.8, 1.0, 0.8));
                    self.ring(r, 1.0, 0.3, Color::new(0.3, 0.8, 1.0, 0.8));
                }
                for o in &u.orders {
                    let (p, c) = match o {
                        Order::Move(p) => (Some(*p), Color::new(0.3, 0.8, 1.0, 0.7)),
                        Order::AttackMove(p) => (Some(*p), Color::new(1.0, 0.4, 0.3, 0.7)),
                        Order::Patrol(p) => (Some(*p), Color::new(0.4, 0.9, 0.9, 0.7)),
                        Order::Build { pos, .. } => (Some(*pos), Color::new(1.0, 0.9, 0.3, 0.7)),
                        Order::Attack(id) => (pos_of.get(id).copied(), Color::new(1.0, 0.2, 0.2, 0.8)),
                        Order::Assist(id) => (pos_of.get(id).copied(), Color::new(0.9, 0.9, 0.3, 0.7)),
                        Order::Reclaim(id) => {
                            (sim.s.wrecks.iter().find(|w| w.id == *id).map(|w| w.pos), Color::new(0.6, 1.0, 0.4, 0.7))
                        }
                    };
                    if let Some(p) = p {
                        let np = vec3(p.x, 0.3, p.y);
                        draw_line_3d(last, np, c);
                        self.ring(p, 0.6, 0.3, c);
                        last = np;
                    }
                }
            }
        }
        // Build / reclaim beams.
        for s in seen {
            let u = &sim.s.units[s.idx];
            if u.work == 0 || s.radar_only || !u.d().is_builder() {
                continue;
            }
            let target = pos_of.get(&u.work).copied().or_else(|| sim.s.wrecks.iter().find(|w| w.id == u.work).map(|w| w.pos));
            if let Some(t) = target {
                let flick = 0.6 + 0.4 * ((get_time() * 20.0 + u.id as f64).sin() as f32);
                let a = vec3(s.pos.x, 1.6, s.pos.y);
                draw_line_3d(a, vec3(t.x, 1.0, t.y), Color::new(0.5, 1.0, 0.6, flick));
                draw_line_3d(a, vec3(t.x + 0.8, 0.4, t.y - 0.6), Color::new(0.8, 1.0, 0.5, flick * 0.7));
            }
        }
        // Formation preview while dragging a line.
        if self.pathing.is_some() && path_len(&self.path) >= 4.0 {
            let c = if self.pathing == Some(MouseButton::Left) || alt() { Color::new(1.0, 0.45, 0.3, 1.0) } else { Color::new(0.3, 0.9, 1.0, 1.0) };
            for w in self.path.windows(2) {
                draw_line_3d(vec3(w[0].x, 0.35, w[0].y), vec3(w[1].x, 0.35, w[1].y), c);
            }
            let movers: Vec<&Unit> =
                selected.iter().copied().filter(|u| !u.d().is_structure() && u.d().layer != Layer::Plane).collect();
            for p in self.formation(sim, &movers, &self.path) {
                self.ring(p, 0.9, 0.35, c);
            }
        }
    }

    fn icon(&self, x: f32, y: f32, d: &UnitDef, col: Color, selected: bool, scale: f32) {
        let big = if d.mass >= 500.0 || d.kind == Kind::Factory { 1.35 } else { 1.0 };
        let s = 5.0 * scale * big;
        let outline = if selected { WHITE } else { Color::new(0.0, 0.0, 0.0, 0.85) };
        match d.kind {
            Kind::Commander => {
                draw_circle(x, y, s * 1.5, outline);
                draw_circle(x, y, s * 1.5 - 1.5, col);
                draw_circle(x, y, s * 0.6, WHITE);
            }
            _ if d.is_structure() => {
                draw_rectangle(x - s - 1.0, y - s - 1.0, s * 2.0 + 2.0, s * 2.0 + 2.0, outline);
                draw_rectangle(x - s, y - s, s * 2.0, s * 2.0, col);
                if d.kind == Kind::Factory {
                    draw_rectangle(x - s * 0.45, y - s * 0.45, s * 0.9, s * 0.9, Color::new(0.0, 0.0, 0.0, 0.6));
                } else if d.armed() {
                    draw_line(x - s, y - s, x + s, y + s, 1.5, BLACK);
                    draw_line(x - s, y + s, x + s, y - s, 1.5, BLACK);
                } else if d.prod_m > 0.0 {
                    draw_circle(x, y, s * 0.5, Color::new(0.85, 0.9, 0.95, 1.0));
                } else if d.prod_e > 0.0 {
                    draw_circle(x, y, s * 0.5, Color::new(1.0, 0.9, 0.2, 1.0));
                }
            }
            _ if d.is_air() => {
                draw_poly(x, y, 3, s * 1.5 + 1.5, -90.0, outline);
                draw_poly(x, y, 3, s * 1.5, -90.0, col);
            }
            _ => {
                draw_poly(x, y, 4, s * 1.3 + 1.5, 0.0, outline);
                draw_poly(x, y, 4, s * 1.3, 0.0, col);
                if d.is_builder() {
                    draw_circle(x, y, s * 0.4, WHITE);
                } else if d.max_range() >= 90.0 {
                    draw_rectangle(x - s * 0.6, y - 1.0, s * 1.2, 2.0, BLACK);
                } else if !d.armed() {
                    draw_circle(x, y, s * 0.35, BLACK);
                }
            }
        }
    }

    fn draw_overlay(&mut self, sim: &Sim, seen: &[Seen], mouse: Vec2, over_hud: bool) {
        let now = get_time();
        // Explosions.
        self.fx.retain(|f| now - f.3 < 0.35 + f.1 as f64 * 0.06);
        for &(pos, r, h, t0) in &self.fx {
            let life = 0.35 + r * 0.06;
            let t = ((now - t0) as f32 / life).clamp(0.0, 1.0);
            if let Some((sp, depth)) = self.w2s(vec3(pos.x, h, pos.y)) {
                let px = self.px_per_unit(depth);
                let rr = (r * (0.35 + 0.75 * t) * px).max(2.0);
                draw_circle(sp.x, sp.y, rr, Color::new(0.25, 0.2, 0.18, 0.45 * (1.0 - t)));
                draw_circle(sp.x, sp.y, rr * 0.75, Color::new(1.0, 0.55, 0.1, 0.8 * (1.0 - t)));
                draw_circle(sp.x, sp.y, rr * 0.4 * (1.0 - t), Color::new(1.0, 0.95, 0.7, 0.9 * (1.0 - t)));
            }
        }
        // Mass deposits stay readable when zoomed far out.
        if self.cam_dist > 160.0 {
            for dep in &sim.map.deposits {
                if let Some((sp, _)) = self.w2s(vec3(dep.x, 0.0, dep.y)) {
                    draw_circle(sp.x, sp.y, 3.0, Color::new(0.0, 0.0, 0.0, 0.6));
                    draw_circle(sp.x, sp.y, 2.0, Color::new(0.8, 0.85, 0.9, 0.9));
                }
            }
        }
        let hover = self.hover(seen, mouse, over_hud);
        for (k, s) in seen.iter().enumerate() {
            if !s.on_screen {
                continue;
            }
            let u = &sim.s.units[s.idx];
            let d = u.d();
            let col = PLAYER_COLORS[sim.s.players[u.owner as usize].color as usize % 4];
            let selected = s.mine && self.sel.contains(&s.id);
            if s.radar_only {
                // Radar blip: we know something is there, not what.
                let c = Color::new(0.75, 0.75, 0.75, 0.9);
                draw_poly_lines(s.sp.x, s.sp.y, 4, 5.0, 0.0, 1.5, c);
                draw_circle(s.sp.x, s.sp.y, 1.5, col);
                continue;
            }
            if s.rpx < 5.5 {
                self.icon(s.sp.x, s.sp.y, d, col, selected, 1.0);
            }
            if hover == Some(k) {
                draw_circle_lines(s.sp.x, s.sp.y, s.rpx.max(9.0) + 2.0, 1.0, Color::new(1.0, 1.0, 1.0, 0.6));
            }
            // Bars.
            let w = s.rpx.clamp(12.0, 60.0) * 1.6;
            let (bx, mut by) = (s.sp.x - w * 0.5, s.sp.y - s.rpx.max(7.0) - 8.0);
            let hp = (u.hp / d.hp).clamp(0.0, 1.0);
            if (hp < 0.999 && u.done()) || selected {
                draw_rectangle(bx - 1.0, by - 1.0, w + 2.0, 5.0, Color::new(0.0, 0.0, 0.0, 0.7));
                let c = if hp > 0.6 { Color::new(0.2, 0.9, 0.3, 1.0) } else if hp > 0.3 { Color::new(0.95, 0.8, 0.2, 1.0) } else { Color::new(0.95, 0.2, 0.2, 1.0) };
                draw_rectangle(bx, by, w * hp, 3.0, c);
                by -= 6.0;
            }
            let prog = if !u.done() {
                Some(u.built)
            } else if u.upg > 0.0 {
                Some(u.upg)
            } else if u.building != 0 {
                sim.unit(u.building).map(|b| b.built)
            } else {
                None
            };
            if let Some(pr) = prog {
                draw_rectangle(bx - 1.0, by - 1.0, w + 2.0, 5.0, Color::new(0.0, 0.0, 0.0, 0.7));
                draw_rectangle(bx, by, w * pr.clamp(0.0, 1.0), 3.0, Color::new(0.4, 0.8, 1.0, 1.0));
            }
        }
        // Selection box.
        if let Some(b) = self.box_start {
            if b.distance(mouse) >= 6.0 {
                let (x, y, w, h) = (b.x.min(mouse.x), b.y.min(mouse.y), (b.x - mouse.x).abs(), (b.y - mouse.y).abs());
                draw_rectangle(x, y, w, h, Color::new(0.3, 1.0, 0.4, 0.12));
                draw_rectangle_lines(x, y, w, h, 1.5, Color::new(0.4, 1.0, 0.5, 0.9));
            }
        }
        let label = match self.mode {
            Mode::AttackMove => "ATTACK-MOVE: left-click or drag a line  (right-click cancels)".to_string(),
            Mode::Patrol => "PATROL: left-click a point  (right-click cancels)".to_string(),
            Mode::Place(d) => format!("BUILD {}: click, or drag for a row. Shift queues more.", def(d).name),
            Mode::None => String::new(),
        };
        if !label.is_empty() {
            text_c(&label, screen_width() * 0.5, TOP + 26.0, 20.0, Color::new(1.0, 0.9, 0.5, 1.0));
        }
    }

    fn button(&self, r: Rect, mouse: Vec2, active: bool) -> bool {
        let hover = r.contains(mouse);
        let bg = if active {
            Color::new(0.25, 0.4, 0.3, 0.95)
        } else if hover {
            Color::new(0.26, 0.3, 0.36, 0.95)
        } else {
            Color::new(0.14, 0.16, 0.2, 0.95)
        };
        draw_rectangle(r.x, r.y, r.w, r.h, bg);
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.5, if hover { WHITE } else { Color::new(0.4, 0.45, 0.5, 1.0) });
        hover
    }

    fn hud(&mut self, sim: &Sim, seen: &[Seen], mouse: Vec2, cmds: &mut Vec<Command>) {
        let (sw, sh) = (screen_width(), screen_height());
        let p = &sim.s.players[self.me as usize];
        let click_l = is_mouse_button_pressed(MouseButton::Left);
        let click_r = is_mouse_button_pressed(MouseButton::Right);

        // --- economy bar
        draw_rectangle(0.0, 0.0, sw, TOP, Color::new(0.07, 0.08, 0.1, 0.92));
        let res = |x: f32, name: &str, cur: f32, max: f32, inc: f32, exp: f32, col: Color| {
            draw_text(name, x, 20.0, 18.0, col);
            let bx = x + 62.0;
            draw_rectangle(bx, 7.0, 150.0, 16.0, Color::new(0.0, 0.0, 0.0, 0.8));
            draw_rectangle(bx, 7.0, 150.0 * (cur / max.max(1.0)).clamp(0.0, 1.0), 16.0, shade(col, 0.75));
            text_c(&format!("{:.0} / {:.0}", cur, max), bx + 75.0, 20.0, 16.0, WHITE);
            draw_text(&format!("+{:.1}", inc), bx + 158.0, 20.0, 18.0, Color::new(0.4, 1.0, 0.5, 1.0));
            let ec = if exp > inc + 0.01 { Color::new(1.0, 0.4, 0.35, 1.0) } else { Color::new(0.85, 0.85, 0.85, 1.0) };
            draw_text(&format!("-{:.1}", exp), bx + 222.0, 20.0, 18.0, ec);
        };
        res(12.0, "MASS", p.mass, p.max_mass, p.income_m, p.spend_m, Color::new(0.75, 0.85, 0.95, 1.0));
        res(360.0, "ENERGY", p.energy, p.max_energy, p.income_e, p.spend_e, Color::new(1.0, 0.85, 0.25, 1.0));
        if p.power_out {
            draw_text("POWER STALL", 690.0, 20.0, 18.0, Color::new(1.0, 0.3, 0.3, 1.0));
        }
        let secs = sim.s.tick / 10;
        let info = format!("{:02}:{:02}   {} fps", secs / 60, secs % 60, get_fps());
        let m = measure_text(&info, None, 18, 1.0);
        draw_text(&info, sw - m.width - 10.0, 20.0, 18.0, Color::new(0.8, 0.8, 0.8, 1.0));
        // Player list.
        for (k, pl) in sim.s.players.iter().enumerate() {
            let y = TOP + 18.0 + k as f32 * 18.0;
            let c = PLAYER_COLORS[pl.color as usize % 4];
            let t = format!("{}{} (team {})", pl.name, if pl.alive { "" } else { " - defeated" }, pl.team + 1);
            let m = measure_text(&t, None, 16, 1.0);
            draw_text(&t, sw - m.width - 10.0, y, 16.0, if pl.alive { c } else { Color::new(0.5, 0.5, 0.5, 1.0) });
        }

        // --- minimap
        let mm = self.mm_rect();
        let world = sim.map.world();
        draw_rectangle(mm.x - 2.0, mm.y - 2.0, mm.w + 4.0, mm.h + 4.0, Color::new(0.0, 0.0, 0.0, 0.9));
        let params = DrawTextureParams { dest_size: Some(vec2(mm.w, mm.h)), ..Default::default() };
        draw_texture_ex(&self.ground_tex, mm.x, mm.y, WHITE, params.clone());
        if !self.reveal(sim) {
            draw_texture_ex(&self.fog_tex, mm.x, mm.y, WHITE, params);
        }
        let to_mm = |p: V2| vec2(mm.x + p.x / world * mm.w, mm.y + p.y / world * mm.h);
        for s in seen {
            let u = &sim.s.units[s.idx];
            let q = to_mm(s.pos);
            let c = if s.radar_only { Color::new(0.8, 0.8, 0.8, 1.0) } else { PLAYER_COLORS[sim.s.players[u.owner as usize].color as usize % 4] };
            let sz = if u.def == COMMANDER { 5.0 } else if u.d().is_structure() { 3.0 } else { 2.0 };
            draw_rectangle(q.x - sz * 0.5, q.y - sz * 0.5, sz, sz, c);
        }
        let corners = [vec2(0.0, TOP), vec2(sw, TOP), vec2(sw, sh), vec2(0.0, sh)].map(|c| to_mm(self.s2g(c)));
        for k in 0..4 {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            let cl = |v: Vec2| vec2(v.x.clamp(mm.x, mm.x + mm.w), v.y.clamp(mm.y, mm.y + mm.h));
            let (a, b) = (cl(a), cl(b));
            draw_line(a.x, a.y, b.x, b.y, 1.0, WHITE);
        }
        if mm.contains(mouse) && !self.menu_open {
            let wp = v2((mouse.x - mm.x) / mm.w * world, (mouse.y - mm.y) / mm.h * world);
            if is_mouse_button_down(MouseButton::Left) && self.box_start.is_none() && self.pathing.is_none() {
                self.cam_target = wp;
            }
            if click_r && !self.sel.is_empty() {
                let kind = if alt() { MoveKind::AttackMove } else { MoveKind::Move };
                self.order_move(sim, &[wp], kind, shift(), cmds);
            }
        }

        // --- selection panel
        let units = self.own_selected(sim);
        if !units.is_empty() {
            let pr = self.panel_rect();
            draw_rectangle(pr.x, pr.y, pr.w, pr.h, Color::new(0.07, 0.08, 0.1, 0.9));
            draw_rectangle_lines(pr.x, pr.y, pr.w, pr.h, 1.5, Color::new(0.3, 0.35, 0.4, 1.0));
            let lead = units.iter().find(|u| !u.d().builds.is_empty()).copied().unwrap_or(units[0]);
            let ld = lead.d();
            let col = PLAYER_COLORS[p.color as usize % 4];
            // Left: description of the selection.
            let ix = pr.x + 10.0;
            if units.len() == 1 {
                self.icon(ix + 10.0, pr.y + 20.0, ld, col, false, 1.4);
                draw_text(ld.name, ix + 28.0, pr.y + 24.0, 20.0, WHITE);
                draw_text(&format!("HP {:.0} / {:.0}", lead.hp, ld.hp), ix, pr.y + 46.0, 16.0, Color::new(0.6, 1.0, 0.6, 1.0));
                let mut y = pr.y + 64.0;
                for w in ld.weapons {
                    let t = format!("{}: {:.0} dmg  r{:.0}  {:.1}s", w.name, w.dmg, w.range, w.reload);
                    draw_text(&t, ix, y, 14.0, Color::new(0.9, 0.8, 0.6, 1.0));
                    y += 15.0;
                }
                if ld.prod_m > 0.0 || ld.prod_e > 0.0 || ld.upkeep_e > 0.0 {
                    let t = format!("+{:.0} mass  +{:.0} energy  -{:.0} upkeep", ld.prod_m, ld.prod_e, ld.upkeep_e);
                    draw_text(&t, ix, y, 14.0, Color::new(0.7, 0.9, 1.0, 1.0));
                    y += 15.0;
                }
                if lead.upg > 0.0 {
                    draw_text(&format!("Upgrading: {:.0}%", lead.upg * 100.0), ix, y, 14.0, Color::new(0.5, 0.9, 1.0, 1.0));
                }
            } else {
                let mut counts: Vec<(DefId, usize)> = vec![];
                for u in &units {
                    match counts.iter_mut().find(|c| c.0 == u.def) {
                        Some(c) => c.1 += 1,
                        None => counts.push((u.def, 1)),
                    }
                }
                draw_text(&format!("{} units", units.len()), ix, pr.y + 22.0, 20.0, WHITE);
                for (k, (d, n)) in counts.iter().take(5).enumerate() {
                    draw_text(&format!("{} x {}", n, def(*d).name), ix, pr.y + 42.0 + k as f32 * 15.0, 15.0, Color::new(0.8, 0.85, 0.9, 1.0));
                }
            }
            // Right: build menu / factory queue / actions.
            let bx0 = pr.x + 310.0;
            let (bw, bh) = (64.0, 54.0);
            let per_row = (((pr.w - 316.0) / (bw + 4.0)).floor() as usize).max(1);
            let mut tip: Option<DefId> = None;
            let is_factory = ld.kind == Kind::Factory;
            for (k, d) in ld.builds.iter().enumerate() {
                let r = Rect::new(bx0 + (k % per_row) as f32 * (bw + 4.0), pr.y + 6.0 + (k / per_row) as f32 * (bh + 4.0), bw, bh);
                let bd = def(*d);
                let active = self.mode == Mode::Place(*d);
                let hover = self.button(r, mouse, active);
                self.icon(r.x + 12.0, r.y + 13.0, bd, col, false, 1.0);
                draw_text(bd.tag, r.x + 26.0, r.y + 19.0, 18.0, WHITE);
                let short: String = bd.name.chars().take(10).collect();
                draw_text(&short, r.x + 3.0, r.y + 35.0, 12.0, Color::new(0.8, 0.85, 0.9, 1.0));
                draw_text(&format!("{:.0}", bd.mass), r.x + 3.0, r.y + 49.0, 12.0, Color::new(0.75, 0.85, 0.95, 1.0));
                if is_factory {
                    let n = lead.queue.iter().filter(|q| **q == *d).count();
                    if n > 0 {
                        let t = format!("{}", n);
                        let m = measure_text(&t, None, 18, 1.0);
                        draw_text(&t, r.x + r.w - m.width - 4.0, r.y + 50.0, 18.0, Color::new(1.0, 0.9, 0.3, 1.0));
                    }
                }
                if hover {
                    tip = Some(*d);
                    if is_factory {
                        let n: i16 = if shift() { 5 } else { 1 };
                        if click_l || click_r {
                            for f in units.iter().filter(|u| u.def == lead.def) {
                                cmds.push(Command::Produce { factory: f.id, def: *d, count: if click_l { n } else { -n } });
                            }
                        }
                    } else if click_l {
                        self.mode = Mode::Place(*d);
                    }
                }
            }
            // Action buttons along the bottom of the panel's left part.
            let mut ax = pr.x + 10.0;
            let ay = pr.y + pr.h - 26.0;
            let mut action = |label: &str, active: bool, this: &Self| -> bool {
                let m = measure_text(label, None, 15, 1.0);
                let r = Rect::new(ax, ay, m.width + 14.0, 20.0);
                ax += r.w + 5.0;
                let hover = this.button(r, mouse, active);
                draw_text(label, r.x + 7.0, r.y + 15.0, 15.0, WHITE);
                hover && click_l
            };
            if units.iter().any(|u| !u.d().is_structure()) {
                if action("Attack-move [A]", self.mode == Mode::AttackMove, self) {
                    self.mode = Mode::AttackMove;
                }
                if action("Patrol [P]", self.mode == Mode::Patrol, self) {
                    self.mode = Mode::Patrol;
                }
            }
            if action("Stop [S]", false, self) {
                cmds.push(Command::Stop { ids: self.sel.clone() });
            }
            if is_factory && action(if lead.repeat { "Repeat: ON [R]" } else { "Repeat: off [R]" }, lead.repeat, self) {
                cmds.push(Command::Repeat { ids: units.iter().filter(|u| u.d().kind == Kind::Factory).map(|u| u.id).collect(), on: !lead.repeat });
            }
            if units.iter().any(|u| u.d().upgrade.is_some() && u.upg == 0.0) && action("Upgrade [U]", false, self) {
                cmds.push(Command::Upgrade { ids: self.sel.clone() });
            }
            // Tooltip.
            if let Some(d) = tip {
                let bd = def(d);
                let rate = ld.build_rate.max(1.0);
                let mut lines = vec![
                    bd.name.to_string(),
                    format!("Mass {:.0}   Energy {:.0}   Time {:.0}s   HP {:.0}", bd.mass, bd.energy, bd.build_time / rate, bd.hp),
                    bd.desc.to_string(),
                ];
                for w in bd.weapons {
                    let mut t = format!("{}: {:.0} dmg / {:.1}s, range {:.0}", w.name, w.dmg, w.reload, w.range);
                    if w.min_range > 0.0 {
                        t += &format!(" (min {:.0})", w.min_range);
                    }
                    if w.aoe > 0.0 {
                        t += &format!(", blast {:.0}", w.aoe);
                    }
                    if w.air && !w.land {
                        t += ", anti-air";
                    }
                    lines.push(t);
                }
                if bd.speed > 0.0 {
                    lines.push(format!("Speed {:.1}   Sight {:.0}{}", bd.speed, bd.vision, if bd.radar > 0.0 { format!("   Radar {:.0}", bd.radar) } else { String::new() }));
                }
                if is_factory {
                    lines.push("Left-click: queue (+Shift x5).  Right-click: remove.".into());
                }
                let w = lines.iter().map(|l| measure_text(l, None, 16, 1.0).width).fold(0.0, f32::max) + 20.0;
                let h = lines.len() as f32 * 18.0 + 12.0;
                let (tx, ty) = ((mouse.x - 20.0).min(sw - w - 8.0).max(8.0), pr.y - h - 6.0);
                draw_rectangle(tx, ty, w, h, Color::new(0.05, 0.06, 0.08, 0.95));
                draw_rectangle_lines(tx, ty, w, h, 1.0, Color::new(0.5, 0.55, 0.6, 1.0));
                for (k, l) in lines.iter().enumerate() {
                    draw_text(l, tx + 10.0, ty + 20.0 + k as f32 * 18.0, 16.0, if k == 0 { Color::new(1.0, 0.9, 0.5, 1.0) } else { WHITE });
                }
            }
        } else {
            draw_text(
                "LMB select / drag box    RMB order    RMB-drag: spread units along a line    Wheel zoom    Arrows / MMB pan    Esc menu",
                MM + 22.0,
                sh - 12.0,
                15.0,
                Color::new(0.75, 0.75, 0.75, 0.9),
            );
        }

        // --- banners
        let team = self.my_team(sim);
        if let Some(w) = sim.s.winner {
            let (t, c) = if w == team { ("VICTORY", Color::new(0.4, 1.0, 0.5, 1.0)) } else { ("DEFEAT", Color::new(1.0, 0.35, 0.3, 1.0)) };
            text_c(t, sw * 0.5, sh * 0.3, 80.0, c);
            text_c("Press Esc for the menu", sw * 0.5, sh * 0.3 + 34.0, 22.0, WHITE);
        } else if !p.alive {
            text_c("Your commander was destroyed - spectating", sw * 0.5, TOP + 50.0, 24.0, Color::new(1.0, 0.5, 0.4, 1.0));
        }

        // --- pause menu
        if self.menu_open {
            draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.5));
            let (w, h) = (260.0, 190.0);
            let (x, y) = (sw * 0.5 - w * 0.5, sh * 0.5 - h * 0.5);
            draw_rectangle(x, y, w, h, Color::new(0.08, 0.09, 0.12, 0.98));
            draw_rectangle_lines(x, y, w, h, 2.0, Color::new(0.5, 0.55, 0.6, 1.0));
            text_c("MENU", sw * 0.5, y + 30.0, 26.0, WHITE);
            let labels = ["Resume", "Resign", "Quit to main menu"];
            for (k, l) in labels.iter().enumerate() {
                let r = Rect::new(x + 30.0, y + 48.0 + k as f32 * 44.0, w - 60.0, 34.0);
                let hover = self.button(r, mouse, false);
                text_c(l, sw * 0.5, r.y + 23.0, 20.0, WHITE);
                if hover && click_l {
                    match k {
                        0 => self.menu_open = false,
                        1 => {
                            cmds.push(Command::Resign);
                            self.menu_open = false;
                        }
                        _ => self.quit = true,
                    }
                }
            }
        }
    }
}
