//! Game data loaded from JSON: rules, factions, unit and weapon definitions.
//! Nothing about a specific unit is hard-coded in Rust; see `assets/data`.
use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub type DefId = u16;

/// Where the `assets` folder lives: next to Cargo.toml during development, next to the exe otherwise.
pub fn asset_root() -> PathBuf {
    if let Ok(p) = std::env::var("LR_ASSETS") {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var("CARGO_MANIFEST_DIR") {
        let p = PathBuf::from(p).join("assets");
        if p.exists() {
            return p;
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("assets");
            if p.exists() {
                return p;
            }
        }
    }
    PathBuf::from("assets")
}

fn capture_energy() -> f32 {
    0.3
}
fn infantry_capture_secs() -> f32 {
    20.0
}
fn infantry_capture_guard() -> f32 {
    15.0
}
fn infantry_capture_reach() -> f32 {
    12.0
}
fn eye_height() -> f32 {
    1.5
}
fn one() -> f32 {
    1.0
}
fn albedo_brightness() -> f32 {
    2.1
}
fn one_u8() -> u8 {
    1
}
fn yes() -> bool {
    true
}
fn near_sight() -> f32 {
    6.0
}
fn spot_forget() -> f32 {
    3.0
}
fn crater_scale() -> f32 {
    0.025
}
fn crater_max_depth() -> f32 {
    2.0
}
fn crater_min_damage() -> f32 {
    35.0
}
fn terraform_rate() -> f32 {
    0.6
}
fn terraform_energy() -> f32 {
    15.0
}
fn terraform_max() -> f32 {
    12.0
}
fn cp_radius() -> f32 {
    12.0
}
fn cp_capture_secs() -> f32 {
    20.0
}
fn cp_start_points() -> f32 {
    10000.0
}
fn cp_bleed() -> f32 {
    1.0
}
fn reclaim_energy_factor() -> f32 {
    5.0
}
fn wreck_cover() -> f32 {
    0.5
}
fn cover_grace() -> f32 {
    2.5
}
fn cover_search() -> f32 {
    10.0
}
fn cover_threat_range() -> f32 {
    100.0
}
fn garrison_eject_damage() -> f32 {
    0.4
}
fn garrison_defense() -> f32 {
    1.25
}
fn armor_front() -> f32 {
    0.6
}
fn armor_rear() -> f32 {
    1.5
}
fn splash_direct() -> f32 {
    0.4
}
fn splash_armored() -> f32 {
    0.5
}
fn garrison_damage() -> f32 {
    0.04
}
fn garrison_vision() -> f32 {
    1.3
}
fn binocular_angle() -> f32 {
    20.0
}
fn binocular_turn() -> f32 {
    40.0
}
fn binocular_scan() -> f32 {
    60.0
}
/// Debris: pieces that break off buildings. Their mass comes out of the building's wreck value
/// (`wreck_mass_fraction` of its cost), so damaging and repairing a building makes no mass.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct DebrisRules {
    /// Off: buildings drop nothing.
    pub enabled: bool,
    /// A single hit doing at least this share of a finished building's hit points breaks pieces off.
    pub min_hit: f32,
    /// Such a hit drops this share of the wreck value the damage stands for (damage / hp x value).
    pub hit_share: f32,
    /// Hits drop at most this share of the wreck value in all; the rest stays for the wreck.
    pub max_shed: f32,
    /// A destroyed building drops this share of the wreck value it has left around its wreck.
    pub death_share: f32,
    /// Pieces a destroyed building drops (fewer for small ones: see `min_mass`).
    pub death_pieces: u32,
    /// Most pieces one hit breaks off.
    pub hit_pieces: u32,
    /// No piece holds less mass than this.
    pub min_mass: f32,
    /// At most this many pieces lie around; the oldest go first.
    pub max: usize,
    /// A falling piece hurts units it lands on: this much damage per mass, at most `land_damage_max`.
    pub land_damage: f32,
    pub land_damage_max: f32,
    /// Pieces at least this big block sight like wrecks.
    pub sight_size: f32,
}

impl Default for DebrisRules {
    fn default() -> Self {
        DebrisRules { enabled: true, min_hit: 0.08, hit_share: 0.5, max_shed: 0.5, death_share: 0.3, death_pieces: 6, hit_pieces: 4, min_mass: 1.5, max: 300, land_damage: 2.0, land_damage_max: 40.0, sight_size: 0.9 }
    }
}

fn wreck_hp() -> f32 {
    0.5
}
fn revive_cooldown() -> f32 {
    60.0
}
fn trunk_color() -> [f32; 3] {
    [0.33, 0.24, 0.16]
}

/// What a map generator does with a feature type.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FeatureClass {
    #[default]
    Tree,
    Rock,
}

/// How a feature type is drawn (procedural low-poly meshes, see `view::flora`).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum FeatureShape {
    /// Stacked cones on a short trunk.
    #[default]
    Pine,
    /// A round crown on a trunk.
    Broadleaf,
    /// A low round crown, no trunk.
    Bush,
    /// A lumpy stone.
    Rock,
}

/// A reclaimable map feature type: a tree or a rock (`rules.json: features`).
#[derive(Deserialize, Clone, Debug)]
pub struct FeatureDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub class: FeatureClass,
    #[serde(default)]
    pub shape: FeatureShape,
    /// What reclaiming it gives.
    #[serde(default)]
    pub mass: f32,
    #[serde(default)]
    pub energy: f32,
    /// Scale of the model (1 = a pine about 3 units tall, a rock about 1 unit across).
    #[serde(default = "one")]
    pub size: f32,
    /// Foliage or stone colour.
    pub color: [f32; 3],
    #[serde(default = "trunk_color")]
    pub trunk: [f32; 3],
    /// How often the map generator picks it among the features of its class.
    #[serde(default = "one")]
    pub weight: f32,
    /// Weapon damage that destroys it; 0 = a default for its shape and size (see `max_hp`).
    #[serde(default)]
    pub hp: f32,
    /// Share of a shot's damage it soaks up when the shot passes through it (cover);
    /// leave it out for a default by shape (see `cover`).
    #[serde(default)]
    pub cover: Option<f32>,
}

