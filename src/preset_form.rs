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
    /// Three numbers, or absent = worked out by the game (the checkbox sets 1, 1, 1).
    OptVec3,
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
    yes("terraforming", tx!("preset_rules.switch_terraforming"), tx!("preset_rules.switch_terraforming_tip")),
    yes("craters", tx!("preset_rules.switch_craters"), tx!("preset_rules.switch_craters_tip")),
    yes("wrecks", tx!("preset_rules.switch_wrecks"), tx!("preset_rules.switch_wrecks_tip")),
    yes("sight_blocked_by_terrain", tx!("preset_rules.switch_sight_blocked_by_terrain"), tx!("preset_rules.switch_sight_blocked_by_terrain_tip")),
    yes("sight_blocked_by_reclaimables", tx!("preset_rules.switch_sight_blocked_by_reclaimables"), tx!("preset_rules.switch_sight_blocked_by_reclaimables_tip")),
    yes("sight_blocked_by_buildings", tx!("preset_rules.switch_sight_blocked_by_buildings"), tx!("preset_rules.switch_sight_blocked_by_buildings_tip")),
    yes("radar_blocked_by_terrain", tx!("preset_rules.switch_radar_blocked_by_terrain"), tx!("preset_rules.switch_radar_blocked_by_terrain_tip")),
    yes("counter_battery", tx!("preset_rules.switch_counter_battery"), tx!("preset_rules.switch_counter_battery_tip")),
    yes("ghosts_show_destroyed", tx!("preset_rules.switch_ghosts_show_destroyed"), tx!("preset_rules.switch_ghosts_show_destroyed_tip")),
    yes("ai_reclaim", tx!("preset_rules.switch_ai_reclaim"), tx!("preset_rules.switch_ai_reclaim_tip")),
    yes("friendly_fire", tx!("preset_rules.switch_friendly_fire"), tx!("preset_rules.switch_friendly_fire_tip")),
];

pub const RULE_ECONOMY: &[Field] = &[
    num("start_mass", tx!("preset_rules.eco_start_mass"), 650.0, BIG, tx!("preset_rules.eco_start_mass_tip")),
    num("start_energy", tx!("preset_rules.eco_start_energy"), 4000.0, BIG, tx!("preset_rules.eco_start_energy_tip")),
    num("reclaim_rate", tx!("preset_rules.eco_reclaim_rate"), 2.0, 1000.0, tx!("preset_rules.eco_reclaim_rate_tip")),
    num("reclaim_energy_factor", tx!("preset_rules.eco_reclaim_energy_factor"), 5.0, 1000.0, tx!("preset_rules.eco_reclaim_energy_factor_tip")),
    num("repair_rate", tx!("preset_rules.eco_repair_rate"), 3.0, 1000.0, tx!("preset_rules.eco_repair_rate_tip")),
    num("power_stall_pump_factor", tx!("preset_rules.eco_power_stall_pump_factor"), 0.25, 1.0, tx!("preset_rules.eco_power_stall_pump_factor_tip")),
    num("capture_time", tx!("preset_rules.eco_capture_time"), 1.0, 100.0, tx!("preset_rules.eco_capture_time_tip")),
    num("capture_energy", tx!("preset_rules.eco_capture_energy"), 0.3, 10.0, tx!("preset_rules.eco_capture_energy_tip")),
    num("infantry_capture_secs", tx!("preset_rules.eco_infantry_capture_secs"), 20.0, 1000.0, tx!("preset_rules.eco_infantry_capture_secs_tip")),
    num("infantry_capture_guard", tx!("preset_rules.eco_infantry_capture_guard"), 15.0, 1000.0, tx!("preset_rules.eco_infantry_capture_guard_tip")),
    num("infantry_capture_reach", tx!("preset_rules.eco_infantry_capture_reach"), 12.0, 1000.0, tx!("preset_rules.eco_infantry_capture_reach_tip")),
];

