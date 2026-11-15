//! Sound effects for shots and explosions.
//!
//! Every sound is synthesised at start-up, so the game ships no audio files. A file
//! `assets/sounds/<name>.wav` (or `.ogg`) replaces the built-in sound of that name; the names are
//! in `sim::SOUNDS`. Built only with the `audio` cargo feature (on by default): on Linux it needs
//! the ALSA development package at build time.
use crate::sim::SOUNDS;
use bevy::prelude::*;

/// Most sounds started in one frame; the quietest are dropped.
const PER_FRAME: usize = 6;

#[cfg(feature = "audio")]
mod real {
    use super::*;
    use bevy::audio::{AudioPlayer, AudioSource, PlaybackSettings, Volume};

    #[derive(Resource)]
    pub struct Sounds(Vec<Handle<AudioSource>>);

    const RATE: u32 = 22050;

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
        let mut seed = 0x2545F4914F6CDD1Du64 ^ name.len() as u64 * 7919;
        let mut low = 0.0f32;
        let mut samples = Vec::with_capacity(n);
        for i in 0..n {
            let t = i as f32 / RATE as f32;
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let white = (seed >> 40) as f32 / (1u64 << 23) as f32 - 1.0;
            low += (white - low) * lowpass;
            let env = (-decay * t).exp() * if attack > 0.0 { (t / attack).min(1.0) } else { 1.0 };
            // The thump drops in pitch as it rings out.
            let tone = (std::f32::consts::TAU * thump * t * (1.0 - 0.35 * t / len)).sin() * thump_level;
            let fade = ((len - t) / 0.02).clamp(0.0, 1.0);
            samples.push(((low * noise * 2.2 + tone) * env * fade).clamp(-1.0, 1.0));
        }
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
            out.extend_from_slice(&((s * 30000.0) as i16).to_le_bytes());
        }
        out
    }

    pub fn load(mut commands: Commands, mut sources: ResMut<Assets<AudioSource>>) {
        let dir = crate::data::asset_root().join("sounds");
        let handles = SOUNDS
            .iter()
            .map(|name| {
                let file = ["wav", "ogg"].iter().find_map(|ext| std::fs::read(dir.join(format!("{name}.{ext}"))).ok());
                sources.add(AudioSource { bytes: file.unwrap_or_else(|| synth(name)).into() })
            })
            .collect();
        commands.insert_resource(Sounds(handles));
    }

    /// Starts the loudest few of this frame's sounds: (index into `SOUNDS`, volume, playback speed).
    pub fn play(commands: &mut Commands, sounds: Option<&Sounds>, mut wanted: Vec<(usize, f32, f32)>) {
        let Some(sounds) = sounds else { return };
        wanted.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (k, volume, speed) in wanted.into_iter().take(PER_FRAME) {
            if let Some(h) = sounds.0.get(k) {
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
}

pub use real::{load, play, Sounds};

/// Whether this build can play sound at all.
pub const AVAILABLE: bool = cfg!(feature = "audio");
