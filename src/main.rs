//! Long Range - game executable: window, main menu, lobby and the host/client session loop.
// Note: no `windows_subsystem = "windows"`: the console window carries the log, which is what
// people need when the game fails to start on their GPU (`longrange.exe > log.txt 2>&1`).

use bevy::prelude::*;
use bevy_egui::egui;
use longrange::ai::{Ai, Ais};
use longrange::data::{asset_root, DataPack, Defs};
use longrange::map::*;
use longrange::net::*;
use longrange::netplay::{ClientSim, HostSim};
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
    /// Fingerprint of that player's game data (from `Hello`).
    hash: u64,
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
    /// `MODE_ANNIHILATION` or `MODE_POINTS`.
    mode: u8,
    /// Control points mode: points every team starts with; 0 = the value in the game data.
    cp_start: u32,
    /// Game data: 0 = built-in, k = `presets[k - 1]`.
    preset: usize,
    presets: Vec<String>,
    /// Seed of the random map (re-rolled from the Game options tab).
    seed: u64,
    /// The chosen map, made for the current choice (see `map_key`), and its preview picture.
    map: Option<(MapKey, MapData)>,
    preview: Vec<u8>,
    starts: Vec<(f32, f32)>,
    tex: Option<egui::TextureHandle>,
    /// Thumbnails of the premade maps for the Map tab (made one per frame).
    thumbs: Vec<Option<egui::TextureHandle>>,
    /// Right-hand tab: 0 = game options, 1 = map.
    tab: u8,
    /// AI engineers reclaim trees and rocks.
    ai_reclaim: bool,
    /// Start positions of each premade map.
    counts: Vec<usize>,
}

/// What a lobby's map depends on: (choice, players, seed, data hash).
type MapKey = (usize, usize, u64, u64);
const PREVIEW: usize = 256;

impl HostLobby {
    fn mode_label(&self) -> &'static str {
        if self.mode == MODE_POINTS {
            "Control points (hold more positions to drain the enemy)"
        } else {
            "Annihilation (destroy the commanders)"
        }
    }
    /// Starting points of a control points game: the lobby's choice or the game data's value.
    fn cp_points(&self, defs: &Defs) -> f32 {
        if self.cp_start == 0 { defs.rules.cp_start_points } else { self.cp_start as f32 }
    }
    fn preset_label(&self) -> String {
        match self.preset {
            0 => "built-in".to_string(),
            k => self.presets.get(k - 1).cloned().unwrap_or_default(),
        }
    }
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
            self.slots.push(Slot { kind: 0, name: String::new(), team: k, faction, conn: 0, hash: 0 });
        }
        self.dirty = true;
    }
    fn map_key(&self, defs: &Defs) -> MapKey {
        (self.map_choice, if self.map_choice < SIZES.len() { self.players } else { 0 }, if self.map_choice < SIZES.len() { self.seed } else { 0 }, defs.hash)
    }
    /// Makes (or loads) the chosen map if the choice changed, with its preview picture.
    fn ensure_map(&mut self, defs: &Defs) -> Result<(), String> {
        let key = self.map_key(defs);
        if self.map.as_ref().map_or(false, |m| m.0 == key) {
            return Ok(());
        }
        let map = match SIZES.get(self.map_choice) {
            Some((n, _)) => {
                let mut m = MapData::generate(*n, self.seed, self.slots.len());
                m.scatter_features(&defs.rules.features, self.seed, 1.0);
                m
            }
            None => MapData::load(&asset_root().join("maps").join(&self.maps[self.map_choice - SIZES.len()]))?,
        };
        self.preview = view::terrain::map_preview(&map, defs, PREVIEW);
        let w = map.world().max(1.0);
        self.starts = map.starts.iter().map(|s| (s.x / w, s.y / w)).collect();
        self.tex = None;
        self.map = Some((key, map));
        self.dirty = true;
        Ok(())
    }
    fn options_text(&self, defs: &Defs) -> String {
        let mut t = format!("Mode: {}", self.mode_label());
        if self.mode == MODE_POINTS {
            t += &format!("\nStarting points: {:.0}", self.cp_points(defs));
        }
        t += &format!("\nGame data: {}\nAI reclaims trees and rocks: {}", self.preset_label(), if self.ai_reclaim { "yes" } else { "no" });
        t
    }
    fn info(&self, you: i8, defs: &Defs) -> LobbyInfo {
        LobbyInfo {
            slots: self.slots.iter().map(|s| SlotInfo { kind: s.kind, name: s.name.clone(), team: s.team, faction: s.faction }).collect(),
            map_name: self.map_label(),
            you,
            extra: self.options_text(defs),
            preview: self.preview.clone(),
            preview_side: PREVIEW as u16,
            starts: self.starts.clone(),
        }
    }
}

struct ClientLobby {
    client: Client,
    info: Option<LobbyInfo>,
    pending: Option<(MapData, u8)>,
    /// The map preview the host sent (keyed by a checksum of its pixels).
    tex: Option<(u64, egui::TextureHandle)>,
}

enum Role {
    Host { host: Host, remote: Vec<(u32, u8)>, sim: HostSim },
    Client { client: Client, sim: ClientSim },
}

struct Session {
    role: Role,
    acc: f32,
    /// Host: the game is paused, by that player.
    paused: Option<u8>,
}

/// The preset editor: a preset is a named copy of the game data that can be changed freely.
#[derive(Default)]
struct PresetUi {
    names: Vec<String>,
    /// The preset being edited (with changes that may not be saved yet).
    pack: Option<DataPack>,
    new_name: String,
    /// 0 = rules.json, k = faction file k - 1.
    file: usize,
    /// Units of the open faction file: (id, name).
    units: Vec<(String, String)>,
    unit: usize,
    /// The JSON being edited: the rules file, or one unit.
    text: String,
    msg: String,
    confirm_delete: bool,
}

impl PresetUi {
    fn new() -> PresetUi {
        PresetUi { names: DataPack::list(&asset_root()), ..Default::default() }
    }
    fn open(&mut self, name: &str) {
        match DataPack::preset(&asset_root(), name) {
            Ok(p) => {
                self.pack = Some(p);
                self.file = 0;
                self.unit = 0;
                self.msg.clear();
                self.show();
            }
            Err(e) => self.msg = e,
        }
        self.confirm_delete = false;
    }
    /// Puts the selected file or unit into the text box.
    fn show(&mut self) {
        let Some(pack) = &self.pack else { return };
        self.units.clear();
        if self.file == 0 || self.file > pack.factions.len() {
            self.file = 0;
            self.text = pack.rules.clone();
            return;
        }
        let v: serde_json::Value = serde_json::from_str(&pack.factions[self.file - 1].1).unwrap_or_default();
        let list = v["units"].as_array().cloned().unwrap_or_default();
        self.units = list.iter().map(|u| (u["id"].as_str().unwrap_or("?").to_string(), u["name"].as_str().unwrap_or("").to_string())).collect();
        self.unit = self.unit.min(list.len().saturating_sub(1));
        self.text = list.get(self.unit).map(|u| serde_json::to_string_pretty(u).unwrap_or_default()).unwrap_or_default();
    }
    /// The pack with the text box written back into it.
    fn edited(&self) -> Result<DataPack, String> {
        let mut pack = self.pack.clone().ok_or("no preset open")?;
        if self.file == 0 {
            serde_json::from_str::<serde_json::Value>(&self.text).map_err(|e| format!("rules.json: {e}"))?;
            pack.rules = self.text.clone();
        } else {
            let f = pack.factions.get_mut(self.file - 1).ok_or("no such file")?;
            let mut v: serde_json::Value = serde_json::from_str(&f.1).map_err(|e| e.to_string())?;
            let u: serde_json::Value = serde_json::from_str(&self.text).map_err(|e| format!("unit JSON: {e}"))?;
            let list = v["units"].as_array_mut().ok_or("the faction file has no units")?;
            match list.get_mut(self.unit) {
                Some(slot) => *slot = u,
                None => list.push(u),
            }
            f.1 = serde_json::to_string_pretty(&v).map_err(|e| e.to_string())?;
        }
        Ok(pack)
    }
    /// Checks the edited data the way the game loads it, and writes the preset if it is sound.
    fn save(&mut self) {
        let done = self.edited().and_then(|pack| {
            Defs::from_pack(pack.clone())?;
            pack.save(&asset_root())?;
            Ok(pack)
        });
        match done {
            Ok(pack) => {
                self.msg = format!("Saved preset '{}'.", pack.name);
                self.pack = Some(pack);
                self.show();
            }
            Err(e) => self.msg = format!("Not saved: {e}"),
        }
    }
    /// Adds a copy of the open unit behind it, or removes the open unit (kept in memory until saved).
    fn change_units(&mut self, copy: bool) {
        let Ok(mut pack) = self.edited() else {
            self.msg = "Fix the JSON in the text box first.".into();
            return;
        };
        let Some(f) = pack.factions.get_mut(self.file.wrapping_sub(1)) else { return };
        let mut v: serde_json::Value = serde_json::from_str(&f.1).unwrap_or_default();
        let Some(list) = v["units"].as_array_mut() else { return };
        if self.unit >= list.len() {
            return;
        }
        if copy {
            let mut u = list[self.unit].clone();
            u["id"] = format!("{}_copy", u["id"].as_str().unwrap_or("unit")).into();
            u["name"] = format!("{} copy", u["name"].as_str().unwrap_or("Unit")).into();
            list.insert(self.unit + 1, u);
            self.unit += 1;
            self.msg = "Copied. Give the copy its own id and icon (shape, glyphs or tier), then save.".into();
        } else {
            list.remove(self.unit);
            self.msg = "Removed (not saved yet). Saving fails if something still refers to the unit.".into();
        }
        f.1 = serde_json::to_string_pretty(&v).unwrap_or_default();
        self.pack = Some(pack);
        self.show();
    }
}

enum Screen {
    Menu,
    Presets(PresetUi),
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
    settings_open: bool,
    /// The built-in game data; a lobby or a host may put a preset in its place for one game.
    base_defs: Defs,
}

