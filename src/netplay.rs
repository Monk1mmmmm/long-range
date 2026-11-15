//! Deterministic netplay with client-side prediction and rollback.
//!
//! Only commands cross the network. Every machine runs the same simulation:
//!
//! - The **host** never rolls back. Each tick it applies its own commands, the AI's, and the client
//!   commands scheduled for that tick, then sends them to everybody as a `Turn`.
//! - A **client** keeps the last state the host confirmed (`confirmed`, after tick K) and shows a
//!   prediction `lead` ticks ahead of it, so that a command stamped with the shown tick reaches
//!   the host before the host simulates that tick. Its own commands are applied at once. When a
//!   turn arrives the client rolls back to `confirmed`, simulates the turn (the new confirmed
//!   state), and re-simulates up to the shown tick with its not yet confirmed commands.
//! - Every `HASH_EVERY` ticks a client reports a fingerprint of its confirmed state. If it differs
//!   from the host's, the host sends that client its full state and the client carries on from it.
use crate::net::{C2S, S2C, TAG_MSG, TAG_SNAP};
use crate::sim::Command;
use crate::sim::*;
use bevy::prelude::*;
use std::collections::{BTreeMap, VecDeque};

pub const HASH_EVERY: u32 = 20;
/// Most ticks a client predicts ahead of the host (0.8 s round trip).
const MAX_LEAD: u32 = 8;

/// Runs one tick with `cmds`. Visual events of ticks that were already simulated once are dropped,
/// so a re-simulated shot does not flash again.
fn step(world: &mut World, cmds: Vec<(u8, Command)>, fx_high: &mut u32) {
    let before = world.resource::<FxLog>().pending.len();
    world.resource_mut::<CommandQueue>().0.extend(cmds);
    world.run_schedule(SimTick);
    let tick = world.resource::<Game>().tick;
    if tick <= *fx_high {
        world.resource_mut::<FxLog>().pending.truncate(before);
    } else {
        *fx_high = tick;
    }
}

// ------------------------------------------------------------------ host

#[derive(Default)]
pub struct HostSim {
    scheduled: BTreeMap<u32, Vec<(u8, u32, Command)>>,
    hashes: VecDeque<(u32, u64)>,
}

impl HostSim {
    pub fn new(world: &mut World) -> HostSim {
        world.insert_resource(TurnLog::default());
        HostSim::default()
    }

    /// A client's command, for the tick the client applied it at - or the next one if that tick
    /// has already been simulated here (the client then corrects its prediction).
    pub fn command(&mut self, world: &World, player: u8, tick: u32, seq: u32, cmd: Command) {
        let next = world.resource::<Game>().tick + 1;
        self.scheduled.entry(tick.clamp(next, next + MAX_LEAD * 2)).or_default().push((player, seq, cmd));
    }

    /// Simulates the next tick and returns the turn to send to every client.
    /// `hashing`: also record the state fingerprint clients are checked against.
    pub fn tick(&mut self, world: &mut World, local: Vec<(u8, Command)>, hashing: bool) -> S2C {
        let tick = world.resource::<Game>().tick + 1;
        let mut cmds: Vec<(u8, u32, Command)> = local.into_iter().map(|(p, c)| (p, 0, c)).collect();
        while let Some(e) = self.scheduled.first_entry() {
            if *e.key() > tick {
                break;
            }
            cmds.extend(e.remove());
        }
        let humans = cmds.len();
        world.resource_mut::<CommandQueue>().0.extend(cmds.iter().map(|(p, _, c)| (*p, c.clone())));
        world.run_schedule(SimTick);
        // The AI added its orders behind ours; clients have no AI and get them in the turn.
        let log = std::mem::take(&mut world.resource_mut::<TurnLog>().0);
        cmds.extend(log.into_iter().skip(humans).map(|(p, c)| (p, 0, c)));
        if hashing && tick % HASH_EVERY == 0 {
            self.hashes.push_back((tick, state_hash(&make_snapshot(world))));
            if self.hashes.len() > 32 {
                self.hashes.pop_front();
            }
        }
        S2C::Turn { tick, cmds }
    }

    /// False when a client's fingerprint for `tick` differs from ours.
    pub fn matches(&self, tick: u32, hash: u64) -> bool {
        self.hashes.iter().find(|h| h.0 == tick).map_or(true, |h| h.1 == hash)
    }
}

// ------------------------------------------------------------------ client

pub struct ClientSim {
    me: u8,
    /// State after the last tick the host confirmed.
    confirmed: Snapshot,
    /// Turns received and not yet simulated.
    turns: Vec<(u32, Vec<(u8, u32, Command)>)>,
    /// Full state from the host, replacing `confirmed` (after a divergence).
    resync: Option<Snapshot>,
    /// Own commands the host has not confirmed yet: (tick applied at, seq, command).
    pending: Vec<(u32, u32, Command)>,
    seq: u32,
    /// Ticks the shown state runs ahead of `confirmed`.
    pub lead: u32,
    acc: f32,
    fx_high: u32,
    /// Times the host had to send a full state because this client diverged.
    pub resyncs: u32,
    clock: std::time::Instant,
    ping_in: f32,
    /// Smoothed round trip in milliseconds.
    pub rtt: f32,
    /// The host paused the game (by that player): the client shows the confirmed state and waits.
    pub paused: Option<u8>,
}

