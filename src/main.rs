//! Long Range - game executable: window, main menu, lobby and the host/client session loop.
// Note: no `windows_subsystem = "windows"`: the console window carries the log, which is what
// people need when the game fails to start on their GPU (`longrange.exe > log.txt 2>&1`).

use bevy::prelude::*;
use bevy::window::{MonitorSelection, PrimaryWindow, WindowMode};
use bevy_egui::egui;
use longrange::ai::{Ai, Ais};
use longrange::data::{asset_root, Defs};
use longrange::map::*;
use longrange::net::*;
use longrange::sim::*;
use longrange::sim::Command;
use longrange::view::{self, terrain::MainCamera, LocalCommands, ViewState};

const SIZES: [(usize, &str); 3] = [(192, "small"), (256, "medium"), (384, "large")];

struct Slot {
    kind: u8,
    name: String,
    team: u8,
    faction: u8,
    conn: u32,
}

struct HostLobby {
    host: Host,
    slots: Vec<Slot>,
    /// 0..SIZES.len() are random maps, the rest index into `maps`.
    map_choice: usize,
    maps: Vec<String>,
    /// Player count for random maps (premade maps bring their own start positions).
    players: usize,
    dirty: bool,
    lan_ip: String,
}

impl HostLobby {
    fn map_label(&self) -> String {
        match SIZES.get(self.map_choice) {
            Some((n, name)) => format!("Random, {name} ({} x {})", *n as f32 * CELL, *n as f32 * CELL),
            None => self.maps.get(self.map_choice - SIZES.len()).cloned().unwrap_or_default(),
        }
    }
    /// Number of start positions of the chosen map.
    fn start_count(&self) -> usize {
        match SIZES.get(self.map_choice) {
            Some(_) => self.players,
            None => MapData::start_count(&asset_root().join("maps").join(&self.maps[self.map_choice - SIZES.len()])),
        }
        .clamp(2, MAX_PLAYERS)
    }
    /// Makes the slot list match the map: one slot per start position.
    fn resize_slots(&mut self, faction: u8) {
        let n = self.start_count();
        while self.slots.len() > n {
            if let Some(s) = self.slots.pop() {
                if s.conn != 0 {
                    self.host.send_msg(s.conn, &S2C::Kicked("The map changed and your slot was removed.".into()));
                    self.host.kick(s.conn);
                }
            }
        }
        while self.slots.len() < n {
            let k = self.slots.len() as u8;
            self.slots.push(Slot { kind: 0, name: String::new(), team: k, faction, conn: 0 });
        }
        self.dirty = true;
    }
    fn info(&self, you: i8) -> LobbyInfo {
        LobbyInfo {
            slots: self.slots.iter().map(|s| SlotInfo { kind: s.kind, name: s.name.clone(), team: s.team, faction: s.faction }).collect(),
            map_name: self.map_label(),
            you,
        }
    }
}

struct ClientLobby {
    client: Client,
    info: Option<LobbyInfo>,
    pending: Option<(MapData, u8)>,
}

enum Role {
    Host { host: Host, remote: Vec<(u32, u8)> },
    Client { client: Client },
}

struct Session {
    role: Role,
    acc: f32,
}

enum Screen {
    Menu,
    HostLobby(HostLobby),
    ClientLobby(ClientLobby),
    Game(Session),
}

