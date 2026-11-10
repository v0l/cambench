use crate::geom::{self, P2, Shape};
use crate::materials::{self, Cutting};
use crate::model::{Job, Op, Tool, ToolKind, parse_points, parse_shape};

pub type P3 = [f64; 3];

const RAPID_PLANE: f64 = 1.0;
const SAMPLE: f64 = 0.05;
const POINT_STEP: f64 = 0.5;

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Tool { n: u32, rpm: f64 },
    Rapid(P3),
    Feed(P3, f64),
    Laser(f64),
    End,
}

#[derive(Clone, Debug)]
pub enum Allowed {
    Region {
        poly: Vec<P2>,
        outside: bool,
        grow: f64,
    },
    Band {
        path: Vec<P2>,
        closed: bool,
        dist: f64,
    },
    Points {
        pts: Vec<P2>,
        dist: f64,
    },
}

#[derive(Clone, Debug)]
pub struct Floor {
    pub poly: Vec<P2>,
    pub inset: f64,
    pub z: f64,
}

#[derive(Clone, Debug, Default)]
pub struct Intent {
    pub allowed: Vec<Allowed>,
    pub floors: Vec<Floor>,
    pub through: Vec<Vec<P2>>,
    pub laser_spot: f64,
    pub laser_depth_only: bool,
}

#[derive(Clone, Debug)]
pub struct OpPlan {
    pub line: usize,
    pub verb: String,
    pub tool: Tool,
    pub rpm: f64,
    pub items: Vec<Item>,
    pub intent: Intent,
}

#[derive(Clone, Debug)]
pub struct Program {
    pub stock: [f64; 3],
    pub clearance: f64,
    pub rapid: f64,
    pub laser_ratio: f64,
    pub ops: Vec<OpPlan>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Move {
    pub rapid: bool,
    pub to: P3,
    pub feed: f64,
    pub tool: u32,
    pub s: f64,
}

impl Program {
    pub fn items(&self) -> Vec<Item> {
        let mut out = vec![Item::Rapid([0.0, 0.0, self.clearance])];
        let mut tool = None;
        for op in &self.ops {
            if tool != Some(op.tool.n) {
                if tool.is_some() {
                    out.push(Item::Laser(0.0));
                }
                out.push(Item::Tool {
                    n: op.tool.n,
                    rpm: op.rpm,
                });
                tool = Some(op.tool.n);
            }
            out.extend(op.items.iter().cloned());
        }
        out.push(Item::End);
        let mut pos = [f64::NAN; 3];
        out.retain(|it| match it {
            Item::Rapid(p) | Item::Feed(p, _) => {
                let same = (0..3).all(|i| (p[i] * 1000.0).round() == (pos[i] * 1000.0).round());
                pos = *p;
                !same
            }
            _ => true,
        });
        out
    }

    pub fn moves(&self) -> Vec<Move> {
        let mut out = Vec::new();
        let (mut tool, mut s, mut feed) = (0, 0.0, 0.0);
        for it in self.items() {
            match it {
                Item::Tool { n, rpm } => {
                    tool = n;
                    s = rpm;
                }
                Item::Laser(p) => s = p,
                Item::End => s = 0.0,
                Item::Rapid(to) => out.push(Move {
                    rapid: true,
                    to,
                    feed,
                    tool,
                    s,
                }),
                Item::Feed(to, f) => {
                    feed = f;
                    out.push(Move {
                        rapid: false,
                        to,
                        feed,
                        tool,
                        s,
                    });
                }
            }
        }
        out
    }

    pub fn op_of_moves(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut seen = 0;
        for k in 0..self.ops.len() {
            let prefix = Program {
                stock: self.stock,
                clearance: self.clearance,
                rapid: self.rapid,
                laser_ratio: self.laser_ratio,
                ops: self.ops[..=k].to_vec(),
            };
            let n = prefix.moves().len();
            out.extend(std::iter::repeat_n(k, n.saturating_sub(seen)));
            seen = n;
        }
        out
    }

    pub fn move_seconds(&self) -> Vec<f64> {
        let mut pos = [0.0, 0.0, self.clearance];
        let mut t = 0.0;
        self.moves()
            .iter()
            .map(|m| {
                let len = dist3(pos, m.to);
                t += if m.rapid {
                    len / self.rapid
                } else {
                    len / m.feed.max(1.0)
                } * 60.0;
                pos = m.to;
                t
            })
            .collect()
    }