pub const RULE_COMBAT: &[Field] = &[
    num("gravity", tx!("preset_rules.combat_gravity"), 4.9, 100.0, tx!("preset_rules.combat_gravity_tip")),
    num("commander_death.damage", tx!("preset_rules.combat_commander_death_damage"), 1500.0, BIG, tx!("preset_rules.combat_commander_death_damage_tip")),
    num("commander_death.radius", tx!("preset_rules.combat_commander_death_radius"), 20.0, 1000.0, tx!("preset_rules.combat_commander_death_radius_tip")),
    num("armor_front", tx!("preset_rules.combat_armor_front"), 0.6, 10.0, tx!("preset_rules.combat_armor_front_tip")),
    num("armor_side", tx!("preset_rules.combat_armor_side"), 1.0, 10.0, tx!("preset_rules.combat_armor_side_tip")),
    num("armor_rear", tx!("preset_rules.combat_armor_rear"), 1.5, 10.0, tx!("preset_rules.combat_armor_rear_tip")),
    num("splash_direct", tx!("preset_rules.combat_splash_direct"), 0.4, 1.0, tx!("preset_rules.combat_splash_direct_tip")),
    num("splash_armored", tx!("preset_rules.combat_splash_armored"), 0.5, 1.0, tx!("preset_rules.combat_splash_armored_tip")),
];

pub const RULE_INTEL: &[Field] = &[
    num("near_sight", tx!("preset_rules.intel_near_sight"), 6.0, 100.0, tx!("preset_rules.intel_near_sight_tip")),
    num("spot_forget", tx!("preset_rules.intel_spot_forget"), 3.0, 600.0, tx!("preset_rules.intel_spot_forget_tip")),
    num("vision_los_height", tx!("preset_rules.intel_vision_los_height"), 1.5, 100.0, tx!("preset_rules.intel_vision_los_height_tip")),
    num("radar_los_height", tx!("preset_rules.intel_radar_los_height"), 4.0, 100.0, tx!("preset_rules.intel_radar_los_height_tip")),
    num("radar_fade_secs", tx!("preset_rules.intel_radar_fade_secs"), 10.0, 600.0, tx!("preset_rules.intel_radar_fade_secs_tip")),
];

pub const RULE_WRECKS: &[Field] = &[
    num("wreck_mass_fraction", tx!("preset_rules.wrecks_wreck_mass_fraction"), 0.6, 10.0, tx!("preset_rules.wrecks_wreck_mass_fraction_tip")),
    f("max_wrecks", tx!("preset_rules.wrecks_max_wrecks"), Kind::Int(400, 0, 100000), tx!("preset_rules.wrecks_max_wrecks_tip")),
    num("wreck_hp", tx!("preset_rules.wrecks_wreck_hp"), 0.5, 100.0, tx!("preset_rules.wrecks_wreck_hp_tip")),
    num("wreck_cover", tx!("preset_rules.wrecks_wreck_cover"), 0.5, 1.0, tx!("preset_rules.wrecks_wreck_cover_tip")),
];

pub const RULE_COVER: &[Field] = &[
    num("cover_grace", tx!("preset_rules.cover_grace"), 2.5, 100.0, tx!("preset_rules.cover_grace_tip")),
    num("cover_search", tx!("preset_rules.cover_search"), 10.0, 500.0, tx!("preset_rules.cover_search_tip")),
    num("cover_threat_range", tx!("preset_rules.cover_threat_range"), 100.0, 2000.0, tx!("preset_rules.cover_threat_range_tip")),
    num("garrison_eject_damage", tx!("preset_rules.cover_garrison_eject_damage"), 0.4, 1.0, tx!("preset_rules.cover_garrison_eject_damage_tip")),
    num("garrison_damage", tx!("preset_rules.cover_garrison_damage"), 0.04, 1.0, tx!("preset_rules.cover_garrison_damage_tip")),
    num("garrison_vision", tx!("preset_rules.cover_garrison_vision"), 1.3, 10.0, tx!("preset_rules.cover_garrison_vision_tip")),
    num("garrison_defense", tx!("preset_rules.cover_garrison_defense"), 1.25, 10.0, tx!("preset_rules.cover_garrison_defense_tip")),
];

pub const RULE_TERRAIN: &[Field] = &[
    num("crater_scale", tx!("preset_rules.terrain_crater_scale"), 0.025, 10.0, tx!("preset_rules.terrain_crater_scale_tip")),
    num("crater_max_depth", tx!("preset_rules.terrain_crater_max_depth"), 2.0, 100.0, tx!("preset_rules.terrain_crater_max_depth_tip")),
    num("crater_min_damage", tx!("preset_rules.terrain_crater_min_damage"), 35.0, BIG, tx!("preset_rules.terrain_crater_min_damage_tip")),
    num("terraform_rate", tx!("preset_rules.terrain_terraform_rate"), 0.6, 100.0, tx!("preset_rules.terrain_terraform_rate_tip")),
    num("terraform_energy", tx!("preset_rules.terrain_terraform_energy"), 15.0, 10000.0, tx!("preset_rules.terrain_terraform_energy_tip")),
    num("terraform_max", tx!("preset_rules.terrain_terraform_max"), 12.0, 500.0, tx!("preset_rules.terrain_terraform_max_tip")),
];