/// Top-level application state (kept off the ECS threads: it owns sockets).
struct AppState {
    screen: Screen,
    name: String,
    ip: String,
    message: String,
    discovery: Discovery,
    skirmish: bool,
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

fn new_lobby(name: &str, defs: &Defs) -> Result<HostLobby, String> {
    let host = Host::start(name)?;
    let faction = defs.playable_factions().first().copied().unwrap_or(0) as u8;
    let mut slots: Vec<Slot> = (0..4).map(|k| Slot { kind: 0, name: String::new(), team: k, faction, conn: 0 }).collect();
    slots[0].kind = 2;
    slots[0].name = name.to_string();
    slots[2].kind = 3;
    Ok(HostLobby { host, slots, map_choice: 1, maps: MapData::list(&asset_root()), players: 4, dirty: true, lan_ip: local_ip() })
}

fn start_game(world: &mut World, l: HostLobby) -> Result<Session, String> {
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(1) | 1;
    let map = match SIZES.get(l.map_choice) {
        Some((n, _)) => MapData::generate(*n, seed, l.slots.len()),
        None => MapData::load(&asset_root().join("maps").join(&l.maps[l.map_choice - SIZES.len()]))?,
    };
    let mut setup = vec![];
    let mut remote = vec![];
    let mut ais = vec![];
    for (k, s) in l.slots.iter().enumerate() {
        if s.kind < 2 || k >= map.starts.len() {
            continue;
        }
        let idx = setup.len() as u8;
        // LR_WATCH=1 lets an AI play the local slot too (spectating / testing).
        if s.kind == 3 || (s.conn == 0 && std::env::var("LR_WATCH").is_ok()) {
            ais.push(Ai::new(idx, seed));
        } else if s.conn != 0 {
            remote.push((s.conn, idx));
        }
        let name = if s.kind == 3 { format!("AI {}", k + 1) } else { s.name.clone() };
        setup.push(PlayerSetup { name, team: s.team, is_ai: s.kind == 3, slot: k, faction: s.faction });
    }
    for (conn, idx) in &remote {
        l.host.send_msg(*conn, &S2C::Start { map: map.clone(), you: *idx });
    }
    setup_game(world, map, &setup);
    world.insert_resource(Ais(ais));
    // LR_DUEL=1 stages a tank fight next to the first start (visual testing of combat).
    if std::env::var("LR_DUEL").is_ok() {
        let defs = world.resource::<Defs>().clone();
        let map = world.resource::<MapData>().clone();
        let mut game = world.resource::<Game>().clone();
        let at = game.players[0].start;
        {
            let mut commands = world.commands();
            let mut put = |name: &str, owner: u8, pos: V2, yaw: f32| {
                if let Some(d) = defs.id(name) {
                    spawn_unit(&mut commands, &defs, &mut game, &map, d, owner, pos, yaw, true, 0);
                }
            };
            for k in 0..4 {
                put("crony", 0, at + v2(8.0, k as f32 * 2.5 - 4.0), 0.0);
                put("overtaker", 1, at + v2(52.0, k as f32 * 2.5 - 4.0), 3.14);
                put("trooper", 1, at + v2(40.0, k as f32 * 2.0 - 10.0), 3.14);
            }
            put("radar_car", 0, at + v2(5.0, 6.0), 0.0);
            put("pike", 0, at + v2(2.0, -8.0), 0.0);
            put("mbfag", 0, at + v2(4.0, 9.0), 0.0);
            put("bomber", 1, at + v2(70.0, 0.0), 3.14);
            put("gunship", 1, at + v2(60.0, 12.0), 3.14);
        }
        world.flush();
        world.insert_resource(game);
    }
    *l.host.info.lock().unwrap() = "(game in progress)".into();
    view::start_view(world, 0, true);
    Ok(Session { role: Role::Host { host: l.host, remote }, acc: 0.0 })
}

fn leave_game(world: &mut World) {
    view::stop_view(world);
    clear_game(world);
    world.remove_resource::<Ais>();
    world.insert_resource(Replicas::default());
}

/// Runs the simulation (host) or applies snapshots (client), once per frame before rendering.
fn session_update(world: &mut World) {
    let Some(mut app) = world.remove_non_send::<AppState>() else { return };
    let mut over: Option<String> = None;
    if let Screen::Game(s) = &mut app.screen {
        let me = world.get_resource::<ViewState>().map_or(0, |v| v.me);
        let local: Vec<Command> = world.get_resource_mut::<LocalCommands>().map(|mut c| c.0.drain(..).collect()).unwrap_or_default();
        match &mut s.role {
            Role::Host { host, remote } => {
                let mut cmds: Vec<(u8, Command)> = local.into_iter().map(|c| (me, c)).collect();
                while let Ok(ev) = host.events.try_recv() {
                    match ev {
                        HostEvent::Msg(id, C2S::Cmd(c)) => {
                            if let Some((_, p)) = remote.iter().find(|r| r.0 == id) {
                                cmds.push((*p, c));
                            }
                        }
                        HostEvent::Left(id) => {
                            if let Some(k) = remote.iter().position(|r| r.0 == id) {
                                cmds.push((remote[k].1, Command::Resign));
                                remote.remove(k);
                            }
                        }
                        HostEvent::Joined(id) => host.send_msg(id, &S2C::Kicked("The game has already started.".into())),
                        _ => {}
                    }
                }
                world.resource_mut::<CommandQueue>().0.extend(cmds);
                // LR_SPEED=4 fast-forwards a hosted game (testing, watching AIs).
                let speed = std::env::var("LR_SPEED").ok().and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0).clamp(0.1, 20.0);
                let dt = world.resource::<Time>().delta_secs().min(0.25);
                s.acc += dt * speed;
                let cap = (3.0 * speed).ceil() as i32;
                let mut n = 0;
                while s.acc >= DT && n < cap {
                    s.acc -= DT;
                    n += 1;
                    world.run_schedule(SimTick);
                    if !remote.is_empty() {
                        let snap = make_snapshot(world);
                        if let Ok(data) = bincode::serialize(&snap) {
                            let f = frame(TAG_SNAP, data);
                            for (conn, _) in remote.iter() {
                                host.send(*conn, &f);
                            }
                        }
                    }
                }
                if n == cap {
                    s.acc = 0.0;
                }
            }
            Role::Client { client } => {
                for c in local {
                    client.send(&C2S::Cmd(c));
                }
                loop {
                    match client.frames.try_recv() {
                        Ok(Some((TAG_SNAP, data))) => {
                            if let Ok(snap) = bincode::deserialize::<Snapshot>(&data) {
                                apply_snapshot(world, snap);
                            }
                        }
                        Ok(Some(_)) => {}
                        Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            over = Some("Connection to the host was lost.".into());
                            break;
                        }
                        Err(_) => break,
                    }
                }
            }
        }
        if world.get_resource::<ViewState>().map_or(false, |v| v.quit) {
            over = Some(String::new());
        }
    }
    if let Some(msg) = over {
        leave_game(world);
        app.message = msg;
        app.screen = Screen::Menu;
        app.discovery = Discovery::new();
    }
    world.insert_non_send(app);
}

