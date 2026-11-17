//! The preset editor's form view: game data edited with checkboxes, number fields, drop-down lists
//! and colour buttons instead of raw JSON. Every field is described once below (JSON path, label,
//! kind with its default, tooltip). The form reads and writes a `serde_json::Value`, so keys it does
//! not know (model files, turret bones, comments) stay as they are; the JSON view edits everything.
use crate::data::DataPack;
use crate::i18n::tr;
use bevy_egui::egui;
use serde_json::{json, Map, Value};

/// Lists of names that come from the data itself.
#[derive(Clone, Copy)]
pub enum Src {
    Armor,
    DamageType,
    Unit,
    BuildCat,
    Category,
    Sound,
    UiSound,
}

#[derive(Clone, Copy)]
pub enum Kind {
    Bool(bool),
    /// Default, min, max.
    Num(f64, f64, f64),
    /// Absent = worked out by the game; the checkbox sets a value of its own (min, max, value when set).
    OptNum(f64, f64, f64),
    Int(i64, i64, i64),
    Text,
    /// Options, default.
    Choice(&'static [&'static str], &'static str),
    /// One name from the data; true = may be left empty ("none" removes the key).
    Pick(Src, bool),
    /// A list of strings from a fixed set, one checkbox each (default when absent).
    Tags(&'static [&'static str], &'static [&'static str]),
    TagsFrom(Src),
    /// Three numbers (x, y, z).
    Vec3,
    Color([f32; 3]),
}

pub struct Field {
    pub path: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    pub tip: &'static str,
}

const fn f(path: &'static str, label: &'static str, kind: Kind, tip: &'static str) -> Field {
    Field { path, label, kind, tip }
}
const fn num(path: &'static str, label: &'static str, default: f64, max: f64, tip: &'static str) -> Field {
    Field { path, label, kind: Kind::Num(default, 0.0, max), tip }
}
const fn yes(path: &'static str, label: &'static str, tip: &'static str) -> Field {
    Field { path, label, kind: Kind::Bool(true), tip }
}
const fn no(path: &'static str, label: &'static str, tip: &'static str) -> Field {
    Field { path, label, kind: Kind::Bool(false), tip }
}

const BIG: f64 = 1.0e7;

// ---------------------------------------------------------------------------------------------
// rules.json

pub const RULE_SWITCHES: &[Field] = &[
    yes("terraforming", tx!("Terraforming"), tx!("Engineers and commanders can level, raise and lower the ground (the Terraform button). Off: the button is gone and terraform orders are ignored.")),
    yes("craters", tx!("Craters"), tx!("Heavy hits and death blasts dig shell holes into the ground (their size is set under Craters and terraforming). Off: shelling never changes the ground.")),
    yes("wrecks", tx!("Wrecks (debris)"), tx!("Destroyed units leave wrecks: they hold mass to reclaim, give cover against shots and block sight. Off: destroyed units leave nothing behind. A unit's own 'Leaves a wreck' setting still applies when this is on.")),
    yes("sight_blocked_by_terrain", tx!("Hills block sight"), tx!("A unit only sees what it has a clear line to over the terrain. Off: units see everything within their vision range, hills or not.")),
    yes("sight_blocked_by_reclaimables", tx!("Trees, rocks and wrecks block sight"), tx!("Standing trees, rocks and wrecks block the view like small hills. Off: sight passes through them (they still soak up shots as cover).")),
    yes("sight_blocked_by_buildings", tx!("Buildings block sight"), tx!("Finished buildings (yours, enemy and map buildings) block the view behind them up to their height. Infantry inside a building still look out from its roof, and a building never hides itself. Off: sight passes through buildings.")),
    yes("radar_blocked_by_terrain", tx!("Hills block radar"), tx!("Radar needs a clear line over the terrain, looking from 'Radar eye height' above the unit. Off: radar sees through hills.")),
    yes("ghosts_show_destroyed", tx!("Dim destroyed remembered buildings"), tx!("An enemy building you have seen and then lost sight of stays on your map as you last saw it. When it is destroyed while nobody of your team looks, its model turns dim (as in SupCom), so you know it is gone without scouting. Off: it looks intact until you see the empty spot.")),
    yes("friendly_fire", tx!("Friendly fire"), tx!("Shots and blasts also hurt your own and allied units, not just enemies.")),
];

pub const RULE_ECONOMY: &[Field] = &[
    num("start_mass", tx!("Starting mass"), 650.0, BIG, tx!("Mass every player has at the start.")),
    num("start_energy", tx!("Starting energy"), 4000.0, BIG, tx!("Energy every player has at the start.")),
    num("reclaim_rate", tx!("Reclaim speed"), 2.0, 1000.0, tx!("Mass reclaimed per second for every point of an engineer's build power.")),
    num("reclaim_energy_factor", tx!("Reclaim energy speed-up"), 5.0, 1000.0, tx!("Reclaiming trees and rocks: energy comes this many times faster than mass.")),
    num("repair_rate", tx!("Repair speed"), 3.0, 1000.0, tx!("Hit points repaired per second for every point of build power.")),
    num("power_stall_pump_factor", tx!("Pump output when out of energy"), 0.25, 1.0, tx!("When energy runs out, mass pumps produce at least this share of their output.")),
    num("capture_time", tx!("Capture time"), 1.0, 100.0, tx!("Capturing a unit takes its build time times this, at the engineer's build power.")),
    num("capture_energy", tx!("Capture energy cost"), 0.3, 10.0, tx!("Share of the target's energy cost that capturing it uses up.")),
];

pub const RULE_COMBAT: &[Field] = &[
    num("gravity", tx!("Gravity"), 4.9, 100.0, tx!("Pulls every shell and bomb down (units per second squared). Higher: shorter, steeper arcs and less reach for artillery.")),
    num("commander_death.damage", tx!("Commander death blast: damage"), 1500.0, BIG, tx!("Damage of the explosion when a commander dies.")),
    num("commander_death.radius", tx!("Commander death blast: radius"), 20.0, 1000.0, tx!("Radius of the explosion when a commander dies.")),
    num("armor_front", tx!("Armor angle: front"), 0.6, 10.0, tx!("Armored units (category armored) take this share of a hit head on. Between front, side and rear it blends with the angle; shells falling steeply count the angle less, one falling straight down not at all.")),
    num("armor_side", tx!("Armor angle: side"), 1.0, 10.0, tx!("Share of a hit an armored unit takes from the side.")),
    num("armor_rear", tx!("Armor angle: rear"), 1.5, 10.0, tx!("Share of a hit an armored unit takes from behind.")),
];

