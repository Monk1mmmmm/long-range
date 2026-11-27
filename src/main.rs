//! Long Range - game executable: window, main menu, lobby and the host/client session loop.
// Note: no `windows_subsystem = "windows"`: the console window carries the log, which is what
// people need when the game fails to start on their GPU (`longrange.exe > log.txt 2>&1`).

use bevy::prelude::*;
use bevy_egui::egui;
use longrange::ai::{Ai, AiKind, Ais, AI_KINDS};
use longrange::data::{asset_root, DataPack, Defs};
use longrange::i18n::tr;
use longrange::{tr, tx};
use longrange::map::*;
use longrange::net::*;
use longrange::netplay::{ClientSim, HostSim};
use longrange::replay::{self, Playback, Recorder, ReplayInfo, ReplayStart};
use longrange::sim::*;
use longrange::sim::Command;
use longrange::view::{self, bench::Bench, terrain::MainCamera, LocalCommands, ViewState};

const SIZES: [(usize, &str); 3] = [(192, tx!("lobby.size_small")), (256, tx!("lobby.size_medium")), (384, tx!("lobby.size_large"))];

struct Slot {
    kind: u8,
    name: String,
    team: u8,
    faction: u8,
    /// Index into `rules.player_colors`; every slot has its own.
    color: u8,
    /// AI slots: which AI plays it.
    ai: AiKind,
    conn: u32,
    /// Fingerprint of that player's game data (from `Hello`).
    hash: u64,
}

/// Someone in the lobby who only watches the game (`conn` 0: the host).
struct Observer {
    name: String,
    conn: u32,
    hash: u64,
}

/// Most observers a lobby takes.
const MAX_OBSERVERS: usize = 8;

struct HostLobby {
    host: Host,
    slots: Vec<Slot>,
    observers: Vec<Observer>,
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
    /// The control points the game will use (0..1 across, see `MapData::control_points`).
    points: Vec<(f32, f32)>,
    tex: Option<egui::TextureHandle>,
    /// Thumbnails of the premade maps for the Map tab (made one per frame).
    thumbs: Vec<Option<egui::TextureHandle>>,
    /// Right-hand tab: 0 = game options, 1 = map.
    tab: u8,
    /// The map's neutral and hostile buildings are known from the start.
    reveal_props: bool,
    /// Infantry capture enemy buildings other than factories (`Game.infantry_capture`).
    infantry_capture: bool,
    /// Everyone sees every player's mass and energy income in the player list (`Game.eco_public`).
    eco_public: bool,
    /// Wrecks and ruins block land units' paths (`Game.rubble_blocks`); set from the data
    /// (`rules.rubble_blocks_paths`) whenever the game data changes.
    rubble_blocks: bool,
    /// Weather: k = `rules.weather[k]`, the length of the list = random (`Game.weather`).
    weather: usize,
    /// Start positions of each premade map.
    counts: Vec<usize>,
    /// Ground of random maps: 0 = picked by the seed, k = `surface::BIOMES[k - 1]`.
    biome: usize,
    /// The map picture each connection was sent last (its hash), so it is sent only when it changes.
    sent_preview: Vec<(u32, u64)>,
}

/// What a lobby's map depends on: (choice, players, seed, data hash, biome).
type MapKey = (usize, usize, u64, u64, usize);
const PREVIEW: usize = 256;

impl HostLobby {
    fn mode_label(&self) -> &'static str {
        if self.mode == MODE_POINTS {
            tr!("lobby.mode_points")
        } else {
            tr!("lobby.mode_annihilation")
        }
    }
    /// Starting points of a control points game: the lobby's choice or the game data's value.
    fn cp_points(&self, defs: &Defs) -> f32 {
        if self.cp_start == 0 { defs.rules.cp_start_points } else { self.cp_start as f32 }
    }
    fn weather_label(&self, defs: &Defs) -> String {
        match defs.rules.weather.get(self.weather) {
            Some(w) => longrange::i18n::weather_name(w).to_string(),
            None => tr!("lobby.weather_random").to_string(),
        }
    }
    /// The weather the game is played in (a random choice is made at the start).
    fn weather_index(&self, defs: &Defs) -> u8 {
        let n = defs.rules.weather.len().max(1);
        if self.weather < n {
            return self.weather as u8;
        }
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos() as u64);
        ((t ^ self.seed).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33) as u8 % n as u8
    }
    fn preset_label(&self) -> String {
        match self.preset {
            0 => tr!("lobby.built_in").to_string(),
            k => self.presets.get(k - 1).cloned().unwrap_or_default(),
        }
    }
    fn map_label(&self) -> String {
        match SIZES.get(self.map_choice) {
            Some((n, name)) => tr!("lobby.random_size", tr(name), *n as f32 * CELL, *n as f32 * CELL),
            None => self.maps.get(self.map_choice - SIZES.len()).map(|m| longrange::data::split_map(m).0.to_string()).unwrap_or_default(),
        }
    }
    /// Number of start positions of the chosen map.
    fn start_count(&self) -> usize {
        match SIZES.get(self.map_choice) {
            Some(_) => self.players,
            None => self.counts.get(self.map_choice - SIZES.len()).copied().unwrap_or(0),
        }
        .clamp(2, MAX_PLAYERS)
    }
    /// Makes the slot list match the map: one slot per start position. Players whose slot is
    /// removed become observers.
    fn resize_slots(&mut self, faction: u8) {
        let n = self.start_count();
        while self.slots.len() > n {
            if let Some(s) = self.slots.pop() {
                if s.kind == 2 {
                    self.observers.push(Observer { name: s.name, conn: s.conn, hash: s.hash });
                }
            }
        }
        while self.slots.len() < n {
            let k = self.slots.len() as u8;
            let color = (0u8..).find(|c| self.slots.iter().all(|s| s.color != *c)).unwrap_or(k);
            self.slots.push(Slot { kind: 0, name: String::new(), team: k, faction, color, ai: AI_KINDS[0], conn: 0, hash: 0 });
        }
        self.dirty = true;
    }
    fn map_key(&self, defs: &Defs) -> MapKey {
        let random = self.map_choice < SIZES.len();
        (self.map_choice, if random { self.players } else { 0 }, if random { self.seed } else { 0 }, defs.hash, if random { self.biome } else { 0 })
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
                if let Some(b) = self.biome.checked_sub(1).and_then(|k| longrange::surface::BIOMES.get(k)) {
                    m.paint_ground(*b);
                }
                m.scatter_features(&defs.rules.features, self.seed, 1.0);
                m
            }
            None => longrange::data::load_map(&self.maps[self.map_choice - SIZES.len()])?,
        };
        self.preview = view::terrain::map_preview(&map, defs, PREVIEW);
        self.starts = map.starts.iter().map(|s| map.pic_uv(*s)).collect();
        self.points = map.control_points().iter().map(|s| map.pic_uv(*s)).collect();
        self.tex = None;
        self.map = Some((key, map));
        self.dirty = true;
        Ok(())
    }
    fn options_text(&self, defs: &Defs) -> String {
        let mut t = tr!("lobby.mode_button", self.mode_label());
        if self.mode == MODE_POINTS {
            t += &tr!("lobby.summary_points", self.cp_points(defs));
        }
        t += &tr!("lobby.summary_data", self.preset_label());
        let off = defs.rules.switched_off();
        if !off.is_empty() {
            t += &tr!("lobby.summary_switched_off", off.join(", "));
        }
        t += &tr!("lobby.summary_props", if self.reveal_props { tr!("lobby.yes") } else { tr!("lobby.no") });
        if self.infantry_capture {
            t += &tr!("lobby.summary_infantry_capture", tr!("lobby.yes"));
        }
        t += &tr!("lobby.summary_eco_public", if self.eco_public { tr!("lobby.yes") } else { tr!("lobby.no") });
        t += &tr!("lobby.summary_rubble_blocks", if self.rubble_blocks { tr!("lobby.yes") } else { tr!("lobby.no") });
        t += &tr!("lobby.summary_weather", self.weather_label(defs));
        if self.map_choice < SIZES.len() {
            let b = self.biome.checked_sub(1).and_then(|k| longrange::surface::BIOMES.get(k).copied()).unwrap_or(longrange::surface::Biome::from_seed(self.seed));
            t += &tr!("lobby.summary_terrain", longrange::i18n::biome_name(b));
        }
        t
    }
    /// Gives slot `k` that colour; whoever had it gets the colour slot `k` had.
    fn set_color(&mut self, k: usize, c: u8) {
        if let Some(j) = self.slots.iter().position(|s| s.color == c) {
            self.slots[j].color = self.slots[k].color;
        }
        self.slots[k].color = c;
        self.dirty = true;
    }
    /// Moves that connection's player (`conn` 0: the host), or an observer, into the open slot `to`.
    fn take_slot(&mut self, conn: u32, to: usize) -> bool {
        if self.slots.get(to).map_or(true, |s| s.kind != 0) {
            return false;
        }
        if let Some(from) = self.slot_of(conn) {
            self.slots.swap(from, to);
        } else if let Some(o) = self.observers.iter().position(|o| o.conn == conn) {
            let o = self.observers.remove(o);
            let s = &mut self.slots[to];
            (s.kind, s.name, s.conn, s.hash) = (2, o.name, o.conn, o.hash);
        } else {
            return false;
        }
        self.dirty = true;
        true
    }
    /// The slot of that connection's player (`conn` 0: the host), if it has one.
    fn slot_of(&self, conn: u32) -> Option<usize> {
        self.slots.iter().position(|s| s.kind == 2 && s.conn == conn)
    }
    /// Moves that connection's player (`conn` 0: the host) to the observers, or from them to the
    /// first open slot. Returns false when that is not possible.
    fn observe(&mut self, conn: u32, watch: bool, faction: u8) -> bool {
        if watch {
            let Some(k) = self.slot_of(conn) else { return false };
            if self.observers.len() >= MAX_OBSERVERS {
                return false;
            }
            let s = &mut self.slots[k];
            self.observers.push(Observer { name: std::mem::take(&mut s.name), conn, hash: s.hash });
            s.kind = 0;
            s.conn = 0;
            s.faction = faction;
        } else {
            let Some(o) = self.observers.iter().position(|o| o.conn == conn) else { return false };
            let Some(s) = self.slots.iter_mut().find(|s| s.kind == 0) else { return false };
            let o = self.observers.remove(o);
            (s.kind, s.name, s.conn, s.hash) = (2, o.name, o.conn, o.hash);
        }
        self.dirty = true;
        true
    }
    /// Lobby state as that connection (`conn` 0: the host) sees it.
    fn info(&self, conn: u32, defs: &Defs) -> LobbyInfo {
        let you = self.slot_of(conn).map_or(-1, |k| k as i8);
        LobbyInfo {
            slots: self.slots.iter().map(|s| SlotInfo { kind: s.kind, name: s.name.clone(), team: s.team, faction: s.faction, ai: s.ai.id(), color: s.color }).collect(),
            map_name: self.map_label(),
            you,
            extra: self.options_text(defs),
            preview: self.preview.clone(),
            preview_side: PREVIEW as u16,
            preview_same: false,
            starts: self.starts.clone(),
            points: if self.mode == MODE_POINTS { self.points.clone() } else { vec![] },
            observers: self.observers.iter().map(|o| o.name.clone()).collect(),
            observing: self.observers.iter().any(|o| o.conn == conn),
            map_file: self.map_file(),
            preset: self.preset_name(),
            data_hash: defs.hash,
        }
    }
    /// Folder name of the chosen premade map; empty for a random map.
    fn map_file(&self) -> String {
        self.map_choice.checked_sub(SIZES.len()).and_then(|k| self.maps.get(k)).cloned().unwrap_or_default()
    }
    /// The chosen data preset; empty for the built-in data.
    fn preset_name(&self) -> String {
        self.preset.checked_sub(1).and_then(|k| self.presets.get(k)).cloned().unwrap_or_default()
    }
    /// What the game browser shows of this lobby.
    fn ad(&self, defs: &Defs) -> GameAd {
        let count = |k: u8| self.slots.iter().filter(|s| s.kind == k).count() as u8;
        GameAd {
            host: String::new(),
            map: self.map_file(),
            cells: SIZES.get(self.map_choice).map_or(0, |s| s.0 as u16),
            preview: ad_preview(&self.preview, PREVIEW),
            starts: self.starts.iter().map(|(x, y)| ((x * 255.0).round().clamp(0.0, 255.0) as u8, (y * 255.0).round().clamp(0.0, 255.0) as u8)).collect(),
            humans: count(2),
            ais: count(3),
            slots: self.slots.len() as u8 - count(1),
            observers: self.observers.len() as u8,
            can_watch: self.observers.len() < MAX_OBSERVERS,
            mode: self.mode,
            preset: self.preset_name(),
            data_hash: defs.hash,
            started: false,
        }
    }
}

struct ClientLobby {
    client: Client,
    info: Option<LobbyInfo>,
    pending: Option<(MapData, u8)>,
    /// The map preview the host sent (keyed by a checksum of its pixels).
    tex: Option<(u64, egui::TextureHandle)>,
    /// Whether the host's premade map (folder name) is on this computer.
    have_map: Option<(String, bool)>,
}

/// Two maps are the same map (a saved copy may differ from the original by rounding).
fn same_map(a: &MapData, b: &MapData) -> bool {
    a.dims() == b.dims()
        && a.heights.len() == b.heights.len()
        && a.heights.iter().zip(&b.heights).all(|(x, y)| (x - y).abs() < 0.01)
        && a.starts == b.starts
        && a.deposits.len() == b.deposits.len()
        && a.props.len() == b.props.len()
        && a.features.len() == b.features.len()
        && a.layers.len() == b.layers.len()
        && a.masks == b.masks
        && a.lake_of == b.lake_of
}

/// Keeps the map a host sent in `assets/maps`: under its folder name, or (when another map has
/// that name here) the first free `name_2`, `name_3`... Nothing is written when the same map is
/// already there. Returns the folder it is in.
fn keep_map(map: &MapData, file: &str) -> Result<String, String> {
    let base = longrange::data::clean_name(file).replace(' ', "_");
    if base.is_empty() {
        return Err("the map has no usable name".into());
    }
    for k in 1..100 {
        let name = if k == 1 { base.clone() } else { format!("{base}_{k}") };
        let dir = longrange::data::map_dir(&name);
        if !dir.join("map.json").exists() {
            let dir = longrange::data::user_maps_dir().join(&name);
            map.save(&dir)?;
            return Ok(name);
        }
        if MapData::load(&dir).map_or(false, |m| same_map(&m, map)) {
            return Ok(name);
        }
    }
    Err("too many maps of that name".into())
}

/// Keeps the host's data preset among the presets: under its name, or `name 2`, `name 3`... when
/// a different preset has that name here. Nothing is written when the same one is already there.
fn keep_preset(pack: &DataPack) -> Result<Option<String>, String> {
    let root = asset_root();
    let base = longrange::data::clean_name(&pack.name);
    if base.is_empty() {
        return Ok(None);
    }
    for k in 1..100 {
        let name = if k == 1 { base.clone() } else { longrange::data::clean_name(&format!("{} {k}", base.chars().take(28).collect::<String>())) };
        if !DataPack::exists(&root, &name) {
            let mut p = pack.clone();
            p.name = name.clone();
            p.files.clear();
            p.save()?;
            return Ok(Some(name));
        }
        if DataPack::preset(&root, &name).map_or(false, |h| h.rules == pack.rules && h.factions == pack.factions) {
            return Ok(Some(name));
        }
    }
    Err("too many presets of that name".into())
}

/// Saves what the host sent for this game (its premade map, its data preset) so this player has
/// them from now on, in the background.
fn keep_downloads(map: Option<(MapData, String)>, pack: Option<DataPack>) {
    std::thread::spawn(move || {
        if let Some((map, file)) = map {
            match keep_map(&map, &file) {
                Ok(name) => info!("the host's map is in assets/maps/{name}"),
                Err(e) => warn!("could not save the host's map {file}: {e}"),
            }
        }
        if let Some(pack) = pack {
            match keep_preset(&pack) {
                Ok(Some(name)) => info!("the host's game data is the preset '{name}'"),
                Ok(None) => {}
                Err(e) => warn!("could not save the host's game data '{}': {e}", pack.name),
            }
        }
    });
}

enum Role {
    /// `waiting`: after taking over from a host that left, the players still expected to
    /// reconnect; the game waits for them up to `wait_left` seconds. `ai_reclaim`: the lobby's
    /// AI option, passed on to whoever takes over next.
    Host { host: Host, remote: Vec<(u32, u8)>, sim: HostSim, waiting: Vec<u8>, wait_left: f32, ai_reclaim: bool },
    /// `heard`: seconds since the last frame from the host. `rejoin`: after the host left,
    /// the connection being made to the player taking over (that player, the result to come).
    /// `skip`: players who could not take over.
    Client { client: Client, sim: ClientSim, heard: f32, rejoin: Option<(u8, std::sync::mpsc::Receiver<Result<Client, String>>)>, skip: Vec<u8> },
    /// Playing back a replay: no network, the commands come from the file.
    Replay(Box<ReplayRun>),
}

/// A replay being watched (`replay::Playback` plus the controls' state).
struct ReplayRun {
    pb: Playback,
    speed: f32,
    paused: bool,
    /// Jumping to this tick.
    seek: Option<u32>,
}

/// A host that sends nothing for this long (turns arrive ten times a second, pongs every
/// second even while paused) is taken for gone.
const HOST_SILENT_SECS: f32 = 10.0;
/// How long the player taking over as host waits for the others to reconnect.
const REJOIN_WAIT_SECS: f32 = 20.0;

/// The host left and this client is next in line: it becomes the host, from the last state the
/// old host confirmed. The AIs start again there (their few own variables are not part of the
/// game state), the old host's player resigns, and the other players get `REJOIN_WAIT_SECS`
/// to reconnect.
fn take_over(world: &mut World, sim: &mut ClientSim, me: u8, name: &str) -> Result<Role, String> {
    let host = Host::start(name)?;
    info!("the host left; this machine takes over as host");
    let snap = sim.host_lost().clone();
    restore(world, &snap);
    flush_limbo(world);
    let seed = (snap.game.tick as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let ais = world.resource::<Game>().players.iter().enumerate().filter(|(_, pl)| pl.is_ai && pl.alive).map(|(k, pl)| {
        let mut ai = Ai::with_kind(k as u8, seed, AiKind::from_id(pl.ai_kind));
        ai.reclaim = sim.ai_reclaim;
        ai
    });
    let ais = Ais(ais.collect());
    world.insert_resource(ais);
    let mut hsim = HostSim::new(world);
    if let Some(old) = sim.host_player {
        hsim.command(world, old, 0, 0, Command::Resign);
    }
    host.set_started(&world.resource::<MapData>().name);
    let waiting: Vec<u8> = sim.peers.iter().map(|p| p.0).filter(|&p| p != me).collect();
    Ok(Role::Host { host, remote: vec![], sim: hsim, waiting, wait_left: REJOIN_WAIT_SECS, ai_reclaim: sim.ai_reclaim })
}

/// Tells every client who would take over if this host left.
fn send_peers(host: &Host, remote: &[(u32, u8)], me: u8, ai_reclaim: bool) {
    // Observers never take over: they are not in the list (they still get it).
    let peers: Vec<(u8, Option<std::net::IpAddr>)> = remote.iter().filter(|r| r.1 != OBSERVER).map(|&(conn, p)| (p, host.peer_ip(conn))).collect();
    for (conn, _) in remote {
        host.send_msg(*conn, &S2C::Peers { host: me, peers: peers.clone(), ai_reclaim });
    }
}

struct Session {
    role: Role,
    acc: f32,
    /// Host: the game is paused, by that player.
    paused: Option<u8>,
    /// Host: commands (with their player) that came in while no tick was due; the next tick takes them.
    held: Vec<(u8, Command)>,
    /// The replay being recorded (`Settings::save_replays`).
    rec: Option<Recorder>,
}

/// Starts recording a game that begins with `map` and `state`, as player `me` sees it
/// (none when replays are switched off in the settings, or `LR_NOREPLAY` is set).
fn start_recording(world: &World, map: &MapData, state: &Snapshot, me: u8) -> Option<Recorder> {
    if !world.resource::<longrange::settings::Settings>().save_replays || std::env::var("LR_NOREPLAY").is_ok() || world.contains_resource::<Bench>() {
        return None;
    }
    let pack = (*world.resource::<Defs>().pack).clone();
    let info = ReplayInfo::new(&state.game, &map.name, &pack.name, me);
    let start = ReplayStart { map: map.clone(), pack, state: state.clone() };
    match Recorder::start(&replay::dir(), &info, &start) {
        Ok(r) => Some(r),
        Err(e) => {
            warn!("cannot record a replay in {}: {e}", replay::dir().display());
            None
        }
    }
}

/// Opens a replay file for watching: its game data, the map and the starting state, seen by everybody.
fn start_replay(world: &mut World, path: &std::path::Path, base: &Defs) -> Result<Session, String> {
    let l = replay::load(path)?;
    let defs = Defs::from_pack(l.start.pack.clone())?;
    // Model and picture files a preset brought along (a client's recording of a host's preset).
    if !l.start.pack.files.is_empty() {
        l.start.pack.store_assets(&asset_root());
    }
    world.insert_resource(if defs.hash == base.hash { base.clone() } else { defs });
    let pb = Playback::begin(world, l);
    view::start_view(world, OBSERVER, true);
    Ok(Session { role: Role::Replay(Box::new(ReplayRun { pb, speed: 1.0, paused: false, seek: None })), acc: 0.0, paused: None, held: vec![], rec: None })
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
    /// Edit the raw JSON instead of the form.
    raw: bool,
    /// `text` parsed, for the form (`Null` when it does not parse).
    value: serde_json::Value,
    /// Names the form's drop-down lists offer.
    ctx: longrange::preset_form::FormCtx,
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
        self.ctx = longrange::preset_form::FormCtx::from_pack(pack);
        if self.file == 0 || self.file > pack.factions.len() {
            self.file = 0;
            self.text = pack.rules.clone();
            self.value = serde_json::from_str(&self.text).unwrap_or_default();
            return;
        }
        let v: serde_json::Value = serde_json::from_str(&pack.factions[self.file - 1].1).unwrap_or_default();
        let list = v["units"].as_array().cloned().unwrap_or_default();
        self.units = list.iter().map(|u| (u["id"].as_str().unwrap_or("?").to_string(), u["name"].as_str().unwrap_or("").to_string())).collect();
        self.unit = self.unit.min(list.len().saturating_sub(1));
        self.text = list.get(self.unit).map(|u| serde_json::to_string_pretty(u).unwrap_or_default()).unwrap_or_default();
        self.value = serde_json::from_str(&self.text).unwrap_or_default();
    }
    /// The form changed `value`: the text follows it (that is what `edited` and saving read).
    fn form_changed(&mut self) {
        self.text = serde_json::to_string_pretty(&self.value).unwrap_or_default();
        if self.file > 0 {
            if let Some(u) = self.units.get_mut(self.unit) {
                *u = (self.value["id"].as_str().unwrap_or("?").to_string(), self.value["name"].as_str().unwrap_or("").to_string());
            }
        }
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
            pack.save()?;
            Ok(pack)
        });
        match done {
            Ok(pack) => {
                self.msg = tr!("preset_editor.saved", pack.name);
                self.pack = Some(pack);
                self.show();
            }
            Err(e) => self.msg = tr!("preset_editor.not_saved", e),
        }
    }
    /// Adds a copy of the open unit behind it, or removes the open unit (kept in memory until saved).
    fn change_units(&mut self, copy: bool) {
        let Ok(mut pack) = self.edited() else {
            self.msg = tr!("preset_editor.fix_json_text").into();
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
            self.msg = tr!("preset_editor.unit_copied").into();
        } else {
            list.remove(self.unit);
            self.msg = tr!("preset_editor.unit_removed").into();
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
    /// The list of games to join.
    Browse(Browser),
    /// The saved replays.
    Replays(ReplayList),
    Game(Session),
}

/// The game browser: games hosts on the network announce, refreshed every second.
struct Browser {
    disc: Discovery,
    selected: Option<std::net::IpAddr>,
    /// Map pictures by host (keyed by a checksum of the pixels).
    thumbs: std::collections::HashMap<std::net::IpAddr, (u64, egui::TextureHandle)>,
    /// Whether a premade map is on this computer, by folder name (asked again on refresh).
    have_map: std::collections::HashMap<String, bool>,
}

impl Browser {
    fn new() -> Browser {
        Browser { disc: Discovery::new(), selected: None, thumbs: Default::default(), have_map: Default::default() }
    }
    fn has_map(&mut self, name: &str) -> bool {
        *self.have_map.entry(name.to_string()).or_insert_with(|| longrange::data::map_dir(name).join("map.json").exists())
    }
}

/// The replay browser: the files in `replay::dir()`, newest first.
struct ReplayList {
    items: Vec<replay::Summary>,
    selected: Option<usize>,
    /// The delete button was pressed once: the next press deletes.
    confirm: bool,
}

impl ReplayList {
    fn new() -> ReplayList {
        let items = replay::list(&replay::dir());
        ReplayList { selected: (!items.is_empty()).then_some(0), items, confirm: false }
    }
}

enum ReplayAction {
    Watch(std::path::PathBuf),
    Back,
}

/// "3 min ago", or the date (UTC) for anything older than a week.
fn replay_age(date: u64) -> String {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let ago = now.saturating_sub(date);
    if ago < 60 {
        tr!("replays.just_now").into()
    } else if ago < 3600 {
        tr!("replays.minutes_ago", ago / 60)
    } else if ago < 86400 {
        tr!("replays.hours_ago", ago / 3600)
    } else if ago < 7 * 86400 {
        tr!("replays.days_ago", ago / 86400)
    } else {
        let (y, m, d, h, mi, _) = replay::utc(date);
        format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02}")
    }
}

/// The players of a replay, team by team: "A, B vs C".
fn replay_players(info: &replay::ReplayInfo) -> String {
    let mut teams: Vec<u8> = info.players.iter().map(|p| p.team).collect();
    teams.sort();
    teams.dedup();
    teams.iter().map(|&t| info.players.iter().filter(|p| p.team == t).map(|p| p.name.as_str()).collect::<Vec<_>>().join(", ")).collect::<Vec<_>>().join(tr!("replays.versus"))
}

