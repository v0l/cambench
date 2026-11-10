use crate::geom::{self, P2};
use crate::model::ToolKind;
use crate::toolpath::{Allowed, Intent, Item, Move, OpPlan, P3, Program};

pub struct Stock {
    pub nx: usize,
    pub ny: usize,
    pub grid: f64,
    pub top: f32,
    pub bottom: f32,
    pub h: Vec<f32>,
}

impl Stock {
    pub fn new(size: [f64; 3], grid: f64) -> Stock {
        let nx = (size[0] / grid).ceil() as usize;
        let ny = (size[1] / grid).ceil() as usize;
        Stock {
            nx,
            ny,
            grid,
            top: 0.0,
            bottom: -size[2] as f32,
            h: vec![0.0; nx * ny],
        }
    }

    pub fn centre(&self, i: usize, j: usize) -> P2 {
        [(i as f64 + 0.5) * self.grid, (j as f64 + 0.5) * self.grid]
    }

    fn range(&self, c: f64, r: f64, n: usize) -> (usize, usize) {
        let lo = ((c - r) / self.grid - 0.5).ceil().max(0.0) as usize;
        let hi = ((c + r) / self.grid - 0.5).floor();
        if hi < 0.0 {
            return (1, 0);
        }
        (lo, (hi as usize).min(n.saturating_sub(1)))
    }

    pub fn cells_in(&self, x: f64, y: f64, r: f64, mut f: impl FnMut(usize, f64)) {
        if self.nx == 0 || self.ny == 0 {
            return;
        }
        let (i0, i1) = self.range(x, r, self.nx);
        let (j0, j1) = self.range(y, r, self.ny);
        if i0 > i1 || j0 > j1 {
            return;
        }
        for j in j0..=j1 {
            for i in i0..=i1 {
                let c = self.centre(i, j);
                let d = (c[0] - x).hypot(c[1] - y);
                if d <= r {
                    f(j * self.nx + i, d);
                }
            }
        }
    }

    pub fn removed_volume(&self) -> f64 {
        self.h.iter().map(|h| (self.top - h) as f64).sum::<f64>() * self.grid * self.grid
    }
}

#[derive(Default, Debug, Clone)]
pub struct Findings {
    pub gouges: Vec<String>,
    pub remaining: Vec<String>,
    pub rapid_hits: Vec<String>,
    pub holder_hits: Vec<String>,
    pub laser_faults: Vec<String>,
    pub depth_faults: Vec<String>,
}

impl Findings {
    pub fn ok(&self) -> bool {
        self.count() == 0
    }

    pub fn count(&self) -> usize {
        self.gouges.len()
            + self.remaining.len()
            + self.rapid_hits.len()
            + self.holder_hits.len()
            + self.laser_faults.len()
            + self.depth_faults.len()
    }

    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (kind, list) in [
            ("gouge", &self.gouges),
            ("remaining stock", &self.remaining),
            ("rapid through material", &self.rapid_hits),
            ("holder collision", &self.holder_hits),
            ("laser", &self.laser_faults),
            ("depth", &self.depth_faults),
        ] {
            for l in list {
                out.push(format!("{kind}: {l}"));
            }
        }
        out
    }
}

pub struct SimResult {
    pub stock: Stock,
    pub findings: Findings,
    pub removed_mm3: f64,
    pub seconds: f64,
}

fn tool_of(op: &OpPlan) -> (f64, f64, f64, f64, ToolKind) {
    (
        op.tool.radius(),
        op.tool.flute_len,
        op.tool.holder_dia / 2.0,
        0.0,
        op.tool.kind,
    )
}

struct Runner<'a> {
    stock: Stock,
    findings: Findings,
    laser_ratio: f64,
    clearance: f64,
    stamp: Vec<u32>,
    stamp_id: u32,
    laser_touched: Vec<usize>,
    laser_depth: f64,
    op: &'a OpPlan,
    limit: usize,
}

impl Runner<'_> {
    fn note(list: &mut Vec<String>, limit: usize, msg: String) {
        if list.len() < limit {
            list.push(msg);
        }
    }

    fn cut_at(&mut self, p: P3) {
        let (r, ..) = tool_of(self.op);
        let kind = self.op.tool.kind;
        let tool = &self.op.tool;
        let st = &mut self.stock;
        let h = &mut st.h as *mut Vec<f32>;
        st.cells_in(p[0], p[1], r, |idx, d| {
            let z = (p[2] + tool.profile(d)) as f32;
            let h = unsafe { &mut *h };
            if z < h[idx] {
                h[idx] = z;
            }
        });
        let _ = kind;
    }

    fn laser_at(&mut self, p: P3, spot: f64) {
        let st = &self.stock;
        let stamp = &mut self.stamp;
        let id = self.stamp_id;
        let touched = &mut self.laser_touched;
        st.cells_in(p[0], p[1], spot, |idx, _| {
            if stamp[idx] != id {
                stamp[idx] = id;
                touched.push(idx);
            }
        });
    }

    fn flush_laser(&mut self) {
        if self.laser_touched.is_empty() {
            return;
        }
        let depth = self.laser_depth as f32;
        for &idx in &self.laser_touched {
            let h = &mut self.stock.h[idx];
            *h = (*h - depth).max(self.stock.bottom);
        }
        self.laser_touched.clear();
        self.stamp_id += 1;
    }

    fn collides(&self, p: P3, with_holder: bool) -> Option<&'static str> {
        let (r, flute, hr, ..) = tool_of(self.op);
        let tool = &self.op.tool;
        let mut hit = None;
        self.stock.cells_in(p[0], p[1], r, |idx, d| {
            if self.stock.h[idx] as f64 > p[2] + tool.profile(d) + 1e-4 {
                hit = Some("tool");
            }
        });
        if hit.is_none() && with_holder && hr > r {
            let bottom = p[2] + flute;
            self.stock.cells_in(p[0], p[1], hr, |idx, _| {
                if self.stock.h[idx] as f64 > bottom + 1e-4 {
                    hit = Some("holder");
                }
            });
        }
        hit
    }
}