pub const RULE_INTEL: &[Field] = &[
    num("near_sight", tx!("Close-range sight"), 6.0, 100.0, tx!("Everything closer than this to a unit is seen at once, whatever stands in between.")),
    num("spot_forget", tx!("Spotting progress lost after (s)"), 3.0, 600.0, tx!("Units with a time to spot: how long out of the enemy's view until he has to start spotting them from nothing again.")),
    num("vision_los_height", tx!("Eye height"), 1.5, 100.0, tx!("Units look from this high above their top; a higher eye sees over more hills and trees.")),
    num("radar_los_height", tx!("Radar eye height"), 4.0, 100.0, tx!("Radar looks from this high above the unit's top (when hills block radar).")),
    num("radar_fade_secs", tx!("Radar blip memory (s)"), 10.0, 600.0, tx!("How long the blip of a moving unit stays on the map after radar has lost it.")),
];

pub const RULE_WRECKS: &[Field] = &[
    num("wreck_mass_fraction", tx!("Wreck mass"), 0.6, 10.0, tx!("A wreck holds this share of the unit's mass cost (a building taken apart gives the same).")),
    f("max_wrecks", tx!("Most wrecks on the map"), Kind::Int(400, 0, 100000), tx!("At most this many wrecks lie on the map at once; the oldest disappear first.")),
    num("wreck_hp", tx!("Wreck hit points"), 0.5, 100.0, tx!("A wreck's hit points as a share of the unit's. Shells and blasts wear wrecks down; the mass goes with the hit points.")),
    num("wreck_cover", tx!("Wreck cover"), 0.5, 1.0, tx!("Share of a shot's damage a wreck soaks up when the shot passes through it.")),
];

pub const RULE_COVER: &[Field] = &[
    num("cover_grace", tx!("Cover grace distance"), 2.5, 100.0, tx!("Trees, rocks and wrecks closer than this to the shooter give no cover from its shots (units fire out from behind their own cover).")),
    num("cover_search", tx!("Infantry cover search"), 10.0, 500.0, tx!("Infantry look for cover within this distance of where they were sent.")),
    num("cover_threat_range", tx!("Infantry threat range"), 100.0, 2000.0, tx!("Infantry take cover from known armed enemies up to this far away.")),
    num("garrison_eject_damage", tx!("Garrison eject damage"), 0.4, 1.0, tx!("Infantry thrown out of a destroyed building lose this share of their hit points.")),
    num("garrison_damage", tx!("Garrison hit share"), 0.04, 1.0, tx!("Infantry inside a building take this share of every hit on it (their own armor applies).")),
    num("garrison_vision", tx!("Roof vision"), 1.3, 10.0, tx!("Infantry inside a building look out from its roof: their vision and binocular range times this.")),
    num("garrison_defense", tx!("Garrison defence bonus"), 1.25, 10.0, tx!("Infantry holding a building deal this many times their close-quarters (assault) damage to infantry storming it.")),
];

pub const RULE_TERRAIN: &[Field] = &[
    num("crater_scale", tx!("Crater depth"), 0.025, 10.0, tx!("Crater depth = this x the square root of the damage. 0 = no craters.")),
    num("crater_max_depth", tx!("Deepest crater"), 2.0, 100.0, tx!("Shelling never digs deeper than this below the original ground.")),
    num("crater_min_damage", tx!("Least damage for a crater"), 35.0, BIG, tx!("Hits weaker than this leave no mark on the ground.")),
    num("terraform_rate", tx!("Terraform speed"), 0.6, 100.0, tx!("Ground height moved per second for every point of build power.")),
    num("terraform_energy", tx!("Terraform energy"), 15.0, 10000.0, tx!("Energy per unit of ground height moved.")),
    num("terraform_max", tx!("Terraform limit"), 12.0, 500.0, tx!("Terraforming stays within this height of the original ground.")),
];

pub const RULE_POINTS: &[Field] = &[
    num("cp_radius", tx!("Capture radius"), 12.0, 500.0, tx!("Control points mode: units within this distance of a point take it.")),
    num("cp_capture_secs", tx!("Capture time (s)"), 20.0, 3600.0, tx!("Seconds a team needs to take a control point.")),
    num("cp_start_points", tx!("Starting points"), 1200.0, BIG, tx!("Points every team starts with (the host can pick another number in the lobby). A team at 0 is defeated.")),
    num("cp_bleed", tx!("Bleed per second"), 1.0, 1000.0, tx!("A team holding fewer control points than its strongest opponent loses this many points per second for each point of difference.")),
];

const FEATURE_CLASSES: &[&str] = &["tree", "rock"];
const FEATURE_SHAPES: &[&str] = &["pine", "broadleaf", "bush", "rock"];

pub const FEATURE_FIELDS: &[Field] = &[
    f("id", tx!("Id"), Kind::Text, tx!("Name maps use for this type. Changing it breaks maps that place it.")),
    f("name", tx!("Name"), Kind::Text, tx!("Shown in the game.")),
    f("class", tx!("Class"), Kind::Choice(FEATURE_CLASSES, "tree"), tx!("Where the map generator puts it: trees in woods, rocks in clusters.")),
    f("shape", tx!("Shape"), Kind::Choice(FEATURE_SHAPES, "pine"), tx!("How it is drawn; also its height, footprint and default cover.")),
    num("mass", tx!("Mass"), 0.0, BIG, tx!("Mass an engineer gets by reclaiming it.")),
    num("energy", tx!("Energy"), 0.0, BIG, tx!("Energy an engineer gets by reclaiming it.")),
    num("size", tx!("Size"), 1.0, 20.0, tx!("Scale of the model (1 = a pine about 3 units tall, a rock about 1 unit across).")),
    num("hp", tx!("Hit points"), 0.0, BIG, tx!("Weapon damage that destroys it (times the map's scale for it). 0 = a default by shape and size.")),
    f("cover", tx!("Cover"), Kind::OptNum(0.0, 1.0, 0.3), tx!("Share of a shot's damage it soaks up when the shot passes through it. Unticked: by shape (pine 0.3, broadleaf 0.35, bush 0.15, rock 0.65).")),
    num("weight", tx!("Generator weight"), 1.0, 1000.0, tx!("How often the map generator picks it among the features of its class.")),
    f("color", tx!("Colour"), Kind::Color([0.2, 0.4, 0.2]), tx!("Foliage or stone colour.")),
    f("trunk", tx!("Trunk colour"), Kind::Color([0.33, 0.24, 0.16]), tx!("Colour of the trunk (trees).")),
];

