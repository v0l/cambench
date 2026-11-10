#[derive(Clone, Debug)]
pub struct Material {
    pub name: &'static str,
    pub surface_speed: f64,
    pub chip_per_dia: f64,
    pub stepdown_per_dia: f64,
    pub stepover: f64,
    pub laser_power: f64,
    pub laser_feed: f64,
}

const TABLE: &[Material] = &[
    Material {
        name: "aluminium",
        surface_speed: 200.0,
        chip_per_dia: 0.008,
        stepdown_per_dia: 0.5,
        stepover: 0.4,
        laser_power: 0.0,
        laser_feed: 0.0,
    },
    Material {
        name: "wood",
        surface_speed: 300.0,
        chip_per_dia: 0.02,
        stepdown_per_dia: 1.0,
        stepover: 0.5,
        laser_power: 600.0,
        laser_feed: 900.0,
    },
    Material {
        name: "plywood",
        surface_speed: 300.0,
        chip_per_dia: 0.018,
        stepdown_per_dia: 1.0,
        stepover: 0.5,
        laser_power: 800.0,
        laser_feed: 600.0,
    },
    Material {
        name: "plastic",
        surface_speed: 250.0,
        chip_per_dia: 0.015,
        stepdown_per_dia: 0.8,
        stepover: 0.45,
        laser_power: 0.0,
        laser_feed: 0.0,
    },
    Material {
        name: "acrylic",
        surface_speed: 250.0,
        chip_per_dia: 0.012,
        stepdown_per_dia: 0.6,
        stepover: 0.45,
        laser_power: 900.0,
        laser_feed: 300.0,
    },
    Material {
        name: "cardboard",
        surface_speed: 300.0,
        chip_per_dia: 0.02,
        stepdown_per_dia: 1.0,
        stepover: 0.5,
        laser_power: 300.0,
        laser_feed: 1500.0,
    },
];

pub fn find(name: &str) -> Option<Material> {
    TABLE.iter().find(|m| m.name == name).cloned()
}

pub fn names() -> Vec<&'static str> {
    TABLE.iter().map(|m| m.name).collect()
}

#[derive(Clone, Debug)]
pub struct Cutting {
    pub rpm: f64,
    pub feed: f64,
    pub plunge: f64,
    pub stepdown: f64,
    pub stepover: f64,
}

pub fn cutting(m: &Material, dia: f64, flutes: u32, max_rpm: f64) -> Cutting {
    let rpm = (m.surface_speed * 1000.0 / (std::f64::consts::PI * dia))
        .min(max_rpm)
        .round();
    let feed = (rpm * flutes as f64 * m.chip_per_dia * dia)
        .round()
        .max(10.0);
    Cutting {
        rpm,
        feed,
        plunge: (feed / 3.0).round().max(5.0),
        stepdown: m.stepdown_per_dia * dia,
        stepover: m.stepover * dia,
    }
}
