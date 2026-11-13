//! Long Range - a standalone RTS based on the "Long Range Tank" mod.
//! Menu, lobby and session plumbing live here.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod ai;
mod defs;
mod game;
mod map;
mod net;
mod render;
mod sim;

use macroquad::prelude::*;
use net::*;
use sim::*;

const CORNERS: [&str; 4] = ["North-West", "North-East", "South-East", "South-West"];
const MAP_SIZES: [(u32, &str); 3] = [(128, "Small (512)"), (192, "Medium (768)"), (256, "Large (1024)")];

struct Slot {
    kind: u8,
    name: String,
    team: u8,
    conn: u32,
}

struct HostLobby {
    host: Host,
    slots: Vec<Slot>,
    map_cells: u32,
    dirty: bool,
    lan_ip: String,
}

struct ClientLobby {
    client: Client,
    info: Option<LobbyInfo>,
    pending: Option<(map::Map, u8)>,
}

enum Role {
    Host { host: Host, ais: Vec<ai::Ai>, remote: Vec<(u32, u8)> },
    Client { client: Client },
}

struct Session {
    sim: Sim,
    view: game::GameView,
    role: Role,
    acc: f32,
}

enum Screen {
    Menu,
    HostLobby(HostLobby),
    ClientLobby(ClientLobby),
    Game(Box<Session>),
}

fn local_ip() -> String {
    std::net::UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("192.168.255.255:9")?;
            s.local_addr()
        })
        .map(|a| a.ip().to_string())
        .unwrap_or_else(|_| "unknown".into())
}

fn button(r: Rect, label: &str, enabled: bool) -> bool {
    let mouse: Vec2 = mouse_position().into();
    let hover = r.contains(mouse) && enabled;
    let bg = if hover { Color::new(0.26, 0.32, 0.4, 1.0) } else { Color::new(0.14, 0.17, 0.22, 1.0) };
    draw_rectangle(r.x, r.y, r.w, r.h, bg);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, if hover { WHITE } else { Color::new(0.4, 0.45, 0.5, 1.0) });
    let size = (r.h * 0.55).min(24.0);
    let m = measure_text(label, None, size as u16, 1.0);
    let c = if enabled { WHITE } else { Color::new(0.5, 0.5, 0.5, 1.0) };
    draw_text(label, r.x + (r.w - m.width) * 0.5, r.y + r.h * 0.5 + size * 0.3, size, c);
    hover && is_mouse_button_pressed(MouseButton::Left)
}

/// Returns true when clicked (to take focus).
fn text_field(r: Rect, text: &mut String, focused: bool, typed: &[char], max: usize) -> bool {
    let mouse: Vec2 = mouse_position().into();
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.05, 0.06, 0.08, 1.0));
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, if focused { Color::new(1.0, 0.85, 0.4, 1.0) } else { Color::new(0.4, 0.45, 0.5, 1.0) });
    if focused {
        for c in typed {
            if !c.is_control() && text.chars().count() < max {
                text.push(*c);
            }
        }
        if is_key_pressed(KeyCode::Backspace) {
            text.pop();
        }
    }
    let cursor = if focused && (get_time() * 2.0) as i64 % 2 == 0 { "|" } else { "" };
    draw_text(&format!("{text}{cursor}"), r.x + 8.0, r.y + r.h * 0.5 + 7.0, 22.0, WHITE);
    r.contains(mouse) && is_mouse_button_pressed(MouseButton::Left)
}

fn lobby_info(l: &HostLobby, you: i8) -> LobbyInfo {
    LobbyInfo {
        slots: l.slots.iter().map(|s| SlotInfo { kind: s.kind, name: s.name.clone(), team: s.team }).collect(),
        map_cells: l.map_cells,
        you,
    }
}