/// Back to the built-in game data (after a game or lobby that used a preset or the host's data).
fn restore_defs(world: &mut World, base: &Defs) {
    if world.resource::<Defs>().hash != base.hash {
        world.insert_resource(base.clone());
    }
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
    let mut slots: Vec<Slot> = (0..4).map(|k| Slot { kind: 0, name: String::new(), team: k, faction, conn: 0, hash: 0 }).collect();
    slots[0].kind = 2;
    slots[0].name = name.to_string();
    slots[2].kind = 3;
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(1) | 1;
    let maps = MapData::list(&asset_root());
    let thumbs = vec![None; maps.len()];
    let counts = maps.iter().map(|m| MapData::start_count(&asset_root().join("maps").join(m))).collect();
    let mut l = HostLobby {
        host,
        slots,
        map_choice: 1,
        maps,
        players: 4,
        dirty: true,
        lan_ip: local_ip(),
        mode: MODE_ANNIHILATION,
        cp_start: 0,
        preset: 0,
        presets: DataPack::list(&asset_root()),
        seed,
        map: None,
        preview: vec![],
        starts: vec![],
        tex: None,
        thumbs,
        tab: 0,
        ai_reclaim: true,
        counts,
    };
    let _ = l.ensure_map(defs);
    Ok(l)
}

fn start_game(world: &mut World, mut l: HostLobby) -> Result<Session, String> {
    // The map the lobby showed (made again if the choice changed since).
    let defs = world.resource::<Defs>().clone();
    l.ensure_map(&defs)?;
    let map = l.map.take().map(|m| m.1).ok_or("no map")?;
    let seed = if l.map_choice < SIZES.len() { l.seed } else { std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(1) | 1 };
    let mut setup = vec![];
    let mut remote = vec![];
    let mut hashes = vec![];
    let mut ais = vec![];
    for (k, s) in l.slots.iter().enumerate() {
        if s.kind < 2 || k >= map.starts.len() {
            continue;
        }
        let idx = setup.len() as u8;
        // LR_WATCH=1 lets an AI play the local slot too (spectating / testing).
        if s.kind == 3 || (s.conn == 0 && std::env::var("LR_WATCH").is_ok()) {
            let mut ai = Ai::new(idx, seed);
            ai.reclaim = l.ai_reclaim;
            ais.push(ai);
        } else if s.conn != 0 {
            remote.push((s.conn, idx));
            hashes.push((s.conn, s.hash));
        }
        let name = if s.kind == 3 { format!("AI {}", k + 1) } else { s.name.clone() };
        setup.push(PlayerSetup { name, team: s.team, is_ai: s.kind == 3, slot: k, faction: s.faction });
    }
    let sent_map = map.clone();
    setup_game(world, map, &setup);
    if l.mode == MODE_POINTS {
        let start = l.cp_points(world.resource::<Defs>());
        enable_control_points(world, start);
    }
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
    // Clients get the map and the complete starting state; from then on only commands travel.
    if !remote.is_empty() {
        let state = frame(TAG_SNAP, bincode::serialize(&make_snapshot(world)).map_err(|e| e.to_string())?);
        // Players whose game data differs from ours (a preset, another version) get ours,
        // together with the model and picture files the standard game does not have.
        let defs = world.resource::<Defs>().clone();
        let mut pack: Option<DataPack> = None;
        for (conn, idx) in &remote {
            let differs = hashes.iter().any(|h| h.0 == *conn && h.1 != defs.hash);
            if differs && pack.is_none() {
                let mut p = (*defs.pack).clone();
                if let Ok(standard) = Defs::load(&asset_root()) {
                    p.attach_assets(&asset_root(), &defs, &standard);
                }
                pack = Some(p);
            }
            l.host.send_msg(*conn, &S2C::Start { map: sent_map.clone(), you: *idx, data: if differs { pack.clone() } else { None } });
            l.host.send(*conn, &state);
        }
    }
    let sim = HostSim::new(world);
    *l.host.info.lock().unwrap() = "(game in progress)".into();
    view::start_view(world, 0, true);
    Ok(Session { role: Role::Host { host: l.host, remote, sim }, acc: 0.0, paused: None })
}

fn leave_game(world: &mut World) {
    view::stop_view(world);
    clear_game(world);
    world.remove_resource::<Ais>();
    world.remove_resource::<TurnLog>();
}

/// Runs the simulation once per frame before rendering: the host at its own pace, a client
/// predicting ahead of the turns the host confirms (`netplay.rs`).
fn session_update(world: &mut World) {
    let Some(mut app) = world.remove_non_send::<AppState>() else { return };
    let mut over: Option<String> = None;
    if let Screen::Game(s) = &mut app.screen {
        let me = world.get_resource::<ViewState>().map_or(0, |v| v.me);
        let local: Vec<Command> = world.get_resource_mut::<LocalCommands>().map(|mut c| c.0.drain(..).collect()).unwrap_or_default();
        let toggle = world.get_resource_mut::<ViewState>().map_or(false, |mut v| std::mem::take(&mut v.pause_toggle));
        match &mut s.role {
            Role::Host { host, remote, sim } => {
                let mut cmds: Vec<(u8, Command)> = local.into_iter().map(|c| (me, c)).collect();
                // Pause requests: (player, pause or resume).
                let mut pause: Vec<(u8, bool)> = vec![];
                if toggle {
                    pause.push((me, s.paused.is_none()));
                }
                while let Ok(ev) = host.events.try_recv() {
                    match ev {
                        HostEvent::Msg(id, C2S::Cmd { tick, seq, cmd }) => {
                            if let Some((_, p)) = remote.iter().find(|r| r.0 == id) {
                                sim.command(world, *p, tick, seq, cmd);
                            }
                        }
                        HostEvent::Msg(id, C2S::Ping(t)) => host.send_msg(id, &S2C::Pong(t)),
                        HostEvent::Msg(id, C2S::Pause(on)) => {
                            if let Some((_, p)) = remote.iter().find(|r| r.0 == id) {
                                pause.push((*p, on));
                            }
                        }
                        HostEvent::Msg(id, C2S::Hash { tick, hash }) => {
                            if let Some((_, p)) = remote.iter().find(|r| r.0 == id) {
                                if !sim.matches(tick, hash) {
                                    // That machine computed something else: hand it our state.
                                    warn!("player {p} diverged at tick {tick}; sending the full state");
                                    if let Ok(data) = bincode::serialize(&make_snapshot(world)) {
                                        host.send(id, &frame(TAG_SNAP, data));
                                    }
                                }
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
                // Any player may pause or resume the whole game. Clients hear of it after the last
                // turn simulated before it, so they stop on exactly the same tick.
                for (p, on) in pause {
                    let now = if on { s.paused.or(Some(p)) } else { None };
                    if now != s.paused {
                        s.paused = now;
                        for (conn, _) in remote.iter() {
                            host.send_msg(*conn, &S2C::Paused(now));
                        }
                    }
                }
                if let Some(mut v) = world.get_resource_mut::<ViewState>() {
                    v.paused = s.paused;
                }
                // LR_SPEED=4 fast-forwards a hosted game (testing, watching AIs).
                let speed = std::env::var("LR_SPEED").ok().and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0).clamp(0.1, 20.0);
                let dt = world.resource::<Time>().delta_secs().min(0.25);
                s.acc += dt * speed;
                if s.paused.is_some() {
                    s.acc = 0.0;
                }
                let cap = (3.0 * speed).ceil() as i32;
                let mut n = 0;
                while s.acc >= DT && n < cap {
                    s.acc -= DT;
                    n += 1;
                    let turn = sim.tick(world, std::mem::take(&mut cmds), !remote.is_empty());
                    if !remote.is_empty() {
                        if let Ok(data) = bincode::serialize(&turn) {
                            let f = frame(TAG_MSG, data);
                            for (conn, _) in remote.iter() {
                                host.send(*conn, &f);
                            }
                        }
                    }
                }
                if n == cap {
                    s.acc = 0.0;
                }
                // Commands given while no tick was due wait for the next frame.
                if !cmds.is_empty() {
                    if let Some(mut lc) = world.get_resource_mut::<LocalCommands>() {
                        lc.0.extend(cmds.into_iter().map(|c| c.1));
                    }
                }
            }
            Role::Client { client, sim } => {
                if toggle {
                    client.send(&C2S::Pause(sim.paused.is_none()));
                }
                for m in sim.issue(world, local) {
                    client.send(&m);
                }
                loop {
                    match client.frames.try_recv() {
                        Ok(Some((tag, data))) => {
                            if let Err(why) = sim.receive(tag, &data) {
                                over = Some(why);
                                break;
                            }
                        }
                        Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            over = Some("Connection to the host was lost.".into());
                            break;
                        }
                        Err(_) => break,
                    }
                }
                if over.is_none() {
                    let dt = world.resource::<Time>().delta_secs().min(0.25);
                    match sim.update(world, dt) {
                        Ok(out) => {
                            for m in out {
                                client.send(&m);
                            }
                        }
                        Err(why) => over = Some(why),
                    }
                }
                if let Some(mut v) = world.get_resource_mut::<ViewState>() {
                    v.paused = sim.paused;
                }
            }
        }
        if world.get_resource::<ViewState>().map_or(false, |v| v.quit) {
            over = Some(String::new());
        }
    }
    if let Some(msg) = over {
        leave_game(world);
        restore_defs(world, &app.base_defs);
        app.message = msg;
        app.screen = Screen::Menu;
        app.discovery = Discovery::new();
    }
    world.insert_non_send(app);
}

fn slot_rows(ui: &mut egui::Ui, defs: &Defs, slots: &[SlotInfo], you: i8, editable: bool, width: f32) -> Option<(usize, u8)> {
    let mut clicked = None;
    // "Start N", then the player, team and faction buttons share the column width.
    let team_w = 90.0;
    let faction_w = (width * 0.24).max(130.0);
    let name_w = (width - team_w - faction_w - 100.0).max(200.0);
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
            if ui.add_sized([name_w, 30.0], egui::Button::new(label)).clicked() && editable {
                clicked = Some((k, 0));
            }
            if ui.add_sized([team_w, 30.0], egui::Button::new(format!("Team {}", s.team + 1))).clicked() && editable {
                clicked = Some((k, 1));
            }
            let fname = defs.factions.get(s.faction as usize).map_or("?", |f| f.name.as_str());
            if ui.add_sized([faction_w, 30.0], egui::Button::new(fname)).clicked() && (editable || k as i8 == you) {
                clicked = Some((k, 2));
            }
            ui.end_row();
        }
    });
    clicked
}

