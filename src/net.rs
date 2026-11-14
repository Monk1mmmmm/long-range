//! LAN networking: TCP for the game (host authoritative, state snapshots),
//! UDP broadcast for finding hosts on the local network.
use crate::map::MapData;
use crate::sim::Command;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, sync_channel, Receiver, Sender, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const TCP_PORT: u16 = 47800;
pub const UDP_PORT: u16 = 47801;
const MAGIC_Q: &[u8] = b"LRT2?";
const MAGIC_A: &[u8] = b"LRT2!";
/// Frame tags.
pub const TAG_MSG: u8 = 0;
pub const TAG_SNAP: u8 = 1;

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct SlotInfo {
    /// 0 open, 1 closed, 2 human, 3 AI
    pub kind: u8,
    pub name: String,
    pub team: u8,
    pub faction: u8,
}

#[derive(Clone, Serialize, Deserialize, PartialEq)]
pub struct LobbyInfo {
    pub slots: Vec<SlotInfo>,
    pub map_name: String,
    pub you: i8,
}

#[derive(Serialize, Deserialize)]
pub enum C2S {
    /// `data_hash` must match the host's game data (rules and factions).
    Hello { name: String, data_hash: u64 },
    /// Lobby: the client asks for another faction.
    Faction(u8),
    Cmd(Command),
}

#[derive(Serialize, Deserialize)]
pub enum S2C {
    Lobby(LobbyInfo),
    Start { map: MapData, you: u8 },
    Kicked(String),
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
    tx: SyncSender<Arc<(u8, Vec<u8>)>>,
}

pub struct Host {
    pub events: Receiver<HostEvent>,
    conns: Arc<Mutex<Vec<Conn>>>,
    stop: Arc<AtomicBool>,
    /// Shown in LAN discovery replies.
    pub info: Arc<Mutex<String>>,
}

impl Host {
    pub fn start(name: &str) -> Result<Host, String> {
        let listener = TcpListener::bind(("0.0.0.0", TCP_PORT)).map_err(|e| format!("cannot listen on port {TCP_PORT}: {e}"))?;
        listener.set_nonblocking(true).ok();
        let (etx, erx) = channel::<HostEvent>();
        let conns: Arc<Mutex<Vec<Conn>>> = Arc::new(Mutex::new(vec![]));
        let stop = Arc::new(AtomicBool::new(false));
        let info = Arc::new(Mutex::new(name.to_string()));

        // Accept loop.
        {
            let (conns, stop, etx) = (conns.clone(), stop.clone(), etx.clone());
            std::thread::spawn(move || {
                let mut next_id = 1u32;
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            stream.set_nonblocking(false).ok();
                            stream.set_nodelay(true).ok();
                            let id = next_id;
                            next_id += 1;
                            let (tx, rx) = sync_channel::<Arc<(u8, Vec<u8>)>>(8);
                            conns.lock().unwrap().push(Conn { id, tx });
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
            let (stop, info) = (stop.clone(), info.clone());
            std::thread::spawn(move || {
                let Ok(sock) = UdpSocket::bind(("0.0.0.0", UDP_PORT)) else { return };
                sock.set_read_timeout(Some(Duration::from_millis(200))).ok();
                let mut buf = [0u8; 64];
                while !stop.load(Ordering::Relaxed) {
                    if let Ok((n, from)) = sock.recv_from(&mut buf) {
                        if &buf[..n] == MAGIC_Q {
                            let mut reply = MAGIC_A.to_vec();
                            reply.extend_from_slice(info.lock().unwrap().as_bytes());
                            sock.send_to(&reply, from).ok();
                        }
                    }
                }
            });
        }
        Ok(Host { events: erx, conns, stop, info })
    }

    /// Queues a frame for one client. Snapshots are dropped if the client is lagging.
    pub fn send(&self, id: u32, f: &Arc<(u8, Vec<u8>)>) {
        let mut conns = self.conns.lock().unwrap();
        let mut dead = None;
        if let Some(c) = conns.iter().find(|c| c.id == id) {
            match c.tx.try_send(f.clone()) {
                Err(TrySendError::Full(_)) if f.0 == TAG_MSG => {
                    c.tx.send(f.clone()).ok();
                }
                Err(TrySendError::Disconnected(_)) => dead = Some(id),
                _ => {}
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
}

impl Client {
    pub fn connect(addr: &str, name: &str, data_hash: u64) -> Result<Client, String> {
        let addr = if addr.contains(':') { addr.to_string() } else { format!("{addr}:{TCP_PORT}") };
        let sa: SocketAddr = addr.parse().map_err(|_| format!("'{addr}' is not a valid address"))?;
        let stream = TcpStream::connect_timeout(&sa, Duration::from_secs(3)).map_err(|e| format!("connect failed: {e}"))?;
        stream.set_nodelay(true).ok();
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
        let mut c = Client { frames: rx, stream };
        c.send(&C2S::Hello { name: name.to_string(), data_hash });
        Ok(c)
    }

    pub fn send(&mut self, m: &C2S) {
        if let Ok(data) = bincode::serialize(m) {
            write_frame(&mut self.stream, TAG_MSG, &data).ok();
        }
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        self.stream.shutdown(std::net::Shutdown::Both).ok();
    }
}

// ------------------------------------------------------------------ discovery

pub struct Discovery {
    sock: Option<UdpSocket>,
    pub found: Vec<(IpAddr, String)>,
    timer: f32,
}

impl Discovery {
    pub fn new() -> Discovery {
        let sock = UdpSocket::bind(("0.0.0.0", 0)).ok();
        if let Some(s) = &sock {
            s.set_broadcast(true).ok();
            s.set_nonblocking(true).ok();
        }
        Discovery { sock, found: vec![], timer: 0.0 }
    }

    pub fn poll(&mut self, dt: f32) {
        let Some(s) = &self.sock else { return };
        self.timer -= dt;
        if self.timer <= 0.0 {
            self.timer = 1.0;
            s.send_to(MAGIC_Q, (Ipv4Addr::BROADCAST, UDP_PORT)).ok();
            s.send_to(MAGIC_Q, (Ipv4Addr::LOCALHOST, UDP_PORT)).ok();
        }
        let mut buf = [0u8; 256];
        while let Ok((n, from)) = s.recv_from(&mut buf) {
            if n >= MAGIC_A.len() && &buf[..MAGIC_A.len()] == MAGIC_A {
                let name = String::from_utf8_lossy(&buf[MAGIC_A.len()..n]).to_string();
                if let Some(e) = self.found.iter_mut().find(|e| e.0 == from.ip()) {
                    e.1 = name;
                } else {
                    self.found.push((from.ip(), name));
                }
            }
        }
    }
}
