//! Game replays.
//!
//! The simulation is deterministic and only commands cross the network, so a replay is the state a
//! client would join with (map, game data, starting snapshot) plus the commands of every tick. Every
//! machine records the turns it confirms (`main.rs::session_update`): the host the turns it makes, a
//! client the turns it receives. Playing back re-simulates them (`main.rs::Playback`).
//!
//! File (`replays/<date>_<map>.lrr` next to `settings.json`): `MAGIC`, then frames
//! `[u32 length][u8 tag][bincode]`: `TAG_INFO` (`ReplayInfo`: small, read by the replay list),
//! `TAG_START` (`ReplayStart`, deflated), then `TAG_TURNS` chunks (`Chunk`) written while the game
//! runs (a crash loses only the last seconds) and `TAG_END` when it ends. The list reads the info and
//! only the first four bytes of each chunk (its end tick), so it never decodes a map.
use crate::data::DataPack;
use crate::map::MapData;
use crate::sim::{Command, Game, Snapshot};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub const EXT: &str = "lrr";
const MAGIC: &[u8; 8] = b"LRREPLAY";
/// Changes when the layout of the frames changes (the simulation's own changes show as a version).
pub const FORMAT: u32 = 1;
const TAG_INFO: u8 = 1;
const TAG_START: u8 = 2;
const TAG_TURNS: u8 = 3;
const TAG_END: u8 = 4;
/// Ticks between the state fingerprints a replay keeps: playing back checks against them.
pub const HASH_EVERY: u32 = 300;
/// The newest turns stay in memory this many ticks before they are written: a client that
/// reconnects to a new host (host migration) may carry on from a slightly earlier tick.
const TAIL: u32 = 100;
/// Games shorter than this (ticks) leave no replay.
const MIN_TICKS: u32 = 150;
/// Most replays kept: the oldest go when a new one starts.
pub const KEEP: usize = 200;

/// One tick's commands: (tick, [(player, command)]).
pub type Turn = (u32, Vec<(u8, Command)>);

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct ReplayPlayer {
    pub name: String,
    pub team: u8,
    pub color: u8,
    pub faction: u8,
    pub is_ai: bool,
    pub ai_kind: u8,
}

#[derive(Serialize, Deserialize, Clone, Default, Debug)]
pub struct ReplayInfo {
    /// Game version that recorded it (`CARGO_PKG_VERSION`): another version may simulate differently.
    pub version: String,
    pub format: u32,
    /// Seconds since 1970 (UTC).
    pub date: u64,
    pub map: String,
    pub mode: u8,
    /// Name of the data preset; empty for the built-in data.
    pub preset: String,
    pub players: Vec<ReplayPlayer>,
    /// The player who recorded it (`sim::OBSERVER` for an observer).
    pub recorder: u8,
}

impl ReplayInfo {
    pub fn new(game: &Game, map: &str, preset: &str, recorder: u8) -> ReplayInfo {
        let players = game.players.iter().map(|p| ReplayPlayer { name: p.name.clone(), team: p.team, color: p.color, faction: p.faction, is_ai: p.is_ai, ai_kind: p.ai_kind }).collect();
        let date = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        ReplayInfo { version: env!("CARGO_PKG_VERSION").into(), format: FORMAT, date, map: map.into(), mode: game.mode, preset: preset.into(), players, recorder }
    }
}

/// What playing back starts from: exactly what a client joins with.
#[derive(Serialize, Deserialize)]
pub struct ReplayStart {
    pub map: MapData,
    pub pack: DataPack,
    pub state: Snapshot,
}

/// Turns written in one go. `end` comes first: the list reads it without decoding the rest.
#[derive(Serialize, Deserialize, Default)]
struct Chunk {
    /// The last tick this chunk covers (also when its last ticks had no commands).
    end: u32,
    turns: Vec<Turn>,
    hashes: Vec<(u32, u64)>,
}

#[derive(Serialize, Deserialize)]
struct End {
    tick: u32,
    winner: Option<u8>,
}