fn draw_slots(slots: &[SlotInfo], you: i8, x: f32, y: f32) -> Option<(usize, u8)> {
    // Returns (slot, column) of a click: column 0 = occupant, 1 = team.
    let mut clicked = None;
    for (k, s) in slots.iter().enumerate() {
        let ry = y + k as f32 * 46.0;
        let c = render::PLAYER_COLORS[k];
        draw_rectangle(x, ry, 14.0, 38.0, c);
        draw_text(CORNERS[k], x + 24.0, ry + 25.0, 20.0, Color::new(0.8, 0.8, 0.8, 1.0));
        let label = match s.kind {
            0 => "Open".to_string(),
            1 => "Closed".to_string(),
            2 => format!("{}{}", s.name, if k as i8 == you { " (you)" } else { "" }),
            _ => "AI".to_string(),
        };
        if button(Rect::new(x + 140.0, ry, 240.0, 38.0), &label, true) {
            clicked = Some((k, 0));
        }
        if button(Rect::new(x + 390.0, ry, 110.0, 38.0), &format!("Team {}", s.team + 1), true) {
            clicked = Some((k, 1));
        }
    }
    clicked
}

fn start_game(l: HostLobby) -> Session {
    let seed = (macroquad::miniquad::date::now() * 1000.0) as u64 | 1;
    let map = map::Map::generate(l.map_cells as usize, seed);
    let mut setup = vec![];
    let mut remote = vec![];
    let mut ais = vec![];
    for (k, s) in l.slots.iter().enumerate() {
        if s.kind < 2 {
            continue;
        }
        let idx = setup.len() as u8;
        if s.kind == 3 {
            ais.push(ai::Ai::new(idx, seed));
        } else if s.conn != 0 {
            remote.push((s.conn, idx));
        }
        let name = if s.kind == 3 { format!("AI {}", CORNERS[k]) } else { s.name.clone() };
        setup.push(PlayerSetup { name, team: s.team, is_ai: s.kind == 3, slot: k });
    }
    let sim = Sim::new(map, &setup);
    for (conn, idx) in &remote {
        l.host.send_msg(*conn, &S2C::Start { map: sim.map.clone(), you: *idx });
    }
    *l.host.info.lock().unwrap() = "(game in progress)".into();
    let view = game::GameView::new(&sim, 0);
    Session { sim, view, role: Role::Host { host: l.host, ais, remote }, acc: 0.0 }
}

impl Session {
    /// Runs one frame. Returns a message when the session is over.
    fn frame(&mut self) -> Option<String> {
        match &mut self.role {
            Role::Host { host, ais, remote } => {
                while let Ok(ev) = host.events.try_recv() {
                    match ev {
                        HostEvent::Msg(id, C2S::Cmd(c)) => {
                            if let Some((_, p)) = remote.iter().find(|r| r.0 == id) {
                                self.sim.apply(*p, c);
                            }
                        }
                        HostEvent::Left(id) => {
                            if let Some(k) = remote.iter().position(|r| r.0 == id) {
                                self.sim.apply(remote[k].1, Command::Resign);
                                remote.remove(k);
                            }
                        }
                        HostEvent::Joined(id) => {
                            host.send_msg(id, &S2C::Kicked("The game has already started.".into()));
                        }
                        _ => {}
                    }
                }
                // LR_SPEED=4 fast-forwards the simulation (handy for testing and AI watching).
                let speed = std::env::var("LR_SPEED").ok().and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0).clamp(0.1, 20.0);
                self.acc += get_frame_time().min(0.25) * speed;
                let mut n = 0;
                let cap = (3.0 * speed).ceil() as i32;
                while self.acc >= DT && n < cap {
                    self.acc -= DT;
                    n += 1;
                    for a in ais.iter_mut() {
                        if (self.sim.s.tick + a.player as u32 * 3) % 10 == 0 {
                            for c in a.think(&self.sim) {
                                self.sim.apply(a.player, c);
                            }
                        }
                    }
                    self.sim.tick();
                    if !remote.is_empty() {
                        if let Ok(data) = bincode::serialize(&self.sim.s) {
                            let f = frame(TAG_SNAP, data);
                            for (conn, _) in remote.iter() {
                                host.send(*conn, &f);
                            }
                        }
                    }
                    self.view.sync(&self.sim);
                }
                if n == cap {
                    self.acc = 0.0;
                }
                let me = self.view.me;
                for c in self.view.frame(&self.sim) {
                    self.sim.apply(me, c);
                }
            }
            Role::Client { client } => {
                loop {
                    match client.frames.try_recv() {
                        Ok(Some((TAG_SNAP, data))) => {
                            if let Ok(s) = bincode::deserialize::<State>(&data) {
                                self.sim.set_state(s);
                                self.view.sync(&self.sim);
                            }
                        }
                        Ok(Some(_)) => {}
                        Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            return Some("Connection to the host was lost.".into());
                        }
                        Err(_) => break,
                    }
                }
                for c in self.view.frame(&self.sim) {
                    client.send(&C2S::Cmd(c));
                }
            }
        }
        if self.view.quit {
            return Some(String::new());
        }
        None
    }
}