fn replays_ui(ui: &mut egui::Ui, l: &mut ReplayList, message: &str, width: f32, height: f32) -> Option<ReplayAction> {
    use egui::Color32;
    let mut act = None;
    ui.label(egui::RichText::new(tr!("replays.title")).size(34.0).color(Color32::from_rgb(255, 215, 100)));
    ui.label(egui::RichText::new(tr!("replays.folder", replay::dir().display())).size(12.0).weak());
    ui.add_space(8.0);
    if l.items.is_empty() {
        ui.add_space(30.0);
        ui.label(tr!("replays.none"));
        ui.add_space(30.0);
    } else {
        let clock = |t: u32| {
            let s = t / 10;
            if s >= 3600 {
                format!("{}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
            } else {
                format!("{}:{:02}", s / 60, s % 60)
            }
        };
        egui::ScrollArea::vertical().max_height((height - 170.0).max(200.0)).auto_shrink([false, true]).show(ui, |ui| {
            ui.set_width(width);
            egui::Grid::new("replays").striped(true).num_columns(6).spacing([16.0, 6.0]).show(ui, |ui| {
                for h in [tr!("replays.col_date"), tr!("replays.col_map"), tr!("replays.col_players"), tr!("replays.col_length"), tr!("replays.col_result"), tr!("replays.col_version")] {
                    ui.label(egui::RichText::new(h).strong());
                }
                ui.end_row();
                for (k, r) in l.items.iter().enumerate() {
                    let (y, m, d, h, mi, _) = replay::utc(r.info.date);
                    let resp = ui.selectable_label(l.selected == Some(k), replay_age(r.info.date)).on_hover_text(format!("{y:04}-{m:02}-{d:02} {h:02}:{mi:02} UTC\n{}", r.name()));
                    if resp.clicked() {
                        l.selected = Some(k);
                        l.confirm = false;
                    }
                    if resp.double_clicked() {
                        act = Some(ReplayAction::Watch(r.path.clone()));
                    }
                    ui.label(&r.info.map);
                    ui.add(egui::Label::new(replay_players(&r.info)).truncate()).on_hover_text(replay_players(&r.info));
                    ui.label(clock(r.ticks));
                    match r.winner {
                        Some(w) => {
                            let names: Vec<&str> = r.info.players.iter().filter(|p| p.team == w).map(|p| p.name.as_str()).collect();
                            ui.label(tr!("replays.won_by", names.join(", ")));
                        }
                        None if !r.finished => {
                            ui.label(egui::RichText::new(tr!("replays.cut_off")).weak()).on_hover_text(tr!("replays.cut_off_tip"));
                        }
                        None => {
                            ui.label("-");
                        }
                    }
                    if r.same_version() {
                        ui.label(&r.info.version);
                    } else {
                        ui.colored_label(Color32::from_rgb(255, 170, 90), &r.info.version).on_hover_text(tr!("replays.other_version_tip"));
                    }
                    ui.end_row();
                }
            });
        });
    }
    ui.add_space(10.0);
    let sel = l.selected.filter(|&k| k < l.items.len());
    ui.horizontal(|ui| {
        if ui.add_enabled(sel.is_some(), egui::Button::new(egui::RichText::new(tr!("replays.watch")).size(17.0)).min_size(egui::vec2(180.0, 38.0))).clicked() {
            if let Some(k) = sel {
                act = Some(ReplayAction::Watch(l.items[k].path.clone()));
            }
        }
        let del = if l.confirm { tr!("replays.delete_confirm") } else { tr!("replays.delete") };
        if ui.add_enabled(sel.is_some(), egui::Button::new(del).min_size(egui::vec2(150.0, 38.0))).clicked() {
            if let Some(k) = sel {
                if l.confirm {
                    let _ = std::fs::remove_file(&l.items[k].path);
                    l.items.remove(k);
                    l.selected = if l.items.is_empty() { None } else { Some(k.min(l.items.len() - 1)) };
                    l.confirm = false;
                } else {
                    l.confirm = true;
                }
            }
        }
        if ui.add(egui::Button::new(tr!("replays.refresh")).min_size(egui::vec2(120.0, 38.0))).clicked() {
            *l = ReplayList::new();
        }
        if ui.add(egui::Button::new(tr!("common.back")).min_size(egui::vec2(120.0, 38.0))).clicked() {
            act = Some(ReplayAction::Back);
        }
    });
    // Delete / Refresh above may have shrunk the list: look the selection up again.
    if let Some(r) = l.selected.and_then(|k| l.items.get(k)) {
        if !r.same_version() {
            ui.colored_label(Color32::from_rgb(255, 170, 90), tr!("replays.other_version", &r.info.version));
        }
    }
    if !message.is_empty() {
        ui.add_space(8.0);
        ui.colored_label(Color32::from_rgb(255, 120, 105), message);
    }
    act
}

/// Top-level application state (kept off the ECS threads: it owns sockets).
struct AppState {
    screen: Screen,
    name: String,
    ip: String,
    message: String,
    skirmish: bool,
    settings_open: bool,
    /// The "Check for updates" window (main menu), with its check or download running in the background.
    update: Option<longrange::update::Updater>,
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
    let mut slots: Vec<Slot> = (0..4).map(|k| Slot { kind: 0, name: String::new(), team: k, faction, color: k, ai: AI_KINDS[0], conn: 0, hash: 0 }).collect();
    slots[0].kind = 2;
    slots[0].name = name.to_string();
    slots[2].kind = 3;
    let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(1) | 1;
    let (maps, counts): (Vec<String>, Vec<usize>) = longrange::data::map_entries(&asset_root()).into_iter().unzip();
    let thumbs = vec![None; maps.len()];
    let mut l = HostLobby {
        host,
        slots,
        observers: vec![],
        map_choice: 1,
        maps,
        players: 4,
        dirty: true,
        sent_preview: vec![],
        lan_ip: local_ip(),
        mode: MODE_ANNIHILATION,
        cp_start: 0,
        preset: 0,
        presets: DataPack::list(&asset_root()),
        seed,
        map: None,
        preview: vec![],
        starts: vec![],
        points: vec![],
        tex: None,
        thumbs,
        tab: 0,
        reveal_props: true,
        infantry_capture: true,
        eco_public: false,
        rubble_blocks: defs.rules.rubble_blocks_paths,
        weather: 0,
        counts,
        biome: 0,
    };
    let _ = l.ensure_map(defs);
    Ok(l)
}

/// `LR_WEATHER=<id>`: the weather of a test or skirmish game, by its id in `rules.weather`.
fn env_weather(defs: &Defs) -> Option<u8> {
    let id = std::env::var("LR_WEATHER").ok()?;
    let k = defs.rules.weather.iter().position(|w| w.id == id);
    if k.is_none() {
        eprintln!("LR_WEATHER: no weather '{id}' (have: {})", defs.rules.weather.iter().map(|w| w.id.as_str()).collect::<Vec<_>>().join(", "));
    }
    k.map(|k| k as u8)
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
    let ai_reclaim = defs.rules.ai_reclaim;
    for (k, s) in l.slots.iter().enumerate() {
        if s.kind < 2 || k >= map.starts.len() {
            continue;
        }
        let idx = setup.len() as u8;
        // LR_WATCH=1 lets an AI play the local slot too (spectating / testing).
        // So does the benchmark (`--bench`).
        if s.kind == 3 || (s.conn == 0 && (std::env::var("LR_WATCH").is_ok() || world.contains_resource::<Bench>())) {
            let mut ai = Ai::with_kind(idx, seed, s.ai);
            ai.reclaim = ai_reclaim;
            ais.push(ai);
        } else if s.conn != 0 {
            remote.push((s.conn, idx));
            hashes.push((s.conn, s.hash));
        }
        let name = if s.kind == 3 { format!("AI {} ({})", k + 1, s.ai.name()) } else { s.name.clone() };
        setup.push(PlayerSetup { name, team: s.team, is_ai: s.kind == 3, slot: k, color: s.color, faction: s.faction });
    }
    // The host's own player, or `OBSERVER` when the host only watches.
    let me = l.slots.iter().enumerate().filter(|(k, s)| s.kind >= 2 && *k < map.starts.len()).position(|(_, s)| s.kind == 2 && s.conn == 0).map_or(OBSERVER, |k| k as u8);
    for o in l.observers.iter().filter(|o| o.conn != 0) {
        remote.push((o.conn, OBSERVER));
        hashes.push((o.conn, o.hash));
    }
    // Deposits attached to starts nobody plays on are left out (the same in every copy of the game).
    let mut map = map;
    map.adapt(&setup.iter().map(|p| p.slot).collect::<Vec<_>>());
    let sent_map = map.clone();
    setup_game(world, map, &setup);
    if l.mode == MODE_POINTS {
        let start = l.cp_points(world.resource::<Defs>());
        enable_control_points(world, start);
    }
    world.resource_mut::<Game>().reveal_props = l.reveal_props;
    world.resource_mut::<Game>().infantry_capture = l.infantry_capture;
    world.resource_mut::<Game>().eco_public = l.eco_public;
    world.resource_mut::<Game>().rubble_blocks = l.rubble_blocks;
    let weather = env_weather(world.resource::<Defs>()).unwrap_or_else(|| l.weather_index(world.resource::<Defs>()));
    world.resource_mut::<Game>().weather = weather;
    for ai in &ais {
        world.resource_mut::<Game>().players[ai.player as usize].ai_kind = ai.kind.id();
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
    let start_state = make_snapshot(world);
    let rec = start_recording(world, &sent_map, &start_state, me);
    // Clients get the map and the complete starting state; from then on only commands travel.
    if !remote.is_empty() {
        let state = frame(TAG_SNAP, bincode::serialize(&start_state).map_err(|e| e.to_string())?);
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
    send_peers(&l.host, &remote, me, ai_reclaim);
    let sim = HostSim::new(world);
    l.host.set_started("");
    view::start_view(world, me, true);
    Ok(Session { role: Role::Host { host: l.host, remote, sim, waiting: vec![], wait_left: 0.0, ai_reclaim }, acc: 0.0, paused: None, held: vec![], rec })
}

/// One frame of a replay: plays on at its speed, jumps where the time line asks, and keeps the
/// HUD's replay bar up to date.
fn replay_update(world: &mut World, r: &mut ReplayRun, acc: &mut f32, toggle: bool) {
    // Simulation time per frame is limited, so a fast speed in a big game slows down instead of freezing.
    const BUDGET: std::time::Duration = std::time::Duration::from_millis(40);
    let dt = world.resource::<Time>().delta_secs().min(0.25);
    if toggle {
        r.paused = !r.paused;
        // Play at the end: from the start again.
        if !r.paused && Playback::tick(world) >= r.pb.end {
            r.seek = Some(r.pb.first);
        }
    }
    let mut vision = None;
    if let Some(mut v) = world.get_resource_mut::<ViewState>() {
        if let Some(bar) = v.replay.as_mut() {
            if let Some(t) = bar.want_seek.take() {
                r.seek = Some(t);
            }
            if let Some(sp) = bar.want_speed.take() {
                r.speed = sp;
            }
            vision = bar.want_vision.take();
        }
    }
    if let Some(p) = vision {
        // Another player's view: their fog, radar and what they remember (from now on).
        let n = world.resource::<MapData>().dims();
        {
            let mut v = world.resource_mut::<ViewState>();
            v.me = p;
            v.sel.clear();
        }
        world.insert_resource(view::intel::IntelMemory::default());
        world.insert_resource(view::intel::FogGrid::new(n));
    }
    let clock = std::time::Instant::now();
    if let Some(t) = r.seek {
        let t = t.clamp(r.pb.first, r.pb.end);
        r.pb.rewind(world, t);
        while Playback::tick(world) < t && clock.elapsed() < BUDGET {
            r.pb.step(world, true);
        }
        if Playback::tick(world) >= t {
            r.seek = None;
        }
        *acc = 0.0;
    } else if !r.paused {
        *acc += dt * r.speed;
        while *acc >= DT && Playback::tick(world) < r.pb.end && clock.elapsed() < BUDGET {
            *acc -= DT;
            r.pb.step(world, false);
        }
        if *acc > DT * 3.0 {
            *acc = 0.0;
        }
        if Playback::tick(world) >= r.pb.end {
            r.paused = true;
            *acc = 0.0;
        }
    }
    flush_limbo(world);
    let tick = Playback::tick(world);
    if let Some(mut v) = world.get_resource_mut::<ViewState>() {
        v.paused = r.paused.then_some(OBSERVER);
        let bar = v.replay.get_or_insert_with(Default::default);
        bar.tick = tick;
        bar.first = r.pb.first;
        bar.end = r.pb.end;
        bar.speed = r.speed;
        bar.paused = r.paused;
        bar.seeking = r.seek.is_some();
        bar.desync = r.pb.desync;
    }
}

fn leave_game(world: &mut World) {
    view::stop_view(world);
    clear_game(world);
    world.remove_resource::<Ais>();
    world.remove_resource::<ClientAi>();
    world.remove_resource::<TurnLog>();
}

/// Runs the simulation once per frame before rendering: the host at its own pace, a client
/// predicting ahead of the turns the host confirms (`netplay.rs`).
/// The benchmark's fast-forward: every 5 game minutes, what the ticks cost (`SimProfile`).
fn bench_ff_report(world: &mut World) {
    let tick = world.resource::<Game>().tick;
    if tick % 3000 != 0 {
        return;
    }
    let units = world.query::<&Unit>().iter(world).count();
    let wrecks = world.query::<&Wreck>().iter(world).count();
    let shells = world.query::<&Projectile>().iter(world).count();
    let Some(mut p) = world.get_resource_mut::<longrange::sim::systems::SimProfile>() else { return };
    let line = format!("{:2} min, {units} units, {wrecks} wrecks, {shells} shells: {}", tick / 600, p.short(5));
    *p = Default::default();
    println!("bench: {line}");
    world.resource_mut::<Bench>().ff_lines.push(line);
}

fn session_update(world: &mut World) {
    let t0 = std::time::Instant::now();
    session_frame(world);
    if let Some(mut p) = world.get_resource_mut::<view::perf::FrameProfile>() {
        p.add(view::perf::SESSION, t0.elapsed());
    }
}

fn session_frame(world: &mut World) {
    let Some(mut app) = world.remove_non_send::<AppState>() else { return };
    let mut over: Option<String> = None;
    let mut promoted: Option<Role> = None;
    if let Screen::Game(s) = &mut app.screen {
        let me = world.get_resource::<ViewState>().map_or(0, |v| v.me);
        let local: Vec<Command> = world.get_resource_mut::<LocalCommands>().map(|mut c| c.0.drain(..).collect()).unwrap_or_default();
        let toggle = world.get_resource_mut::<ViewState>().map_or(false, |mut v| std::mem::take(&mut v.pause_toggle));
        // Pings and drawings: the local player's go out, the others' are shown (`view::marks`).
        let my_marks: Vec<Mark> = world.get_resource_mut::<view::marks::Marks>().map(|mut m| std::mem::take(&mut m.out)).unwrap_or_default();
        let mut heard_marks: Vec<(u8, Mark)> = vec![];
        match &mut s.role {
            Role::Host { host, remote, sim, waiting, wait_left, ai_reclaim } => {
                let mut cmds: Vec<(u8, Command)> = std::mem::take(&mut s.held);
                if me != OBSERVER {
                    cmds.extend(local.into_iter().map(|c| (me, c)));
                }
                if me != OBSERVER {
                    for m in &my_marks {
                        for (conn, _) in remote.iter() {
                            host.send_msg(*conn, &S2C::Mark(me, m.clone()));
                        }
                    }
                }
                // Pause requests: (player, pause or resume).
                let mut pause: Vec<(u8, bool)> = vec![];
                if toggle {
                    pause.push((me, s.paused.is_none()));
                }
                while let Ok(ev) = host.events.try_recv() {
                    match ev {
                        HostEvent::Msg(id, C2S::Cmd { tick, seq, cmd }) => {
                            if let Some((_, p)) = remote.iter().find(|r| r.0 == id && r.1 != OBSERVER) {
                                sim.command(world, *p, tick, seq, cmd);
                            }
                        }
                        HostEvent::Msg(id, C2S::Ping(t)) => host.send_msg(id, &S2C::Pong(t)),
                        HostEvent::Msg(id, C2S::Mark(m)) => {
                            if let Some(m) = remote.iter().find(|r| r.0 == id && r.1 != OBSERVER).and_then(|r| Some((r.1, view::marks::sanitize(m, world.resource::<MapData>())?))) {
                                for (conn, _) in remote.iter().filter(|r| r.0 != id) {
                                    host.send_msg(*conn, &S2C::Mark(m.0, m.1.clone()));
                                }
                                heard_marks.push(m);
                            }
                        }
                        HostEvent::Msg(id, C2S::Pause(on)) => {
                            if let Some((_, p)) = remote.iter().find(|r| r.0 == id && r.1 != OBSERVER) {
                                pause.push((*p, on));
                            }
                        }
                        HostEvent::Msg(id, C2S::Hash { tick, hash }) => {
                            if let Some((_, p)) = remote.iter().find(|r| r.0 == id) {
                                let now = world.resource::<Game>().tick;
                                if !sim.matches(tick, hash) && sim.repair_due(id, tick, now) {
                                    // That machine computed something else: hand it our state.
                                    warn!("player {p} diverged at tick {tick}; sending the full state");
                                    if let Ok(data) = bincode::serialize(&make_snapshot(world)) {
                                        host.send(id, &frame(TAG_SNAP, data));
                                    }
                                }
                            }
                        }
                        HostEvent::Msg(id, C2S::Rejoin { player }) => {
                            // A player of this game, after its host left and this machine took over.
                            // Observers are not waited for, but welcome back.
                            if let Some(k) = waiting.iter().position(|&w| w == player).or((player == OBSERVER).then_some(usize::MAX)) {
                                if k != usize::MAX {
                                    waiting.remove(k);
                                }
                                info!("player {player} reconnected");
                                remote.push((id, player));
                                if let Ok(data) = bincode::serialize(&make_snapshot(world)) {
                                    host.send(id, &frame(TAG_SNAP, data));
                                }
                                if s.paused.is_some() {
                                    host.send_msg(id, &S2C::Paused(s.paused));
                                }
                                send_peers(host, remote, me, *ai_reclaim);
                            } else {
                                host.send_msg(id, &S2C::Kicked(tx!("connection.kicked_not_in_game").into()));
                            }
                        }
                        HostEvent::Msg(id, C2S::Hello { .. }) => host.send_msg(id, &S2C::Kicked(tx!("connection.kicked_started").into())),
                        HostEvent::Left(id) => {
                            if let Some(k) = remote.iter().position(|r| r.0 == id) {
                                if remote[k].1 != OBSERVER {
                                    info!("player {} left; the game goes on without them", remote[k].1);
                                    cmds.push((remote[k].1, Command::Resign));
                                }
                                remote.remove(k);
                                send_peers(host, remote, me, *ai_reclaim);
                            }
                        }
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
                let mut speed = std::env::var("LR_SPEED").ok().and_then(|s| s.parse::<f32>().ok()).unwrap_or(1.0).clamp(0.1, 20.0);
                // A game of AIs only: the host sets the speed in the HUD.
                let ai_only = world.resource::<Game>().players.iter().all(|p| p.is_ai);
                if let Some(mut v) = world.get_resource_mut::<ViewState>() {
                    v.host_speed_ui = ai_only;
                    if ai_only {
                        speed *= v.host_speed;
                    }
                }
                let dt = world.resource::<Time>().delta_secs().min(0.25);
                s.acc += dt * speed;
                if s.paused.is_some() {
                    s.acc = 0.0;
                }
                // Taken over from a host that left: hold the game until the others are back
                // (they reconnect with the full state), or resign those who do not come.
                let mut note = None;
                if !waiting.is_empty() {
                    *wait_left -= dt;
                    s.acc = 0.0;
                    if *wait_left <= 0.0 {
                        for p in waiting.drain(..) {
                            warn!("player {p} did not reconnect");
                            cmds.push((p, Command::Resign));
                        }
                        s.acc = DT;
                    } else {
                        let game = world.resource::<Game>();
                        let names: Vec<String> = waiting.iter().map(|&p| game.players.get(p as usize).map_or(tr!("connection.player_fallback", p), |pl| pl.name.clone())).collect();
                        note = Some(tr!("connection.host_left_now_host", names.join(", "), wait_left.ceil()));
                    }
                }
                if let Some(mut v) = world.get_resource_mut::<ViewState>() {
                    v.net_note = note;
                }
                let mut cap = (3.0 * speed).ceil() as i32;
                // The benchmark: ticks flat out for up to 100 ms a frame, or none at all.
                let (ff, hold) = world.get_resource::<Bench>().map_or((false, false), |b| (b.ff, b.hold));
                let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
                if hold {
                    s.acc = 0.0;
                } else if ff {
                    s.acc = DT * 1e6;
                    cap = i32::MAX;
                }
                let mut n = 0;
                while s.acc >= DT && n < cap {
                    if ff && std::time::Instant::now() > deadline {
                        s.acc = 0.0;
                        break;
                    }
                    s.acc -= DT;
                    n += 1;
                    let turn = sim.tick(world, std::mem::take(&mut cmds), !remote.is_empty());
                    if ff {
                        bench_ff_report(world);
                    }
                    if let (Some(rec), S2C::Turn { tick, cmds }) = (s.rec.as_mut(), &turn) {
                        rec.push(*tick, cmds.iter().map(|(p, _, c)| (*p, c.clone())).collect());
                        if tick % replay::HASH_EVERY == 0 {
                            rec.hash(*tick, state_hash(&make_snapshot(world)));
                        }
                    }
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
                // Commands given while no tick was due wait for the next one, each still under its
                // own player (a leaver's `Resign` must not become the host's).
                s.held = cmds;
            }
            Role::Client { client, sim, heard, rejoin, skip } => {
                if let Some(mut v) = world.get_resource_mut::<ViewState>() {
                    v.host_speed_ui = false;
                }
                let dt = world.resource::<Time>().delta_secs().min(0.25);
                let mut lost = false;
                let mut note = None;
                if let Some((to, rx)) = rejoin {
                    // The host left: connecting to the player who takes over.
                    let name = world.resource::<Game>().players.get(*to as usize).map_or(String::new(), |p| p.name.clone());
                    note = Some(tr!("connection.host_left_reconnecting_to", name));
                    match rx.try_recv() {
                        Ok(Ok(c)) => {
                            info!("connected to the new host (player {to})");
                            *client = c;
                            *rejoin = None;
                            *heard = 0.0;
                        }
                        Ok(Err(_)) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            warn!("player {to} did not take over as host");
                            skip.push(*to);
                            *rejoin = None;
                            lost = true;
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => {}
                    }
                } else {
                    if toggle && me != OBSERVER {
                        client.send(&C2S::Pause(sim.paused.is_none()));
                    }
                    let mut local = if me == OBSERVER { vec![] } else { local };
                    // LR_WATCH=1 on a client: an AI plays this player too (testing).
                    if me != OBSERVER && std::env::var("LR_WATCH").is_ok() {
                        let mut st = world.remove_resource::<ClientAi>().unwrap_or(ClientAi(Some(Ai::with_kind(me, 7, AI_KINDS[0])), u32::MAX));
                        local.extend(client_ai(world, &mut st.0, &mut st.1));
                        world.insert_resource(st);
                    }
                    for m in sim.issue(world, local) {
                        client.send(&m);
                    }
                    if me != OBSERVER {
                        for m in my_marks {
                            client.send(&C2S::Mark(m));
                        }
                    }
                    *heard += dt;
                    loop {
                        match client.frames.try_recv() {
                            Ok(Some((tag, data))) => {
                                *heard = 0.0;
                                if let Err(why) = sim.receive(tag, &data) {
                                    over = Some(why);
                                    break;
                                }
                            }
                            Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                lost = true;
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                    if *heard > HOST_SILENT_SECS || client.broken() {
                        lost = true;
                    }
                    if lost {
                        warn!("lost the connection to the host");
                        sim.host_lost();
                    }
                    if over.is_none() && !lost {
                        match sim.update(world, dt) {
                            Ok(out) => {
                                for m in out {
                                    client.send(&m);
                                }
                            }
                            Err(why) => over = Some(why),
                        }
                    }
                    // The turns the host confirmed go into the replay.
                    if let Some(rec) = s.rec.as_mut() {
                        if let Some(t) = sim.rewound.take() {
                            rec.rewind(t);
                        }
                        for (t, c) in sim.log.drain(..) {
                            rec.push(t, c);
                        }
                        for (t, h) in sim.log_hashes.drain(..) {
                            rec.hash(t, h);
                        }
                    } else {
                        sim.log.clear();
                        sim.log_hashes.clear();
                    }
                    heard_marks.extend(sim.marks.drain(..));
                }
                // Who takes over: the human player with the lowest number who has not failed
                // to already. Every client works it out from the same list, so they agree.
                if lost && over.is_none() {
                    match sim.peers.iter().find(|p| !skip.contains(&p.0)).copied() {
                        None => over = Some(tr!("connection.host_lost").into()),
                        Some((p, _)) if p == me => match take_over(world, sim, me, &app.name) {
                            Ok(role) => promoted = Some(role),
                            Err(e) => over = Some(tr!("connection.host_left_takeover_failed", e)),
                        },
                        Some((p, ip)) => {
                            let ip = ip.unwrap_or(client.addr.ip());
                            info!("the host left; reconnecting to player {p} at {ip}");
                            *rejoin = Some((p, Client::rejoin(ip, me, std::time::Duration::from_secs(15))));
                            note = Some(tr!("connection.host_left_reconnecting").into());
                        }
                    }
                }
                if let Some(mut v) = world.get_resource_mut::<ViewState>() {
                    v.paused = sim.paused;
                    v.net_note = note;
                }
            }
            Role::Replay(r) => replay_update(world, r, &mut s.acc, toggle),
        }
        view::marks::heard(world, heard_marks);
        if let Some(role) = promoted {
            s.role = role;
            s.paused = None;
            s.held.clear();
            s.acc = 0.0;
        }
        if world.get_resource::<ViewState>().map_or(false, |v| v.quit) {
            over = Some(String::new());
        }
    }
    if let Some(msg) = over {
        if let Screen::Game(s) = &mut app.screen {
            if let Some(rec) = s.rec.as_mut() {
                rec.finish(world.get_resource::<Game>().and_then(|g| g.winner));
            }
        }
        // Back from watching a replay: to the replay list.
        let watched = matches!(&app.screen, Screen::Game(s) if matches!(s.role, Role::Replay(_)));
        leave_game(world);
        restore_defs(world, &app.base_defs);
        app.message = msg;
        app.screen = if watched { Screen::Replays(ReplayList::new()) } else { Screen::Menu };
    }
    world.insert_non_send(app);
}

enum ObserverClick {
    /// Move yourself to the observers (`true`) or into an open slot.
    Watch(bool),
    /// The host removes that observer.
    Remove(usize),
}

/// The observers under the slots, with the button that moves you between a slot and them.
/// `playing` / `watching`: where you are now; `open`: a slot is free.
fn observer_rows(ui: &mut egui::Ui, names: &[String], playing: bool, watching: bool, open: bool, host: bool, width: f32) -> Option<ObserverClick> {
    let mut clicked = None;
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(tr!("lobby.observers", names.len(), MAX_OBSERVERS)).size(16.0));
        if playing && names.len() < MAX_OBSERVERS && ui.button(tr!("lobby.watch_instead")).on_hover_text(tr!("lobby.watch_tip")).clicked() {
            clicked = Some(ObserverClick::Watch(true));
        }
        if watching && ui.add_enabled(open, egui::Button::new(tr!("lobby.take_a_slot"))).on_hover_text(tr!("lobby.take_slot_tip")).on_disabled_hover_text(tr!("lobby.no_slot_is_open")).clicked() {
            clicked = Some(ObserverClick::Watch(false));
        }
    });
    if names.is_empty() {
        ui.label(egui::RichText::new(tr!("lobby.nobody")).weak());
    }
    for (k, n) in names.iter().enumerate() {
        let b = ui.add_sized([(width * 0.5).max(200.0), 26.0], egui::Button::new(n));
        if host && b.on_hover_text(tr!("lobby.remove_tip")).clicked() {
            clicked = Some(ObserverClick::Remove(k));
        }
    }
    clicked
}

/// What a slot's entry list can do.
#[derive(Clone, Copy, PartialEq)]
enum SlotChoice {
    Open,
    Closed,
    Ai(u8),
    /// Swap this slot's player with slot `j`.
    Move(usize),
    /// Move yourself into this open slot.
    Take,
}

enum SlotEdit {
    Choice(SlotChoice),
    Team(u8),
    Color(u8),
    Faction,
}

fn rgb8(c: [f32; 3]) -> egui::Color32 {
    egui::Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8)
}

/// The slot list: per slot its entry (open / closed / AI / player, with moves), team, colour and
/// faction. `host` may change everything; others only their own team, colour and faction, and
/// move into open slots.
fn slot_rows(ui: &mut egui::Ui, defs: &Defs, slots: &[SlotInfo], you: i8, observing: bool, host: bool, width: f32) -> Option<(usize, SlotEdit)> {
    let mut clicked = None;
    let team_w = 56.0;
    let color_w = 70.0;
    let faction_w = (width * 0.2).max(120.0);
    let name_w = (width - team_w - color_w - faction_w - 130.0).max(160.0);
    let palette = &defs.rules.player_colors;
    egui::Grid::new("slots").num_columns(5).spacing([10.0, 8.0]).show(ui, |ui| {
        for (k, s) in slots.iter().enumerate() {
            let mine = k as i8 == you;
            let c = palette[s.color as usize % palette.len()];
            ui.colored_label(rgb8(c), tr!("lobby.start_n", k + 1));
            // The slot's entry list.
            let label = match s.kind {
                0 => tr!("lobby.open").to_string(),
                1 => tr!("lobby.closed").to_string(),
                2 => format!("{}{}", s.name, if mine { tr!("lobby.you_suffix") } else { "" }),
                _ => tr!("lobby.slot_ai", longrange::i18n::ai_name(AiKind::from_id(s.ai))),
            };
            let mut entries: Vec<(String, SlotChoice, bool)> = vec![];
            if host && !mine {
                entries.push((tr!("lobby.open").to_string(), SlotChoice::Open, s.kind == 0));
                entries.push((tr!("lobby.closed").to_string(), SlotChoice::Closed, s.kind == 1));
                for a in AI_KINDS.iter() {
                    entries.push((tr!("lobby.slot_ai", longrange::i18n::ai_name(*a)), SlotChoice::Ai(a.id()), s.kind == 3 && s.ai == a.id()));
                }
            }
            if s.kind >= 2 && (host || mine) {
                for (j, o) in slots.iter().enumerate().filter(|(j, o)| *j != k && if host { o.kind != 1 } else { o.kind == 0 }) {
                    let _ = o;
                    entries.push((tr!("lobby.move_to_slot", j + 1), SlotChoice::Move(j), false));
                }
            }
            if s.kind == 0 && (you >= 0 || observing || host) {
                entries.push((tr!("lobby.take_slot").to_string(), SlotChoice::Take, false));
            }
            let tip = if s.kind == 3 { Some(longrange::i18n::ai_desc(AiKind::from_id(s.ai))) } else { None };
            if entries.is_empty() {
                let b = ui.add_sized([name_w, 30.0], egui::Button::new(label).sense(egui::Sense::hover()));
                if let Some(t) = tip {
                    b.on_hover_text(t);
                }
            } else {
                let r = egui::ComboBox::from_id_salt(("slot_entry", k)).width(name_w).selected_text(label).show_ui(ui, |ui| {
                    for (text, choice, on) in &entries {
                        if ui.selectable_label(*on, text).clicked() {
                            clicked = Some((k, SlotEdit::Choice(*choice)));
                        }
                    }
                });
                if let Some(t) = tip {
                    r.response.on_hover_text(t);
                }
            }
            let can_edit = host || mine;
            ui.add_enabled_ui(can_edit, |ui| {
                egui::ComboBox::from_id_salt(("slot_team", k)).width(team_w).selected_text(format!("{}", s.team + 1)).show_ui(ui, |ui| {
                    for t in 0..slots.len() as u8 {
                        if ui.selectable_label(s.team == t, format!("{}", t + 1)).clicked() {
                            clicked = Some((k, SlotEdit::Team(t)));
                        }
                    }
                })
                .response
                .on_hover_text(tr!("lobby.team_tip"));
                egui::ComboBox::from_id_salt(("slot_color", k))
                    .width(color_w)
                    .selected_text(egui::RichText::new("\u{25A0}\u{25A0}\u{25A0}\u{25A0}").color(rgb8(c)))
                    .show_ui(ui, |ui| {
                        for (ci, col) in palette.iter().enumerate() {
                            let text = egui::RichText::new("\u{25A0}\u{25A0}\u{25A0}\u{25A0}\u{25A0}\u{25A0}").color(rgb8(*col));
                            if ui.selectable_label(s.color as usize == ci, text).clicked() {
                                clicked = Some((k, SlotEdit::Color(ci as u8)));
                            }
                        }
                    })
                    .response
                    .on_hover_text(tr!("lobby.color_tip"));
            });
            let fname = defs.factions.get(s.faction as usize).map_or("?", |f| f.name.as_str());
            if ui.add_sized([faction_w, 30.0], egui::Button::new(fname)).clicked() && can_edit {
                clicked = Some((k, SlotEdit::Faction));
            }
            ui.end_row();
        }
    });
    clicked
}

/// The lobby's map picture with numbered start positions in the players' colours.
fn map_picture(ui: &mut egui::Ui, tex: Option<&egui::TextureHandle>, starts: &[(f32, f32)], colors: &[u8], points: &[(f32, f32)], defs: &Defs, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let p = ui.painter_at(rect);
    match tex {
        Some(t) => {
            p.image(t.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
        }
        None => {
            p.rect_filled(rect, 4.0, egui::Color32::from_rgb(30, 34, 40));
            p.text(rect.center(), egui::Align2::CENTER_CENTER, tr!("lobby.no_preview"), egui::FontId::proportional(16.0), egui::Color32::GRAY);
        }
    }
    p.rect_stroke(rect.shrink(0.5), 0.0, egui::Stroke::new(1.0, egui::Color32::from_gray(90)), egui::StrokeKind::Inside);
    // Start markers grow with the picture.
    let r = (size / 35.0).clamp(9.0, 16.0);
    // Control points: white diamonds.
    for (x, y) in points {
        let at = rect.min + egui::vec2(x * size, y * size);
        let d = r * 0.6;
        let shape = vec![at + egui::vec2(0.0, -d), at + egui::vec2(d, 0.0), at + egui::vec2(0.0, d), at + egui::vec2(-d, 0.0)];
        p.add(egui::Shape::convex_polygon(shape, egui::Color32::from_rgb(235, 235, 235), egui::Stroke::new(1.5, egui::Color32::BLACK)));
    }
    for (k, (x, y)) in starts.iter().enumerate() {
        let c = defs.rules.player_colors[colors.get(k).map_or(k, |c| *c as usize) % defs.rules.player_colors.len()];
        let at = rect.min + egui::vec2(x * size, y * size);
        p.circle_filled(at, r, egui::Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8));
        p.circle_stroke(at, r, egui::Stroke::new(1.5, egui::Color32::BLACK));
        p.text(at, egui::Align2::CENTER_CENTER, format!("{}", k + 1), egui::FontId::proportional(r * 1.35), egui::Color32::BLACK);
    }
}

enum BrowserAction {
    Join(String),
    Back,
}

/// Columns of the game browser after the map picture: (heading, share of the width).
const BROWSER_COLS: [(&str, f32); 8] = [
    (tx!("browser.col_host"), 0.16),
    (tx!("browser.col_map"), 0.22),
    (tx!("browser.col_players"), 0.11),
    (tx!("browser.col_mode"), 0.12),
    (tx!("browser.col_data"), 0.15),
    (tx!("browser.col_version"), 0.08),
    (tx!("browser.col_ping"), 0.06),
    (tx!("browser.col_status"), 0.10),
];

/// The game browser: a row per game found (map picture, host, map, players, mode, game data,
/// version, ping, state), Refresh, Join, and joining by address.
#[allow(clippy::too_many_arguments)]
fn browser_ui(ui: &mut egui::Ui, ctx: &egui::Context, b: &mut Browser, defs: &Defs, ip: &mut String, message: &str, width: f32, height: f32) -> Option<BrowserAction> {
    use egui::{pos2, vec2, Align2, Color32, FontId, Rect};
    let mut act = None;
    ui.label(egui::RichText::new(tr!("browser.title")).size(34.0).color(Color32::from_rgb(255, 215, 100)));
    ui.label(egui::RichText::new(tr!("browser.hint")).size(13.0).weak());
    ui.add_space(10.0);
    let pic = 56.0;
    let row_h = 66.0;
    let cols_w = width - pic - 40.0;
    let col_x = |left: f32, k: usize| left + pic + 16.0 + BROWSER_COLS[..k].iter().map(|c| c.1).sum::<f32>() * cols_w;
    let (head, _) = ui.allocate_exact_size(vec2(width, 22.0), egui::Sense::hover());
    for (k, (name, _)) in BROWSER_COLS.iter().enumerate() {
        ui.painter().text(pos2(col_x(head.min.x, k), head.center().y), Align2::LEFT_CENTER, tr(name), FontId::proportional(14.0), Color32::from_gray(160));
    }
    ui.separator();
    let amber = Color32::from_rgb(240, 200, 110);
    let red = Color32::from_rgb(255, 120, 105);
    let list_h = (height - 200.0).max(240.0);
    let mut joinable_sel: Option<std::net::IpAddr> = None;
    egui::ScrollArea::vertical().max_height(list_h).min_scrolled_height(list_h).show(ui, |ui| {
        if b.disc.found.is_empty() {
            ui.add_space(24.0);
            ui.label(egui::RichText::new(tr!("browser.searching")).size(16.0).weak());
        }
        for k in 0..b.disc.found.len() {
            let f = &b.disc.found[k];
            let (addr, ad, version, ping) = (f.ip, f.ad.clone(), f.version.clone(), f.ping_ms);
            let compatible = version == longrange::net::version();
            let open = ad.humans + ad.ais < ad.slots;
            let joinable = compatible && !ad.started && (open || ad.can_watch);
            let (rect, resp) = ui.allocate_exact_size(vec2(width, row_h), egui::Sense::click());
            let selected = b.selected == Some(addr);
            if selected && joinable {
                joinable_sel = Some(addr);
            }
            let fill = if selected {
                Color32::from_rgb(52, 68, 92)
            } else if resp.hovered() {
                Color32::from_rgb(36, 43, 53)
            } else if k % 2 == 0 {
                Color32::from_rgb(25, 29, 35)
            } else {
                Color32::from_rgb(20, 23, 28)
            };
            ui.painter().rect_filled(rect, 3.0, fill);
            // The map picture with the start positions.
            let pr = Rect::from_min_size(rect.min + vec2(8.0, (row_h - pic) / 2.0), vec2(pic, pic));
            let side = AD_PREVIEW;
            let sum = ad.preview.iter().step_by(31).fold(ad.preview.len() as u64, |h, v| h.wrapping_mul(31).wrapping_add(*v as u64));
            if ad.preview.len() == side * side * 3 && b.thumbs.get(&addr).map_or(true, |t| t.0 != sum) {
                let t = ctx.load_texture(format!("browse_{addr}"), egui::ColorImage::from_rgb([side, side], &ad.preview), egui::TextureOptions::LINEAR);
                b.thumbs.insert(addr, (sum, t));
            }
            match b.thumbs.get(&addr).filter(|_| !ad.preview.is_empty()) {
                Some((_, t)) => {
                    ui.painter().image(t.id(), pr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
                None => {
                    ui.painter().rect_filled(pr, 3.0, Color32::from_rgb(40, 46, 54));
                    ui.painter().text(pr.center(), Align2::CENTER_CENTER, "?", FontId::proportional(22.0), Color32::GRAY);
                }
            }
            for (j, (x, y)) in ad.starts.iter().enumerate() {
                let c = defs.rules.player_colors[j % defs.rules.player_colors.len().max(1)];
                let at = pr.min + vec2(*x as f32 / 255.0 * pic, *y as f32 / 255.0 * pic);
                ui.painter().circle(at, 3.0, Color32::from_rgb((c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8), egui::Stroke::new(1.0, Color32::BLACK));
            }
            // One or two lines of text per column, clipped to it.
            let cell = |col: usize, top: String, top_col: Color32, sub: Option<(String, Color32)>| {
                let x0 = col_x(rect.min.x, col);
                let clip = Rect::from_min_max(pos2(x0, rect.min.y), pos2(x0 + BROWSER_COLS[col].1 * cols_w - 8.0, rect.max.y));
                let p = ui.painter().with_clip_rect(clip);
                let dy = if sub.is_some() { -9.0 } else { 0.0 };
                p.text(pos2(x0, rect.center().y + dy), Align2::LEFT_CENTER, top, FontId::proportional(16.0), top_col);
                if let Some((t, c)) = sub {
                    p.text(pos2(x0, rect.center().y + 11.0), Align2::LEFT_CENTER, t, FontId::proportional(12.5), c);
                }
            };
            let text = Color32::from_gray(225);
            let weak = Color32::from_gray(140);
            let host = if ad.host.is_empty() { "?".to_string() } else { ad.host.clone() };
            cell(0, host, Color32::WHITE, Some((addr.to_string(), weak)));
            let map = match SIZES.iter().find(|s| s.0 == ad.cells as usize) {
                Some((n, name)) if ad.map.is_empty() => tr!("lobby.random_size", tr(name), *n as f32 * CELL, *n as f32 * CELL),
                _ => ad.map.clone(),
            };
            let map_sub = (!ad.map.is_empty() && !b.has_map(&ad.map)).then(|| (tr!("browser.map_download").to_string(), amber));
            cell(1, map, text, map_sub);
            let mut extra = vec![];
            if ad.ais > 0 {
                extra.push(tr!("browser.ais", ad.ais));
            }
            if ad.observers > 0 {
                extra.push(tr!("browser.watching", ad.observers));
            }
            cell(2, format!("{} / {}", ad.humans + ad.ais, ad.slots), text, (!extra.is_empty()).then(|| (extra.join(", "), weak)));
            cell(3, if ad.mode == MODE_POINTS { tr!("browser.mode_points") } else { tr!("browser.mode_annihilation") }.to_string(), text, None);
            let data = if ad.preset.is_empty() { tr!("lobby.built_in").to_string() } else { ad.preset.clone() };
            let data_sub = (compatible && ad.data_hash != defs.hash).then(|| (tr!("browser.data_download").to_string(), amber));
            cell(4, data, text, data_sub);
            cell(5, version.clone(), if compatible { text } else { red }, None);
            cell(6, tr!("browser.ms", ping), if ping < 60 { text } else { amber }, None);
            let (status, status_col) = if !compatible {
                (tr!("browser.status_other_version"), red)
            } else if ad.started {
                (tr!("browser.status_started"), weak)
            } else if open {
                (tr!("browser.status_open"), Color32::from_rgb(140, 220, 130))
            } else if ad.can_watch {
                (tr!("browser.status_watch"), amber)
            } else {
                (tr!("browser.status_full"), red)
            };
            cell(7, status.to_string(), status_col, None);
            let resp = if compatible { resp } else { resp.on_hover_text(tr!("browser.other_version_tip", version, longrange::net::version())) };
            if resp.clicked() {
                b.selected = Some(addr);
            }
            if resp.double_clicked() && joinable {
                act = Some(BrowserAction::Join(std::net::SocketAddr::new(addr, TCP_PORT).to_string()));
            }
            ui.add_space(2.0);
        }
    });
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        if ui.add_sized([150.0, 34.0], egui::Button::new(tr!("browser.refresh"))).clicked() || ui.input(|i| i.key_pressed(egui::Key::F5)) {
            b.disc.refresh();
            b.have_map.clear();
        }
        if ui.add_enabled(joinable_sel.is_some(), egui::Button::new(egui::RichText::new(tr!("browser.join")).size(16.0)).min_size(vec2(170.0, 34.0))).clicked() {
            if let Some(addr) = joinable_sel {
                act = Some(BrowserAction::Join(std::net::SocketAddr::new(addr, TCP_PORT).to_string()));
            }
        }
        ui.add_space(30.0);
        ui.label(tr!("browser.by_address"));
        let r = ui.add(egui::TextEdit::singleline(ip).desired_width(200.0).hint_text("192.168.1.20"));
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.add_sized([110.0, 34.0], egui::Button::new(tr!("browser.join_address"))).clicked() || enter) && !ip.trim().is_empty() {
            act = Some(BrowserAction::Join(ip.trim().to_string()));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_sized([150.0, 34.0], egui::Button::new(tr!("common.back"))).clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                act = Some(BrowserAction::Back);
            }
        });
    });
    if !message.is_empty() {
        ui.add_space(8.0);
        ui.colored_label(Color32::from_rgb(255, 120, 105), message);
    }
    act
}

/// Main menu and lobbies.
/// The updater window; true when closed.
fn update_ui(ctx: &egui::Context, u: &longrange::update::Updater) -> bool {
    use longrange::update::{mb, State};
    let mut close = false;
    egui::Window::new(tr!("update.title")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
        ui.set_width(520.0);
        ui.label(tr!("update.running", env!("CARGO_PKG_VERSION")));
        ui.add_space(6.0);
        match u.state() {
            State::Busy(t) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(t);
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
            State::UpToDate => {
                ui.label(tr!("update.up_to_date"));
            }
            State::Ready(p) => {
                ui.label(egui::RichText::new(tr!("update.available", p.tag)).strong());
                ui.label(tr!("update.size", p.files, p.removed, mb(p.bytes)));
                ui.add_space(4.0);
                ui.label(egui::RichText::new(tr!("update.changes")).strong());
                egui::ScrollArea::vertical().max_height(320.0).show(ui, |ui| {
                    for (k, (tag, body)) in p.notes.iter().enumerate() {
                        egui::CollapsingHeader::new(egui::RichText::new(tag).strong()).default_open(k == 0).show(ui, |ui| {
                            // The CHANGELOG section: category lines ("Graphics") and "- " bullets.
                            for line in body.lines().map(str::trim).filter(|l| !l.is_empty()) {
                                if let Some(b) = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label("•");
                                        ui.label(egui::RichText::new(b).size(12.5));
                                    });
                                } else {
                                    ui.add_space(4.0);
                                    ui.label(egui::RichText::new(line.trim_start_matches('#').trim()).strong());
                                }
                            }
                            if body.is_empty() {
                                ui.label(egui::RichText::new(tr!("update.no_notes")).weak());
                            }
                        });
                    }
                });
                ui.add_space(6.0);
                if ui.button(tr!("update.install")).clicked() {
                    u.install(p.clone());
                }
            }
            State::Done(tag) => {
                ui.label(tr!("update.done", tag));
                if ui.button(tr!("update.restart")).clicked() {
                    match longrange::update::restart() {
                        Ok(()) => std::process::exit(0),
                        Err(e) => {
                            ui.colored_label(egui::Color32::from_rgb(255, 120, 105), e);
                        }
                    }
                }
            }
            State::Failed(e) => {
                ui.colored_label(egui::Color32::from_rgb(255, 120, 105), e);
                ui.hyperlink_to(tr!("update.releases_page"), format!("https://github.com/{}/releases", longrange::update::REPO));
            }
        }
        // No closing while a check or download runs.
        if !u.busy() {
            ui.add_space(6.0);
            if ui.button(tr!("update.close")).clicked() {
                close = true;
            }
        }
    });
    close
}

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
    // LR_HOST=1 opens a host lobby at once (it starts with an AI once a player joined), LR_JOIN=<ip>
    // joins that host at once: testing multiplayer without clicking through the menus.
    static AUTO_NET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
    if matches!(app.screen, Screen::Menu) && AUTO_NET.swap(false, std::sync::atomic::Ordering::Relaxed) {
        if std::env::var("LR_HOST").is_ok() {
            match new_lobby(&app.name, &defs) {
                Ok(l) => next = Some(Screen::HostLobby(l)),
                Err(e) => app.message = e,
            }
        } else if let Ok(addr) = std::env::var("LR_JOIN") {
            match Client::connect(&addr, &app.name, defs.hash) {
                Ok(client) => next = Some(Screen::ClientLobby(ClientLobby { client, info: None, pending: None, tex: None, have_map: None })),
                Err(e) => app.message = e.to_string(),
            }
        }
    }
    if app.skirmish {
        // `--skirmish`: straight into a game against three AIs.
        app.skirmish = false;
        match new_lobby(&app.name, &defs) {
            Ok(mut l) => {
                for (k, s) in l.slots.iter_mut().skip(1).enumerate() {
                    s.kind = 3;
                    s.ai = AI_KINDS[k % 3];
                }
                // LR_MAP=<name> picks a premade map for --skirmish.
                if let Ok(m) = std::env::var("LR_MAP") {
                    if let Some(k) = l.maps.iter().position(|x| *x == m) {
                        l.map_choice = SIZES.len() + k;
                        l.resize_slots(0);
                        for (k, s) in l.slots.iter_mut().skip(1).enumerate() {
                            s.kind = 3;
                            s.ai = AI_KINDS[k % 3];
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
            let lobby = matches!(app.screen, Screen::HostLobby(_) | Screen::ClientLobby(_) | Screen::Browse(_) | Screen::Replays(_));
            ui.set_max_width(if lobby { lobby_w } else { 560.0 });
            match &mut app.screen {
                Screen::Menu => {
                    ui.add_space(30.0);
                    ui.label(egui::RichText::new(tr!("main_menu.title")).size(56.0).color(egui::Color32::from_rgb(255, 215, 100)).strong());
                    ui.label(egui::RichText::new(tr!("main_menu.tagline")).size(16.0));
                    ui.add_space(26.0);
                    ui.horizontal(|ui| {
                        ui.label(tr!("common.name"));
                        if ui.add(egui::TextEdit::singleline(&mut app.name).desired_width(300.0).char_limit(16)).changed() {
                            // Remembered for the next start.
                            let mut s = world.resource_mut::<longrange::settings::Settings>();
                            s.name = app.name.clone();
                            s.save();
                        }
                    });
                    ui.add_space(10.0);
                    if ui.add_sized([440.0, 44.0], egui::Button::new(egui::RichText::new(tr!("main_menu.host_game")).size(18.0))).clicked() {
                        match new_lobby(&app.name, &defs) {
                            Ok(l) => {
                                next = Some(Screen::HostLobby(l));
                                app.message.clear();
                            }
                            Err(e) => app.message = e,
                        }
                    }
                    ui.add_space(6.0);
                    if ui.add_sized([440.0, 44.0], egui::Button::new(egui::RichText::new(tr!("main_menu.join_game")).size(18.0))).clicked() {
                        next = Some(Screen::Browse(Browser::new()));
                        app.message.clear();
                    }
                    ui.add_space(6.0);
                    if ui.add_sized([440.0, 34.0], egui::Button::new(tr!("main_menu.replays"))).clicked() {
                        next = Some(Screen::Replays(ReplayList::new()));
                        app.message.clear();
                    }
                    ui.add_space(14.0);
                    if ui.add_sized([440.0, 34.0], egui::Button::new(tr!("main_menu.game_data_presets"))).clicked() {
                        next = Some(Screen::Presets(PresetUi::new()));
                        app.message.clear();
                    }
                    ui.add_space(6.0);
                    if ui.add_sized([440.0, 34.0], egui::Button::new(tr!("main_menu.map_editor"))).clicked() {
                        // The editor is this same program started with --editor, in its own window and process.
                        let started = std::env::current_exe().and_then(|exe| std::process::Command::new(exe).arg("--editor").spawn());
                        match started {
                            Ok(_) => app.message.clear(),
                            Err(e) => app.message = tr!("main_menu.editor_failed", e),
                        }
                    }
                    ui.add_space(6.0);
                    if ui.add_sized([440.0, 34.0], egui::Button::new(tr!("common.settings"))).clicked() {
                        app.settings_open = true;
                    }
                    ui.add_space(6.0);
                    if ui.add_sized([440.0, 34.0], egui::Button::new(tr!("update.check"))).clicked() && app.update.is_none() {
                        app.update = Some(longrange::update::Updater::check());
                    }
                    ui.add_space(6.0);
                    if ui.add_sized([440.0, 34.0], egui::Button::new(tr!("main_menu.quit"))).clicked() {
                        world.write_message(AppExit::Success);
                    }
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(tr!("main_menu.fullscreen_hint")).size(12.0).weak());
                    if !app.message.is_empty() {
                        ui.add_space(10.0);
                        ui.colored_label(egui::Color32::from_rgb(255, 120, 105), &app.message);
                    }
                }
                Screen::HostLobby(l) => {
                    let first_faction = playable.first().copied().unwrap_or(0) as u8;
                    while let Ok(ev) = l.host.events.try_recv() {
                        match ev {
                            HostEvent::Msg(id, C2S::Hello { name, data_hash }) => {
                                // Different game data is fine: that player gets ours when the game starts.
                                // No open slot: they watch.
                                let name: String = name.chars().take(16).collect();
                                if let Some(s) = l.slots.iter_mut().find(|s| s.kind == 0) {
                                    s.kind = 2;
                                    s.name = name;
                                    s.conn = id;
                                    s.hash = data_hash;
                                } else if l.observers.len() < MAX_OBSERVERS {
                                    l.observers.push(Observer { name, conn: id, hash: data_hash });
                                } else {
                                    l.host.send_msg(id, &S2C::Kicked(tx!("connection.kicked_full").into()));
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
                            HostEvent::Msg(id, C2S::Observe(watch)) => {
                                l.observe(id, watch, first_faction);
                            }
                            HostEvent::Msg(id, C2S::Slot(to)) => {
                                l.take_slot(id, to as usize);
                            }
                            HostEvent::Msg(id, C2S::Team(t)) => {
                                let n = l.slots.len() as u8;
                                if let Some(s) = l.slots.iter_mut().find(|s| s.kind == 2 && s.conn == id) {
                                    s.team = t.min(n - 1);
                                    l.dirty = true;
                                }
                            }
                            HostEvent::Msg(id, C2S::Color(c)) => {
                                if let Some(k) = l.slot_of(id) {
                                    l.set_color(k, c % defs.rules.player_colors.len() as u8);
                                }
                            }
                            HostEvent::Left(id) => {
                                l.observers.retain(|o| o.conn != id);
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
                    ui.label(egui::RichText::new(tr!("lobby.game_lobby")).size(34.0).color(egui::Color32::from_rgb(255, 215, 100)));
                    ui.label(tr!("lobby.lan_address", l.lan_ip, TCP_PORT));
                    ui.add_space(12.0);
                    let cur = world.resource::<Defs>().clone();
                    if let Err(e) = l.ensure_map(&cur) {
                        app.message = e;
                    }
                    let (mut start, mut back) = (false, false);
                    ui.horizontal_top(|ui| {
                        // Left: the players.
                        ui.vertical(|ui| {
                            ui.set_width(left_w);
                            let me = l.slot_of(0);
                            let watching = l.observers.iter().any(|o| o.conn == 0);
                            if let Some((k, edit)) = slot_rows(ui, &cur, &l.info(0, &cur).slots, me.map_or(-1, |k| k as i8), watching, true, left_w) {
                                match edit {
                                    SlotEdit::Team(t) => l.slots[k].team = t,
                                    SlotEdit::Color(c) => l.set_color(k, c),
                                    SlotEdit::Faction => l.slots[k].faction = next_faction(l.slots[k].faction),
                                    SlotEdit::Choice(SlotChoice::Move(j)) => l.slots.swap(k, j),
                                    SlotEdit::Choice(SlotChoice::Take) => {
                                        l.take_slot(0, k);
                                    }
                                    SlotEdit::Choice(choice) => {
                                        let s = &mut l.slots[k];
                                        if s.kind == 2 {
                                            l.host.send_msg(s.conn, &S2C::Kicked(tx!("connection.kicked_removed").into()));
                                            l.host.kick(s.conn);
                                            s.name.clear();
                                            s.conn = 0;
                                        }
                                        match choice {
                                            SlotChoice::Open => s.kind = 0,
                                            SlotChoice::Closed => s.kind = 1,
                                            SlotChoice::Ai(id) => (s.kind, s.ai) = (3, AiKind::from_id(id)),
                                            _ => {}
                                        }
                                    }
                                }
                                l.dirty = true;
                            }
                            ui.label(egui::RichText::new(tr!("lobby.slots_hint")).size(12.0).weak());
                            let names: Vec<String> = l.observers.iter().map(|o| if o.conn == 0 { tr!("lobby.observer_you", o.name) } else { o.name.clone() }).collect();
                            match observer_rows(ui, &names, me.is_some(), l.observers.iter().any(|o| o.conn == 0), l.slots.iter().any(|s| s.kind == 0), true, left_w) {
                                Some(ObserverClick::Watch(w)) => {
                                    l.observe(0, w, first_faction);
                                }
                                Some(ObserverClick::Remove(k)) => {
                                    let o = &l.observers[k];
                                    if o.conn != 0 {
                                        l.host.send_msg(o.conn, &S2C::Kicked(tx!("connection.kicked_removed").into()));
                                        l.host.kick(o.conn);
                                        l.observers.remove(k);
                                        l.dirty = true;
                                    }
                                }
                                None => {}
                            }
                            if !app.message.is_empty() {
                                ui.colored_label(egui::Color32::from_rgb(255, 120, 105), &app.message);
                            }
                            ui.add_space(14.0);
                            start = ui.add_sized([left_w - 20.0, 50.0], egui::Button::new(egui::RichText::new(tr!("lobby.start_button")).size(22.0))).clicked();
                            back = ui.add_sized([left_w - 20.0, 34.0], egui::Button::new(tr!("common.back"))).clicked();
                        });
                        ui.add_space(20.0);
                        // Right: the map and the tabs with the game settings.
                        ui.vertical(|ui| {
                            ui.set_width(right_w);
                            if l.tex.is_none() && l.preview.len() == PREVIEW * PREVIEW * 4 {
                                l.tex = Some(ctx.load_texture("lobby_map", egui::ColorImage::from_rgba_unmultiplied([PREVIEW, PREVIEW], &l.preview), egui::TextureOptions::LINEAR));
                            }
                            map_picture(ui, l.tex.as_ref(), &l.starts, &l.slots.iter().map(|s| s.color).collect::<Vec<_>>(), if l.mode == MODE_POINTS { &l.points } else { &[] }, &cur, pic);
                            ui.label(egui::RichText::new(l.map_label()).strong());
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.selectable_value(&mut l.tab, 0, egui::RichText::new(tr!("lobby.game_options")).size(16.0));
                                ui.selectable_value(&mut l.tab, 1, egui::RichText::new(tr!("lobby.map")).size(16.0));
                            });
                            ui.separator();
                            if l.tab == 0 {
                                if ui.add_sized([right_w - 20.0, 30.0], egui::Button::new(tr!("lobby.mode_button", l.mode_label()))).clicked() {
                                    l.mode = if l.mode == MODE_POINTS { MODE_ANNIHILATION } else { MODE_POINTS };
                                    l.dirty = true;
                                }
                                if l.mode == MODE_POINTS {
                                    // Typed in by hand; 0 = the game data's value until the host changes it.
                                    ui.horizontal(|ui| {
                                        ui.label(egui::RichText::new(tr!("lobby.starting_points")).size(15.0));
                                        let mut v = l.cp_points(&cur).round().max(1.0) as u32;
                                        if ui.add(egui::DragValue::new(&mut v).range(1..=1_000_000).speed(10.0)).changed() {
                                            l.cp_start = v;
                                            l.dirty = true;
                                        }
                                        if l.cp_start == 0 {
                                            ui.label(egui::RichText::new(tr!("lobby.points_default")).weak());
                                        } else if ui.button(tr!("common.default")).on_hover_text(tr!("lobby.default_points_tip", cur.rules.cp_start_points)).clicked() {
                                            l.cp_start = 0;
                                            l.dirty = true;
                                        }
                                    });
                                    let own = l.map.as_ref().map_or(false, |m| !m.1.points.is_empty());
                                    ui.label(egui::RichText::new(tr!("lobby.points_count", l.points.len(), if own { tr!("lobby.points_by_map") } else { tr!("lobby.points_auto") })).weak());
                                }
                                // Game data: the built-in values or one of the presets (main menu -> Game data presets).
                                if ui.add_sized([right_w - 20.0, 30.0], egui::Button::new(tr!("lobby.game_data_button", l.preset_label()))).clicked() && !l.presets.is_empty() {
                                    let k = (l.preset + 1) % (l.presets.len() + 1);
                                    let name = if k == 0 { String::new() } else { l.presets[k - 1].clone() };
                                    match Defs::load_preset(&asset_root(), &name) {
                                        Ok(d) => {
                                            l.preset = k;
                                            l.rubble_blocks = d.rules.rubble_blocks_paths;
                                            let f = d.playable_factions().first().copied().unwrap_or(0) as u8;
                                            for s in l.slots.iter_mut() {
                                                s.faction = f;
                                            }
                                            world.insert_resource(d);
                                            l.dirty = true;
                                            app.message.clear();
                                        }
                                        Err(e) => app.message = tr!("lobby.preset_unusable", name, e),
                                    }
                                }
                                if l.map_choice < SIZES.len() {
                                    ui.horizontal(|ui| {
                                        if ui.add_sized([(right_w - 28.0) / 2.0, 30.0], egui::Button::new(tr!("lobby.players_button", l.players))).clicked() {
                                            l.players = if l.players >= MAX_PLAYERS { 2 } else { l.players + 1 };
                                            l.resize_slots(first_faction);
                                        }
                                        if ui.add_sized([(right_w - 28.0) / 2.0, 30.0], egui::Button::new(tr!("lobby.new_random_map"))).clicked() {
                                            l.seed = l.seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407) | 1;
                                        }
                                    });
                                    let biome_label = |k: usize| match k.checked_sub(1).and_then(|k| longrange::surface::BIOMES.get(k)) {
                                        Some(b) => longrange::i18n::biome_name(*b).to_string(),
                                        None => tr!("lobby.random_biome", longrange::i18n::biome_name(longrange::surface::Biome::from_seed(l.seed))),
                                    };
                                    if ui.add_sized([right_w - 20.0, 30.0], egui::Button::new(tr!("lobby.terrain_button", biome_label(l.biome)))).on_hover_text(tr!("lobby.terrain_tip")).clicked() {
                                        l.biome = (l.biome + 1) % (longrange::surface::BIOMES.len() + 1);
                                    }
                                }
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(tr!("lobby.weather_label")).size(15.0));
                                    let tip = cur.rules.weather.get(l.weather).map_or_else(|| tr!("lobby.weather_random_tip").to_string(), |w| longrange::i18n::weather_desc(w).to_string());
                                    egui::ComboBox::from_id_salt("weather").width(right_w - 120.0).selected_text(l.weather_label(&cur)).show_ui(ui, |ui| {
                                        for (k, w) in cur.rules.weather.iter().enumerate() {
                                            if ui.selectable_value(&mut l.weather, k, longrange::i18n::weather_name(w)).on_hover_text(longrange::i18n::weather_desc(w)).changed() {
                                                l.dirty = true;
                                            }
                                        }
                                        let random = cur.rules.weather.len();
                                        if ui.selectable_value(&mut l.weather, random, tr!("lobby.weather_random")).on_hover_text(tr!("lobby.weather_random_tip")).changed() {
                                            l.dirty = true;
                                        }
                                    }).response.on_hover_text(tip);
                                });
                                if ui.checkbox(&mut l.reveal_props, tr!("lobby.reveal_props")).changed() {
                                    l.dirty = true;
                                }
                                if ui.checkbox(&mut l.infantry_capture, tr!("lobby.infantry_capture")).on_hover_text(tr!("lobby.infantry_capture_tip")).changed() {
                                    l.dirty = true;
                                }
                                if ui.checkbox(&mut l.eco_public, tr!("lobby.eco_public")).on_hover_text(tr!("lobby.eco_public_tip")).changed() {
                                    l.dirty = true;
                                }
                                if ui.checkbox(&mut l.rubble_blocks, tr!("lobby.rubble_blocks")).on_hover_text(tr!("lobby.rubble_blocks_tip")).changed() {
                                    l.dirty = true;
                                }
                            } else {
                                // One thumbnail per frame, so opening the tab does not stall.
                                if let Some(k) = l.thumbs.iter().position(|t| t.is_none()) {
                                    let px = longrange::data::load_map(&l.maps[k]).map(|m| view::terrain::map_preview(&m, &cur, 64)).unwrap_or_else(|_| vec![60; 64 * 64 * 4]);
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
                                                Some((n, name)) => tr!("lobby.random_size", tr(name), *n as f32 * CELL, *n as f32 * CELL),
                                                None => {
                                                    let k = c - SIZES.len();
                                                    let starts = l.counts.get(k).copied().unwrap_or(0);
                                                    tr!("lobby.map_players", longrange::data::split_map(&l.maps[k]).0, starts)
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
                    if std::env::var("LR_HOST").is_ok() && l.slots.iter().any(|s| s.conn != 0) {
                        // Some seconds in the lobby first, the way people start.
                        static JOINED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
                        start = JOINED.get_or_init(std::time::Instant::now).elapsed().as_secs() >= 8;
                    }
                    if start && !l.slots.iter().any(|s| s.kind >= 2) {
                        app.message = tr!("lobby.nobody_plays").into();
                        start = false;
                    }
                    if l.dirty {
                        l.dirty = false;
                        let conns: Vec<u32> = l.slots.iter().map(|s| s.conn).chain(l.observers.iter().map(|o| o.conn)).filter(|c| *c != 0).collect();
                        let sum = {
                            use std::hash::{Hash, Hasher};
                            let mut h = std::collections::hash_map::DefaultHasher::new();
                            l.preview.hash(&mut h);
                            h.finish()
                        };
                        for c in conns {
                            let mut info = l.info(c, world.resource::<Defs>());
                            if l.sent_preview.contains(&(c, sum)) {
                                info.preview = vec![];
                                info.preview_same = true;
                            } else {
                                l.sent_preview.retain(|s| s.0 != c);
                                l.sent_preview.push((c, sum));
                            }
                            l.host.send_msg(c, &S2C::Lobby(info));
                        }
                        l.host.advertise(l.ad(world.resource::<Defs>()));
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
                    let mut started: Option<(ClientSim, Option<Recorder>)> = None;
                    loop {
                        match l.client.frames.try_recv() {
                            Ok(Some((TAG_MSG, data))) => match bincode::deserialize::<S2C>(&data) {
                                Ok(S2C::Lobby(mut i)) => {
                                    if i.preview_same {
                                        i.preview = l.info.as_mut().map(|o| std::mem::take(&mut o.preview)).unwrap_or_default();
                                    }
                                    l.info = Some(i);
                                }
                                Ok(S2C::Start { map, you, data }) => {
                                    // The host plays with other game data than ours: use the host's for this game.
                                    let mut keep_pack = None;
                                    if let Some(pack) = data {
                                        match Defs::from_pack(pack.clone()) {
                                            Ok(d) => {
                                                pack.store_assets(&asset_root());
                                                world.insert_resource(d);
                                                keep_pack = Some(pack);
                                            }
                                            Err(e) => lost = Some(tr!("connection.host_data_unusable", e)),
                                        }
                                    }
                                    // A premade map or a preset this player lacks is theirs from now on.
                                    let file = l.info.as_ref().map(|i| i.map_file.clone()).unwrap_or_default();
                                    // (A variant of a map with several is the host's file; ours is the whole map.)
                                    let keep = (!file.is_empty() && longrange::data::split_map(&file).1.is_none()).then(|| (map.clone(), file));
                                    keep_downloads(keep, keep_pack);
                                    l.pending = Some((map, you));
                                }
                                Ok(S2C::Kicked(why)) => lost = Some(tr(&why).to_string()),
                                Ok(_) => {}
                                Err(_) => lost = Some(tr!("connection.protocol_mismatch").into()),
                            },
                            Ok(Some((_, data))) => {
                                if let (Some((map, you)), Ok(snap)) = (l.pending.take(), bincode::deserialize::<Snapshot>(&data)) {
                                    let rec = start_recording(world, &map, &snap, you);
                                    join_game(world, map, &snap);
                                    view::start_view(world, you, true);
                                    started = Some((ClientSim::new(you, snap), rec));
                                    break;
                                }
                            }
                            Ok(None) | Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                lost.get_or_insert(tr!("connection.disconnected").into());
                                break;
                            }
                            Err(_) => break,
                        }
                    }
                    ui.label(egui::RichText::new(tr!("lobby.game_lobby")).size(34.0).color(egui::Color32::from_rgb(255, 215, 100)));
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
                                    if let Some((k, edit)) = slot_rows(ui, &defs, &i.slots, i.you, i.observing, false, left_w) {
                                        match edit {
                                            SlotEdit::Faction => l.client.send(&C2S::Faction(next_faction(i.slots[k].faction))),
                                            SlotEdit::Team(t) => l.client.send(&C2S::Team(t)),
                                            SlotEdit::Color(c) => l.client.send(&C2S::Color(c)),
                                            SlotEdit::Choice(SlotChoice::Move(j)) => l.client.send(&C2S::Slot(j as u8)),
                                            SlotEdit::Choice(SlotChoice::Take) => l.client.send(&C2S::Slot(k as u8)),
                                            SlotEdit::Choice(_) => {}
                                        }
                                    }
                                    if let Some(ObserverClick::Watch(w)) = observer_rows(ui, &i.observers, i.you >= 0, i.observing, i.slots.iter().any(|s| s.kind == 0), false, left_w) {
                                        l.client.send(&C2S::Observe(w));
                                    }
                                    ui.add_space(8.0);
                                    ui.label(tr!("lobby.waiting_for_host"));
                                    ui.add_space(14.0);
                                    back = ui.add_sized([left_w - 20.0, 34.0], egui::Button::new(tr!("lobby.leave"))).clicked();
                                });
                                ui.add_space(20.0);
                                ui.vertical(|ui| {
                                    ui.set_width(right_w);
                                    map_picture(ui, l.tex.as_ref().map(|t| &t.1), &i.starts, &i.slots.iter().map(|s| s.color).collect::<Vec<_>>(), &i.points, &defs, pic);
                                    ui.label(egui::RichText::new(&i.map_name).strong());
                                    // What the game will download from the host.
                                    if !i.map_file.is_empty() {
                                        if l.have_map.as_ref().map_or(true, |h| h.0 != i.map_file) {
                                            l.have_map = Some((i.map_file.clone(), longrange::data::map_dir(&i.map_file).join("map.json").exists()));
                                        }
                                        let have = l.have_map.as_ref().map_or(false, |h| h.1);
                                        ui.label(egui::RichText::new(if have { tr!("lobby.map_here") } else { tr!("lobby.map_download") }).weak());
                                    }
                                    let preset = if i.preset.is_empty() { tr!("lobby.built_in").to_string() } else { i.preset.clone() };
                                    if i.data_hash == defs.hash {
                                        ui.label(egui::RichText::new(tr!("lobby.data_same", preset)).weak());
                                    } else {
                                        ui.label(egui::RichText::new(tr!("lobby.data_download", preset)).color(egui::Color32::from_rgb(240, 200, 110)));
                                    }
                                    ui.add_space(6.0);
                                    ui.label(egui::RichText::new(tr!("lobby.game_options")).size(16.0));
                                    ui.separator();
                                    ui.label(&i.extra);
                                });
                            });
                        }
                        None => {
                            ui.label(tr!("lobby.connecting"));
                            ui.add_space(10.0);
                            back = ui.add_sized([left_w - 20.0, 34.0], egui::Button::new(tr!("lobby.leave"))).clicked();
                        }
                    }
                    if let Some((sim, rec)) = started {
                        if let Screen::ClientLobby(l) = std::mem::replace(&mut app.screen, Screen::Menu) {
                            next = Some(Screen::Game(Session { role: Role::Client { client: l.client, sim, heard: 0.0, rejoin: None, skip: vec![] }, acc: 0.0, paused: None, held: vec![], rec }));
                        }
                    } else if let Some(why) = lost {
                        app.message = why;
                        next = Some(Screen::Browse(Browser::new()));
                    } else if back {
                        next = Some(Screen::Browse(Browser::new()));
                    }
                }
                Screen::Browse(b) => {
                    b.disc.poll(dt);
                    let join = browser_ui(ui, &ctx, b, &defs, &mut app.ip, &app.message, lobby_w, lobby_h);
                    match join {
                        Some(BrowserAction::Join(addr)) => match Client::connect(&addr, &app.name, defs.hash) {
                            Ok(client) => {
                                next = Some(Screen::ClientLobby(ClientLobby { client, info: None, pending: None, tex: None, have_map: None }));
                                app.message.clear();
                            }
                            Err(e) => app.message = e,
                        },
                        Some(BrowserAction::Back) => {
                            next = Some(Screen::Menu);
                            app.message.clear();
                        }
                        None => {}
                    }
                }
                Screen::Replays(r) => match replays_ui(ui, r, &app.message, lobby_w, lobby_h) {
                    Some(ReplayAction::Watch(path)) => match start_replay(world, &path, &app.base_defs) {
                        Ok(s) => {
                            next = Some(Screen::Game(s));
                            app.message.clear();
                        }
                        Err(e) => {
                            restore_defs(world, &app.base_defs);
                            app.message = tr!("replays.load_failed", e);
                        }
                    },
                    Some(ReplayAction::Back) => {
                        next = Some(Screen::Menu);
                        app.message.clear();
                    }
                    None => {}
                },
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
    if matches!(app.screen, Screen::Menu) && next.is_none() {
        if let Some(u) = &app.update {
            if update_ui(&ctx, u) {
                app.update = None;
            }
        }
    }
    if let Some(n) = next {
        if matches!(n, Screen::Menu | Screen::Browse(_)) {
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
        ui.label(egui::RichText::new(tr!("preset_editor.title")).size(30.0).color(egui::Color32::from_rgb(255, 215, 100)));
        ui.label(tr!("preset_editor.intro"));
        ui.add_space(8.0);
        ui.horizontal_top(|ui| {
            // --- the presets
            ui.vertical(|ui| {
                ui.set_width(230.0);
                ui.label(egui::RichText::new(tr!("preset_editor.presets")).strong());
                let mut open: Option<String> = None;
                for n in &p.names {
                    if ui.add_sized([220.0, 26.0], egui::Button::selectable(open_name.as_deref() == Some(n.as_str()), n.as_str())).clicked() {
                        open = Some(n.clone());
                    }
                }
                if p.names.is_empty() {
                    ui.label(egui::RichText::new(tr!("preset_editor.none_yet")).weak());
                }
                if let Some(n) = open {
                    p.open(&n);
                }
                ui.add_space(10.0);
                ui.label(tr!("preset_editor.new_preset"));
                ui.add(egui::TextEdit::singleline(&mut p.new_name).desired_width(214.0).char_limit(32).hint_text("name"));
                let from = open_name.clone().unwrap_or_default();
                if ui.add_sized([220.0, 26.0], egui::Button::new(if from.is_empty() { tr!("preset_editor.create_from_the_built_in_data").to_string() } else { tr!("preset_editor.create_as_a_copy_of", from) })).clicked() {
                    let name = longrange::data::clean_name(&p.new_name);
                    let made = if name.is_empty() {
                        Err(tr!("preset_editor.name_needed").to_string())
                    } else if p.names.contains(&name) {
                        Err(tr!("preset_editor.name_taken", name))
                    } else {
                        // Unsaved edits of the open preset are part of the copy.
                        p.edited().or_else(|_| DataPack::preset(&asset_root(), &from)).and_then(|mut pack| {
                            pack.name = name.clone();
                            pack.save()
                        })
                    };
                    match made {
                        Ok(()) => {
                            p.names = DataPack::list(&asset_root());
                            p.new_name.clear();
                            p.open(&name);
                            p.msg = tr!("preset_editor.created", name);
                        }
                        Err(e) => p.msg = e,
                    }
                }
                if let Some(n) = open_name.clone() {
                    ui.add_space(10.0);
                    if ui.add_sized([220.0, 26.0], egui::Button::new(if p.confirm_delete { tr!("preset_editor.really_delete", n) } else { tr!("preset_editor.delete", n) })).clicked() {
                        if p.confirm_delete {
                            p.msg = match DataPack::delete(&asset_root(), &n) {
                                Ok(()) => tr!("preset_editor.deleted", n),
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
                if ui.add_sized([220.0, 32.0], egui::Button::new(tr!("common.back"))).clicked() {
                    back = true;
                }
            });
            let Some(pack) = p.pack.clone() else {
                ui.label(tr!("preset_editor.pick_preset"));
                return;
            };
            // --- its files and units
            ui.vertical(|ui| {
                ui.set_width(list_w);
                ui.label(egui::RichText::new(tr!("preset_editor.files")).strong());
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
                    ui.label(egui::RichText::new(tr!("preset_editor.units")).strong());
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
                        Err(e) => p.msg = tr!("preset_editor.fix_before_switching", e),
                    }
                }
            });
            // --- the text
            ui.vertical(|ui| {
                ui.set_width(text_w);
                ui.horizontal(|ui| {
                    if ui.add_sized([150.0, 28.0], egui::Button::new(tr!("preset_editor.check_and_save"))).on_hover_text(tr!("preset_editor.check_and_save_tip")).clicked() {
                        p.save();
                    }
                    let label = if p.raw { tr!("preset_editor.form_view") } else { tr!("preset_editor.json_view") };
                    let tip = if p.raw { tr!("preset_editor.form_view_tip") } else { tr!("preset_editor.json_view_tip") };
                    if ui.add_sized([110.0, 28.0], egui::Button::new(label)).on_hover_text(tip).clicked() {
                        if p.raw {
                            match serde_json::from_str(&p.text) {
                                Ok(v) => {
                                    p.value = v;
                                    p.raw = false;
                                    p.msg.clear();
                                }
                                Err(e) => p.msg = tr!("preset_editor.fix_the_json_first", e),
                            }
                        } else {
                            p.raw = true;
                        }
                    }
                    if p.file > 0 {
                        if ui.add_sized([110.0, 28.0], egui::Button::new(tr!("preset_editor.copy_unit"))).clicked() {
                            p.change_units(true);
                        }
                        if ui.add_sized([110.0, 28.0], egui::Button::new(tr!("preset_editor.remove_unit"))).clicked() {
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
                        if p.raw {
                            ui.add(egui::TextEdit::multiline(&mut p.text).code_editor().desired_width(text_w - 14.0).min_size(egui::vec2(text_w - 14.0, text_h - 4.0)));
                        } else if !p.value.is_object() {
                            ui.label(tr!("preset_editor.not_json"));
                        } else {
                            ui.set_width(text_w - 14.0);
                            let changed = if p.file == 0 {
                                longrange::preset_form::rules_form(ui, &mut p.value, &p.ctx)
                            } else {
                                longrange::preset_form::unit_form(ui, &mut p.value, &p.ctx)
                            };
                            if changed {
                                p.form_changed();
                            }
                        }
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
/// units of that type are selected first, so the selection panel is in the picture. A fifth field
/// `shore` points the camera at a steep shore near the map centre (water checks), `x:z` at that point.
#[allow(clippy::too_many_arguments)]
fn dev_screenshot(mut commands: Commands, game: Option<Res<Game>>, view: Option<ResMut<ViewState>>, defs: Res<Defs>, units: Query<&Unit>, time: Res<Time>, map: Option<Res<MapData>>, mut state: Local<(u8, f32)>, mut exit: MessageWriter<AppExit>) {
    static SPEC: std::sync::OnceLock<Option<(String, f32, String, f32)>> = std::sync::OnceLock::new();
    static SHORE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let shore = *SHORE.get_or_init(|| std::env::var("LR_SHOT").is_ok_and(|v| v.split(',').nth(4) == Some("shore")));
    // Or `x:z`, a point to look at.
    static AT: std::sync::OnceLock<Option<(f32, f32)>> = std::sync::OnceLock::new();
    let look = *AT.get_or_init(|| {
        let v = std::env::var("LR_SHOT").ok()?;
        let (x, z) = v.split(',').nth(4)?.split_once(':')?;
        Some((x.parse().ok()?, z.parse().ok()?))
    });
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
            if let Some((x, z)) = look {
                view.cam_target = longrange::map::v2(x, z);
            }
            if let (true, Some(map)) = (shore, &map) {
                let (n, ny) = map.dims();
                let c = map.center();
                let wet = |x: usize, y: usize| map.vertex(x, y) < map.water_cell(x.min(n - 1), y.min(ny - 1));
                // A shore away from the border with high ground right behind it, so reflections show.
                let rise = |x: usize, y: usize| {
                    let mut h = f32::MIN;
                    for j in y.saturating_sub(4)..(y + 5).min(ny) {
                        for i in x.saturating_sub(4)..(x + 5).min(n) {
                            h = h.max(map.vertex(i, j));
                        }
                    }
                    h - map.water_cell(x, y)
                };
                let best = (8..ny.saturating_sub(8)).flat_map(|y| (8..n.saturating_sub(8)).map(move |x| (x, y))).filter(|&(x, y)| wet(x, y) != wet(x + 1, y + 1)).max_by(|a, b| {
                    let score = |p: (usize, usize)| rise(p.0, p.1).min(12.0) - (longrange::map::v2(p.0 as f32 * CELL, p.1 as f32 * CELL) - c).len() * 0.01;
                    score(*a).total_cmp(&score(*b))
                });
                if let Some((x, y)) = best {
                    view.cam_target = longrange::map::v2(x as f32 * CELL, y as f32 * CELL);
                }
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
    init_headless_pool();
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
    // LR_MAP=<name> plays a premade map instead (one AI per start position).
    let (map, n) = match std::env::var("LR_MAP") {
        Ok(name) => {
            let map = longrange::data::load_map(&name).unwrap_or_else(|e| panic!("map {name}: {e}"));
            let k = map.starts.len().clamp(2, MAX_PLAYERS);
            (map, k)
        }
        Err(_) => {
            let mut map = MapData::generate(if n > 4 { 256 } else { 192 }, 12345 + seed, n);
            // LR_MOAT=1 digs water around the second start: nobody reaches the other by land.
            if std::env::var("LR_MOAT").is_ok() {
                let (c, water, side) = (map.starts[1], map.water, map.vx());
                for k in 0..map.heights.len() {
                    let d = v2((k % side) as f32 * CELL, (k / side) as f32 * CELL).dist(c);
                    if d > 46.0 && d < 56.0 {
                        map.heights[k] = water - 3.0;
                    }
                }
                map.deposits.retain(|d| d.dist(c) <= 44.0 || d.dist(c) >= 58.0);
                map.rebuild_blocked();
            }
            map.scatter_features(&defs.rules.features, 12345 + seed, 1.0);
            (map, n)
        }
    };
    println!("map: {} ({}x{} cells), {} trees and rocks, {} props", map.name, map.nx, map.ny, map.features.len(), map.props.len());
    let faction = defs.playable_factions()[0] as u8;
    // LR_AI=zones,overseer,simple,... picks the AI of every player in turn (default: one of each type).
    let kinds: Vec<AiKind> = std::env::var("LR_AI").ok().map(|s| s.split(',').filter_map(AiKind::parse).collect()).filter(|v: &Vec<AiKind>| !v.is_empty()).unwrap_or(AI_KINDS.to_vec());
    let kind = |i: usize| kinds[i % kinds.len()];
    // LR_TEAMS=2 puts the players into that many teams (player i in team i % n).
    let teams = std::env::var("LR_TEAMS").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(n).clamp(2, n);
    let setup: Vec<PlayerSetup> = (0..n).map(|i| PlayerSetup { name: format!("AI {i} {}", kind(i).name()), team: (i % teams) as u8, is_ai: true, slot: i, color: i as u8, faction }).collect();
    let world = app.world_mut();
    setup_game(world, map, &setup);
    for i in 0..n {
        world.resource_mut::<Game>().players[i].ai_kind = kind(i).id();
    }
    // LR_POINTS=n plays control points mode with that many starting points.
    if let Some(points) = std::env::var("LR_POINTS").ok().and_then(|s| s.parse::<f32>().ok()) {
        enable_control_points(world, points);
    }
    // The lobby options "infantry capture buildings" (on by default, LR_INFCAP=0: off) and
    // "rubble blocks paths" (from the data, LR_RUBBLE=0/1 overrides).
    world.resource_mut::<Game>().infantry_capture = std::env::var("LR_INFCAP").as_deref() != Ok("0");
    let rubble = std::env::var("LR_RUBBLE").map_or(world.resource::<Defs>().rules.rubble_blocks_paths, |v| v != "0");
    world.resource_mut::<Game>().rubble_blocks = rubble;
    // LR_WEATHER=rain (an id of `rules.weather`).
    if let Some(w) = env_weather(world.resource::<Defs>()) {
        world.resource_mut::<Game>().weather = w;
    }
    world.insert_resource(Ais((0..n).map(|i| Ai::with_kind(i as u8, 99 + seed, kind(i))).collect()));
    // LR_PROFILE=1 prints how long each tick system took at the end.
    if std::env::var("LR_PROFILE").is_ok() {
        world.insert_resource(longrange::sim::systems::SimProfile::default());
    }
    let t0 = std::time::Instant::now();
    let mut whole = longrange::sim::systems::SimProfile::default();
    let mut alive = vec![true; n];
    // LR_EVERY=600 prints the report every that many ticks (default 3000: every five minutes).
    let every = std::env::var("LR_EVERY").ok().and_then(|s| s.parse::<u32>().ok()).unwrap_or(3000).max(1);
    for t in 0..36000u32 {
        world.run_schedule(SimTick);
        let game = world.resource::<Game>().clone();
        for (k, p) in game.players.iter().enumerate() {
            if !p.alive && alive[k] {
                alive[k] = false;
                println!("   -- {} is out after {}s", p.name, t / 10);
            }
        }
        if t % every == 0 || game.winner.is_some() {
            let snap = make_snapshot(world);
            let bytes = bincode::serialize(&snap).map(|b| b.len()).unwrap_or(0);
            println!("t={:5}s units={} projectiles={} wrecks={} features touched={} fallen={} state={}B hash={:016x} wall={:?}", t / 10, snap.units.len(), snap.projectiles.len(), snap.wrecks.len(), snap.features.len(), snap.fallen.len(), bytes, state_hash(&snap), t0.elapsed());
            // With LR_PROFILE=1: what the ticks since the last report cost (the lag late in a game).
            if let Some(mut p) = world.get_resource_mut::<longrange::sim::systems::SimProfile>() {
                if p.ticks > 0 {
                    println!("   profile {}", p.short(6));
                    let part = std::mem::take(&mut *p);
                    whole.add(&part);
                }
            }
            for (k, p) in game.players.iter().enumerate() {
                let mine: Vec<&UnitSnap> = snap.units.iter().filter(|u| u.unit.owner as usize == k).collect();
                let top = mine.iter().map(|u| defs.get(u.unit.def).tier).max().unwrap_or(0);
                let army: f32 = mine.iter().map(|u| defs.get(u.unit.def)).filter(|d| !d.is_structure() && d.armed() && d.build.is_none()).map(|d| d.cost.mass).sum();
                let strategy = world.resource::<Ais>().0.iter().find(|a| a.player as usize == k).map_or(String::new(), |a| format!("\n        {}", a.debug()));
                let com = mine.iter().find(|u| defs.factions[p.faction as usize].commander == Some(u.unit.def)).map_or(String::new(), |u| format!(" commander hp {:.0} at {:.0} from start", u.health.0, u.body.pos.dist(p.start)));
                // LR_COMP=1 lists what every player has.
                if std::env::var("LR_COMP").is_ok() {
                    let mut n: std::collections::BTreeMap<&str, u32> = Default::default();
                    for u in &mine {
                        *n.entry(defs.get(u.unit.def).id.as_str()).or_default() += 1;
                    }
                    println!("        {}", n.iter().map(|(k, v)| format!("{k} {v}")).collect::<Vec<_>>().join(", "));
                }
                println!("   {} alive={} mass {:.0}/{:.0} +{:.1} -{:.1}  energy {:.0}/{:.0} +{:.0} -{:.0}  units={} army={:.0} top tier={}{}{}", p.name, p.alive, p.mass, p.max_mass, p.income_m, p.spend_m, p.energy, p.max_energy, p.income_e, p.spend_e, mine.len(), army, top, com, strategy);
            }
        }
        if game.winner.is_some() {
            println!("winner: team {:?} after {}s", game.winner, t / 10);
            break;
        }
    }
    if let Some(p) = world.get_resource::<longrange::sim::systems::SimProfile>() {
        whole.add(p);
        print!("{}", whole.report());
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
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
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
                    commands.spawn(Wreck { id, def: defs.id("crony").unwrap(), mass: 300.0, pos: v2(140.0 + k as f32 * 3.0, 140.0), y: 2.0, yaw: 0.0, ..Default::default() });
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
        let side = map.vx();
        for (i, h) in map.heights.iter_mut().enumerate() {
            let x = (i % side) as f32 * CELL;
            *h = if ridge && (x - 125.0).abs() < 6.0 { 22.0 } else { 2.0 };
        }
        map.rebuild_blocked();
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
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
    // 4b. Radar levels against stealth: each radar tower tier, targets of each stealth level at two
    //     distances on flat ground ("+" = on radar).
    println!("== Radar levels vs stealth (targets at 140 / 200)");
    for tower in ["radar_tower", "radar_tower2", "radar_tower3", "radar_car3"] {
        if defs.id(tower).is_none() {
            continue;
        }
        let mut line = format!("   {tower:13}");
        for target in ["crony", "gunship", "delivery_boy", "trooper"] {
            let mut app = headless(defs.clone());
            let mut map = MapData::generate(192, 4242, 2);
            map.heights.iter_mut().for_each(|h| *h = 2.0);
            map.rebuild_blocked();
            let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
            let world = app.world_mut();
            setup_game(world, map.clone(), &setup);
            spawn(world, &map, tower, 0, v2(60.0, 190.0), 0.0);
            let ids = [140.0, 200.0].map(|dist| spawn(world, &map, target, 1, v2(60.0 + dist, 190.0), 0.0));
            world.flush();
            for _ in 0..3 {
                world.run_schedule(SimTick);
            }
            let mut q = world.query::<(&Unit, &Intel)>();
            let marks: Vec<&str> = ids.iter().map(|id| if q.iter(world).any(|(u, i)| u.id == *id && i.radar & 1 != 0) { "+" } else { "-" }).collect();
            line += &format!("  {target} {}", marks.join(""));
        }
        println!("{line}");
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
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
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
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
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
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
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
                spawn_wreck(&mut commands, Wreck { id, def: defs.id("crony").unwrap(), mass: 200.0, pos: v2(110.0 + k as f32 * 4.0, 140.0), y: 2.0, yaw: 0.0, ..Default::default() });
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
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
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
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
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
    // 7d. Weapons never destroy reclaimables, only unfinished buildings: a Pike shells a grove with
    // a rock and a wreck in it (the pines fall, the rock shatters, the wreck is shoved about, all
    // keep their worth), a Crony shoots an enemy windmill left half built.
    {
        let mut app = headless(defs.clone());
        let mut map = MapData::generate(192, 4242, 2);
        map.heights.iter_mut().for_each(|h| *h = 2.0);
        map.rebuild_blocked();
        let (pine, rock) = (map.feature_kind("pine"), map.feature_kind("rock"));
        let spot = v2(170.0, 180.0);
        for k in 0..4 {
            map.features.push(Feature { kind: pine, pos: spot + V2::from_angle(k as f32 * 1.57) * 2.0, yaw: 0.0, scale: 1.0 });
        }
        map.features.push(Feature { kind: rock, pos: spot, yaw: 0.0, scale: 1.0 });
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
        setup_game(app.world_mut(), map.clone(), &setup);
        let world = app.world_mut();
        let pike = spawn(world, &map, "pike", 0, v2(100.0, 180.0), 0.0);
        spawn(world, &map, "crony", 0, v2(100.0, 100.0), 0.0);
        spawn(world, &map, "radar_car", 0, v2(96.0, 106.0), 0.0);
        let mut game = world.resource::<Game>().clone();
        let wreck_at = spot + v2(1.0, -2.5);
        let crony = defs.id("crony").unwrap();
        let wreck_mass = defs.get(crony).cost.mass * defs.rules.wreck_mass_fraction;
        let (wreck, site) = {
            let mut commands = world.commands();
            let id = game.alloc();
            spawn_wreck(&mut commands, Wreck { id, def: crony, mass: wreck_mass, pos: wreck_at, y: 2.0, yaw: 0.0, ..Default::default() });
            // A small piece of debris next to it (`id + 1`, allocated right after).
            let piece = game.alloc();
            spawn_wreck(&mut commands, Wreck { id: piece, def: crony, mass: 3.0, pos: spot + v2(-1.5, 1.5), y: 2.0, yaw: 0.0, piece: 0.6, ..Default::default() });
            let site = longrange::sim::spawn_unit_built(&mut commands, &defs, &mut game, &map, defs.id("windmill").unwrap(), 1, v2(140.0, 100.0), 0.0, Some(0.5), 0);
            (id, site)
        };
        world.insert_resource(game);
        for p in world.resource_mut::<Game>().players.iter_mut() {
            p.energy = 4000.0;
            p.max_energy = 4000.0;
        }
        world.flush();
        let before = make_snapshot(world);
        order(world, vec![pike], Order::AttackGround { pos: spot, radius: 3.0 });
        let mut site_gone = None;
        let (mut felled, mut moved, mut slid, mut thrown, mut flew) = (None, 0.0f32, false, 0.0f32, false);
        let piece_at = spot + v2(-1.5, 1.5);
        for t in 0..900u32 {
            world.run_schedule(SimTick);
            if site_gone.is_none() && !world.query::<&Unit>().iter(world).any(|u| u.id == site) {
                site_gone = Some(t + 1);
            }
            if felled.is_none() && (0..4).all(|i| world.resource::<Features>().list[i].fall != 0) {
                felled = Some(t + 1);
            }
            if let Some(w) = world.query::<&Wreck>().iter(world).find(|w| w.id == wreck) {
                moved = moved.max(w.pos.dist(wreck_at));
                slid |= w.vel.v().length() > 0.0;
            }
            if let Some(w) = world.query::<&Wreck>().iter(world).find(|w| w.id == wreck + 1) {
                thrown = thrown.max(w.pos.dist(piece_at));
                flew |= w.falling;
            }
        }
        let fs = world.resource::<Features>();
        let standing = (0..5).filter(|i| fs.alive(*i)).count();
        let worth = (0..5).map(|i| fs.list[i].left).fold(1.0f32, f32::min);
        let kept = world.query::<&Wreck>().iter(world).find(|w| w.id == wreck).map(|w| w.mass);
        let after = make_snapshot(world);
        restore(world, &before);
        let back = (0..5).all(|i| world.resource::<Features>().list[i].fall == 0) && world.query::<&Wreck>().iter(world).any(|w| w.id == wreck && w.pos == wreck_at);
        restore(world, &after);
        let same = state_hash(&make_snapshot(world)) == state_hash(&after);
        println!("== Weapons vs reclaimables: 4 pines felled after {felled:?} ticks (rock shattered: {}), {standing} of 5 still there ({:.0}% worth at least), wreck {} ({kept:?} of {wreck_mass:.0} mass) shoved {moved:.2} away (slid: {slid}), a small piece {thrown:.1} (thrown: {flew}).  Half-built enemy windmill destroyed after {site_gone:?} ticks.  Rollback restores them: {back}, hash equal {same}", world.resource::<Features>().list[4].fall != 0, worth * 100.0, if kept.is_some() { "kept" } else { "GONE" });
        assert!(felled.is_some() && standing == 5 && worth >= 1.0 && kept == Some(wreck_mass) && thrown > 1.0 && site_gone.is_some() && back && same, "weapons vs reclaimables and sites");
    }
    // 7d2. Rubble blocks paths (lobby option): a tank drives past a line of wrecks lying across its
    // way; with the option on it goes round them and never enters a cell they lie on.
    {
        let mut res = [(false, None::<u32>); 2];
        for (k, on) in [false, true].into_iter().enumerate() {
            let (mut app, map) = flat_world(192);
            let world = app.world_mut();
            world.resource_mut::<Game>().rubble_blocks = on;
            let tank = spawn(world, &map, "overtaker", 0, v2(120.0, 99.0), 0.0);
            let crony = defs.id("crony").unwrap();
            let mut game = world.resource::<Game>().clone();
            {
                let mut commands = world.commands();
                for j in 0..7 {
                    let at = v2(151.0, 93.0 + j as f32 * 2.0);
                    let id = game.alloc();
                    spawn_wreck(&mut commands, Wreck { id, def: crony, mass: 300.0, pos: at, y: map.h(at), yaw: 1.57, ..Default::default() });
                }
            }
            world.insert_resource(game);
            world.flush();
            order(world, vec![tank], Order::Move(v2(180.0, 99.0)));
            let lying: Vec<(i32, i32)> = (0..7).map(|j| map.cell_of(v2(151.0, 93.0 + j as f32 * 2.0))).collect();
            for t in 0..600u32 {
                world.run_schedule(SimTick);
                let p = world.query::<(&Unit, &Body)>().iter(world).find(|(u, _)| u.id == tank).map(|(_, b)| b.pos).unwrap();
                let (nav, map) = (world.resource::<Nav>(), world.resource::<MapData>());
                let (cx, cy) = map.cell_of(p);
                res[k].0 |= lying.contains(&(cx, cy)) || nav.occ[map.idx(cx, cy)] == longrange::map::OCC_RUBBLE;
                if res[k].1.is_none() && p.dist(v2(180.0, 99.0)) < 2.0 {
                    res[k].1 = Some(t + 1);
                }
            }
        }
        println!("== Rubble blocks paths: off - drove over a wreck: {}, there after {:?} ticks; on - drove over one: {}, there after {:?} ticks", res[0].0, res[0].1, res[1].0, res[1].1);
        assert!(res[0].0 && res[0].1.is_some() && !res[1].0 && res[1].1.is_some(), "rubble blocks paths");
    }
    // 7e. Debris: hard hits on a finished enemy factory break pieces off, which drop next to its
    // walls; a rollback while they fall brings them back in the air. Destroyed, it spills more
    // around its wreck, and wreck plus debris are worth exactly its wreck value. An engineer then
    // reclaims every piece.
    {
        let (mut app, map) = flat_world(192);
        let world = app.world_mut();
        let at = v2(120.0, 120.0);
        let fac = spawn(world, &map, "land_factory", 1, at, 0.0);
        let eng = spawn(world, &map, "engineer", 0, v2(104.0, 104.0), 0.0);
        world.flush();
        let d = defs.get(defs.id("land_factory").unwrap()).clone();
        let value = d.cost.mass * defs.rules.wreck_mass_fraction;
        let hit = |world: &mut World, share: f32| world.resource_mut::<Effects>().damage.push((fac, d.hp * share, 0));
        let pieces = |world: &mut World| -> Vec<Wreck> { world.query::<&Wreck>().iter(world).filter(|w| w.piece > 0.0).cloned().collect() };
        world.run_schedule(SimTick);
        hit(world, 0.15);
        world.run_schedule(SimTick);
        let first = pieces(world);
        let falling = first.iter().all(|w| w.falling);
        let mid = make_snapshot(world);
        for _ in 0..40 {
            world.run_schedule(SimTick);
        }
        let landed = pieces(world);
        let all_down = landed.len() == first.len() && landed.iter().all(|w| !w.falling);
        let near = landed.iter().map(|w| (w.pos - at).len2().sqrt()).fold((f32::MAX, 0.0f32), |(lo, hi), r| (lo.min(r), hi.max(r)));
        let later = state_hash(&make_snapshot(world));
        restore(world, &mid);
        let back_in_air = pieces(world).iter().all(|w| w.falling);
        for _ in 0..40 {
            world.run_schedule(SimTick);
        }
        let replay_same = state_hash(&make_snapshot(world)) == later;
        let shed = world.query::<(&Unit, &Shed)>().iter(world).find(|(u, _)| u.id == fac).map_or(0.0, |(_, s)| s.0);
        let first_mass: f32 = first.iter().map(|w| w.mass).sum();
        // Hit and repaired again and again: what it drops stops at its share of the wreck value.
        for k in 0..12 {
            hit(world, 0.15);
            for _ in 0..(if k == 11 { 60 } else { 5 }) {
                world.run_schedule(SimTick);
            }
            for (u, mut h) in world.query::<(&Unit, &mut Health)>().iter_mut(world) {
                if u.id == fac {
                    h.0 = d.hp;
                }
            }
        }
        let shed_all = world.query::<(&Unit, &Shed)>().iter(world).find(|(u, _)| u.id == fac).map_or(0.0, |(_, s)| s.0);
        hit(world, 20.0);
        for _ in 0..60 {
            world.run_schedule(SimTick);
        }
        let gone = !world.query::<&Unit>().iter(world).any(|u| u.id == fac);
        let total: f32 = world.query::<&Wreck>().iter(world).map(|w| w.mass).sum();
        let n_pieces = pieces(world).len();
        world.resource_mut::<Game>().players[0].mass = 0.0;
        let mass0 = 0.0;
        order(world, vec![eng], Order::ReclaimArea { pos: at, radius: 16.0 });
        let mut cleared = None;
        for t in 0..3000u32 {
            world.run_schedule(SimTick);
            if world.query::<&Wreck>().iter(world).count() == 0 {
                cleared = Some(t + 1);
                break;
            }
        }
        // (Includes the commander's income meanwhile.)
        let gained = world.resource::<Game>().players[0].mass - mass0;
        println!(
            "== Debris: a 15% hit dropped {} pieces ({first_mass:.1} mass, factory shed {shed:.1}), falling {falling}, all landed {all_down} {:.1}-{:.1} from the centre (walls at {:.0}); rollback puts them back in the air {back_in_air}, replay hash equal {replay_same}.  After 13 hits shed {shed_all:.0} of {value:.0} (cap {:.0}); destroyed: {gone}, wreck + {n_pieces} pieces worth {total:.1}.  Engineer reclaimed it all after {cleared:?} ticks, +{gained:.0} mass",
            first.len(),
            near.0,
            near.1,
            d.radius(),
            value * defs.rules.debris.max_shed
        );
        assert!(!first.is_empty() && falling && all_down && near.0 > d.radius() * 0.8 && near.1 < d.radius() * 1.5 + 4.0, "debris drops next to the walls");
        assert!(back_in_air && replay_same && (shed - first_mass).abs() < 0.01, "debris rollback");
        assert!(gone && (total - value).abs() < 0.05 && (value * defs.rules.debris.max_shed - shed_all).clamp(-0.01, defs.rules.debris.min_mass) == value * defs.rules.debris.max_shed - shed_all && cleared.is_some() && gained >= total - 0.5, "debris mass and reclaim");
    }
    // 8. Craters and terraforming, and a state that is saved and restored with them.
    {
        let (mut app, map) = flat_world(192);
        let world = app.world_mut();
        world.resource_mut::<Defs>().rules.terraforming = true; // off in the default data
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
        let riders: Vec<u32> = (0..3).map(|k| spawn(world, &map, "trooper", 0, v2(110.0 + k as f32 * 2.5, 146.0), 0.0)).collect();
        world.flush();
        order(world, vec![stork], Order::LoadArea { pos: v2(105.0, 101.0), radius: 12.0 });
        world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: vec![stork], orders: vec![Order::Unload { pos: v2(220.0, 220.0), radius: 6.0, occupy: false }], queue: true }));
        order(world, riders.clone(), Order::Load(apc));
        // Orders queued behind boarding are kept for after the ride.
        world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: riders.clone(), orders: vec![Order::Move(v2(166.0, 148.0))], queue: true }));
        let (mut most, mut in_apc) = (0, 0);
        for t in 0..900u32 {
            world.run_schedule(SimTick);
            let mut q = world.query::<&Hosted>();
            most = most.max(q.iter(world).filter(|h| h.0 == stork).count());
            in_apc = in_apc.max(q.iter(world).filter(|h| h.0 == apc).count());
            if t == 200 {
                world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: vec![apc], orders: vec![Order::Unload { pos: v2(150.0, 150.0), radius: 4.0, occupy: false }], queue: false }));
            }
        }
        let mut q = world.query::<(&Unit, &Body, Has<Hosted>)>();
        let moved = q.iter(world).filter(|(u, b, h)| tanks.contains(&u.id) && !h && b.pos.dist(v2(220.0, 220.0)) < 8.0).count();
        let left = q.iter(world).filter(|(u, b, h)| tanks.contains(&u.id) && !h && b.pos.dist(v2(105.0, 101.0)) < 14.0).count();
        let out = q.iter(world).filter(|(u, b, h)| riders.contains(&u.id) && !h && b.pos.dist(v2(166.0, 148.0)) < 7.0).count();
        let cap = defs.get(defs.id("air_transport").unwrap()).transport.as_ref().map_or(0, |t| t.capacity);
        println!("== Transport aircraft (capacity {cap}, a tank takes 4 slots) offered 10 tanks: carried {most}, set down at the target {moved}, left behind {left}.  APC: {in_apc} of 3 boarded, {out} set down and drove on to their queued goal");
        assert!(most == cap as usize / 4 && moved == most && left == 10 - most && in_apc == 3 && out == 3, "transports");
    }
    // 9a. Swift transport plane: scoops up troopers it flies over, then drops them over a dragged circle
    // without stopping; they fall spread over it and land about 2.5 s later.
    {
        let (mut app, map) = flat_world(192);
        let world = app.world_mut();
        let swift = spawn(world, &map, "fast_transport", 0, v2(60.0, 100.0), 0.0);
        let men: Vec<u32> = (0..8).map(|k| spawn(world, &map, "trooper", 0, v2(100.0 + (k % 4) as f32 * 1.5, 100.0 + (k / 4) as f32 * 1.5), 0.0)).collect();
        world.flush();
        let (target, radius) = (v2(220.0, 100.0), 14.0);
        let house = spawn(world, &map, "house", NEUTRAL, v2(224.0, 104.0), 0.0);
        order(world, vec![swift], Order::LoadArea { pos: v2(101.0, 100.7), radius: 10.0 });
        world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: vec![swift], orders: vec![Order::Unload { pos: target, radius, occupy: true }], queue: true }));
        let (mut most, mut slowest, mut first_fall, mut landed, mut high) = (0, f32::MAX, None, None, 0.0f32);
        let mut last = v2(0.0, 0.0);
        for t in 0..1500u32 {
            world.run_schedule(SimTick);
            let mut q = world.query::<&Hosted>();
            most = most.max(q.iter(world).filter(|h| h.0 == swift).count());
            let mut q = world.query::<(&Unit, &Body)>();
            let plane = q.iter(world).find(|(u, _)| u.id == swift).map(|(_, b)| b.pos).unwrap();
            let mut q = world.query::<(&Unit, &Body, &Mover)>();
            let falling = q.iter(world).filter(|(u, _, m)| men.contains(&u.id) && m.drop > 0.0).count();
            if falling > 0 {
                first_fall.get_or_insert(t);
                if t > 0 {
                    slowest = slowest.min(plane.dist(last) / 0.1);
                }
                for (u, b, _) in q.iter(world).filter(|(u, _, m)| men.contains(&u.id) && m.drop > 0.0) {
                    let _ = u;
                    high = high.max(b.y);
                }
            }
            if first_fall.is_some() && falling == 0 && landed.is_none() {
                landed = Some(t);
            }
            last = plane;
        }
        let mut q = world.query::<(&Unit, &Body, Has<Hosted>)>();
        let spots: Vec<V2> = q.iter(world).filter(|(u, _, h)| men.contains(&u.id) && !h).map(|(_, b, _)| b.pos).collect();
        let inside = spots.iter().filter(|p| p.dist(target) <= radius + 2.0).count();
        let in_house = world.query::<&Hosted>().iter(world).filter(|h| h.0 == house).count();
        let spread = spots.iter().map(|p| p.dist(target)).fold(0.0, f32::max);
        let secs = landed.zip(first_fall).map_or(0.0, |(l, f)| (l - f) as f32 * 0.1);
        println!("== Swift airdrop: {most} of 8 scooped up, {inside} landed in the circle and {in_house} walked into the house there (widest {spread:.1} of {radius}), first fall at tick {first_fall:?}, all down {secs:.1} s after, plane never slower than {slowest:.1}, highest rider {high:.1}");
        assert!(most == 8 && inside + in_house == 8 && in_house > 0 && spread > radius * 0.5 && slowest > 15.0 && (2.0..4.0).contains(&secs) && high > 4.0, "swift airdrop");
    }
    // 9b. Ferry route: a transport shuttles between its beacon and the drop point until everybody
    // waiting at the beacon is across; they then go on with the orders queued behind the ferry.
    {
        let (mut app, map) = flat_world(192);
        let world = app.world_mut();
        let stork = spawn(world, &map, "air_transport", 0, v2(60.0, 60.0), 0.0);
        let tanks: Vec<u32> = (0..6).map(|k| spawn(world, &map, "crony", 0, v2(70.0 + (k % 4) as f32 * 2.5, 48.0 + (k / 4) as f32 * 2.5), 0.0)).collect();
        world.flush();
        let (beacon, drop, on) = (v2(64.0, 56.0), v2(250.0, 250.0), v2(262.0, 236.0));
        order(world, vec![stork], Order::Ferry { pos: beacon, pick: true });
        world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: vec![stork], orders: vec![Order::Ferry { pos: drop, pick: false }], queue: true }));
        order(world, tanks.clone(), Order::Ferry { pos: beacon, pick: true });
        world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: tanks.clone(), orders: vec![Order::Move(on)], queue: true }));
        let (mut trips, mut was_full, mut done) = (0, false, None);
        for t in 0..4000u32 {
            world.run_schedule(SimTick);
            let mut q = world.query::<&Hosted>();
            let n = q.iter(world).filter(|h| h.0 == stork).count();
            if n > 0 && !was_full {
                trips += 1;
            }
            was_full = n > 0;
            let mut q = world.query::<(&Unit, &Body, Has<Hosted>)>();
            if done.is_none() && q.iter(world).filter(|(u, b, h)| tanks.contains(&u.id) && !h && b.pos.dist(on) < 9.0).count() == tanks.len() {
                done = Some(t);
            }
        }
        let route = world.query::<(&Unit, &Orders)>().iter(world).find(|(u, _)| u.id == stork).map_or(false, |(_, o)| o.0.iter().any(|o| matches!(o, Order::Ferry { pick: true, .. })));
        println!("== Ferry: 6 tanks waiting at the beacon, all across and at their queued goal after {done:?} ticks in {trips} trips; route kept: {route}");
        assert!(done.is_some() && trips >= 2 && route, "ferry");
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
    air_tests(&defs);
    infantry_tests(&defs);
    fieldwork_tests(&defs);
    binocular_tests(&defs);
    scavenger_tests(&defs);
    apc_tests(&defs);
}

/// The armoured carriers (Tavria, Slavuta, Zaporozhets): small arms barely scratch them, their
/// infantry bail out of the wreck, and the Slavuta floats over water of any depth where
/// other vehicles stop (`--combattest`, or alone with `LR_TEST=apc`).
fn apc_tests(defs: &Defs) {
    let carriers: Vec<&str> = ["apc", "apc_amph", "apc_heavy", "apc_heavy3"].into_iter().filter(|c| defs.id(c).is_some()).collect();
    if carriers.is_empty() {
        return;
    }
    let faction = defs.playable_factions()[0] as u8;
    // A flat range, with a river across it (bed `bed` under the water) when given.
    let world_with = |bed: Option<f32>| -> (App, MapData) {
        let mut app = headless(defs.clone());
        let mut map = MapData::generate(128, 4242, 2);
        map.water = 0.0;
        map.lakes.clear();
        map.lake_of.clear();
        map.feature_kinds = vec![];
        map.features = vec![];
        let side = map.vx();
        for (i, h) in map.heights.iter_mut().enumerate() {
            let x = (i % side) as f32 * CELL;
            *h = match bed {
                Some(bed) => ((x - 130.0).abs() * 0.7 - 6.0).clamp(bed, 2.0),
                None => 2.0,
            };
        }
        map.rebuild_blocked();
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
        setup_game(app.world_mut(), map.clone(), &setup);
        (app, map)
    };
    let spawn = |world: &mut World, map: &MapData, name: &str, owner: u8, pos: V2, yaw: f32| -> u32 {
        let mut game = world.resource::<Game>().clone();
        let id = {
            let mut commands = world.commands();
            spawn_unit(&mut commands, defs, &mut game, map, defs.id(name).expect("unit"), owner, pos, yaw, true, 0)
        };
        world.insert_resource(game);
        world.flush();
        world.resource_mut::<Nav>().dirty = true;
        id
    };
    let order = |world: &mut World, owner: u8, ids: Vec<u32>, o: Order| world.resource_mut::<CommandQueue>().0.push((owner, Command::Orders { ids, orders: vec![o], queue: false }));
    let hp = |world: &mut World, id: u32| -> f32 { world.query::<(&Unit, &Health)>().iter(world).find(|(u, _)| u.id == id).map_or(0.0, |(_, h)| h.0) };
    let set_hp = |world: &mut World, id: u32, v: f32| {
        if let Some((_, mut h)) = world.query::<(&Unit, &mut Health)>().iter_mut(world).find(|(u, _)| u.id == id) {
            h.0 = v;
        }
    };
    let body = |world: &mut World, id: u32| -> Option<(V2, f32, bool)> { world.query::<(&Unit, &Body, Has<Hosted>)>().iter(world).find(|(u, ..)| u.id == id).map(|(_, b, h)| (b.pos, b.y, h)) };
    // 1. Six riflemen and an HMG tower fire at each carrier for 30 s; then an Overtaker's gun.
    for &name in &carriers {
        let full = defs.get(defs.id(name).unwrap()).hp;
        let (mut app, map) = world_with(None);
        let world = app.world_mut();
        let apc = spawn(world, &map, name, 0, v2(130.0, 128.0), 3.14);
        for i in 0..6 {
            spawn(world, &map, "trooper", 1, v2(118.0, 120.0 + i as f32 * 3.0), 0.0);
        }
        spawn(world, &map, "hmg_turret", 1, v2(150.0, 128.0), 3.14);
        for _ in 0..300 {
            world.run_schedule(SimTick);
        }
        let left = hp(world, apc);
        let (mut app, map) = world_with(None);
        let world = app.world_mut();
        let apc = spawn(world, &map, name, 0, v2(130.0, 128.0), 1.57);
        spawn(world, &map, "overtaker", 1, v2(130.0, 110.0), 1.57);
        let mut killed = None;
        for t in 0..1200u32 {
            world.run_schedule(SimTick);
            if hp(world, apc) <= 0.0 {
                killed = Some(t as f32 / 10.0);
                break;
            }
        }
        println!("== {name} ({full:.0} hp) under small arms: 6 riflemen + an HMG tower for 30 s leave it {left:.0} hp; an Overtaker kills it in {killed:?} s");
        assert!(left > full * 0.2 && killed.is_some(), "small arms barely scratch an armoured carrier, a tank gun does not");
    }

    // 1b. Two AT troopers fire missiles at each heavy carrier for 60 s: reactive armor takes the edge off.
    //     And its roof machine gun against three riflemen.
    for &name in carriers.iter().filter(|c| !defs.get(defs.id(c).unwrap()).weapons.is_empty()) {
        let (mut app, map) = world_with(None);
        let world = app.world_mut();
        let apc = spawn(world, &map, name, 0, v2(135.0, 129.0), 3.14);
        set_hp(world, apc, 1.0e5);
        for i in 0..2 {
            let at = spawn(world, &map, "at_trooper", 1, v2(100.0, 125.0 + i as f32 * 6.0), 0.0);
            set_hp(world, at, 1.0e6);
        }
        for _ in 0..600 {
            world.run_schedule(SimTick);
        }
        let lost = 1.0e5 - hp(world, apc);
        let (mut app, map) = world_with(None);
        let world = app.world_mut();
        spawn(world, &map, name, 0, v2(130.0, 128.0), 3.14);
        let men: Vec<u32> = (0..3).map(|i| spawn(world, &map, "trooper", 1, v2(112.0, 124.0 + i as f32 * 4.0), 0.0)).collect();
        for _ in 0..300 {
            world.run_schedule(SimTick);
        }
        let dead = men.iter().filter(|&&m| hp(world, m) <= 0.0).count();
        println!("== {name}: 2 AT troopers in 60 s take {lost:.0} hp (reactive armor: {}); its machine gun kills {dead} of 3 riflemen in 30 s", defs.get(defs.id(name).unwrap()).reactive.is_some());
        assert!(dead > 0, "a carrier's machine gun kills riflemen");
    }

    // 2. Destroyed with four riflemen inside: they bail out, hurt.
    let (mut app, map) = world_with(None);
    let world = app.world_mut();
    let apc = spawn(world, &map, carriers[0], 0, v2(130.0, 128.0), 0.0);
    let men: Vec<u32> = (0..4).map(|i| spawn(world, &map, "trooper", 0, v2(124.0, 124.0 + i as f32 * 2.5), 0.0)).collect();
    order(world, 0, men.clone(), Order::Load(apc));
    for _ in 0..300 {
        world.run_schedule(SimTick);
        if men.iter().all(|&m| body(world, m).is_some_and(|b| b.2)) {
            break;
        }
    }
    let boarded = men.iter().filter(|&&m| body(world, m).is_some_and(|b| b.2)).count();
    world.resource_mut::<CommandQueue>().0.push((0, Command::SelfDestruct { ids: vec![apc] }));
    for _ in 0..20 {
        world.run_schedule(SimTick);
    }
    let out = men.iter().filter(|&&m| body(world, m).is_some_and(|b| !b.2)).count();
    println!("== {} destroyed with {boarded} riflemen inside: {out} bail out", carriers[0]);
    assert!(boarded == 4 && out == 4, "infantry bail out of a destroyed carrier");

    // 2b. A land factory queues the carrier with a loadout: the riders are built after it, inside,
    //     and it leaves with them aboard; the whole job is paid (transport + riders).
    let (mut app, map) = world_with(None);
    let world = app.world_mut();
    let fac = spawn(world, &map, "land_factory", 0, v2(100.0, 100.0), 0.0);
    {
        let mut game = world.resource_mut::<Game>();
        let p = &mut game.players[0];
        p.mass = 5000.0;
        p.energy = 50000.0;
        p.max_mass = 5000.0;
        p.max_energy = 50000.0;
    }
    let fac_def = defs.id("land_factory").unwrap();
    let host = defs.id(carriers[0]).unwrap();
    let cap = defs.get(host).transport.as_ref().map_or(0, |t| t.capacity) as usize;
    let rider = defs.id("trooper").unwrap();
    let cargo = vec![rider; cap];
    assert!(loadout_ok(defs, fac_def, host, &cargo), "the land factory fills its own carrier with riflemen");
    assert!(!loadout_ok(defs, fac_def, host, &vec![rider; cap + 1]), "no more riders than the carrier holds");
    world.run_schedule(SimTick);
    let mut paid = 0.0;
    let mut last_mass = world.resource::<Game>().players[0].mass;
    world.resource_mut::<CommandQueue>().0.push((0, Command::ProduceLoaded { factory: fac, def: host, cargo: cargo.clone(), count: 1 }));
    let mut out_at = None;
    for t in 0..6000 {
        world.run_schedule(SimTick);
        // Real mass taken (income added back; `spend_m` shows demand during a stall).
        let pl = &world.resource::<Game>().players[0];
        paid += last_mass - pl.mass + pl.income_m * DT;
        last_mass = pl.mass;
        let left = world.query::<(&Unit, Has<Hosted>)>().iter(world).find(|(u, h)| u.def == host && !h).map(|(u, _)| u.id);
        if let Some(c) = left {
            out_at = Some((c, t));
            break;
        }
    }
    let (c, t) = out_at.expect("the filled carrier leaves the factory");
    let aboard = world.query::<(&Unit, &Hosted)>().iter(world).filter(|(u, h)| h.0 == c && u.def == rider).count();
    let want = defs.get(host).cost.mass + cap as f32 * defs.get(rider).cost.mass;
    println!("== {} queued with {cap} riflemen: out after {:.0} s with {aboard} aboard, ~{paid:.0} mass paid (job {want:.0})", carriers[0], t as f32 * DT);
    assert!(aboard == cap, "the carrier comes out with its loadout aboard");
    assert!(paid > want * 0.9 && paid < want * 1.1, "the job costs the transport and its riders");

    // 3. Rivers 2.5 and 6 deep: the Slavuta floats across both, an Overtaker cannot.
    let Some(&amph) = carriers.iter().find(|c| defs.get(defs.id(c).unwrap()).mv.amphibious) else { return };
    for (bed, crosses) in [(-2.5f32, true), (-6.0, true)] {
        let (mut app, map) = world_with(Some(bed));
        let world = app.world_mut();
        let apc = spawn(world, &map, amph, 0, v2(100.0, 128.0), 0.0);
        let tank = spawn(world, &map, "overtaker", 0, v2(100.0, 140.0), 0.0);
        order(world, 0, vec![apc], Order::Move(v2(160.0, 128.0)));
        order(world, 0, vec![tank], Order::Move(v2(160.0, 140.0)));
        let mut lowest = f32::MAX;
        for _ in 0..600 {
            world.run_schedule(SimTick);
            lowest = lowest.min(body(world, apc).map_or(f32::MAX, |b| b.1));
        }
        let (ax, tx) = (body(world, apc).unwrap().0.x, body(world, tank).unwrap().0.x);
        println!("== River with its bed at {bed}: {amph} at x {ax:.0} (lowest {lowest:.1}, water 0), Overtaker at x {tx:.0} (goal 160)");
        assert!(tx < 130.0, "an Overtaker does not drive into the river");
        if crosses {
            assert!(ax > 155.0 && lowest > -0.1, "an amphibious carrier floats across the river on its surface");
        } else {
            assert!(ax < 130.0, "a river 6 deep stops an amphibious carrier");
        }
    }
}

/// Scavengers on every kind of reclaimable, reviving a building, and a commander killed inside a
/// transport (`--combattest`, or alone with `LR_TEST=scavenger`).
fn scavenger_tests(defs: &Defs) {
    let faction = defs.playable_factions()[0] as u8;
    let mut app = headless(defs.clone());
    let mut map = MapData::generate(128, 4242, 2);
    map.heights.iter_mut().for_each(|h| *h = 2.0);
    map.feature_kinds = vec!["pine".into(), "rock".into()];
    map.features = vec![];
    for k in 0..4 {
        map.features.push(Feature { kind: 0, pos: v2(150.0 + k as f32 * 2.5, 100.0), yaw: 0.0, scale: 1.0 });
    }
    map.features.push(Feature { kind: 1, pos: v2(154.0, 104.0), yaw: 0.0, scale: 1.0 });
    map.rebuild_blocked();
    let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
    setup_game(app.world_mut(), map.clone(), &setup);
    let world = app.world_mut();
    for p in world.resource_mut::<Game>().players.iter_mut() {
        p.energy = 100000.0;
        p.max_energy = 100000.0;
        p.max_mass = 100000.0;
    }
    let spawn = |world: &mut World, name: &str, owner: u8, pos: V2| -> u32 {
        let mut game = world.resource::<Game>().clone();
        let id = {
            let mut commands = world.commands();
            spawn_unit(&mut commands, defs, &mut game, &map, defs.id(name).expect("unit"), owner, pos, 0.0, true, 0)
        };
        world.insert_resource(game);
        world.flush();
        world.resource_mut::<Nav>().dirty = true;
        id
    };
    let order = |world: &mut World, ids: Vec<u32>, o: Order| world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids, orders: vec![o], queue: false }));
    let mill = spawn(world, "windmill", 1, v2(100.0, 60.0));
    let tank = spawn(world, "crony", 1, v2(110.0, 60.0));
    let scav = spawn(world, "scavenger", 0, v2(100.0, 80.0));
    world.resource_mut::<CommandQueue>().0.push((1, Command::SelfDestruct { ids: vec![mill, tank] }));
    for _ in 0..40 {
        world.run_schedule(SimTick);
    }
    let wrecks: Vec<(u32, longrange::data::DefId, f32)> = world.query::<&Wreck>().iter(world).filter(|w| w.piece <= 0.0).map(|w| (w.id, w.def, w.mass)).collect();
    let pieces = world.query::<&Wreck>().iter(world).filter(|w| w.piece > 0.0).count();
    let wid = |name: &str| wrecks.iter().find(|w| w.1 == defs.id(name).unwrap()).map(|w| w.0);
    // 1. Revive the windmill's wreck, then the tank's.
    let (mw, tw) = (wid("windmill").expect("windmill wreck"), wid("crony").expect("tank wreck"));
    order(world, vec![scav], Order::Revive(mw));
    world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: vec![scav], orders: vec![Order::Revive(tw)], queue: true }));
    let mut back: [Option<u32>; 2] = [None; 2];
    for t in 0..3000u32 {
        world.run_schedule(SimTick);
        world.resource_mut::<Game>().players[0].mass = 5000.0;
        for (u, uc) in world.query::<(&Unit, Has<UnderConstruction>)>().iter(world) {
            for (k, name) in ["windmill", "crony"].iter().enumerate() {
                if u.owner == 0 && !uc && defs.get(u.def).id == *name && back[k].is_none() {
                    back[k] = Some(t);
                }
            }
        }
        if back.iter().all(|b| b.is_some()) {
            break;
        }
    }
    // 2. One area reclaim takes the debris, the trees and the rock.
    world.resource_mut::<Game>().players[0].mass = 0.0;
    let mass0 = 0.0;
    order(world, vec![scav], Order::ReclaimArea { pos: v2(125.0, 85.0), radius: 45.0 });
    let mut cleared = None;
    for t in 0..4000u32 {
        world.run_schedule(SimTick);
        let left_f = world.resource::<Features>().list.iter().filter(|f| f.left > 0.0).count();
        let left_w = world.query::<&Wreck>().iter(world).count();
        if left_f == 0 && left_w == 0 {
            cleared = Some(t);
            break;
        }
    }
    let gained = world.resource::<Game>().players[0].mass - mass0;
    println!("== Scavenger: windmill wreck revived after {:?} ticks, tank wreck after {:?} (wrecks {}, debris pieces {pieces}).  Area reclaim of debris + 4 pines + a rock done after {cleared:?} ticks, {gained:.0} mass", back[0], back[1], wrecks.len());
    assert!(back.iter().all(|b| b.is_some()) && cleared.is_some(), "scavenger");

    // 3. Teamwork: two scavengers on one area revive work on the same wreck when it is close, a
    // third one assisting the first helps with whatever it does, an engineer cannot help revive.
    let t1 = spawn(world, "crony", 1, v2(60.0, 150.0));
    let t2 = spawn(world, "crony", 1, v2(67.0, 150.0));
    world.resource_mut::<CommandQueue>().0.push((1, Command::SelfDestruct { ids: vec![t1, t2] }));
    for _ in 0..40 {
        world.run_schedule(SimTick);
    }
    let sa = spawn(world, "scavenger", 0, v2(58.0, 170.0));
    let sb = spawn(world, "scavenger", 0, v2(72.0, 172.0));
    let sc = spawn(world, "scavenger", 0, v2(50.0, 175.0));
    let eng = spawn(world, "engineer", 0, v2(64.0, 170.0));
    order(world, vec![sa, sb], Order::ReviveArea { pos: v2(63.0, 150.0), radius: 10.0 });
    order(world, vec![sc], Order::Assist(sa));
    let work_of = |world: &mut World, id: u32| world.query::<(&Unit, &Builder)>().iter(world).find(|(u, _)| u.id == id).map_or(0, |(_, b)| b.work);
    let (mut shared, mut helped, mut eng_worked, mut eng_order) = (false, false, false, false);
    let mut revived = None;
    for t in 0..3000u32 {
        world.run_schedule(SimTick);
        world.resource_mut::<Game>().players[0].mass = 5000.0;
        let (wa, wb, wc) = (work_of(world, sa), work_of(world, sb), work_of(world, sc));
        shared |= wa != 0 && wa == wb;
        helped |= wa != 0 && wa == wc;
        // The engineer is told to assist whatever revival site exists.
        let site = world.query::<(&Unit, Has<UnderConstruction>)>().iter(world).find(|(u, uc)| u.owner == 0 && *uc && defs.get(u.def).id == "crony").map(|(u, _)| u.id);
        if let (Some(s), false) = (site, eng_order) {
            order(world, vec![eng], Order::Assist(s));
            eng_order = true;
        }
        eng_worked |= site.map_or(false, |s| work_of(world, eng) == s);
        let n = world.query::<(&Unit, Has<UnderConstruction>)>().iter(world).filter(|(u, uc)| u.owner == 0 && !*uc && defs.get(u.def).id == "crony").count();
        if n >= 3 {
            revived = Some(t);
            break;
        }
    }
    println!("== Scavenger teamwork: both wrecks revived after {revived:?} ticks; two on one revival: {shared}, assisting scavenger helped: {helped}, engineer helped revive: {eng_worked}");
    assert!(revived.is_some() && shared && helped && !eng_worked, "scavenger teamwork");

    // 4. A commander riding in a transport aircraft dies with it, and so does its side.
    let com = world.query::<&Unit>().iter(world).find(|u| u.owner == 0 && defs.factions[faction as usize].commander == Some(u.def)).map(|u| u.id).expect("commander");
    let cpos = world.query::<(&Unit, &Body)>().iter(world).find(|(u, _)| u.id == com).map(|(_, b)| b.pos).unwrap();
    let apc = spawn(world, "air_transport", 0, cpos + v2(4.0, 0.0));
    order(world, vec![com], Order::Load(apc));
    let mut boarded = false;
    for _ in 0..300u32 {
        world.run_schedule(SimTick);
        boarded = world.query::<&Hosted>().iter(world).any(|h| h.0 == apc);
        if boarded {
            break;
        }
    }
    world.resource_mut::<CommandQueue>().0.push((0, Command::SelfDestruct { ids: vec![apc] }));
    for _ in 0..20 {
        world.run_schedule(SimTick);
    }
    let alive = world.resource::<Game>().players[0].alive;
    let com_left = world.query::<&Unit>().iter(world).any(|u| u.id == com);
    println!("== Commander in a transport aircraft: boarded {boarded}; APC destroyed -> commander still there: {com_left}, player alive: {alive}");
    assert!(boarded && !com_left && !alive, "commander in a transport");
}