fn allowed(intent: &Intent, p: P2) -> bool {
    intent.allowed.iter().all(|a| match a {
        Allowed::Region {
            poly,
            outside,
            grow,
        } => {
            let inside = geom::point_in_poly(poly, p);
            let near = geom::dist_to_closed(poly, p) <= *grow;
            near || inside != *outside
        }
        Allowed::Points { pts, dist } => pts.iter().any(|q| geom::dist(*q, p) <= *dist),
        Allowed::Band { path, closed, dist } => {
            let d = if *closed {
                geom::dist_to_closed(path, p)
            } else {
                geom::dist_to_open(path, p)
            };
            d <= *dist
        }
    })
}

fn check_op(
    stock: &Stock,
    before: &[f32],
    op: &OpPlan,
    tol: f64,
    findings: &mut Findings,
    limit: usize,
) {
    let label = format!("line {} {}", op.line, op.verb);
    let mut bad = 0usize;
    let mut first = None;
    for j in 0..stock.ny {
        for i in 0..stock.nx {
            let idx = j * stock.nx + i;
            if before[idx] - stock.h[idx] > 1e-4 {
                let c = stock.centre(i, j);
                if !allowed(&op.intent, c) {
                    bad += 1;
                    first.get_or_insert((c, stock.h[idx]));
                }
            }
        }
    }
    if let Some((c, z)) = first {
        Runner::note(
            &mut findings.gouges,
            limit,
            format!(
                "{label} removed {bad} cells outside its intent, first at X{:.2} Y{:.2} Z{z:.2}",
                c[0], c[1]
            ),
        );
    }
    for f in &op.intent.floors {
        let (lo, hi) = geom::bounds_of(&f.poly);
        let (mut over, mut under) = (0usize, 0usize);
        let (mut over_at, mut under_at) = (None, None);
        for j in 0..stock.ny {
            for i in 0..stock.nx {
                let c = stock.centre(i, j);
                if c[0] < lo[0] || c[0] > hi[0] || c[1] < lo[1] || c[1] > hi[1] {
                    continue;
                }
                if !geom::point_in_poly(&f.poly, c) || geom::dist_to_closed(&f.poly, c) < f.inset {
                    continue;
                }
                let h = stock.h[j * stock.nx + i] as f64;
                if h < f.z - tol {
                    over += 1;
                    over_at.get_or_insert((c, h));
                } else if h > f.z + tol {
                    under += 1;
                    under_at.get_or_insert((c, h));
                }
            }
        }
        if let Some((c, h)) = over_at {
            Runner::note(
                &mut findings.gouges,
                limit,
                format!(
                    "{label} cut below the floor Z{:.3} on {over} cells, first at X{:.2} Y{:.2} Z{h:.3}",
                    f.z, c[0], c[1]
                ),
            );
        }
        if let Some((c, h)) = under_at {
            Runner::note(
                &mut findings.remaining,
                limit,
                format!(
                    "{label} left stock above the floor Z{:.3} on {under} cells, first at X{:.2} Y{:.2} Z{h:.3}",
                    f.z, c[0], c[1]
                ),
            );
        }
    }
    for path in &op.intent.through {
        let spot = &(op.intent.laser_spot * 0.8);
        let mut left = 0usize;
        let mut at = None;
        let bottom = stock.bottom as f64;
        for j in 0..stock.ny {
            for i in 0..stock.nx {
                let c = stock.centre(i, j);
                if geom::dist_to_closed(path, c) <= *spot
                    && (stock.h[j * stock.nx + i] as f64) > bottom + tol
                {
                    left += 1;
                    at.get_or_insert(c);
                }
            }
        }
        if let Some(c) = at {
            Runner::note(
                &mut findings.remaining,
                limit,
                format!(
                    "{label} did not cut through on {left} cells, first at X{:.2} Y{:.2}",
                    c[0], c[1]
                ),
            );
        }
    }
}