// ---------------------------------------------------------------------------------------------
// units

const MOVE_KINDS: &[&str] = &["none", "land", "hover", "plane"];
const SHAPES: &[&str] = &["land", "bot", "air", "structure", "factory", "commander", "civilian"];
const GLYPHS: &[&str] = &["plus", "dot", "arc", "chevron", "ring", "wave", "eng", "mass", "bolt", "store", "cargo", "frame", "bar", "land", "tri"];
const WEAPON_KINDS: &[&str] = &["direct", "arc", "high_arc", "missile", "bomb"];
const TARGETS: &[&str] = &["land", "air"];
const PRIORITIES: &[&str] = &["none", "armored", "infantry"];

pub const UNIT_GENERAL: &[Field] = &[
    f("id", tx!("Id"), Kind::Text, tx!("Unique name the data refers to the unit by (upgrades, commanders, map props). Rename it everywhere if you change it.")),
    f("name", tx!("Name"), Kind::Text, tx!("Shown in the game.")),
    f("desc", tx!("Description"), Kind::Text, tx!("Shown in the build menu tooltip.")),
    f("tier", tx!("Tier"), Kind::Int(0, 0, 9), tx!("Tech level: a builder of tier N can build units up to tier N. Also the ticks under the strategic icon.")),
    num("hp", tx!("Hit points"), 100.0, BIG, tx!("Damage the unit takes before it is destroyed.")),
    f("armor", tx!("Armor"), Kind::Pick(Src::Armor, false), tx!("Armor class: decides how much each damage type hurts it (Armor table in rules.json).")),
    f("categories", tx!("Categories"), Kind::TagsFrom(Src::Category), tx!("Special roles. infantry: takes cover behind trees, rocks and wrecks and can occupy buildings. armored: what cannons (target priority armored) shoot first.")),
    f("built_by", tx!("Built by"), Kind::Pick(Src::BuildCat, true), tx!("Build category: builders and factories that build this category can make it. none = nobody builds it.")),
    yes("buildable", tx!("Buildable"), tx!("Off: it never shows in a build menu (map buildings, units that only come from upgrades).")),
    no("needs_deposit", tx!("Needs a mass deposit"), tx!("Can only be placed on a mass deposit (extractors).")),
    f("upgrade", tx!("Upgrades to"), Kind::Pick(Src::Unit, true), tx!("The unit this one turns into when upgraded in place (factories, pumps).")),
    yes("wreck", tx!("Leaves a wreck"), tx!("Leaves a wreck when destroyed (aircraft never do, and the Wrecks switch in rules.json turns all of them off).")),
    f("cargo", tx!("Cargo slots"), Kind::Int(1, 0, 100), tx!("Slots it takes in a transport; 0 = cannot be carried.")),
    num("assault", tx!("Close-quarters damage"), 0.0, 1000.0, tx!("Infantry: damage per second in the fight for an occupied building, storming it (ordered into an enemy-held building) or holding it. 0 = cannot storm buildings.")),
    f("select_sound", tx!("Selection sound"), Kind::Pick(Src::UiSound, true), tx!("Sound played when it is selected; none = chosen by role.")),
];

pub const UNIT_COST: &[Field] = &[
    num("cost.mass", tx!("Mass"), 0.0, BIG, tx!("Mass it costs to build.")),
    num("cost.energy", tx!("Energy"), 0.0, BIG, tx!("Energy it costs to build.")),
    num("cost.time", tx!("Build time"), 1.0, BIG, tx!("Build work: seconds it takes at 1 build power (an engineer with build power 10 needs a tenth of it).")),
];

pub const UNIT_BODY: &[Field] = &[
    f("size", tx!("Size (width, height, length)"), Kind::Vec3, tx!("Collision box: shots hit it (unless a hit radius / height is set below) and units bump into it. Height also sets where it looks from.")),
    f("hit_radius", tx!("Hit radius"), Kind::OptNum(0.05, 100.0, 0.5), tx!("Radius of what shots and blasts hit. Unset = half the size (structures: the footprint).")),
    f("hit_height", tx!("Hit height"), Kind::OptNum(0.05, 100.0, 1.0), tx!("Height of what shots and blasts hit. Unset = the size's height.")),
    f("footprint", tx!("Footprint (cells)"), Kind::Int(0, 0, 64), tx!("Structures: side length on the build grid (1 cell = 2 units). Mobile units: 0.")),
    f("move.kind", tx!("Movement"), Kind::Choice(MOVE_KINDS, "none"), tx!("none = a structure; land drives, hover floats over water, plane flies.")),
    num("move.speed", tx!("Speed"), 0.0, 1000.0, tx!("Top speed forward (units per second).")),
    num("move.reverse", tx!("Reverse speed"), 0.0, 1000.0, tx!("Top speed backwards; 0 = it never reverses.")),
    num("move.accel", tx!("Acceleration"), 1.0, 1000.0, tx!("Speed gained or lost per second.")),
    num("move.turn", tx!("Turn rate (deg/s)"), 0.0, 3600.0, tx!("How fast it turns on the spot.")),
    num("move.altitude", tx!("Flying height"), 0.0, 1000.0, tx!("Aircraft: height above the ground.")),
];

pub const UNIT_INTEL: &[Field] = &[
    num("vision", tx!("Vision range"), 0.0, 5000.0, tx!("How far it sees (hills, trees and rocks may block it, see the rules switches).")),
    num("spot_time", tx!("Time to spot, hiding (s)"), 0.0, 600.0, tx!("How long an enemy must have it in view before it is seen, while it keeps still and holds its fire. 0 = at once (tanks, buildings).")),
    num("spot_time_moving", tx!("Time to spot, moving (s)"), 0.0, 600.0, tx!("The same while it moves or has just fired. 0 = at once.")),
    num("radar", tx!("Radar range"), 0.0, 5000.0, tx!("How far its radar reaches; radar shows contacts but not what they are. This level finds only units without stealth.")),
    num("radar2", tx!("T2 radar range"), 0.0, 5000.0, tx!("How far its level 2 radar reaches: also finds stealth 1 units. A unit is found within the largest range of the levels above its stealth.")),
    num("radar3", tx!("T3 radar range"), 0.0, 5000.0, tx!("How far its level 3 radar reaches: also finds stealth 2 units.")),
    f("stealth", tx!("Radar stealth"), Kind::Int(0, 0, 3), tx!("0 = shows on any radar; 1 = only T2 or T3 radar finds it; 2 = only T3 radar; 3 = never on radar (it has to be seen).")),
];