pub const RULE_POINTS: &[Field] = &[
    num("cp_radius", tx!("preset_rules.points_cp_radius"), 12.0, 500.0, tx!("preset_rules.points_cp_radius_tip")),
    num("cp_capture_secs", tx!("preset_rules.points_cp_capture_secs"), 20.0, 3600.0, tx!("preset_rules.points_cp_capture_secs_tip")),
    num("cp_start_points", tx!("preset_rules.points_cp_start_points"), 10000.0, BIG, tx!("preset_rules.points_cp_start_points_tip")),
    num("cp_bleed", tx!("preset_rules.points_cp_bleed"), 1.0, 1000.0, tx!("preset_rules.points_cp_bleed_tip")),
];

const FEATURE_CLASSES: &[&str] = &["tree", "rock"];
const FEATURE_SHAPES: &[&str] = &["pine", "broadleaf", "bush", "rock"];

pub const FEATURE_FIELDS: &[Field] = &[
    f("id", tx!("preset_rules.feature_id"), Kind::Text, tx!("preset_rules.feature_id_tip")),
    f("name", tx!("preset_rules.feature_name"), Kind::Text, tx!("preset_rules.feature_name_tip")),
    f("class", tx!("preset_rules.feature_class"), Kind::Choice(FEATURE_CLASSES, "tree"), tx!("preset_rules.feature_class_tip")),
    f("shape", tx!("preset_rules.feature_shape"), Kind::Choice(FEATURE_SHAPES, "pine"), tx!("preset_rules.feature_shape_tip")),
    num("mass", tx!("preset_rules.feature_mass"), 0.0, BIG, tx!("preset_rules.feature_mass_tip")),
    num("energy", tx!("preset_rules.feature_energy"), 0.0, BIG, tx!("preset_rules.feature_energy_tip")),
    num("size", tx!("preset_rules.feature_size"), 1.0, 20.0, tx!("preset_rules.feature_size_tip")),
    num("hp", tx!("preset_rules.feature_hp"), 0.0, BIG, tx!("preset_rules.feature_hp_tip")),
    f("cover", tx!("preset_rules.feature_cover"), Kind::OptNum(0.0, 1.0, 0.3), tx!("preset_rules.feature_cover_tip")),
    num("weight", tx!("preset_rules.feature_weight"), 1.0, 1000.0, tx!("preset_rules.feature_weight_tip")),
    f("color", tx!("preset_rules.feature_color"), Kind::Color([0.2, 0.4, 0.2]), tx!("preset_rules.feature_color_tip")),
    f("trunk", tx!("preset_rules.feature_trunk"), Kind::Color([0.33, 0.24, 0.16]), tx!("preset_rules.feature_trunk_tip")),
];

// ---------------------------------------------------------------------------------------------
// units

const MOVE_KINDS: &[&str] = &["none", "land", "hover", "plane"];
const SHAPES: &[&str] = &["land", "bot", "air", "structure", "factory", "commander", "civilian"];
const GLYPHS: &[&str] = &["plus", "dot", "arc", "chevron", "line", "ring", "scope", "wave", "eng", "mass", "bolt", "store", "cargo", "frame", "bar", "land", "tri"];
const WEAPON_KINDS: &[&str] = &["direct", "arc", "high_arc", "missile", "bomb"];
const TARGETS: &[&str] = &["land", "air"];
const PRIORITIES: &[&str] = &["none", "armored", "infantry"];