/// The lobby's map picture with numbered start positions in the players' colours.
fn map_picture(ui: &mut egui::Ui, tex: Option<&egui::TextureHandle>, starts: &[(f32, f32)], defs: &Defs, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let p = ui.painter_at(rect);
    match tex {
        Some(t) => {
            p.image(t.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        None => {
            p.rect_filled(rect, 4.0, egui::Color32::from_rgb(30, 34, 40));
            p.text(rect.center(), egui::Align2::CENTER_CENTER, "no preview", egui::FontId::proportional(16.0), egui::Color32::GRAY);
        }
    }
    p.rect_stroke(rect.shrink(0.5), 0.0, egui::Stroke::new(1.0, egui::Color32::from_gray(90)), egui::StrokeKind::Inside);
    // Start markers grow with the picture.
    let r = (size / 35.0).clamp(9.0, 16.0);
    for (k, (x, y)) in starts.iter().enumerate() {
        let c = defs.rules.player_colors[k % defs.rules.player_colors.len()];
        let at = rect.min + egui::vec2(x * size, y * size);
        p.circle_filled(at, r, egui::Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8));
        p.circle_stroke(at, r, egui::Stroke::new(1.5, egui::Color32::BLACK));
        p.text(at, egui::Align2::CENTER_CENTER, format!("{}", k + 1), egui::FontId::proportional(r * 1.35), egui::Color32::BLACK);
    }
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
    // The lobby fills the screen like the preset editor: the players column and the map column share the
    // width, the map picture (kept small) grows with the height and the map list takes what is left under it.
    let screen = ctx.content_rect();
    let lobby_w = (screen.width() - 48.0).max(960.0);
    let lobby_h = (screen.height() - 120.0).max(560.0);
    let left_w = ((lobby_w - 36.0) * 0.52).floor();
    let right_w = lobby_w - 36.0 - left_w;
    let pic = ((right_w - 20.0).min(lobby_h * 0.6) * 0.45).max(160.0);
    let list_h = (lobby_h - pic - 100.0).max(300.0);
    egui::Area::new(egui::Id::new("menu")).anchor(egui::Align2::CENTER_TOP, [0.0, 20.0]).show(&ctx, |ui| {
        ui.vertical_centered(|ui| {
            let lobby = matches!(app.screen, Screen::HostLobby(_) | Screen::ClientLobby(_));
            ui.set_max_width(if lobby { lobby_w } else { 560.0 });
            match &mut app.screen {
                Screen::Menu => {
                    app.discovery.poll(dt);
                    ui.add_space(30.0);
                    ui.label(egui::RichText::new("LONG RANGE").size(56.0).color(egui::Color32::from_rgb(255, 215, 100)).strong());
                    ui.label(egui::RichText::new("flow economy  -  long guns  -  scouts win wars").size(16.0));
                    ui.add_space(26.0);
                    ui.horizontal(|ui| {
                        ui.label("Name");
                        if ui.add(egui::TextEdit::singleline(&mut app.name).desired_width(300.0).char_limit(16)).changed() {
                            // Remembered for the next start.
                            let mut s = world.resource_mut::<longrange::settings::Settings>();
                            s.name = app.name.clone();
                            s.save();
                        }
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
                                next = Some(Screen::ClientLobby(ClientLobby { client, info: None, pending: None, tex: None }));
                                app.message.clear();
                            }
                            Err(e) => app.message = e,
                        }
                    }
                    ui.add_space(14.0);
                    if ui.add_sized([440.0, 34.0], egui::Button::new("Game data presets")).clicked() {
                        next = Some(Screen::Presets(PresetUi::new()));
                        app.message.clear();
                    }
                    ui.add_space(6.0);
                    if ui.add_sized([440.0, 34.0], egui::Button::new("Settings")).clicked() {
                        app.settings_open = true;
                    }
                    ui.add_space(6.0);
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
                                // Different game data is fine: that player gets ours when the game starts.
                                if let Some(s) = l.slots.iter_mut().find(|s| s.kind == 0) {
                                    s.kind = 2;
                                    s.name = name.chars().take(16).collect();
                                    s.conn = id;
                                    s.hash = data_hash;
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
                    let cur = world.resource::<Defs>().clone();
                    if let Err(e) = l.ensure_map(&cur) {
                        app.message = e;
                    }
                    let first_faction = playable.first().copied().unwrap_or(0) as u8;
                    let (mut start, mut back) = (false, false);
                    ui.horizontal_top(|ui| {
                        // Left: the players.
                        ui.vertical(|ui| {
                            ui.set_width(left_w);
                            if let Some((k, col)) = slot_rows(ui, &cur, &l.info(0, &cur).slots, 0, true, left_w) {
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
                            if !app.message.is_empty() {
                                ui.colored_label(egui::Color32::from_rgb(255, 120, 105), &app.message);
                            }
                            ui.add_space(14.0);
                            start = ui.add_sized([left_w - 20.0, 50.0], egui::Button::new(egui::RichText::new("START").size(22.0))).clicked();
                            back = ui.add_sized([left_w - 20.0, 34.0], egui::Button::new("Back")).clicked();
                        });
                        ui.add_space(20.0);
                        // Right: the map and the tabs with the game settings.
                        ui.vertical(|ui| {
                            ui.set_width(right_w);
                            if l.tex.is_none() && l.preview.len() == PREVIEW * PREVIEW * 4 {
                                l.tex = Some(ctx.load_texture("lobby_map", egui::ColorImage::from_rgba_unmultiplied([PREVIEW, PREVIEW], &l.preview), egui::TextureOptions::LINEAR));
                            }
                            map_picture(ui, l.tex.as_ref(), &l.starts, &cur, pic);
                            ui.label(egui::RichText::new(l.map_label()).strong());
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut l.tab, 0, egui::RichText::new("Game options").size(16.0));
                                ui.selectable_value(&mut l.tab, 1, egui::RichText::new("Map").size(16.0));
                            });
                            ui.separator();
                            if l.tab == 0 {
                                if ui.add_sized([right_w - 20.0, 30.0], egui::Button::new(format!("Mode: {}", l.mode_label()))).clicked() {
                                    l.mode = if l.mode == MODE_POINTS { MODE_ANNIHILATION } else { MODE_POINTS };
                                    l.dirty = true;
                                }
                                if l.mode == MODE_POINTS {
                                    // Left click: next choice, right click: previous. 0 = the game data's value.
                                    const CP_STARTS: [u32; 8] = [0, 100, 200, 300, 400, 600, 800, 1200];
                                    let label = format!("Starting points: {:.0}{}", l.cp_points(&cur), if l.cp_start == 0 { " (game data default)" } else { "" });
                                    let r = ui.add_sized([right_w - 20.0, 30.0], egui::Button::new(label));
                                    let at = CP_STARTS.iter().position(|&n| n == l.cp_start).unwrap_or(0);
                                    if r.clicked() {
                                        l.cp_start = CP_STARTS[(at + 1) % CP_STARTS.len()];
                                        l.dirty = true;
                                    } else if r.secondary_clicked() {
                                        l.cp_start = CP_STARTS[(at + CP_STARTS.len() - 1) % CP_STARTS.len()];
                                        l.dirty = true;
                                    }
                                }
                                // Game data: the built-in values or one of the presets (main menu -> Game data presets).
                                if ui.add_sized([right_w - 20.0, 30.0], egui::Button::new(format!("Game data: {}", l.preset_label()))).clicked() && !l.presets.is_empty() {
                                    let k = (l.preset + 1) % (l.presets.len() + 1);
                                    let name = if k == 0 { String::new() } else { l.presets[k - 1].clone() };
                                    match Defs::load_preset(&asset_root(), &name) {
                                        Ok(d) => {
                                            l.preset = k;
                                            let f = d.playable_factions().first().copied().unwrap_or(0) as u8;
                                            for s in l.slots.iter_mut() {
                                                s.faction = f;
                                            }
                                            world.insert_resource(d);
                                            l.dirty = true;
                                            app.message.clear();
                                        }
                                        Err(e) => app.message = format!("Preset '{name}' cannot be used: {e}"),
                                    }
                                }
                                if l.map_choice < SIZES.len() {
                                    ui.horizontal(|ui| {
                                        if ui.add_sized([(right_w - 28.0) / 2.0, 30.0], egui::Button::new(format!("Players: {}", l.players))).clicked() {
                                            l.players = if l.players >= MAX_PLAYERS { 2 } else { l.players + 1 };
                                            l.resize_slots(first_faction);
                                        }
                                        if ui.add_sized([(right_w - 28.0) / 2.0, 30.0], egui::Button::new("New random map")).clicked() {
                                            l.seed = l.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1;
                                        }
                                    });
                                }
                                if ui.checkbox(&mut l.ai_reclaim, "AI reclaims trees and rocks").changed() {
                                    l.dirty = true;
                                }
                            } else {
                                // One thumbnail per frame, so opening the tab does not stall.
                                if let Some(k) = l.thumbs.iter().position(|t| t.is_none()) {
                                    let px = MapData::load(&asset_root().join("maps").join(&l.maps[k])).map(|m| view::terrain::map_preview(&m, &cur, 64)).unwrap_or_else(|_| vec![60; 64 * 64 * 4]);
                                    l.thumbs[k] = Some(ctx.load_texture(format!("thumb_{k}"), egui::ColorImage::from_rgba_unmultiplied([64, 64], &px), egui::TextureOptions::LINEAR));
                                }
                                let mut pick = None;
                                egui::ScrollArea::vertical().max_height(list_h).min_scrolled_height(list_h).show(ui, |ui| {
                                    for c in 0..SIZES.len() + l.maps.len() {
                                        ui.horizontal(|ui| {
                                            let (rect, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
                                            match c.checked_sub(SIZES.len()).and_then(|k| l.thumbs.get(k)).and_then(|t| t.as_ref()) {
                                                Some(t) => {
                                                    ui.painter().image(t.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
                                                }
                                                None => {
                                                    ui.painter().rect_filled(rect, 3.0, egui::Color32::from_rgb(40, 46, 54));
                                                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, "?", egui::FontId::proportional(22.0), egui::Color32::GRAY);
                                                }
                                            }
                                            let label = match SIZES.get(c) {
                                                Some((n, name)) => format!("Random, {name} ({0} x {0})", *n as f32 * CELL),
                                                None => {
                                                    let k = c - SIZES.len();
                                                    let starts = l.counts.get(k).copied().unwrap_or(0);
                                                    format!("{}  ({starts} players)", l.maps[k])
                                                }
                                            };
                                            if ui.add_sized([right_w - 80.0, 48.0], egui::Button::selectable(l.map_choice == c, label)).clicked() {
                                                pick = Some(c);
                                            }
                                        });
                                    }
                                });
                                if let Some(c) = pick {
                                    if c != l.map_choice {
                                        l.map_choice = c;
                                        l.resize_slots(first_faction);
                                    }
                                }
                            }
                        });
                    });
                    if l.dirty {
                        l.dirty = false;
                        for (k, s) in l.slots.iter().enumerate() {
                            if s.conn != 0 {
                                l.host.send_msg(s.conn, &S2C::Lobby(l.info(k as i8, world.resource::<Defs>())));
                            }
                        }
                    }
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
                    let mut started: Option<ClientSim> = None;
                    loop {
                        match l.client.frames.try_recv() {
                            Ok(Some((TAG_MSG, data))) => match bincode::deserialize::<S2C>(&data) {
                                Ok(S2C::Lobby(i)) => l.info = Some(i),
                                Ok(S2C::Start { map, you, data }) => {
                                    // The host plays with other game data than ours: use the host's for this game.
                                    if let Some(pack) = data {
                                        match Defs::from_pack(pack.clone()) {
                                            Ok(d) => {
                                                pack.store_assets(&asset_root());
                                                world.insert_resource(d);
                                            }
                                            Err(e) => lost = Some(format!("The host's game data could not be used: {e}")),
                                        }
                                    }
                                    l.pending = Some((map, you));
                                }
                                Ok(S2C::Kicked(why)) => lost = Some(why),
                                Ok(_) => {}
                                Err(_) => lost = Some("Protocol mismatch - are both players on the same version?".into()),
                            },
                            Ok(Some((_, data))) => {
                                if let (Some((map, you)), Ok(snap)) = (l.pending.take(), bincode::deserialize::<Snapshot>(&data)) {
                                    join_game(world, map, &snap);
                                    view::start_view(world, you, true);
                                    started = Some(ClientSim::new(you, snap));
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
                    let mut back = false;
                    match l.info.clone() {
                        Some(i) => {
                            let side = i.preview_side as usize;
                            let sum = i.preview.iter().step_by(97).fold(i.preview.len() as u64, |h, b| h.wrapping_mul(31).wrapping_add(*b as u64));
                            if l.tex.as_ref().map_or(true, |t| t.0 != sum) && side > 0 && i.preview.len() == side * side * 4 {
                                l.tex = Some((sum, ctx.load_texture("lobby_map", egui::ColorImage::from_rgba_unmultiplied([side, side], &i.preview), egui::TextureOptions::LINEAR)));
                            }
                            ui.horizontal_top(|ui| {
                                ui.vertical(|ui| {
                                    ui.set_width(left_w);
                                    if let Some((k, 2)) = slot_rows(ui, &defs, &i.slots, i.you, false, left_w) {
                                        l.client.send(&C2S::Faction(next_faction(i.slots[k].faction)));
                                    }
                                    ui.add_space(8.0);
                                    ui.label("Waiting for the host to start...");
                                    ui.add_space(14.0);
                                    back = ui.add_sized([left_w - 20.0, 34.0], egui::Button::new("Leave")).clicked();
                                });
                                ui.add_space(20.0);
                                ui.vertical(|ui| {
                                    ui.set_width(right_w);
                                    map_picture(ui, l.tex.as_ref().map(|t| &t.1), &i.starts, &defs, pic);
                                    ui.label(egui::RichText::new(&i.map_name).strong());
                                    ui.add_space(6.0);
                                    ui.label(egui::RichText::new("Game options").size(16.0));
                                    ui.separator();
                                    ui.label(&i.extra);
                                });
                            });
                        }
                        None => {
                            ui.label("Connecting...");
                            ui.add_space(10.0);
                            back = ui.add_sized([left_w - 20.0, 34.0], egui::Button::new("Leave")).clicked();
                        }
                    }
                    if let Some(sim) = started {
                        if let Screen::ClientLobby(l) = std::mem::replace(&mut app.screen, Screen::Menu) {
                            next = Some(Screen::Game(Session { role: Role::Client { client: l.client, sim }, acc: 0.0, paused: None }));
                        }
                    } else if let Some(why) = lost {
                        app.message = why;
                        next = Some(Screen::Menu);
                    } else if back {
                        next = Some(Screen::Menu);
                    }
                }
                Screen::Game(_) | Screen::Presets(_) => {}
            }
        });
    });
    if let Screen::Presets(p) = &mut app.screen {
        if presets_ui(&ctx, p) {
            next = Some(Screen::Menu);
        }
    }
    if app.settings_open && next.is_none() {
        use longrange::settings::{settings_ui, RenderErrors, Settings, StartSettings};
        let mut s = world.resource::<Settings>().clone();
        let started = world.resource::<StartSettings>().0.clone();
        let errors = world.get_resource::<RenderErrors>().map(|e| e.0.clone()).unwrap_or_default();
        let adapter = longrange::settings::adapter_line(world);
        let mut close = false;
        egui::Window::new("settings").title_bar(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(&ctx, |ui| {
            close = settings_ui(ui, &mut s, &started, &adapter, &errors, false);
        });
        if s != *world.resource::<Settings>() {
            world.insert_resource(s);
        }
        if close {
            app.settings_open = false;
        }
    }
    if let Some(n) = next {
        if matches!(n, Screen::Menu) {
            restore_defs(world, &app.base_defs);
        }
        app.screen = n;
    }
    world.insert_non_send(app);
}

/// The preset editor. Returns true when the player goes back to the main menu.
fn presets_ui(ctx: &egui::Context, p: &mut PresetUi) -> bool {
    let mut back = false;
    let open_name = p.pack.as_ref().map(|k| k.name.clone());
    // Fill the screen: the text column takes whatever width is left, the lists the height.
    let screen = ctx.content_rect();
    let total_w = (screen.width() - 48.0).max(1040.0);
    let body_h = (screen.height() - 150.0).max(400.0);
    let list_w = 260.0;
    let text_w = total_w - 230.0 - list_w - 2.0 * 8.0;
    egui::Area::new(egui::Id::new("presets")).anchor(egui::Align2::CENTER_TOP, [0.0, 16.0]).show(ctx, |ui| {
        ui.set_width(total_w);
        ui.label(egui::RichText::new("GAME DATA PRESETS").size(30.0).color(egui::Color32::from_rgb(255, 215, 100)));
        ui.label("A preset is a named copy of the game data (unit stats, costs, rules) that you can change. The host picks one in the lobby; the other players receive it when the game starts.");
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            // --- the presets
            ui.vertical(|ui| {
                ui.set_width(230.0);
                ui.label(egui::RichText::new("Presets").strong());
                let mut open: Option<String> = None;
                for n in &p.names {
                    if ui.add_sized([220.0, 26.0], egui::Button::selectable(open_name.as_deref() == Some(n.as_str()), n.as_str())).clicked() {
                        open = Some(n.clone());
                    }
                }
                if p.names.is_empty() {
                    ui.label(egui::RichText::new("none yet").weak());
                }
                if let Some(n) = open {
                    p.open(&n);
                }
                ui.add_space(10.0);
                ui.label("New preset:");
                ui.add(egui::TextEdit::singleline(&mut p.new_name).desired_width(214.0).char_limit(32).hint_text("name"));
                let from = open_name.clone().unwrap_or_default();
                if ui.add_sized([220.0, 26.0], egui::Button::new(if from.is_empty() { "Create from the built-in data".to_string() } else { format!("Create as a copy of '{from}'") })).clicked() {
                    let name = longrange::data::clean_name(&p.new_name);
                    let made = if name.is_empty() {
                        Err("Type a name first (letters, digits, spaces, - and _).".to_string())
                    } else if p.names.contains(&name) {
                        Err(format!("There already is a preset '{name}'."))
                    } else {
                        // Unsaved edits of the open preset are part of the copy.
                        p.edited().or_else(|_| DataPack::preset(&asset_root(), &from)).and_then(|mut pack| {
                            pack.name = name.clone();
                            pack.save(&asset_root())
                        })
                    };
                    match made {
                        Ok(()) => {
                            p.names = DataPack::list(&asset_root());
                            p.new_name.clear();
                            p.open(&name);
                            p.msg = format!("Created '{name}'.");
                        }
                        Err(e) => p.msg = e,
                    }
                }
                if let Some(n) = open_name.clone() {
                    ui.add_space(10.0);
                    if ui.add_sized([220.0, 26.0], egui::Button::new(if p.confirm_delete { format!("Really delete '{n}'?") } else { format!("Delete '{n}'") })).clicked() {
                        if p.confirm_delete {
                            p.msg = match DataPack::delete(&asset_root(), &n) {
                                Ok(()) => format!("Deleted '{n}'."),
                                Err(e) => e,
                            };
                            p.names = DataPack::list(&asset_root());
                            p.pack = None;
                            p.confirm_delete = false;
                        } else {
                            p.confirm_delete = true;
                        }
                    }
                }
                ui.add_space(14.0);
                if ui.add_sized([220.0, 32.0], egui::Button::new("Back")).clicked() {
                    back = true;
                }
            });
            let Some(pack) = p.pack.clone() else {
                ui.label("Pick a preset on the left, or create one.");
                return;
            };
            // --- its files and units
            ui.vertical(|ui| {
                ui.set_width(list_w);
                ui.label(egui::RichText::new("Files").strong());
                let mut pick: Option<usize> = None;
                if ui.add_sized([list_w - 10.0, 24.0], egui::Button::selectable(p.file == 0, "rules.json")).clicked() {
                    pick = Some(0);
                }
                for (k, f) in pack.factions.iter().enumerate() {
                    if ui.add_sized([list_w - 10.0, 24.0], egui::Button::selectable(p.file == k + 1, f.0.as_str())).clicked() {
                        pick = Some(k + 1);
                    }
                }
                let mut pick_unit: Option<usize> = None;
                if p.file > 0 {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Units").strong());
                    let units_h = (body_h - 60.0 - 28.0 * (pack.factions.len() + 1) as f32).max(150.0);
                    egui::ScrollArea::vertical()
                        .id_salt("preset_units")
                        .auto_shrink([false, false])
                        .min_scrolled_height(units_h)
                        .max_height(units_h)
                        .show(ui, |ui| {
                        for (k, (id, name)) in p.units.iter().enumerate() {
                            let label = if name.is_empty() { id.clone() } else { format!("{name}  ({id})") };
                            if ui.add_sized([list_w - 22.0, 22.0], egui::Button::selectable(p.unit == k, label)).clicked() {
                                pick_unit = Some(k);
                            }
                        }
                    });
                }
                // Switching keeps what was typed, as long as it is valid JSON.
                if pick.is_some() || pick_unit.is_some() {
                    match p.edited() {
                        Ok(e) => {
                            p.pack = Some(e);
                            p.file = pick.unwrap_or(p.file);
                            p.unit = pick_unit.unwrap_or(if pick.is_some() { 0 } else { p.unit });
                            p.msg.clear();
                            p.show();
                        }
                        Err(e) => p.msg = format!("{e}  (fix it before switching)"),
                    }
                }
            });
            // --- the text
            ui.vertical(|ui| {
                ui.set_width(text_w);
                ui.horizontal(|ui| {
                    if ui.add_sized([150.0, 28.0], egui::Button::new("Check and save")).clicked() {
                        p.save();
                    }
                    if p.file > 0 {
                        if ui.add_sized([110.0, 28.0], egui::Button::new("Copy unit")).clicked() {
                            p.change_units(true);
                        }
                        if ui.add_sized([110.0, 28.0], egui::Button::new("Remove unit")).clicked() {
                            p.change_units(false);
                        }
                    }
                });
                if !p.msg.is_empty() {
                    ui.colored_label(egui::Color32::from_rgb(255, 210, 100), &p.msg);
                }
                let text_h = body_h - 40.0 - if p.msg.is_empty() { 0.0 } else { 20.0 };
                egui::ScrollArea::vertical()
                    .id_salt("preset_text")
                    .auto_shrink([false, false])
                    .min_scrolled_height(text_h)
                    .max_height(text_h)
                    .show(ui, |ui| {
                        ui.add(egui::TextEdit::multiline(&mut p.text).code_editor().desired_width(text_w - 14.0).min_size(egui::vec2(text_w - 14.0, text_h - 4.0)));
                    });
            });
        });
    });
    back
}

fn setup(mut commands: Commands) {
    commands.spawn((
        Camera3d::default(),
        Msaa::Off,
        Transform::from_xyz(0.0, 50.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y),
        DistanceFog { color: Color::srgb(0.55, 0.62, 0.7), falloff: FogFalloff::Linear { start: 900.0, end: 4000.0 }, ..default() },
        MainCamera,
    ));
}

/// Developer aid: `LR_SHOT=<file.png>,<game seconds>[,<unit id>[,<camera distance>]]` saves one
/// screenshot of a running game and quits. With a unit id ("land_factory"), the local player's
/// units of that type are selected first, so the selection panel is in the picture.
fn dev_screenshot(mut commands: Commands, game: Option<Res<Game>>, view: Option<ResMut<ViewState>>, defs: Res<Defs>, units: Query<&Unit>, time: Res<Time>, mut state: Local<(u8, f32)>, mut exit: MessageWriter<AppExit>) {
    static SPEC: std::sync::OnceLock<Option<(String, f32, String, f32)>> = std::sync::OnceLock::new();
    let spec = SPEC.get_or_init(|| {
        let v = std::env::var("LR_SHOT").ok()?;
        let mut it = v.split(',');
        Some((it.next()?.to_string(), it.next().and_then(|s| s.parse().ok()).unwrap_or(5.0), it.next().unwrap_or("").to_string(), it.next().and_then(|s| s.parse().ok()).unwrap_or(0.0)))
    });
    let (Some((path, at, select, dist)), Some(game), Some(mut view)) = (spec, game, view) else { return };
    match state.0 {
        0 if game.tick as f32 * DT >= *at => {
            if let Some(d) = defs.id(select) {
                view.sel = units.iter().filter(|u| u.owner == view.me && u.def == d).map(|u| u.id).collect();
            }
            if *dist > 0.0 {
                view.cam_dist = *dist;
            }
            *state = (1, time.elapsed_secs());
        }
        1 if time.elapsed_secs() > state.1 + 0.5 => {
            commands.spawn(bevy::render::view::screenshot::Screenshot::primary_window()).observe(bevy::render::view::screenshot::save_to_disk(path.clone()));
            *state = (2, time.elapsed_secs());
        }
        2 if time.elapsed_secs() > state.1 + 1.5 => {
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

fn fullscreen_toggle(keys: Res<ButtonInput<KeyCode>>, mut settings: ResMut<longrange::settings::Settings>) {
    if keys.just_pressed(KeyCode::F11) {
        settings.fullscreen = !settings.fullscreen;
        settings.save();
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
    // LR_SEED=n plays another map and AI seed (to tell an effect from the luck of one game).
    let seed = std::env::var("LR_SEED").ok().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let mut map = MapData::generate(if n > 4 { 256 } else { 192 }, 12345 + seed, n);
    map.scatter_features(&defs.rules.features, 12345 + seed, 1.0);
    println!("map: {} trees and rocks", map.features.len());
    let faction = defs.playable_factions()[0] as u8;
    let setup: Vec<PlayerSetup> = (0..n).map(|i| PlayerSetup { name: format!("AI {i}"), team: i as u8, is_ai: true, slot: i, faction }).collect();
    let world = app.world_mut();
    setup_game(world, map, &setup);
    world.insert_resource(Ais((0..n).map(|i| Ai::new(i as u8, 99 + seed)).collect()));
    let t0 = std::time::Instant::now();
    for t in 0..36000u32 {
        world.run_schedule(SimTick);
        let game = world.resource::<Game>().clone();
        if t % 3000 == 0 || game.winner.is_some() {
            let snap = make_snapshot(world);
            let bytes = bincode::serialize(&snap).map(|b| b.len()).unwrap_or(0);
            println!("t={:5}s units={} projectiles={} wrecks={} features touched={} state={}B hash={:016x} wall={:?}", t / 10, snap.units.len(), snap.projectiles.len(), snap.wrecks.len(), snap.features.len(), bytes, state_hash(&snap), t0.elapsed());
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
    // 5. Firing at radar contacts: every armed land unit that out-ranges its own sight gets a radar
    //    tower next to it and an enemy building just inside its range, on flat ground.
    println!("== Shooting at radar contacts beyond vision (flat ground, radar tower alongside)");
    let names: Vec<String> = defs.units.iter().filter(|d| d.armed() && !d.is_structure() && !d.is_air() && d.weapons[0].hits_land && d.max_range() > d.vision + 8.0).map(|d| d.id.clone()).collect();
    for name in names {
        let d = defs.get(defs.id(&name).unwrap()).clone();
        let mut app = headless(defs.clone());
        let mut map = MapData::generate(192, 4242, 2);
        map.heights.iter_mut().for_each(|h| *h = 2.0);
        map.rebuild_blocked();
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, faction }).collect();
        let world = app.world_mut();
        setup_game(world, map.clone(), &setup);
        let dist = (d.max_range() * 0.9).min(160.0).max(d.vision + 6.0);
        spawn(world, &map, "radar_tower", 0, v2(100.0, 110.0), 0.0);
        spawn(world, &map, &name, 0, v2(100.0, 100.0), 0.0);
        let t = spawn(world, &map, "windmill", 1, v2(100.0 + dist, 100.0), 0.0);
        world.flush();
        let (mut shots, mut intel) = (0, Intel::default());
        for _ in 0..(d.weapons[0].reload * 10.0) as u32 + 250 {
            world.run_schedule(SimTick);
            shots += world.resource::<FxLog>().tick.iter().filter(|f| f.kind == FX_MUZZLE).count();
            let mut q = world.query::<(&Unit, &Intel)>();
            if let Some((_, i)) = q.iter(world).find(|(u, _)| u.id == t) {
                intel = *i;
            }
        }
        println!("   {name:10} range {:5.0} vision {:4.0} target at {dist:5.0}: on radar {}, seen {}, shots fired {shots}", d.max_range(), d.vision, intel.radar & 1 != 0, intel.seen & 1 != 0);
    }
    // 6. Capture and revive on flat ground.
    {
        let mut app = headless(defs.clone());
        let mut map = MapData::generate(192, 4242, 2);
        map.heights.iter_mut().for_each(|h| *h = 2.0);
        map.rebuild_blocked();
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, faction }).collect();
        let world = app.world_mut();
        setup_game(world, map.clone(), &setup);
        world.resource_mut::<Game>().players[0].energy = 4000.0;
        let eng = spawn(world, &map, "engineer", 0, v2(100.0, 100.0), 0.0);
        let mill = spawn(world, &map, "windmill", NEUTRAL, v2(112.0, 100.0), 0.0);
        let scav = spawn(world, &map, "scavenger", 0, v2(100.0, 120.0), 0.0);
        let tank = spawn(world, &map, "crony", 1, v2(112.0, 120.0), 0.0);
        world.flush();
        world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: vec![eng], orders: vec![Order::Capture(mill)], queue: false }));
        world.resource_mut::<CommandQueue>().0.push((1, Command::SelfDestruct { ids: vec![tank] }));
        let (mut captured, mut revived, mut wreck_mass) = (None, None, 0.0);
        for t in 0..1500u32 {
            world.run_schedule(SimTick);
            if t == 5 {
                let mut q = world.query::<&Wreck>();
                if let Some(w) = q.iter(world).next() {
                    wreck_mass = w.mass;
                    let wid = w.id;
                    world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: vec![scav], orders: vec![Order::Revive(wid)], queue: false }));
                }
            }
            let mut q = world.query::<(&Unit, Has<UnderConstruction>)>();
            for (u, uc) in q.iter(world) {
                if u.id == mill && u.owner == 0 && captured.is_none() {
                    captured = Some(t);
                }
                if defs.get(u.def).id == "crony" && u.owner == 0 && !uc && revived.is_none() {
                    revived = Some(t);
                }
            }
        }
        let wrecks_left = world.query::<&Wreck>().iter(world).count();
        let p = &world.resource::<Game>().players[0];
        println!("== Capture: neutral windmill taken over after {:?} ticks.  Revive: wreck ({wreck_mass:.0} mass) back as our tank after {:?} ticks, wrecks left {wrecks_left}.  Energy left {:.0}, mass {:.0}", captured, revived, p.energy, p.mass);
    }
    // A flat two-player world for the scenarios below.
    let flat_world = |n: usize| -> (App, MapData) {
        let mut app = headless(defs.clone());
        let mut map = MapData::generate(n, 4242, 2);
        map.heights.iter_mut().for_each(|h| *h = 2.0);
        map.rebuild_blocked();
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, faction }).collect();
        setup_game(app.world_mut(), map.clone(), &setup);
        for p in app.world_mut().resource_mut::<Game>().players.iter_mut() {
            p.energy = 100000.0;
            p.max_energy = 100000.0;
        }
        (app, map)
    };
    let order = |world: &mut World, ids: Vec<u32>, o: Order| world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids, orders: vec![o], queue: false }));
    // 7. Reclaiming a standing building, and reviving every wreck in a circle.
    {
        let (mut app, map) = flat_world(192);
        let world = app.world_mut();
        let eng = spawn(world, &map, "engineer", 0, v2(100.0, 100.0), 0.0);
        let mill = spawn(world, &map, "windmill", 0, v2(107.0, 101.0), 0.0);
        let scav = spawn(world, &map, "scavenger", 0, v2(100.0, 140.0), 0.0);
        let mut game = world.resource::<Game>().clone();
        {
            let mut commands = world.commands();
            for k in 0..3 {
                let id = game.alloc();
                spawn_wreck(&mut commands, Wreck { id, def: defs.id("crony").unwrap(), mass: 200.0, pos: v2(110.0 + k as f32 * 4.0, 140.0), y: 2.0, yaw: 0.0 });
            }
        }
        world.insert_resource(game);
        world.flush();
        // The commander's storage holds 650; keep the bank empty so the reclaimed mass shows.
        world.resource_mut::<Game>().players[0].mass = 0.0;
        order(world, vec![eng], Order::Reclaim(mill));
        let mut gone = None;
        let mut gained = 0.0;
        for t in 0..2500u32 {
            world.run_schedule(SimTick);
            let alive = world.query::<&Unit>().iter(world).any(|u| u.id == mill);
            if !alive && gone.is_none() {
                gone = Some(t);
                // (The commander's own income of 1 mass per second is in there too.)
                gained = world.resource::<Game>().players[0].mass - (t + 1) as f32 * 0.1;
                order(world, vec![scav], Order::ReviveArea { pos: v2(114.0, 140.0), radius: 10.0 });
            }
            // Reviving costs mass: keep the bank filled once the reclaim result has been read.
            if gone.is_some() {
                world.resource_mut::<Game>().players[0].mass = 600.0;
            }
        }
        let wrecks = world.query::<&Wreck>().iter(world).count();
        let tanks = world.query::<(&Unit, Has<UnderConstruction>)>().iter(world).filter(|(u, uc)| u.owner == 0 && !uc && defs.get(u.def).id == "crony").count();
        let worth = defs.get(defs.id("windmill").unwrap()).cost.mass * defs.rules.wreck_mass_fraction;
        println!("== Reclaim of a standing windmill: gone after {gone:?} ticks, {gained:.0} mass gained (worth {worth:.0}), wrecks left behind {wrecks}.  Area revive: {tanks} of 3 wrecks back as our tanks");
        assert!(gone.is_some() && (gained - worth).abs() < 2.0 && wrecks == 0 && tanks == 3, "reclaim / area revive");
    }
    // 7b. Trees and rocks: one area order clears a grove and a rock for their energy and mass, a
    // rollback brings them back, and a building put on a rock flattens it.
    {
        let mut app = headless(defs.clone());
        let mut map = MapData::generate(192, 4242, 2);
        map.heights.iter_mut().for_each(|h| *h = 2.0);
        map.rebuild_blocked();
        let (pine, rock) = (map.feature_kind("pine"), map.feature_kind("rock"));
        for k in 0..5 {
            map.features.push(Feature { kind: pine, pos: v2(110.0 + k as f32 * 2.5, 125.0), yaw: 0.0, scale: 1.0 });
        }
        map.features.push(Feature { kind: rock, pos: v2(114.0, 129.0), yaw: 0.0, scale: 1.0 });
        // (The spot test 7 builds a windmill on.)
        let mill_at = snap(&defs, &map, defs.id("windmill").unwrap(), v2(107.0, 101.0));
        map.features.push(Feature { kind: rock, pos: mill_at + v2(0.5, -0.5), yaw: 0.0, scale: 1.0 });
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, faction }).collect();
        setup_game(app.world_mut(), map.clone(), &setup);
        let world = app.world_mut();
        let eng = spawn(world, &map, "engineer", 0, v2(100.0, 100.0), 0.0);
        world.flush();
        world.run_schedule(SimTick);
        let (inc_m, inc_e) = {
            let p = &mut world.resource_mut::<Game>().players[0];
            p.max_mass = 1.0e6;
            p.max_energy = 1.0e6;
            p.mass = 0.0;
            p.energy = 0.0;
            (p.income_m, p.income_e)
        };
        let before = make_snapshot(world);
        order(world, vec![eng], Order::ReclaimArea { pos: v2(115.0, 127.0), radius: 8.0 });
        let mut done = None;
        for t in 0..1500u32 {
            world.run_schedule(SimTick);
            if (0..6).all(|i| !world.resource::<Features>().alive(i)) {
                done = Some(t + 1);
                break;
            }
        }
        let ticks = done.unwrap_or(1500) as f32;
        let p = world.resource::<Game>().players[0].clone();
        let (gm, ge) = (p.mass - inc_m * DT * ticks, p.energy - inc_e * DT * ticks);
        let fd = |id: &str| defs.rules.features[defs.feature_id(id).unwrap() as usize].clone();
        let (want_m, want_e) = (5.0 * fd("pine").mass + fd("rock").mass, 5.0 * fd("pine").energy + fd("rock").energy);
        let after = make_snapshot(world);
        restore(world, &before);
        let back = (0..6).filter(|i| world.resource::<Features>().alive(*i)).count();
        restore(world, &after);
        let again = (0..6).filter(|i| world.resource::<Features>().alive(*i)).count();
        let same = state_hash(&make_snapshot(world)) == state_hash(&after);
        order(world, vec![eng], Order::Build { def: defs.id("windmill").unwrap(), pos: mill_at });
        let mut flat = None;
        for t in 0..600u32 {
            world.run_schedule(SimTick);
            if !world.resource::<Features>().alive(6) {
                flat = Some(t + 1);
                break;
            }
        }
        println!("== Trees and rocks: area reclaim done after {done:?} ticks, {gm:.1} mass / {ge:.1} energy gained (worth {want_m:.1} / {want_e:.1}).  Rollback: {back} of 6 back, {again} after redo, hash equal {same}.  Rock under a new windmill gone after {flat:?} ticks");
        assert!(done.is_some() && (gm - want_m).abs() < 1.0 && (ge - want_e).abs() < 2.0 && back == 6 && again == 0 && same && flat.is_some(), "trees and rocks");
    }
    // 7c. A tree line blocks sight (not radar); once it is reclaimed the enemy is seen again.
    {
        let mut app = headless(defs.clone());
        let mut map = MapData::generate(192, 4242, 2);
        map.heights.iter_mut().for_each(|h| *h = 2.0);
        map.rebuild_blocked();
        let pine = map.feature_kind("pine");
        let vision = defs.get(defs.id("crony").unwrap()).vision;
        let (a, b) = (v2(150.0, 200.0), v2(150.0 + vision * 0.8, 200.0));
        let mid = (a.x + b.x) * 0.5;
        for k in 0..16 {
            map.features.push(Feature { kind: pine, pos: v2(mid, 188.0 + k as f32 * 1.5), yaw: 0.0, scale: 1.0 });
        }
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, faction }).collect();
        setup_game(app.world_mut(), map.clone(), &setup);
        let world = app.world_mut();
        spawn(world, &map, "crony", 0, a, 0.0);
        let enemy = spawn(world, &map, "crony", 1, b, std::f32::consts::PI);
        world.flush();
        let seen_by_0 = |world: &mut World| world.query::<(&Unit, &Intel)>().iter(world).any(|(u, i)| u.id == enemy && i.seen & 1 != 0);
        for _ in 0..3 {
            world.run_schedule(SimTick);
        }
        let behind_trees = seen_by_0(world);
        for i in 0..16 {
            world.resource_mut::<Features>().set_left(i, 0.0);
        }
        for _ in 0..3 {
            world.run_schedule(SimTick);
        }
        let in_the_open = seen_by_0(world);
        println!("== Sight through a tree line: enemy {:.0} away seen behind the trees: {behind_trees}, after they are gone: {in_the_open}", a.dist(b));
        assert!(!behind_trees && in_the_open, "trees block sight");
    }
    // 8. Craters and terraforming, and a state that is saved and restored with them.
    {
        let (mut app, map) = flat_world(192);
        let world = app.world_mut();
        let pike = spawn(world, &map, "pike", 0, v2(100.0, 180.0), 0.0);
        let eng = spawn(world, &map, "engineer", 0, v2(100.0, 100.0), 0.0);
        world.flush();
        order(world, vec![pike], Order::AttackGround { pos: v2(150.0, 180.0), radius: 5.0 });
        order(world, vec![eng], Order::Terraform { pos: v2(110.0, 100.0), radius: 5.0, height: 5.0 });
        for _ in 0..300 {
            world.run_schedule(SimTick);
        }
        let mid = make_snapshot(world);
        let mid_heights = world.resource::<MapData>().heights.clone();
        for _ in 0..600 {
            world.run_schedule(SimTick);
        }
        let end = make_snapshot(world);
        let end_heights = world.resource::<MapData>().heights.clone();
        let (changed, lowest) = (end.ground.len(), end_heights.iter().cloned().fold(f32::MAX, f32::min));
        let plateau = world.resource::<MapData>().h(v2(110.0, 100.0));
        let idle = world.query::<(&Unit, &Orders)>().iter(world).find(|(u, _)| u.id == eng).map_or(false, |(_, o)| o.0.is_empty());
        // Roll back to the middle, and forward again: the terrain has to follow both ways.
        restore(world, &mid);
        let back_ok = world.resource::<MapData>().heights == mid_heights && state_hash(&make_snapshot(world)) == state_hash(&mid);
        restore(world, &end);
        let fwd_ok = world.resource::<MapData>().heights == end_heights && state_hash(&make_snapshot(world)) == state_hash(&end);
        // The blocked-cell grid kept up to date cell by cell must equal a full rebuild.
        let mut fresh = world.resource::<MapData>().clone();
        fresh.rebuild_blocked();
        let blocked_ok = fresh.blocked == world.resource::<MapData>().blocked;
        println!("== Terrain: {changed} vertices changed, deepest crater {:.2} below the ground; terraformed plateau at {plateau:.2} (asked for 5.00), engineer done: {idle}", 2.0 - lowest);
        println!("   state restored backwards: {back_ok}, forwards: {fwd_ok}, passability grid consistent: {blocked_ok}; changed vertices after 30 s: {}, after 90 s: {}", mid.ground.len(), end.ground.len());
        assert!(changed > 0 && lowest < 1.9 && (plateau - 5.0).abs() < 0.05 && idle && back_ok && fwd_ok && blocked_ok, "terrain");
    }
    // 9. Transports: an aircraft picks up an area and sets it down elsewhere; tanks board an APC.
    {
        let (mut app, map) = flat_world(192);
        let world = app.world_mut();
        let stork = spawn(world, &map, "air_transport", 0, v2(90.0, 100.0), 0.0);
        let apc = spawn(world, &map, "apc", 0, v2(100.0, 140.0), 0.0);
        let tanks: Vec<u32> = (0..10).map(|k| spawn(world, &map, "crony", 0, v2(100.0 + (k % 5) as f32 * 2.5, 100.0 + (k / 5) as f32 * 2.5), 0.0)).collect();
        let riders: Vec<u32> = (0..3).map(|k| spawn(world, &map, "overtaker", 0, v2(110.0 + k as f32 * 2.5, 146.0), 0.0)).collect();
        world.flush();
        order(world, vec![stork], Order::LoadArea { pos: v2(105.0, 101.0), radius: 12.0 });
        world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: vec![stork], orders: vec![Order::Unload { pos: v2(220.0, 220.0), radius: 6.0 }], queue: true }));
        order(world, riders.clone(), Order::Load(apc));
        let (mut most, mut in_apc) = (0, 0);
        for t in 0..900u32 {
            world.run_schedule(SimTick);
            let mut q = world.query::<&Hosted>();
            most = most.max(q.iter(world).filter(|h| h.0 == stork).count());
            in_apc = in_apc.max(q.iter(world).filter(|h| h.0 == apc).count());
            if t == 200 {
                world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: vec![apc], orders: vec![Order::Unload { pos: v2(150.0, 150.0), radius: 4.0 }], queue: false }));
            }
        }
        let mut q = world.query::<(&Unit, &Body, Has<Hosted>)>();
        let moved = q.iter(world).filter(|(u, b, h)| tanks.contains(&u.id) && !h && b.pos.dist(v2(220.0, 220.0)) < 8.0).count();
        let left = q.iter(world).filter(|(u, b, h)| tanks.contains(&u.id) && !h && b.pos.dist(v2(105.0, 101.0)) < 14.0).count();
        let out = q.iter(world).filter(|(u, b, h)| riders.contains(&u.id) && !h && b.pos.dist(v2(150.0, 150.0)) < 7.0).count();
        let cap = defs.get(defs.id("air_transport").unwrap()).transport.as_ref().map_or(0, |t| t.capacity);
        println!("== Transport aircraft (capacity {cap}) offered 10 tanks: carried {most}, set down at the target {moved}, left behind {left}.  APC: {in_apc} of 3 boarded, {out} set down at its target");
        assert!(most == cap as usize && moved == most && left == 10 - most && in_apc == 3 && out == 3, "transports");
    }
    // 10. Control points: the team that holds fewer points loses its score and then the game.
    {
        let (mut app, map) = flat_world(192);
        let world = app.world_mut();
        enable_control_points(world, 400.0);
        let points: Vec<V2> = world.resource::<Game>().points.iter().map(|p| p.pos).collect();
        spawn(world, &map, "crony", 0, points[0] + v2(2.0, 0.0), 0.0);
        spawn(world, &map, "crony", 0, points[1] + v2(2.0, 0.0), 0.0);
        spawn(world, &map, "crony", 1, points[2] + v2(-3.0, 0.0), 0.0);
        world.flush();
        let mut taken = None;
        let mut won = None;
        for t in 0..6000u32 {
            world.run_schedule(SimTick);
            let g = world.resource::<Game>();
            if g.points[0].owner == 0 && taken.is_none() {
                taken = Some(t);
            }
            if g.winner.is_some() {
                won = Some((t, g.winner));
                break;
            }
        }
        let g = world.resource::<Game>();
        println!("== Control points: {} on the map; point 1 taken after {taken:?} ticks; points left team 1: {:.0}, team 2: {:.0}; game ended: {won:?}", points.len(), g.scores[0], g.scores[1]);
        assert!(points.len() >= 3 && taken.is_some() && won.map_or(false, |w| w.1 == Some(0)) && g.scores[0] > 0.0 && g.scores[1] <= 0.0, "control points");
    }
}