pub const UNIT_ECO: &[Field] = &[
    num("eco.mass", tx!("Mass income"), 0.0, BIG, tx!("Mass it produces per second.")),
    num("eco.energy", tx!("Energy income"), 0.0, BIG, tx!("Energy it produces per second.")),
    num("eco.upkeep", tx!("Energy upkeep"), 0.0, BIG, tx!("Energy it uses per second while working (radars, pumps).")),
    num("eco.store_mass", tx!("Mass storage"), 0.0, BIG, tx!("How much mass it adds to its owner's storage.")),
    num("eco.store_energy", tx!("Energy storage"), 0.0, BIG, tx!("How much energy it adds to its owner's storage.")),
];

pub const UNIT_BUILD: &[Field] = &[
    num("rate", tx!("Build power"), 10.0, 10000.0, tx!("Build work per second; also the speed of reclaim, repair, capture and terraforming.")),
    num("range", tx!("Build range"), 0.0, 1000.0, tx!("How far away it can build, reclaim and repair.")),
    f("categories", tx!("Builds"), Kind::TagsFrom(Src::BuildCat), tx!("Build categories it can make (units whose 'Built by' is one of these). None ticked: it only assists, reclaims and repairs.")),
    f("tier", tx!("Highest tier"), Kind::Int(1, 0, 9), tx!("It builds units up to this tier.")),
    no("station", tx!("Engineering station"), tx!("Immobile builder that assists anything in range on its own.")),
    no("scavenger", tx!("Scavenger"), tx!("Cannot build or assist; reclaims wrecks and revives them as its owner's units.")),
];

pub const UNIT_BINOCULARS: &[Field] = &[
    num("range", tx!("Range"), 140.0, 5000.0, tx!("How far it sees inside the cone (hills, trees and rocks still block it).")),
    num("angle", tx!("Cone width (deg)"), 20.0, 170.0, tx!("Full width of the view cone.")),
    num("turn", tx!("Turn rate (deg/s)"), 40.0, 3600.0, tx!("How fast the binoculars follow a target; scanning goes at half that.")),
    num("scan", tx!("Scan arc (deg)"), 60.0, 180.0, tx!("With nothing to look at it scans this far to either side of where the unit faces; 180 = all round (always so from a building's roof).")),
];

pub const UNIT_TRANSPORT: &[Field] = &[
    f("capacity", tx!("Capacity (slots)"), Kind::Int(4, 1, 255), tx!("Cargo slots it carries. On a structure: a building infantry can occupy and fire from.")),
    num("vision", tx!("Roof vision"), 0.0, 10.0, tx!("A building infantry occupy: their vision and binocular range times this (a watch tower). 0 or absent: the rule 'Roof vision'.")),
];

pub const UNIT_FIELDWORK: &[Field] = &[
    num("cover", tx!("Cover"), 0.5, 1.0, tx!("Share of a shot's damage it takes instead of what it shelters (0 - 1).")),
    num("dig", tx!("Trench depth"), 0.0, 1.4, tx!("Above 0 it is a trench: the ground of its cell is dug this deep (and stays so when the trench is destroyed) and it shelters the infantry standing in it from every side. 0: a wall (sandbags) that covers what enemy shots must fly through it to reach.")),
];

pub const UNIT_REACTIVE: &[Field] = &[
    num("charges", tx!("Charges"), 4.0, 50.0, tx!("Missile hits it can take the edge off. Each hit uses one; a full repair replaces them all.")),
    num("block", tx!("Block"), 0.75, 1.0, tx!("Share of a missile hit's damage a charge stops (0 - 1).")),
];

pub const UNIT_DEATH: &[Field] = &[
    num("damage", tx!("Damage"), 100.0, BIG, tx!("Damage of the explosion when it is destroyed.")),
    num("radius", tx!("Radius"), 5.0, 1000.0, tx!("Radius of that explosion. Full damage at the centre, fading to nothing at the edge.")),
];

pub const UNIT_ICON: &[Field] = &[
    f("icon.shape", tx!("Shape"), Kind::Choice(SHAPES, "land"), tx!("Base shape of the strategic icon: land (diamond), bot, air (triangle), structure (square), factory, commander, civilian.")),
    f("icon.glyphs", tx!("Glyphs"), Kind::Tags(GLYPHS, &[]), tx!("Symbols on the icon: plus = direct fire, dot = artillery, arc = anti-air, chevron = missile, ring = scout, wave = radar, eng = builder, mass, bolt = energy, store = storage, cargo = transport. Playable units need an icon (with the tier) no other unit has.")),
    no("icon.no_tier", tx!("No tier ticks"), tx!("Leave the tier ticks off the icon (storages and the like).")),
    num("model.scale", tx!("Model scale"), 1.0, 1000.0, tx!("Size of the 3D model (model files are set in the JSON view).")),
];