pub const UNIT_GENERAL: &[Field] = &[
    f("id", tx!("preset_units.general_id"), Kind::Text, tx!("preset_units.general_id_tip")),
    f("name", tx!("preset_units.general_name"), Kind::Text, tx!("preset_units.general_name_tip")),
    f("desc", tx!("preset_units.general_desc"), Kind::Text, tx!("preset_units.general_desc_tip")),
    f("tier", tx!("preset_units.general_tier"), Kind::Int(0, 0, 9), tx!("preset_units.general_tier_tip")),
    num("hp", tx!("preset_units.general_hp"), 100.0, BIG, tx!("preset_units.general_hp_tip")),
    num("regen", tx!("preset_units.general_regen"), 0.0, 1000.0, tx!("preset_units.general_regen_tip")),
    f("armor", tx!("preset_units.general_armor"), Kind::Pick(Src::Armor, false), tx!("preset_units.general_armor_tip")),
    f("categories", tx!("preset_units.general_categories"), Kind::TagsFrom(Src::Category), tx!("preset_units.general_categories_tip")),
    f("built_by", tx!("preset_units.general_built_by"), Kind::Pick(Src::BuildCat, true), tx!("preset_units.general_built_by_tip")),
    yes("buildable", tx!("preset_units.general_buildable"), tx!("preset_units.general_buildable_tip")),
    no("needs_deposit", tx!("preset_units.general_needs_deposit"), tx!("preset_units.general_needs_deposit_tip")),
    f("upgrade", tx!("preset_units.general_upgrade"), Kind::Pick(Src::Unit, true), tx!("preset_units.general_upgrade_tip")),
    yes("wreck", tx!("preset_units.general_wreck"), tx!("preset_units.general_wreck_tip")),
    f("cargo", tx!("preset_units.general_cargo"), Kind::Int(1, 0, 100), tx!("preset_units.general_cargo_tip")),
    num("assault", tx!("preset_units.general_assault"), 0.0, 1000.0, tx!("preset_units.general_assault_tip")),
    f("select_sound", tx!("preset_units.general_select_sound"), Kind::Pick(Src::UiSound, true), tx!("preset_units.general_select_sound_tip")),
];

pub const UNIT_COST: &[Field] = &[
    num("cost.mass", tx!("preset_units.cost_mass"), 0.0, BIG, tx!("preset_units.cost_mass_tip")),
    num("cost.energy", tx!("preset_units.cost_energy"), 0.0, BIG, tx!("preset_units.cost_energy_tip")),
    num("cost.time", tx!("preset_units.cost_time"), 1.0, BIG, tx!("preset_units.cost_time_tip")),
];

pub const UNIT_BODY: &[Field] = &[
    f("size", tx!("preset_units.body_size"), Kind::Vec3, tx!("preset_units.body_size_tip")),
    f("hit_box", tx!("preset_units.body_hit_box"), Kind::OptVec3, tx!("preset_units.body_hit_box_tip")),
    f("footprint", tx!("preset_units.body_footprint"), Kind::Int(0, 0, 64), tx!("preset_units.body_footprint_tip")),
    f("move.kind", tx!("preset_units.body_move_kind"), Kind::Choice(MOVE_KINDS, "none"), tx!("preset_units.body_move_kind_tip")),
    num("move.speed", tx!("preset_units.body_move_speed"), 0.0, 1000.0, tx!("preset_units.body_move_speed_tip")),
    num("move.reverse", tx!("preset_units.body_move_reverse"), 0.0, 1000.0, tx!("preset_units.body_move_reverse_tip")),
    num("move.accel", tx!("preset_units.body_move_accel"), 1.0, 1000.0, tx!("preset_units.body_move_accel_tip")),
    num("move.turn", tx!("preset_units.body_move_turn"), 0.0, 3600.0, tx!("preset_units.body_move_turn_tip")),
    num("move.altitude", tx!("preset_units.body_move_altitude"), 0.0, 1000.0, tx!("preset_units.body_move_altitude_tip")),
    no("move.amphibious", tx!("preset_units.body_move_amphibious"), tx!("preset_units.body_move_amphibious_tip")),
];

pub const UNIT_INTEL: &[Field] = &[
    num("vision", tx!("preset_units.intel_vision"), 0.0, 5000.0, tx!("preset_units.intel_vision_tip")),
    num("spot_time", tx!("preset_units.intel_spot_time"), 0.0, 600.0, tx!("preset_units.intel_spot_time_tip")),
    num("spot_time_moving", tx!("preset_units.intel_spot_time_moving"), 0.0, 600.0, tx!("preset_units.intel_spot_time_moving_tip")),
    num("radar", tx!("preset_units.intel_radar"), 0.0, 5000.0, tx!("preset_units.intel_radar_tip")),
    num("radar2", tx!("preset_units.intel_radar2"), 0.0, 5000.0, tx!("preset_units.intel_radar2_tip")),
    num("radar3", tx!("preset_units.intel_radar3"), 0.0, 5000.0, tx!("preset_units.intel_radar3_tip")),
    f("stealth", tx!("preset_units.intel_stealth"), Kind::Int(0, 0, 3), tx!("preset_units.intel_stealth_tip")),
];