/// Loopback check of discovery, the handshake and the netplay: a host with three AIs and a client
/// that predicts ahead play for ten minutes of game time (`LR_TICKS` to change); the client's
/// confirmed state must stay identical to the host's although the client rolls back and
/// re-simulates on every turn. In the second half the client predicts too little, so its commands
/// reach the host late and are corrected.
fn nettest() {
    use longrange::netplay::{ClientSim, HostSim};
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
    let ticks: u32 = std::env::var("LR_TICKS").ok().and_then(|s| s.parse().ok()).unwrap_or(6000);
    let mut map = MapData::generate(128, 7, 4);
    map.scatter_features(&defs.rules.features, 7, 1.0);
    let faction = defs.playable_factions()[0] as u8;
    let setup: Vec<PlayerSetup> = (0..4).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: i != 1, slot: i, faction }).collect();
    host.send_msg(conn, &S2C::Start { map: map.clone(), you: 1, data: None });
    let hw = happ.world_mut();
    setup_game(hw, map, &setup);
    hw.insert_resource(Ais(vec![Ai::new(0, 7), Ai::new(2, 7), Ai::new(3, 7)]));
    let mut hsim = HostSim::new(hw);
    host.send(conn, &frame(TAG_SNAP, bincode::serialize(&make_snapshot(hw)).unwrap()));

    let mut capp = headless(defs.clone());
    let cw = capp.world_mut();
    let mut start: Option<MapData> = None;
    let mut csim: Option<ClientSim> = None;
    while csim.is_none() {
        let (tag, data) = client.frames.recv_timeout(Duration::from_secs(5)).expect("frame").expect("open");
        if tag == TAG_MSG {
            if let Ok(S2C::Start { map, you, .. }) = bincode::deserialize::<S2C>(&data) {
                println!("client got Start: map {} cells ({} bytes), you={you}", map.n, data.len());
                start = Some(map);
            }
        } else {
            let s: Snapshot = bincode::deserialize(&data).expect("state");
            println!("client got the starting state: tick {} with {} units ({} bytes)", s.game.tick, s.units.len(), data.len());
            join_game(cw, start.take().expect("Start first"), &s);
            csim = Some(ClientSim::new(1, s));
        }
    }
    let mut csim = csim.unwrap();
    csim.lead = 3;
    let com = cw.query::<(&Unit, &Body)>().iter(cw).find(|(u, _)| u.owner == 1).map(|(u, b)| (u.id, b.pos)).expect("own commander");

    let (mut mismatches, mut turn_bytes, mut late, mut shots, mut most_units) = (0, 0usize, 0, 0u32, 0usize);
    for t in 0..ticks {
        csim.lead = if t < ticks / 2 { 3 } else { 1 };
        // The client orders its units around every few seconds.
        if t % 50 == 7 {
            let to = com.1 + v2(15.0 * ((t / 50) % 3) as f32, 6.0 * ((t / 50) % 2) as f32);
            let ids: Vec<u32> = cw.query::<&Unit>().iter(cw).filter(|u| u.owner == 1).map(|u| u.id).collect();
            for m in csim.issue(cw, vec![Command::Orders { ids, orders: vec![Order::Move(to)], queue: false }]) {
                client.send(&m);
            }
        }
        std::thread::sleep(Duration::from_micros(300));
        while let Ok(ev) = host.events.try_recv() {
            match ev {
                HostEvent::Msg(_, C2S::Cmd { tick, seq, cmd }) => {
                    if tick <= hw.resource::<Game>().tick {
                        late += 1;
                    }
                    hsim.command(hw, 1, tick, seq, cmd)
                }
                HostEvent::Msg(_, C2S::Hash { tick, hash }) => {
                    if !hsim.matches(tick, hash) {
                        mismatches += 1;
                    }
                }
                _ => {}
            }
        }
        let turn = bincode::serialize(&hsim.tick(hw, vec![], true)).unwrap();
        shots += hw.resource::<FxLog>().tick.iter().filter(|f| f.kind == FX_MUZZLE).count() as u32;
        if t % 100 == 0 {
            most_units = most_units.max(hw.query::<&Unit>().iter(hw).count());
        }
        turn_bytes += turn.len();
        host.send(conn, &frame(TAG_MSG, turn));
        // Jitter: the client does not look at the network on every third tick; in the second half
        // it goes deaf for five ticks out of eight, so the orders above are stamped too early.
        if if t < ticks / 2 { t % 3 != 0 } else { t % 8 < 3 } {
            while let Ok(Some((tag, data))) = client.frames.recv_timeout(Duration::from_micros(300)) {
                csim.receive(tag, &data).expect("frame");
            }
        }
        for m in csim.update(cw, DT).expect("client update") {
            if !matches!(m, C2S::Ping(_)) {
                client.send(&m);
            }
        }
    }
    // Let the client catch up with everything the host sent.
    let end = hw.resource::<Game>().tick;
    while csim.confirmed_tick() < end {
        let (tag, data) = client.frames.recv_timeout(Duration::from_secs(5)).expect("frame").expect("open");
        csim.receive(tag, &data).expect("frame");
        csim.update(cw, 0.0).expect("client update");
    }
    let hs = make_snapshot(hw);
    println!("host tick {end}: {} units, {} projectiles; full state {} bytes, turns averaged {} bytes", hs.units.len(), hs.projectiles.len(), bincode::serialize(&hs).unwrap().len(), turn_bytes / ticks as usize);
    println!("during the game: {shots} shots fired, up to {most_units} units, next id {}", hs.game.next_id);
    println!("{late} of the client's commands reached the host late; its shown tick {} is {} ahead of the confirmed one", cw.resource::<Game>().tick, cw.resource::<Game>().tick - csim.confirmed_tick());
    println!("state hash host {:016x} client {:016x}, mismatches reported during the game: {mismatches}", state_hash(&hs), csim.confirmed_hash());
    assert!(late > 0, "the late-command path was not exercised");
    assert_eq!(state_hash(&hs), csim.confirmed_hash(), "host and client diverged");
    assert_eq!(mismatches, 0);

    // Divergence repair: change the host's state behind the client's back. The client's next
    // fingerprint is wrong, the host answers with its full state, and they agree again.
    hw.resource_mut::<Game>().players[0].mass += 1.0;
    for _ in 0..3 * longrange::netplay::HASH_EVERY {
        std::thread::sleep(Duration::from_micros(300));
        while let Ok(ev) = host.events.try_recv() {
            if let HostEvent::Msg(_, C2S::Hash { tick, hash }) = ev {
                if !hsim.matches(tick, hash) {
                    mismatches += 1;
                    host.send(conn, &frame(TAG_SNAP, bincode::serialize(&make_snapshot(hw)).unwrap()));
                }
            }
        }
        host.send(conn, &frame(TAG_MSG, bincode::serialize(&hsim.tick(hw, vec![], true)).unwrap()));
        while let Ok(Some((tag, data))) = client.frames.recv_timeout(Duration::from_micros(300)) {
            csim.receive(tag, &data).expect("frame");
        }
        for m in csim.update(cw, DT).expect("client update") {
            if !matches!(m, C2S::Ping(_)) {
                client.send(&m);
            }
        }
    }
    let end = hw.resource::<Game>().tick;
    while csim.confirmed_tick() < end {
        let (tag, data) = client.frames.recv_timeout(Duration::from_secs(5)).expect("frame").expect("open");
        csim.receive(tag, &data).expect("frame");
        csim.update(cw, 0.0).expect("client update");
    }
    println!("after tampering with the host: {mismatches} mismatch(es) reported, {} full state(s) sent, hashes equal again: {}", csim.resyncs, state_hash(&make_snapshot(hw)) == csim.confirmed_hash());
    assert!(mismatches > 0 && csim.resyncs > 0);
    assert_eq!(state_hash(&make_snapshot(hw)), csim.confirmed_hash(), "the full state did not repair the client");

    // Pause: the client asks, the host stops after its last turn. The client must drop its
    // prediction and stand on the host's tick; an order given while paused lands after resuming.
    fn net_step(host: &Host, conn: u32, client: &mut Client, hw: &mut World, cw: &mut World, csim: &mut ClientSim, hsim: &mut HostSim, paused: &mut bool, ticking: bool) {
        std::thread::sleep(Duration::from_micros(300));
        while let Ok(ev) = host.events.try_recv() {
            match ev {
                HostEvent::Msg(_, C2S::Pause(on)) if on != *paused => {
                    *paused = on;
                    host.send_msg(conn, &S2C::Paused(if on { Some(1) } else { None }));
                }
                HostEvent::Msg(_, C2S::Cmd { tick, seq, cmd }) => hsim.command(hw, 1, tick, seq, cmd),
                _ => {}
            }
        }
        if ticking && !*paused {
            host.send(conn, &frame(TAG_MSG, bincode::serialize(&hsim.tick(hw, vec![], true)).unwrap()));
        }
        while let Ok(Some((tag, data))) = client.frames.recv_timeout(Duration::from_micros(300)) {
            csim.receive(tag, &data).expect("frame");
        }
        for m in csim.update(cw, DT).expect("client update") {
            if !matches!(m, C2S::Ping(_) | C2S::Hash { .. }) {
                client.send(&m);
            }
        }
    }
    let mut paused = false;
    csim.lead = 3;
    for _ in 0..10 {
        net_step(&host, conn, &mut client, hw, cw, &mut csim, &mut hsim, &mut paused, true);
    }
    client.send(&C2S::Pause(true));
    for _ in 0..50 {
        net_step(&host, conn, &mut client, hw, cw, &mut csim, &mut hsim, &mut paused, true);
    }
    let at = hw.resource::<Game>().tick;
    assert!(paused && csim.paused == Some(1), "the pause did not reach the client");
    assert_eq!(cw.resource::<Game>().tick, at, "a paused client must show the host's tick");
    assert_eq!(state_hash(&make_snapshot(hw)), state_hash(&make_snapshot(cw)), "paused states differ");
    let ids: Vec<u32> = cw.query::<&Unit>().iter(cw).filter(|u| u.owner == 1).map(|u| u.id).collect();
    for m in csim.issue(cw, vec![Command::Orders { ids, orders: vec![Order::Move(com.1 + v2(-12.0, -8.0))], queue: false }]) {
        client.send(&m);
    }
    for _ in 0..20 {
        net_step(&host, conn, &mut client, hw, cw, &mut csim, &mut hsim, &mut paused, true);
    }
    assert_eq!(hw.resource::<Game>().tick, at, "the host simulated while paused");
    client.send(&C2S::Pause(false));
    for _ in 0..60 {
        net_step(&host, conn, &mut client, hw, cw, &mut csim, &mut hsim, &mut paused, true);
    }
    let end = hw.resource::<Game>().tick;
    while csim.confirmed_tick() < end {
        net_step(&host, conn, &mut client, hw, cw, &mut csim, &mut hsim, &mut paused, false);
    }
    println!("paused at tick {at} with the client on the same tick, resumed to {end}, hashes equal: {}", state_hash(&make_snapshot(hw)) == csim.confirmed_hash());
    assert!(!paused && csim.paused.is_none() && end > at);
    assert_eq!(state_hash(&make_snapshot(hw)), csim.confirmed_hash(), "host and client diverged after the pause");
    drop(client);
    loop {
        match host.events.recv_timeout(Duration::from_secs(2)).expect("left") {
            HostEvent::Left(id) => {
                println!("conn {id} left cleanly");
                break;
            }
            _ => {}
        }
    }
    println!("nettest OK");
}