/// Cover behind trees, rocks and wrecks, infantry looking for it, and infantry in buildings
/// (`--combattest`, or alone with `LR_TEST=infantry`); also the mortar hold.
fn infantry_tests(defs: &Defs) {
    let faction = defs.playable_factions()[0] as u8;
    // A flat test range with the given trees and rocks: (feature type, position).
    let world_with = |features: &[(&str, V2)]| -> (App, MapData) {
        let mut app = headless(defs.clone());
        let mut map = MapData::generate(128, 4242, 2);
        map.heights.iter_mut().for_each(|h| *h = 2.0);
        map.feature_kinds = vec![];
        map.features = vec![];
        for (kind, pos) in features {
            let k = match map.feature_kinds.iter().position(|x| x == kind) {
                Some(k) => k,
                None => {
                    map.feature_kinds.push(kind.to_string());
                    map.feature_kinds.len() - 1
                }
            };
            map.features.push(Feature { kind: k as u16, pos: *pos, yaw: 0.0, scale: 1.0 });
        }
        map.rebuild_blocked();
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
        setup_game(app.world_mut(), map.clone(), &setup);
        for p in app.world_mut().resource_mut::<Game>().players.iter_mut() {
            p.energy = 100000.0;
            p.max_energy = 100000.0;
        }
        (app, map)
    };
    let spawn = |world: &mut World, map: &MapData, name: &str, owner: u8, pos: V2, yaw: f32| -> u32 {
        let mut game = world.resource::<Game>().clone();
        let id = {
            let mut commands = world.commands();
            spawn_unit(&mut commands, defs, &mut game, map, defs.id(name).expect("unit"), owner, pos, yaw, true, 0)
        };
        world.insert_resource(game);
        world.flush();
        world.resource_mut::<Nav>().dirty = true;
        id
    };
    let order = |world: &mut World, owner: u8, ids: Vec<u32>, o: Order| world.resource_mut::<CommandQueue>().0.push((owner, Command::Orders { ids, orders: vec![o], queue: false }));
    let hp = |world: &mut World, id: u32| -> f32 { world.query::<(&Unit, &Health)>().iter(world).find(|(u, _)| u.id == id).map_or(0.0, |(_, h)| h.0) };
    let set_hp = |world: &mut World, id: u32, v: f32| {
        if let Some((_, mut h)) = world.query::<(&Unit, &mut Health)>().iter_mut(world).find(|(u, _)| u.id == id) {
            h.0 = v;
        }
    };
    let body = |world: &mut World, id: u32| -> Option<(V2, bool)> { world.query::<(&Unit, &Body, Has<Hosted>)>().iter(world).find(|(u, ..)| u.id == id).map(|(_, b, h)| (b.pos, h)) };
    let owner_of = |world: &mut World, id: u32| -> Option<u8> { world.query::<&Unit>().iter(world).find(|u| u.id == id).map(|u| u.owner) };

    // 0. Reactive armor: two AT troopers fire at a Bastion for 60 s, with its charges and with them
    // used up; then an engineer repairs it to full and the charges come back.
    if defs.id("bastion").is_some() {
        let charges = |world: &mut World, id: u32| world.query::<(&Unit, &Reactive)>().iter(world).find(|(u, _)| u.id == id).map(|(_, r)| r.0);
        let mut lost = [0.0f32; 2];
        let mut left = [0u8; 2];
        for (k, era) in [true, false].into_iter().enumerate() {
            let (mut app, map) = world_with(&[]);
            let world = app.world_mut();
            let tank = spawn(world, &map, "bastion", 1, v2(135.0, 129.0), 3.14);
            set_hp(world, tank, 1.0e5);
            if !era {
                for (u, mut r) in world.query::<(&Unit, &mut Reactive)>().iter_mut(world) {
                    if u.id == tank {
                        r.0 = 0;
                    }
                }
            }
            for i in 0..2 {
                let at = spawn(world, &map, "at_trooper", 0, v2(100.0, 125.0 + i as f32 * 6.0), 0.0);
                set_hp(world, at, 1.0e6);
            }
            for _ in 0..600 {
                world.run_schedule(SimTick);
            }
            lost[k] = 1.0e5 - hp(world, tank);
            left[k] = charges(world, tank).unwrap_or(0);
        }
        // Repair: a damaged Bastion with no charges left, an engineer next to it.
        let (mut app, map) = world_with(&[]);
        let world = app.world_mut();
        let tank = spawn(world, &map, "bastion", 0, v2(130.0, 129.0), 0.0);
        let full = defs.get(defs.id("bastion").unwrap()).reactive.map_or(0, |r| r.charges);
        let start_hp = defs.get(defs.id("bastion").unwrap()).hp * 0.6;
        set_hp(world, tank, start_hp);
        for (u, mut r) in world.query::<(&Unit, &mut Reactive)>().iter_mut(world) {
            if u.id == tank {
                r.0 = 0;
            }
        }
        let eng = spawn(world, &map, "engineer2", 0, v2(124.0, 129.0), 0.0);
        order(world, 0, vec![eng], Order::Assist(tank));
        for _ in 0..1500 {
            world.run_schedule(SimTick);
        }
        let (hp_after, back) = (hp(world, tank), charges(world, tank).unwrap_or(0));
        println!("== Reactive armor: 2 AT troopers on a Bastion for 60 s take {:.0} hp with its {full} charges ({} left), {:.0} without; repaired from {start_hp:.0} to {hp_after:.0} hp, charges back to {back}", lost[0], left[0], lost[1]);
        assert!(left[0] == 0 && lost[0] < lost[1], "reactive armor stops part of the missile damage");
        assert!(hp_after >= defs.get(defs.id("bastion").unwrap()).hp && back == full, "a full repair replaces the reactive armor");
    }

    // Counter-battery radar: a Pike shells a tank 90 away; the tank's side has a radar car 80
    // beyond it, out of reach of the Pike itself but covering where its shells come down.
    if defs.id("pike").is_some() && defs.id("radar_car").is_some() {
        let mut found = [None; 2];
        for (k, on) in [true, false].into_iter().enumerate() {
            let (mut app, map) = world_with(&[]);
            let world = app.world_mut();
            world.resource_mut::<Defs>().rules.counter_battery = on;
            let pike = spawn(world, &map, "pike", 0, v2(30.0, 128.0), 0.0);
            spawn(world, &map, "radar_car", 0, v2(34.0, 134.0), 0.0);
            let tank = spawn(world, &map, "overtaker", 1, v2(120.0, 128.0), 3.14);
            set_hp(world, tank, 1.0e6);
            spawn(world, &map, "radar_car", 1, v2(200.0, 128.0), 3.14);
            for t in 0..600 {
                world.run_schedule(SimTick);
                let traced = world.query::<(&Unit, &Intel)>().iter(world).any(|(u, i)| u.id == pike && i.radar & 2 != 0);
                if traced && found[k].is_none() {
                    found[k] = Some(t as f32 / 10.0);
                }
            }
        }
        println!("== Counter-battery: a Pike firing into enemy radar cover is found after {}; with the rule off {}", found[0].map_or("never".to_string(), |s| format!("{s:.1} s")), found[1].map_or("never".to_string(), |s| format!("after {s:.1} s")));
        assert!(found[0].is_some() && found[1].is_none(), "counter-battery radar finds the gun only with the rule on");
    }

    // 1. A boulder between a tank and its target 10 in front of it: the tank does not shoot
    // through it (line of fire) but steps aside a little to get a clear shot, and shells sent through it
    // anyway (ground fire at the target) lose part of their damage to it, which never wears it away.
    let mut lost = [0.0f32; 4];
    let mut left = 1.0;
    let (mut stepped, mut gap) = (0.0f32, 0.0f32);
    for (k, (rock, ground)) in [(false, false), (true, false), (false, true), (true, true)].into_iter().enumerate() {
        // (Ground fire: right in front of the target, so every shell flies through it.)
        let boulder = [("boulder", v2(if ground { 146.0 } else { 140.0 }, 100.0))];
        let (mut app, map) = world_with(if rock { &boulder[..] } else { &[] });
        let world = app.world_mut();
        let tank = spawn(world, &map, "crony", 0, v2(100.0, 100.0), 0.0);
        // Radar sees through the rock (sight does not).
        spawn(world, &map, "radar_car", 0, v2(96.0, 106.0), 0.0);
        let target = spawn(world, &map, "overtaker", 1, v2(150.0, 100.0), 3.14);
        set_hp(world, target, 1.0e6);
        if ground {
            order(world, 0, vec![tank], Order::AttackGround { pos: v2(150.0, 100.0), radius: 0.5 });
        }
        for _ in 0..300 {
            world.run_schedule(SimTick);
        }
        lost[k] = 1.0e6 - hp(world, target);
        if rock && ground {
            left = world.resource::<Features>().list[0].left;
        }
        if rock && !ground {
            let p = world.query::<(&Unit, &Body)>().iter(world).find(|(u, _)| u.id == tank).map(|(_, b)| b.pos).unwrap();
            (stepped, gap) = (p.dist(v2(100.0, 100.0)), p.dist(v2(150.0, 100.0)));
        }
    }
    println!("== Cover: a Crony with an Overtaker 50 away for 30 s does {:.0} damage in the open, {:.0} with a boulder in between (stepped {:.1} aside, {:.0} from the target); ground fire at it {:.0} in the open, {:.0} through the boulder ({:.0}% of the boulder left)", lost[0], lost[1], stepped, gap, lost[2], lost[3], left * 100.0);
    assert!(lost[0] > 0.0 && lost[1] > lost[0] * 0.3 && stepped > 1.0 && stepped < 30.0 && gap > 30.0, "a tank behind cover steps aside to shoot");
    assert!(lost[2] > 0.0 && lost[3] > 0.0 && lost[3] < lost[2] * 0.7 && left >= 1.0, "cover soaks shots and is not worn away");

    // 1b. Armor angle: a Crony shelling an Overtaker for 30 s that faces it, shows it its side, or
    // turns its back on it.
    {
        let mut lost = [0.0f32; 3];
        for (k, yaw) in [std::f32::consts::PI, std::f32::consts::FRAC_PI_2, 0.0].into_iter().enumerate() {
            let (mut app, map) = world_with(&[]);
            let world = app.world_mut();
            let gun = spawn(world, &map, "crony", 0, v2(100.0, 129.0), 0.0);
            set_hp(world, gun, 1.0e6);
            spawn(world, &map, "radar_car", 0, v2(96.0, 135.0), 0.0);
            let target = spawn(world, &map, "overtaker", 1, v2(150.0, 129.0), yaw);
            set_hp(world, target, 1.0e6);
            for _ in 0..300 {
                world.run_schedule(SimTick);
            }
            lost[k] = 1.0e6 - hp(world, target);
        }
        let r = &defs.rules;
        println!("== Armor angle: a Crony shelling an Overtaker for 30 s does {:.0} head on, {:.0} to its side, {:.0} from behind (rules {}/{}/{})", lost[0], lost[1], lost[2], r.armor_front, r.armor_side, r.armor_rear);
        assert!(lost[0] > 0.0 && lost[0] < lost[1] && lost[1] < lost[2], "armor angle");
    }

    // 1b2. Splash: a Crony (its blast widened to 4 for the test) shelling an Overtaker with another
    // Overtaker and a Trooper 2.5 away on either side.
    {
        let (mut app, map) = world_with(&[]);
        let world = app.world_mut();
        let crony = defs.id("crony").unwrap();
        world.resource_mut::<Defs>().units[crony as usize].weapons[0].radius = 4.0;
        let gun = spawn(world, &map, "crony", 0, v2(100.0, 129.0), 0.0);
        set_hp(world, gun, 1.0e6);
        spawn(world, &map, "radar_car", 0, v2(96.0, 135.0), 0.0);
        let target = spawn(world, &map, "overtaker", 1, v2(150.0, 129.0), std::f32::consts::FRAC_PI_2);
        let tank = spawn(world, &map, "overtaker", 1, v2(150.0, 131.5), std::f32::consts::FRAC_PI_2);
        let man = spawn(world, &map, "trooper", 1, v2(150.0, 126.5), 0.0);
        for id in [target, tank, man] {
            set_hp(world, id, 1.0e6);
        }
        order(world, 0, vec![gun], Order::Attack(target));
        for _ in 0..300 {
            world.run_schedule(SimTick);
        }
        let [a, b, c] = [target, tank, man].map(|id| 1.0e6 - hp(world, id));
        let r = &defs.rules;
        println!("== Splash: a Crony (blast 4) shelling an Overtaker for 30 s does {a:.0} to it, {b:.0} to the Overtaker beside it, {c:.0} to the Trooper beside it (rules splash {} / armored {})", r.splash_direct, r.splash_armored);
        assert!(a > 0.0 && c > 0.0 && c < a * 0.4 && b < c, "splash is much less than a direct hit, and less again on armor");
    }

    // 1c. Line of fire: a Crony does not shell a neutral house, nor (with friendly fire) an own
    // tank, standing between it and the nearest enemy; it shoots another enemy in the clear.
    for (blocker, owner) in [("house", NEUTRAL), ("overtaker", 0)] {
        let (mut app, map) = world_with(&[]);
        let world = app.world_mut();
        world.resource_mut::<Defs>().rules.friendly_fire = owner == 0;
        let gun = spawn(world, &map, "crony", 0, v2(100.0, 129.0), 0.0);
        set_hp(world, gun, 1.0e6);
        spawn(world, &map, "radar_car", 0, v2(96.0, 135.0), 0.0);
        let wall = spawn(world, &map, blocker, owner, v2(125.0, 129.0), 0.0);
        set_hp(world, wall, 1.0e6);
        let hidden = spawn(world, &map, "radar_car", 1, v2(150.0, 129.0), 0.0);
        let open = spawn(world, &map, "radar_car", 1, v2(140.0, 170.0), 0.0);
        set_hp(world, hidden, 1.0e6);
        set_hp(world, open, 1.0e6);
        for _ in 0..300 {
            world.run_schedule(SimTick);
        }
        let (lost_wall, lost_hidden, lost_open) = (1.0e6 - hp(world, wall), 1.0e6 - hp(world, hidden), 1.0e6 - hp(world, open));
        println!(
            "== Line of fire: a Crony with {blocker} (owner {owner}) in front of the nearest enemy, 30 s: {blocker} lost {lost_wall:.0}, enemy behind it {lost_hidden:.0}, enemy in the clear {lost_open:.0}"
        );
        assert!(lost_wall < 1.0 && lost_open > 0.0, "line of fire past a {blocker}");
    }

    // 1d. Engineers before buildings: a Crony shelling a windmill switches to an engineer that
    // comes into reach further away, and leaves the windmill alone while the engineer is there.
    {
        let (mut app, map) = world_with(&[]);
        let world = app.world_mut();
        let gun = spawn(world, &map, "crony", 0, v2(100.0, 129.0), 0.0);
        set_hp(world, gun, 1.0e6);
        spawn(world, &map, "radar_car", 0, v2(96.0, 135.0), 0.0);
        // Shell holes would sink the engineer out of the line of fire.
        world.resource_mut::<Defs>().rules.craters = false;
        let mill = spawn(world, &map, "windmill", 1, v2(135.0, 129.0), 0.0);
        set_hp(world, mill, 1.0e6);
        for _ in 0..150 {
            world.run_schedule(SimTick);
        }
        let before = 1.0e6 - hp(world, mill);
        let eng = spawn(world, &map, "engineer", 1, v2(150.0, 150.0), 0.0);
        set_hp(world, eng, 1.0e6);
        // A shell already in the air may still land on the windmill.
        for _ in 0..40 {
            world.run_schedule(SimTick);
        }
        let mid = 1.0e6 - hp(world, mill);
        for _ in 0..300 {
            world.run_schedule(SimTick);
        }
        let (mill_lost, eng_lost) = (1.0e6 - hp(world, mill) - mid, 1.0e6 - hp(world, eng));
        println!("== Engineers first: a Crony did {before:.0} to a windmill in 15 s; with an engineer further off, 30 s: windmill {mill_lost:.0}, engineer {eng_lost:.0}");
        assert!(before > 0.0 && mill_lost < 1.0 && eng_lost > 0.0, "engineers are shot before buildings");
    }

    // 2. An idle trooper moves behind the nearest tree, away from the enemy it can see.
    {
        let tree = v2(128.0, 104.0);
        let (mut app, map) = world_with(&[("oak", tree)]);
        let world = app.world_mut();
        let t = spawn(world, &map, "trooper", 0, v2(130.0, 100.0), 0.0);
        let enemy = spawn(world, &map, "crony", 1, v2(166.0, 100.0), 3.14);
        set_hp(world, t, 1.0e6);
        set_hp(world, enemy, 1.0e6);
        for _ in 0..150 {
            world.run_schedule(SimTick);
        }
        let (p, _) = body(world, t).unwrap();
        let behind = (tree - v2(166.0, 100.0)).norm().dot((p - tree).norm());
        println!("== Infantry cover: trooper went from (130,100) to ({:.1},{:.1}), {:.1} from the oak, on its far side: {:.2} (1 = straight behind)", p.x, p.y, p.dist(tree), behind);
        assert!(p.dist(tree) < 3.0 && behind > 0.9, "infantry cover");
    }

    // 3b. Storming an occupied house: infantry fight the enemy infantry inside at the door and walk
    // in when it is cleared. Riflemen beat AT/SAM troopers inside; the defenders win an even fight.
    // `from`: how far west of the house the stormers start (6 = at the door, so only the
    // close-quarters fight counts; further out the defenders shoot at them on the way in).
    // `attack`: the stormers get an attack order on the house instead of a load order (they storm it all the same).
    // A field bunker or watch tower the enemy built is held and stormed the same way.
    for (inside, storm, n_in, n_out, from, want_taken, attack, building) in [
        ("at_trooper", "trooper", 1, 1, 6.0, Some(true), false, "house"),
        ("at_trooper", "trooper", 2, 2, 6.0, Some(true), false, "house"),
        ("sam_trooper", "trooper", 2, 2, 6.0, Some(true), false, "house"),
        ("trooper", "at_trooper", 1, 1, 6.0, Some(false), false, "house"),
        ("trooper", "trooper", 1, 1, 6.0, Some(false), false, "house"),
        ("trooper", "trooper", 1, 2, 6.0, Some(true), false, "house"),
        ("at_trooper", "trooper", 2, 3, 40.0, None, false, "house"),
        ("at_trooper", "trooper", 2, 2, 6.0, Some(true), true, "house"),
        ("at_trooper", "trooper", 2, 2, 6.0, Some(true), false, "field_bunker"),
        ("at_trooper", "trooper", 1, 2, 6.0, Some(true), true, "watch_tower"),
    ] {
        let (mut app, map) = world_with(&[]);
        let world = app.world_mut();
        let house = spawn(world, &map, building, if building == "house" { NEUTRAL } else { 1 }, v2(129.0, 129.0), 0.0);
        let held: Vec<u32> = (0..n_in).map(|k| spawn(world, &map, inside, 1, v2(140.0, 126.0 + k as f32 * 2.0), 3.14)).collect();
        order(world, 1, held.clone(), Order::Load(house));
        for _ in 0..80 {
            world.run_schedule(SimTick);
        }
        let in_before = held.iter().filter(|id| body(world, **id).map_or(false, |b| b.1)).count();
        let men: Vec<u32> = (0..n_out).map(|k| spawn(world, &map, storm, 0, v2(129.0 - from, 127.0 + k as f32 * 1.5), 0.0)).collect();
        order(world, 0, men.clone(), if attack { Order::Attack(house) } else { Order::Load(house) });
        let mut done_at = None;
        for t in 0..400 {
            world.run_schedule(SimTick);
            if done_at.is_none() && owner_of(world, house) == Some(0) {
                done_at = Some(t);
            }
        }
        let left_in = held.iter().filter(|id| body(world, **id).is_some()).count();
        let ours_in = men.iter().filter(|id| body(world, **id).map_or(false, |b| b.1)).count();
        let hps: Vec<f32> = men.iter().map(|id| hp(world, *id)).collect();
        println!(
            "== Storm from {from}{}: {n_out} {storm} vs {in_before}/{n_in} {inside} in a {building}: defenders left {left_in}, stormers inside {ours_in} (hp {hps:.0?}), house owner {:?} after {:?} s",
            if attack { " (attack order)" } else { "" },
            owner_of(world, house),
            done_at.map(|t| (t + 1) as f32 / 10.0)
        );
        assert!(in_before == n_in, "storm setup");
        match want_taken {
            Some(true) => assert!(left_in == 0 && ours_in > 0 && owner_of(world, house) == Some(0), "storm should take the house"),
            Some(false) => assert!(left_in > 0 && ours_in == 0 && owner_of(world, house) == Some(1), "storm should fail"),
            None => {}
        }
    }

    // 3b2. Infantry capture (lobby option, Cossacks style): troopers take an enemy building over
    // while no armed enemy stands near it; never a factory or an unarmed garrison (house: entered or
    // stormed), and nothing with the option off. Turrets are taken too, so is an empty bunker (a held
    // one is stormed first). No order is needed: idle troopers and
    // troopers on a plain move or attack-move take what they stand near (`how`: 0 Capture order, 1 idle,
    // 2 move past it, 3 attack-move past it).
    for (target, on, guard, how, want) in [
        ("radar_tower", false, false, 0, false),
        ("radar_tower", true, false, 0, true),
        ("land_factory", true, false, 0, false),
        ("radar_tower", true, true, 0, false),
        ("radar_tower", true, false, 1, true),
        ("radar_tower", false, false, 1, false),
        ("radar_tower", true, true, 1, false),
        ("radar_tower", true, false, 2, true),
        ("radar_tower", true, false, 3, true),
        ("land_factory", true, false, 1, false),
        ("house", true, false, 1, false),
        ("bunker", true, false, 1, true),
        ("bunker", true, false, 2, true),
        ("aa_turret", true, false, 0, true),
        ("gauss_turret", true, false, 1, true),
    ] {
        let (mut app, map) = world_with(&[]);
        let world = app.world_mut();
        world.resource_mut::<Game>().infantry_capture = on;
        let b = spawn(world, &map, target, 1, v2(129.0, 129.0), 0.0);
        if guard {
            spawn(world, &map, "overtaker", 1, v2(136.0, 129.0), 3.14);
        }
        let x0 = if how == 1 { 119.0 } else { 110.0 };
        let men: Vec<u32> = (0..3).map(|k| spawn(world, &map, "trooper", 0, v2(x0, 127.0 + k as f32 * 1.5), 0.0)).collect();
        match how {
            0 => order(world, 0, men.clone(), Order::Capture(b)),
            2 => order(world, 0, men.clone(), Order::Move(v2(160.0, 129.0))),
            3 => order(world, 0, men.clone(), Order::AttackMove(v2(160.0, 129.0))),
            _ => {}
        }
        let mut done_at = None;
        for t in 0..400 {
            world.run_schedule(SimTick);
            if done_at.is_none() && owner_of(world, b) == Some(0) {
                done_at = Some(t);
            }
        }
        let alive = men.iter().filter(|id| body(world, **id).is_some()).count();
        println!(
            "== Infantry capture ({}{}, {}): 3 troopers on a {target}: owner {:?} after {:?} s, {alive} troopers left",
            if on { "on" } else { "off" },
            if guard { ", guarded by an Overtaker" } else { "" },
            ["ordered", "idle", "moving", "attack-moving"][how],
            owner_of(world, b),
            done_at.map(|t| (t + 1) as f32 / 10.0)
        );
        assert!((owner_of(world, b) == Some(0)) == want, "infantry capture");
    }

    // 3c. Limited ammunition: an AT trooper with `ammo` missiles against five Pike mortars kills
    // that many at most, then falls silent while its side has no mass; given mass, it rearms one
    // round per `rearm` seconds and fires again (the round may only damage a tank, not kill it).
    if let Some(w) = defs.id("at_trooper").and_then(|d| defs.get(d).weapons.first()).filter(|w| w.ammo > 0) {
        let rounds = w.ammo;
        let (mut app, map) = world_with(&[]);
        let world = app.world_mut();
        let set_mass = |world: &mut World, m: f32| {
            let p = &mut world.resource_mut::<Game>().players[0];
            p.mass = m;
            p.max_mass = m.max(1.0);
        };
        // The commander's income would pay for rearming: paused, it earns nothing.
        let own: Vec<u32> = world.query::<&Unit>().iter(world).filter(|u| u.owner == 0).map(|u| u.id).collect();
        world.resource_mut::<CommandQueue>().0.push((0, Command::Pause { ids: own, on: true }));
        let at = spawn(world, &map, "at_trooper", 0, v2(100.0, 129.0), 0.0);
        set_hp(world, at, 1.0e6);
        let tanks: Vec<u32> = (0..5).map(|k| spawn(world, &map, "pike", 1, v2(132.0, 121.0 + k as f32 * 4.0), 3.14)).collect();
        for _ in 0..2000 {
            set_mass(world, 0.0);
            world.run_schedule(SimTick);
        }
        let killed = tanks.iter().filter(|id| body(world, **id).is_none()).count();
        let fired = world.query::<(&Unit, &Weapons)>().iter(world).find(|(u, _)| u.id == at).map_or(0, |(_, w)| w.list[0].fired);
        println!("== Ammunition: an AT trooper with {rounds} missiles vs 5 Pike mortars for 200 s, no mass: fired {fired}, killed {killed}");
        assert!(fired == rounds && killed <= rounds as usize, "limited ammunition");
        if w.rearm > 0.0 && killed < tanks.len() {
            let left_hp = |world: &mut World| -> f32 {
                let mut sum = 0.0;
                for id in &tanks {
                    if body(world, *id).is_some() {
                        sum += hp(world, *id);
                    }
                }
                sum
            };
            let hp_before = left_hp(world);
            let mut spent = 0.0;
            for _ in 0..((w.rearm * 10.0) as u32 + 300) {
                set_mass(world, 100.0);
                world.run_schedule(SimTick);
                let p = &world.resource::<Game>().players[0];
                spent += 100.0 + p.income_m * 0.1 - p.mass;
            }
            let killed2 = tanks.iter().filter(|id| body(world, **id).is_none()).count();
            let hp_after = left_hp(world);
            println!(
                "== Rearm: {:.0} s with mass: {spent:.0} mass spent ({:.0} per round), {} more killed, hp of the rest {hp_before:.0} -> {hp_after:.0}",
                w.rearm + 30.0,
                w.rearm_mass,
                killed2 - killed
            );
            assert!(spent >= w.rearm_mass * 0.99 && spent <= w.rearm_mass * 2.0 + 1.0 && (killed2 > killed || hp_after < hp_before - 1.0), "rearming");
        }
    }

    // 3. Troopers occupy a neutral house, fire from it, leave it, and are thrown out when it falls.
    {
        let (mut app, map) = world_with(&[]);
        let world = app.world_mut();
        let house = spawn(world, &map, "house", NEUTRAL, v2(129.0, 129.0), 0.0);
        let cap = defs.get(defs.id("house").unwrap()).transport.as_ref().map_or(0, |t| t.capacity) as usize;
        let men: Vec<u32> = (0..cap).map(|k| spawn(world, &map, "trooper", 0, v2(110.0 + k as f32 * 2.0, 120.0), 0.0)).collect();
        order(world, 0, men.clone(), Order::Load(house));
        for _ in 0..200 {
            world.run_schedule(SimTick);
        }
        let inside = men.iter().filter(|id| body(world, **id).map_or(false, |b| b.1)).count();
        let held = owner_of(world, house);
        // An enemy tank drives up: the infantry inside shoot it, the house takes the return fire.
        let tank = spawn(world, &map, "overtaker", 1, v2(140.0, 129.0), 3.14);
        set_hp(world, tank, 1.0e6);
        set_hp(world, house, 1.0e6);
        let house_hp = hp(world, house);
        for _ in 0..200 {
            world.run_schedule(SimTick);
        }
        let dealt = 1.0e6 - hp(world, tank);
        let men_hp: f32 = men.iter().map(|id| hp(world, *id)).sum();
        let house_lost = house_hp - hp(world, house);
        world.query::<(&Unit, &mut Health)>().iter_mut(world).filter(|(u, _)| u.id == tank).for_each(|(_, mut h)| h.0 = -1.0);
        set_hp(world, house, 1.0e6);
        // Out again, to a spot west of the house.
        order(world, 0, vec![house], Order::Unload { pos: v2(110.0, 129.0), radius: 4.0, occupy: false });
        for _ in 0..150 {
            world.run_schedule(SimTick);
        }
        let out = men.iter().filter(|id| body(world, **id).map_or(false, |b| !b.1 && b.0.dist(v2(110.0, 129.0)) < 7.0)).count();
        let freed = owner_of(world, house);
        // In again, then the house is destroyed.
        order(world, 0, men.clone(), Order::Load(house));
        for _ in 0..200 {
            world.run_schedule(SimTick);
        }
        let again = men.iter().filter(|id| body(world, **id).map_or(false, |b| b.1)).count();
        set_hp(world, house, -1.0);
        for _ in 0..20 {
            world.run_schedule(SimTick);
        }
        let out_now: Vec<u32> = men.iter().copied().filter(|id| body(world, *id).map_or(false, |b| !b.1)).collect();
        let thrown: Vec<f32> = out_now.iter().map(|id| hp(world, *id)).collect();
        println!("== Garrison: {inside} of {cap} troopers in the house (owner now {held:?}); in 20 s they did {dealt:.0} damage to a tank, the house lost {house_lost:.0} hp, troopers' hp {men_hp:.0}");
        println!("   unloaded {out} at the spot (house owner then {freed:?}); in again {again}; house destroyed: {} thrown out with hp {:?}", thrown.len(), thrown);
        let full = defs.get(defs.id("trooper").unwrap()).hp;
        // The men inside take a small share of the hits on the house (rules.garrison_damage).
        let all = full * cap as f32;
        assert!(inside == cap && held == Some(0) && dealt > 0.0 && house_lost > 0.0 && men_hp < all - 0.01 && men_hp > all * 0.5, "garrison");
        assert!(out == cap && freed == Some(NEUTRAL) && again == cap && thrown.len() == cap && thrown.iter().all(|h| *h > 0.0 && *h < full * 0.7), "garrison exit");
    }

    // 3c. AT Troopers (weapon `hold`) fire from inside a building: the set-up hold must not be blocked
    // by the speed and goal they had when they walked in.
    {
        let (mut app, map) = world_with(&[]);
        let world = app.world_mut();
        let house = spawn(world, &map, "house", NEUTRAL, v2(129.0, 129.0), 0.0);
        let men: Vec<u32> = (0..2).map(|k| spawn(world, &map, "at_trooper", 0, v2(110.0, 124.0 + k as f32 * 3.0), 0.0)).collect();
        order(world, 0, men.clone(), Order::Load(house));
        for _ in 0..200 {
            world.run_schedule(SimTick);
        }
        let inside = men.iter().filter(|id| body(world, **id).map_or(false, |b| b.1)).count();
        let tank = spawn(world, &map, "overtaker", 1, v2(150.0, 129.0), 3.14);
        set_hp(world, tank, 1.0e6);
        for _ in 0..300 {
            world.run_schedule(SimTick);
        }
        let dealt = 1.0e6 - hp(world, tank);
        println!("== AT from a building: {inside} of 2 AT troopers inside, {dealt:.0} damage to a tank in 30 s");
        assert!(inside == 2 && dealt > 0.0, "AT trooper fires from a building");
    }

    // 4. A mortar (weapon `hold`) stays put after firing and does not fire on the move.
    {
        let (mut app, map) = world_with(&[]);
        let world = app.world_mut();
        let pike = spawn(world, &map, "pike", 0, v2(100.0, 100.0), 0.0);
        spawn(world, &map, "radar_car", 0, v2(96.0, 106.0), 0.0);
        let target = spawn(world, &map, "overtaker", 1, v2(160.0, 100.0), 3.14);
        set_hp(world, target, 1.0e6);
        let hold = defs.get(defs.id("pike").unwrap()).weapons[0].hold;
        let state = |world: &mut World| -> (V2, f32) { world.query::<(&Unit, &Body, &Mover)>().iter(world).find(|(u, ..)| u.id == pike).map(|(_, b, m)| (b.pos, m.hold)).unwrap() };
        let mut fired = None;
        for t in 0..300 {
            world.run_schedule(SimTick);
            if state(world).1 > 0.0 {
                fired = Some(t);
                break;
            }
        }
        // Ordered away right after the first shot; the target stays in range the whole drive.
        let dest = v2(100.0, 160.0);
        order(world, 0, vec![pike], Order::Move(dest));
        let (start, _) = state(world);
        let (mut waited, mut on_move, mut arrived, mut prev) = (None, 0, None, state(world).1);
        for t in 0..600 {
            world.run_schedule(SimTick);
            let (pos, h) = state(world);
            if waited.is_none() && pos.dist(start) > 0.05 {
                waited = Some(t);
            }
            if h > prev && arrived.is_none() {
                on_move += 1;
            }
            prev = h;
            if arrived.is_none() && pos.dist(dest) < 1.0 {
                arrived = Some(t);
            }
            if arrived.map_or(false, |a| t > a + 100) {
                break;
            }
        }
        let after = prev > 0.0 || {
            let mut any = false;
            for _ in 0..50 {
                world.run_schedule(SimTick);
                any |= state(world).1 > 0.0;
            }
            any
        };
        println!("== Mortar hold ({hold}s): first shot after {fired:?} ticks, ordered away then, started moving after {waited:?} ticks; shots while driving {on_move}; arrived after {arrived:?}, firing again there: {after}");
        let w = waited.unwrap_or(0) as f32 / 10.0;
        assert!(fired.is_some() && w >= hold - 0.25 && w <= hold + 1.0 && on_move == 0 && arrived.is_some() && after, "mortar hold");
    }
}