fn slot_rows(ui: &mut egui::Ui, defs: &Defs, slots: &[SlotInfo], you: i8, editable: bool) -> Option<(usize, u8)> {
    let mut clicked = None;
    egui::Grid::new("slots").num_columns(4).spacing([12.0, 8.0]).show(ui, |ui| {
        for (k, s) in slots.iter().enumerate() {
            let c = defs.rules.player_colors[k % defs.rules.player_colors.len()];
            ui.colored_label(egui::Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8), format!("Start {}", k + 1));
            let label = match s.kind {
                0 => "Open".to_string(),
                1 => "Closed".to_string(),
                2 => format!("{}{}", s.name, if k as i8 == you { " (you)" } else { "" }),
                _ => "AI".to_string(),
            };
            if ui.add_sized([220.0, 30.0], egui::Button::new(label)).clicked() && editable {
                clicked = Some((k, 0));
            }
            if ui.add_sized([90.0, 30.0], egui::Button::new(format!("Team {}", s.team + 1))).clicked() && editable {
                clicked = Some((k, 1));
            }
            let fname = defs.factions.get(s.faction as usize).map_or("?", |f| f.name.as_str());
            if ui.add_sized([130.0, 30.0], egui::Button::new(fname)).clicked() && (editable || k as i8 == you) {
                clicked = Some((k, 2));
            }
            ui.end_row();
        }
    });
    clicked
}

