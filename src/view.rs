use crate::sim::SimResult;
use crate::toolpath::Program;

fn shade(h: f32, bottom: f32) -> String {
    let t = (h / bottom).clamp(0.0, 1.0);
    let v = (225.0 - 170.0 * t) as u8;
    format!(
        "rgb({v},{},{})",
        (v as f32 * 0.93) as u8,
        (v as f32 * 0.8) as u8
    )
}

pub fn svg(prog: &Program, sim: &SimResult) -> String {
    let scale = (900.0 / prog.stock[0].max(prog.stock[1])).min(20.0);
    let w = prog.stock[0] * scale;
    let h = prog.stock[1] * scale;
    let pad = 20.0;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{:.0}\" height=\"{:.0}\" font-family=\"monospace\" font-size=\"13\">\n<rect width=\"100%\" height=\"100%\" fill=\"#1e1e1e\"/>\n",
        w + pad * 2.0,
        h + pad * 2.0 + 24.0
    );
    let cell = sim.stock.grid * scale;
    for j in 0..sim.stock.ny {
        for i in 0..sim.stock.nx {
            let hh = sim.stock.h[j * sim.stock.nx + i];
            let x = pad + i as f64 * cell;
            let y = pad + h - (j as f64 + 1.0) * cell;
            out.push_str(&format!(
                "<rect x=\"{x:.2}\" y=\"{y:.2}\" width=\"{:.2}\" height=\"{:.2}\" fill=\"{}\"/>\n",
                cell + 0.4,
                cell + 0.4,
                shade(hh, sim.stock.bottom)
            ));
        }
    }
    let mut pos: Option<[f64; 3]> = None;
    for m in prog.moves() {
        if let Some(p) = pos {
            let (colour, dash) = if m.rapid {
                ("#3aa0ff", " stroke-dasharray=\"3 3\"")
            } else {
                ("#e8453c", "")
            };
            if m.rapid && p[0] == m.to[0] && p[1] == m.to[1] {
                pos = Some(m.to);
                continue;
            }
            out.push_str(&format!(
                "<line x1=\"{:.2}\" y1=\"{:.2}\" x2=\"{:.2}\" y2=\"{:.2}\" stroke=\"{colour}\" stroke-width=\"0.7\"{dash}/>\n",
                pad + p[0] * scale,
                pad + h - p[1] * scale,
                pad + m.to[0] * scale,
                pad + h - m.to[1] * scale
            ));
        }
        pos = Some(m.to);
    }
    let secs = prog.estimate_seconds();
    out.push_str(&format!(
        "<text x=\"{pad}\" y=\"{:.0}\" fill=\"#ddd\">{} ops, {:.0} mm3 removed, time {}m{:02}s, {} findings</text>\n</svg>\n",
        h + pad * 2.0 + 14.0,
        prog.ops.len(),
        sim.removed_mm3,
        (secs / 60.0) as u32,
        (secs % 60.0) as u32,
        sim.findings.count()
    ));
    out
}