/// Developer aid: `--icons FILE.png` draws every strategic icon and order icon into one picture.
fn icon_sheet(path: &str) {
    use longrange::view::icons;
    let defs = load_defs();
    let orders = ["move", "attack_move", "patrol", "ground", "reclaim", "revive", "capture", "terraform", "load", "unload", "assist", "stop", "pause", "resume", "repeat", "upgrade", "mass", "energy", "time", "power", "flag"];
    let (cell, cols) = (icons::ICON + 8, 12);
    let n = defs.units.len() + orders.len();
    let rows = (n + cols - 1) / cols;
    let mut img = image::RgbaImage::from_pixel((cols * cell) as u32, (rows * cell) as u32, image::Rgba([40, 60, 45, 255]));
    let mut blit = |k: usize, px: &[u8], tint: [f32; 3]| {
        let (ox, oy) = ((k % cols) * cell + 4, (k / cols) * cell + 4);
        for y in 0..icons::ICON {
            for x in 0..icons::ICON {
                let s = &px[(y * icons::ICON + x) * 4..];
                let a = s[3] as f32 / 255.0;
                let dst = img.get_pixel_mut((ox + x) as u32, (oy + y) as u32);
                for c in 0..3 {
                    dst.0[c] = (dst.0[c] as f32 * (1.0 - a) + s[c] as f32 * tint[c] * a) as u8;
                }
            }
        }
    };
    for (k, d) in defs.units.iter().enumerate() {
        blit(k, &icons::render(&d.icon, d.tier), [0.3, 0.6, 1.0]);
    }
    for (k, o) in orders.iter().enumerate() {
        blit(defs.units.len() + k, &icons::render_order(o), [1.0, 1.0, 1.0]);
    }
    match img.save(path) {
        Ok(()) => println!("wrote {path}: {} unit icons in data order, then {} order icons", defs.units.len(), orders.len()),
        Err(e) => eprintln!("{path}: {e}"),
    }
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
    if let Some(k) = args.iter().position(|a| a == "--icons") {
        return icon_sheet(args.get(k + 1).map_or("icons.png", |s| s.as_str()));
    }
    let defs = load_defs();
    let mut name = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "Player".into());
    name.truncate(16);
    let settings = longrange::settings::Settings::load();
    if !settings.name.trim().is_empty() {
        name = settings.name.clone();
    }
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "Long Range".into(), resolution: bevy::window::WindowResolution::new(1280, 720), ..default() }),
        ..default()
    }).set(longrange::settings::render_plugin(&settings)).set(longrange::settings::log_plugin()))
    .add_plugins(bevy_egui::EguiPlugin::default())
    .insert_resource(ClearColor(Color::srgb(0.06, 0.07, 0.09)))
    .insert_resource(defs.clone())
    .insert_resource(longrange::settings::StartSettings(settings.clone()))
    .insert_resource(settings)
    .insert_non_send(AppState { screen: Screen::Menu, name, ip: String::new(), message: String::new(), discovery: Discovery::new(), skirmish: args.iter().any(|a| a == "--skirmish"), settings_open: false, base_defs: defs })
    .add_plugins((SimPlugin, view::ViewPlugin))
    .add_systems(Startup, setup)
    .add_systems(PreUpdate, session_update)
    .add_systems(Update, (fullscreen_toggle, longrange::settings::apply_settings).chain())
    .add_systems(Update, dev_screenshot)
    .add_systems(bevy_egui::EguiPrimaryContextPass, menu_ui);
    longrange::settings::tolerate_render_errors(&mut app);
    app.run();
}