impl FeatureDef {
    /// Footprint radius on the ground: reclaim reach and the spacing of brushes and the generator.
    pub fn radius(&self) -> f32 {
        self.size
            * match self.shape {
                FeatureShape::Pine => 0.8,
                FeatureShape::Broadleaf => 1.1,
                FeatureShape::Bush => 0.6,
                FeatureShape::Rock => 0.9,
            }
    }
    /// Height of the model (it blocks sight up to there).
    pub fn height(&self) -> f32 {
        self.size
            * match self.shape {
                FeatureShape::Pine => 3.1,
                FeatureShape::Broadleaf => 2.8,
                FeatureShape::Bush => 0.75,
                FeatureShape::Rock => 0.6,
            }
    }
    /// Hit points at the map's scale 1.
    pub fn max_hp(&self) -> f32 {
        if self.hp > 0.0 {
            return self.hp;
        }
        self.size
            * match self.shape {
                FeatureShape::Pine => 80.0,
                FeatureShape::Broadleaf => 110.0,
                FeatureShape::Bush => 20.0,
                FeatureShape::Rock => 400.0,
            }
    }
    /// Share of the damage of a shot passing through that it takes instead of what is behind it.
    pub fn cover(&self) -> f32 {
        self.cover
            .unwrap_or(match self.shape {
                FeatureShape::Pine => 0.3,
                FeatureShape::Broadleaf => 0.35,
                FeatureShape::Bush => 0.15,
                FeatureShape::Rock => 0.65,
            })
            .clamp(0.0, 1.0)
    }
    /// Reclaim work in mass units (energy comes `reclaim_energy_factor` times faster than mass).
    pub fn work(&self, rules: &Rules) -> f32 {
        self.mass + self.energy / rules.reclaim_energy_factor.max(0.01)
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct Blast {
    pub damage: f32,
    pub radius: f32,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Rules {
    pub gravity: f32,
    pub start_mass: f32,
    pub start_energy: f32,
    pub wreck_mass_fraction: f32,
    pub max_wrecks: usize,
    /// A wreck's hit points as a share of the unit's (weapons wear wrecks down).
    #[serde(default = "wreck_hp")]
    pub wreck_hp: f32,
    /// Seconds before the wreck of a revived unit (or of an unfinished revival site) can be revived again.
    #[serde(default = "revive_cooldown")]
    pub revive_cooldown: f32,
    /// Buildings dropping pieces of debris when hit hard or destroyed.
    #[serde(default)]
    pub debris: DebrisRules,
    pub reclaim_rate: f32,
    pub repair_rate: f32,
    pub radar_fade_secs: f32,
    pub radar_los_height: f32,
    /// Eye height above a unit's top for vision; hills taller than that block the view.
    #[serde(default = "eye_height")]
    pub vision_los_height: f32,
    pub power_stall_pump_factor: f32,
    /// Capturing takes the target's build time times this, at the engineer's build power.
    #[serde(default = "one")]
    pub capture_time: f32,
    /// Share of the target's energy cost a capture uses up.
    #[serde(default = "capture_energy")]
    pub capture_energy: f32,
    /// Lobby option "infantry capture buildings" (`Game.infantry_capture`): seconds one
    /// infantryman takes to take a building over (n men take 1/n of it), free of energy.
    #[serde(default = "infantry_capture_secs")]
    pub infantry_capture_secs: f32,
    /// ... and an armed enemy unit within this distance of the building stops it.
    #[serde(default = "infantry_capture_guard")]
    pub infantry_capture_guard: f32,
    /// Idle infantry (and infantry on attack-move or patrol with nothing to shoot) take a
    /// capturable building by themselves when one is within this distance, no order needed.
    #[serde(default = "infantry_capture_reach")]
    pub infantry_capture_reach: f32,
    #[serde(default = "yes")]
    pub friendly_fire: bool,
    /// Armored units (category `armored`) take this share of a hit on their front, side or rear,
    /// blended by the angle the shot comes in at; the steeper a shell falls, the less the angle
    /// counts (straight down: 1). See `systems::armor_angle`.
    #[serde(default = "armor_front")]
    pub armor_front: f32,
    #[serde(default = "one")]
    pub armor_side: f32,
    #[serde(default = "armor_rear")]
    pub armor_rear: f32,
    /// Splash of direct-fire guns (weapons of kind `direct` without their own `splash`): the share
    /// of the shell's damage a unit it did not hit takes at the centre of the blast (fading to
    /// nothing at the edge). The unit hit directly always takes all of it.
    #[serde(default = "splash_direct")]
    pub splash_direct: f32,
    /// Armored units (category `armored`, not structures) take this share of any splash: shell
    /// blasts they were not hit by directly and death blasts.
    #[serde(default = "splash_armored")]
    pub splash_armored: f32,
    /// Switches for whole game features (the preset editor's "Switches" section).
    /// Engineers can level, raise and lower the ground.
    #[serde(default = "yes")]
    pub terraforming: bool,
    /// Heavy hits and blasts dig craters (their size: `crater_*`).
    #[serde(default = "yes")]
    pub craters: bool,
    /// Destroyed units leave wrecks.
    #[serde(default = "yes")]
    pub wrecks: bool,
    /// Hills block sight.
    #[serde(default = "yes")]
    pub sight_blocked_by_terrain: bool,
    /// Standing trees, rocks and wrecks block sight.
    #[serde(default = "yes")]
    pub sight_blocked_by_reclaimables: bool,
    /// Finished buildings block sight (their footprint, up to their height).
    #[serde(default = "yes")]
    pub sight_blocked_by_buildings: bool,
    /// Hills block radar.
    #[serde(default = "yes")]
    pub radar_blocked_by_terrain: bool,
    /// Counter-battery radar: a team's radar that has an artillery shell in flight in range finds
    /// the gun that fired it (it shows as a radar contact while its shells fly).
    #[serde(default = "yes")]
    pub counter_battery: bool,
    /// A remembered enemy building that was destroyed out of sight shows dimmed (SupCom style; view only).
    #[serde(default = "yes")]
    pub ghosts_show_destroyed: bool,
    /// AI engineers reclaim trees and rocks when short of resources.
    #[serde(default = "yes")]
    pub ai_reclaim: bool,
    /// Everything closer than this to a unit is seen, whatever stands in between.
    #[serde(default = "near_sight")]
    pub near_sight: f32,
    /// Spotting progress on a unit with a `spot_time` drains over this many seconds once the
    /// enemy has lost sight of it.
    #[serde(default = "spot_forget")]
    pub spot_forget: f32,
    /// Shell holes: depth = `crater_scale` x sqrt(damage); 0 switches craters off.
    #[serde(default = "crater_scale")]
    pub crater_scale: f32,
    /// Shelling never digs deeper than this below the original ground.
    #[serde(default = "crater_max_depth")]
    pub crater_max_depth: f32,
    /// Hits weaker than this leave no mark.
    #[serde(default = "crater_min_damage")]
    pub crater_min_damage: f32,
    /// Terraforming: height x vertices moved per second per point of build power.
    #[serde(default = "terraform_rate")]
    pub terraform_rate: f32,
    /// Energy per unit of height moved.
    #[serde(default = "terraform_energy")]
    pub terraform_energy: f32,
    /// Terraforming stays within this distance of the original ground.
    #[serde(default = "terraform_max")]
    pub terraform_max: f32,
    /// Control points mode: capture radius, seconds to take a point, score that wins
    /// (every held point gives one point per second).
    #[serde(default = "cp_radius")]
    pub cp_radius: f32,
    #[serde(default = "cp_capture_secs")]
    pub cp_capture_secs: f32,
    /// Points every team starts with unless the host picks another number in the lobby.
    #[serde(default = "cp_start_points", alias = "cp_score_limit")]
    pub cp_start_points: f32,
    /// Points lost per second for every control point the leading opponent holds more.
    #[serde(default = "cp_bleed")]
    pub cp_bleed: f32,
    /// Reclaiming trees and rocks: energy comes this many times faster than mass.
    #[serde(default = "reclaim_energy_factor")]
    pub reclaim_energy_factor: f32,
    /// Share of a shot's damage a wreck soaks up when the shot passes through it.
    #[serde(default = "wreck_cover")]
    pub wreck_cover: f32,
    /// Trees, rocks and wrecks closer than this to where a shot was fired give no cover from it
    /// (a unit fires over or around its own cover).
    #[serde(default = "cover_grace")]
    pub cover_grace: f32,
    /// Infantry: how far from where they were sent they look for cover.
    #[serde(default = "cover_search")]
    pub cover_search: f32,
    /// Infantry take cover from known armed enemies up to this far away.
    #[serde(default = "cover_threat_range")]
    pub cover_threat_range: f32,
    /// Infantry thrown out of a destroyed building lose this share of their hit points.
    #[serde(default = "garrison_eject_damage")]
    pub garrison_eject_damage: f32,
    /// Infantry holding a building deal this many times their `assault` damage to those storming it.
    #[serde(default = "garrison_defense")]
    pub garrison_defense: f32,
    /// Infantry inside a building take this share of every hit on it (their own armor applies).
    #[serde(default = "garrison_damage")]
    pub garrison_damage: f32,
    /// Infantry inside a building look from its roof: their vision and binocular range times this.
    #[serde(default = "garrison_vision")]
    pub garrison_vision: f32,
    /// Reclaimable trees and rocks maps can place.
    #[serde(default)]
    pub features: Vec<FeatureDef>,
    pub commander_death: Blast,
    pub damage_types: Vec<String>,
    pub armor: HashMap<String, HashMap<String, f32>>,
    pub player_colors: Vec<[f32; 3]>,
    pub neutral_color: [f32; 3],
    pub hostile_color: [f32; 3],
    /// armor index -> damage type index -> multiplier (built at load).
    #[serde(skip)]
    pub armor_table: Vec<Vec<f32>>,
    #[serde(skip)]
    pub armor_names: Vec<String>,
}

impl Rules {
    /// Craters are dug at all.
    pub fn craters_on(&self) -> bool {
        self.craters && self.crater_scale > 0.0
    }
    /// The game features this data switches off (for the lobby).
    pub fn switched_off(&self) -> Vec<&'static str> {
        let mut out = vec![];
        for (on, name) in [
            (self.terraforming, "terraforming"),
            (self.craters_on(), "craters"),
            (self.wrecks, "wrecks"),
            (self.sight_blocked_by_terrain, "hills blocking sight"),
            (self.sight_blocked_by_reclaimables, "trees, rocks and wrecks blocking sight"),
            (self.sight_blocked_by_buildings, "buildings blocking sight"),
            (self.radar_blocked_by_terrain, "hills blocking radar"),
            (self.counter_battery, "counter-battery radar"),
            (self.ghosts_show_destroyed, "dimming remembered buildings destroyed out of sight"),
            (self.ai_reclaim, "AI reclaiming trees and rocks"),
        ] {
            if !on {
                out.push(name);
            }
        }
        out
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Cost {
    pub mass: f32,
    pub energy: f32,
    pub time: f32,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum MoveKind {
    #[default]
    None,
    Land,
    Hover,
    Plane,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Move {
    #[serde(default)]
    pub kind: MoveKind,
    #[serde(default)]
    pub speed: f32,
    #[serde(default)]
    pub reverse: f32,
    #[serde(default = "one")]
    pub accel: f32,
    /// Degrees per second.
    #[serde(default)]
    pub turn: f32,
    #[serde(default)]
    pub altitude: f32,
    /// Land units only: also drives along the bottom of water up to `map::AMPHIBIOUS_DEPTH` deep
    /// (submerged, out of reach of shells and guns).
    #[serde(default)]
    pub amphibious: bool,
}

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Eco {
    #[serde(default)]
    pub mass: f32,
    #[serde(default)]
    pub energy: f32,
    #[serde(default)]
    pub upkeep: f32,
    #[serde(default)]
    pub store_mass: f32,
    #[serde(default)]
    pub store_energy: f32,
}

#[derive(Deserialize, Clone, Debug)]
pub struct BuildCap {
    pub rate: f32,
    #[serde(default)]
    pub range: f32,
    /// Build categories this unit can construct (matched against `built_by`).
    #[serde(default)]
    pub categories: Vec<String>,
    #[serde(default = "one_u8")]
    pub tier: u8,
    /// Engineering station: immobile, assists everything in range on its own.
    #[serde(default)]
    pub station: bool,
    /// Scavenger: cannot build or assist; reclaims wrecks and revives them for its owner.
    #[serde(default)]
    pub scavenger: bool,
}

/// Binoculars: a narrow view cone that reaches much further than the unit's vision. It scans
/// around the way the unit faces (all round from a building's roof) and locks onto the enemy
/// it finds, or turns to a known contact in range, and follows it.
#[derive(Deserialize, Clone, Debug)]
pub struct BinocularsDef {
    pub range: f32,
    /// Full width of the cone (degrees).
    #[serde(default = "binocular_angle")]
    pub angle: f32,
    /// How fast it turns (degrees per second); scanning goes at half that.
    #[serde(default = "binocular_turn")]
    pub turn: f32,
    /// With nothing to look at it scans this far (degrees) to either side of where the unit faces.
    #[serde(default = "binocular_scan")]
    pub scan: f32,
}

/// A unit that carries other units.
#[derive(Deserialize, Clone, Debug)]
pub struct TransportDef {
    /// Cargo slots (a unit takes `cargo` slots, 1 unless its JSON says otherwise).
    pub capacity: u8,
    /// A building infantry occupy: their vision and binocular range times this instead of
    /// `rules.garrison_vision` (a watch tower).
    #[serde(default)]
    pub vision: Option<f32>,
}

/// A field work (sandbags, a trench piece): a structure that is cover rather than a target.
/// Shots fly through it and lose its `cover` share to it, units walk and drive over it, nobody
/// aims at it unless ordered to, and blasts wear it down like anything else.
#[derive(Deserialize, Clone, Debug)]
pub struct FieldworkDef {
    /// Share of a shot's damage it takes instead of what it shelters.
    pub cover: f32,
    /// A trench: the ground of its cell is dug this much below the original ground (and stays so
    /// when the trench is gone). It shelters the infantry standing in it, from every side.
    /// 0 = a wall: it covers whatever enemy shots have to fly through it to reach.
    #[serde(default)]
    pub dig: f32,
}

/// Explosive reactive armor: tiles that go off when a missile (an anti-tank guided missile, say)
/// hits, taking most of its damage. Each hit uses up one charge; a unit repaired to full health
/// gets all of them back.
#[derive(Deserialize, Clone, Copy, Debug)]
pub struct ReactiveDef {
    pub charges: u8,
    /// Share of a missile hit's damage a charge stops (0 - 1).
    pub block: f32,
}

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WeaponKind {
    /// Flat ballistic shot, needs line of fire.
    Direct,
    /// Low ballistic arc.
    Arc,
    /// High ballistic arc (mortars, howitzers).
    HighArc,
    Missile,
    Bomb,
}

#[derive(Deserialize, Clone, Debug)]
pub struct TurretDef {
    /// Degrees per second.
    pub yaw_speed: f32,
    pub pitch_speed: f32,
    #[serde(default)]
    pub yaw_bone: Option<String>,
    #[serde(default)]
    pub pitch_bone: Option<String>,
    #[serde(default)]
    pub muzzle_bone: Option<String>,
}

fn land_only() -> Vec<String> {
    vec!["land".into()]
}
fn salvo_delay() -> f32 {
    0.1
}
fn spread() -> f32 {
    0.3
}
fn normal() -> String {
    "Normal".into()
}

#[derive(Deserialize, Clone, Debug)]
pub struct WeaponDef {
    pub name: String,
    /// Sound of a shot, one of `sim::SOUNDS` ("cannon", "mg", "artillery", "missile", "bomb"); empty = by weapon kind.
    #[serde(default)]
    pub sound: String,
    pub damage: f32,
    #[serde(default = "normal")]
    pub damage_type: String,
    /// Blast radius, 0 = direct hits only. The unit hit directly takes all the damage, others
    /// around `splash` of it at the centre, fading to none at the edge.
    #[serde(default)]
    pub radius: f32,
    /// Share of the damage the blast does to units not hit directly (at its centre). None = by
    /// kind: `rules.splash_direct` for direct-fire guns, all of it for artillery, bombs and missiles.
    #[serde(default)]
    pub splash: Option<f32>,
    #[serde(skip)]
    pub splash_share: f32,
    pub range: f32,
    #[serde(default)]
    pub min_range: f32,
    pub reload: f32,
    /// Muzzle velocity.
    pub speed: f32,
    pub kind: WeaponKind,
    #[serde(default = "one_u8")]
    pub salvo: u8,
    #[serde(default = "salvo_delay")]
    pub salvo_delay: f32,
    /// Random dispersion in degrees.
    #[serde(default = "spread")]
    pub spread: f32,
    /// Seconds the gun must stay on target before it fires.
    #[serde(default)]
    pub aim_time: f32,
    /// Seconds the unit stays put after each shot (mortars). A weapon with a hold also fires
    /// only while its unit stands still, so a moving order is never frozen by a passing target,
    /// and its `aim_time` only counts from the moment the unit stands still.
    #[serde(default)]
    pub hold: f32,
    /// Rounds the weapon carries (each projectile counts, a salvo of 4 uses 4); it falls silent
    /// when they are spent. 0 = unlimited.
    #[serde(default)]
    pub ammo: u16,
    /// Seconds to rearm one spent round (SupCom tactical missile style: one at a time, paid as a
    /// flow of `rearm_mass` over that time, slowed by a mass stall). 0 = never rearmed.
    #[serde(default)]
    pub rearm: f32,
    /// Mass one rearmed round costs.
    #[serde(default)]
    pub rearm_mass: f32,
    /// None = fixed forward weapon (the whole unit has to face the target).
    #[serde(default)]
    pub turret: Option<TurretDef>,
    #[serde(default)]
    pub muzzle_bone: Option<String>,
    #[serde(default = "land_only")]
    pub targets: Vec<String>,
    /// Missile turn rate, degrees per second.
    #[serde(default)]
    pub turn_rate: f32,
    /// Missiles: launch elevation in degrees (90 = straight up, like a cold-launched SAM); 0 = a low 20 degrees.
    #[serde(default)]
    pub launch_pitch: f32,
    /// Sniper rifle: a hit on a building with enemy infantry inside harms one of them and leaves
    /// the building alone; the weapon never shoots at buildings without enemy riders.
    #[serde(default)]
    pub snipe: bool,
    /// Suicide drone: firing launches the unit itself at the target as a homing missile (a
    /// straight dive from where it flies) and removes it, without a wreck or death blast.
    #[serde(default)]
    pub suicide: bool,
    /// Target priority: "armored" shoots units of category `armored` (tanks) first, "infantry" shoots
    /// infantry first; otherwise (or when none is in range) the nearest enemy.
    #[serde(default)]
    pub prefer: String,
    #[serde(skip)]
    pub dtype: usize,
    #[serde(skip)]
    pub hits_land: bool,
    #[serde(skip)]
    pub hits_air: bool,
}

impl WeaponDef {
    /// A machine gun: small-arms direct fire with no set-up hold. Its unit fires on the move.
    pub fn is_machine_gun(&self) -> bool {
        self.damage_type == "SmallArms" && self.kind == WeaponKind::Direct && self.hold <= 0.0 && !self.snipe
    }
}

#[derive(Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum ColorSpec {
    /// "team", "light" (pale team colour), "dark", "metal".
    Name(String),
    Rgb([f32; 3]),
}

#[derive(Deserialize, Clone, Debug)]
pub struct BoxDef {
    pub pos: [f32; 3],
    pub size: [f32; 3],
    pub color: ColorSpec,
    /// Degrees per second around the vertical axis.
    #[serde(default)]
    pub spin: Option<f32>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct SpinDef {
    pub bone: String,
    pub axis: [f32; 3],
    pub speed: f32,
}

/// Either a glTF mesh (with optional textures) or a list of coloured boxes.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct ModelDef {
    #[serde(default)]
    pub mesh: Option<String>,
    #[serde(default)]
    pub albedo: Option<String>,
    #[serde(default)]
    pub team_mask: Option<String>,
    #[serde(default)]
    pub normal: Option<String>,
    #[serde(default = "one")]
    pub scale: f32,
    /// Multiplier on the albedo texture's colours. The mod's textures are dark (default 2.1);
    /// textures painted for this game (`tools/blend.py`) use 1.
    #[serde(default = "albedo_brightness")]
    pub brightness: f32,
    #[serde(default)]
    pub spin: Vec<SpinDef>,
    #[serde(default)]
    pub boxes: Vec<BoxDef>,
}

/// Strategic icon: a base shape plus overlaid glyphs; the tier adds pips underneath.
/// Every unit must have a unique combination (checked at load).
#[derive(Deserialize, Clone, Debug, PartialEq, Eq, Hash)]
pub struct IconDef {
    pub shape: String,
    #[serde(default)]
    pub glyphs: Vec<String>,
    /// Leave the tier ticks out (storage buildings and the like, as in SupCom).
    #[serde(default)]
    pub no_tier: bool,
}

#[derive(Deserialize, Clone, Debug)]
pub struct UnitDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub desc: String,
    #[serde(default)]
    pub tier: u8,
    pub hp: f32,
    /// Hit points it heals by itself per second (troopers), up to `hp`. 0 = none.
    #[serde(default)]
    pub regen: f32,
    pub armor: String,
    pub cost: Cost,
    /// Collision box: width (x), height (y), length (z).
    pub size: [f32; 3],
    /// What shots and blasts hit: a box turning with the unit, width (x), height (y), length (z),
    /// standing on the unit's base and centred on it, measured from the model (gun barrels left out).
    /// Absent = `size`.
    #[serde(default)]
    pub hit_box: Option<[f32; 3]>,
    /// Structures: side length in grid cells.
    #[serde(default)]
    pub footprint: i32,
    #[serde(default, rename = "move")]
    pub mv: Move,
    #[serde(default)]
    pub vision: f32,
    /// Seconds an enemy team must have it in view before it counts as seen, while it keeps still
    /// and holds its fire (0 = at once). Closer than `rules.near_sight` it is seen at once.
    #[serde(default)]
    pub spot_time: f32,
    /// The same while it moves or has fired within its reload time.
    #[serde(default)]
    pub spot_time_moving: f32,
    /// A long, narrow view cone on top of `vision` (infantry spotters).
    #[serde(default)]
    pub binoculars: Option<BinocularsDef>,
    /// Radar range against unstealthed units (radar level 1).
    #[serde(default)]
    pub radar: f32,
    /// Level 2 radar range: also finds stealth level 1 units.
    #[serde(default)]
    pub radar2: f32,
    /// Level 3 radar range: also finds stealth level 2 units.
    #[serde(default)]
    pub radar3: f32,
    /// Radar stealth level: shows only on radar of a higher level (within that level's range).
    /// 3 = never on radar (infantry); it has to be seen.
    #[serde(default)]
    pub stealth: u8,
    /// Old form of `stealth: 3` (kept so older presets still load).
    #[serde(default)]
    pub radar_stealth: bool,
    #[serde(default)]
    pub eco: Eco,
    #[serde(default)]
    pub build: Option<BuildCap>,
    /// Build category ("land", "air", "structure"...). None = cannot be built by anyone.
    #[serde(default)]
    pub built_by: Option<String>,
    #[serde(default = "yes")]
    pub buildable: bool,
    #[serde(default)]
    pub needs_deposit: bool,
    #[serde(default)]
    pub upgrade: Option<String>,
    #[serde(default)]
    pub weapons: Vec<WeaponDef>,
    pub model: ModelDef,
    pub icon: IconDef,
    #[serde(default)]
    pub portrait: Option<String>,
    #[serde(default)]
    pub death: Option<Blast>,
    #[serde(default = "yes")]
    pub wreck: bool,
    #[serde(default)]
    pub categories: Vec<String>,
    /// Carries land units (transport plane, APC).
    #[serde(default)]
    pub transport: Option<TransportDef>,
    /// Slots this unit takes in a transport; 0 = cannot be carried.
    #[serde(default = "one_u8")]
    pub cargo: u8,
    /// Infantry: close-quarters damage per second when storming an occupied building or holding
    /// one against those storming it. 0 = cannot storm buildings.
    #[serde(default)]
    pub assault: f32,
    /// Sandbags, trenches: cover that is built (see `FieldworkDef`).
    #[serde(default)]
    pub fieldwork: Option<FieldworkDef>,
    /// Explosive reactive armor (see `ReactiveDef`).
    #[serde(default)]
    pub reactive: Option<ReactiveDef>,
    /// Sound played when the unit is selected (one of `view::sound::UI_SOUNDS`); empty = by role.
    #[serde(default)]
    pub select_sound: String,

    #[serde(skip)]
    pub idx: DefId,
    #[serde(skip)]
    pub faction: u8,
    #[serde(skip)]
    pub upgrade_id: Option<DefId>,
    #[serde(skip)]
    pub armor_id: usize,
    /// What this unit can build, resolved from categories and tiers.
    #[serde(skip)]
    pub builds: Vec<DefId>,
}

impl UnitDef {
    pub fn is_structure(&self) -> bool {
        self.mv.kind == MoveKind::None
    }
    pub fn is_air(&self) -> bool {
        matches!(self.mv.kind, MoveKind::Plane | MoveKind::Hover)
    }
    pub fn is_plane(&self) -> bool {
        self.mv.kind == MoveKind::Plane
    }
    pub fn is_factory(&self) -> bool {
        self.is_structure() && self.build.as_ref().map_or(false, |b| !b.categories.is_empty())
    }
    pub fn is_station(&self) -> bool {
        self.build.as_ref().map_or(false, |b| b.station)
    }
    /// Mobile constructor (engineer, commander).
    pub fn is_engineer(&self) -> bool {
        !self.is_structure() && self.build.as_ref().map_or(false, |b| !b.categories.is_empty())
    }
    pub fn is_scavenger(&self) -> bool {
        self.build.as_ref().map_or(false, |b| b.scavenger)
    }
    pub fn can_assist(&self) -> bool {
        self.is_engineer() || self.is_station()
    }
    pub fn build_rate(&self) -> f32 {
        self.build.as_ref().map_or(0.0, |b| b.rate)
    }
    pub fn build_range(&self) -> f32 {
        self.build.as_ref().map_or(0.0, |b| b.range)
    }
    pub fn radius(&self) -> f32 {
        if self.footprint > 0 {
            self.footprint as f32 * crate::map::CELL * 0.5
        } else {
            self.size[0].max(self.size[2]) * 0.5
        }
    }
    pub fn height(&self) -> f32 {
        self.size[1]
    }
    /// The box shots and blasts hit: width, height, length (`hit_box`, else `size`).
    pub fn hit_box(&self) -> [f32; 3] {
        self.hit_box.unwrap_or(self.size)
    }
    /// How far its radar finds a unit of this stealth level (0 = none): the largest range among
    /// the radar levels above it.
    pub fn radar_vs(&self, stealth: u8) -> f32 {
        let r = [self.radar, self.radar2, self.radar3];
        r.iter().skip(stealth as usize).fold(0.0, |a, b| a.max(*b))
    }
    /// Its furthest radar range (against unstealthed units).
    pub fn radar_reach(&self) -> f32 {
        self.radar_vs(0)
    }
    pub fn has_radar(&self) -> bool {
        self.radar_reach() > 0.0
    }
    /// The highest radar level it has (0 = no radar).
    pub fn radar_level(&self) -> u8 {
        [self.radar, self.radar2, self.radar3].iter().rposition(|r| *r > 0.0).map_or(0, |i| i as u8 + 1)
    }
    pub fn armed(&self) -> bool {
        !self.weapons.is_empty()
    }
    pub fn max_range(&self) -> f32 {
        self.weapons.iter().map(|w| w.range).fold(0.0, f32::max)
    }
    pub fn has_category(&self, c: &str) -> bool {
        self.categories.iter().any(|x| x == c)
    }
    /// Foot soldiers (category "infantry"): they take cover and can occupy buildings.
    pub fn is_infantry(&self) -> bool {
        self.has_category("infantry")
    }
    /// Tanks and other heavy vehicles (category "armored"): what cannons go for first.
    pub fn is_armored(&self) -> bool {
        self.has_category("armored")
    }
    /// A building infantry can occupy and fire from (a structure with a `transport` block).
    pub fn is_garrison(&self) -> bool {
        self.is_structure() && self.transport.is_some()
    }
    /// How much further infantry inside this building see (vision and binoculars).
    pub fn roof_vision(&self, rules: &Rules) -> f32 {
        self.transport.as_ref().and_then(|t| t.vision).filter(|v| *v > 0.0).unwrap_or(rules.garrison_vision)
    }
    /// Built cover (sandbags, a trench piece), see `FieldworkDef`.
    pub fn is_fieldwork(&self) -> bool {
        self.fieldwork.is_some() && self.is_structure()
    }
    /// Cover share and dug depth of a field work.
    pub fn work(&self) -> Option<(f32, f32)> {
        self.fieldwork.as_ref().filter(|_| self.is_structure()).map(|f| (f.cover.clamp(0.0, 1.0), f.dig.max(0.0)))
    }
}

#[derive(Deserialize)]
struct FactionFile {
    id: String,
    name: String,
    #[serde(default)]
    playable: bool,
    #[serde(default)]
    commander: Option<String>,
    units: Vec<UnitDef>,
}

#[derive(Clone, Debug)]
pub struct Faction {
    pub id: String,
    pub name: String,
    pub playable: bool,
    pub commander: Option<DefId>,
    pub units: Vec<DefId>,
}

/// The text of every data file of one data set (the built-in data or a preset). This is what
/// `Defs` is built from, what a preset is on disk, and what the host sends to the other players.
#[derive(Clone, Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct DataPack {
    /// Name of the preset; empty for the built-in data.
    pub name: String,
    pub rules: String,
    /// (file name, text) of every faction file, in file-name order.
    pub factions: Vec<(String, String)>,
    /// Asset files the data refers to that the standard game does not have: (path below `assets`, content).
    #[serde(default)]
    pub files: Vec<(String, Vec<u8>)>,
}

/// Folder below `assets/data` that holds the presets shipped with the game, one sub-folder each.
pub fn presets_dir(root: &Path) -> PathBuf {
    root.join("data/presets")
}

/// The player's own presets (made in the preset editor, or kept from a host's game), in the user
/// folder so they survive a new version. A preset here hides a shipped one of the same name.
pub fn user_presets_dir() -> PathBuf {
    crate::settings::base_dir().join("presets")
}

/// The player's own maps (premade maps kept from a host's game), in the user folder.
pub fn user_maps_dir() -> PathBuf {
    crate::settings::base_dir().join("maps")
}

/// Folder of a premade map: the player's own copy if there is one, else the one shipped in `assets/maps`.
pub fn map_dir(name: &str) -> PathBuf {
    let own = user_maps_dir().join(name);
    if own.join("map.json").exists() { own } else { asset_root().join("maps").join(name) }
}

/// A name that is safe as a folder name.
pub fn clean_name(name: &str) -> String {
    name.trim().chars().filter(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_')).take(32).collect::<String>().trim().to_string()
}

/// A path below `assets` that a data pack may bring along: model and picture files only, in the
/// folders the game keeps them in, with plain names (no `..`, nothing hidden, nothing absolute).
pub fn safe_asset_path(p: &str) -> bool {
    let parts: Vec<&str> = p.split('/').collect();
    let ext = p.rsplit('.').next().unwrap_or("");
    parts.len() >= 2
        && ["models", "icons"].contains(&parts[0])
        && ["glb", "png"].contains(&ext)
        && parts.iter().all(|s| !s.is_empty() && !s.starts_with('.') && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.')))
}

impl DataPack {
    /// Reads `rules.json` and `factions/*.json` from a data folder.
    pub fn read(dir: &Path, name: &str) -> Result<DataPack, String> {
        let rules_path = dir.join("rules.json");
        let rules = std::fs::read_to_string(&rules_path).map_err(|e| format!("{}: {e}", rules_path.display()))?;
        let fdir = dir.join("factions");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&fdir)
            .map_err(|e| format!("{}: {e}", fdir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map_or(false, |x| x == "json"))
            .collect();
        files.sort();
        let mut factions = vec![];
        for f in files {
            let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            factions.push((f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(), text));
        }
        Ok(DataPack { name: name.to_string(), rules, factions, files: vec![] })
    }

    /// The built-in data (`assets/data`).
    pub fn builtin(root: &Path) -> Result<DataPack, String> {
        DataPack::read(&root.join("data"), "")
    }

    /// A preset by name; the empty name is the built-in data.
    pub fn preset(root: &Path, name: &str) -> Result<DataPack, String> {
        if name.is_empty() {
            DataPack::builtin(root)
        } else {
            let own = user_presets_dir().join(name);
            DataPack::read(&if own.join("rules.json").exists() { own } else { presets_dir(root).join(name) }, name)
        }
    }

    /// Whether a preset of that name exists, the player's own or a shipped one.
    pub fn exists(root: &Path, name: &str) -> bool {
        user_presets_dir().join(name).join("rules.json").exists() || presets_dir(root).join(name).join("rules.json").exists()
    }

    /// Names of the presets on disk (the player's own and the shipped ones).
    pub fn list(root: &Path) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for dir in [user_presets_dir(), presets_dir(root)] {
            if let Ok(rd) = std::fs::read_dir(dir) {
                out.extend(rd.filter_map(|e| e.ok()).filter(|e| e.path().join("rules.json").exists()).filter_map(|e| e.file_name().into_string().ok()));
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// Writes the pack as preset `self.name` in the user folder (replacing what is there).
    pub fn save(&self) -> Result<(), String> {
        let name = clean_name(&self.name);
        if name.is_empty() {
            return Err("A preset needs a name (letters, digits, spaces, - and _).".into());
        }
        let dir = user_presets_dir().join(&name);
        let fdir = dir.join("factions");
        std::fs::create_dir_all(&fdir).map_err(|e| format!("{}: {e}", fdir.display()))?;
        std::fs::write(dir.join("rules.json"), &self.rules).map_err(|e| e.to_string())?;
        // Faction files that are no longer part of the pack go away.
        if let Ok(rd) = std::fs::read_dir(&fdir) {
            for e in rd.filter_map(|e| e.ok()) {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.ends_with(".json") && !self.factions.iter().any(|f| f.0 == n) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        for (n, text) in &self.factions {
            if !n.ends_with(".json") || n.contains(['/', '\\']) {
                return Err(format!("bad faction file name '{n}'"));
            }
            std::fs::write(fdir.join(n), text).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn delete(root: &Path, name: &str) -> Result<(), String> {
        let name = clean_name(name);
        if name.is_empty() {
            return Err("The built-in data cannot be deleted.".into());
        }
        // The player's own copy goes first (a shipped preset of that name shows again); a shipped one can be deleted too.
        let own = user_presets_dir().join(&name);
        let dir = if own.exists() { own } else { presets_dir(root).join(&name) };
        std::fs::remove_dir_all(dir).map_err(|e| e.to_string())
    }

    /// Adds the asset files this data needs that `standard` (the built-in data) does not refer to,
    /// so players without them can still show the units. Big or odd files are left out.
    pub fn attach_assets(&mut self, root: &Path, defs: &Defs, standard: &Defs) {
        let known = standard.asset_paths();
        let mut total = 0usize;
        self.files.clear();
        for p in defs.asset_paths() {
            if known.contains(&p) || !safe_asset_path(&p) || self.files.iter().any(|f| f.0 == p) {
                continue;
            }
            if let Ok(bytes) = std::fs::read(root.join(&p)) {
                if bytes.len() <= 8 << 20 && total + bytes.len() <= 40 << 20 {
                    total += bytes.len();
                    self.files.push((p, bytes));
                }
            }
        }
    }

    /// Stores the asset files that came with the pack where the game looks for them. Files that
    /// already exist are never replaced. Returns how many were written.
    pub fn store_assets(&self, root: &Path) -> usize {
        let mut n = 0;
        for (p, bytes) in &self.files {
            let path = root.join(p);
            if !safe_asset_path(p) || path.exists() || bytes.len() > 8 << 20 {
                continue;
            }
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if std::fs::write(&path, bytes).is_ok() {
                n += 1;
            }
        }
        n
    }
}

#[derive(Resource, Clone)]
pub struct Defs {
    pub rules: Rules,
    pub units: Vec<UnitDef>,
    pub by_name: HashMap<String, DefId>,
    pub factions: Vec<Faction>,
    /// Feature type id -> index into `rules.features`.
    pub feature_by_name: HashMap<String, u16>,
    /// Hash of all data files; host and clients must match.
    pub hash: u64,
    /// The files this was built from.
    pub pack: Arc<DataPack>,
}

fn fnv(h: &mut u64, bytes: &[u8]) {
    for b in bytes {
        *h ^= *b as u64;
        *h = h.wrapping_mul(0x100000001b3);
    }
}

impl Defs {
    pub fn get(&self, id: DefId) -> &UnitDef {
        &self.units[id as usize]
    }
    pub fn id(&self, name: &str) -> Option<DefId> {
        self.by_name.get(name).copied()
    }
    /// Feature type by id.
    pub fn feature_id(&self, name: &str) -> Option<u16> {
        self.feature_by_name.get(name).copied()
    }
    pub fn armor_mult(&self, dtype: usize, armor: usize) -> f32 {
        self.rules.armor_table.get(armor).and_then(|r| r.get(dtype)).copied().unwrap_or(1.0)
    }
    pub fn playable_factions(&self) -> Vec<usize> {
        (0..self.factions.len()).filter(|i| self.factions[*i].playable).collect()
    }

    /// Every asset file the units refer to (models, textures, portraits), as paths below `assets`.
    pub fn asset_paths(&self) -> Vec<String> {
        let mut out: Vec<String> = vec![];
        for u in &self.units {
            for p in [&u.model.mesh, &u.model.albedo, &u.model.team_mask, &u.model.normal, &u.portrait].into_iter().flatten() {
                if !out.contains(p) {
                    out.push(p.clone());
                }
            }
        }
        out
    }

    /// The built-in game data in `assets/data`.
    pub fn load(root: &Path) -> Result<Defs, String> {
        Defs::from_pack(DataPack::builtin(root)?)
    }

    /// A preset by name (empty = built-in).
    pub fn load_preset(root: &Path, name: &str) -> Result<Defs, String> {
        Defs::from_pack(DataPack::preset(root, name)?)
    }

    pub fn from_pack(pack: DataPack) -> Result<Defs, String> {
        let mut hash = 0xcbf29ce484222325u64;
        let text = &pack.rules;
        fnv(&mut hash, text.as_bytes());
        let mut rules: Rules = serde_json::from_str(text).map_err(|e| format!("rules.json: {e}"))?;
        let mut armor_names: Vec<String> = rules.armor.keys().cloned().collect();
        armor_names.sort();
        rules.armor_table = armor_names
            .iter()
            .map(|a| rules.damage_types.iter().map(|d| rules.armor[a].get(d).copied().unwrap_or(1.0)).collect())
            .collect();
        rules.armor_names = armor_names;
        if rules.player_colors.is_empty() || rules.damage_types.is_empty() {
            return Err("rules.json: player_colors and damage_types must not be empty".into());
        }
        let mut feature_by_name = HashMap::new();
        for (k, f) in rules.features.iter().enumerate() {
            if feature_by_name.insert(f.id.clone(), k as u16).is_some() {
                return Err(format!("rules.json: duplicate feature id '{}'", f.id));
            }
            if !(f.mass >= 0.0 && f.energy >= 0.0 && f.size > 0.0 && f.weight >= 0.0) {
                return Err(format!("rules.json: feature '{}' needs mass, energy and weight >= 0 and size > 0", f.id));
            }
        }
        if rules.features.len() >= u16::MAX as usize {
            return Err("rules.json: too many features".into());
        }

        let mut units: Vec<UnitDef> = vec![];
        let mut factions = vec![];
        let mut commanders = vec![];
        if pack.factions.is_empty() {
            return Err("no faction files".into());
        }
        for (fname, text) in &pack.factions {
            fnv(&mut hash, text.as_bytes());
            let ff: FactionFile = serde_json::from_str(text).map_err(|e| format!("{fname}: {e}"))?;
            let fi = factions.len() as u8;
            let mut ids = vec![];
            for mut u in ff.units {
                u.idx = units.len() as DefId;
                u.faction = fi;
                ids.push(u.idx);
                units.push(u);
            }
            commanders.push(ff.commander);
            factions.push(Faction { id: ff.id, name: ff.name, playable: ff.playable, commander: None, units: ids });
        }
        let mut by_name = HashMap::new();
        for u in &units {
            if by_name.insert(u.id.clone(), u.idx).is_some() {
                return Err(format!("duplicate unit id '{}'", u.id));
            }
        }
        for (fi, c) in commanders.into_iter().enumerate() {
            if let Some(c) = c {
                factions[fi].commander = Some(*by_name.get(&c).ok_or(format!("unknown commander '{c}'"))?);
            }
        }
        // Resolve references.
        let snapshot = units.clone();
        let mut icons: HashMap<(IconDef, u8), String> = HashMap::new();
        for u in units.iter_mut() {
            u.armor_id = rules.armor_names.iter().position(|a| *a == u.armor).ok_or(format!("{}: unknown armor '{}'", u.id, u.armor))?;
            if let Some(up) = &u.upgrade {
                u.upgrade_id = Some(*by_name.get(up).ok_or(format!("{}: unknown upgrade '{up}'", u.id))?);
            }
            for w in u.weapons.iter_mut() {
                w.dtype = rules.damage_types.iter().position(|d| *d == w.damage_type).ok_or(format!("{}: unknown damage type '{}'", u.id, w.damage_type))?;
                w.hits_land = w.targets.iter().any(|t| t == "land");
                w.hits_air = w.targets.iter().any(|t| t == "air");
                w.splash_share = w.splash.unwrap_or(if w.kind == WeaponKind::Direct { rules.splash_direct } else { 1.0 }).max(0.0);
            }
            if let Some(b) = &u.build {
                u.builds = snapshot
                    .iter()
                    .filter(|o| o.faction == u.faction && o.buildable && o.tier <= b.tier)
                    .filter(|o| o.built_by.as_ref().map_or(false, |c| b.categories.contains(c)))
                    .map(|o| o.idx)
                    .collect();
            }
            if u.radar_stealth && u.stealth == 0 {
                u.stealth = 3;
            }
            if !(u.hp > 0.0 && u.cost.time > 0.0) {
                return Err(format!("{}: hp and cost.time must be above 0", u.id));
            }
            if u.hit_box().iter().any(|x| !(*x > 0.0)) {
                return Err(format!("{}: hit_box (or size, without one) must be above 0 in every direction", u.id));
            }
            // An upgrade happens in place: the occupancy grid and the unit's components stay.
            if let Some(t) = u.upgrade_id.map(|t| &snapshot[t as usize]) {
                if !u.is_structure() || !t.is_structure() || t.footprint != u.footprint {
                    return Err(format!("{}: upgrade '{}' must be a structure with the same footprint", u.id, t.id));
                }
            }
            if u.stealth > 3 {
                return Err(format!("{}: stealth goes from 0 to 3", u.id));
            }
            if u.reactive.is_some_and(|r| !(0.0..=1.0).contains(&r.block)) {
                return Err(format!("{}: reactive armor block goes from 0 to 1", u.id));
            }
            if u.mv.amphibious && u.mv.kind != MoveKind::Land {
                return Err(format!("{}: only land units can be amphibious", u.id));
            }
            if !u.is_structure() && u.footprint != 0 {
                return Err(format!("{}: mobile units must not have a footprint", u.id));
            }
            if u.is_structure() && u.footprint <= 0 {
                return Err(format!("{}: structures need a footprint", u.id));
            }
            // Playable units need telling apart; map buildings (non-playable factions) may share
            // an icon by kind (homes, civic buildings, industry).
            let playable_unit = factions.get(u.faction as usize).map_or(true, |f: &Faction| f.playable);
            if playable_unit {
                if let Some(other) = icons.insert((u.icon.clone(), u.tier), u.id.clone()) {
                    return Err(format!("units '{}' and '{}' share the same strategic icon; every unit needs a unique one", other, u.id));
                }
            }
        }
        if units.len() >= DefId::MAX as usize {
            return Err("too many units".into());
        }
        // A side is defeated with its commander (and `Resign` kills it): every playable faction needs one.
        if let Some(f) = factions.iter().find(|f: &&Faction| f.playable && f.commander.is_none()) {
            return Err(format!("playable faction '{}' has no commander", f.id));
        }
        if !factions.iter().any(|f: &Faction| f.playable) {
            return Err("no playable faction".into());
        }
        Ok(Defs { rules, units, by_name, factions, feature_by_name, hash, pack: Arc::new(pack) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every preset shipped in `assets/data/presets` loads and passes validation.
    #[test]
    fn presets_load() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets");
        Defs::load(&root).expect("built-in data");
        let mut n = 0;
        for e in std::fs::read_dir(presets_dir(&root)).expect("presets folder") {
            let name = e.unwrap().file_name().to_string_lossy().into_owned();
            Defs::load_preset(&root, &name).unwrap_or_else(|err| panic!("preset {name}: {err}"));
            n += 1;
        }
        assert!(n > 0);
    }
}