/// Folder of the replays, next to `settings.json`.
pub fn dir() -> PathBuf {
    crate::settings::Settings::path().parent().map(|p| p.to_path_buf()).unwrap_or_default().join("replays")
}

fn write_frame(w: &mut impl Write, tag: u8, data: &[u8]) -> std::io::Result<()> {
    w.write_all(&(data.len() as u32).to_le_bytes())?;
    w.write_all(&[tag])?;
    w.write_all(data)
}

/// (year, month, day, hour, minute, second) of a UTC time stamp.
pub fn utc(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + (m <= 2) as i64;
    (y, m, d, (rem / 3600) as u32, (rem / 60 % 60) as u32, (rem % 60) as u32)
}

/// A file name part made of the map's name (letters, digits, `-` and `_` only).
fn safe(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).take(40).collect();
    if s.is_empty() {
        "game".into()
    } else {
        s
    }
}

// ------------------------------------------------------------------ recording

pub struct Recorder {
    out: Option<BufWriter<File>>,
    pub path: PathBuf,
    /// Turns (with commands) not written yet.
    tail: Vec<Turn>,
    hashes: Vec<(u32, u64)>,
    first: u32,
    /// Last tick recorded, and last tick written.
    end: u32,
    written: u32,
    done: bool,
}

impl Recorder {
    /// Opens a new replay file in `dir` (oldest replays beyond `KEEP` are deleted).
    pub fn start(dir: &Path, info: &ReplayInfo, start: &ReplayStart) -> Result<Recorder, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        prune(dir, KEEP.saturating_sub(1));
        let (y, mo, d, h, mi, s) = utc(info.date);
        let base = format!("{y:04}-{mo:02}-{d:02}_{h:02}-{mi:02}-{s:02}_{}", safe(&info.map));
        let mut path = dir.join(format!("{base}.{EXT}"));
        let mut k = 2;
        while path.exists() {
            path = dir.join(format!("{base}_{k}.{EXT}"));
            k += 1;
        }
        let mut out = BufWriter::new(File::create(&path).map_err(|e| e.to_string())?);
        let info_b = bincode::serialize(info).map_err(|e| e.to_string())?;
        let start_b = bincode::serialize(start).map_err(|e| e.to_string())?;
        let mut z = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        z.write_all(&start_b).map_err(|e| e.to_string())?;
        let start_z = z.finish().map_err(|e| e.to_string())?;
        (|| -> std::io::Result<()> {
            out.write_all(MAGIC)?;
            write_frame(&mut out, TAG_INFO, &info_b)?;
            write_frame(&mut out, TAG_START, &start_z)?;
            out.flush()
        })()
        .map_err(|e| e.to_string())?;
        let t = start.state.game.tick;
        Ok(Recorder { out: Some(out), path, tail: vec![], hashes: vec![], first: t, end: t, written: t, done: false })
    }

    /// The commands of `tick` (called for every tick, also those without commands).
    pub fn push(&mut self, tick: u32, cmds: Vec<(u8, Command)>) {
        if tick <= self.end {
            return;
        }
        self.end = tick;
        if !cmds.is_empty() {
            self.tail.push((tick, cmds));
        }
        if self.end >= self.written + TAIL * 3 {
            self.write(self.end - TAIL);
        }
    }

    /// The state fingerprint after `tick` (`HASH_EVERY`).
    pub fn hash(&mut self, tick: u32, hash: u64) {
        if tick > self.written {
            self.hashes.push((tick, hash));
        }
    }

    /// The game carries on from `tick` (a new host's state): what was recorded after it is dropped.
    pub fn rewind(&mut self, tick: u32) {
        if tick < self.written {
            bevy::log::warn!("replay: cannot rewind to tick {tick}, {} is already written", self.written);
        }
        let t = tick.max(self.written);
        self.tail.retain(|x| x.0 <= t);
        self.hashes.retain(|x| x.0 <= t);
        self.end = self.end.min(t);
    }

    /// Writes the turns and fingerprints up to `upto`.
    fn write(&mut self, upto: u32) {
        let Some(out) = self.out.as_mut() else { return };
        let n = self.tail.partition_point(|x| x.0 <= upto);
        let h = self.hashes.partition_point(|x| x.0 <= upto);
        let chunk = Chunk { end: upto, turns: self.tail.drain(..n).collect(), hashes: self.hashes.drain(..h).collect() };
        let ok = bincode::serialize(&chunk).map_err(|e| std::io::Error::other(e.to_string())).and_then(|b| {
            write_frame(out, TAG_TURNS, &b)?;
            out.flush()
        });
        if let Err(e) = ok {
            bevy::log::warn!("replay: could not write {}: {e}", self.path.display());
            self.out = None;
        }
        self.written = upto;
    }

    /// The game is over (or left): writes the rest. A very short game leaves no replay.
    pub fn finish(&mut self, winner: Option<u8>) {
        if self.done {
            return;
        }
        self.done = true;
        if self.end < self.first + MIN_TICKS {
            self.out = None;
            let _ = std::fs::remove_file(&self.path);
            return;
        }
        self.write(self.end);
        if let Some(out) = self.out.as_mut() {
            let end = End { tick: self.end, winner };
            if let Ok(b) = bincode::serialize(&end) {
                let _ = write_frame(out, TAG_END, &b).and_then(|_| out.flush());
            }
            bevy::log::info!("replay saved: {}", self.path.display());
        }
        self.out = None;
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.finish(None);
    }
}

