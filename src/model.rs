use std::collections::BTreeMap;

use crate::geom::{P2, Shape};
use crate::materials::{self, Material};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ToolKind {
    Flat,
    Ball,
    VBit(f64),
    Laser,
}

#[derive(Clone, Debug)]
pub struct Tool {
    pub n: u32,
    pub kind: ToolKind,
    pub dia: f64,
    pub flutes: u32,
    pub flute_len: f64,
    pub holder_dia: f64,
}

impl Tool {
    pub fn radius(&self) -> f64 {
        self.dia / 2.0
    }

    pub fn profile(&self, d: f64) -> f64 {
        match self.kind {
            ToolKind::Flat | ToolKind::Laser => 0.0,
            ToolKind::Ball => {
                let r = self.radius();
                r - (r * r - d * d).max(0.0).sqrt()
            }
            ToolKind::VBit(angle) => d / (angle.to_radians() / 2.0).tan(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Machine {
    pub rapid: f64,
    pub max_rpm: f64,
    pub clearance: f64,
}

#[derive(Clone, Debug)]
pub struct Op {
    pub line: usize,
    pub verb: String,
    pub args: Vec<String>,
    pub kv: BTreeMap<String, String>,
}

impl Op {
    pub fn num(&self, key: &str) -> Result<Option<f64>, String> {
        match self.kv.get(key) {
            None => Ok(None),
            Some(v) => v
                .parse::<f64>()
                .map(Some)
                .map_err(|_| format!("line {}: {key}={v} is not a number", self.line)),
        }
    }

    pub fn need(&self, key: &str) -> Result<f64, String> {
        self.num(key)?
            .ok_or_else(|| format!("line {}: {} needs {key}=", self.line, self.verb))
    }

    pub fn or(&self, key: &str, default: f64) -> Result<f64, String> {
        Ok(self.num(key)?.unwrap_or(default))
    }

    pub fn text(&self, key: &str) -> Option<&str> {
        self.kv.get(key).map(String::as_str)
    }

    pub fn err(&self, msg: impl std::fmt::Display) -> String {
        format!("line {}: {msg}", self.line)
    }
}

#[derive(Clone, Debug)]
pub struct PartRef {
    pub path: String,
    pub at: [f64; 3],
}

#[derive(Clone, Debug)]
pub struct Job {
    pub stock: [f64; 3],
    pub material: Material,
    pub machine: Machine,
    pub tools: BTreeMap<u32, Tool>,
    pub ops: Vec<Op>,
    pub part: Option<PartRef>,
    pub grid: f64,
    pub tol: f64,
}

fn nums(s: &str) -> Option<Vec<f64>> {
    s.split(',').map(|p| p.parse::<f64>().ok()).collect()
}

fn pair(s: &str) -> Option<P2> {
    nums(s).filter(|v| v.len() == 2).map(|v| [v[0], v[1]])
}

pub fn parse_shape(op: &Op, args: &[String]) -> Result<Shape, String> {
    let kind = args
        .first()
        .ok_or_else(|| op.err(format!("{} needs a shape", op.verb)))?;
    let rest: Result<Vec<f64>, _> = args[1..].iter().map(|a| a.parse::<f64>()).collect();
    match kind.as_str() {
        "rect" => {
            let v = rest.map_err(|_| op.err("rect takes numbers: rect X Y W H"))?;
            if v.len() != 4 || v[2] <= 0.0 || v[3] <= 0.0 {
                return Err(op.err("rect X Y W H with positive size"));
            }
            Ok(Shape::Rect {
                x: v[0],
                y: v[1],
                w: v[2],
                h: v[3],
            })
        }
        "circle" => {
            let v = rest.map_err(|_| op.err("circle takes numbers: circle CX CY R"))?;
            if v.len() != 3 || v[2] <= 0.0 {
                return Err(op.err("circle CX CY R with positive radius"));
            }
            Ok(Shape::Circle {
                cx: v[0],
                cy: v[1],
                r: v[2],
            })
        }
        "poly" => {
            let pts: Option<Vec<P2>> = args[1..].iter().map(|a| pair(a)).collect();
            match pts {
                Some(p) if p.len() >= 3 => Ok(Shape::Poly(p)),
                _ => Err(op.err("poly X,Y X,Y X,Y ... needs three or more points")),
            }
        }
        other => Err(op.err(format!("unknown shape {other}"))),
    }
}

pub fn parse_points(op: &Op, args: &[String]) -> Result<Vec<P2>, String> {
    let pts: Option<Vec<P2>> = args.iter().map(|a| pair(a)).collect();
    pts.filter(|p| !p.is_empty())
        .ok_or_else(|| op.err("expected X,Y points"))
}

pub const OPS: &[&str] = &[
    "face", "pocket", "adaptive", "profile", "drill", "chamfer", "engrave", "cut", "raster",
];

pub fn parse(src: &str) -> Result<Job, String> {
    let mut stock = None;
    let mut material = materials::find("aluminium").unwrap();
    let mut machine = Machine {
        rapid: 5000.0,
        max_rpm: 24000.0,
        clearance: 5.0,
    };
    let mut tools = BTreeMap::new();
    let mut ops = Vec::new();
    let mut part = None;
    let mut grid = 0.25;
    let mut tol = 0.05;

    for (i, raw) in src.lines().enumerate() {
        let line = i + 1;
        let text = raw.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        let mut args = Vec::new();
        let mut kv = BTreeMap::new();
        let mut words = text.split_whitespace();
        let verb = words.next().unwrap().to_string();
        for w in words {
            match w.split_once('=') {
                Some((k, v)) => {
                    kv.insert(k.to_string(), v.to_string());
                }
                None => args.push(w.to_string()),
            }
        }
        let op = Op {
            line,
            verb: verb.clone(),
            args,
            kv,
        };
        match verb.as_str() {
            "units" => {
                if op.args.first().map(String::as_str) != Some("mm") {
                    return Err(op.err("only units mm"));
                }
            }
            "stock" => {
                let v: Result<Vec<f64>, _> = op.args.iter().map(|a| a.parse::<f64>()).collect();
                match v {
                    Ok(v) if v.len() == 3 && v.iter().all(|x| *x > 0.0) => {
                        stock = Some([v[0], v[1], v[2]])
                    }
                    _ => return Err(op.err("stock W D H with positive sizes")),
                }
                if let Some(m) = op.text("material") {
                    material = materials::find(m)
                        .ok_or_else(|| op.err(format!("unknown material {m}")))?;
                }
            }
            "material" => {
                let name = op.args.first().ok_or_else(|| op.err("material NAME"))?;
                material = materials::find(name)
                    .ok_or_else(|| op.err(format!("unknown material {name}")))?;
            }
            "machine" => {
                machine.rapid = op.or("rapid", machine.rapid)?;
                machine.max_rpm = op.or("max_rpm", machine.max_rpm)?;
                machine.clearance = op.or("clearance", machine.clearance)?;
                grid = op.or("grid", grid)?;
                tol = op.or("tol", tol)?;
                if grid <= 0.0 || machine.rapid <= 0.0 || machine.clearance < 2.0 {
                    return Err(op.err("grid and rapid must be positive, clearance at least 2"));
                }
            }
            "tool" => {
                let n: u32 = op
                    .args
                    .first()
                    .and_then(|a| a.parse().ok())
                    .ok_or_else(|| op.err("tool N KIND DIA"))?;
                let kind_word = op
                    .args
                    .get(1)
                    .ok_or_else(|| op.err("tool N KIND DIA"))?
                    .as_str();
                let dia: f64 = op
                    .args
                    .get(2)
                    .and_then(|a| a.parse().ok())
                    .ok_or_else(|| op.err("tool N KIND DIA"))?;
                if dia <= 0.0 {
                    return Err(op.err("tool diameter must be positive"));
                }
                let kind = match kind_word {
                    "endmill" => ToolKind::Flat,
                    "ball" => ToolKind::Ball,
                    "vbit" => ToolKind::VBit(op.or("angle", 90.0)?),
                    "laser" => ToolKind::Laser,
                    other => return Err(op.err(format!("unknown tool kind {other}"))),
                };
                tools.insert(
                    n,
                    Tool {
                        n,
                        kind,
                        dia,
                        flutes: op.or("flutes", 2.0)? as u32,
                        flute_len: op.or("flute_len", dia * 3.0)?,
                        holder_dia: op.or("holder", dia * 3.0)?,
                    },
                );
            }
            "part" => {
                let path = op.args.first().ok_or_else(|| op.err("part FILE"))?.clone();
                let at = match op.text("at") {
                    Some(s) => nums(s)
                        .filter(|v| v.len() == 3)
                        .map(|v| [v[0], v[1], v[2]])
                        .ok_or_else(|| op.err("at=X,Y,Z"))?,
                    None => [0.0, 0.0, 0.0],
                };
                part = Some(PartRef { path, at });
            }
            v if OPS.contains(&v) => ops.push(op),
            other => return Err(op.err(format!("unknown operation {other}"))),
        }
    }
    let stock = stock.ok_or("the file needs a stock line: stock W D H")?;
    if ops.is_empty() {
        return Err("the file has no operations".into());
    }
    Ok(Job {
        stock,
        material,
        machine,
        tools,
        ops,
        part,
        grid,
        tol,
    })
}