pub const UNIT_ECO: &[Field] = &[
    num("eco.mass", tx!("preset_units.eco_mass"), 0.0, BIG, tx!("preset_units.eco_mass_tip")),
    num("eco.energy", tx!("preset_units.eco_energy"), 0.0, BIG, tx!("preset_units.eco_energy_tip")),
    num("eco.upkeep", tx!("preset_units.eco_upkeep"), 0.0, BIG, tx!("preset_units.eco_upkeep_tip")),
    num("eco.store_mass", tx!("preset_units.eco_store_mass"), 0.0, BIG, tx!("preset_units.eco_store_mass_tip")),
    num("eco.store_energy", tx!("preset_units.eco_store_energy"), 0.0, BIG, tx!("preset_units.eco_store_energy_tip")),
];

pub const UNIT_BUILD: &[Field] = &[
    num("rate", tx!("preset_units.build_rate"), 10.0, 10000.0, tx!("preset_units.build_rate_tip")),
    num("range", tx!("preset_units.build_range"), 0.0, 1000.0, tx!("preset_units.build_range_tip")),
    f("categories", tx!("preset_units.build_categories"), Kind::TagsFrom(Src::BuildCat), tx!("preset_units.build_categories_tip")),
    f("tier", tx!("preset_units.build_tier"), Kind::Int(1, 0, 9), tx!("preset_units.build_tier_tip")),
    no("station", tx!("preset_units.build_station"), tx!("preset_units.build_station_tip")),
    no("scavenger", tx!("preset_units.build_scavenger"), tx!("preset_units.build_scavenger_tip")),
];

pub const UNIT_BINOCULARS: &[Field] = &[
    num("range", tx!("preset_units.binoculars_range"), 140.0, 5000.0, tx!("preset_units.binoculars_range_tip")),
    num("angle", tx!("preset_units.binoculars_angle"), 20.0, 170.0, tx!("preset_units.binoculars_angle_tip")),
    num("turn", tx!("preset_units.binoculars_turn"), 40.0, 3600.0, tx!("preset_units.binoculars_turn_tip")),
    num("scan", tx!("preset_units.binoculars_scan"), 60.0, 180.0, tx!("preset_units.binoculars_scan_tip")),
];

pub const UNIT_TRANSPORT: &[Field] = &[
    f("capacity", tx!("preset_units.transport_capacity"), Kind::Int(4, 1, 255), tx!("preset_units.transport_capacity_tip")),
    num("vision", tx!("preset_units.transport_vision"), 0.0, 10.0, tx!("preset_units.transport_vision_tip")),
];

pub const UNIT_FIELDWORK: &[Field] = &[
    num("cover", tx!("preset_units.fieldwork_cover"), 0.5, 1.0, tx!("preset_units.fieldwork_cover_tip")),
    num("dig", tx!("preset_units.fieldwork_dig"), 0.0, 1.4, tx!("preset_units.fieldwork_dig_tip")),
];

pub const UNIT_REACTIVE: &[Field] = &[
    num("charges", tx!("preset_units.reactive_charges"), 4.0, 50.0, tx!("preset_units.reactive_charges_tip")),
    num("block", tx!("preset_units.reactive_block"), 0.75, 1.0, tx!("preset_units.reactive_block_tip")),
];

pub const UNIT_DEATH: &[Field] = &[
    num("damage", tx!("preset_units.death_damage"), 100.0, BIG, tx!("preset_units.death_damage_tip")),
    num("radius", tx!("preset_units.death_radius"), 5.0, 1000.0, tx!("preset_units.death_radius_tip")),
];

pub const UNIT_ICON: &[Field] = &[
    f("icon.shape", tx!("preset_units.icon_shape"), Kind::Choice(SHAPES, "land"), tx!("preset_units.icon_shape_tip")),
    f("icon.glyphs", tx!("preset_units.icon_glyphs"), Kind::Tags(GLYPHS, &[]), tx!("preset_units.icon_glyphs_tip")),
    no("icon.no_tier", tx!("preset_units.icon_no_tier"), tx!("preset_units.icon_no_tier_tip")),
    num("model.scale", tx!("preset_units.icon_model_scale"), 1.0, 1000.0, tx!("preset_units.icon_model_scale_tip")),
];