fn run_op(
    r: &mut Runner,
    op: &OpPlan,
    pos: &mut P3,
    state: &mut (f64, f64),
    seconds: &mut f64,
    rapid_speed: f64,
) {
    let step = r.stock.grid * 0.5;
    let is_laser = op.tool.kind == ToolKind::Laser;
    let mut burning = false;
    let mut feed_now = 0.0;
    for it in &op.items {
        match it {
            Item::Laser(p) => {
                if *p <= 0.0 {
                    r.flush_laser();
                }
                state.1 = *p;
                burning = *p > 0.0;
            }
            Item::Tool { .. } | Item::End => {}
            Item::Rapid(to) | Item::Feed(to, _) => {
                let rapid = matches!(it, Item::Rapid(_));
                if let Item::Feed(_, f) = it {
                    feed_now = *f;
                }
                let len = crate::toolpath::dist3(*pos, *to);
                *seconds += if rapid {
                    len / rapid_speed * 60.0
                } else {
                    len / feed_now * 60.0
                };
                let n = ((len / step).ceil() as usize).max(1);
                if is_laser {
                    if rapid && burning {
                        Runner::note(
                            &mut r.findings.laser_faults,
                            r.limit,
                            format!("line {} laser on during a rapid", op.line),
                        );
                    }
                    if !rapid && burning {
                        let depth = 3.0 * state.1 / (r.laser_ratio * feed_now);
                        r.laser_depth = depth;
                        for k in 0..=n {
                            let t = k as f64 / n as f64;
                            let p = [
                                pos[0] + (to[0] - pos[0]) * t,
                                pos[1] + (to[1] - pos[1]) * t,
                                0.0,
                            ];
                            r.laser_at(p, op.intent.laser_spot);
                        }
                    }
                } else {
                    for k in 1..=n {
                        let t = k as f64 / n as f64;
                        let p = [
                            pos[0] + (to[0] - pos[0]) * t,
                            pos[1] + (to[1] - pos[1]) * t,
                            pos[2] + (to[2] - pos[2]) * t,
                        ];
                        let hit = r.collides(p, true);
                        if rapid {
                            if let Some(what) = hit {
                                Runner::note(
                                    &mut r.findings.rapid_hits,
                                    r.limit,
                                    format!(
                                        "line {} {what} at X{:.2} Y{:.2} Z{:.2}",
                                        op.line, p[0], p[1], p[2]
                                    ),
                                );
                                break;
                            }
                        } else {
                            if p[2] < r.stock.bottom as f64 - 1.0 - 1e-6 {
                                Runner::note(
                                    &mut r.findings.depth_faults,
                                    r.limit,
                                    format!(
                                        "line {} tool tip at Z{:.2} is below the stock bottom",
                                        op.line, p[2]
                                    ),
                                );
                                break;
                            }
                            r.cut_at(p);
                            if let Some("holder") = r.collides(p, true) {
                                Runner::note(
                                    &mut r.findings.holder_hits,
                                    r.limit,
                                    format!(
                                        "line {} holder meets stock at X{:.2} Y{:.2} Z{:.2}, flute length {}",
                                        op.line, p[0], p[1], p[2], op.tool.flute_len
                                    ),
                                );
                                break;
                            }
                        }
                    }
                }
                *pos = *to;
            }
        }
    }
    r.flush_laser();
}

pub fn simulate(prog: &Program, grid: f64, tol: f64) -> SimResult {
    let mut runner_stock = Stock::new(prog.stock, grid);
    runner_stock.top = 0.0;
    let n = runner_stock.h.len();
    let first = &prog.ops[0];
    let mut runner = Runner {
        stock: runner_stock,
        findings: Findings::default(),
        laser_ratio: prog.laser_ratio,
        clearance: prog.clearance,
        stamp: vec![0; n],
        stamp_id: 1,
        laser_touched: Vec::new(),
        laser_depth: 0.0,
        op: first,
        limit: 5,
    };
    let _ = runner.clearance;
    let mut pos = [0.0, 0.0, prog.clearance];
    let mut state = (0.0, 0.0);
    let mut seconds = 0.0;
    for op in &prog.ops {
        runner.op = unsafe { &*(op as *const OpPlan) };
        let before = runner.stock.h.clone();
        run_op(
            &mut runner,
            op,
            &mut pos,
            &mut state,
            &mut seconds,
            prog.rapid,
        );
        let mut findings = std::mem::take(&mut runner.findings);
        check_op(&runner.stock, &before, op, tol, &mut findings, runner.limit);
        runner.findings = findings;
    }
    let removed = runner.stock.removed_volume();
    SimResult {
        stock: runner.stock,
        findings: runner.findings,
        removed_mm3: removed,
        seconds,
    }
}

pub fn moves_equal(a: &Move, b: &Move) -> bool {
    a.rapid == b.rapid
        && (a.tool == b.tool || a.tool == 0)
        && (0..3).all(|i| (a.to[i] - b.to[i]).abs() < 6e-4)
        && (a.rapid || (a.feed - b.feed).abs() < 0.06)
        && (a.s - b.s).abs() < 0.6
}