// ------------------------------------------------------------------ reading

/// A replay in the list.
#[derive(Clone, Debug)]
pub struct Summary {
    pub path: PathBuf,
    pub info: ReplayInfo,
    /// Ticks it covers.
    pub ticks: u32,
    pub winner: Option<u8>,
    /// It has an end frame (else the game was cut off: crash, closed window).
    pub finished: bool,
    pub size: u64,
}

impl Summary {
    /// Recorded by this version of the game (another one may play it back differently, or not at all).
    pub fn same_version(&self) -> bool {
        self.info.version == env!("CARGO_PKG_VERSION") && self.info.format == FORMAT
    }
    pub fn name(&self) -> String {
        self.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
    }
}

fn read_head(r: &mut impl Read) -> std::io::Result<Option<(u32, u8)>> {
    let mut head = [0u8; 5];
    match r.read_exact(&mut head) {
        Ok(()) => Ok(Some((u32::from_le_bytes([head[0], head[1], head[2], head[3]]), head[4]))),
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(None),
        Err(e) => Err(e),
    }
}

fn bad(why: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, why.to_string())
}

/// Reads a replay's info and length without decoding the map or the turns.
pub fn summary(path: &Path) -> std::io::Result<Summary> {
    let file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut r = BufReader::new(file);
    let mut magic = [0u8; 8];
    r.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(bad("not a replay"));
    }
    let Some((len, TAG_INFO)) = read_head(&mut r)? else { return Err(bad("no info")) };
    let mut b = vec![0u8; len as usize];
    r.read_exact(&mut b)?;
    let info: ReplayInfo = bincode::deserialize(&b).map_err(|e| bad(&e.to_string()))?;
    let (mut ticks, mut winner, mut finished) = (0, None, false);
    let mut pos = r.stream_position()?;
    while let Some((len, tag)) = read_head(&mut r)? {
        pos += 5;
        if pos + len as u64 > size {
            break; // cut off while writing
        }
        match tag {
            TAG_TURNS if len >= 4 => {
                let mut e = [0u8; 4];
                r.read_exact(&mut e)?;
                ticks = ticks.max(u32::from_le_bytes(e));
                r.seek(SeekFrom::Start(pos + len as u64))?;
            }
            TAG_END => {
                let mut b = vec![0u8; len as usize];
                r.read_exact(&mut b)?;
                if let Ok(e) = bincode::deserialize::<End>(&b) {
                    ticks = ticks.max(e.tick);
                    winner = e.winner;
                    finished = true;
                }
            }
            _ => {
                r.seek(SeekFrom::Start(pos + len as u64))?;
            }
        }
        pos += len as u64;
    }
    Ok(Summary { path: path.to_path_buf(), info, ticks, winner, finished, size })
}

