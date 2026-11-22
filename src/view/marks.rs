//! Pings and lines drawn on the map (right-click / right-drag with nothing selected), in the
//! colour of the player who made them, shown to that player's team (observers see everybody's).
//!
//! They are view state only: they travel as their own messages (`C2S::Mark`, passed on by the host
//! as `S2C::Mark`), outside the turns, so they never touch the simulation, its hash or the replay,
//! and they still get through while the game is paused. The HUD draws them (`hud::ui_system`).
use crate::map::{MapData, V2};
use crate::net::Mark;
use crate::sim::{Game, OBSERVER};
use bevy::prelude::*;

/// How long a ping shows, in seconds.
pub const PING_SECS: f64 = 8.0;
/// How long a drawn line shows; it fades out over the last `FADE_SECS`.
pub const LINE_SECS: f64 = 45.0;
pub const FADE_SECS: f64 = 6.0;
/// Most points of one line (longer ones are thinned out) and most marks shown at once.
pub const MAX_POINTS: usize = 400;
const MAX_SHOWN: usize = 300;

pub struct Shown {
    pub owner: u8,
    pub mark: Mark,
    /// Real time (`Time::elapsed_secs_f64`) it arrived.
    pub born: f64,
}

impl Shown {
    pub fn life(&self) -> f64 {
        match self.mark {
            Mark::Ping(_) => PING_SECS,
            Mark::Line(_) => LINE_SECS,
        }
    }
    /// 1 while fresh, fading to 0 at the end of its life.
    pub fn alpha(&self, now: f64) -> f32 {
        ((self.born + self.life() - now) / FADE_SECS.min(self.life())).clamp(0.0, 1.0) as f32
    }
}

#[derive(Resource, Default)]
pub struct Marks {
    /// Made by the local player this frame, for the session to send.
    pub out: Vec<Mark>,
    pub shown: Vec<Shown>,
}

impl Marks {
    /// The local player's own mark: shown at once and sent to the others.
    pub fn make(&mut self, me: u8, mark: Mark, now: f64) {
        self.push(me, mark.clone(), now);
        self.out.push(mark);
    }

    fn push(&mut self, owner: u8, mark: Mark, now: f64) {
        self.shown.push(Shown { owner, mark, born: now });
        if self.shown.len() > MAX_SHOWN {
            self.shown.remove(0);
        }
    }

    pub fn expire(&mut self, now: f64) {
        self.shown.retain(|s| now < s.born + s.life());
    }
}

/// A mark as it may be shown: points on the map, a line of at least two points and at most
/// `MAX_POINTS` (thinned evenly). `None` for nonsense.
pub fn sanitize(mark: Mark, map: &MapData) -> Option<Mark> {
    let ok = |p: &V2| p.x.is_finite() && p.y.is_finite();
    match mark {
        Mark::Ping(p) => ok(&p).then(|| Mark::Ping(map.clamp(p))),
        Mark::Line(pts) => {
            let pts: Vec<V2> = pts.iter().filter(|p| ok(p)).map(|p| map.clamp(*p)).collect();
            if pts.len() < 2 {
                return None;
            }
            Some(Mark::Line(thin(pts)))
        }
    }
}

/// At most `MAX_POINTS` points, keeping both ends.
pub fn thin(pts: Vec<V2>) -> Vec<V2> {
    if pts.len() <= MAX_POINTS {
        return pts;
    }
    let n = pts.len() - 1;
    (0..MAX_POINTS).map(|k| pts[k * n / (MAX_POINTS - 1)]).collect()
}

/// Marks of other players reaching this machine: kept when the viewer is on their team (or watches),
/// with the ping sound for pings.
pub fn heard(world: &mut World, list: Vec<(u8, Mark)>) {
    if list.is_empty() {
        return;
    }
    let Some(me) = world.get_resource::<super::ViewState>().map(|v| v.me) else { return };
    let now = world.resource::<Time>().elapsed_secs_f64();
    let mut kept = vec![];
    {
        let game = world.resource::<Game>();
        let map = world.resource::<MapData>();
        for (owner, mark) in list {
            if owner == me || game.players.get(owner as usize).is_none() {
                continue;
            }
            if me != OBSERVER && game.team_of(owner) != game.team_of(me) {
                continue;
            }
            if let Some(m) = sanitize(mark, map) {
                kept.push((owner, m));
            }
        }
    }
    let ping = kept.iter().any(|k| matches!(k.1, Mark::Ping(_)));
    if let Some(mut marks) = world.get_resource_mut::<Marks>() {
        for (owner, m) in kept {
            marks.push(owner, m, now);
        }
    }
    if ping {
        if let Some(mut s) = world.get_resource_mut::<super::sound::UiSounds>() {
            s.0.push(("ping", 0.8));
        }
    }
}