pub const WEAPON_FIELDS: &[Field] = &[
    f("name", tx!("Name"), Kind::Text, tx!("Shown in the unit's info panel.")),
    f("kind", tx!("Kind"), Kind::Choice(WEAPON_KINDS, "direct"), tx!("direct: flat shot, needs a clear line of fire and leads its target. arc / high_arc: ballistic, fires over hills. missile: homing. bomb: dropped from a plane.")),
    num("damage", tx!("Damage"), 10.0, BIG, tx!("Damage of one shell.")),
    f("damage_type", tx!("Damage type"), Kind::Pick(Src::DamageType, false), tx!("Decides the armor multiplier (Armor table in rules.json).")),
    num("radius", tx!("Blast radius"), 0.0, 1000.0, tx!("0 = it only hurts what it hits directly. Otherwise full damage at the centre, fading to nothing at the edge.")),
    num("range", tx!("Range"), 50.0, 10000.0, tx!("Furthest target distance. Units must see or radar their target to shoot it.")),
    num("min_range", tx!("Least range"), 0.0, 10000.0, tx!("It cannot fire at targets closer than this.")),
    num("reload", tx!("Reload (s)"), 1.0, 3600.0, tx!("Seconds between shots (or salvos).")),
    num("speed", tx!("Muzzle velocity"), 50.0, 10000.0, tx!("Shell speed; slow shells are easier to dodge, artillery needs enough of it to reach.")),
    f("salvo", tx!("Shots per salvo"), Kind::Int(1, 1, 255), tx!("Shells fired one after another each time it fires.")),
    num("salvo_delay", tx!("Salvo delay (s)"), 0.1, 100.0, tx!("Seconds between the shells of a salvo.")),
    num("spread", tx!("Spread (deg)"), 0.3, 90.0, tx!("Random dispersion of each shot.")),
    num("aim_time", tx!("Aim time (s)"), 0.0, 100.0, tx!("Seconds the gun must stay on target before it fires.")),
    num("hold", tx!("Hold after firing (s)"), 0.0, 100.0, tx!("Seconds the unit cannot move after each shot; it also fires only while standing still and starts its aim time only then (mortars). 0 = fires on the move.")),
    num("ammo", tx!("Ammunition"), 0.0, 1000.0, tx!("Rounds the weapon carries (every shot of a salvo counts). When they are spent it stops firing until a round is rearmed. 0 = unlimited.")),
    num("rearm", tx!("Rearm time (s)"), 0.0, 3600.0, tx!("Limited ammunition: seconds to rearm one spent round, one at a time (like SupCom tactical missiles). 0 = never rearmed.")),
    num("rearm_mass", tx!("Rearm mass"), 0.0, 10000.0, tx!("Limited ammunition: mass one rearmed round costs, paid as a flow over the rearm time (a mass stall slows it).")),
    f("targets", tx!("Targets"), Kind::Tags(TARGETS, &["land"]), tx!("What it can shoot at: land (ground units and buildings), air.")),
    num("turn_rate", tx!("Missile turn rate (deg/s)"), 0.0, 3600.0, tx!("Missiles: how fast they steer toward their target.")),
    num("launch_pitch", tx!("Missile launch angle (deg)"), 0.0, 90.0, tx!("Missiles: launch elevation (90 = straight up, like a SAM); 0 = a low 20 degrees.")),
    f("prefer", tx!("Target priority"), Kind::Choice(PRIORITIES, "none"), tx!("Which enemies it shoots first when several are in range: armored (units of category armored: tanks) or infantry. none = the nearest.")),
    no("snipe", tx!("Sniper rifle"), tx!("A hit on a building kills one enemy rider inside instead of damaging the building; it never shoots buildings without enemy riders.")),
    f("sound", tx!("Sound"), Kind::Pick(Src::Sound, true), tx!("Shot sound; none = chosen by weapon kind.")),
];

pub const TURRET_FIELDS: &[Field] = &[
    num("yaw_speed", tx!("Traverse (deg/s)"), 90.0, 3600.0, tx!("How fast the turret turns sideways.")),
    num("pitch_speed", tx!("Elevation (deg/s)"), 90.0, 3600.0, tx!("How fast the barrel moves up and down.")),
];

// ---------------------------------------------------------------------------------------------

/// Names the drop-down lists offer, gathered from the data being edited.
#[derive(Default, Clone)]
pub struct FormCtx {
    pub armor: Vec<String>,
    pub damage: Vec<String>,
    pub units: Vec<String>,
    pub build_cats: Vec<String>,
    pub categories: Vec<String>,
}

impl FormCtx {
    pub fn from_pack(pack: &DataPack) -> FormCtx {
        let mut c = FormCtx::default();
        let rules: Value = serde_json::from_str(&pack.rules).unwrap_or_default();
        if let Some(a) = rules["armor"].as_object() {
            c.armor = a.keys().cloned().collect();
            c.armor.sort();
        }
        c.damage = rules["damage_types"].as_array().map(|a| a.iter().filter_map(|d| d.as_str().map(String::from)).collect()).unwrap_or_default();
        let add = |list: &mut Vec<String>, s: &str| {
            if !s.is_empty() && !list.iter().any(|x| x == s) {
                list.push(s.to_string());
            }
        };
        for cat in ["land", "air", "structure"] {
            add(&mut c.build_cats, cat);
        }
        add(&mut c.categories, "infantry");
        add(&mut c.categories, "armored");
        for (_, text) in &pack.factions {
            let v: Value = serde_json::from_str(text).unwrap_or_default();
            for u in v["units"].as_array().into_iter().flatten() {
                add(&mut c.units, u["id"].as_str().unwrap_or(""));
                add(&mut c.build_cats, u["built_by"].as_str().unwrap_or(""));
                for s in u["build"]["categories"].as_array().into_iter().flatten() {
                    add(&mut c.build_cats, s.as_str().unwrap_or(""));
                }
                for s in u["categories"].as_array().into_iter().flatten() {
                    add(&mut c.categories, s.as_str().unwrap_or(""));
                }
            }
        }
        c.units.sort();
        c
    }
    fn list(&self, s: Src) -> Vec<String> {
        match s {
            Src::Armor => self.armor.clone(),
            Src::DamageType => self.damage.clone(),
            Src::Unit => self.units.clone(),
            Src::BuildCat => self.build_cats.clone(),
            Src::Category => self.categories.clone(),
            Src::Sound => crate::sim::SOUNDS.iter().map(|s| s.to_string()).collect(),
            Src::UiSound => crate::view::sound::UI_SOUNDS.iter().map(|s| s.to_string()).collect(),
        }
    }
}

fn get<'a>(v: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = v;
    for k in path.split('.') {
        cur = cur.get(k)?;
    }
    Some(cur)
}

/// Writes (or with `None` removes) the value at a dotted path, making the objects on the way.
fn set(v: &mut Value, path: &str, x: Option<Value>) {
    let mut cur = v;
    let mut parts = path.split('.').peekable();
    while let Some(k) = parts.next() {
        if !cur.is_object() {
            *cur = Value::Object(Map::new());
        }
        let obj = cur.as_object_mut().unwrap();
        if parts.peek().is_none() {
            match x {
                Some(x) => {
                    obj.insert(k.to_string(), x);
                }
                None => {
                    obj.remove(k);
                }
            }
            return;
        }
        cur = obj.entry(k.to_string()).or_insert_with(|| Value::Object(Map::new()));
    }
}

/// A number the way it reads best in JSON: whole numbers without a fraction, the rest rounded.
fn number(x: f64) -> Value {
    let r = (x * 10000.0).round() / 10000.0;
    if r.fract() == 0.0 && r.abs() < 1.0e15 {
        json!(r as i64)
    } else {
        json!(r)
    }
}

