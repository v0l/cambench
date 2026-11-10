use crate::toolpath::{Item, Move, P3, Program};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostKind {
    Grbl,
    LinuxCnc,
    Marlin,
    Fanuc,
}

pub const POSTS: &[(&str, PostKind)] = &[
    ("grbl", PostKind::Grbl),
    ("linuxcnc", PostKind::LinuxCnc),
    ("marlin", PostKind::Marlin),
    ("fanuc", PostKind::Fanuc),
];

pub fn post_by_name(name: &str) -> Option<PostKind> {
    POSTS.iter().find(|(n, _)| *n == name).map(|(_, k)| *k)
}

pub struct Post {
    pub header: &'static str,
    pub tool_change: &'static str,
    pub spindle_on: &'static str,
    pub spindle_off: &'static str,
    pub laser_on: &'static str,
    pub laser_off: &'static str,
    pub rapid: &'static str,
    pub feed: &'static str,
    pub footer: &'static str,
    pub numbered: bool,
    pub milling: bool,
}

impl PostKind {
    pub fn post(self) -> Post {
        match self {
            PostKind::Grbl => Post {
                header: "(cambench grbl)\nG21 G90 G17 G94",
                tool_change: "M5\n(change to tool {t})\nT{t}\nM0",
                spindle_on: "M3 S{s}",
                spindle_off: "M5",
                laser_on: "M4 S{s}",
                laser_off: "M5",
                rapid: "G0{xyz}",
                feed: "G1{xyz} F{f}",
                footer: "M5\nM2",
                numbered: false,
                milling: true,
            },
            PostKind::LinuxCnc => Post {
                header: "(cambench linuxcnc)\nG21 G90 G17 G40 G49 G54 G64 P0.01",
                tool_change: "M5\nT{t} M6\nG43 H{t}",
                spindle_on: "M3 S{s}",
                spindle_off: "M5",
                laser_on: "M3 S{s}",
                laser_off: "M5",
                rapid: "G0{xyz}",
                feed: "G1{xyz} F{f}",
                footer: "M5\nM2",
                numbered: false,
                milling: true,
            },
            PostKind::Marlin => Post {
                header: "; cambench marlin laser\nG21\nG90\nG17",
                tool_change: "M5",
                spindle_on: "",
                spindle_off: "M5",
                laser_on: "M3 S{s}",
                laser_off: "M5",
                rapid: "G0{xyz}",
                feed: "G1{xyz} F{f}",
                footer: "M5\nM84",
                numbered: false,
                milling: false,
            },
            PostKind::Fanuc => Post {
                header: "%\nO0001 (CAMBENCH)\nG21 G90 G17 G40 G49 G80\nG54",
                tool_change: "M5\nT{t} M6\nG43 H{t}",
                spindle_on: "M3 S{s}",
                spindle_off: "M5",
                laser_on: "M3 S{s}",
                laser_off: "M5",
                rapid: "G0{xyz}",
                feed: "G1{xyz} F{f}",
                footer: "M5\nM30\n%",
                numbered: true,
                milling: true,
            },
        }
    }
}

fn num(v: f64, places: usize) -> String {
    let s = format!("{v:.places$}");
    let s = if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    };
    if s == "-0" { "0".into() } else { s }
}

fn axes(prev: Option<P3>, to: P3) -> String {
    let mut out = String::new();
    for (i, name) in ["X", "Y", "Z"].iter().enumerate() {
        let changed = match prev {
            Some(p) => num(p[i], 3) != num(to[i], 3),
            None => true,
        };
        if changed {
            out.push_str(&format!(" {name}{}", num(to[i], 3)));
        }
    }
    out
}

fn fill(t: &str, xyz: &str, f: &str, s: &str, tool: &str) -> String {
    t.replace("{xyz}", xyz)
        .replace("{f}", f)
        .replace("{s}", s)
        .replace("{t}", tool)
}

const ARC_TOL: f64 = 3e-3;
const MIN_ARC_POINTS: usize = 3;
const MAX_RADIUS: f64 = 5000.0;

struct ArcFit {
    end: P3,
    center: [f64; 2],
    cw: bool,
    feed: f64,
    count: usize,
}

fn round3(p: P3) -> P3 {
    [
        (p[0] * 1000.0).round() / 1000.0,
        (p[1] * 1000.0).round() / 1000.0,
        (p[2] * 1000.0).round() / 1000.0,
    ]
}