    pub fn estimate_seconds(&self) -> f64 {
        let mut pos = [0.0; 3];
        let mut t = 0.0;
        let mut last_tool = None;
        for op in &self.ops {
            if last_tool != Some(op.tool.n) {
                t += 10.0;
                last_tool = Some(op.tool.n);
            }
            for it in &op.items {
                match it {
                    Item::Rapid(p) => {
                        t += dist3(pos, *p) / self.rapid * 60.0;
                        pos = *p;
                    }
                    Item::Feed(p, f) => {
                        t += dist3(pos, *p) / f * 60.0;
                        pos = *p;
                    }
                    _ => {}
                }
            }
        }
        t
    }
}

pub fn dist3(a: P3, b: P3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

struct Gen {
    items: Vec<Item>,
    pos: P3,
    clearance: f64,
}

impl Gen {
    fn rapid(&mut self, p: P3) {
        if p != self.pos {
            self.items.push(Item::Rapid(p));
            self.pos = p;
        }
    }

    fn feed(&mut self, p: P3, f: f64) {
        if p != self.pos {
            self.items.push(Item::Feed(p, f));
            self.pos = p;
        }
    }

    fn retract(&mut self) {
        let z = self.pos[2].max(RAPID_PLANE);
        let p = [self.pos[0], self.pos[1], z.max(RAPID_PLANE)];
        self.rapid(p);
    }

    fn lift(&mut self) {
        let p = [self.pos[0], self.pos[1], self.clearance];
        self.rapid(p);
    }

    fn approach(&mut self, xy: P2, z: f64, plunge: f64) {
        self.lift();
        self.rapid([xy[0], xy[1], self.clearance]);
        self.rapid([xy[0], xy[1], RAPID_PLANE]);
        self.feed([xy[0], xy[1], z], plunge);
    }
}

fn levels(depth: f64, step: f64) -> Vec<f64> {
    let n = (depth / step).ceil().max(1.0) as usize;
    (1..=n).map(|i| -depth * i as f64 / n as f64).collect()
}

fn resample(pts: &[P2], closed: bool) -> Vec<(P2, f64)> {
    let mut out = Vec::new();
    let mut walked = 0.0;
    let n = if closed { pts.len() } else { pts.len() - 1 };
    for i in 0..n {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        let l = geom::dist(a, b);
        let k = ((l / POINT_STEP).ceil() as usize).max(1);
        for j in 0..k {
            let t = j as f64 / k as f64;
            out.push((
                [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
                walked + l * t,
            ));
        }
        walked += l;
    }
    if !closed {
        out.push((*pts.last().unwrap(), walked));
    } else {
        out.push((pts[0], walked));
    }
    out
}

fn centroid(pts: &[P2]) -> P2 {
    let n = pts.len() as f64;
    [
        pts.iter().map(|p| p[0]).sum::<f64>() / n,
        pts.iter().map(|p| p[1]).sum::<f64>() / n,
    ]
}

fn valid(base: &[P2], r: f64, p: P2) -> bool {
    geom::point_in_poly(base, p) && geom::dist_to_closed(base, p) >= r - 1e-6
}

fn zigzag_runs(base: &[P2], r: f64, stepover: f64) -> Vec<(P2, P2)> {
    let (lo, hi) = geom::bounds_of(base);
    let (y0, y1) = (lo[1] + r, hi[1] - r);
    if y1 < y0 - 1e-9 {
        return vec![];
    }
    let rows = (((y1 - y0) / stepover).ceil() as usize).max(1);
    let mut out = Vec::new();
    let mut flip = false;
    for k in 0..=rows {
        let y = if y1 - y0 < 1e-9 {
            y0
        } else {
            y0 + (y1 - y0) * k as f64 / rows as f64
        };
        let mut runs: Vec<(P2, P2)> = Vec::new();
        let mut start: Option<f64> = None;
        let mut x = lo[0];
        let mut last_ok = lo[0];
        while x <= hi[0] + SAMPLE {
            let ok = valid(base, r, [x, y]);
            match (ok, start) {
                (true, None) => start = Some(x),
                (false, Some(s)) => {
                    runs.push(([s, y], [last_ok, y]));
                    start = None;
                }
                _ => {}
            }
            if ok {
                last_ok = x;
            }
            x += SAMPLE;
        }
        if let Some(s) = start {
            runs.push(([s, y], [last_ok, y]));
        }
        if flip {
            runs.reverse();
            for r in runs.iter_mut() {
                *r = (r.1, r.0);
            }
        }
        out.extend(runs);
        flip = !flip;
        if y1 - y0 < 1e-9 {
            break;
        }
    }
    out
}

fn shape_arg(op: &Op) -> Result<(Shape, usize), String> {
    let shape = parse_shape(op, &op.args)?;
    let used = match op.args[0].as_str() {
        "rect" => 5,
        "circle" => 4,
        _ => op.args.len(),
    };
    Ok((shape, used))
}

fn pick_tool(job: &Job, op: &Op) -> Result<Tool, String> {
    let n = match op.num("tool")? {
        Some(n) => n as u32,
        None => *job
            .tools
            .keys()
            .next()
            .ok_or_else(|| op.err("no tool defined"))?,
    };
    job.tools
        .get(&n)
        .cloned()
        .ok_or_else(|| op.err(format!("tool {n} is not defined")))
}

fn contour_at(shape: &Shape, off: f64) -> Vec<P2> {
    shape.contour(off)
}

fn cutting(job: &Job, op: &Op, tool: &Tool) -> Result<Cutting, String> {
    let mut c = materials::cutting(&job.material, tool.dia, tool.flutes, job.machine.max_rpm);
    c.feed = op.or("feed", c.feed)?;
    c.plunge = op.or("plunge", c.plunge)?;
    c.rpm = op.or("rpm", c.rpm)?;
    c.stepdown = op.or("stepdown", c.stepdown)?;
    c.stepover = op.or("stepover", c.stepover)?;
    if c.feed <= 0.0 || c.plunge <= 0.0 || c.stepdown <= 0.0 || c.stepover <= 0.0 || c.rpm <= 0.0 {
        return Err(op.err("feeds, stepdown, stepover and rpm must be positive"));
    }
    Ok(c)
}

fn depth_arg(job: &Job, op: &Op, allow_over: bool) -> Result<f64, String> {
    let d = op.need("depth")?;
    let max = job.stock[2] + if allow_over { 1.0 } else { 0.0 };
    if d <= 0.0 || d > max + 1e-9 {
        return Err(op.err(format!("depth {d} must be above 0 and at most {max}")));
    }
    Ok(d)
}

fn grow(job: &Job) -> f64 {
    job.tol.max(job.grid * 1.5)
}

fn clear_runs(g: &mut Gen, runs: &[(P2, P2)], z: f64, c: &Cutting, link: f64, base: &[P2], r: f64) {
    for (a, b) in runs {
        let linked = g.pos[2] == z
            && geom::dist([g.pos[0], g.pos[1]], *a) <= link
            && valid(base, r, [(g.pos[0] + a[0]) / 2.0, (g.pos[1] + a[1]) / 2.0]);
        if linked {
            g.feed([a[0], a[1], z], c.feed);
        } else {
            g.approach(*a, z, c.plunge);
        }
        g.feed([b[0], b[1], z], c.feed);
    }
}

fn ring_pass(g: &mut Gen, ring: &[P2], z: f64, c: &Cutting) {
    let pts = resample(ring, true);
    let first = pts[0].0;
    let near = geom::dist([g.pos[0], g.pos[1]], first) < 2.0 * c.stepover && g.pos[2] == z;
    if near {
        g.feed([first[0], first[1], z], c.feed);
    } else {
        g.approach(first, z, c.plunge);
    }
    for (p, _) in &pts[1..] {
        g.feed([p[0], p[1], z], c.feed);
    }
}

fn plan_op(job: &Job, op: &Op) -> Result<OpPlan, String> {
    let tool = pick_tool(job, op)?;
    let laser_verb = op.verb == "cut" || op.verb == "raster";
    if laser_verb != (tool.kind == ToolKind::Laser) {
        return Err(op.err(format!(
            "{} needs a {} tool",
            op.verb,
            if laser_verb { "laser" } else { "milling" }
        )));
    }
    let clearance = job.machine.clearance;
    let mut g = Gen {
        items: Vec::new(),
        pos: [0.0, 0.0, clearance],
        clearance,
    };
    let mut intent = Intent::default();
    let mut rpm = 0.0;
    let r = tool.radius();
    let gr = grow(job);
    let [sw, sd, sh] = job.stock;
    let stock_rect = Shape::Rect {
        x: 0.0,
        y: 0.0,
        w: sw,
        h: sd,
    };

    match op.verb.as_str() {
        "face" => {
            let depth = op.or("depth", 0.5)?;
            if depth <= 0.0 || depth > sh {
                return Err(op.err("face depth must be positive and within the stock"));
            }
            let c = cutting(job, op, &tool)?;
            rpm = c.rpm;
            let rows = ((sd / c.stepover).ceil() as usize).max(1);
            for z in levels(depth, c.stepdown) {
                for k in 0..=rows {
                    let y = sd * k as f64 / rows as f64;
                    let (x0, x1) = if k % 2 == 0 {
                        (-r, sw + r)
                    } else {
                        (sw + r, -r)
                    };
                    if k == 0 {
                        g.approach([x0, y], z, c.plunge);
                    } else {
                        g.feed([x0, y, z], c.feed);
                    }
                    g.feed([x1, y, z], c.feed);
                }
            }
            g.lift();
            intent.floors.push(Floor {
                poly: stock_rect.contour(0.0),
                inset: 0.0,
                z: -depth,
            });
        }
        "pocket" | "adaptive" => {
            let (shape, _) = shape_arg(op)?;
            let depth = depth_arg(job, op, false)?;
            let mut c = cutting(job, op, &tool)?;
            let adaptive = op.verb == "adaptive";
            if adaptive {
                c.stepover = op.or("stepover", 0.15 * tool.dia)?;
                c.stepdown = op.or("stepdown", (tool.flute_len * 0.8).max(tool.dia))?;
            }
            rpm = c.rpm;
            let base = contour_at(&shape, 0.0);
            let rings = adaptive
                .then(|| ring_offsets(&shape, r, c.stepover))
                .flatten();
            let runs = if rings.is_none() {
                zigzag_runs(&base, r, c.stepover)
            } else {
                vec![]
            };
            if rings.is_none() && runs.is_empty() {
                return Err(op.err(format!(
                    "tool {} of diameter {} does not fit the pocket",
                    tool.n, tool.dia
                )));
            }
            for z in levels(depth, c.stepdown) {
                match &rings {
                    Some(list) => {
                        for ring in list {
                            ring_pass(&mut g, ring, z, &c);
                        }
                    }
                    None => {
                        clear_runs(&mut g, &runs, z, &c, c.stepover * 1.5, &base, r);
                        let finish = contour_at(&shape, -r);
                        ring_pass(&mut g, &finish, z, &c);
                    }
                }
                g.retract();
            }
            g.lift();
            intent.allowed.push(Allowed::Region {
                poly: base.clone(),
                outside: false,
                grow: gr,
            });
            if tool.kind == ToolKind::Flat {
                intent.floors.push(Floor {
                    poly: base,
                    inset: r + gr,
                    z: -depth,
                });
            }
        }
        "profile" => {
            let (shape, used) = shape_arg(op)?;
            let _ = used;
            let depth = depth_arg(job, op, true)?;
            let c = cutting(job, op, &tool)?;
            rpm = c.rpm;
            let side = op.text("side").unwrap_or("outside");
            let off = match side {
                "outside" => r,
                "inside" => -r,
                "on" => 0.0,
                other => return Err(op.err(format!("side={other} must be outside, inside or on"))),
            };
            let ring = contour_at(&shape, off);
            let lead = op.or("lead", 0.0)?;
            let tabs = op.or("tabs", 0.0)? as usize;
            let tab_w = op.or("tab_w", tool.dia + 4.0)?;
            let tab_h = op.or("tab_h", 1.5)?;
            let pts = resample(&ring, true);
            let total = pts.last().unwrap().1;
            let tab_top = -depth + tab_h;
            let in_tab = |s: f64| {
                tabs > 0
                    && (0..tabs).any(|k| {
                        let centre = total * (k as f64 + 0.5) / tabs as f64;
                        (s - centre).abs() <= tab_w / 2.0
                    })
            };
            let centre = centroid(&ring);
            let start = pts[0].0;
            let away = {
                let v = [start[0] - centre[0], start[1] - centre[1]];
                let l = v[0].hypot(v[1]).max(1e-9);
                let sign = if side == "inside" { -1.0 } else { 1.0 };
                [sign * v[0] / l, sign * v[1] / l]
            };
            let lead_start = [start[0] + away[0] * lead, start[1] + away[1] * lead];
            for z in levels(depth, c.stepdown) {
                g.approach(lead_start, z, c.plunge);
                for (p, s) in &pts {
                    let zz = if in_tab(*s) { z.max(tab_top) } else { z };
                    g.feed([p[0], p[1], zz], c.feed);
                }
                g.feed(
                    [
                        start[0],
                        start[1],
                        z.max(if in_tab(0.0) { tab_top } else { z }),
                    ],
                    c.feed,
                );
                g.retract();
            }
            g.lift();
            let base = contour_at(&shape, 0.0);
            match side {
                "outside" => intent.allowed.push(Allowed::Region {
                    poly: base,
                    outside: true,
                    grow: gr,
                }),
                "inside" => intent.allowed.push(Allowed::Region {
                    poly: base,
                    outside: false,
                    grow: gr,
                }),
                _ => {}
            }
            intent.allowed.push(Allowed::Band {
                path: ring,
                closed: true,
                dist: r + gr + lead,
            });
        }
        "drill" => {
            let pts = parse_points(op, &op.args)?;
            let depth = depth_arg(job, op, true)?;
            let c = cutting(job, op, &tool)?;
            rpm = c.rpm;
            let peck = op.or("peck", (tool.dia * 3.0).max(1.0))?;
            for p in &pts {
                g.lift();
                g.rapid([p[0], p[1], g.clearance]);
                g.rapid([p[0], p[1], RAPID_PLANE]);
                for z in levels(depth, peck) {
                    g.feed([p[0], p[1], z], c.plunge);
                    g.rapid([p[0], p[1], RAPID_PLANE]);
                    if z > -depth + 1e-9 {
                        g.rapid([p[0], p[1], (z + 0.5).min(RAPID_PLANE)]);
                    }
                }
                if tool.kind == ToolKind::Flat && r > 1.5 * gr {
                    let dz = Shape::Circle {
                        cx: p[0],
                        cy: p[1],
                        r,
                    };
                    intent.floors.push(Floor {
                        poly: dz.contour(0.0),
                        inset: gr,
                        z: -depth.min(sh),
                    });
                }
            }
            g.lift();
            intent.allowed.push(Allowed::Points {
                pts: pts.clone(),
                dist: r + gr,
            });
        }
        "chamfer" | "engrave" => {
            let (path, closed, shape_opt) = if op.verb == "chamfer" {
                let (shape, _) = shape_arg(op)?;
                let ring = contour_at(&shape, op.or("offset", 0.0)?);
                (ring, true, Some(shape))
            } else {
                (
                    parse_points(op, &op.args)?,
                    op.text("closed") == Some("1"),
                    None,
                )
            };
            let _ = shape_opt;
            let half = match tool.kind {
                ToolKind::VBit(a) => (a / 2.0).to_radians().tan(),
                _ => 0.0,
            };
            let depth = if op.verb == "chamfer" {
                if half == 0.0 {
                    return Err(op.err("chamfer needs a vbit tool"));
                }
                op.need("width")? / half
            } else if let Some(w) = op.num("width")? {
                if half == 0.0 {
                    return Err(op.err("width= needs a vbit tool, use depth="));
                }
                w / 2.0 / half
            } else {
                op.need("depth")?
            };
            if depth <= 0.0 || depth > sh {
                return Err(op.err(format!(
                    "depth {depth:.3} must be above 0 and within the stock"
                )));
            }
            if matches!(tool.kind, ToolKind::VBit(_)) {
                let cone = depth * half;
                if cone > r + 1e-9 {
                    return Err(op.err(format!(
                        "the cut needs a {cone:.2} mm radius but the tool has {r}"
                    )));
                }
            }
            let c = cutting(job, op, &tool)?;
            rpm = c.rpm;
            let pts = resample(&path, closed);
            for z in levels(depth, c.stepdown) {
                g.approach(pts[0].0, z, c.plunge);
                for (p, _) in &pts[1..] {
                    g.feed([p[0], p[1], z], c.feed);
                }
                g.retract();
            }
            g.lift();
            intent.allowed.push(Allowed::Band {
                path,
                closed,
                dist: r + gr,
            });
        }
        "cut" | "raster" => {
            plan_laser(job, op, &tool, &mut g, &mut intent, &mut rpm)?;
        }
        other => return Err(op.err(format!("no planner for {other}"))),
    }
    Ok(OpPlan {
        line: op.line,
        verb: op.verb.clone(),
        tool,
        rpm,
        items: g.items,
        intent,
    })
}

fn ring_offsets(shape: &Shape, r: f64, step: f64) -> Option<Vec<Vec<P2>>> {
    let limit = match shape {
        Shape::Rect { w, h, .. } => w.min(*h) / 2.0,
        Shape::Circle { r, .. } => *r,
        _ => return None,
    };
    if limit < r - 1e-9 {
        return None;
    }
    let mut out = Vec::new();
    let mut inset = r;
    while inset < limit - 1e-9 {
        out.push(shape.contour(-inset));
        inset += step;
    }
    out.push(shape.contour(-limit));
    Some(out)
}

fn plan_laser(
    job: &Job,
    op: &Op,
    tool: &Tool,
    g: &mut Gen,
    intent: &mut Intent,
    rpm: &mut f64,
) -> Result<(), String> {
    let h = job.stock[2];
    let m = &job.material;
    if m.laser_power <= 0.0 {
        return Err(op.err(format!("material {} has no laser settings", m.name)));
    }
    let ref_ratio = m.laser_power / m.laser_feed;
    let kerf = op.or("kerf", 0.15)?;
    let feed_default = (m.laser_feed * 3.0 / h).clamp(50.0, 6000.0);
    let feed = op.or("feed", feed_default)?;
    let passes = op.or("passes", 1.0)?.max(1.0) as usize;
    let _ = tool;
    if feed <= 0.0 {
        return Err(op.err("feed must be positive"));
    }
    *rpm = 0.0;
    if op.verb == "cut" {
        let power = op.or("power", m.laser_power)?.clamp(0.0, 1000.0);
        let (shape, _) = shape_arg(op)?;
        let side = op.text("side").unwrap_or("outside");
        let off = match side {
            "outside" => kerf / 2.0,
            "inside" => -kerf / 2.0,
            "on" => 0.0,
            other => return Err(op.err(format!("side={other} must be outside, inside or on"))),
        };
        let ring = shape.contour(off);
        let pts = resample(&ring, true);
        g.rapid([pts[0].0[0], pts[0].0[1], 0.0]);
        for _ in 0..passes {
            g.items.push(Item::Laser(power));
            for (p, _) in &pts[1..] {
                g.feed([p[0], p[1], 0.0], feed);
            }
            g.items.push(Item::Laser(0.0));
        }
        intent.allowed.push(Allowed::Band {
            path: ring.clone(),
            closed: true,
            dist: kerf + job.grid * 2.0,
        });
        intent.through.push(ring);
        intent.laser_spot = (kerf / 2.0).max(job.grid * 0.71);
    } else {
        let (shape, _) = shape_arg(op)?;
        let line = op.or("line", 0.1)?;
        if line <= 0.0 {
            return Err(op.err("line must be positive"));
        }
        let depth = op.or("depth", 0.2)?;
        let power = match op.num("power")? {
            Some(p) => p.clamp(0.0, 1000.0),
            None => (depth * ref_ratio / 3.0 * feed).clamp(1.0, 1000.0),
        };
        let base = shape.contour(0.0);
        let (lo, hi) = shape.bounds();
        let rows = (((hi[1] - lo[1]) / line).floor() as usize).max(1);
        for k in 0..=rows {
            let y = lo[1] + line * k as f64;
            let mut runs: Vec<(f64, f64)> = Vec::new();
            let mut start: Option<f64> = None;
            let mut x = lo[0];
            let mut last = x;
            while x <= hi[0] + 1e-9 {
                let ok = geom::point_in_poly(&base, [x, y]) || shape.contains([x, y]);
                match (ok, start) {
                    (true, None) => start = Some(x),
                    (false, Some(s)) => {
                        runs.push((s, last));
                        start = None;
                    }
                    _ => {}
                }
                if ok {
                    last = x;
                }
                x += SAMPLE;
            }
            if let Some(s) = start {
                runs.push((s, last));
            }
            if k % 2 == 1 {
                runs.reverse();
                for r in runs.iter_mut() {
                    *r = (r.1, r.0);
                }
            }
            for (a, b) in runs {
                if (a - b).abs() < 1e-9 {
                    continue;
                }
                g.rapid([a, y, 0.0]);
                g.items.push(Item::Laser(power));
                g.feed([b, y, 0.0], feed);
                g.items.push(Item::Laser(0.0));
            }
        }
        intent.laser_spot = (line / 2.0).max(job.grid * 0.71);
        intent.laser_depth_only = true;
        intent.allowed.push(Allowed::Region {
            poly: base,
            outside: false,
            grow: grow(job),
        });
    }
    Ok(())
}

pub fn plan(job: &Job) -> Result<Program, String> {
    let mut ops = Vec::new();
    for op in &job.ops {
        ops.push(plan_op(job, op)?);
    }
    let laser_ratio = if job.material.laser_feed > 0.0 {
        job.material.laser_power / job.material.laser_feed
    } else {
        1.0
    };
    Ok(Program {
        stock: job.stock,
        clearance: job.machine.clearance,
        rapid: job.machine.rapid,
        laser_ratio,
        ops,
    })
}