/// Field works: sandbags, trenches, the field bunker and the watch tower
/// (`--combattest`, or alone with `LR_TEST=fieldworks`).
fn fieldwork_tests(defs: &Defs) {
    let faction = defs.playable_factions()[0] as u8;
    let new_world = || -> (App, MapData) {
        let mut app = headless(defs.clone());
        let mut map = MapData::generate(128, 4242, 2);
        map.heights.iter_mut().for_each(|h| *h = 2.0);
        map.feature_kinds = vec![];
        map.features = vec![];
        map.rebuild_blocked();
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
        setup_game(app.world_mut(), map.clone(), &setup);
        for p in app.world_mut().resource_mut::<Game>().players.iter_mut() {
            p.energy = 100000.0;
            p.max_energy = 100000.0;
            p.mass = 10000.0;
            p.max_mass = 10000.0;
        }
        (app, map)
    };
    let spawn = |world: &mut World, map: &MapData, name: &str, owner: u8, pos: V2, yaw: f32| -> u32 {
        let mut game = world.resource::<Game>().clone();
        let id = {
            let mut commands = world.commands();
            spawn_unit(&mut commands, defs, &mut game, map, defs.id(name).expect("unit"), owner, pos, yaw, true, 0)
        };
        world.insert_resource(game);
        world.flush();
        world.resource_mut::<Nav>().dirty = true;
        id
    };
    let hp = |world: &mut World, id: u32| -> f32 { world.query::<(&Unit, &Health)>().iter(world).find(|(u, _)| u.id == id).map_or(0.0, |(_, h)| h.0) };
    let set_hp = |world: &mut World, id: u32, v: f32| {
        if let Some((_, mut h)) = world.query::<(&Unit, &mut Health)>().iter_mut(world).find(|(u, _)| u.id == id) {
            h.0 = v;
        }
    };
    let body = |world: &mut World, id: u32| -> Option<(V2, f32, bool)> { world.query::<(&Unit, &Body, Has<Hosted>)>().iter(world).find(|(u, ..)| u.id == id).map(|(_, b, h)| (b.pos, b.y, h)) };
    let run = |world: &mut World, n: u32| (0..n).for_each(|_| world.run_schedule(SimTick));
    let id_of = |name: &str| defs.id(name).expect("unit");

    // 1. Only the combat engineers (Mk2, Mk3) build them.
    let works = ["sandbags", "trench", "field_bunker", "watch_tower"];
    let builders: Vec<&str> = defs.units.iter().filter(|u| works.iter().all(|w| u.builds.contains(&id_of(w)))).map(|u| u.id.as_str()).collect();
    let some: Vec<&str> = defs.units.iter().filter(|u| works.iter().any(|w| u.builds.contains(&id_of(w)))).map(|u| u.id.as_str()).collect();
    println!("== Field works: built by {builders:?}");
    assert!(builders == ["engineer2", "engineer3"] && some == builders, "who builds field works");

    // 2. An Engineer Mk2 builds a line of trench; the ground goes down, stays passable, and stays down
    // when the trench is destroyed.
    {
        let (mut app, map) = new_world();
        let world = app.world_mut();
        let sparky = spawn(world, &map, "engineer2", 0, v2(120.0, 101.0), 0.0);
        let spots: Vec<V2> = (0..4).map(|k| v2(127.0 + k as f32 * 2.0, 101.0)).collect();
        for (k, p) in spots.iter().enumerate() {
            world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: vec![sparky], orders: vec![Order::Build { def: id_of("trench"), pos: *p }], queue: k > 0 }));
        }
        run(world, 600);
        let trenches: Vec<(u32, V2, f32, bool)> = world.query::<(&Unit, &Body, Has<UnderConstruction>)>().iter(world).filter(|(u, ..)| u.def == id_of("trench")).map(|(u, b, uc)| (u.id, b.pos, b.y, !uc)).collect();
        let floor = world.resource::<MapData>().h(spots[1]);
        let free = spots.iter().all(|p| passable(world.resource::<Nav>().at(world.resource::<MapData>(), *p)));
        println!("== Trench: {} of 4 pieces built, floor {:.2} below the ground, pieces stand at {:.2}, cells passable: {free}", trenches.iter().filter(|t| t.3).count(), 2.0 - floor, 2.0 - trenches.first().map_or(0.0, |t| t.2));
        assert!(trenches.len() == 4 && trenches.iter().all(|t| t.3) && (2.0 - floor - 0.8).abs() < 0.01 && free, "trench line");
        assert!(trenches.iter().all(|t| (t.2 - floor).abs() < 0.01), "trench pieces follow the ground");
        assert!(world.query::<&Unit>().iter(world).filter(|u| u.def == id_of("trench")).all(|u| u.owner == NEUTRAL), "finished field works are neutral");
        // A second work cannot go on top of one.
        assert!(!can_place(defs, world.resource::<MapData>(), &world.resource::<Nav>().occ, id_of("sandbags"), spots[0]), "works do not stack");
        for t in &trenches {
            set_hp(world, t.0, -1.0);
        }
        run(world, 5);
        let left = world.query::<&Unit>().iter(world).filter(|u| u.def == id_of("trench")).count();
        let after = world.resource::<MapData>().h(spots[1]);
        let wrecks = world.query::<&Wreck>().iter(world).count();
        println!("== Trench destroyed: {left} pieces left, {wrecks} wrecks, the ground is still {:.2} lower", 2.0 - after);
        assert!(left == 0 && wrecks == 0 && (after - floor).abs() < 0.01, "a destroyed trench leaves its ditch");
    }

    // 3. Cover: a trooper under fire from three enemy troopers for 30 s, in the open, in a trench
    // and behind sandbags. The work takes what the trooper does not, and wears down.
    let mut lost = [0.0f32; 3];
    let mut work_left = [1.0f32; 3];
    let mut fired = [0.0f32; 3];
    for (k, work) in [None, Some("trench"), Some("sandbags")].into_iter().enumerate() {
        let (mut app, map) = new_world();
        let world = app.world_mut();
        let at = v2(131.0, 101.0);
        // Sandbags stand between the trooper and the enemy to the east. They are the enemy's own:
        // sandbags shelter every side.
        let w = work.map(|n| if n == "sandbags" { spawn(world, &map, n, 1, at + v2(2.0, 0.0), 0.0) } else { spawn(world, &map, n, 0, at, 0.0) });
        if let Some(w) = w {
            set_hp(world, w, 1.0e5);
        }
        let t = spawn(world, &map, "trooper", 0, at, 0.0);
        set_hp(world, t, 1.0e6);
        let foes: Vec<u32> = (0..3).map(|i| spawn(world, &map, "trooper", 1, v2(142.0, 97.0 + i as f32 * 4.0), 3.14)).collect();
        for f in &foes {
            set_hp(world, *f, 1.0e6);
        }
        run(world, 300);
        lost[k] = 1.0e6 - hp(world, t);
        fired[k] = foes.iter().map(|f| 1.0e6 - hp(world, *f)).sum();
        if let Some(w) = w {
            work_left[k] = 1.0e5 - hp(world, w);
        }
        let (p, ..) = body(world, t).unwrap();
        assert!(p.dist(at) < 1.0, "the trooper stays in its cover ({:.1},{:.1})", p.x, p.y);
    }
    println!("== Field cover: a trooper shot at for 30 s loses {:.0} hp in the open, {:.0} in a trench (the trench {:.0}), {:.0} behind sandbags (the sandbags {:.0}); it deals {:.0} / {:.0} / {:.0}", lost[0], lost[1], work_left[1], lost[2], work_left[2], fired[0], fired[1], fired[2]);
    assert!(lost[0] > 0.0 && lost[1] > 0.0 && lost[1] < lost[0] * 0.6 && work_left[1] > 0.0, "trench cover");
    assert!(lost[2] > 0.0 && lost[2] < lost[0] * 0.7 && work_left[2] > 0.0, "sandbag cover");
    assert!(fired[1] > fired[0] * 0.7 && fired[2] > fired[0] * 0.7, "troopers fire out of their cover");

    // 3b. Fighting for a trench: 4 troopers hold a line of 4 trench pieces (or the same spot in the
    // open), riflemen attack-move onto them from 50 away. Prints who is left after 60 s.
    // `trench_fight(n, dug)` -> (defenders left, attackers left, attackers standing in the trench).
    let trench_fight = |n: usize, dug: bool, storm: bool| -> (usize, usize, usize) {
        let (mut app, map) = new_world();
        let world = app.world_mut();
        let line: Vec<V2> = (0..4).map(|k| v2(131.0, 97.0 + k as f32 * 2.0)).collect();
        if dug {
            for p in &line {
                spawn(world, &map, "trench", 1, *p, 0.0);
            }
        }
        let held: Vec<u32> = line.iter().map(|p| spawn(world, &map, "trooper", 1, *p, 3.14)).collect();
        let men: Vec<u32> = (0..n).map(|k| spawn(world, &map, "trooper", 0, v2(81.0, 92.0 + k as f32 * 2.5), 0.0)).collect();
        // Storming: every attacker gets an attack order on one of the defenders, in turn (in the
        // open that is just an attack order).
        let orders: Vec<Order> = if storm { (0..n).map(|k| Order::Attack(held[k % held.len()])).collect() } else { vec![Order::AttackMove(v2(150.0, 101.0))] };
        world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: men.clone(), orders, queue: false }));
        run(world, 600);
        let alive = |world: &mut World, ids: &[u32]| ids.iter().filter(|id| body(world, **id).is_some()).count();
        let (d, a) = (alive(world, &held), alive(world, &men));
        let inside = men.iter().filter_map(|id| body(world, *id)).filter(|b| line.iter().any(|p| (b.0.x - p.x).abs() <= 1.0 && (b.0.y - p.y).abs() <= 1.0)).count();
        (d, a, inside)
    };
    for n in [4, 6, 8] {
        let open = trench_fight(n, false, false);
        let dug = trench_fight(n, true, false);
        let stormed = trench_fight(n, true, true);
        println!(
            "== Trench fight: {n} riflemen attack 4 riflemen, defenders/attackers left: in the open {}/{}; in a trench on attack-move {}/{}, storming it {}/{} ({} attackers then hold the trench)",
            open.0, open.1, dug.0, dug.1, stormed.0, stormed.1, stormed.2
        );
        if n == 6 {
            assert!(open.0 == 0 && dug.0 > 0, "a trench holds against 6 on attack-move");
            assert!(stormed.0 == 0 && stormed.2 > 0, "6 riflemen storm a trench 4 hold and take it");
        }
    }

    // 4. Idle infantry walk into a trench near by, one per piece, enemy or not.
    {
        let (mut app, map) = new_world();
        let world = app.world_mut();
        let pieces = [v2(131.0, 101.0), v2(133.0, 101.0)];
        for p in pieces {
            spawn(world, &map, "trench", 0, p, 0.0);
        }
        let men: Vec<u32> = (0..2).map(|i| spawn(world, &map, "trooper", 0, v2(127.0, 99.0 + i as f32 * 3.0), 0.0)).collect();
        run(world, 150);
        let at: Vec<V2> = men.iter().map(|m| body(world, *m).unwrap().0).collect();
        let filled = pieces.iter().filter(|p| at.iter().any(|a| a.dist(**p) < 0.6)).count();
        println!("== Trench: 2 idle troopers near 2 pieces stand in {filled} of them");
        assert!(filled == 2, "infantry man the trench");
    }

    // 4b. An idle trooper with enemies in sight walks behind a sandbag piece near by (anybody's).
    {
        let (mut app, map) = new_world();
        let world = app.world_mut();
        let bags = v2(131.0, 101.0);
        spawn(world, &map, "sandbags", 1, bags, 0.0);
        let t = spawn(world, &map, "trooper", 0, v2(125.0, 103.0), 0.0);
        set_hp(world, t, 1.0e6);
        let foes: Vec<u32> = (0..2).map(|i| spawn(world, &map, "trooper", 1, v2(145.0, 99.0 + i as f32 * 4.0), 3.14)).collect();
        for f in &foes {
            set_hp(world, *f, 1.0e6);
        }
        run(world, 150);
        let p = body(world, t).unwrap().0;
        println!("== Sandbag cover: an idle trooper under fire moved to ({:.1}, {:.1}), the sandbags stand at ({:.0}, {:.0})", p.x, p.y, bags.x, bags.y);
        assert!(p.x < bags.x - 1.0 && p.dist(bags) < 2.5, "infantry hide behind sandbags");
    }

    // 5. Bombed out: a mortar shelling sandbags and a trench destroys both.
    {
        let (mut app, map) = new_world();
        let world = app.world_mut();
        let spot = v2(171.0, 101.0);
        spawn(world, &map, "sandbags", 1, spot, 0.0);
        spawn(world, &map, "trench", 1, spot + v2(2.0, 0.0), 0.0);
        let pike = spawn(world, &map, "pike", 0, v2(111.0, 101.0), 0.0);
        world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: vec![pike], orders: vec![Order::AttackGround { pos: spot + v2(1.0, 0.0), radius: 0.0 }], queue: false }));
        let mut gone_at = None;
        for t in 0..3000 {
            world.run_schedule(SimTick);
            if world.query::<&Unit>().iter(world).filter(|u| u.owner == 1 && defs.get(u.def).is_fieldwork()).count() == 0 {
                gone_at = Some(t);
                break;
            }
        }
        let h = world.resource::<MapData>().h(spot + v2(2.0, 0.0));
        println!("== Bombed out: a Pike Mortar destroys a sandbag piece and a trench piece in {:?} s; the ditch is {:.2} deep", gone_at.map(|t| t / 10), 2.0 - h);
        assert!(gone_at.is_some() && h < 2.0 - 0.75, "bombing out field works");
    }

    // 6. Bunker and watch tower take infantry; from the tower they see further.
    {
        let (mut app, map) = new_world();
        let world = app.world_mut();
        let bunker = spawn(world, &map, "field_bunker", 0, v2(130.0, 130.0), 0.0);
        let tower = spawn(world, &map, "watch_tower", 0, v2(131.0, 141.0), 0.0);
        let a: Vec<u32> = (0..5).map(|i| spawn(world, &map, "trooper", 0, v2(124.0, 126.0 + i as f32 * 1.5), 0.0)).collect();
        let b: Vec<u32> = (0..3).map(|i| spawn(world, &map, "trooper", 0, v2(125.0, 139.0 + i as f32 * 1.5), 0.0)).collect();
        world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: a.clone(), orders: vec![Order::Load(bunker)], queue: false }));
        world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: b.clone(), orders: vec![Order::Load(tower)], queue: false }));
        run(world, 150);
        let inside = |world: &mut World, ids: &[u32]| ids.iter().filter(|id| body(world, **id).map_or(false, |x| x.2)).count();
        let (n_bunker, n_tower) = (inside(world, &a), inside(world, &b));
        // An enemy tank beyond a trooper's own sight, inside the tower's.
        let vision = defs.get(id_of("trooper")).vision;
        let gain = defs.get(id_of("watch_tower")).roof_vision(&defs.rules);
        let far = spawn(world, &map, "overtaker", 1, v2(131.0 + vision * (1.0 + gain) * 0.5, 141.0), 3.14);
        run(world, 20);
        let seen = world.query::<(&Unit, &Intel)>().iter(world).find(|(u, _)| u.id == far).map_or(false, |(_, i)| i.seen & 1 != 0);
        println!("== Garrisons: {n_bunker} of 5 troopers in the field bunker, {n_tower} of 3 in the watch tower; a tank at {:.0} (trooper vision {vision:.0}, x{gain} from the tower) is seen: {seen}", vision * (1.0 + gain) * 0.5);
        assert!(n_bunker == 4 && n_tower == 2 && seen, "bunker and watch tower");
    }

    // 7. Like any building of the map, an empty field bunker the enemy built is nobody's: our
    // troopers walk in and it is ours while they hold it.
    {
        let owner_of = |world: &mut World, id: u32| -> Option<u8> { world.query::<&Unit>().iter(world).find(|u| u.id == id).map(|u| u.owner) };
        let (mut app, map) = new_world();
        let world = app.world_mut();
        let bunker = spawn(world, &map, "field_bunker", 1, v2(130.0, 130.0), 0.0);
        run(world, 5);
        let empty_owner = owner_of(world, bunker);
        let men: Vec<u32> = (0..2).map(|i| spawn(world, &map, "trooper", 0, v2(122.0, 128.0 + i as f32 * 1.5), 0.0)).collect();
        world.resource_mut::<CommandQueue>().0.push((0, Command::Orders { ids: men.clone(), orders: vec![Order::Load(bunker)], queue: false }));
        run(world, 150);
        let inside = men.iter().filter(|id| body(world, **id).map_or(false, |x| x.2)).count();
        println!("== Abandoned field bunker: enemy-built and empty, owner {empty_owner:?}; {inside} of 2 troopers walked in, owner {:?}", owner_of(world, bunker));
        assert!(empty_owner == Some(NEUTRAL) && inside == 2 && owner_of(world, bunker) == Some(0), "abandoned field bunker");
    }
}