async fn amain() {
    let mut screen = Screen::Menu;
    let mut name = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "Player".into());
    name.truncate(16);
    let mut ip = String::new();
    let mut focus = 0;
    let mut message = String::new();
    let mut discovery = Discovery::new();
    let mut fullscreen = false;
    prevent_quit();
    // `--skirmish` jumps straight into a game against three AIs.
    if std::env::args().any(|a| a == "--skirmish") {
        if let Ok(host) = Host::start(&name) {
            let mut slots: Vec<Slot> = (0..4).map(|k| Slot { kind: 3, name: String::new(), team: k, conn: 0 }).collect();
            slots[0] = Slot { kind: 2, name: name.clone(), team: 0, conn: 0 };
            screen = Screen::Game(Box::new(start_game(HostLobby { host, slots, map_cells: 192, dirty: false, lan_ip: String::new() })));
        }
    }

    loop {
        if is_quit_requested() {
            break;
        }
        if is_key_pressed(KeyCode::F11) {
            fullscreen = !fullscreen;
            set_fullscreen(fullscreen);
        }
        let mut typed = vec![];
        while let Some(c) = get_char_pressed() {
            typed.push(c);
        }
        let (sw, sh) = (screen_width(), screen_height());
        let mut next: Option<Screen> = None;

        match &mut screen {
            Screen::Menu => {
                clear_background(Color::new(0.06, 0.07, 0.09, 1.0));
                discovery.poll(get_frame_time());
                let cx = sw * 0.5;
                let t = "LONG RANGE";
                let m = measure_text(t, None, 72, 1.0);
                draw_text(t, cx - m.width * 0.5, 100.0, 72.0, Color::new(1.0, 0.85, 0.4, 1.0));
                let t = "flow economy  -  long guns  -  scouts win wars";
                let m = measure_text(t, None, 22, 1.0);
                draw_text(t, cx - m.width * 0.5, 130.0, 22.0, Color::new(0.7, 0.75, 0.8, 1.0));

                let x = cx - 220.0;
                draw_text("Name", x, 195.0, 22.0, WHITE);
                if text_field(Rect::new(x + 80.0, 170.0, 360.0, 38.0), &mut name, focus == 0, &typed, 16) {
                    focus = 0;
                }
                if button(Rect::new(x, 225.0, 440.0, 46.0), "Host game / Skirmish vs AI", true) {
                    match Host::start(&name) {
                        Ok(host) => {
                            let mut slots: Vec<Slot> = (0..4).map(|k| Slot { kind: 0, name: String::new(), team: k, conn: 0 }).collect();
                            slots[0] = Slot { kind: 2, name: name.clone(), team: 0, conn: 0 };
                            slots[2].kind = 3;
                            next = Some(Screen::HostLobby(HostLobby { host, slots, map_cells: 192, dirty: true, lan_ip: local_ip() }));
                            message.clear();
                        }
                        Err(e) => message = e,
                    }
                }
                draw_text("LAN games", x, 310.0, 22.0, WHITE);
                let mut join: Option<String> = None;
                if discovery.found.is_empty() {
                    draw_text("searching the local network...", x, 338.0, 18.0, Color::new(0.6, 0.6, 0.6, 1.0));
                }
                for (k, (addr, host_name)) in discovery.found.iter().enumerate().take(4) {
                    if button(Rect::new(x, 320.0 + k as f32 * 44.0, 440.0, 38.0), &format!("Join {host_name}  ({addr})"), true) {
                        join = Some(addr.to_string());
                    }
                }
                let y = 510.0;
                draw_text("Or by IP", x, y + 25.0, 22.0, WHITE);
                if text_field(Rect::new(x + 100.0, y, 220.0, 38.0), &mut ip, focus == 1, &typed, 40) {
                    focus = 1;
                }
                if button(Rect::new(x + 330.0, y, 110.0, 38.0), "Join", !ip.is_empty()) || (focus == 1 && is_key_pressed(KeyCode::Enter) && !ip.is_empty()) {
                    join = Some(ip.trim().to_string());
                }
                if let Some(addr) = join {
                    match Client::connect(&addr, &name) {
                        Ok(client) => {
                            next = Some(Screen::ClientLobby(ClientLobby { client, info: None, pending: None }));
                            message.clear();
                        }
                        Err(e) => message = e,
                    }
                }
                if button(Rect::new(x, y + 60.0, 440.0, 40.0), "Quit", true) {
                    break;
                }
                if !message.is_empty() {
                    let m = measure_text(&message, None, 20, 1.0);
                    draw_text(&message, cx - m.width * 0.5, y + 130.0, 20.0, Color::new(1.0, 0.45, 0.4, 1.0));
                }
                draw_text("F11 fullscreen", 10.0, sh - 10.0, 16.0, Color::new(0.5, 0.5, 0.5, 1.0));
            }

            Screen::HostLobby(l) => {
                clear_background(Color::new(0.06, 0.07, 0.09, 1.0));
                while let Ok(ev) = l.host.events.try_recv() {
                    match ev {
                        HostEvent::Msg(id, C2S::Hello { name }) => {
                            if let Some(s) = l.slots.iter_mut().find(|s| s.kind == 0) {
                                *s = Slot { kind: 2, name: name.chars().take(16).collect(), team: s.team, conn: id };
                            } else {
                                l.host.send_msg(id, &S2C::Kicked("The game is full.".into()));
                                l.host.kick(id);
                            }
                            l.dirty = true;
                        }
                        HostEvent::Left(id) => {
                            for s in l.slots.iter_mut().filter(|s| s.conn == id) {
                                *s = Slot { kind: 0, name: String::new(), team: s.team, conn: 0 };
                            }
                            l.dirty = true;
                        }
                        _ => {}
                    }
                }
                let x = sw * 0.5 - 250.0;
                draw_text("GAME LOBBY", x, 70.0, 40.0, Color::new(1.0, 0.85, 0.4, 1.0));
                draw_text(&format!("Other players on your LAN can join at {}  (port {})", l.lan_ip, TCP_PORT), x, 100.0, 18.0, Color::new(0.7, 0.75, 0.8, 1.0));
                let info = lobby_info(l, 0);
                if let Some((k, col)) = draw_slots(&info.slots, 0, x, 130.0) {
                    if col == 1 {
                        l.slots[k].team = (l.slots[k].team + 1) % 4;
                    } else if k != 0 {
                        let s = &mut l.slots[k];
                        if s.kind == 2 {
                            l.host.send_msg(s.conn, &S2C::Kicked("You were removed by the host.".into()));
                            l.host.kick(s.conn);
                            *s = Slot { kind: 0, name: String::new(), team: s.team, conn: 0 };
                        } else {
                            s.kind = match s.kind {
                                0 => 3,
                                3 => 1,
                                _ => 0,
                            };
                        }
                    }
                    l.dirty = true;
                }
                draw_text("Click a slot to switch between Open / AI / Closed. Same team number = allies.", x, 335.0, 16.0, Color::new(0.6, 0.6, 0.6, 1.0));
                let size_name = MAP_SIZES.iter().find(|m| m.0 == l.map_cells).map(|m| m.1).unwrap_or("?");
                if button(Rect::new(x, 360.0, 500.0, 40.0), &format!("Map size: {size_name}"), true) {
                    let k = MAP_SIZES.iter().position(|m| m.0 == l.map_cells).unwrap_or(0);
                    l.map_cells = MAP_SIZES[(k + 1) % MAP_SIZES.len()].0;
                    l.dirty = true;
                }
                if l.dirty {
                    l.dirty = false;
                    for (k, s) in l.slots.iter().enumerate() {
                        if s.conn != 0 {
                            l.host.send_msg(s.conn, &S2C::Lobby(lobby_info(l, k as i8)));
                        }
                    }
                }
                let start = button(Rect::new(x, 420.0, 500.0, 54.0), "START", true);
                let back = button(Rect::new(x, 490.0, 500.0, 40.0), "Back", true) || is_key_pressed(KeyCode::Escape);
                if start {
                    if let Screen::HostLobby(l) = std::mem::replace(&mut screen, Screen::Menu) {
                        next = Some(Screen::Game(Box::new(start_game(l))));
                    }
                } else if back {
                    next = Some(Screen::Menu);
                }
            }

            Screen::ClientLobby(l) => {
                clear_background(Color::new(0.06, 0.07, 0.09, 1.0));
                let mut lost: Option<String> = None;
                let mut session: Option<(Sim, game::GameView)> = None;
                loop {
                    match l.client.frames.try_recv() {
                        Ok(Some((TAG_MSG, data))) => match bincode::deserialize::<S2C>(&data) {
                            Ok(S2C::Lobby(i)) => l.info = Some(i),
                            Ok(S2C::Start { map, you }) => l.pending = Some((map, you)),
                            Ok(S2C::Kicked(why)) => lost = Some(why),
                            Err(_) => lost = Some("Protocol mismatch - are both players on the same version?".into()),
                        },
                        Ok(Some((_, data))) => {
                            if let (Some((map, you)), Ok(s)) = (l.pending.take(), bincode::deserialize::<State>(&data)) {
                                let mut sim = Sim::view(map);
                                sim.set_state(s);
                                let view = game::GameView::new(&sim, you);
                                session = Some((sim, view));
                                break;
                            }
                        }
                        Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            lost.get_or_insert("Disconnected from the host.".into());
                            break;
                        }
                        Err(_) => break,
                    }
                }
                let x = sw * 0.5 - 250.0;
                draw_text("GAME LOBBY", x, 70.0, 40.0, Color::new(1.0, 0.85, 0.4, 1.0));
                match &l.info {
                    Some(i) => {
                        draw_slots(&i.slots, i.you, x, 130.0);
                        let size_name = MAP_SIZES.iter().find(|m| m.0 == i.map_cells).map(|m| m.1).unwrap_or("?");
                        draw_text(&format!("Map size: {size_name}"), x, 345.0, 22.0, WHITE);
                        draw_text("Waiting for the host to start...", x, 380.0, 22.0, Color::new(0.7, 0.75, 0.8, 1.0));
                    }
                    None => {
                        draw_text("Connecting...", x, 150.0, 24.0, WHITE);
                    }
                }
                let back = button(Rect::new(x, 490.0, 500.0, 40.0), "Leave", true) || is_key_pressed(KeyCode::Escape);
                if let Some((sim, view)) = session {
                    if let Screen::ClientLobby(l) = std::mem::replace(&mut screen, Screen::Menu) {
                        next = Some(Screen::Game(Box::new(Session { sim, view, role: Role::Client { client: l.client }, acc: 0.0 })));
                    }
                } else if let Some(why) = lost {
                    message = why;
                    next = Some(Screen::Menu);
                } else if back {
                    next = Some(Screen::Menu);
                }
            }

            Screen::Game(s) => {
                if let Some(why) = s.frame() {
                    message = why;
                    next = Some(Screen::Menu);
                    discovery = Discovery::new();
                }
            }
        }
        if let Some(n) = next {
            screen = n;
        }
        next_frame().await;
    }
}