/// Every replay in `dir`, newest first.
pub fn list(dir: &Path) -> Vec<Summary> {
    let mut v: Vec<Summary> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == EXT)).filter_map(|p| summary(&p).ok()).collect())
        .unwrap_or_default();
    v.sort_by(|a, b| b.info.date.cmp(&a.info.date).then_with(|| b.path.cmp(&a.path)));
    v
}

/// Deletes the oldest replays so that at most `keep` remain.
fn prune(dir: &Path, keep: usize) {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .map(|rd| rd.filter_map(|e| e.ok()).filter(|e| e.path().extension().map_or(false, |x| x == EXT)).map(|e| (e.metadata().and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), e.path())).collect())
        .unwrap_or_default();
    if files.len() <= keep {
        return;
    }
    files.sort();
    let n = files.len() - keep;
    for (_, p) in files.into_iter().take(n) {
        let _ = std::fs::remove_file(p);
    }
}

/// A whole replay, ready to play back.
pub struct Loaded {
    pub info: ReplayInfo,
    pub start: ReplayStart,
    /// Ticks with commands, in order.
    pub turns: Vec<Turn>,
    pub hashes: Vec<(u32, u64)>,
    /// The last tick recorded.
    pub end: u32,
    pub winner: Option<u8>,
}

pub fn load(path: &Path) -> Result<Loaded, String> {
    let s = summary(path).map_err(|e| e.to_string())?;
    if s.info.format != FORMAT {
        return Err(format!("format {}", s.info.format));
    }
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    let mut at = 8usize;
    let mut start: Option<ReplayStart> = None;
    let (mut turns, mut hashes, mut end) = (vec![], vec![], 0u32);
    while at + 5 <= data.len() {
        let len = u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]]) as usize;
        let tag = data[at + 4];
        at += 5;
        if at + len > data.len() {
            break;
        }
        let b = &data[at..at + len];
        at += len;
        match tag {
            TAG_START => {
                let mut raw = vec![];
                flate2::read::DeflateDecoder::new(b).read_to_end(&mut raw).map_err(|e| e.to_string())?;
                let st: ReplayStart = bincode::deserialize(&raw).map_err(|e| e.to_string())?;
                end = st.state.game.tick;
                start = Some(st);
            }
            TAG_TURNS => {
                let c: Chunk = bincode::deserialize(b).map_err(|e| e.to_string())?;
                end = end.max(c.end);
                turns.extend(c.turns);
                hashes.extend(c.hashes);
            }
            TAG_END => {
                if let Ok(e) = bincode::deserialize::<End>(b) {
                    end = end.max(e.tick);
                }
            }
            _ => {}
        }
    }
    let start = start.ok_or("no start state")?;
    turns.sort_by_key(|t| t.0);
    Ok(Loaded { info: s.info, start, turns, hashes, end, winner: s.winner })
}

// ------------------------------------------------------------------ playing back

/// Plays a loaded replay into the simulation world: one tick at a time, or jumping to a tick
/// (back: from the nearest state kept on the way, every `HASH_EVERY` ticks).
pub struct Playback {
    turns: Vec<Turn>,
    hashes: Vec<(u32, u64)>,
    /// Index of the next turn to apply.
    next: usize,
    /// Tick of the starting state and the last recorded tick.
    pub first: u32,
    pub end: u32,
    keys: std::collections::BTreeMap<u32, Snapshot>,
    /// First tick whose state differs from the recording (another version, or a bug).
    pub desync: Option<u32>,
}

impl Playback {
    /// Sets the world up as the game began (as a joining client would) and returns the player.
    /// The world must already hold the replay's `Defs`.
    pub fn begin(world: &mut bevy::prelude::World, l: Loaded) -> Playback {
        let Loaded { start, turns, hashes, end, .. } = l;
        crate::sim::join_game(world, start.map, &start.state);
        let first = start.state.game.tick;
        let mut keys = std::collections::BTreeMap::new();
        keys.insert(first, start.state);
        Playback { turns, hashes, next: 0, first, end, keys, desync: None }
    }