/// Binoculars, spotting delay, buildings blocking sight (`--combattest`, or alone with `LR_TEST=binoculars`).
fn binocular_tests(defs: &Defs) {
    if !defs.id("trooper").map_or(false, |t| defs.get(t).binoculars.is_some()) {
        println!("== Binoculars: the trooper has none in this data, skipped");
        return;
    }
    let faction = defs.playable_factions()[0] as u8;
    let flat = || -> (App, MapData) {
        let mut app = headless(defs.clone());
        let mut map = MapData::generate(128, 4242, 2);
        map.heights.iter_mut().for_each(|h| *h = 2.0);
        map.feature_kinds = vec![];
        map.features = vec![];
        map.rebuild_blocked();
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
        setup_game(app.world_mut(), map.clone(), &setup);
        // The commanders wait in the far corners, out of binocular range of the test.
        let world = app.world_mut();
        let mut q = world.query::<(&Unit, &mut Body)>();
        for (_, mut b) in q.iter_mut(world) {
            b.pos = v2(if b.pos.x < 128.0 { 5.0 } else { 250.0 }, 250.0);
        }
        (app, map)
    };
    let spawn = |world: &mut World, map: &MapData, name: &str, owner: u8, pos: V2, yaw: f32| -> u32 {
        let mut game = world.resource::<Game>().clone();
        let id = {
            let mut commands = world.commands();
            spawn_unit(&mut commands, defs, &mut game, map, defs.id(name).expect("unit"), owner, pos, yaw, true, 0)
        };
        world.insert_resource(game);
        world.flush();
        world.resource_mut::<Nav>().dirty = true;
        id
    };
    let order = |world: &mut World, owner: u8, ids: Vec<u32>, o: Order| world.resource_mut::<CommandQueue>().0.push((owner, Command::Orders { ids, orders: vec![o], queue: false }));
    let set_hp = |world: &mut World, id: u32, v: f32| {
        if let Some((_, mut h)) = world.query::<(&Unit, &mut Health)>().iter_mut(world).find(|(u, _)| u.id == id) {
            h.0 = v;
        }
    };
    let body = |world: &mut World, id: u32| -> Option<(V2, bool)> { world.query::<(&Unit, &Body, Has<Hosted>)>().iter(world).find(|(u, ..)| u.id == id).map(|(_, b, h)| (b.pos, h)) };
    // A trooper does not see an enemy behind it beyond its vision, scans its front
    // and finds one there, locks on and keeps it in sight while it drives round to its side.
    {
        let (mut app, map) = flat();
        let world = app.world_mut();
        let intel = |world: &mut World, id: u32| -> Intel { world.query::<(&Unit, &Intel)>().iter(world).find(|(u, _)| u.id == id).map_or(Intel::default(), |(_, i)| *i) };
        let t = spawn(world, &map, "trooper", 0, v2(120.0, 150.0), 0.0);
        let behind = spawn(world, &map, "overtaker", 1, v2(20.0, 150.0), 0.0);
        set_hp(world, t, 1.0e6);
        set_hp(world, behind, 1.0e6);
        let mut behind_seen = 0;
        for _ in 0..150 {
            world.run_schedule(SimTick);
            behind_seen += (intel(world, behind).seen & 1 != 0) as u32;
        }
        let ahead = spawn(world, &map, "overtaker", 1, v2(230.0, 150.0), 3.14);
        set_hp(world, ahead, 1.0e6);
        let mut found = None;
        for k in 0..200 {
            world.run_schedule(SimTick);
            if found.is_none() && intel(world, ahead).seen & 1 != 0 {
                found = Some(k);
            }
        }
        let locked = intel(world, t).lock == ahead;
        // It drives to the trooper's right, 90 degrees off where the trooper faces (outside the scan).
        order(world, 1, vec![ahead], Order::Move(v2(120.0, 40.0)));
        let (mut kept, mut ticks) = (0, 0);
        for _ in 0..500 {
            world.run_schedule(SimTick);
            ticks += 1;
            kept += (intel(world, ahead).seen & 1 != 0) as u32;
        }
        let (end, _) = body(world, ahead).unwrap();
        let i = intel(world, t);
        println!("== Binoculars: enemy behind seen {behind_seen} of 150 ticks; enemy 110 ahead found after {found:?} ticks, locked {locked}; driving round to ({:.0},{:.0}) it stayed in sight {kept} of {ticks} ticks, binoculars at {:.0} deg, still locked {}", end.x, end.y, i.look.to_degrees(), i.lock == ahead);
        assert!(behind_seen == 0 && found.is_some() && locked && i.lock == ahead && intel(world, ahead).seen & 1 != 0 && kept > ticks * 9 / 10, "binoculars");
    }
    // Spotting delay: a tank in view is seen at once, walking infantry quickly, infantry that
    // keeps still only after its `spot_time`; out of view the progress is lost again.
    {
        let (mut app, map) = flat();
        let world = app.world_mut();
        let seen = |world: &mut World, id: u32| -> bool { world.query::<(&Unit, &Intel)>().iter(world).find(|(u, _)| u.id == id).map_or(false, |(_, i)| i.seen & 1 != 0) };
        let td = defs.get(defs.id("trooper").unwrap());
        let watcher = spawn(world, &map, "overtaker", 0, v2(120.0, 150.0), 0.0);
        let tank = spawn(world, &map, "overtaker", 1, v2(150.0, 140.0), 3.14);
        let still = spawn(world, &map, "trooper", 1, v2(150.0, 150.0), 3.14);
        let walker = spawn(world, &map, "trooper", 1, v2(150.0, 160.0), 3.14);
        for id in [watcher, tank, still, walker] {
            set_hp(world, id, 1.0e6);
        }
        order(world, 1, vec![walker], Order::Move(v2(150.0, 175.0)));
        let mut first = [None; 3];
        for k in 1..=200 {
            world.run_schedule(SimTick);
            for (n, id) in [tank, still, walker].into_iter().enumerate() {
                if first[n].is_none() && seen(world, id) {
                    first[n] = Some(k);
                }
            }
        }
        println!("== Spotting (trooper: {} s hiding, {} s moving): at 30 from an Overtaker the tank is seen after {:?} ticks, the walking trooper after {:?}, the one keeping still after {:?}", td.spot_time, td.spot_time_moving, first[0], first[2], first[1]);
        if td.spot_time > td.spot_time_moving {
            let (t, s, w) = (first[0].unwrap_or(999), first[1].unwrap_or(999), first[2].unwrap_or(999));
            assert!(t <= 2 && w > t && s > w && s as f32 <= td.spot_time * 10.0 + 3.0, "spotting delay");
        }
    }
    // Buildings block sight: a tank hides behind a residential tower (which itself stays in view),
    // and a trooper that cannot see past it from the ground does from its roof once inside.
    if defs.id("residential_tower").is_some() && defs.rules.sight_blocked_by_buildings {
        let seen = |world: &mut World, id: u32| -> bool { world.query::<(&Unit, &Intel)>().iter(world).find(|(u, _)| u.id == id).map_or(false, |(_, i)| i.seen & 1 != 0) };
        let (hidden, open, tower_seen) = {
            let (mut app, map) = flat();
            let world = app.world_mut();
            let watcher = spawn(world, &map, "overtaker", 0, v2(110.0, 150.0), 0.0);
            let tower = spawn(world, &map, "residential_tower", NEUTRAL, v2(126.0, 150.0), 0.0);
            let behind = spawn(world, &map, "overtaker", 1, v2(138.0, 150.0), 3.14);
            let beside = spawn(world, &map, "overtaker", 1, v2(130.0, 170.0), 3.14);
            for id in [watcher, tower, behind, beside] {
                set_hp(world, id, 1.0e6);
            }
            let mut n = [0; 3];
            for _ in 0..30 {
                world.run_schedule(SimTick);
                for (k, id) in [behind, beside, tower].into_iter().enumerate() {
                    n[k] += seen(world, id) as u32;
                }
            }
            (n[0], n[1], n[2])
        };
        let (ground, roof, inside) = {
            let (mut app, map) = flat();
            let world = app.world_mut();
            let t = spawn(world, &map, "trooper", 0, v2(106.0, 150.0), 0.0);
            let tower = spawn(world, &map, "residential_tower", NEUTRAL, v2(126.0, 150.0), 0.0);
            let tank = spawn(world, &map, "overtaker", 1, v2(146.0, 150.0), 3.14);
            for id in [t, tower, tank] {
                set_hp(world, id, 1.0e6);
            }
            let mut ground = 0;
            for _ in 0..30 {
                world.run_schedule(SimTick);
                ground += seen(world, tank) as u32;
            }
            order(world, 0, vec![t], Order::Load(tower));
            for _ in 0..250 {
                world.run_schedule(SimTick);
                if body(world, t).map_or(false, |b| b.1) {
                    break;
                }
            }
            let inside = body(world, t).map_or(false, |b| b.1);
            let mut roof = 0;
            for _ in 0..30 {
                world.run_schedule(SimTick);
                roof += seen(world, tank) as u32;
            }
            (ground, roof, inside)
        };
        println!("== Buildings block sight: a tank behind a residential tower seen {hidden} of 30 ticks, one beside it {open}, the tower {tower_seen}; a trooper sees a tank past the tower {ground} of 30 ticks from the ground, {roof} of 30 from its roof (inside: {inside})");
        assert!(hidden == 0 && open > 25 && tower_seen > 25 && ground == 0 && inside && roof > 25, "buildings block sight");
    }
}

