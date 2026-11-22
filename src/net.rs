//! LAN networking: TCP for the game (the host orders everybody's commands into turns, see
//! `netplay.rs`), UDP broadcast for finding hosts on the local network.
use crate::map::MapData;
use crate::sim::Command;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const TCP_PORT: u16 = 47800;
pub const UDP_PORT: u16 = 47801;
/// Discovery query: magic + a 4-byte sequence number. Reply: magic + that number + the length and
/// text of the game version + the `GameAd` (bincode), so another version still shows up, as such.
const MAGIC_Q: &[u8] = b"LRT5?";
const MAGIC_A: &[u8] = b"LRT5!";
/// Side of the map picture in a `GameAd` (RGB).
pub const AD_PREVIEW: usize = 48;
/// A host that has not answered for this long drops off the list of games.
const AD_STALE_SECS: f32 = 4.0;
/// Frame tags.
pub const TAG_MSG: u8 = 0;
pub const TAG_SNAP: u8 = 1;
/// Frames waiting to be written to one client before it is given up on.
const QUEUE: usize = 600;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct SlotInfo {
    /// 0 open, 1 closed, 2 human, 3 AI
    pub kind: u8,
    pub name: String,
    pub team: u8,
    pub faction: u8,
    /// AI slots: `ai::AiKind::id`.
    pub ai: u8,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct LobbyInfo {
    pub slots: Vec<SlotInfo>,
    pub map_name: String,
    pub you: i8,
    /// Game mode and data preset, as text.
    pub extra: String,
    /// Picture of the map (RGBA, `preview_side` squared) and its start positions (0..1 across).
    pub preview: Vec<u8>,
    pub preview_side: u16,
    pub starts: Vec<(f32, f32)>,
    /// Control points (0..1 across); empty unless the mode is control points.
    pub points: Vec<(f32, f32)>,
    /// Names of those who only watch the game.
    pub observers: Vec<String>,
    /// This player is one of the observers (`you` is then -1).
    pub observing: bool,
    /// Folder name of a premade map (empty: random), so a client can tell whether it has it.
    pub map_file: String,
    /// Data preset (empty: built-in) and the hash of the data the game will use.
    pub preset: String,
    pub data_hash: u64,
}

/// What a host tells the game browser about its game (UDP discovery reply). Texts that need
/// translating travel as data (map size, mode, preset name), the browser words them.
#[derive(Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct GameAd {
    /// The host player's name.
    pub host: String,
    /// Folder name of a premade map; empty for a random map (`cells` across).
    pub map: String,
    pub cells: u16,
    /// Map picture, RGB, `AD_PREVIEW` squared (empty: none), and the start positions (0..255 across).
    pub preview: Vec<u8>,
    pub starts: Vec<(u8, u8)>,
    pub humans: u8,
    pub ais: u8,
    /// Slots that are not closed.
    pub slots: u8,
    pub observers: u8,
    /// Room for another observer.
    pub can_watch: bool,
    /// `sim::MODE_*`.
    pub mode: u8,
    /// Data preset; empty = the built-in data.
    pub preset: String,
    pub data_hash: u64,
    pub started: bool,
}

/// The game version a host advertises.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// RGB picture of `AD_PREVIEW` squared from an RGBA picture of `side` squared (box average).
pub fn ad_preview(rgba: &[u8], side: usize) -> Vec<u8> {
    if side == 0 || rgba.len() != side * side * 4 {
        return vec![];
    }
    let n = AD_PREVIEW;
    let mut out = Vec::with_capacity(n * n * 3);
    for y in 0..n {
        for x in 0..n {
            let (x0, x1) = (x * side / n, ((x + 1) * side / n).max(x * side / n + 1));
            let (y0, y1) = (y * side / n, ((y + 1) * side / n).max(y * side / n + 1));
            let mut sum = [0u32; 3];
            for yy in y0..y1 {
                for xx in x0..x1 {
                    let i = (yy * side + xx) * 4;
                    for c in 0..3 {
                        sum[c] += rgba[i + c] as u32;
                    }
                }
            }
            let k = ((x1 - x0) * (y1 - y0)) as u32;
            out.extend(sum.iter().map(|v| (v / k) as u8));
        }
    }
    out
}

