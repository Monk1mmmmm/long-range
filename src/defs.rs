//! Unit, weapon and armor definitions. Numbers are derived from the
//! Long Range Tank mod blueprints (lightly rebalanced for a standalone game).
use serde::{Deserialize, Serialize};

pub type DefId = u8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Commander,
    Engineer,
    Mobile,
    Factory,
    Structure,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Land,
    /// Fixed-wing: always moving, circles when idle.
    Plane,
    /// Hovering aircraft (gunship).
    Hover,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Armor {
    Light,
    LightTank,
    MediumTank,
    Structure,
    Commander,
    Air,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub enum Dmg {
    /// Small arms. Bounces off tanks.
    Normal,
    Autocannon,
    /// Armor penetrating (gauss, missiles, plasma).
    AP,
    Explosive,
}

pub fn armor_mult(d: Dmg, a: Armor) -> f32 {
    match (d, a) {
        (Dmg::Normal, Armor::LightTank) => 0.5,
        (Dmg::Normal, Armor::MediumTank) => 0.2,
        (Dmg::Normal, Armor::Commander) => 0.5,
        (Dmg::Autocannon, Armor::LightTank) => 0.66,
        (Dmg::Autocannon, Armor::MediumTank) => 0.33,
        (Dmg::Explosive, Armor::Air) => 0.5,
        _ => 1.0,
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PK {
    Direct,
    Arc,
    Homing,
    Bomb,
}

#[derive(Clone, Copy, Debug)]
pub struct Weapon {
    pub name: &'static str,
    pub dmg: f32,
    pub dtype: Dmg,
    pub aoe: f32,
    pub range: f32,
    pub min_range: f32,
    pub reload: f32,
    pub speed: f32,
    pub pk: PK,
    pub land: bool,
    pub air: bool,
    pub scatter: f32,
}

const W: Weapon = Weapon {
    name: "",
    dmg: 1.0,
    dtype: Dmg::Normal,
    aoe: 0.0,
    range: 20.0,
    min_range: 0.0,
    reload: 1.0,
    speed: 60.0,
    pk: PK::Direct,
    land: true,
    air: false,
    scatter: 0.0,
};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Shape {
    Commander,
    Engineer,
    Buggy,
    Bot,
    Tank,
    Spg,
    Launcher,
    Flak,
    RadarCar,
    Jet,
    Gunship,
    Factory,
    AirFactory,
    Pump,
    Windmill,
    Solar,
    Storage,
    Turret,
    RadarTower,
}

#[derive(Clone, Copy, Debug)]
pub struct UnitDef {
    pub name: &'static str,
    pub desc: &'static str,
    pub tag: &'static str,
    pub kind: Kind,
    pub layer: Layer,
    pub armor: Armor,
    pub shape: Shape,
    pub hp: f32,
    pub mass: f32,
    pub energy: f32,
    pub build_time: f32,
    pub build_rate: f32,
    pub speed: f32,
    pub turn: f32,
    pub radius: f32,
    /// Footprint in map cells (0 for mobile units).
    pub foot: i32,
    pub vision: f32,
    pub radar: f32,
    pub prod_m: f32,
    pub prod_e: f32,
    pub upkeep_e: f32,
    pub store_m: f32,
    pub store_e: f32,
    pub weapons: &'static [Weapon],
    pub builds: &'static [DefId],
    pub needs_deposit: bool,
    pub upgrade: Option<DefId>,
    pub alt: f32,
}

impl UnitDef {
    pub fn is_structure(&self) -> bool {
        self.foot > 0
    }
    pub fn is_air(&self) -> bool {
        self.layer != Layer::Land
    }
    pub fn is_builder(&self) -> bool {
        matches!(self.kind, Kind::Commander | Kind::Engineer)
    }
    pub fn max_range(&self) -> f32 {
        self.weapons.iter().map(|w| w.range).fold(0.0, f32::max)
    }
    pub fn armed(&self) -> bool {
        !self.weapons.is_empty()
    }
}

const B: UnitDef = UnitDef {
    name: "",
    desc: "",
    tag: "",
    kind: Kind::Mobile,
    layer: Layer::Land,
    armor: Armor::Light,
    shape: Shape::Tank,
    hp: 100.0,
    mass: 100.0,
    energy: 500.0,
    build_time: 300.0,
    build_rate: 0.0,
    speed: 0.0,
    turn: 2.0,
    radius: 1.5,
    foot: 0,
    vision: 30.0,
    radar: 0.0,
    prod_m: 0.0,
    prod_e: 0.0,
    upkeep_e: 0.0,
    store_m: 0.0,
    store_e: 0.0,
    weapons: &[],
    builds: &[],
    needs_deposit: false,
    upgrade: None,
    alt: 0.0,
};

const S: UnitDef = UnitDef { kind: Kind::Structure, armor: Armor::Structure, foot: 1, radius: 2.0, vision: 20.0, turn: 0.0, ..B };

pub const COMMANDER: DefId = 0;
pub const ENGINEER: DefId = 1;
pub const BUGGY: DefId = 2;
pub const TROOPER: DefId = 3;
pub const OVERTAKER: DefId = 4;
pub const CRONY: DefId = 5;
pub const DRAGOON: DefId = 6;
pub const MORTAR: DefId = 7;
pub const ROBIN: DefId = 8;
pub const BOHDANA: DefId = 9;
pub const FLAK: DefId = 10;
pub const RADAR_CAR: DefId = 11;
pub const AIR_SCOUT: DefId = 12;
pub const INTERCEPTOR: DefId = 13;
pub const BOMBER: DefId = 14;
pub const GUNSHIP: DefId = 15;
pub const LAND_FACTORY: DefId = 16;
pub const AIR_FACTORY: DefId = 17;
pub const PUMP: DefId = 18;
pub const PUMP2: DefId = 19;
pub const WINDMILL: DefId = 20;
pub const SOLAR: DefId = 21;
pub const E_STORAGE: DefId = 22;
pub const M_STORAGE: DefId = 23;
pub const MG_TURRET: DefId = 24;
pub const GAUSS_TURRET: DefId = 25;
pub const AA_TURRET: DefId = 26;
pub const RADAR_TOWER: DefId = 27;
pub const BATTERY: DefId = 28;

const BASIC_BUILDS: &[DefId] = &[
    PUMP, WINDMILL, SOLAR, E_STORAGE, M_STORAGE, LAND_FACTORY, AIR_FACTORY, MG_TURRET, GAUSS_TURRET, AA_TURRET,
    RADAR_TOWER, BATTERY,
];
const LAND_BUILDS: &[DefId] =
    &[ENGINEER, BUGGY, TROOPER, OVERTAKER, CRONY, DRAGOON, MORTAR, ROBIN, BOHDANA, FLAK, RADAR_CAR];
const AIR_BUILDS: &[DefId] = &[AIR_SCOUT, INTERCEPTOR, BOMBER, GUNSHIP];

const MG: Weapon = Weapon { name: "Machine Gun", dmg: 2.0, range: 30.0, reload: 0.3, speed: 100.0, ..W };

pub static DEFS: [UnitDef; 29] = [
    UnitDef {
        name: "Commander",
        desc: "Command unit. Builds the base. Lose it and you lose the battle.",
        tag: "C",
        kind: Kind::Commander,
        armor: Armor::Commander,
        shape: Shape::Commander,
        hp: 8000.0,
        mass: 5000.0,
        energy: 50000.0,
        build_time: 60000.0,
        build_rate: 10.0,
        speed: 2.2,
        turn: 2.5,
        radius: 2.2,
        vision: 35.0,
        prod_m: 1.0,
        prod_e: 20.0,
        store_m: 650.0,
        store_e: 4000.0,
        weapons: &[Weapon { name: "Heavy Autocannon", dmg: 60.0, dtype: Dmg::AP, range: 30.0, reload: 0.8, ..W }],
        builds: BASIC_BUILDS,
        ..B
    },
    UnitDef {
        name: "Engineer",
        desc: "Builds structures, assists construction, repairs and reclaims wrecks.",
        tag: "E",
        kind: Kind::Engineer,
        shape: Shape::Engineer,
        hp: 150.0,
        mass: 52.0,
        energy: 260.0,
        build_time: 260.0,
        build_rate: 5.0,
        speed: 2.6,
        turn: 2.1,
        radius: 1.2,
        vision: 18.0,
        store_m: 10.0,
        builds: BASIC_BUILDS,
        ..B
    },
    UnitDef {
        name: "Buggy",
        desc: "Land scout. Fast, fragile, long sight. The eyes of your artillery.",
        tag: "s",
        shape: Shape::Buggy,
        hp: 40.0,
        mass: 20.0,
        energy: 300.0,
        build_time: 120.0,
        speed: 6.5,
        turn: 2.6,
        radius: 1.0,
        vision: 50.0,
        weapons: &[Weapon { name: "Light MG", dmg: 2.0, range: 18.0, reload: 0.25, speed: 75.0, ..W }],
        ..B
    },
    UnitDef {
        name: "Trooper",
        desc: "Armored assault bot. Cheap plasma gun that also hurts armor.",
        tag: "b",
        shape: Shape::Bot,
        hp: 65.0,
        mass: 65.0,
        energy: 500.0,
        build_time: 140.0,
        speed: 4.0,
        turn: 3.0,
        radius: 0.9,
        vision: 45.0,
        weapons: &[Weapon { name: "Plasma Gun", dmg: 4.0, dtype: Dmg::AP, range: 18.0, reload: 0.33, speed: 32.0, ..W }],
        ..B
    },
    UnitDef {
        name: "Overtaker M1",
        desc: "Light tank. Quick flanker with a 75 mm gauss cannon.",
        tag: "L",
        armor: Armor::LightTank,
        hp: 300.0,
        mass: 320.0,
        energy: 1000.0,
        build_time: 450.0,
        speed: 4.0,
        turn: 1.3,
        radius: 1.5,
        vision: 35.0,
        weapons: &[
            Weapon { name: "Gauss Cannon", dmg: 75.0, dtype: Dmg::AP, aoe: 1.0, range: 45.0, reload: 2.0, speed: 45.0, ..W },
            MG,
        ],
        ..B
    },
    UnitDef {
        name: "Crony M1",
        desc: "Main battle tank. Slow, heavily armored, hits hard at 60 range.",
        tag: "T",
        armor: Armor::MediumTank,
        hp: 500.0,
        mass: 560.0,
        energy: 1500.0,
        build_time: 600.0,
        speed: 2.4,
        turn: 0.8,
        radius: 1.9,
        vision: 30.0,
        weapons: &[
            Weapon { name: "Gauss Cannon", dmg: 200.0, dtype: Dmg::AP, aoe: 1.0, range: 60.0, reload: 4.0, speed: 60.0, ..W },
            MG,
        ],
        ..B
    },
    UnitDef {
        name: "Dragoon ARV",
        desc: "Armored recon vehicle. Very fast, autocannon, long sight.",
        tag: "R",
        armor: Armor::LightTank,
        shape: Shape::Buggy,
        hp: 320.0,
        mass: 375.0,
        energy: 1200.0,
        build_time: 900.0,
        speed: 6.0,
        turn: 1.0,
        radius: 1.5,
        vision: 45.0,
        weapons: &[Weapon { name: "Autocannon", dmg: 30.0, dtype: Dmg::Autocannon, range: 45.0, reload: 0.8, speed: 50.0, ..W }],
        ..B
    },
    UnitDef {
        name: "Pike Mortar",
        desc: "Mobile mortar. Indirect fire, range 100, cannot hit close targets.",
        tag: "m",
        shape: Shape::Spg,
        hp: 220.0,
        mass: 220.0,
        energy: 1200.0,
        build_time: 1200.0,
        speed: 3.0,
        turn: 1.2,
        radius: 1.4,
        weapons: &[Weapon {
            name: "Mortar",
            dmg: 50.0,
            dtype: Dmg::Explosive,
            aoe: 4.0,
            range: 100.0,
            min_range: 25.0,
            reload: 2.0,
            speed: 50.0,
            pk: PK::Arc,
            scatter: 0.03,
            ..W
        }],
        ..B
    },
    UnitDef {
        name: "Robin",
        desc: "Mobile anti-tank. One guided missile, long reload, kills most tanks.",
        tag: "M",
        shape: Shape::Launcher,
        hp: 220.0,
        mass: 450.0,
        energy: 3000.0,
        build_time: 2500.0,
        speed: 3.0,
        turn: 1.2,
        radius: 1.5,
        weapons: &[Weapon { name: "Guided Missile", dmg: 500.0, dtype: Dmg::AP, range: 90.0, reload: 14.0, speed: 28.0, pk: PK::Homing, ..W }],
        ..B
    },
    UnitDef {
        name: "Bohdana",
        desc: "Self-propelled howitzer. Range 300 - needs scouts or radar to find targets.",
        tag: "A",
        armor: Armor::LightTank,
        shape: Shape::Spg,
        hp: 430.0,
        mass: 1250.0,
        energy: 3000.0,
        build_time: 2000.0,
        speed: 1.8,
        turn: 0.95,
        radius: 2.1,
        weapons: &[Weapon {
            name: "155 mm Howitzer",
            dmg: 400.0,
            dtype: Dmg::Explosive,
            aoe: 5.0,
            range: 300.0,
            min_range: 60.0,
            reload: 20.0,
            speed: 55.0,
            pk: PK::Arc,
            scatter: 0.015,
            ..W
        }],
        ..B
    },
    UnitDef {
        name: "MBFAG-113",
        desc: "Mobile anti-air system. Rotary cannon, long reach against aircraft.",
        tag: "F",
        armor: Armor::LightTank,
        shape: Shape::Flak,
        hp: 250.0,
        mass: 200.0,
        energy: 600.0,
        build_time: 500.0,
        speed: 2.6,
        turn: 0.8,
        radius: 1.5,
        weapons: &[
            Weapon { name: "Rotary Cannon (Air)", dmg: 10.0, range: 70.0, reload: 0.5, speed: 110.0, land: false, air: true, ..W },
            Weapon { name: "Rotary Cannon (Ground)", dmg: 3.0, range: 25.0, reload: 0.5, speed: 50.0, ..W },
        ],
        ..B
    },
    UnitDef {
        name: "UC-01R Radar",
        desc: "Mobile surveillance radar. Detects units in a 120 radius. Needs energy.",
        tag: "r",
        shape: Shape::RadarCar,
        hp: 75.0,
        mass: 115.0,
        energy: 2500.0,
        build_time: 600.0,
        speed: 2.6,
        turn: 1.5,
        radius: 1.3,
        vision: 20.0,
        radar: 120.0,
        upkeep_e: 30.0,
        ..B
    },
    UnitDef {
        name: "Kestrel",
        desc: "Air scout. Huge sight radius, unarmed.",
        tag: "s",
        layer: Layer::Plane,
        armor: Armor::Air,
        shape: Shape::Jet,
        hp: 120.0,
        mass: 80.0,
        energy: 1600.0,
        build_time: 400.0,
        speed: 17.0,
        turn: 1.6,
        radius: 1.3,
        vision: 60.0,
        alt: 14.0,
        ..B
    },
    UnitDef {
        name: "Cyclone-L",
        desc: "Interceptor. Linked railguns, air targets only.",
        tag: "I",
        layer: Layer::Plane,
        armor: Armor::Air,
        shape: Shape::Jet,
        hp: 295.0,
        mass: 200.0,
        energy: 3500.0,
        build_time: 500.0,
        speed: 15.0,
        turn: 1.7,
        radius: 1.5,
        vision: 35.0,
        alt: 14.0,
        weapons: &[Weapon { name: "Linked Railgun", dmg: 25.0, range: 35.0, reload: 0.5, speed: 120.0, land: false, air: true, ..W }],
        ..B
    },
    UnitDef {
        name: "Firebrand",
        desc: "Attack bomber. Drops heavy bombs on ground targets.",
        tag: "B",
        layer: Layer::Plane,
        armor: Armor::Air,
        shape: Shape::Jet,
        hp: 320.0,
        mass: 250.0,
        energy: 2400.0,
        build_time: 600.0,
        speed: 11.0,
        turn: 1.1,
        radius: 1.9,
        vision: 32.0,
        radar: 50.0,
        alt: 14.0,
        weapons: &[Weapon {
            name: "Heavy Bomb",
            dmg: 150.0,
            dtype: Dmg::Explosive,
            aoe: 4.0,
            range: 13.0,
            reload: 5.0,
            pk: PK::Bomb,
            ..W
        }],
        ..B
    },
    UnitDef {
        name: "Hornet",
        desc: "Gunship. Hovers and suppresses ground units with a riot gun.",
        tag: "G",
        layer: Layer::Hover,
        armor: Armor::Air,
        shape: Shape::Gunship,
        hp: 450.0,
        mass: 420.0,
        energy: 4800.0,
        build_time: 1600.0,
        speed: 7.0,
        turn: 2.0,
        radius: 1.8,
        vision: 30.0,
        alt: 8.0,
        weapons: &[Weapon { name: "Riot Gun", dmg: 15.0, dtype: Dmg::Autocannon, range: 30.0, reload: 0.35, speed: 60.0, ..W }],
        ..B
    },
    UnitDef {
        name: "Land Factory",
        desc: "Produces land units. Right-click to set a rally point.",
        tag: "LF",
        kind: Kind::Factory,
        shape: Shape::Factory,
        hp: 4800.0,
        mass: 240.0,
        energy: 2100.0,
        build_time: 300.0,
        build_rate: 20.0,
        foot: 3,
        radius: 6.0,
        store_m: 80.0,
        builds: LAND_BUILDS,
        ..S
    },
    UnitDef {
        name: "Air Factory",
        desc: "Produces aircraft. Right-click to set a rally point.",
        tag: "AF",
        kind: Kind::Factory,
        shape: Shape::AirFactory,
        hp: 4800.0,
        mass: 210.0,
        energy: 2400.0,
        build_time: 300.0,
        build_rate: 20.0,
        foot: 3,
        radius: 6.0,
        store_m: 80.0,
        builds: AIR_BUILDS,
        ..S
    },
    UnitDef {
        name: "Mass Pump",
        desc: "Extracts mass. Must be built on a mass deposit. Upgradeable (U).",
        tag: "M",
        shape: Shape::Pump,
        hp: 600.0,
        mass: 30.0,
        energy: 250.0,
        build_time: 60.0,
        build_rate: 10.0,
        prod_m: 2.0,
        upkeep_e: 5.0,
        needs_deposit: true,
        upgrade: Some(PUMP2),
        ..S
    },
    UnitDef {
        name: "Mass Pump Mk2",
        desc: "Improved extractor: triple output, hungry for energy.",
        tag: "M2",
        shape: Shape::Pump,
        hp: 3000.0,
        mass: 600.0,
        energy: 4000.0,
        build_time: 900.0,
        prod_m: 6.0,
        upkeep_e: 30.0,
        needs_deposit: true,
        ..S
    },
    UnitDef {
        name: "Windmill",
        desc: "Cheap power generator: +20 energy.",
        tag: "W",
        shape: Shape::Windmill,
        hp: 600.0,
        mass: 75.0,
        energy: 750.0,
        build_time: 125.0,
        prod_e: 20.0,
        ..S
    },
    UnitDef {
        name: "Solar Array",
        desc: "Large power generator: +120 energy.",
        tag: "S",
        shape: Shape::Solar,
        hp: 1200.0,
        mass: 500.0,
        energy: 3000.0,
        build_time: 800.0,
        prod_e: 120.0,
        foot: 2,
        radius: 4.0,
        ..S
    },
    UnitDef {
        name: "Energy Storage",
        desc: "Stores 5000 energy.",
        tag: "ES",
        shape: Shape::Storage,
        hp: 500.0,
        mass: 250.0,
        energy: 1200.0,
        build_time: 200.0,
        store_e: 5000.0,
        ..S
    },
    UnitDef {
        name: "Mass Storage",
        desc: "Stores 500 mass.",
        tag: "MS",
        shape: Shape::Storage,
        hp: 1600.0,
        mass: 200.0,
        energy: 1500.0,
        build_time: 250.0,
        store_m: 500.0,
        ..S
    },
    UnitDef {
        name: "HMG Turret",
        desc: "Point defense. Shreds bots and scouts, weak against armor.",
        tag: "PD",
        shape: Shape::Turret,
        hp: 1300.0,
        mass: 250.0,
        energy: 2000.0,
        build_time: 250.0,
        vision: 26.0,
        weapons: &[Weapon { name: "Heavy Machine Gun", dmg: 4.0, range: 60.0, reload: 0.2, speed: 75.0, ..W }],
        ..S
    },
    UnitDef {
        name: "Gauss Turret",
        desc: "Anti-tank point defense. Range 90, cannot hit targets closer than 30.",
        tag: "GT",
        shape: Shape::Turret,
        hp: 1300.0,
        mass: 450.0,
        energy: 2000.0,
        build_time: 500.0,
        vision: 26.0,
        weapons: &[Weapon {
            name: "Gauss Cannon",
            dmg: 50.0,
            dtype: Dmg::AP,
            aoe: 2.0,
            range: 90.0,
            min_range: 30.0,
            reload: 2.0,
            speed: 50.0,
            ..W
        }],
        ..S
    },
    UnitDef {
        name: "DA1 Railgun",
        desc: "Anti-air turret.",
        tag: "AA",
        shape: Shape::Turret,
        hp: 1200.0,
        mass: 400.0,
        energy: 3000.0,
        build_time: 250.0,
        vision: 24.0,
        weapons: &[Weapon { name: "Linked Railgun", dmg: 30.0, range: 90.0, reload: 0.5, speed: 150.0, land: false, air: true, ..W }],
        ..S
    },
    UnitDef {
        name: "Radar Tower",
        desc: "Detects units in a 160 radius. Needs energy.",
        tag: "RT",
        shape: Shape::RadarTower,
        hp: 300.0,
        mass: 80.0,
        energy: 700.0,
        build_time: 200.0,
        radar: 160.0,
        upkeep_e: 15.0,
        ..S
    },
    UnitDef {
        name: "M1 Heavy Battery",
        desc: "Fortified artillery emplacement. Range 220, large blast.",
        tag: "HB",
        shape: Shape::Turret,
        hp: 6000.0,
        mass: 2200.0,
        energy: 12000.0,
        build_time: 3000.0,
        foot: 2,
        radius: 4.0,
        vision: 26.0,
        weapons: &[Weapon {
            name: "Heavy Gun",
            dmg: 300.0,
            dtype: Dmg::Explosive,
            aoe: 6.0,
            range: 220.0,
            min_range: 40.0,
            reload: 10.0,
            speed: 55.0,
            pk: PK::Arc,
            scatter: 0.012,
            ..W
        }],
        ..S
    },
];

pub fn def(id: DefId) -> &'static UnitDef {
    &DEFS[id as usize]
}