impl ClientSim {
    /// `state` is the starting state, already loaded into the world (`join_game`).
    pub fn new(me: u8, state: Snapshot) -> ClientSim {
        let fx_high = state.game.tick;
        ClientSim { me, confirmed: state, turns: vec![], resync: None, pending: vec![], seq: 0, lead: 1, acc: 0.0, fx_high, resyncs: 0, clock: std::time::Instant::now(), ping_in: 0.0, rtt: 0.0, paused: None }
    }

    pub fn confirmed_tick(&self) -> u32 {
        self.confirmed.game.tick
    }

    pub fn confirmed_hash(&self) -> u64 {
        state_hash(&self.confirmed)
    }

    /// Takes one frame from the host. `Err` ends the game with that message.
    pub fn receive(&mut self, tag: u8, data: &[u8]) -> Result<(), String> {
        match tag {
            TAG_MSG => match bincode::deserialize::<S2C>(data) {
                Ok(S2C::Turn { tick, cmds }) => self.turns.push((tick, cmds)),
                Ok(S2C::Pong(sent)) => {
                    let ms = (self.clock.elapsed().as_millis() as u32).saturating_sub(sent) as f32;
                    self.rtt = if self.rtt == 0.0 { ms } else { self.rtt * 0.8 + ms * 0.2 };
                    self.lead = (((self.rtt + 30.0) / (DT * 1000.0)).ceil() as u32).clamp(1, MAX_LEAD);
                }
                Ok(S2C::Paused(p)) => self.paused = p,
                Ok(S2C::Kicked(why)) => return Err(why),
                Ok(_) => {}
                Err(_) => return Err("Protocol mismatch - are both players on the same version?".into()),
            },
            TAG_SNAP => match bincode::deserialize::<Snapshot>(data) {
                Ok(s) => self.resync = Some(s),
                Err(_) => return Err("Protocol mismatch - are both players on the same version?".into()),
            },
            _ => {}
        }
        Ok(())
    }

    /// The local player's new commands: they take effect at the next shown tick. Returns what to send.
    pub fn issue(&mut self, world: &World, cmds: Vec<Command>) -> Vec<C2S> {
        let tick = world.resource::<Game>().tick + 1;
        cmds.into_iter()
            .map(|cmd| {
                self.seq += 1;
                self.pending.push((tick, self.seq, cmd.clone()));
                C2S::Cmd { tick, seq: self.seq, cmd }
            })
            .collect()
    }

    /// Once per frame: confirms the turns that arrived, re-predicts, and advances the shown tick
    /// with the clock. Returns what to send, or the reason the game cannot continue.
    pub fn update(&mut self, world: &mut World, dt: f32) -> Result<Vec<C2S>, String> {
        let mut out = vec![];
        self.ping_in -= dt;
        if self.ping_in <= 0.0 {
            self.ping_in = 1.0;
            out.push(C2S::Ping(self.clock.elapsed().as_millis() as u32));
        }
        self.acc += dt;
        let mut due = 0;
        while self.acc >= DT {
            self.acc -= DT;
            due += 1;
        }
        if due > 3 {
            due = 3;
            self.acc = 0.0;
        }
        let shown = world.resource::<Game>().tick;
        let mut k = self.confirmed.game.tick;
        if self.resync.is_some() || !self.turns.is_empty() {
            if let Some(s) = self.resync.take() {
                info!("the host sent a full state for tick {} (this machine had diverged)", s.game.tick);
                self.confirmed = s;
                self.resyncs += 1;
                k = self.confirmed.game.tick;
            }
            restore(world, &self.confirmed);
            for (tick, cmds) in std::mem::take(&mut self.turns) {
                if tick <= k {
                    continue; // already part of a full state
                }
                if tick != k + 1 {
                    return Err("Lost synchronisation with the host.".into());
                }
                let acked = cmds.iter().filter(|c| c.0 == self.me).map(|c| c.1).max().unwrap_or(0);
                self.pending.retain(|p| p.1 > acked);
                step(world, cmds.into_iter().map(|(p, _, c)| (p, c)).collect(), &mut self.fx_high);
                k = tick;
                if k % HASH_EVERY == 0 {
                    out.push(C2S::Hash { tick: k, hash: state_hash(&make_snapshot(world)) });
                }
            }
            self.confirmed = make_snapshot(world);
        }
        // Paused: the host simulates nothing more, so drop the prediction and show what it confirmed.
        // The turns before the pause arrived ahead of the pause message, so this is the paused tick.
        if self.paused.is_some() {
            if world.resource::<Game>().tick != k {
                restore(world, &self.confirmed);
            }
            self.acc = 0.0;
            flush_limbo(world);
            return Ok(out);
        }
        // Prediction: from the confirmed state (after a rollback) or from where it already was.
        let target = (shown + due).clamp(k + self.lead, k + self.lead + 2);
        loop {
            let next = world.resource::<Game>().tick + 1;
            if next > target {
                break;
            }
            // A command the host has passed over without confirming is late: it will land soon.
            let cmds = self.pending.iter().filter(|p| p.0.max(k + 1) == next).map(|p| (self.me, p.2.clone())).collect();
            step(world, cmds, &mut self.fx_high);
        }
        flush_limbo(world);
        Ok(out)
    }
}
