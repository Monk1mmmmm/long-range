//! Game data loaded from JSON: rules, factions, unit and weapon definitions.
//! Nothing about a specific unit is hard-coded in Rust; see `assets/data`.
use bevy::prelude::Resource;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

fn one() -> f32 {
    1.0
}
fn one_u8() -> u8 {
    1
}
fn yes() -> bool {
    true
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
    pub reclaim_rate: f32,
    pub repair_rate: f32,
    pub radar_fade_secs: f32,
    pub radar_los_height: f32,
    pub power_stall_pump_factor: f32,
    #[serde(default)]
    pub friendly_fire: bool,
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
    pub damage: f32,
    #[serde(default = "normal")]
    pub damage_type: String,
    /// Blast radius, 0 = direct hits only.
    #[serde(default)]
    pub radius: f32,
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
    #[serde(skip)]
    pub dtype: usize,
    #[serde(skip)]
    pub hits_land: bool,
    #[serde(skip)]
    pub hits_air: bool,
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
    pub armor: String,
    pub cost: Cost,
    /// Collision box: width (x), height (y), length (z).
    pub size: [f32; 3],
    /// Structures: side length in grid cells.
    #[serde(default)]
    pub footprint: i32,
    #[serde(default, rename = "move")]
    pub mv: Move,
    #[serde(default)]
    pub vision: f32,
    #[serde(default)]
    pub radar: f32,
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
    pub fn armed(&self) -> bool {
        !self.weapons.is_empty()
    }
    pub fn max_range(&self) -> f32 {
        self.weapons.iter().map(|w| w.range).fold(0.0, f32::max)
    }
    pub fn has_category(&self, c: &str) -> bool {
        self.categories.iter().any(|x| x == c)
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

#[derive(Resource, Clone)]
pub struct Defs {
    pub rules: Rules,
    pub units: Vec<UnitDef>,
    pub by_name: HashMap<String, DefId>,
    pub factions: Vec<Faction>,
    /// Hash of all data files; host and clients must match.
    pub hash: u64,
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
    pub fn armor_mult(&self, dtype: usize, armor: usize) -> f32 {
        self.rules.armor_table.get(armor).and_then(|r| r.get(dtype)).copied().unwrap_or(1.0)
    }
    pub fn playable_factions(&self) -> Vec<usize> {
        (0..self.factions.len()).filter(|i| self.factions[*i].playable).collect()
    }

    pub fn load(root: &Path) -> Result<Defs, String> {
        let mut hash = 0xcbf29ce484222325u64;
        let rules_path = root.join("data/rules.json");
        let text = std::fs::read_to_string(&rules_path).map_err(|e| format!("{}: {e}", rules_path.display()))?;
        fnv(&mut hash, text.as_bytes());
        let mut rules: Rules = serde_json::from_str(&text).map_err(|e| format!("rules.json: {e}"))?;
        let mut armor_names: Vec<String> = rules.armor.keys().cloned().collect();
        armor_names.sort();
        rules.armor_table = armor_names
            .iter()
            .map(|a| rules.damage_types.iter().map(|d| rules.armor[a].get(d).copied().unwrap_or(1.0)).collect())
            .collect();
        rules.armor_names = armor_names;

        let dir = root.join("data/factions");
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().map_or(false, |x| x == "json"))
            .collect();
        files.sort();
        let mut units: Vec<UnitDef> = vec![];
        let mut factions = vec![];
        let mut commanders = vec![];
        for f in files {
            let text = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            fnv(&mut hash, text.as_bytes());
            let ff: FactionFile = serde_json::from_str(&text).map_err(|e| format!("{}: {e}", f.display()))?;
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
            }
            if let Some(b) = &u.build {
                u.builds = snapshot
                    .iter()
                    .filter(|o| o.faction == u.faction && o.buildable && o.tier <= b.tier)
                    .filter(|o| o.built_by.as_ref().map_or(false, |c| b.categories.contains(c)))
                    .map(|o| o.idx)
                    .collect();
            }
            if !u.is_structure() && u.footprint != 0 {
                return Err(format!("{}: mobile units must not have a footprint", u.id));
            }
            if u.is_structure() && u.footprint <= 0 {
                return Err(format!("{}: structures need a footprint", u.id));
            }
            if let Some(other) = icons.insert((u.icon.clone(), u.tier), u.id.clone()) {
                return Err(format!("units '{}' and '{}' share the same strategic icon; every unit needs a unique one", other, u.id));
            }
        }
        Ok(Defs { rules, units, by_name, factions, hash })
    }
}