#[derive(Serialize, Deserialize)]
pub enum C2S {
    /// `data_hash` tells the host whether this player has the same game data (rules and
    /// factions); if not, the host sends its own with `Start`.
    Hello { name: String, data_hash: u64 },
    /// Lobby: the client asks for another faction.
    Faction(u8),
    /// Lobby: the client wants to watch (`true`) or to take an open slot (`false`).
    Observe(bool),
    /// A command the client applied (in its prediction) at `tick`. `seq` counts up per client.
    Cmd { tick: u32, seq: u32, cmd: Command },
    /// State fingerprint after a confirmed tick, so the host can spot a diverged client.
    Hash { tick: u32, hash: u64 },
    /// Echoed back as `Pong`; the client measures the round trip with it.
    Ping(u32),
    /// The player wants the whole game paused (`true`) or running again.
    Pause(bool),
    /// After the host left: this player connects to the player who took over as host
    /// (instead of `Hello`). The new host answers with its full state.
    Rejoin { player: u8 },
    /// A ping or a line drawn on the map, for the player's team (the host passes it on).
    Mark(Mark),
}

/// A map marking a player makes for their team: never part of the simulation, the replay or the
/// state hash (`view::marks`).
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
pub enum Mark {
    Ping(crate::map::V2),
    Line(Vec<crate::map::V2>),
}

#[derive(Serialize, Deserialize)]
pub enum S2C {
    Lobby(LobbyInfo),
    /// Followed by a `TAG_SNAP` frame with the starting state. `data` is the host's game data
    /// (a preset, or simply another version), sent to players whose own data differs.
    Start { map: MapData, you: u8, data: Option<crate::data::DataPack> },
    Kicked(String),
    /// Every command of one simulation tick: (player, the sender's `seq` or 0, command).
    Turn { tick: u32, cmds: Vec<(u8, u32, Command)> },
    Pong(u32),
    /// The game was paused by that player after the last turn sent, or (`None`) resumed.
    Paused(Option<u8>),
    /// Who takes over when the host leaves: the host's player, and every human player connected
    /// to it with their address (`None`: on the host's own machine, so reached at the host's address).
    /// Sent when the game starts and whenever a player leaves. `ai_reclaim` is the lobby's AI option,
    /// which the next host's AIs keep.
    Peers { host: u8, peers: Vec<(u8, Option<IpAddr>)>, ai_reclaim: bool },
    /// A player's ping or drawing (`C2S::Mark`), passed on to everybody else; the receiver keeps
    /// those of its own team.
    Mark(u8, Mark),
}

fn write_frame(s: &mut TcpStream, tag: u8, data: &[u8]) -> std::io::Result<()> {
    let mut head = [0u8; 5];
    head[..4].copy_from_slice(&(data.len() as u32).to_le_bytes());
    head[4] = tag;
    s.write_all(&head)?;
    s.write_all(data)
}

fn read_frame(s: &mut TcpStream) -> std::io::Result<(u8, Vec<u8>)> {
    let mut head = [0u8; 5];
    s.read_exact(&mut head)?;
    let len = u32::from_le_bytes([head[0], head[1], head[2], head[3]]) as usize;
    if len > 64 << 20 {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "frame too large"));
    }
    let mut buf = vec![0u8; len];
    s.read_exact(&mut buf)?;
    Ok((head[4], buf))
}

pub fn frame(tag: u8, data: Vec<u8>) -> Arc<(u8, Vec<u8>)> {
    Arc::new((tag, data))
}

// ------------------------------------------------------------------ host

pub enum HostEvent {
    Joined(u32),
    Msg(u32, C2S),
    Left(u32),
}