fn drag(x: &mut f64, min: f64, max: f64) -> egui::DragValue<'_> {
    let speed = (x.abs() * 0.01).max(if max <= 1.0 { 0.005 } else { 0.05 });
    egui::DragValue::new(x).range(min..=max).speed(speed).max_decimals(4)
}

/// The widget of one field. Returns true when it changed the value.
fn widget(ui: &mut egui::Ui, v: &mut Value, fd: &Field, ctx: &FormCtx, salt: &str) -> bool {
    let cur = get(v, fd.path).cloned();
    let id = egui::Id::new(("form", salt, fd.path));
    let mut out: Option<Option<Value>> = None;
    let tip = tr(fd.tip);
    match fd.kind {
        Kind::Bool(d) => {
            let mut b = cur.as_ref().and_then(|x| x.as_bool()).unwrap_or(d);
            if ui.checkbox(&mut b, "").on_hover_text(tip).changed() {
                out = Some(Some(json!(b)));
            }
        }
        Kind::Num(d, min, max) => {
            let mut x = cur.as_ref().and_then(|x| x.as_f64()).unwrap_or(d);
            if ui.add(drag(&mut x, min, max)).on_hover_text(tip).changed() {
                out = Some(Some(number(x)));
            }
        }
        Kind::OptNum(min, max, d) => {
            let mut on = cur.as_ref().map_or(false, |x| x.is_number());
            let mut x = cur.as_ref().and_then(|x| x.as_f64()).unwrap_or(d);
            ui.horizontal(|ui| {
                if ui.checkbox(&mut on, tr!("own value")).on_hover_text(tip).changed() {
                    out = Some(on.then(|| number(x)));
                }
                if on && ui.add(drag(&mut x, min, max)).on_hover_text(tip).changed() {
                    out = Some(Some(number(x)));
                }
                if !on {
                    ui.label(egui::RichText::new(tr!("default")).weak());
                }
            });
        }
        Kind::Int(d, min, max) => {
            let mut x = cur.as_ref().and_then(|x| x.as_i64()).unwrap_or(d);
            if ui.add(egui::DragValue::new(&mut x).range(min..=max).speed(0.1)).on_hover_text(tip).changed() {
                out = Some(Some(json!(x)));
            }
        }
        Kind::Text => {
            let mut s = cur.as_ref().and_then(|x| x.as_str()).unwrap_or("").to_string();
            if ui.add(egui::TextEdit::singleline(&mut s).desired_width(260.0)).on_hover_text(tip).changed() {
                out = Some(Some(json!(s)));
            }
        }
        Kind::Choice(opts, d) => {
            let s = cur.as_ref().and_then(|x| x.as_str()).unwrap_or(d).to_string();
            egui::ComboBox::from_id_salt(id).selected_text(&s).width(180.0).show_ui(ui, |ui| {
                for o in opts {
                    if ui.selectable_label(s == *o, *o).clicked() && s != *o {
                        out = Some(Some(json!(o)));
                    }
                }
            })
            .response
            .on_hover_text(tip);
        }
        Kind::Pick(src, optional) => {
            let s = cur.as_ref().and_then(|x| x.as_str()).unwrap_or("").to_string();
            let mut opts = ctx.list(src);
            if !s.is_empty() && !opts.contains(&s) {
                opts.insert(0, s.clone());
            }
            let shown = if s.is_empty() { tr!("none").to_string() } else { s.clone() };
            egui::ComboBox::from_id_salt(id).selected_text(shown).width(180.0).show_ui(ui, |ui| {
                if optional && ui.selectable_label(s.is_empty(), tr!("none")).clicked() && !s.is_empty() {
                    out = Some(None);
                }
                for o in &opts {
                    if ui.selectable_label(s == *o, o.as_str()).clicked() && s != *o {
                        out = Some(Some(json!(o)));
                    }
                }
            })
            .response
            .on_hover_text(tip);
        }
        Kind::Tags(..) | Kind::TagsFrom(_) => {
            let (opts, d): (Vec<String>, Vec<String>) = match fd.kind {
                Kind::Tags(o, d) => (o.iter().map(|s| s.to_string()).collect(), d.iter().map(|s| s.to_string()).collect()),
                Kind::TagsFrom(src) => (ctx.list(src), vec![]),
                _ => unreachable!(),
            };
            let mut have: Vec<String> = cur.as_ref().and_then(|x| x.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(String::from)).collect()).unwrap_or(d);
            let mut all = opts;
            for h in &have {
                if !all.contains(h) {
                    all.push(h.clone());
                }
            }
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(420.0);
                for o in &all {
                    let mut on = have.contains(o);
                    if ui.checkbox(&mut on, o.as_str()).on_hover_text(tip).changed() {
                        if on {
                            have.push(o.clone());
                        } else {
                            have.retain(|h| h != o);
                        }
                        out = Some(Some(json!(have.clone())));
                    }
                }
            });
        }
        Kind::Vec3 => {
            let mut xs = [0.0f64; 3];
            if let Some(a) = cur.as_ref().and_then(|x| x.as_array()) {
                for (k, x) in a.iter().take(3).enumerate() {
                    xs[k] = x.as_f64().unwrap_or(0.0);
                }
            }
            ui.horizontal(|ui| {
                let mut changed = false;
                for x in xs.iter_mut() {
                    changed |= ui.add(drag(x, 0.0, 1000.0)).on_hover_text(tip).changed();
                }
                if changed {
                    out = Some(Some(Value::Array(xs.iter().map(|x| number(*x)).collect())));
                }
            });
        }
        Kind::Color(d) => {
            let mut c = d;
            if let Some(a) = cur.as_ref().and_then(|x| x.as_array()) {
                for (k, x) in a.iter().take(3).enumerate() {
                    c[k] = x.as_f64().unwrap_or(0.0) as f32;
                }
            }
            if ui.color_edit_button_rgb(&mut c).on_hover_text(tip).changed() {
                out = Some(Some(Value::Array(c.iter().map(|x| number(*x as f64)).collect())));
            }
        }
    }
    match out {
        Some(x) => {
            set(v, fd.path, x);
            true
        }
        None => false,
    }
}