/// Main menu and lobbies.
fn menu_ui(world: &mut World) {
    let Some(mut app) = world.remove_non_send::<AppState>() else { return };
    if matches!(app.screen, Screen::Game(_)) {
        world.insert_non_send(app);
        return;
    }
    let ctx = {
        let mut q = world.query_filtered::<&mut bevy_egui::EguiContext, With<bevy_egui::PrimaryEguiContext>>();
        match q.single_mut(world) {
            Ok(mut c) => c.get_mut().clone(),
            Err(_) => {
                world.insert_non_send(app);
                return;
            }
        }
    };
    let defs = world.resource::<Defs>().clone();
    let dt = world.resource::<Time>().delta_secs();
    let mut next: Option<Screen> = None;
    let playable = defs.playable_factions();
    let next_faction = |f: u8| -> u8 {
        let k = playable.iter().position(|x| *x == f as usize).unwrap_or(0);
        playable.get((k + 1) % playable.len().max(1)).copied().unwrap_or(0) as u8
    };
    if app.skirmish {
        // `--skirmish`: straight into a game against three AIs.
        app.skirmish = false;
        match new_lobby(&app.name, &defs) {
            Ok(mut l) => {
                for s in l.slots.iter_mut().skip(1) {
                    s.kind = 3;
                }
                // LR_MAP=<name> picks a premade map for --skirmish.
                if let Ok(m) = std::env::var("LR_MAP") {
                    if let Some(k) = l.maps.iter().position(|x| *x == m) {
                        l.map_choice = SIZES.len() + k;
                        l.resize_slots(0);
                        for s in l.slots.iter_mut().skip(1) {
                            s.kind = 3;
                        }
                    }
                }
                match start_game(world, l) {
                    Ok(s) => next = Some(Screen::Game(s)),
                    Err(e) => app.message = e,
                }
            }
            Err(e) => app.message = e,
        }
    }
    ctx.layer_painter(egui::LayerId::background()).rect_filled(ctx.content_rect(), 0.0, egui::Color32::from_rgb(15, 18, 23));
    egui::Area::new(egui::Id::new("menu")).anchor(egui::Align2::CENTER_TOP, [0.0, 20.0]).show(&ctx, |ui| {
        ui.vertical_centered(|ui| {
            ui.set_max_width(560.0);
            match &mut app.screen {
                Screen::Menu => {
                    app.discovery.poll(dt);
                    ui.add_space(30.0);
                    ui.label(egui::RichText::new("LONG RANGE").size(56.0).color(egui::Color32::from_rgb(255, 215, 100)).strong());
                    ui.label(egui::RichText::new("flow economy  -  long guns  -  scouts win wars").size(16.0));
                    ui.add_space(26.0);
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        ui.add(egui::TextEdit::singleline(&mut app.name).desired_width(300.0).char_limit(16));
                    });
                    ui.add_space(10.0);
                    if ui.add_sized([440.0, 44.0], egui::Button::new(egui::RichText::new("Host game / Skirmish vs AI").size(18.0))).clicked() {
                        match new_lobby(&app.name, &defs) {
                            Ok(l) => {
                                next = Some(Screen::HostLobby(l));
                                app.message.clear();
                            }
                            Err(e) => app.message = e,
                        }
                    }
                    ui.add_space(18.0);
                    ui.label(egui::RichText::new("LAN games").size(18.0));
                    let mut join: Option<String> = None;
                    if app.discovery.found.is_empty() {
                        ui.label("searching the local network...");
                    }
                    for (addr, host_name) in app.discovery.found.iter().take(5) {
                        if ui.add_sized([440.0, 34.0], egui::Button::new(format!("Join {host_name}  ({addr})"))).clicked() {
                            join = Some(addr.to_string());
                        }
                    }
                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        ui.label("Or by IP");
                        let r = ui.add(egui::TextEdit::singleline(&mut app.ip).desired_width(200.0));
                        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                        if (ui.button("Join").clicked() || enter) && !app.ip.trim().is_empty() {
                            join = Some(app.ip.trim().to_string());
                        }
                    });
                    if let Some(addr) = join {
                        match Client::connect(&addr, &app.name, defs.hash) {
                            Ok(client) => {
                                next = Some(Screen::ClientLobby(ClientLobby { client, info: None, pending: None }));
                                app.message.clear();
                            }
                            Err(e) => app.message = e,
                        }
                    }
                    ui.add_space(14.0);
                    if ui.add_sized([440.0, 34.0], egui::Button::new("Quit")).clicked() {
                        world.write_message(AppExit::Success);
                    }
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new("Map editor: run the lr-editor program.   F11 fullscreen.").size(12.0).weak());
                    if !app.message.is_empty() {
                        ui.add_space(10.0);
                        ui.colored_label(egui::Color32::from_rgb(255, 120, 105), &app.message);
                    }
                }
                Screen::HostLobby(l) => {
                    while let Ok(ev) = l.host.events.try_recv() {
                        match ev {
                            HostEvent::Msg(id, C2S::Hello { name, data_hash }) => {
                                if data_hash != defs.hash {
                                    l.host.send_msg(id, &S2C::Kicked("Your game data (assets/data) differs from the host's.".into()));
                                    l.host.kick(id);
                                } else if let Some(s) = l.slots.iter_mut().find(|s| s.kind == 0) {
                                    s.kind = 2;
                                    s.name = name.chars().take(16).collect();
                                    s.conn = id;
                                } else {
                                    l.host.send_msg(id, &S2C::Kicked("The game is full.".into()));
                                    l.host.kick(id);
                                }
                                l.dirty = true;
                            }
                            HostEvent::Msg(id, C2S::Faction(f)) => {
                                if let Some(s) = l.slots.iter_mut().find(|s| s.conn == id) {
                                    if playable.contains(&(f as usize)) {
                                        s.faction = f;
                                    }
                                }
                                l.dirty = true;
                            }
                            HostEvent::Left(id) => {
                                for s in l.slots.iter_mut().filter(|s| s.conn == id) {
                                    s.kind = 0;
                                    s.name.clear();
                                    s.conn = 0;
                                }
                                l.dirty = true;
                            }
                            _ => {}
                        }
                    }
                    ui.label(egui::RichText::new("GAME LOBBY").size(34.0).color(egui::Color32::from_rgb(255, 215, 100)));
                    ui.label(format!("Other players on your LAN can join at {}  (port {})", l.lan_ip, TCP_PORT));
                    ui.add_space(12.0);
                    if let Some((k, col)) = slot_rows(ui, &defs, &l.info(0).slots, 0, true) {
                        match col {
                            1 => l.slots[k].team = (l.slots[k].team + 1) % l.slots.len() as u8,
                            2 => l.slots[k].faction = next_faction(l.slots[k].faction),
                            _ if k != 0 => {
                                let s = &mut l.slots[k];
                                if s.kind == 2 {
                                    l.host.send_msg(s.conn, &S2C::Kicked("You were removed by the host.".into()));
                                    l.host.kick(s.conn);
                                    s.kind = 0;
                                    s.name.clear();
                                    s.conn = 0;
                                } else {
                                    s.kind = match s.kind {
                                        0 => 3,
                                        3 => 1,
                                        _ => 0,
                                    };
                                }
                            }
                            _ => {}
                        }
                        l.dirty = true;
                    }
                    ui.label(egui::RichText::new("Click a slot to switch Open / AI / Closed. Same team number = allies.").size(12.0).weak());
                    ui.add_space(10.0);
                    if ui.add_sized([480.0, 36.0], egui::Button::new(format!("Map: {}", l.map_label()))).clicked() {
                        l.map_choice = (l.map_choice + 1) % (SIZES.len() + l.maps.len());
                        l.resize_slots(playable.first().copied().unwrap_or(0) as u8);
                    }
                    if l.map_choice < SIZES.len() && ui.add_sized([480.0, 30.0], egui::Button::new(format!("Players: {}", l.players))).clicked() {
                        l.players = if l.players >= MAX_PLAYERS { 2 } else { l.players + 1 };
                        l.resize_slots(playable.first().copied().unwrap_or(0) as u8);
                    }
                    if l.dirty {
                        l.dirty = false;
                        for (k, s) in l.slots.iter().enumerate() {
                            if s.conn != 0 {
                                l.host.send_msg(s.conn, &S2C::Lobby(l.info(k as i8)));
                            }
                        }
                    }
                    ui.add_space(10.0);
                    let start = ui.add_sized([480.0, 50.0], egui::Button::new(egui::RichText::new("START").size(22.0))).clicked();
                    let back = ui.add_sized([480.0, 34.0], egui::Button::new("Back")).clicked();
                    if start {
                        if let Screen::HostLobby(l) = std::mem::replace(&mut app.screen, Screen::Menu) {
                            match start_game(world, l) {
                                Ok(s) => next = Some(Screen::Game(s)),
                                Err(e) => app.message = e,
                            }
                        }
                    } else if back {
                        next = Some(Screen::Menu);
                    }
                }
                Screen::ClientLobby(l) => {
                    let mut lost: Option<String> = None;
                    let mut started = false;
                    loop {
                        match l.client.frames.try_recv() {
                            Ok(Some((TAG_MSG, data))) => match bincode::deserialize::<S2C>(&data) {
                                Ok(S2C::Lobby(i)) => l.info = Some(i),
                                Ok(S2C::Start { map, you }) => l.pending = Some((map, you)),
                                Ok(S2C::Kicked(why)) => lost = Some(why),
                                Err(_) => lost = Some("Protocol mismatch - are both players on the same version?".into()),
                            },
                            Ok(Some((_, data))) => {
                                if let (Some((mut map, you)), Ok(snap)) = (l.pending.take(), bincode::deserialize::<Snapshot>(&data)) {
                                    map.rebuild_blocked();
                                    clear_game(world);
                                    world.insert_resource(map);
                                    world.insert_resource(Replicas::default());
                                    world.insert_resource(FxLog::default());
                                    apply_snapshot(world, snap);
                                    view::start_view(world, you, true);
                                    started = true;
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
                    ui.label(egui::RichText::new("GAME LOBBY").size(34.0).color(egui::Color32::from_rgb(255, 215, 100)));
                    ui.add_space(12.0);
                    match l.info.clone() {
                        Some(i) => {
                            if let Some((k, 2)) = slot_rows(ui, &defs, &i.slots, i.you, false) {
                                l.client.send(&C2S::Faction(next_faction(i.slots[k].faction)));
                            }
                            ui.add_space(8.0);
                            ui.label(format!("Map: {}", i.map_name));
                            ui.label("Waiting for the host to start...");
                        }
                        None => {
                            ui.label("Connecting...");
                        }
                    }
                    ui.add_space(10.0);
                    let back = ui.add_sized([480.0, 34.0], egui::Button::new("Leave")).clicked();
                    if started {
                        if let Screen::ClientLobby(l) = std::mem::replace(&mut app.screen, Screen::Menu) {
                            next = Some(Screen::Game(Session { role: Role::Client { client: l.client }, acc: 0.0 }));
                        }
                    } else if let Some(why) = lost {
                        app.message = why;
                        next = Some(Screen::Menu);
                    } else if back {
                        next = Some(Screen::Menu);
                    }
                }
                Screen::Game(_) => {}
            }
        });
    });
    if let Some(n) = next {
        app.screen = n;
    }
    world.insert_non_send(app);
}

