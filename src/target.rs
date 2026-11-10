use crate::sim::{Findings, Stock};

pub struct Target {
    pub top: Vec<Option<f32>>,
}

fn tris_from_ascii(text: &str) -> Vec<[[f64; 3]; 3]> {
    let mut v: Vec<[f64; 3]> = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        if let Some(rest) = l.strip_prefix("vertex") {
            let n: Vec<f64> = rest
                .split_whitespace()
                .filter_map(|x| x.parse().ok())
                .collect();
            if n.len() == 3 {
                v.push([n[0], n[1], n[2]]);
            }
        }
    }
    v.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect()
}

fn tris_from_binary(b: &[u8]) -> Result<Vec<[[f64; 3]; 3]>, String> {
    if b.len() < 84 {
        return Err("stl too short".into());
    }
    let n = u32::from_le_bytes([b[80], b[81], b[82], b[83]]) as usize;
    if b.len() < 84 + n * 50 {
        return Err("stl is truncated".into());
    }
    let f = |o: usize| f32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]]) as f64;
    Ok((0..n)
        .map(|i| {
            let o = 84 + i * 50 + 12;
            let mut t = [[0.0; 3]; 3];
            for (k, v) in t.iter_mut().enumerate() {
                *v = [f(o + k * 12), f(o + k * 12 + 4), f(o + k * 12 + 8)];
            }
            t
        })
        .collect())
}

pub fn load_stl(bytes: &[u8]) -> Result<Vec<[[f64; 3]; 3]>, String> {
    if bytes.len() >= 84 {
        let n = u32::from_le_bytes([bytes[80], bytes[81], bytes[82], bytes[83]]) as usize;
        if 84 + n * 50 == bytes.len() {
            return tris_from_binary(bytes);
        }
    }
    let t = tris_from_ascii(&String::from_utf8_lossy(bytes));
    if t.is_empty() {
        Err("no triangles in the stl".into())
    } else {
        Ok(t)
    }
}

pub fn heightfield(tris: &[[[f64; 3]; 3]], at: [f64; 3], stock: &Stock) -> Target {
    let mut top: Vec<Option<f32>> = vec![None; stock.nx * stock.ny];
    for t in tris {
        let p: Vec<[f64; 3]> = t
            .iter()
            .map(|v| [v[0] + at[0], v[1] + at[1], v[2] + at[2]])
            .collect();
        let minx = p.iter().map(|v| v[0]).fold(f64::MAX, f64::min);
        let maxx = p.iter().map(|v| v[0]).fold(f64::MIN, f64::max);
        let miny = p.iter().map(|v| v[1]).fold(f64::MAX, f64::min);
        let maxy = p.iter().map(|v| v[1]).fold(f64::MIN, f64::max);
        let i0 = ((minx / stock.grid) - 0.5).floor().max(0.0) as usize;
        let j0 = ((miny / stock.grid) - 0.5).floor().max(0.0) as usize;
        let i1 =
            (((maxx / stock.grid) - 0.5).ceil().max(0.0) as usize).min(stock.nx.saturating_sub(1));
        let j1 =
            (((maxy / stock.grid) - 0.5).ceil().max(0.0) as usize).min(stock.ny.saturating_sub(1));
        let det =
            (p[1][1] - p[2][1]) * (p[0][0] - p[2][0]) + (p[2][0] - p[1][0]) * (p[0][1] - p[2][1]);
        if det.abs() < 1e-12 {
            continue;
        }
        for j in j0..=j1 {
            for i in i0..=i1 {
                let c = stock.centre(i, j);
                let a = ((p[1][1] - p[2][1]) * (c[0] - p[2][0])
                    + (p[2][0] - p[1][0]) * (c[1] - p[2][1]))
                    / det;
                let b = ((p[2][1] - p[0][1]) * (c[0] - p[2][0])
                    + (p[0][0] - p[2][0]) * (c[1] - p[2][1]))
                    / det;
                let g = 1.0 - a - b;
                let e = -1e-9;
                if a >= e && b >= e && g >= e {
                    let z = (a * p[0][2] + b * p[1][2] + g * p[2][2]) as f32;
                    let cell = &mut top[j * stock.nx + i];
                    if cell.is_none_or(|old| z > old) {
                        *cell = Some(z);
                    }
                }
            }
        }
    }
    Target { top }
}

pub fn compare(stock: &Stock, target: &Target, tol: f64, findings: &mut Findings) {
    let (mut gouge, mut left) = (0usize, 0usize);
    let (mut g_at, mut l_at) = (None, None);
    let mut worst_g = 0.0f64;
    let mut worst_l = 0.0f64;
    for j in 0..stock.ny {
        for i in 0..stock.nx {
            let idx = j * stock.nx + i;
            let Some(t) = target.top[idx] else { continue };
            let h = stock.h[idx];
            let over = (t - h) as f64;
            let under = (h - t) as f64;
            if over > tol {
                gouge += 1;
                if over > worst_g {
                    worst_g = over;
                    g_at = Some(stock.centre(i, j));
                }
            } else if under > tol {
                left += 1;
                if under > worst_l {
                    worst_l = under;
                    l_at = Some(stock.centre(i, j));
                }
            }
        }
    }
    if let Some(c) = g_at {
        findings.gouges.push(format!(
            "part: {gouge} cells cut below the target, worst {worst_g:.3} mm at X{:.2} Y{:.2}",
            c[0], c[1]
        ));
    }
    if let Some(c) = l_at {
        findings.remaining.push(format!(
            "part: {left} cells above the target by more than {tol} mm, worst {worst_l:.3} mm at X{:.2} Y{:.2}",
            c[0], c[1]
        ));
    }
}