/// A two-column grid: label (hover for the explanation) and widget.
pub fn grid(ui: &mut egui::Ui, v: &mut Value, fields: &[Field], ctx: &FormCtx, salt: &str) -> bool {
    let mut changed = false;
    egui::Grid::new(("form_grid", salt)).num_columns(2).spacing([16.0, 6.0]).min_col_width(230.0).striped(true).show(ui, |ui| {
        for fd in fields {
            ui.label(tr(fd.label)).on_hover_text(tr(fd.tip));
            changed |= widget(ui, v, fd, ctx, salt);
            ui.end_row();
        }
    });
    changed
}

fn section(ui: &mut egui::Ui, title: &str, open: bool, body: impl FnOnce(&mut egui::Ui)) {
    egui::CollapsingHeader::new(egui::RichText::new(title).strong().size(15.0)).default_open(open).show(ui, body);
}

/// An optional block of fields (`build`, `transport`, `death`, a weapon's `turret`): a checkbox adds
/// it with `fresh` or removes it.
fn optional(ui: &mut egui::Ui, v: &mut Value, key: &str, label: &str, tip: &str, fresh: Value, fields: &[Field], ctx: &FormCtx, salt: &str) -> bool {
    let mut on = v.get(key).map_or(false, |x| x.is_object());
    let mut changed = false;
    if ui.checkbox(&mut on, label).on_hover_text(tip).changed() {
        if on {
            set(v, key, Some(fresh));
        } else {
            set(v, key, None);
        }
        changed = true;
    }
    if let Some(block) = v.get_mut(key).filter(|b| b.is_object()) {
        ui.indent(("opt", salt, key), |ui| {
            changed |= grid(ui, block, fields, ctx, &format!("{salt}.{key}"));
        });
    }
    changed
}

// ---------------------------------------------------------------------------------------------

/// The rules.json form. Returns true when something changed.
pub fn rules_form(ui: &mut egui::Ui, v: &mut Value, ctx: &FormCtx) -> bool {
    let mut ch = false;
    section(ui, tr!("Switches"), true, |ui| {
        ui.label(egui::RichText::new(tr!("Turn whole game features on or off. Hover over a name for what it does.")).weak());
        ch |= grid(ui, v, RULE_SWITCHES, ctx, "sw");
    });
    section(ui, tr!("Economy"), true, |ui| ch |= grid(ui, v, RULE_ECONOMY, ctx, "eco"));
    section(ui, tr!("Combat"), false, |ui| ch |= grid(ui, v, RULE_COMBAT, ctx, "combat"));
    section(ui, tr!("Sight and radar"), false, |ui| ch |= grid(ui, v, RULE_INTEL, ctx, "intel"));
    section(ui, tr!("Wrecks"), false, |ui| ch |= grid(ui, v, RULE_WRECKS, ctx, "wrecks"));
    section(ui, tr!("Cover and infantry"), false, |ui| ch |= grid(ui, v, RULE_COVER, ctx, "cover"));
    section(ui, tr!("Craters and terraforming"), false, |ui| ch |= grid(ui, v, RULE_TERRAIN, ctx, "terrain"));
    section(ui, tr!("Control points mode"), false, |ui| ch |= grid(ui, v, RULE_POINTS, ctx, "cp"));
    section(ui, tr!("Armor table"), false, |ui| ch |= armor_table(ui, v));
    section(ui, tr!("Trees and rocks"), false, |ui| ch |= features(ui, v, ctx));
    section(ui, tr!("Colours"), false, |ui| ch |= colours(ui, v));
    ch
}

/// Damage multipliers: one row per armor class, one column per damage type.
fn armor_table(ui: &mut egui::Ui, v: &mut Value) -> bool {
    let types: Vec<String> = v["damage_types"].as_array().map(|a| a.iter().filter_map(|d| d.as_str().map(String::from)).collect()).unwrap_or_default();
    let mut names: Vec<String> = v["armor"].as_object().map(|a| a.keys().cloned().collect()).unwrap_or_default();
    names.sort();
    let mut changed = false;
    ui.label(egui::RichText::new(tr!("Damage a hit does = weapon damage x the number for the target's armor (row) and the weapon's damage type (column). Add or rename armor classes and damage types in the JSON view.")).weak());
    egui::Grid::new("armor_table").striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
        ui.label("");
        for t in &types {
            ui.label(egui::RichText::new(t).strong()).on_hover_text(tr!("Weapons with damage type {}", t));
        }
        ui.end_row();
        for a in &names {
            ui.label(egui::RichText::new(a).strong()).on_hover_text(tr!("Units with armor {}", a));
            for t in &types {
                let mut x = v["armor"][a][t].as_f64().unwrap_or(1.0);
                if ui.add(drag(&mut x, 0.0, 100.0)).on_hover_text(tr!("{} damage against {} armor is multiplied by this (1 = full damage, 0 = none).", t, a)).changed() {
                    v["armor"][a][t] = number(x);
                    changed = true;
                }
            }
            ui.end_row();
        }
    });
    changed
}

fn features(ui: &mut egui::Ui, v: &mut Value, ctx: &FormCtx) -> bool {
    let mut changed = false;
    ui.label(egui::RichText::new(tr!("Reclaimable trees and rocks that maps place. Maps refer to them by id.")).weak());
    if !v["features"].is_array() {
        v["features"] = json!([]);
    }
    let list = v["features"].as_array_mut().unwrap();
    let mut remove = None;
    let mut copy = None;
    for (k, ft) in list.iter_mut().enumerate() {
        let title = format!("{}  ({})", ft["name"].as_str().unwrap_or("?"), ft["id"].as_str().unwrap_or("?"));
        egui::CollapsingHeader::new(title).id_salt(("feature", k)).show(ui, |ui| {
            changed |= grid(ui, ft, FEATURE_FIELDS, ctx, &format!("feature{k}"));
            ui.horizontal(|ui| {
                if ui.button(tr!("Copy")).on_hover_text(tr!("Add a copy of this type (give it its own id).")).clicked() {
                    copy = Some(k);
                }
                if ui.button(tr!("Remove")).on_hover_text(tr!("Remove this type. Maps that place it can then not be played with this preset.")).clicked() {
                    remove = Some(k);
                }
            });
        });
    }
    if let Some(k) = copy {
        let mut c = list[k].clone();
        c["id"] = json!(format!("{}_copy", c["id"].as_str().unwrap_or("feature")));
        list.insert(k + 1, c);
        changed = true;
    }
    if let Some(k) = remove {
        list.remove(k);
        changed = true;
    }
    changed
}