fn setup(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        view::camera_aa(),
        Transform::from_xyz(0.0, 50.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y),
        DistanceFog { color: Color::srgb(0.55, 0.62, 0.7), falloff: FogFalloff::Linear { start: 900.0, end: 4000.0 }, ..default() },
        MainCamera,
    ));
}

fn fullscreen_toggle(keys: Res<ButtonInput<KeyCode>>, mut windows: Query<&mut Window, With<PrimaryWindow>>) {
    if keys.just_pressed(KeyCode::F11) {
        for mut w in &mut windows {
            w.mode = if matches!(w.mode, WindowMode::Windowed) { WindowMode::BorderlessFullscreen(MonitorSelection::Current) } else { WindowMode::Windowed };
        }
    }
}

fn load_defs() -> Defs {
    match Defs::load(&asset_root()) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Cannot load game data from {}: {e}", asset_root().display());
            std::process::exit(1);
        }
    }
}

/// Headless world with just the simulation.
fn headless(defs: Defs) -> App {
    let mut app = App::new();
    app.insert_resource(defs);
    app.add_plugins(SimPlugin);
    app
}

fn selftest() {
    let defs = load_defs();
    println!("loaded {} unit definitions in {} factions", defs.units.len(), defs.factions.len());
    let mut app = headless(defs.clone());
    // LR_PLAYERS=10 runs the self-test with that many AIs (2-10).
    let n = std::env::var("LR_PLAYERS").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(4).clamp(2, MAX_PLAYERS);
    let map = MapData::generate(if n > 4 { 256 } else { 192 }, 12345, n);
    let faction = defs.playable_factions()[0] as u8;
    let setup: Vec<PlayerSetup> = (0..n).map(|i| PlayerSetup { name: format!("AI {i}"), team: i as u8, is_ai: true, slot: i, faction }).collect();
    let world = app.world_mut();
    setup_game(world, map, &setup);
    world.insert_resource(Ais((0..n).map(|i| Ai::new(i as u8, 99)).collect()));
    let t0 = std::time::Instant::now();
    for t in 0..36000u32 {
        world.run_schedule(SimTick);
        let game = world.resource::<Game>().clone();
        if t % 3000 == 0 || game.winner.is_some() {
            let snap = make_snapshot(world);
            let bytes = bincode::serialize(&snap).map(|b| b.len()).unwrap_or(0);
            println!("t={:5}s units={} projectiles={} wrecks={} snapshot={}B wall={:?}", t / 10, snap.units.len(), snap.projectiles.len(), snap.wrecks.len(), bytes, t0.elapsed());
            for (k, p) in game.players.iter().enumerate() {
                let mine: Vec<&UnitSnap> = snap.units.iter().filter(|u| u.unit.owner as usize == k).collect();
                let top = mine.iter().map(|u| defs.get(u.unit.def).tier).max().unwrap_or(0);
                println!("   {} alive={} mass {:.0}/{:.0} +{:.1} -{:.1}  energy {:.0}/{:.0} +{:.0} -{:.0}  units={} top tier={}", p.name, p.alive, p.mass, p.max_mass, p.income_m, p.spend_m, p.energy, p.max_energy, p.income_e, p.spend_e, mine.len(), top);
            }
        }
        if game.winner.is_some() {
            println!("winner: team {:?} after {}s", game.winner, t / 10);
            break;
        }
    }
}