struct Conn {
    id: u32,
    ip: IpAddr,
    tx: SyncSender<Arc<(u8, Vec<u8>)>>,
}

pub struct Host {
    pub events: Receiver<HostEvent>,
    conns: Arc<Mutex<Vec<Conn>>>,
    stop: Arc<AtomicBool>,
    /// Shown in LAN discovery replies.
    ad: Arc<Mutex<GameAd>>,
}

impl Host {
    pub fn start(name: &str) -> Result<Host, String> {
        let listener = TcpListener::bind(("0.0.0.0", TCP_PORT)).map_err(|e| format!("cannot listen on port {TCP_PORT}: {e}"))?;
        listener.set_nonblocking(true).ok();
        let (etx, erx) = channel::<HostEvent>();
        let conns: Arc<Mutex<Vec<Conn>>> = Arc::new(Mutex::new(vec![]));
        let stop = Arc::new(AtomicBool::new(false));
        let ad = Arc::new(Mutex::new(GameAd { host: name.to_string(), ..Default::default() }));

        // Accept loop.
        {
            let (conns, stop, etx) = (conns.clone(), stop.clone(), etx.clone());
            std::thread::spawn(move || {
                let mut next_id = 1u32;
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, from)) => {
                            stream.set_nonblocking(false).ok();
                            stream.set_nodelay(true).ok();
                            let id = next_id;
                            next_id += 1;
                            let (tx, rx) = sync_channel::<Arc<(u8, Vec<u8>)>>(QUEUE);
                            conns.lock().unwrap().push(Conn { id, ip: from.ip(), tx });
                            // Writer.
                            if let Ok(mut ws) = stream.try_clone() {
                                ws.set_write_timeout(Some(Duration::from_secs(5))).ok();
                                std::thread::spawn(move || {
                                    while let Ok(f) = rx.recv() {
                                        if write_frame(&mut ws, f.0, &f.1).is_err() {
                                            break;
                                        }
                                    }
                                    ws.shutdown(std::net::Shutdown::Both).ok();
                                });
                            }
                            // Reader.
                            let (etx, conns) = (etx.clone(), conns.clone());
                            let mut rs = stream;
                            std::thread::spawn(move || {
                                etx.send(HostEvent::Joined(id)).ok();
                                while let Ok((_, data)) = read_frame(&mut rs) {
                                    match bincode::deserialize::<C2S>(&data) {
                                        Ok(m) => {
                                            if etx.send(HostEvent::Msg(id, m)).is_err() {
                                                break;
                                            }
                                        }
                                        Err(_) => break,
                                    }
                                }
                                conns.lock().unwrap().retain(|c| c.id != id);
                                etx.send(HostEvent::Left(id)).ok();
                            });
                        }
                        Err(_) => std::thread::sleep(Duration::from_millis(50)),
                    }
                }
            });
        }
        // Discovery responder.
        {
            let (stop, ad) = (stop.clone(), ad.clone());
            std::thread::spawn(move || {
                let Ok(sock) = UdpSocket::bind(("0.0.0.0", UDP_PORT)) else { return };
                sock.set_read_timeout(Some(Duration::from_millis(200))).ok();
                let mut buf = [0u8; 64];
                while !stop.load(Ordering::Relaxed) {
                    if let Ok((n, from)) = sock.recv_from(&mut buf) {
                        let q = MAGIC_Q.len();
                        if n == q + 4 && &buf[..q] == MAGIC_Q {
                            let mut reply = MAGIC_A.to_vec();
                            reply.extend_from_slice(&buf[q..q + 4]);
                            let v = version().as_bytes();
                            reply.push(v.len().min(255) as u8);
                            reply.extend_from_slice(&v[..v.len().min(255)]);
                            reply.extend(bincode::serialize(&*ad.lock().unwrap()).unwrap_or_default());
                            sock.send_to(&reply, from).ok();
                        }
                    }
                }
            });
        }
        Ok(Host { events: erx, conns, stop, ad })
    }

    /// What the game browser shows of this game from now on (the host's name is kept).
    pub fn advertise(&self, mut ad: GameAd) {
        let mut cur = self.ad.lock().unwrap();
        ad.host = std::mem::take(&mut cur.host);
        *cur = ad;
    }

    /// The game has begun: the browser shows it as in progress.
    pub fn set_started(&self, map: &str) {
        let mut ad = self.ad.lock().unwrap();
        ad.started = true;
        if ad.map.is_empty() && ad.cells == 0 {
            ad.map = map.to_string();
        }
    }

    /// Queues a frame for one client. Every frame matters (a client that misses a turn cannot
    /// continue), so a client whose queue is full - a minute behind - is dropped.
    pub fn send(&self, id: u32, f: &Arc<(u8, Vec<u8>)>) {
        let mut conns = self.conns.lock().unwrap();
        let mut dead = None;
        if let Some(c) = conns.iter().find(|c| c.id == id) {
            if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) = c.tx.try_send(f.clone()) {
                dead = Some(id);
            }
        }
        if let Some(id) = dead {
            conns.retain(|c| c.id != id);
        }
    }

    pub fn send_msg(&self, id: u32, m: &S2C) {
        if let Ok(data) = bincode::serialize(m) {
            self.send(id, &frame(TAG_MSG, data));
        }
    }

    /// Address of a connected player as this machine sees it; `None` for a player on this machine.
    pub fn peer_ip(&self, id: u32) -> Option<IpAddr> {
        self.conns.lock().unwrap().iter().find(|c| c.id == id).map(|c| c.ip).filter(|ip| !ip.is_loopback())
    }

    pub fn kick(&self, id: u32) {
        self.conns.lock().unwrap().retain(|c| c.id != id);
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.conns.lock().unwrap().clear();
    }
}