fn colours(ui: &mut egui::Ui, v: &mut Value) -> bool {
    let mut changed = false;
    let mut rgb = |ui: &mut egui::Ui, x: &mut Value, tip: &str| {
        let mut c = [0.5f32; 3];
        if let Some(a) = x.as_array() {
            for (k, n) in a.iter().take(3).enumerate() {
                c[k] = n.as_f64().unwrap_or(0.5) as f32;
            }
        }
        if ui.color_edit_button_rgb(&mut c).on_hover_text(tip).changed() {
            *x = Value::Array(c.iter().map(|n| number(*n as f64)).collect());
            changed = true;
        }
    };
    egui::Grid::new("colours").spacing([16.0, 6.0]).show(ui, |ui| {
        ui.label(tr!("Players")).on_hover_text(tr!("Team colours, in slot order (10 for 10 players)."));
        ui.horizontal_wrapped(|ui| {
            for (k, c) in v["player_colors"].as_array_mut().into_iter().flatten().enumerate() {
                rgb(ui, c, &tr!("Player {}", k + 1));
            }
        });
        ui.end_row();
        ui.label(tr!("Neutral")).on_hover_text(tr!("Colour of neutral map buildings."));
        rgb(ui, &mut v["neutral_color"], tr!("Colour of neutral map buildings."));
        ui.end_row();
        ui.label(tr!("Hostile")).on_hover_text(tr!("Colour of the hostile side's units and buildings."));
        rgb(ui, &mut v["hostile_color"], tr!("Colour of the hostile side's units and buildings."));
        ui.end_row();
    });
    changed
}

/// One unit's form. Returns true when something changed.
pub fn unit_form(ui: &mut egui::Ui, v: &mut Value, ctx: &FormCtx) -> bool {
    let mut ch = false;
    section(ui, tr!("General"), true, |ui| ch |= grid(ui, v, UNIT_GENERAL, ctx, "gen"));
    section(ui, tr!("Cost"), true, |ui| ch |= grid(ui, v, UNIT_COST, ctx, "cost"));
    section(ui, tr!("Body and movement"), false, |ui| ch |= grid(ui, v, UNIT_BODY, ctx, "body"));
    section(ui, tr!("Sight and radar"), false, |ui| {
        ch |= grid(ui, v, UNIT_INTEL, ctx, "intel");
        ch |= optional(ui, v, "binoculars", tr!("Binoculars"), tr!("A narrow view cone reaching much further than its vision: it scans around, locks onto an enemy it finds (or turns to a known contact in range) and follows it."), json!({ "range": 140, "angle": 20, "turn": 40, "scan": 60 }), UNIT_BINOCULARS, ctx, "binoculars");
    });
    section(ui, tr!("Economy"), false, |ui| ch |= grid(ui, v, UNIT_ECO, ctx, "eco"));
    section(ui, tr!("Weapons"), true, |ui| ch |= weapons(ui, v, ctx));
    section(ui, tr!("Construction, transport, death blast"), false, |ui| {
        ch |= optional(ui, v, "build", tr!("Can build"), tr!("Engineers, factories and stations: builds, assists, reclaims, repairs."), json!({ "rate": 10, "categories": [] }), UNIT_BUILD, ctx, "build");
        ch |= optional(ui, v, "transport", tr!("Carries units"), tr!("A transport (aircraft, APC) or, on a structure, a building infantry can occupy."), json!({ "capacity": 4 }), UNIT_TRANSPORT, ctx, "transport");
        ch |= optional(ui, v, "fieldwork", tr!("Field work (built cover)"), tr!("Sandbags or a trench piece: shots fly through it and lose its cover share to it, units cross it, nobody aims at it unless ordered to. Blasts and the shots it stops wear it down."), json!({ "cover": 0.5 }), UNIT_FIELDWORK, ctx, "fieldwork");
        ch |= optional(ui, v, "reactive", tr!("Reactive armor"), tr!("Explosive tiles that go off when a missile hits (AT troopers, Robins) and stop most of its damage, one hit per charge."), json!({ "charges": 4, "block": 0.75 }), UNIT_REACTIVE, ctx, "reactive");
        ch |= optional(ui, v, "death", tr!("Explodes when destroyed"), tr!("A blast that hurts everything around it."), json!({ "damage": 100, "radius": 5 }), UNIT_DEATH, ctx, "death");
    });
    section(ui, tr!("Icon and model"), false, |ui| ch |= grid(ui, v, UNIT_ICON, ctx, "icon"));
    ch
}

fn weapons(ui: &mut egui::Ui, v: &mut Value, ctx: &FormCtx) -> bool {
    let mut changed = false;
    if !v["weapons"].is_array() {
        v["weapons"] = json!([]);
    }
    let list = v["weapons"].as_array_mut().unwrap();
    let (mut remove, mut copy) = (None, None);
    for (k, w) in list.iter_mut().enumerate() {
        let title = format!("{}  ({})", w["name"].as_str().unwrap_or("weapon"), w["kind"].as_str().unwrap_or("?"));
        egui::CollapsingHeader::new(title).id_salt(("weapon", k)).default_open(true).show(ui, |ui| {
            let salt = format!("weapon{k}");
            changed |= grid(ui, w, WEAPON_FIELDS, ctx, &salt);
            changed |= optional(ui, w, "turret", tr!("Turret"), tr!("The gun turns on its own; without a turret the whole unit has to face the target. (Model bones are set in the JSON view.)"), json!({ "yaw_speed": 90, "pitch_speed": 90 }), TURRET_FIELDS, ctx, &salt);
            ui.horizontal(|ui| {
                if ui.button(tr!("Copy weapon")).clicked() {
                    copy = Some(k);
                }
                if ui.button(tr!("Remove weapon")).clicked() {
                    remove = Some(k);
                }
            });
        });
    }
    if let Some(k) = copy {
        let c = list[k].clone();
        list.insert(k + 1, c);
        changed = true;
    }
    if let Some(k) = remove {
        list.remove(k);
        changed = true;
    }
    if ui.button(tr!("Add weapon")).on_hover_text(tr!("A new cannon with middling numbers; change it to taste.")).clicked() {
        list.push(json!({ "name": "Cannon", "kind": "direct", "damage": 10, "range": 50, "reload": 1, "speed": 50, "turret": { "yaw_speed": 90, "pitch_speed": 90 } }));
        changed = true;
    }
    changed
}