/// Scripted duels and manoeuvres that print what happened: accuracy, misses, reverse driving,
/// radar shadowing and the assisted-upgrade regression.
fn combattest() {
    let defs = load_defs();
    let faction = defs.playable_factions()[0] as u8;
    let run = |title: &str, n: usize, place: &dyn Fn(&mut World, &MapData), ticks: u32, report: &dyn Fn(&mut World, u32)| {
        let mut app = headless(defs.clone());
        let mut map = MapData::generate(n, 4242, 2);
        // A flat test range.
        for h in map.heights.iter_mut() {
            *h = 2.0;
        }
        map.rebuild_blocked();
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, faction }).collect();
        let world = app.world_mut();
        setup_game(world, map.clone(), &setup);
        place(world, &map);
        world.flush();
        println!("== {title}");
        for t in 0..ticks {
            world.run_schedule(SimTick);
            report(world, t);
        }
    };
    let spawn = |world: &mut World, map: &MapData, name: &str, owner: u8, pos: V2, yaw: f32| -> u32 {
        let defs = world.resource::<Defs>().clone();
        let mut game = world.resource::<Game>().clone();
        let id = {
            let mut commands = world.commands();
            spawn_unit(&mut commands, &defs, &mut game, map, defs.id(name).expect("unit"), owner, pos, yaw, true, 0)
        };
        world.insert_resource(game);
        id
    };
    let hp_of = |world: &mut World, owner: u8| -> (usize, f32) {
        let mut q = world.query::<(&Unit, &Health)>();
        let v: Vec<f32> = q.iter(world).filter(|(u, _)| u.owner == owner && u.def != 0).map(|(_, h)| h.0).collect();
        (v.len(), v.iter().sum())
    };
    // 1. Tank duel against stationary and moving targets.
    for moving in [false, true] {
        run(
            if moving { "5 Crony vs 5 Overtakers driving across at range 50" } else { "5 Crony vs 5 parked Overtakers at range 50" },
            128,
            &|world, map| {
                let mut ids = vec![];
                for k in 0..5 {
                    spawn(world, map, "crony", 0, v2(100.0, 100.0 + k as f32 * 3.0), 0.0);
                    ids.push(spawn(world, map, "overtaker", 1, v2(150.0, 100.0 + k as f32 * 3.0), 3.14));
                }
                // Tanks see 30-35 but shoot 45-60: without spotters nobody fires.
                spawn(world, map, "radar_car", 0, v2(96.0, 106.0), 0.0);
                spawn(world, map, "radar_car", 1, v2(154.0, 106.0), 0.0);
                for p in world.resource_mut::<Game>().players.iter_mut() {
                    p.energy = 4000.0;
                    p.max_energy = 4000.0;
                }
                if moving {
                    let orders = vec![Order::Patrol(v2(150.0, 60.0))];
                    world.resource_mut::<CommandQueue>().0.push((1, Command::Orders { ids, orders, queue: false }));
                }
            },
            300,
            &|world, t| {
                if t % 100 == 99 {
                    let shells = world.query::<&Projectile>().iter(world).count();
                    let fired = world.resource::<Game>().next_id;
                    println!("   t={}s  team 0 (units, hp) {:?}  team 1 {:?}  shells in flight {}  ids used {}", (t + 1) / 10, hp_of(world, 0), hp_of(world, 1), shells, fired);
                }
            },
        );
    }
    // 2. Reverse driving: a short hop backwards should not turn the hull around.
    run(
        "Crony ordered 6 units straight back, then 80 units back",
        128,
        &|world, map| {
            let a = spawn(world, map, "crony", 0, v2(100.0, 100.0), 0.0);
            let b = spawn(world, map, "crony", 0, v2(100.0, 140.0), 0.0);
            let q = &mut world.resource_mut::<CommandQueue>().0;
            q.push((0, Command::Orders { ids: vec![a], orders: vec![Order::Move(v2(94.0, 100.0))], queue: false }));
            q.push((0, Command::Orders { ids: vec![b], orders: vec![Order::Move(v2(20.0, 140.0))], queue: false }));
        },
        120,
        &|world, t| {
            if t == 20 || t == 119 {
                let mut q = world.query::<(&Unit, &Body, &Mover)>();
                for (u, b, m) in q.iter(world).filter(|(u, ..)| u.def != 0) {
                    println!("   t={:.0}s unit {} at ({:.1},{:.1}) heading {:.0} deg reversing={} speed {:.2}", (t + 1) as f32 / 10.0, u.id, b.pos.x, b.pos.y, b.yaw.to_degrees(), m.reversing, m.speed);
                }
            }
        },
    );
    // 3. Assisted pump upgrade must happen exactly once.
    run(
        "Pump upgrade assisted by 3 engineers",
        128,
        &|world, map| {
            let dep = map.deposits[0];
            let pump = spawn(world, map, "pump", 0, dep, 0.0);
            let mut eng = vec![];
            for k in 0..3 {
                eng.push(spawn(world, map, "engineer", 0, dep + v2(3.0, k as f32 * 1.5 - 1.5), 0.0));
            }
            for p in world.resource_mut::<Game>().players.iter_mut() {
                p.mass = 100000.0;
                p.energy = 1000000.0;
                p.max_mass = 100000.0;
            }
            let q = &mut world.resource_mut::<CommandQueue>().0;
            q.push((0, Command::Upgrade { ids: vec![pump] }));
            q.push((0, Command::Orders { ids: eng, orders: vec![Order::Assist(pump)], queue: false }));
        },
        900,
        &|world, t| {
            for p in world.resource_mut::<Game>().players.iter_mut() {
                p.mass = 5000.0;
                p.energy = 50000.0;
            }
            if t % 150 == 149 {
                let defs = world.resource::<Defs>().clone();
                let mut q = world.query::<(&Unit, Option<&Upgrading>)>();
                for (u, up) in q.iter(world).filter(|(u, _)| defs.get(u.def).needs_deposit) {
                    println!("   t={}s pump is '{}' upgrading={:?}", (t + 1) / 10, defs.get(u.def).id, up.map(|x| (x.0 * 100.0).round()));
                }
            }
        },
    );
    // 3b. Engineering station, area reclaim and ground fire.
    run(
        "Station finishes a nearby site; engineer reclaims an area; mortar shells the ground",
        128,
        &|world, map| {
            let defs = world.resource::<Defs>().clone();
            spawn(world, map, "eng_station", 0, v2(101.0, 101.0), 0.0);
            let mut game = world.resource::<Game>().clone();
            {
                let mut commands = world.commands();
                spawn_unit(&mut commands, &defs, &mut game, map, defs.id("windmill").unwrap(), 0, v2(107.0, 101.0), 0.0, false, 0);
                for k in 0..4 {
                    let id = game.alloc();
                    commands.spawn(Wreck { id, def: defs.id("crony").unwrap(), mass: 300.0, pos: v2(140.0 + k as f32 * 3.0, 140.0), y: 2.0, yaw: 0.0 });
                }
            }
            world.insert_resource(game);
            let eng = spawn(world, map, "engineer", 0, v2(130.0, 140.0), 0.0);
            let pike = spawn(world, map, "pike", 0, v2(100.0, 180.0), 0.0);
            for p in world.resource_mut::<Game>().players.iter_mut() {
                p.mass = 0.0;
            }
            let q = &mut world.resource_mut::<CommandQueue>().0;
            q.push((0, Command::Orders { ids: vec![eng], orders: vec![Order::ReclaimArea { pos: v2(145.0, 140.0), radius: 12.0 }], queue: false }));
            q.push((0, Command::Orders { ids: vec![pike], orders: vec![Order::AttackGround { pos: v2(170.0, 180.0), radius: 6.0 }], queue: false }));
        },
        900,
        &|world, t| {
            if t % 300 == 299 {
                let defs = world.resource::<Defs>().clone();
                let wrecks = world.query::<&Wreck>().iter(world).count();
                let shells = world.query::<&Projectile>().iter(world).count();
                let mut q = world.query::<(&Unit, Option<&UnderConstruction>)>();
                let mill = q.iter(world).find(|(u, _)| defs.get(u.def).id == "windmill").map(|(_, uc)| uc.map(|x| (x.0 * 100.0).round()));
                let blasts = world.resource::<FxLog>().pending.iter().filter(|f| f.kind == FX_BLAST).count();
                let mass = world.resource::<Game>().players[0].mass;
                println!("   t={}s windmill under construction: {:?}  wrecks left {}  mass {:.0}  shells in flight {}  impacts so far {}", (t + 1) / 10, mill, wrecks, mass, shells, blasts);
            }
        },
    );
    // 3c. Energy stall: upkeep above income and an empty bank must not lock construction up.
    run(
        "Energy stall: 8 pumps (upkeep 32) on +20 income, empty bank, commander builds a windmill; then pumps paused",
        128,
        &|world, map| {
            let defs = world.resource::<Defs>().clone();
            for k in 0..8 {
                spawn(world, map, "pump", 0, map.deposits[k], 0.0);
            }
            for p in world.resource_mut::<Game>().players.iter_mut() {
                p.energy = 0.0;
            }
            let mut q = world.query::<(&Unit, &Body)>();
            let (com, at) = q.iter(world).find(|(u, _)| u.owner == 0 && Some(u.def) == defs.factions[defs.playable_factions()[0]].commander).map(|(u, b)| (u.id, b.pos)).unwrap();
            let site = longrange::sim::snap(&defs, map, defs.id("windmill").unwrap(), at + v2(6.0, 9.0));
            world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: vec![com], orders: vec![Order::Build { def: defs.id("windmill").unwrap(), pos: site }], queue: false }));
        },
        1200,
        &|world, t| {
            let defs = world.resource::<Defs>().clone();
            if t == 800 {
                let mut q = world.query::<&Unit>();
                let pumps: Vec<u32> = q.iter(world).filter(|u| defs.get(u.def).needs_deposit).map(|u| u.id).collect();
                world.resource_mut::<CommandQueue>().0.push((0, Command::Pause { ids: pumps, on: true }));
            }
            if t % 200 == 199 {
                let mut q = world.query::<(&Unit, Option<&UnderConstruction>)>();
                let mill = q.iter(world).find(|(u, _)| defs.get(u.def).id == "windmill").map(|(_, uc)| uc.map_or("complete".to_string(), |x| format!("{:.0}%", x.0 * 100.0)));
                let p = &world.resource::<Game>().players[0];
                println!("   t={}s windmill: {:?}  energy {:.0}  +{:.0} -{:.0}  power {:.2}  mass income {:.1}", (t + 1) / 10, mill, p.energy, p.income_e, p.spend_e, p.power, p.income_m);
            }
        },
    );
    // 4. Radar line of sight: a ridge between the radar and the target hides it.
    for ridge in [false, true] {
        let mut app = headless(defs.clone());
        let mut map = MapData::generate(128, 4242, 2);
        let n = map.n;
        for (i, h) in map.heights.iter_mut().enumerate() {
            let x = (i % (n + 1)) as f32 * CELL;
            *h = if ridge && (x - 125.0).abs() < 6.0 { 22.0 } else { 2.0 };
        }
        map.rebuild_blocked();
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, faction }).collect();
        let world = app.world_mut();
        setup_game(world, map.clone(), &setup);
        spawn(world, &map, "radar_tower", 0, v2(101.0, 101.0), 0.0);
        let t = spawn(world, &map, "crony", 1, v2(160.0, 101.0), 0.0);
        world.flush();
        for _ in 0..5 {
            world.run_schedule(SimTick);
        }
        let mut q = world.query::<(&Unit, &Intel)>();
        let intel = q.iter(world).find(|(u, _)| u.id == t).map(|(_, i)| *i).unwrap();
        println!("== Radar tower, target 59 away, ridge between: {ridge} -> on radar: {}, seen: {}", intel.radar & 1 != 0, intel.seen & 1 != 0);
    }
}