// ------------------------------------------------------------------ client

pub struct Client {
    pub frames: Receiver<Option<(u8, Vec<u8>)>>,
    stream: TcpStream,
    /// The host's address.
    pub addr: SocketAddr,
    /// A write failed or timed out (the host is gone or stuck): the connection is unusable.
    pub broken: bool,
}

impl Client {
    pub fn connect(addr: &str, name: &str, data_hash: u64) -> Result<Client, String> {
        let addr = if addr.contains(':') { addr.to_string() } else { format!("{addr}:{TCP_PORT}") };
        let sa: SocketAddr = addr.parse().map_err(|_| format!("'{addr}' is not a valid address"))?;
        let mut c = Client::open(sa, Duration::from_secs(3))?;
        c.send(&C2S::Hello { name: name.to_string(), data_hash });
        Ok(c)
    }

    /// Connects to the player who took over as host, in the background: tries for `wait`
    /// (that machine may not be listening yet), then gives up.
    pub fn rejoin(ip: IpAddr, player: u8, wait: Duration) -> Receiver<Result<Client, String>> {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let until = std::time::Instant::now() + wait;
            let sa = SocketAddr::new(ip, TCP_PORT);
            loop {
                match Client::open(sa, Duration::from_secs(1)) {
                    Ok(mut c) => {
                        c.send(&C2S::Rejoin { player });
                        tx.send(Ok(c)).ok();
                        return;
                    }
                    Err(e) if std::time::Instant::now() >= until => {
                        tx.send(Err(e)).ok();
                        return;
                    }
                    Err(_) => std::thread::sleep(Duration::from_millis(300)),
                }
            }
        });
        rx
    }

    fn open(sa: SocketAddr, timeout: Duration) -> Result<Client, String> {
        let stream = TcpStream::connect_timeout(&sa, timeout).map_err(|e| format!("connect failed: {e}"))?;
        stream.set_nodelay(true).ok();
        // A host that stopped reading must not freeze this game's frame loop.
        stream.set_write_timeout(Some(Duration::from_secs(3))).ok();
        let (tx, rx): (Sender<Option<(u8, Vec<u8>)>>, _) = channel();
        let mut rs = stream.try_clone().map_err(|e| e.to_string())?;
        std::thread::spawn(move || {
            while let Ok(f) = read_frame(&mut rs) {
                if tx.send(Some(f)).is_err() {
                    return;
                }
            }
            tx.send(None).ok();
        });
        Ok(Client { frames: rx, stream, addr: sa, broken: false })
    }

    pub fn send(&mut self, m: &C2S) {
        if self.broken {
            return;
        }
        if let Ok(data) = bincode::serialize(m) {
            if write_frame(&mut self.stream, TAG_MSG, &data).is_err() {
                self.broken = true;
            }
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stream.shutdown(std::net::Shutdown::Both).ok();
    }
}