fn selftest() {
    let map = map::Map::generate(192, 12345);
    let setup: Vec<PlayerSetup> =
        (0..4).map(|i| PlayerSetup { name: format!("AI {i}"), team: i as u8, is_ai: true, slot: i }).collect();
    let mut sim = Sim::new(map, &setup);
    let mut ais: Vec<ai::Ai> = (0..4).map(|i| ai::Ai::new(i as u8, 99)).collect();
    let t0 = std::time::Instant::now();
    for t in 0..36000u32 {
        for a in ais.iter_mut() {
            if (t + a.player as u32 * 3) % 10 == 0 {
                for c in a.think(&sim) {
                    sim.apply(a.player, c);
                }
            }
        }
        sim.tick();
        if t % 3000 == 0 || sim.s.winner.is_some() {
            let snap = bincode::serialize(&sim.s).unwrap().len();
            println!("t={:5}s units={} projs={} wrecks={} snapshot={}B wall={:?}", t / 10, sim.s.units.len(), sim.s.projs.len(), sim.s.wrecks.len(), snap, t0.elapsed());
            for (k, p) in sim.s.players.iter().enumerate() {
                let n = sim.s.units.iter().filter(|u| u.owner as usize == k).count();
                println!("   {} alive={} mass {:.0}/{:.0} +{:.1} -{:.1}  energy {:.0}/{:.0} +{:.0} -{:.0} units={}", p.name, p.alive, p.mass, p.max_mass, p.income_m, p.spend_m, p.energy, p.max_energy, p.income_e, p.spend_e, n);
            }
        }
        if sim.s.winner.is_some() {
            println!("winner: team {:?} after {}s", sim.s.winner, t / 10);
            break;
        }
    }
}