pub const WEAPON_FIELDS: &[Field] = &[
    f("name", tx!("preset_weapons.name"), Kind::Text, tx!("preset_weapons.name_tip")),
    f("kind", tx!("preset_weapons.kind"), Kind::Choice(WEAPON_KINDS, "direct"), tx!("preset_weapons.kind_tip")),
    num("damage", tx!("preset_weapons.damage"), 10.0, BIG, tx!("preset_weapons.damage_tip")),
    f("damage_type", tx!("preset_weapons.damage_type"), Kind::Pick(Src::DamageType, false), tx!("preset_weapons.damage_type_tip")),
    num("radius", tx!("preset_weapons.radius"), 0.0, 1000.0, tx!("preset_weapons.radius_tip")),
    f("splash", tx!("preset_weapons.splash"), Kind::OptNum(0.0, 1.0, 0.4), tx!("preset_weapons.splash_tip")),
    num("range", tx!("preset_weapons.range"), 50.0, 10000.0, tx!("preset_weapons.range_tip")),
    num("min_range", tx!("preset_weapons.min_range"), 0.0, 10000.0, tx!("preset_weapons.min_range_tip")),
    num("reload", tx!("preset_weapons.reload"), 1.0, 3600.0, tx!("preset_weapons.reload_tip")),
    num("speed", tx!("preset_weapons.speed"), 50.0, 10000.0, tx!("preset_weapons.speed_tip")),
    f("salvo", tx!("preset_weapons.salvo"), Kind::Int(1, 1, 255), tx!("preset_weapons.salvo_tip")),
    num("salvo_delay", tx!("preset_weapons.salvo_delay"), 0.1, 100.0, tx!("preset_weapons.salvo_delay_tip")),
    num("spread", tx!("preset_weapons.spread"), 0.3, 90.0, tx!("preset_weapons.spread_tip")),
    num("aim_time", tx!("preset_weapons.aim_time"), 0.0, 100.0, tx!("preset_weapons.aim_time_tip")),
    num("hold", tx!("preset_weapons.hold"), 0.0, 100.0, tx!("preset_weapons.hold_tip")),
    num("ammo", tx!("preset_weapons.ammo"), 0.0, 1000.0, tx!("preset_weapons.ammo_tip")),
    num("rearm", tx!("preset_weapons.rearm"), 0.0, 3600.0, tx!("preset_weapons.rearm_tip")),
    num("rearm_mass", tx!("preset_weapons.rearm_mass"), 0.0, 10000.0, tx!("preset_weapons.rearm_mass_tip")),
    f("targets", tx!("preset_weapons.targets"), Kind::Tags(TARGETS, &["land"]), tx!("preset_weapons.targets_tip")),
    num("turn_rate", tx!("preset_weapons.turn_rate"), 0.0, 3600.0, tx!("preset_weapons.turn_rate_tip")),
    num("launch_pitch", tx!("preset_weapons.launch_pitch"), 0.0, 90.0, tx!("preset_weapons.launch_pitch_tip")),
    f("prefer", tx!("preset_weapons.prefer"), Kind::Choice(PRIORITIES, "none"), tx!("preset_weapons.prefer_tip")),
    no("snipe", tx!("preset_weapons.snipe"), tx!("preset_weapons.snipe_tip")),
    no("suicide", tx!("preset_weapons.suicide"), tx!("preset_weapons.suicide_tip")),
    f("sound", tx!("preset_weapons.sound"), Kind::Pick(Src::Sound, true), tx!("preset_weapons.sound_tip")),
];