/// Loopback check of discovery, handshake, snapshots and remote commands.
fn nettest() {
    use std::time::Duration;
    let defs = load_defs();
    let host = Host::start("testhost").expect("host");
    let mut disc = Discovery::new();
    for _ in 0..20 {
        disc.poll(1.1);
        std::thread::sleep(Duration::from_millis(50));
    }
    println!("discovery found: {:?}", disc.found);
    let mut client = Client::connect("127.0.0.1", "tester", defs.hash).expect("connect");
    let mut conn = 0;
    for _ in 0..2 {
        match host.events.recv_timeout(Duration::from_secs(2)).expect("event") {
            HostEvent::Joined(id) => conn = id,
            HostEvent::Msg(id, C2S::Hello { name, data_hash }) => println!("hello from {name} (conn {id}), data hash match: {}", data_hash == defs.hash),
            _ => panic!("unexpected event"),
        }
    }
    let mut happ = headless(defs.clone());
    let map = MapData::generate(128, 7, 2);
    let faction = defs.playable_factions()[0] as u8;
    let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, faction }).collect();
    host.send_msg(conn, &S2C::Start { map: map.clone(), you: 1 });
    setup_game(happ.world_mut(), map, &setup);
    happ.world_mut().run_schedule(SimTick);
    let snap = make_snapshot(happ.world_mut());
    host.send(conn, &frame(TAG_SNAP, bincode::serialize(&snap).unwrap()));
    let mut capp = headless(defs.clone());
    for _ in 0..2 {
        let (tag, data) = client.frames.recv_timeout(Duration::from_secs(5)).expect("frame").expect("open");
        if tag == TAG_MSG {
            if let Ok(S2C::Start { map, you }) = bincode::deserialize::<S2C>(&data) {
                println!("client got Start: map {} cells ({} bytes), you={you}", map.n, data.len());
                capp.world_mut().insert_resource(map);
            }
        } else {
            let s: Snapshot = bincode::deserialize(&data).expect("snapshot");
            println!("client got snapshot tick {} with {} units ({} bytes)", s.game.tick, s.units.len(), data.len());
            apply_snapshot(capp.world_mut(), s);
        }
    }
    let cw = capp.world_mut();
    let com = cw.query::<(&Unit, &Body)>().iter(cw).find(|(u, _)| u.owner == 1).map(|(u, b)| (u.id, b.pos)).expect("replicated commander");
    client.send(&C2S::Cmd(Command::Orders { ids: vec![com.0], orders: vec![Order::Move(com.1 + v2(15.0, 0.0))], queue: false }));
    match host.events.recv_timeout(Duration::from_secs(2)).expect("cmd") {
        HostEvent::Msg(_, C2S::Cmd(c)) => happ.world_mut().resource_mut::<CommandQueue>().0.push((1, c)),
        _ => panic!("expected command"),
    }
    for _ in 0..80 {
        happ.world_mut().run_schedule(SimTick);
    }
    let hw = happ.world_mut();
    let after = hw.query::<(&Unit, &Body)>().iter(hw).find(|(u, _)| u.id == com.0).map(|(_, b)| b.pos).unwrap();
    println!("remote commander moved {:.1} units toward its order", com.1.dist(after));
    assert!(com.1.dist(after) > 5.0);
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
    if args.iter().any(|a| a == "--combattest") {
        return combattest();
    }
    if args.iter().any(|a| a == "--nettest") {
        return nettest();
    }
    let defs = load_defs();
    let mut name = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "Player".into());
    name.truncate(16);
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "Long Range".into(), resolution: bevy::window::WindowResolution::new(1280, 720), ..default() }),
        ..default()
    }).set(view::render_plugin()))
    .add_plugins(bevy_egui::EguiPlugin::default())
    .insert_resource(ClearColor(Color::srgb(0.06, 0.07, 0.09)))
    .insert_resource(defs)
    .insert_non_send(AppState { screen: Screen::Menu, name, ip: String::new(), message: String::new(), discovery: Discovery::new(), skirmish: args.iter().any(|a| a == "--skirmish") })
    .add_plugins((SimPlugin, view::ViewPlugin))
    .add_systems(Startup, setup)
    .add_systems(PreUpdate, session_update)
    .add_systems(Update, fullscreen_toggle)
    .add_systems(bevy_egui::EguiPrimaryContextPass, menu_ui);
    view::tolerate_render_errors(&mut app);
    app.run();
}