/// Loopback check of discovery, lobby handshake, snapshots and commands.
fn nettest() {
    use std::time::Duration;
    let host = Host::start("testhost").expect("host");
    let mut disc = Discovery::new();
    for _ in 0..20 {
        disc.poll(1.1);
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("discovery found: {:?}", disc.found);
    let mut client = Client::connect("127.0.0.1", "tester").expect("connect");
    let mut conn = 0;
    for _ in 0..2 {
        match host.events.recv_timeout(Duration::from_secs(2)).expect("event") {
            HostEvent::Joined(id) => conn = id,
            HostEvent::Msg(id, C2S::Hello { name }) => println!("hello from {name} (conn {id})"),
            _ => panic!("unexpected event"),
        }
    }
    let map = map::Map::generate(128, 7);
    let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i * 2 }).collect();
    let mut sim = Sim::new(map, &setup);
    host.send_msg(conn, &S2C::Start { map: sim.map.clone(), you: 1 });
    sim.tick();
    host.send(conn, &frame(TAG_SNAP, bincode::serialize(&sim.s).unwrap()));
    let mut view: Option<Sim> = None;
    for _ in 0..2 {
        let (tag, data) = client.frames.recv_timeout(Duration::from_secs(2)).expect("frame").expect("open");
        if tag == TAG_MSG {
            if let Ok(S2C::Start { map, you }) = bincode::deserialize::<S2C>(&data) {
                println!("client got Start: map {} cells, you={you}", map.n);
                view = Some(Sim::view(map));
            }
        } else {
            let s: State = bincode::deserialize(&data).expect("state");
            println!("client got snapshot tick {} with {} units ({} bytes)", s.tick, s.units.len(), data.len());
            view.as_mut().expect("start first").set_state(s);
        }
    }
    let v = view.unwrap();
    let com = v.s.units.iter().find(|u| u.owner == 1).unwrap();
    let target = com.pos + map::v2(20.0, 0.0);
    client.send(&C2S::Cmd(Command::Orders { ids: vec![com.id], orders: vec![Order::Move(target)], queue: false }));
    match host.events.recv_timeout(Duration::from_secs(2)).expect("cmd") {
        HostEvent::Msg(_, C2S::Cmd(c)) => sim.apply(1, c),
        _ => panic!("expected command"),
    }
    let before = sim.unit(com.id).unwrap().pos;
    for _ in 0..50 {
        sim.tick();
    }
    let after = sim.unit(com.id).unwrap().pos;
    println!("remote commander moved {:.1} units toward its order", before.dist(after));
    assert!(before.dist(after) > 5.0);
    drop(client);
    match host.events.recv_timeout(Duration::from_secs(2)).expect("left") {
        HostEvent::Left(id) => println!("conn {id} left cleanly"),
        _ => panic!("expected Left"),
    }
    println!("nettest OK");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--selftest") {
        return selftest();
    }
    if args.iter().any(|a| a == "--nettest") {
        return nettest();
    }
    let conf = macroquad::conf::Conf {
        miniquad_conf: macroquad::miniquad::conf::Conf {
            window_title: "Long Range".into(),
            window_width: 1280,
            window_height: 720,
            sample_count: 4,
            ..Default::default()
        },
        update_on: None,
        draw_call_vertex_capacity: 40000,
        draw_call_index_capacity: 60000,
        ..Default::default()
    };
    macroquad::Window::from_config(conf, amain());
}
