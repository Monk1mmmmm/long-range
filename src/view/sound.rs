//! Sound effects: shots and explosions, and the interface sounds (a unit answering when it is
//! selected, the blip of an accepted order).
//!
//! Every sound is synthesised at start-up, so the game ships no audio files. A file
//! `assets/sounds/<name>.wav` (or `.ogg`) replaces the built-in sound of that name; the names are
//! in `sim::SOUNDS` and `UI_SOUNDS`. Built only with the `audio` cargo feature (on by default): on
//! Linux it needs the ALSA development package at build time.
use crate::data::UnitDef;
use crate::sim::SOUNDS;
use bevy::prelude::*;

/// Most sounds started in one frame; the quietest are dropped.
const PER_FRAME: usize = 6;

/// Interface sounds. A unit picks its selection sound with `"select_sound"` in its JSON;
/// without it the kind of unit decides (`select_sound`).
pub const UI_SOUNDS: [&str; 9] = ["select_tank", "select_vehicle", "select_infantry", "select_air", "select_radar", "select_building", "select_engineer", "select_commander", "ack"];

/// Interface sounds asked for this frame: (name, volume 0..1).
#[derive(Resource, Default)]
pub struct UiSounds(pub Vec<(&'static str, f32)>);

/// The sound a unit makes when it is selected.
pub fn select_sound(d: &UnitDef, commander: bool) -> &'static str {
    if let Some(s) = UI_SOUNDS.iter().find(|s| **s == d.select_sound) {
        return s;
    }
    if commander {
        "select_commander"
    } else if d.radar > 0.0 && !d.armed() {
        "select_radar"
    } else if d.is_air() {
        "select_air"
    } else if d.is_structure() {
        "select_building"
    } else if d.build.is_some() {
        "select_engineer"
    } else if d.icon.shape == "bot" {
        "select_infantry"
    } else if d.armed() {
        "select_tank"
    } else {
        "select_vehicle"
    }
}

#[cfg(feature = "audio")]
mod real {
    use super::*;
    use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume};

    #[derive(Resource)]
    pub struct Sounds {
        fx: Vec<Handle<AudioSource>>,
        ui: Vec<Handle<AudioSource>>,
    }

    const RATE: u32 = 22050;

    struct Noise(u64);
    impl Noise {
        /// White noise in -1..1.
        fn next(&mut self) -> f32 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            (self.0 >> 40) as f32 / (1u64 << 23) as f32 - 1.0
        }
    }

    fn wav(samples: &[f32]) -> Vec<u8> {
        let n = samples.len();
        let mut out = Vec::with_capacity(44 + n * 2);
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&(36 + n as u32 * 2).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes()); // PCM
        out.extend_from_slice(&1u16.to_le_bytes()); // mono
        out.extend_from_slice(&RATE.to_le_bytes());
        out.extend_from_slice(&(RATE * 2).to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(n as u32 * 2).to_le_bytes());
        for s in samples {
            out.extend_from_slice(&((s.clamp(-1.0, 1.0) * 30000.0) as i16).to_le_bytes());
        }
        out
    }

    /// One of the interface sounds as WAV data.
    fn synth_ui(name: &str) -> Vec<u8> {
        use std::f32::consts::TAU;
        let len: f32 = match name {
            "select_tank" => 0.62,
            "select_vehicle" => 0.42,
            "select_infantry" => 0.30,
            "select_air" => 0.55,
            "select_radar" => 0.46,
            "select_building" => 0.40,
            "select_engineer" => 0.34,
            "select_commander" => 0.50,
            _ => 0.07,
        };
        let n = (len * RATE as f32) as usize;
        let mut noise = Noise(0x9E3779B97F4A7C15 ^ (name.len() as u64 * 104729));
        let (mut low, mut band, mut phase) = (0.0f32, 0.0f32, 0.0f32);
        let mut samples = Vec::with_capacity(n);
        // A short tone burst starting at `at`.
        let beep = |t: f32, at: f32, dur: f32, hz: f32| -> f32 {
            let x = t - at;
            if x < 0.0 || x > dur {
                return 0.0;
            }
            (TAU * hz * x).sin() * (x / 0.004).min(1.0) * ((dur - x) / 0.012).min(1.0)
        };
        for i in 0..n {
            let t = i as f32 / RATE as f32;
            let white = noise.next();
            // Fade in and out so nothing clicks.
            let edge = (t / 0.01).min(1.0) * ((len - t) / 0.04).clamp(0.0, 1.0);
            let s = match name {
                "select_tank" => {
                    // Track links clattering over the sprocket, on top of a diesel idling.
                    low += (white - low) * 0.5;
                    band += (low - band) * 0.12;
                    let link = (t * 19.0).fract();
                    let clank = (low - band) * (-link * 14.0).exp() * 1.5;
                    phase += (52.0 + 6.0 * (TAU * 9.0 * t).sin()) / RATE as f32;
                    let engine = (phase.fract() * 2.0 - 1.0) * 0.30 + (TAU * 26.0 * t).sin() * 0.25;
                    (clank + engine) * (0.6 + 0.4 * (t / len))
                }
                "select_vehicle" => {
                    // A petrol engine being blipped.
                    let rev = 85.0 + 70.0 * (t / len * 3.1).sin().max(0.0);
                    phase += rev / RATE as f32;
                    low += (white - low) * 0.2;
                    (phase.fract() * 2.0 - 1.0) * 0.4 + ((phase * 2.0).fract() - 0.5) * 0.25 + low * 0.25
                }
                "select_infantry" => {
                    // Kit rattling: two sharp clacks.
                    low += (white - low) * 0.6;
                    let hit = |at: f32| if t >= at { (-(t - at) * 70.0).exp() } else { 0.0 };
                    low * (hit(0.01) + hit(0.13) * 0.8) * 1.4 + beep(t, 0.01, 0.03, 420.0) * 0.3
                }
                "select_air" => {
                    // A turbine spooling up.
                    let k = t / len;
                    phase += (520.0 + 520.0 * k) / RATE as f32;
                    low += (white - low) * 0.35;
                    ((TAU * phase).sin() * 0.28 + (TAU * phase * 1.5).sin() * 0.12 + (white - low) * 0.22) * (0.4 + 0.6 * k)
                }
                "select_radar" => beep(t, 0.0, 0.07, 1250.0) * 0.5 + beep(t, 0.13, 0.07, 1250.0) * 0.5 + beep(t, 0.26, 0.14, 1660.0) * 0.5,
                "select_building" => {
                    // A relay clunk, then mains hum.
                    low += (white - low) * 0.08;
                    let clunk = ((TAU * 75.0 * t).sin() * 0.9 + low * 1.2) * (-t * 28.0).exp();
                    clunk + ((TAU * 100.0 * t).sin() * 0.22 + (TAU * 200.0 * t).sin() * 0.1) * (t / 0.08).min(1.0)
                }
                "select_engineer" => {
                    // A servo chirp.
                    let k = t / len;
                    phase += (480.0 + 900.0 * (k * 3.3).sin().abs() + 40.0 * (TAU * 30.0 * t).sin()) / RATE as f32;
                    (TAU * phase).sin() * 0.3 + ((phase * 2.0).fract() - 0.5) * 0.12
                }
                "select_commander" => {
                    // Heavy hydraulics and a two-tone acknowledgement.
                    low += (white - low) * 0.05;
                    low * 1.6 * (-t * 7.0).exp() + (TAU * 60.0 * t).sin() * 0.4 * (-t * 9.0).exp() + beep(t, 0.16, 0.1, 660.0) * 0.35 + beep(t, 0.29, 0.16, 880.0) * 0.35
                }
                _ => beep(t, 0.0, 0.06, 980.0) * 0.45,
            };
            samples.push(s * edge * 0.8);
        }
        wav(&samples)
    }

    /// One of the built-in sounds as 16-bit mono WAV data.
    fn synth(name: &str) -> Vec<u8> {
        // (length s, noise level, decay per second, low-pass 0..1, thump Hz, thump level, attack s)
        let (len, noise, decay, lowpass, thump, thump_level, attack): (f32, f32, f32, f32, f32, f32, f32) = match name {
            "mg" => (0.09, 0.9, 45.0, 0.55, 190.0, 0.3, 0.0),
            "cannon" => (0.45, 0.9, 11.0, 0.22, 75.0, 0.8, 0.0),
            "artillery" => (0.8, 0.8, 6.0, 0.10, 48.0, 1.0, 0.0),
            "missile" => (0.7, 0.7, 4.5, 0.30, 0.0, 0.0, 0.12),
            "bomb" => (0.25, 0.4, 14.0, 0.15, 120.0, 0.5, 0.02),
            "death" => (1.2, 1.0, 4.0, 0.07, 38.0, 1.0, 0.0),
            _ => (0.8, 1.0, 6.5, 0.12, 55.0, 0.8, 0.0), // blast
        };
        let n = (len * RATE as f32) as usize;
        let mut rng = Noise(0x2545F4914F6CDD1Du64 ^ name.len() as u64 * 7919);
        let mut low = 0.0f32;
        let mut samples = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / RATE as f32;
            let white = rng.next();
            low += (white - low) * lowpass;
            let env = (-decay * t).exp() * if attack > 0.0 { (t / attack).min(1.0) } else { 1.0 };
            // The thump drops in pitch as it rings out.
            let tone = (std::f32::consts::TAU * thump * t * (1.0 - 0.35 * t / len)).sin() * thump_level;
            let fade = ((len - t) / 0.02).clamp(0.0, 1.0);
            samples.push(((low * noise * 2.2 + tone) * env * fade).clamp(-1.0, 1.0));
        }
        wav(&samples)
    }

    pub fn load(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
        let dir = crate::data::asset_root().join("sounds");
        let mut make = |name: &str, built_in: fn(&str) -> Vec<u8>| {
            let file = ["wav", "ogg"].iter().find_map(|ext| std::fs::read(dir.join(format!("{name}.{ext}"))).ok());
            sources.add(AudioSource { bytes: file.unwrap_or_else(|| built_in(name)).into() })
        };
        let fx = SOUNDS.iter().map(|name| make(name, synth)).collect();
        let ui = UI_SOUNDS.iter().map(|name| make(name, synth_ui)).collect();
        commands.insert_resource(Sounds { fx, ui });
    }

    /// Plays the interface sounds asked for this frame (each name once).
    pub fn play_ui(mut commands: Commands, mut wanted: ResMut<UiSounds>, sounds: Option<Res<Sounds>>, settings: Res<crate::settings::Settings>) {
        let list = std::mem::take(&mut wanted.0);
        let Some(sounds) = sounds else { return };
        if settings.sfx_volume <= 0.0 {
            return;
        }
        let mut done: Vec<&str> = vec![];
        for (name, volume) in list {
            if done.contains(&name) {
                continue;
            }
            done.push(name);
            if let Some(h) = UI_SOUNDS.iter().position(|s| *s == name).and_then(|k| sounds.ui.get(k)) {
                commands.spawn((AudioPlayer::new(h.clone()), PlaybackSettings::DESPAWN.with_volume(Volume::Linear((volume * settings.sfx_volume).min(1.0)))));
            }
        }
    }

    /// Starts the loudest few of this frame's sounds: (index into `SOUNDS`, volume, playback speed).
    pub fn play(commands: &mut Commands, sounds: Option<&Sounds>, mut wanted: Vec<(usize, f32, f32)>) {
        let Some(sounds) = sounds else { return };
        wanted.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (k, volume, speed) in wanted.into_iter().take(PER_FRAME) {
            if let Some(h) = sounds.fx.get(k) {
                commands.spawn((AudioPlayer::new(h.clone()), PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume)).with_speed(speed)));
            }
        }
    }
}

#[cfg(not(feature = "audio"))]
mod real {
    use super::*;

    #[derive(Resource)]
    pub struct Sounds;

    pub fn load() {}

    pub fn play(_: &mut Commands, _: Option<&Sounds>, _: Vec<(usize, f32, f32)>) {
        let _ = (PER_FRAME, SOUNDS);
    }

    pub fn play_ui(mut wanted: ResMut<UiSounds>) {
        wanted.0.clear();
    }
}

pub use real::{load, play, play_ui, Sounds};

/// Whether this build can play sound at all.
pub const AVAILABLE: bool = cfg!(feature = "audio");