pub const TURRET_FIELDS: &[Field] = &[
    num("yaw_speed", tx!("preset_weapons.turret_yaw_speed"), 90.0, 3600.0, tx!("preset_weapons.turret_yaw_speed_tip")),
    num("pitch_speed", tx!("preset_weapons.turret_pitch_speed"), 90.0, 3600.0, tx!("preset_weapons.turret_pitch_speed_tip")),
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
                if ui.checkbox(&mut on, tr!("preset_editor.own_value")).on_hover_text(tip).changed() {
                    out = Some(on.then(|| number(x)));
                }
                if on && ui.add(drag(&mut x, min, max)).on_hover_text(tip).changed() {
                    out = Some(Some(number(x)));
                }
                if !on {
                    ui.label(egui::RichText::new(tr!("preset_editor.default")).weak());
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
            let shown = if s.is_empty() { tr!("preset_editor.none").to_string() } else { s.clone() };
            egui::ComboBox::from_id_salt(id).selected_text(shown).width(180.0).show_ui(ui, |ui| {
                if optional && ui.selectable_label(s.is_empty(), tr!("preset_editor.none")).clicked() && !s.is_empty() {
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
        Kind::OptVec3 => {
            let mut on = cur.as_ref().map_or(false, |x| x.is_array());
            let mut xs = [1.0f64; 3];
            if let Some(a) = cur.as_ref().and_then(|x| x.as_array()) {
                for (k, x) in a.iter().take(3).enumerate() {
                    xs[k] = x.as_f64().unwrap_or(1.0);
                }
            }
            ui.horizontal(|ui| {
                if ui.checkbox(&mut on, tr!("preset_editor.own_value")).on_hover_text(tip).changed() {
                    out = Some(on.then(|| Value::Array(xs.iter().map(|x| number(*x)).collect())));
                }
                if on {
                    let mut changed = false;
                    for x in xs.iter_mut() {
                        changed |= ui.add(drag(x, 0.05, 1000.0)).on_hover_text(tip).changed();
                    }
                    if changed {
                        out = Some(Some(Value::Array(xs.iter().map(|x| number(*x)).collect())));
                    }
                } else {
                    ui.label(egui::RichText::new(tr!("preset_editor.default")).weak());
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
    section(ui, tr!("preset_rules.switches"), true, |ui| {
        ui.label(egui::RichText::new(tr!("preset_rules.switches_hint")).weak());
        ch |= grid(ui, v, RULE_SWITCHES, ctx, "sw");
    });
    section(ui, tr!("common.economy"), true, |ui| ch |= grid(ui, v, RULE_ECONOMY, ctx, "eco"));
    section(ui, tr!("preset_rules.combat"), false, |ui| ch |= grid(ui, v, RULE_COMBAT, ctx, "combat"));
    section(ui, tr!("common.sight_and_radar"), false, |ui| ch |= grid(ui, v, RULE_INTEL, ctx, "intel"));
    section(ui, tr!("preset_rules.wrecks"), false, |ui| ch |= grid(ui, v, RULE_WRECKS, ctx, "wrecks"));
    section(ui, tr!("preset_rules.cover_and_infantry"), false, |ui| ch |= grid(ui, v, RULE_COVER, ctx, "cover"));
    section(ui, tr!("preset_rules.craters_and_terraforming"), false, |ui| ch |= grid(ui, v, RULE_TERRAIN, ctx, "terrain"));
    section(ui, tr!("preset_rules.control_points_mode"), false, |ui| ch |= grid(ui, v, RULE_POINTS, ctx, "cp"));
    section(ui, tr!("preset_rules.armor_table"), false, |ui| ch |= armor_table(ui, v));
    section(ui, tr!("common.trees_and_rocks"), false, |ui| ch |= features(ui, v, ctx));
    section(ui, tr!("preset_rules.colours"), false, |ui| ch |= colours(ui, v));
    ch
}

/// Damage multipliers: one row per armor class, one column per damage type.
fn armor_table(ui: &mut egui::Ui, v: &mut Value) -> bool {
    let types: Vec<String> = v["damage_types"].as_array().map(|a| a.iter().filter_map(|d| d.as_str().map(String::from)).collect()).unwrap_or_default();
    let mut names: Vec<String> = v["armor"].as_object().map(|a| a.keys().cloned().collect()).unwrap_or_default();
    names.sort();
    let mut changed = false;
    ui.label(egui::RichText::new(tr!("preset_rules.armor_table_hint")).weak());
    egui::Grid::new("armor_table").striped(true).spacing([10.0, 4.0]).show(ui, |ui| {
        ui.label("");
        for t in &types {
            ui.label(egui::RichText::new(t).strong()).on_hover_text(tr!("preset_rules.armor_column_tip", t));
        }
        ui.end_row();
        for a in &names {
            ui.label(egui::RichText::new(a).strong()).on_hover_text(tr!("preset_rules.armor_row_tip", a));
            for t in &types {
                let mut x = v["armor"][a][t].as_f64().unwrap_or(1.0);
                if ui.add(drag(&mut x, 0.0, 100.0)).on_hover_text(tr!("preset_rules.armor_cell_tip", t, a)).changed() {
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
    ui.label(egui::RichText::new(tr!("preset_rules.features_hint")).weak());
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
                if ui.button(tr!("preset_rules.feature_copy")).on_hover_text(tr!("preset_rules.feature_copy_tip")).clicked() {
                    copy = Some(k);
                }
                if ui.button(tr!("preset_rules.feature_remove")).on_hover_text(tr!("preset_rules.feature_remove_tip")).clicked() {
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
    // The player colours sit in a row of their own: a wrapping layout inside a grid cell has no
    // width to wrap at and stacked every button on a line of its own.
    ui.label(tr!("preset_rules.player_colours")).on_hover_text(tr!("preset_rules.players_tip"));
    ui.horizontal_wrapped(|ui| {
        for (k, c) in v["player_colors"].as_array_mut().into_iter().flatten().enumerate() {
            rgb(ui, c, &tr!("preset_rules.player_n", k + 1));
        }
    });
    ui.add_space(4.0);
    egui::Grid::new("colours").spacing([16.0, 6.0]).show(ui, |ui| {
        ui.label(tr!("preset_rules.neutral")).on_hover_text(tr!("preset_rules.neutral_tip"));
        rgb(ui, &mut v["neutral_color"], tr!("preset_rules.neutral_tip"));
        ui.end_row();
        ui.label(tr!("preset_rules.hostile")).on_hover_text(tr!("preset_rules.hostile_tip"));
        rgb(ui, &mut v["hostile_color"], tr!("preset_rules.hostile_tip"));
        ui.end_row();
    });
    changed
}

/// One unit's form. Returns true when something changed.
pub fn unit_form(ui: &mut egui::Ui, v: &mut Value, ctx: &FormCtx) -> bool {
    let mut ch = false;
    section(ui, tr!("preset_units.general"), true, |ui| ch |= grid(ui, v, UNIT_GENERAL, ctx, "gen"));
    section(ui, tr!("preset_units.cost"), true, |ui| ch |= grid(ui, v, UNIT_COST, ctx, "cost"));
    section(ui, tr!("preset_units.body_and_movement"), false, |ui| ch |= grid(ui, v, UNIT_BODY, ctx, "body"));
    section(ui, tr!("common.sight_and_radar"), false, |ui| {
        ch |= grid(ui, v, UNIT_INTEL, ctx, "intel");
        ch |= optional(ui, v, "binoculars", tr!("preset_units.binoculars"), tr!("preset_units.binoculars_tip"), json!({ "range": 140, "angle": 20, "turn": 40, "scan": 60 }), UNIT_BINOCULARS, ctx, "binoculars");
    });
    section(ui, tr!("common.economy"), false, |ui| ch |= grid(ui, v, UNIT_ECO, ctx, "eco"));
    section(ui, tr!("preset_units.weapons"), true, |ui| ch |= weapons(ui, v, ctx));
    section(ui, tr!("preset_units.construction"), false, |ui| {
        ch |= optional(ui, v, "build", tr!("preset_units.can_build"), tr!("preset_units.can_build_tip"), json!({ "rate": 10, "categories": [] }), UNIT_BUILD, ctx, "build");
        ch |= optional(ui, v, "transport", tr!("preset_units.carries_units"), tr!("preset_units.carries_units_tip"), json!({ "capacity": 4 }), UNIT_TRANSPORT, ctx, "transport");
        ch |= optional(ui, v, "fieldwork", tr!("preset_units.field_work"), tr!("preset_units.field_work_tip"), json!({ "cover": 0.5 }), UNIT_FIELDWORK, ctx, "fieldwork");
        ch |= optional(ui, v, "reactive", tr!("preset_units.reactive_armor"), tr!("preset_units.reactive_armor_tip"), json!({ "charges": 4, "block": 0.75 }), UNIT_REACTIVE, ctx, "reactive");
        ch |= optional(ui, v, "death", tr!("preset_units.explodes_when_destroyed"), tr!("preset_units.explodes_tip"), json!({ "damage": 100, "radius": 5 }), UNIT_DEATH, ctx, "death");
    });
    section(ui, tr!("preset_units.icon_and_model"), false, |ui| ch |= grid(ui, v, UNIT_ICON, ctx, "icon"));
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
            changed |= optional(ui, w, "turret", tr!("preset_weapons.turret"), tr!("preset_weapons.turret_tip"), json!({ "yaw_speed": 90, "pitch_speed": 90 }), TURRET_FIELDS, ctx, &salt);
            ui.horizontal(|ui| {
                if ui.button(tr!("preset_weapons.copy_weapon")).clicked() {
                    copy = Some(k);
                }
                if ui.button(tr!("preset_weapons.remove_weapon")).clicked() {
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
    if ui.button(tr!("preset_weapons.add_weapon")).on_hover_text(tr!("preset_weapons.add_weapon_tip")).clicked() {
        list.push(json!({ "name": "Cannon", "kind": "direct", "damage": 10, "range": 50, "reload": 1, "speed": 50, "turret": { "yaw_speed": 90, "pitch_speed": 90 } }));
        changed = true;
    }
    changed
}