/// Bombing runs, fighter-bombers and the SAM site (`--combattest`, or alone with `LR_TEST=air`).
fn air_tests(defs: &Defs) {
    use std::collections::HashMap;
    let faction = defs.playable_factions()[0] as u8;
    struct Side<'a> {
        name: &'a str,
        owner: u8,
        n: usize,
        at: V2,
        yaw: f32,
    }
    let run = |title: &str, sides: &[Side], orders: &dyn Fn(&HashMapIds) -> Vec<(u8, Vec<u32>, Order)>, ticks: u32| {
        if sides.iter().any(|s| defs.id(s.name).is_none()) {
            println!("== {title}: skipped (unit missing)");
            return;
        }
        let mut app = headless(defs.clone());
        let mut map = MapData::generate(192, 4242, 2);
        map.heights.iter_mut().for_each(|h| *h = 2.0);
        map.rebuild_blocked();
        let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
        let world = app.world_mut();
        setup_game(world, map.clone(), &setup);
        for p in world.resource_mut::<Game>().players.iter_mut() {
            p.energy = 100000.0;
            p.max_energy = 100000.0;
        }
        let mut ids: HashMapIds = Default::default();
        for s in sides {
            for k in 0..s.n {
                let side = V2::from_angle(s.yaw).perp();
                let pos = s.at + side * ((k as f32 - (s.n as f32 - 1.0) * 0.5) * 4.0);
                let mut game = world.resource::<Game>().clone();
                let id = {
                    let mut commands = world.commands();
                    spawn_unit(&mut commands, defs, &mut game, &map, defs.id(s.name).unwrap(), s.owner, pos, s.yaw, true, 0)
                };
                world.insert_resource(game);
                ids.entry(s.name.to_string()).or_default().push(id);
            }
        }
        world.flush();
        for (owner, list, o) in orders(&ids) {
            world.resource_mut::<CommandQueue>().0.push((owner, Command::Orders { ids: list, orders: vec![o], queue: false }));
        }
        let all: Vec<(String, u32)> = ids.iter().flat_map(|(k, v)| v.iter().map(move |id| (k.clone(), *id))).collect();
        let hp0: HashMap<u32, f32> = world.query::<(&Unit, &Health)>().iter(world).map(|(u, h)| (u.id, h.0)).collect();
        let mut shots: std::collections::HashSet<u32> = Default::default();
        let mut first: Option<u32> = None;
        let trace = std::env::var("LR_TRACE").unwrap_or_default();
        for t in 0..ticks {
            world.run_schedule(SimTick);
            if !trace.is_empty() && t % 5 == 0 {
                if let Some(list) = ids.get(&trace) {
                    let mut q = world.query::<(&Unit, &Body, &Weapons, &Orders, &Mover)>();
                    for (u, b, wsn, o, m) in q.iter(world).filter(|(u, ..)| list.contains(&u.id)) {
                        println!("   t={:.1} #{} ({:.0},{:.0}) yaw {:.0} targets {:?} cd {:?} orders {} goal {:?} extend {:.1}", t as f32 / 10.0, u.id, b.pos.x, b.pos.y, b.yaw.to_degrees(), wsn.list.iter().map(|w| w.target).collect::<Vec<_>>(), wsn.list.iter().map(|w| w.cd.round()).collect::<Vec<_>>(), o.0.len(), m.goal.map(|g| (g.x.round(), g.y.round())), m.extend);
                    }
                }
            }
            for p in world.query::<&Projectile>().iter(world) {
                if p.kind != PROJ_SHELL && shots.insert(p.id) && first.is_none() {
                    first = Some(t);
                }
            }
        }
        let hp: HashMap<u32, f32> = world.query::<(&Unit, &Health)>().iter(world).map(|(u, h)| (u.id, h.0)).collect();
        println!("== {title}  ({}s; bombs/missiles released {}, first after {:.1}s)", ticks / 10, shots.len(), first.map_or(-1.0, |t| t as f32 / 10.0));
        for s in sides {
            let mine: Vec<u32> = all.iter().filter(|(n, _)| n == s.name).map(|a| a.1).collect();
            let alive = mine.iter().filter(|id| hp.contains_key(id)).count();
            let lost: f32 = mine.iter().map(|id| hp0[id] - hp.get(id).copied().unwrap_or(0.0)).sum();
            let total: f32 = mine.iter().map(|id| hp0[id]).sum();
            println!("   P{} {:12} {alive}/{} alive, hp lost {lost:.0} of {total:.0}", s.owner, s.name, s.n);
        }
    };
    type HashMapIds = HashMap<String, Vec<u32>>;
    let first = |ids: &HashMapIds, n: &str| ids[n][0];
    let every = |ids: &HashMapIds, n: &str| ids[n].clone();
    let e = 0.0;
    let w = std::f32::consts::PI;
    // Mobile anti-air: the T1 MBFAG, the T2 Hailstorm flak and the T3 Skylance SAM against two
    // Hornets (stealth 1) attack-moving onto them.
    for aa in ["mbfag", "flak_carrier", "mobile_sam"] {
        run(
            &format!("2 Hornets attack-move onto a {aa}"),
            &[Side { name: "gunship", owner: 0, n: 2, at: v2(60.0, 190.0), yaw: e }, Side { name: aa, owner: 1, n: 1, at: v2(190.0, 190.0), yaw: w }],
            &|ids| vec![(0, every(ids, "gunship"), Order::AttackMove(v2(200.0, 190.0)))],
            450,
        );
    }
    // Dove suicide drones: a workable payload, but slow and fragile, so even T1 anti-air stops them.
    run(
        "4 Doves attack-move onto 3 parked Overtakers",
        &[Side { name: "dove", owner: 0, n: 4, at: v2(80.0, 190.0), yaw: e }, Side { name: "overtaker", owner: 1, n: 3, at: v2(190.0, 190.0), yaw: w }],
        &|ids| vec![(0, every(ids, "dove"), Order::AttackMove(v2(200.0, 190.0)))],
        450,
    );
    for aa in ["mbfag", "aa_turret"] {
        run(
            &format!("6 Doves attack-move onto 2 Overtakers guarded by a {aa} (and a radar tower)"),
            &[
                Side { name: "dove", owner: 0, n: 6, at: v2(60.0, 190.0), yaw: e },
                Side { name: "overtaker", owner: 1, n: 2, at: v2(190.0, 190.0), yaw: w },
                Side { name: aa, owner: 1, n: 1, at: v2(196.0, 200.0), yaw: w },
                Side { name: "radar_tower", owner: 1, n: 1, at: v2(215.0, 190.0), yaw: w },
            ],
            &|ids| vec![(0, every(ids, "dove"), Order::AttackMove(v2(200.0, 190.0)))],
            600,
        );
    }
    run(
        "Hornet flies over 2 tanks with 4 troopers just behind them, all in range (target priority: infantry first)",
        &[Side { name: "gunship", owner: 0, n: 1, at: v2(110.0, 190.0), yaw: e }, Side { name: "crony", owner: 1, n: 2, at: v2(150.0, 190.0), yaw: w }, Side { name: "trooper", owner: 1, n: 4, at: v2(158.0, 190.0), yaw: w }],
        &|ids| vec![(0, every(ids, "gunship"), Order::Move(v2(146.0, 190.0)))],
        300,
    );
    run(
        "3 Firebrands, attack order on the middle one of 5 parked mortars",
        &[Side { name: "bomber", owner: 0, n: 3, at: v2(40.0, 190.0), yaw: e }, Side { name: "pike", owner: 1, n: 5, at: v2(190.0, 190.0), yaw: w }, Side { name: "radar_tower", owner: 0, n: 1, at: v2(130.0, 150.0), yaw: e }],
        &|ids| vec![(0, every(ids, "bomber"), Order::Attack(ids["pike"][2]))],
        900,
    );
    run(
        "3 Firebrands attack-move across 5 parked mortars",
        &[Side { name: "bomber", owner: 0, n: 3, at: v2(40.0, 190.0), yaw: e }, Side { name: "pike", owner: 1, n: 5, at: v2(190.0, 190.0), yaw: w }],
        &|ids| vec![(0, every(ids, "bomber"), Order::AttackMove(v2(330.0, 190.0)))],
        900,
    );
    run(
        "3 Firebrands, attack order on a driving mortar (5 driving)",
        &[Side { name: "bomber", owner: 0, n: 3, at: v2(40.0, 190.0), yaw: e }, Side { name: "pike", owner: 1, n: 5, at: v2(190.0, 190.0), yaw: w }, Side { name: "radar_tower", owner: 0, n: 1, at: v2(130.0, 150.0), yaw: e }],
        &|ids| vec![(0, every(ids, "bomber"), Order::Attack(ids["pike"][2])), (1, every(ids, "pike"), Order::Patrol(v2(190.0, 100.0)))],
        900,
    );
    run(
        "Delivery Boy, attack order on a land factory",
        &[Side { name: "delivery_boy", owner: 0, n: 1, at: v2(40.0, 190.0), yaw: e }, Side { name: "land_factory", owner: 1, n: 1, at: v2(190.0, 190.0), yaw: w }, Side { name: "radar_tower", owner: 0, n: 1, at: v2(130.0, 150.0), yaw: e }],
        &|ids| vec![(0, every(ids, "delivery_boy"), Order::Attack(first(ids, "land_factory")))],
        900,
    );
    run(
        "2 fighter-bombers attack-move: 2 Kestrels overhead, 5 parked mortars",
        &[
            Side { name: "fighter_bomber", owner: 0, n: 2, at: v2(40.0, 190.0), yaw: e },
            Side { name: "air_scout", owner: 1, n: 2, at: v2(150.0, 190.0), yaw: w },
            Side { name: "pike", owner: 1, n: 5, at: v2(190.0, 190.0), yaw: w },
        ],
        &|ids| vec![(0, every(ids, "fighter_bomber"), Order::AttackMove(v2(330.0, 190.0)))],
        900,
    );
    // T2 fighter-bombers should beat as many T1 interceptors.
    for n in 1..=4 {
        run(
            &format!("{n} fighter-bombers vs {n} interceptors, both attack-move to the same spot"),
            &[Side { name: "fighter_bomber", owner: 0, n, at: v2(60.0, 190.0), yaw: e }, Side { name: "interceptor", owner: 1, n, at: v2(200.0, 190.0), yaw: w }],
            &|ids| vec![(0, every(ids, "fighter_bomber"), Order::AttackMove(v2(130.0, 190.0))), (1, every(ids, "interceptor"), Order::AttackMove(v2(130.0, 190.0)))],
            1800,
        );
    }
    // About equal mass (720 against 750).
    run(
        "2 fighter-bombers vs 3 interceptors",
        &[Side { name: "fighter_bomber", owner: 0, n: 2, at: v2(60.0, 190.0), yaw: e }, Side { name: "interceptor", owner: 1, n: 3, at: v2(200.0, 190.0), yaw: w }],
        &|ids| vec![(0, every(ids, "fighter_bomber"), Order::AttackMove(v2(130.0, 190.0))), (1, every(ids, "interceptor"), Order::AttackMove(v2(130.0, 190.0)))],
        1800,
    );
    run(
        "SAM site, 4 Firebrands fly over it",
        &[Side { name: "sam", owner: 0, n: 1, at: v2(190.0, 190.0), yaw: e }, Side { name: "bomber", owner: 1, n: 4, at: v2(30.0, 200.0), yaw: e }],
        &|ids| vec![(1, every(ids, "bomber"), Order::Move(v2(360.0, 200.0)))],
        600,
    );
    run(
        "2 Cyclones vs 2 fighter-bombers",
        &[Side { name: "interceptor3", owner: 0, n: 2, at: v2(60.0, 190.0), yaw: e }, Side { name: "fighter_bomber", owner: 1, n: 2, at: v2(200.0, 190.0), yaw: w }],
        &|ids| vec![(0, every(ids, "interceptor3"), Order::AttackMove(v2(300.0, 190.0))), (1, every(ids, "fighter_bomber"), Order::AttackMove(v2(40.0, 190.0)))],
        600,
    );
    run(
        "Phalanx battery, a Delivery Boy and 3 Harriers fly over it",
        &[Side { name: "aa_turret3", owner: 0, n: 1, at: v2(190.0, 190.0), yaw: e }, Side { name: "delivery_boy", owner: 1, n: 1, at: v2(30.0, 200.0), yaw: e }, Side { name: "fighter_bomber", owner: 1, n: 3, at: v2(30.0, 180.0), yaw: e }],
        &|ids| vec![(1, every(ids, "delivery_boy"), Order::Move(v2(360.0, 200.0))), (1, every(ids, "fighter_bomber"), Order::Move(v2(360.0, 180.0)))],
        600,
    );
    run(
        "SAM site, 3 interceptors circle 60 away",
        &[Side { name: "sam", owner: 0, n: 1, at: v2(190.0, 190.0), yaw: e }, Side { name: "interceptor", owner: 1, n: 3, at: v2(250.0, 190.0), yaw: e }],
        &|_| vec![],
        900,
    );
}