fn circumcircle(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> Option<([f64; 2], f64)> {
    let d = 2.0 * (a[0] * (b[1] - c[1]) + b[0] * (c[1] - a[1]) + c[0] * (a[1] - b[1]));
    if d.abs() < 1e-9 {
        return None;
    }
    let (a2, b2, c2) = (
        a[0] * a[0] + a[1] * a[1],
        b[0] * b[0] + b[1] * b[1],
        c[0] * c[0] + c[1] * c[1],
    );
    let ux = (a2 * (b[1] - c[1]) + b2 * (c[1] - a[1]) + c2 * (a[1] - b[1])) / d;
    let uy = (a2 * (c[0] - b[0]) + b2 * (a[0] - c[0]) + c2 * (b[0] - a[0])) / d;
    let r = ((a[0] - ux).powi(2) + (a[1] - uy).powi(2)).sqrt();
    (r <= MAX_RADIUS).then_some(([ux, uy], r))
}

fn fit_arc(start: P3, items: &[Item]) -> Option<ArcFit> {
    let mut pts: Vec<P3> = Vec::new();
    let mut feed = None;
    for it in items {
        match it {
            Item::Feed(p, f)
                if feed.is_none_or(|x: f64| (x - f).abs() < 1e-9)
                    && (p[2] - start[2]).abs() < 5e-4 =>
            {
                feed = Some(*f);
                pts.push(*p);
            }
            _ => break,
        }
    }
    if pts.len() < MIN_ARC_POINTS {
        return None;
    }
    let start = round3(start);
    let (center, r) = circumcircle(
        [start[0], start[1]],
        [pts[0][0], pts[0][1]],
        [pts[1][0], pts[1][1]],
    )?;
    let angle = |p: P3| (p[1] - center[1]).atan2(p[0] - center[0]);
    let first = {
        let d = angle(pts[0]) - angle(start);
        (d + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI
    };
    if first == 0.0 {
        return None;
    }
    let ccw = first > 0.0;
    let mut sweep = 0.0;
    let mut prev = start;
    let mut count = 0;
    for p in &pts {
        let radius = ((p[0] - center[0]).powi(2) + (p[1] - center[1]).powi(2)).sqrt();
        if (radius - r).abs() > ARC_TOL / 2.0 {
            break;
        }
        let d = (angle(*p) - angle(prev) + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
            - std::f64::consts::PI;
        if (d > 0.0) != ccw || d == 0.0 || d.abs() > std::f64::consts::FRAC_PI_2 {
            break;
        }
        if sweep + d.abs() > std::f64::consts::PI {
            break;
        }
        sweep += d.abs();
        prev = *p;
        count += 1;
    }
    if count < MIN_ARC_POINTS {
        return None;
    }
    Some(ArcFit {
        end: pts[count - 1],
        center,
        cw: !ccw,
        feed: feed?,
        count,
    })
}

pub fn emit(prog: &Program, kind: PostKind) -> Result<String, String> {
    let post = kind.post();
    let mut lines: Vec<String> = Vec::new();
    for l in post.header.lines() {
        lines.push(l.to_string());
    }
    let mut pos: Option<P3> = None;
    let mut feed = f64::NAN;
    let mut cutting = false;
    let mut rpm_on = false;
    let items = prog.items();
    let mut idx = 0;
    while idx < items.len() {
        let item = items[idx].clone();
        idx += 1;
        if let (Item::Feed(..), Some(start)) = (&item, pos)
            && let Some(fit) = fit_arc(start, &items[idx - 1..])
        {
            {
                let start = round3(start);
                let f = fit.feed;
                let f_changed = num(f, 1) != num(feed, 1);
                let mut l = format!(
                    "{}{} I{} J{}",
                    if fit.cw { "G2" } else { "G3" },
                    axes(Some(start), fit.end),
                    num(fit.center[0] - start[0], 4),
                    num(fit.center[1] - start[1], 4)
                );
                if f_changed {
                    l.push_str(&format!(" F{}", num(f, 1)));
                }
                lines.push(l);
                feed = f;
                pos = Some(fit.end);
                idx += fit.count - 1;
                continue;
            }
        }
        match item {
            Item::Tool { n, rpm } => {
                let milling_tool = rpm > 0.0;
                if milling_tool && !post.milling {
                    return Err(format!(
                        "the {} post only drives lasers, tool {n} is a milling tool",
                        "marlin"
                    ));
                }
                for l in fill(post.tool_change, "", "", "", &n.to_string()).lines() {
                    lines.push(l.to_string());
                }
                rpm_on = false;
                if milling_tool {
                    lines.push(fill(post.spindle_on, "", "", &num(rpm, 0), ""));
                    rpm_on = true;
                }
            }
            Item::Laser(p) => {
                if p > 0.0 {
                    lines.push(fill(post.laser_on, "", "", &num(p, 0), ""));
                    cutting = true;
                } else if cutting {
                    lines.push(post.laser_off.to_string());
                    cutting = false;
                }
            }
            Item::Rapid(to) => {
                lines.push(fill(post.rapid, &axes(pos, to), "", "", ""));
                pos = Some(to);
            }
            Item::Feed(to, f) => {
                let f_changed = num(f, 1) != num(feed, 1);
                let mut l = fill(post.feed, &axes(pos, to), &num(f, 1), "", "");
                if !f_changed {
                    l = l.replace(&format!(" F{}", num(f, 1)), "");
                }
                lines.push(l);
                feed = f;
                pos = Some(to);
            }
            Item::End => {
                let _ = rpm_on;
                for l in post.footer.lines() {
                    lines.push(l.to_string());
                }
            }
        }
    }
    if post.numbered {
        let mut n = 10;
        for l in lines.iter_mut() {
            if l.starts_with('%') || l.starts_with('O') {
                continue;
            }
            *l = format!("N{n} {l}");
            n += 10;
        }
    }
    Ok(lines.join("\n") + "\n")
}

#[derive(Clone, Debug, PartialEq)]
pub struct ArcSeg {
    pub from: P3,
    pub center: [f64; 2],
    pub cw: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Seg {
    pub mv: Move,
    pub arc: Option<ArcSeg>,
}

pub fn parse_gcode(text: &str) -> Result<Vec<Move>, String> {
    let mut out = Vec::new();
    for seg in parse_segments(text)? {
        match &seg.arc {
            None => out.push(seg.mv),
            Some(arc) => out.extend(expand_arc(&seg.mv, arc)),
        }
    }
    Ok(out)
}

fn expand_arc(mv: &Move, arc: &ArcSeg) -> Vec<Move> {
    let (cx, cy) = (arc.center[0], arc.center[1]);
    let r = ((arc.from[0] - cx).powi(2) + (arc.from[1] - cy).powi(2)).sqrt();
    let a0 = (arc.from[1] - cy).atan2(arc.from[0] - cx);
    let a1 = (mv.to[1] - cy).atan2(mv.to[0] - cx);
    let tau = std::f64::consts::TAU;
    let mut sweep = if arc.cw { a0 - a1 } else { a1 - a0 };
    while sweep <= 1e-9 {
        sweep += tau;
    }
    let n = ((sweep / 0.035).ceil() as usize).max(1);
    let sign = if arc.cw { -1.0 } else { 1.0 };
    (1..=n)
        .map(|k| {
            let t = k as f64 / n as f64;
            let a = a0 + sign * sweep * t;
            let to = if k == n {
                mv.to
            } else {
                [
                    cx + r * a.cos(),
                    cy + r * a.sin(),
                    arc.from[2] + (mv.to[2] - arc.from[2]) * t,
                ]
            };
            Move { to, ..mv.clone() }
        })
        .collect()
}

pub fn parse_segments(text: &str) -> Result<Vec<Seg>, String> {
    let mut out = Vec::new();
    let mut pos = [0.0f64; 3];
    let mut feed = 0.0;
    let mut rapid_mode = true;
    let mut arc_mode: Option<bool> = None;
    let mut tool = 0u32;
    let mut pending_tool: Option<u32> = None;
    let mut spindle = 0.0;
    let mut s_word = 0.0;
    let mut on = false;
    for (i, raw) in text.lines().enumerate() {
        let lineno = i + 1;
        let mut line = raw.to_string();
        while let (Some(a), Some(b)) = (line.find('('), line.find(')')) {
            if b < a {
                break;
            }
            line.replace_range(a..=b, " ");
        }
        if let Some(c) = line.find(';') {
            line.truncate(c);
        }
        let line = line.trim();
        if line.is_empty() || line.starts_with('%') {
            continue;
        }
        let mut target = pos;
        let mut moved = false;
        let (mut off_i, mut off_j) = (0.0, 0.0);
        let mut words = Vec::new();
        for w in line.split_whitespace() {
            let mut chars = w.chars();
            let letter = chars.next().unwrap().to_ascii_uppercase();
            let val: f64 = chars
                .as_str()
                .parse()
                .map_err(|_| format!("gcode line {lineno}: bad word {w}"))?;
            words.push((letter, val));
        }
        for &(letter, val) in &words {
            match letter {
                'N' | 'O' | 'H' | 'P' | 'K' | 'R' => {}
                'I' => off_i = val,
                'J' => off_j = val,
                'X' => {
                    target[0] = val;
                    moved = true;
                }
                'Y' => {
                    target[1] = val;
                    moved = true;
                }
                'Z' => {
                    target[2] = val;
                    moved = true;
                }
                'F' => feed = val,
                'S' => {
                    s_word = val;
                    if on {
                        spindle = val;
                    }
                }
                'T' => pending_tool = Some(val as u32),
                'G' => match val as i64 {
                    0 => {
                        rapid_mode = true;
                        arc_mode = None;
                    }
                    1 => {
                        rapid_mode = false;
                        arc_mode = None;
                    }
                    2 | 3 => {
                        rapid_mode = false;
                        arc_mode = Some(val as i64 == 2);
                    }
                    4 | 17 | 21 | 40 | 43 | 49 | 54 | 64 | 80 | 90 | 94 => {}
                    other => return Err(format!("gcode line {lineno}: unsupported G{other}")),
                },
                'M' => match val as i64 {
                    3 | 4 => {
                        on = true;
                        spindle = s_word;
                    }
                    5 => {
                        on = false;
                        spindle = 0.0;
                    }
                    6 => {
                        if let Some(t) = pending_tool.take() {
                            tool = t;
                        }
                    }
                    0 | 2 | 30 | 84 => {}
                    other => return Err(format!("gcode line {lineno}: unsupported M{other}")),
                },
                other => return Err(format!("gcode line {lineno}: unsupported word {other}")),
            }
        }
        if let Some(t) = pending_tool {
            if !words.iter().any(|(l, v)| *l == 'M' && *v as i64 == 6) {
                tool = t;
            }
            pending_tool = None;
        }
        if moved {
            let mv = Move {
                rapid: rapid_mode,
                to: target,
                feed: if rapid_mode { 0.0 } else { feed },
                tool,
                s: spindle,
            };
            let arc = arc_mode.map(|cw| ArcSeg {
                from: pos,
                center: [pos[0] + off_i, pos[1] + off_j],
                cw,
            });
            out.push(Seg { mv, arc });
            pos = target;
        }
    }
    Ok(out)
}

pub fn backplot_check(prog: &Program, text: &str) -> Result<usize, String> {
    let segs = parse_segments(text)?;
    let want = prog.moves();
    let mut at = 0;
    for (i, seg) in segs.iter().enumerate() {
        let Some(w) = want.get(at) else {
            return Err(format!(
                "the gcode has more moves than the toolpath's {}",
                want.len()
            ));
        };
        match &seg.arc {
            None => {
                if !crate::sim::moves_equal(&seg.mv, w) {
                    return Err(format!(
                        "move {} differs: gcode {:?}, toolpath {:?}",
                        i + 1,
                        seg.mv,
                        w
                    ));
                }
                at += 1;
            }
            Some(arc) => {
                let r = ((arc.from[0] - arc.center[0]).powi(2)
                    + (arc.from[1] - arc.center[1]).powi(2))
                .sqrt();
                let mut covered = 0;
                loop {
                    let Some(p) = want.get(at + covered) else {
                        return Err(format!("arc {} runs past the end of the toolpath", i + 1));
                    };
                    let radius = ((p.to[0] - arc.center[0]).powi(2)
                        + (p.to[1] - arc.center[1]).powi(2))
                    .sqrt();
                    let on_arc = !p.rapid
                        && (radius - r).abs() < ARC_TOL
                        && (p.to[2] - seg.mv.to[2]).abs() < 6e-4
                        && (p.feed - seg.mv.feed).abs() < 0.06
                        && p.tool == seg.mv.tool.max(p.tool)
                        && (p.s - seg.mv.s).abs() < 0.6;
                    if !on_arc {
                        return Err(format!(
                            "arc {} leaves the toolpath at move {}",
                            i + 1,
                            at + covered + 1
                        ));
                    }
                    covered += 1;
                    if (0..2).all(|k| (p.to[k] - seg.mv.to[k]).abs() < 6e-4) {
                        break;
                    }
                }
                at += covered;
            }
        }
    }
    if at != want.len() {
        return Err(format!(
            "the gcode has {} moves, the toolpath has {}",
            at,
            want.len()
        ));
    }
    Ok(at)
}