    pub fn tick(world: &bevy::prelude::World) -> u32 {
        world.resource::<Game>().tick
    }

    /// Simulates the next tick. `quiet`: no visual effects (while seeking).
    pub fn step(&mut self, world: &mut bevy::prelude::World, quiet: bool) {
        let tick = Self::tick(world) + 1;
        while self.turns.get(self.next).map_or(false, |t| t.0 < tick) {
            self.next += 1;
        }
        let cmds = match self.turns.get(self.next) {
            Some(t) if t.0 == tick => {
                self.next += 1;
                t.1.clone()
            }
            _ => vec![],
        };
        let before = world.resource::<crate::sim::FxLog>().pending.len();
        world.resource_mut::<crate::sim::CommandQueue>().0.extend(cmds);
        world.run_schedule(crate::sim::SimTick);
        if quiet {
            world.resource_mut::<crate::sim::FxLog>().pending.truncate(before);
        }
        if tick % HASH_EVERY == 0 {
            let snap = crate::sim::make_snapshot(world);
            if let Ok(k) = self.hashes.binary_search_by_key(&tick, |h| h.0) {
                if self.desync.is_none() && crate::sim::state_hash(&snap) != self.hashes[k].1 {
                    bevy::log::warn!("replay: the state at tick {tick} differs from the recording");
                    self.desync = Some(tick);
                }
            }
            self.keys.entry(tick).or_insert(snap);
        }
    }

    /// Goes back to the latest kept state at or before `tick` (the caller then steps on to it).
    /// Nothing happens when `tick` is ahead of the world's tick.
    pub fn rewind(&mut self, world: &mut bevy::prelude::World, tick: u32) {
        if tick >= Self::tick(world) {
            return;
        }
        let Some((&at, snap)) = self.keys.range(..=tick).next_back() else { return };
        crate::sim::restore(world, snap);
        crate::sim::flush_limbo(world);
        self.next = self.turns.partition_point(|t| t.0 <= at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(utc(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(utc(1_700_000_000), (2023, 11, 14, 22, 13, 20));
        assert_eq!(utc(951_782_400), (2000, 2, 29, 0, 0, 0));
    }

    #[test]
    fn write_and_read() {
        let dir = std::env::temp_dir().join(format!("lr-replay-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut game = Game::default();
        game.players.push(crate::sim::Player { name: "A".into(), ..Default::default() });
        let info = ReplayInfo::new(&game, "Some map", "", 0);
        let start = ReplayStart { map: MapData::default(), pack: DataPack::default(), state: Snapshot { game, ..Default::default() } };
        let mut rec = Recorder::start(&dir, &info, &start).unwrap();
        for t in 1..=1000 {
            let cmds = if t % 7 == 0 { vec![(0u8, Command::Resign)] } else { vec![] };
            rec.push(t, cmds);
            if t % HASH_EVERY == 0 {
                rec.hash(t, t as u64 * 3);
            }
        }
        // Mid-game, the list already sees most of it.
        let s = summary(&rec.path).unwrap();
        assert!(!s.finished && s.ticks >= 1000 - TAIL * 3 && s.ticks <= 1000);
        rec.rewind(990);
        rec.finish(Some(1));
        let path = rec.path.clone();
        let s = &list(&dir)[0];
        assert_eq!((s.ticks, s.winner, s.finished, s.info.players[0].name.as_str(), s.info.map.as_str()), (990, Some(1), true, "A", "Some map"));
        let l = load(&path).unwrap();
        assert_eq!(l.end, 990);
        assert_eq!(l.turns.len(), 990 / 7);
        assert!(l.turns.windows(2).all(|w| w[0].0 + 7 == w[1].0));
        assert_eq!(l.hashes, vec![(300, 900), (600, 1800), (900, 2700)]);
        // Too short to keep.
        let mut rec = Recorder::start(&dir, &info, &start).unwrap();
        rec.push(10, vec![]);
        let p = rec.path.clone();
        drop(rec);
        assert!(!p.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