// ------------------------------------------------------------------ discovery

/// A game a host on the network answered with.
pub struct Found {
    pub ip: IpAddr,
    /// The host's game version; the ad is only read when it matches ours.
    pub version: String,
    pub ad: GameAd,
    pub ping_ms: u32,
    seen: Instant,
}

/// Finds games: asks the local network (broadcast) and this machine once a second; hosts that
/// stop answering drop off the list.
pub struct Discovery {
    sock: Option<UdpSocket>,
    pub found: Vec<Found>,
    timer: f32,
    seq: u32,
    /// Recent queries: (sequence number, when sent), for the ping.
    sent: Vec<(u32, Instant)>,
}

impl Discovery {
    pub fn new() -> Discovery {
        let sock = UdpSocket::bind(("0.0.0.0", 0)).ok();
        if let Some(s) = &sock {
            s.set_broadcast(true).ok();
            s.set_nonblocking(true).ok();
        }
        Discovery { sock, found: vec![], timer: 0.0, seq: 0, sent: vec![] }
    }

    /// Forgets the games found and asks again at once.
    pub fn refresh(&mut self) {
        self.found.clear();
        self.timer = 0.0;
    }

    pub fn poll(&mut self, dt: f32) {
        let Some(s) = &self.sock else { return };
        self.timer -= dt;
        if self.timer <= 0.0 {
            self.timer = 1.0;
            self.seq = self.seq.wrapping_add(1);
            let mut q = MAGIC_Q.to_vec();
            q.extend_from_slice(&self.seq.to_le_bytes());
            s.send_to(&q, (Ipv4Addr::BROADCAST, UDP_PORT)).ok();
            s.send_to(&q, (Ipv4Addr::LOCALHOST, UDP_PORT)).ok();
            self.sent.push((self.seq, Instant::now()));
            if self.sent.len() > 8 {
                self.sent.remove(0);
            }
        }
        let now = Instant::now();
        let mut buf = vec![0u8; 65536];
        while let Ok((n, from)) = s.recv_from(&mut buf) {
            let a = MAGIC_A.len();
            if n < a + 5 || &buf[..a] != MAGIC_A {
                continue;
            }
            let seq = u32::from_le_bytes([buf[a], buf[a + 1], buf[a + 2], buf[a + 3]]);
            let vlen = buf[a + 4] as usize;
            let Some(v) = buf.get(a + 5..a + 5 + vlen) else { continue };
            let version = String::from_utf8_lossy(v).to_string();
            let ad = if version == self::version() { bincode::deserialize::<GameAd>(&buf[a + 5 + vlen..n]).unwrap_or_default() } else { GameAd::default() };
            let ping_ms = self.sent.iter().find(|q| q.0 == seq).map_or(999, |q| now.duration_since(q.1).as_millis().min(999) as u32);
            let f = Found { ip: from.ip(), version, ad, ping_ms, seen: now };
            match self.found.iter_mut().find(|e| e.ip == from.ip()) {
                Some(e) => *e = f,
                None => self.found.push(f),
            }
        }
        self.found.retain(|f| now.duration_since(f.seen).as_secs_f32() < AD_STALE_SECS);
    }
}