#[derive(Resource)]
struct ClientAi(Option<Ai>, u32);

/// An AI playing a client's own player (testing): it thinks on the client's shown state and its
/// commands go the way a human's do (`ClientSim::issue`: predicted at once, sent to the host).
/// Runs at most once per shown tick (`last`).
fn client_ai(world: &mut World, ai: &mut Option<Ai>, last: &mut u32) -> Vec<Command> {
    use bevy::ecs::system::RunSystemOnce;
    let tick = world.resource::<Game>().tick;
    if tick == *last {
        return vec![];
    }
    *last = tick;
    let Some(a) = ai.take() else { return vec![] };
    world.insert_resource(Ais(vec![a]));
    let before = world.resource::<CommandQueue>().0.len();
    let _ = world.run_system_once(longrange::ai::ai_system);
    let cmds = world.resource_mut::<CommandQueue>().0.drain(before..).map(|(_, c)| c).collect();
    *ai = world.remove_resource::<Ais>().and_then(|mut a| a.0.pop());
    cmds
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
    println!("discovery found: {:?}", disc.found.iter().map(|f| (f.ip, f.version.clone(), f.ad.host.clone())).collect::<Vec<_>>());
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
    // LR_MAP=<name>: a premade 4-player map (with its neutral buildings) instead of a generated one.
    let map = match std::env::var("LR_MAP") {
        Ok(name) => longrange::data::load_map(&name).unwrap_or_else(|e| panic!("map {name}: {e}")),
        Err(_) => {
            let mut map = MapData::generate(128, 7, 4);
            map.scatter_features(&defs.rules.features, 7, 1.0);
            map
        }
    };
    let faction = defs.playable_factions()[0] as u8;
    // LR_PLAYERS=3: fewer players than the map has starts (the lobby's default leaves slots open).
    let players = std::env::var("LR_PLAYERS").ok().and_then(|s| s.parse::<usize>().ok()).unwrap_or(4).clamp(2, map.starts.len());
    let setup: Vec<PlayerSetup> = (0..players).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: i != 1, slot: i, color: i as u8, faction }).collect();
    host.send_msg(conn, &S2C::Start { map: map.clone(), you: 1, data: None });
    let replay_map = map.clone();
    let hw = happ.world_mut();
    setup_game(hw, map, &setup);
    // LR_AI=zones,overseer,... picks the three AIs in turn (default: Simple, as before).
    let kinds: Vec<AiKind> = std::env::var("LR_AI").ok().map(|s| s.split(',').filter_map(AiKind::parse).collect()).filter(|v: &Vec<AiKind>| !v.is_empty()).unwrap_or(vec![AiKind::Simple]);
    let ais: Vec<Ai> = (0..players as u8).filter(|p| *p != 1).collect::<Vec<u8>>().iter().enumerate().map(|(k, &p)| Ai::with_kind(p, 7, kinds[k % kinds.len()])).collect();
    for ai in &ais {
        hw.resource_mut::<Game>().players[ai.player as usize].ai_kind = ai.kind.id();
    }
    hw.insert_resource(Ais(ais));
    let mut hsim = HostSim::new(hw);
    let replay_start = make_snapshot(hw);
    host.send(conn, &frame(TAG_SNAP, bincode::serialize(&replay_start).unwrap()));
    // What the host would record for its replay.
    let (mut host_turns, mut host_hashes): (Vec<replay::Turn>, Vec<(u32, u64)>) = (vec![], vec![]);

    let mut capp = headless(defs.clone());
    let cw = capp.world_mut();
    let mut start: Option<MapData> = None;
    let mut csim: Option<ClientSim> = None;
    while csim.is_none() {
        let (tag, data) = client.frames.recv_timeout(Duration::from_secs(5)).expect("frame").expect("open");
        if tag == TAG_MSG {
            if let Ok(S2C::Start { map, you, .. }) = bincode::deserialize::<S2C>(&data) {
                println!("client got Start: map {}x{} cells ({} bytes), you={you}", map.nx, map.ny, data.len());
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
    // LR_CLIENT_AI=zones: an AI of that type plays the client's player through the client's commands.
    let mut cai = std::env::var("LR_CLIENT_AI").ok().and_then(|s| AiKind::parse(&s)).map(|k| Ai::with_kind(1, 7, k));
    let mut cai_tick = u32::MAX;
    let mut cai_cmds = 0usize;
    for t in 0..ticks {
        csim.lead = if t < ticks / 2 { 3 } else { 1 };
        if cai.is_some() {
            let cmds = client_ai(cw, &mut cai, &mut cai_tick);
            cai_cmds += cmds.len();
            for m in csim.issue(cw, cmds) {
                client.send(&m);
            }
        } else if t % 50 == 7 {
            // The client orders its units around every few seconds.
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
        let turn = hsim.tick(hw, vec![], true);
        if let S2C::Turn { tick, cmds } = &turn {
            host_turns.push((*tick, cmds.iter().map(|(p, _, c)| (*p, c.clone())).collect()));
            if tick % replay::HASH_EVERY == 0 {
                host_hashes.push((*tick, state_hash(&make_snapshot(hw))));
            }
        }
        let turn = bincode::serialize(&turn).unwrap();
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
    if cai_cmds > 0 {
        println!("the client's AI gave {cai_cmds} commands");
    }
    println!("{late} of the client's commands reached the host late; its shown tick {} is {} ahead of the confirmed one", cw.resource::<Game>().tick, cw.resource::<Game>().tick - csim.confirmed_tick());
    println!("state hash host {:016x} client {:016x}, mismatches reported during the game: {mismatches}", state_hash(&hs), csim.confirmed_hash());
    // An AI playing the client gives its orders when it likes, not in the jittery second half.
    assert!(late > 0 || cai_cmds > 0, "the late-command path was not exercised");
    assert_eq!(state_hash(&hs), csim.confirmed_hash(), "host and client diverged");
    assert_eq!(mismatches, 0);
    // Replays: the host's and the client's recordings, written to files and played back, end in the same state.
    let client_turns = std::mem::take(&mut csim.log);
    let client_hashes = std::mem::take(&mut csim.log_hashes);
    let want = state_hash(&hs);
    check_replay(&defs, &replay_map, &replay_start, host_turns, host_hashes, end, want, "host");
    check_replay(&defs, &replay_map, &replay_start, client_turns, client_hashes, end, want, "client");

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

    // Host migration: the host leaves; the client (player 1) takes over from the last state the
    // host confirmed, and the old host's machine comes back as a client of it (the way another
    // player would reconnect). They must go on in step, and the old host's player resigns.
    host.send_msg(conn, &S2C::Peers { host: 0, peers: vec![(1, None)], ai_reclaim: true });
    for _ in 0..5 {
        net_step(&host, conn, &mut client, hw, cw, &mut csim, &mut hsim, &mut paused, true);
    }
    assert!(csim.host_player == Some(0) && csim.peers == vec![(1, None)], "the peer list did not reach the client");
    let old_tick = hw.resource::<Game>().tick;
    drop(host);
    drop(client);
    std::thread::sleep(Duration::from_millis(300));
    let Role::Host { host: host2, sim: mut hsim2, mut waiting, .. } = take_over(cw, &mut csim, 1, "tester").expect("take over") else { unreachable!() };
    assert!(waiting.is_empty());
    waiting.push(0);
    let taken_at = cw.resource::<Game>().tick;
    hw.remove_resource::<Ais>();
    hw.remove_resource::<TurnLog>();
    let mut rsim = ClientSim::new(0, make_snapshot(hw));
    let mut rclient = Client::rejoin("127.0.0.1".parse().unwrap(), 0, Duration::from_secs(5)).recv().expect("rejoin thread").expect("rejoin");
    let mut conn2 = 0;
    while conn2 == 0 {
        if let HostEvent::Msg(id, C2S::Rejoin { player: 0 }) = host2.events.recv_timeout(Duration::from_secs(2)).expect("rejoin event") {
            conn2 = id;
            host2.send(id, &frame(TAG_SNAP, bincode::serialize(&make_snapshot(cw)).unwrap()));
        }
    }
    let mut mismatches2 = 0;
    for t in 0..400u32 {
        if t % 50 == 7 {
            let ids: Vec<u32> = hw.query::<&Unit>().iter(hw).filter(|u| u.owner == 0).map(|u| u.id).collect();
            for m in rsim.issue(hw, vec![Command::Orders { ids, orders: vec![Order::Move(com.1)], queue: false }]) {
                rclient.send(&m);
            }
        }
        std::thread::sleep(Duration::from_micros(300));
        while let Ok(ev) = host2.events.try_recv() {
            match ev {
                HostEvent::Msg(_, C2S::Cmd { tick, seq, cmd }) => hsim2.command(cw, 0, tick, seq, cmd),
                HostEvent::Msg(_, C2S::Hash { tick, hash }) if !hsim2.matches(tick, hash) => mismatches2 += 1,
                _ => {}
            }
        }
        host2.send(conn2, &frame(TAG_MSG, bincode::serialize(&hsim2.tick(cw, vec![], true)).unwrap()));
        while let Ok(Some((tag, data))) = rclient.frames.recv_timeout(Duration::from_micros(300)) {
            rsim.receive(tag, &data).expect("frame");
        }
        for m in rsim.update(hw, DT).expect("client update") {
            if !matches!(m, C2S::Ping(_)) {
                rclient.send(&m);
            }
        }
    }
    let end = cw.resource::<Game>().tick;
    while rsim.confirmed_tick() < end {
        let (tag, data) = rclient.frames.recv_timeout(Duration::from_secs(5)).expect("frame").expect("open");
        rsim.receive(tag, &data).expect("frame");
        rsim.update(hw, 0.0).expect("client update");
    }
    let old_host_out = !cw.resource::<Game>().players[0].alive;
    println!("host left at tick {old_tick}; the client took over at tick {taken_at} and the old host rejoined; at tick {end} hashes equal: {}, mismatches {mismatches2}, old host's player resigned: {old_host_out}", state_hash(&make_snapshot(cw)) == rsim.confirmed_hash());
    assert_eq!(state_hash(&make_snapshot(cw)), rsim.confirmed_hash(), "new host and rejoined client diverged");
    assert_eq!(mismatches2, 0);
    assert!(old_host_out, "the old host's player did not resign");
    assert!(!cw.resource::<Ais>().0.is_empty(), "the new host runs no AI");

    drop(rclient);
    loop {
        match host2.events.recv_timeout(Duration::from_secs(2)).expect("left") {
            HostEvent::Left(id) => {
                println!("conn {id} left cleanly");
                break;
            }
            _ => {}
        }
    }
    println!("nettest OK");
}

/// `--nettest`: records `turns` the way a game does (through a file), plays the file back in a
/// fresh world and checks the state at `end` against `want`, also after jumping back half way.
fn check_replay(defs: &Defs, map: &MapData, start: &Snapshot, turns: Vec<replay::Turn>, hashes: Vec<(u32, u64)>, end: u32, want: u64, who: &str) {
    let dir = std::env::temp_dir().join(format!("lr-nettest-replays-{}", std::process::id()));
    let info = ReplayInfo::new(&start.game, &map.name, &defs.pack.name, 0);
    let mut rec = Recorder::start(&dir, &info, &ReplayStart { map: map.clone(), pack: (*defs.pack).clone(), state: start.clone() }).expect("replay file");
    let n = turns.iter().filter(|t| !t.1.is_empty()).count();
    for (t, c) in turns {
        rec.push(t, c);
    }
    for (t, h) in &hashes {
        rec.hash(*t, *h);
    }
    rec.finish(None);
    let path = rec.path.clone();
    let size = std::fs::metadata(&path).map_or(0, |m| m.len());
    let l = replay::load(&path).expect("load replay");
    assert_eq!(l.end, end, "{who}'s replay ends early");
    let mut app = headless(Defs::from_pack(l.start.pack.clone()).expect("replay data"));
    let w = app.world_mut();
    let mut pb = Playback::begin(w, l);
    while Playback::tick(w) < end {
        pb.step(w, false);
    }
    let got = state_hash(&make_snapshot(w));
    pb.rewind(w, end / 2);
    let back = Playback::tick(w);
    while Playback::tick(w) < end {
        pb.step(w, true);
    }
    let again = state_hash(&make_snapshot(w));
    println!("{who}'s replay: {n} ticks with commands, {} fingerprints, {size} bytes; played back to tick {end}: same state {}, again from tick {back}: {}", hashes.len(), got == want, again == want);
    assert_eq!(got, want, "{who}'s replay plays back differently");
    assert_eq!(again, want, "{who}'s replay differs after jumping back");
    assert!(pb.desync.is_none() && !hashes.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Developer aid: `--icons FILE.png` draws every strategic icon, order icon and mouse cursor
/// (at 64 and 32 px) into one picture.
fn icon_sheet(path: &str) {
    use longrange::view::{cursor, icons};
    let defs = load_defs();
    let orders = ["move", "attack_move", "patrol", "ground", "reclaim", "revive", "capture", "terraform", "load", "unload", "ferry", "assist", "stop", "pause", "resume", "repeat", "upgrade", "mass", "energy", "time", "power", "flag"];
    let (cell, cols) = (icons::ICON + 8, 12);
    let n0 = defs.units.len() + orders.len();
    let cur0 = (n0 + cols - 1) / cols * cols;
    let ncur = cursor::CursorKind::ALL.len();
    let n = cur0 + 2 * ncur;
    let rows = (n + cols - 1) / cols;
    let mut img = image::RgbaImage::from_pixel((cols * cell) as u32, (rows * cell) as u32, image::Rgba([40, 60, 45, 255]));
    let mut blit_sized = |k: usize, px: &[u8], tint: [f32; 3], size: usize| {
        let (ox, oy) = ((k % cols) * cell + 4, (k / cols) * cell + 4);
        for y in 0..size {
            for x in 0..size {
                let s = &px[(y * size + x) * 4..];
                let a = s[3] as f32 / 255.0;
                let dst = img.get_pixel_mut((ox + x) as u32, (oy + y) as u32);
                for c in 0..3 {
                    dst.0[c] = (dst.0[c] as f32 * (1.0 - a) + s[c] as f32 * tint[c] * a) as u8;
                }
            }
        }
    };
    for (k, d) in defs.units.iter().enumerate() {
        blit_sized(k, &icons::render(&d.icon, d.tier), [0.3, 0.6, 1.0], icons::ICON);
    }
    for (k, o) in orders.iter().enumerate() {
        blit_sized(defs.units.len() + k, &icons::render_order(o), [1.0, 1.0, 1.0], icons::ICON);
    }
    for (k, c) in cursor::CursorKind::ALL.iter().enumerate() {
        blit_sized(cur0 + k, &c.pixels(64), [1.0, 1.0, 1.0], 64);
        blit_sized(cur0 + ncur + k, &c.pixels(32), [1.0, 1.0, 1.0], 32);
    }
    match img.save(path) {
        Ok(()) => println!("wrote {path}: {} unit icons in data order, {} order icons, then the {ncur} cursors at 64 and at 32 px", defs.units.len(), orders.len()),
        Err(e) => eprintln!("{path}: {e}"),
    }
}

/// Equal-cost fights of tanks against their counters on open, flat ground, and in a wood
/// (`--combattest` with `LR_TEST=tanks`). Prints the mass each side has left.
fn tank_tests(defs: &Defs) {
    let faction = defs.playable_factions()[0] as u8;
    let fights: &[(&str, usize, &str, usize)] = &[
        ("crony", 1, "trooper", 8),
        ("overtaker", 2, "trooper", 10),
        ("crony", 2, "trooper", 17),
        ("crony", 1, "at_trooper", 3),
        ("crony", 1, "sniper_trooper", 4),
        ("crony", 1, "robin", 1),
        ("overtaker", 2, "dragoon", 2),
        ("crony", 1, "mbfag", 3),
        ("crony", 1, "pike", 3),
    ];
    let custom: Vec<(String, usize, String, usize)> = std::env::var("LR_FIGHTS")
        .unwrap_or_default()
        .split(';')
        .filter_map(|f| {
            let p: Vec<&str> = f.split(':').collect();
            (p.len() == 4).then(|| (p[0].to_string(), p[1].parse().unwrap(), p[2].to_string(), p[3].parse().unwrap()))
        })
        .collect();
    let custom: Vec<(&str, usize, &str, usize)> = custom.iter().map(|(a, b, c, d)| (a.as_str(), *b, c.as_str(), *d)).collect();
    let fights: &[(&str, usize, &str, usize)] = if custom.is_empty() { fights } else { &custom };
    let woods: &[bool] = if std::env::var("LR_OPEN").is_ok() { &[false] } else { &[false, true] };
    for &wood in woods {
        println!("== Tanks, equal mass, {}", if wood { "trees scattered around" } else { "open ground" });
        for &(tank, nt, foe, nf) in fights {
            for seed in [1u64, 2, 3] {
                let mut app = headless(defs.clone());
                let mut map = MapData::generate(128, 4242 + seed, 2);
                map.heights.iter_mut().for_each(|h| *h = 2.0);
                map.feature_kinds = vec![];
                map.features = vec![];
                if wood {
                    map.feature_kinds.push("oak".into());
                    let mut r = seed.wrapping_mul(2654435761) as u32;
                    let mut rnd = || {
                        r ^= r << 13;
                        r ^= r >> 17;
                        r ^= r << 5;
                        (r % 10000) as f32 / 10000.0
                    };
                    for _ in 0..120 {
                        let p = v2(60.0 + rnd() * 140.0, 90.0 + rnd() * 80.0);
                        map.features.push(Feature { kind: 0, pos: p, yaw: 0.0, scale: 1.0 });
                    }
                }
                map.rebuild_blocked();
                let setup: Vec<PlayerSetup> = (0..2).map(|i| PlayerSetup { name: format!("P{i}"), team: i as u8, is_ai: false, slot: i, color: i as u8, faction }).collect();
                setup_game(app.world_mut(), map.clone(), &setup);
                let world = app.world_mut();
                let mut game = world.resource::<Game>().clone();
                let mut tanks = vec![];
                let mut foes = vec![];
                {
                    let mut commands = world.commands();
                    for k in 0..nt {
                        tanks.push(spawn_unit(&mut commands, defs, &mut game, &map, defs.id(tank).unwrap(), 0, v2(80.0, 126.0 + k as f32 * 4.0), 0.0, true, 0));
                    }
                    for k in 0..nf {
                        let row = (k / 6) as f32;
                        let col = (k % 6) as f32;
                        foes.push(spawn_unit(&mut commands, defs, &mut game, &map, defs.id(foe).unwrap(), 1, v2(190.0 + row * 3.0, 118.0 + col * 3.0 + seed as f32), 3.14, true, 0));
                    }
                }
                world.insert_resource(game);
                world.flush();
                // The commanders wait in far corners, out of the fight.
                let fighters: Vec<u32> = tanks.iter().chain(foes.iter()).copied().collect();
                for (u, mut b) in world.query::<(&Unit, &mut Body)>().iter_mut(world) {
                    if !fighters.contains(&u.id) {
                        b.pos = if u.owner == 0 { v2(12.0, 12.0) } else { v2(244.0, 244.0) };
                    }
                }
                world.resource_mut::<Nav>().dirty = true;
                // Both sides attack-move into each other.
                let mut q = world.resource_mut::<CommandQueue>();
                q.0.push((0, Command::Orders { ids: tanks.clone(), orders: vec![Order::AttackMove(v2(200.0, 128.0))], queue: false }));
                q.0.push((1, Command::Orders { ids: foes.clone(), orders: vec![Order::AttackMove(v2(70.0, 128.0))], queue: false }));
                let mut end = 0;
                let left = |world: &mut World, owner: u8| -> (usize, f32) {
                    let ids = if owner == 0 { &tanks } else { &foes };
                    let mut n = 0;
                    let mut m = 0.0;
                    for (u, h) in world.query::<(&Unit, &Health)>().iter(world) {
                        let d = &defs.units[u.def as usize];
                        if ids.contains(&u.id) && h.0 > 0.0 {
                            n += 1;
                            m += d.cost.mass * h.0 / d.hp;
                        }
                    }
                    (n, m)
                };
                for t in 0..3000 {
                    world.run_schedule(SimTick);
                    end = t;
                    if t % 10 == 0 && (left(world, 0).0 == 0 || left(world, 1).0 == 0) {
                        break;
                    }
                }
                let (a, am) = left(world, 0);
                let (b, bm) = left(world, 1);
                let cost = |n: &str, k: usize| defs.units[defs.id(n).unwrap() as usize].cost.mass * k as f32;
                println!(
                    "  {nt} {tank} ({:.0}) vs {nf} {foe} ({:.0}) seed {seed}: {a} tanks left ({am:.0} mass worth), {b} {foe} left ({bm:.0}), {:.0} s",
                    cost(tank, nt),
                    cost(foe, nf),
                    end as f32 / 10.0
                );
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    longrange::update::cleanup();
    if args.iter().any(|a| a == "--editor") {
        return longrange::editor::run();
    }
    if args.iter().any(|a| a == "--selftest") {
        return selftest();
    }
    if args.iter().any(|a| a == "--combattest") {
        if std::env::var("LR_TEST").as_deref() == Ok("infantry") {
            return infantry_tests(&load_defs());
        }
        if std::env::var("LR_TEST").as_deref() == Ok("fieldworks") {
            return fieldwork_tests(&load_defs());
        }
        if std::env::var("LR_TEST").as_deref() == Ok("binoculars") {
            return binocular_tests(&load_defs());
        }
        if std::env::var("LR_TEST").as_deref() == Ok("air") {
            return air_tests(&load_defs());
        }
        if std::env::var("LR_TEST").as_deref() == Ok("tanks") {
            return tank_tests(&load_defs());
        }
        if std::env::var("LR_TEST").as_deref() == Ok("scavenger") {
            return scavenger_tests(&load_defs());
        }
        if std::env::var("LR_TEST").as_deref() == Ok("apc") {
            return apc_tests(&load_defs());
        }
        return combattest();
    }
    if args.iter().any(|a| a == "--nettest") {
        return nettest();
    }
    if let Some(k) = args.iter().position(|a| a == "--ground") {
        // Every ground texture side by side, with its measured average colour.
        let out = args.get(k + 1).map_or("ground.png", |s| s.as_str());
        for g in longrange::surface::KINDS {
            let m = view::surface::measured_color(g.id);
            println!("{:<13} table {:.2?}  measured [{:.2}, {:.2}, {:.2}]", g.id, g.color, m[0], m[1], m[2]);
        }
        match view::surface::ground_sheet(std::path::Path::new(out)) {
            Ok(()) => println!("wrote {out}"),
            Err(e) => eprintln!("failed: {e}"),
        }
        return;
    }
    if let Some(k) = args.iter().position(|a| a == "--skies") {
        return longrange::view::sky::preview_sheet(args.get(k + 1).map_or("skies.png", |s| s.as_str()));
    }
    if let Some(k) = args.iter().position(|a| a == "--icons") {
        return icon_sheet(args.get(k + 1).map_or("icons.png", |s| s.as_str()));
    }
    let defs = load_defs();
    let mut name = std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "Player".into());
    name.truncate(16);
    let settings = longrange::settings::Settings::load();
    longrange::i18n::set_language(&settings.language);
    if !settings.name.trim().is_empty() {
        name = settings.name.clone();
    }
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window { title: "Long Range".into(), resolution: bevy::window::WindowResolution::new(1280, 720), ..default() }),
        ..default()
    }).set(longrange::settings::render_plugin(&settings)).set(longrange::settings::log_plugin()))
    .add_plugins(bevy_egui::EguiPlugin::default())
    // Nothing runs in Bevy's fixed timestep (the simulation has its own clock): at the default
    // 64 Hz its empty schedules ran several times a frame when the frame rate dropped late in a game.
    .insert_resource(Time::<bevy::time::Fixed>::from_seconds(1.0))
    .insert_resource(ClearColor(Color::srgb(0.06, 0.07, 0.09)))
    .insert_resource(defs.clone())
    .insert_resource(longrange::settings::StartSettings(settings.clone()))
    .insert_resource(settings)
    .insert_non_send(AppState { screen: Screen::Menu, name, ip: String::new(), message: String::new(), skirmish: args.iter().any(|a| a == "--skirmish" || a == "--bench"), settings_open: false, update: None, base_defs: defs })
    .add_plugins((SimPlugin, view::ViewPlugin))
    .add_systems(Startup, setup)
    .add_systems(PreUpdate, session_update)
    .add_systems(Update, (fullscreen_toggle, longrange::settings::apply_settings).chain())
    .add_systems(Update, dev_screenshot)
    .add_systems(bevy_egui::EguiPrimaryContextPass, (longrange::settings::apply_ui_scale, menu_ui));
    // `--bench [minutes]`: the benchmark (view/bench.rs). LR_PROFILE=1: frame and tick times in the log.
    let bench = args.iter().position(|a| a == "--bench").map(|k| args.get(k + 1).and_then(|s| s.parse::<f32>().ok()).unwrap_or(25.0));
    if bench.is_some() || std::env::var("LR_PROFILE").is_ok() {
        // GPU time per render pass (timestamp queries). Off on DirectX 12 unless LR_BENCH_GPU=1: an
        // RX 5700 XT lost the device ("GPU device instance has been suspended") with them on.
        let dx12 = app.world().resource::<longrange::settings::Settings>().effective_backend() == "dx12";
        let gpu = match std::env::var("LR_BENCH_GPU").as_deref() {
            Ok("0") => false,
            Ok(_) => true,
            Err(_) => !dx12,
        };
        view::perf::add_profiling(&mut app, gpu);
        app.insert_resource(view::perf::FrameProfile::default()).insert_resource(longrange::sim::systems::SimProfile::default());
    }
    if let Some(minutes) = bench {
        app.insert_resource(Bench::new(minutes)).insert_resource(view::perf::Benching).add_systems(Update, view::bench::bench_system);
    }
    longrange::settings::tolerate_render_errors(&mut app);
    longrange::settings::add_window_icon(&mut app);
    longrange::settings::gl_window_display(&mut app);
    app.run();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A map the host sent, saved here and loaded again, is still the same map: the next game on
    /// it must not store another copy.
    #[test]
    fn kept_map_counts_as_same() {
        let root = asset_root();
        let dir = std::env::temp_dir().join(format!("lr_keep_map_{}", std::process::id()));
        for name in ["duel", "liberec"] {
            let m = MapData::load(&root.join("maps").join(name)).unwrap();
            m.save(&dir).unwrap();
            let back = MapData::load(&dir).unwrap();
            assert!(same_map(&m, &back), "{name}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
