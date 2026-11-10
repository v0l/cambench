use crate::sim::Stock;
use crate::target;
use crate::toolpath::Program;

pub fn project(prog: &Program, nc_file: &str, resolution: f64) -> String {
    let mut tools = Vec::new();
    let mut seen = Vec::new();
    for op in &prog.ops {
        if seen.contains(&op.tool.n) {
            continue;
        }
        seen.push(op.tool.n);
        tools.push(format!(
            "    \"{n}\": {{\"number\": {n}, \"units\": \"metric\", \"shape\": \"cylindrical\", \"length\": {}, \"diameter\": {}}}",
            op.tool.flute_len.max(1.0),
            op.tool.dia,
            n = op.tool.n
        ));
    }
    format!(
        "{{\n  \"units\": \"metric\",\n  \"resolution-mode\": \"manual\",\n  \"resolution\": {resolution},\n  \"tools\": {{\n{}\n  }},\n  \"workpiece\": {{\"automatic\": false, \"margin\": 0, \"bounds\": {{\"min\": [0, 0, {}], \"max\": [{}, {}, 0]}}}},\n  \"files\": [\"{nc_file}\"]\n}}\n",
        tools.join(",\n"),
        -prog.stock[2],
        prog.stock[0],
        prog.stock[1]
    )
}

pub struct Agreement {
    pub cells: usize,
    pub agree: usize,
    pub worst: f64,
}

impl Agreement {
    pub fn fraction(&self) -> f64 {
        if self.cells == 0 {
            0.0
        } else {
            self.agree as f64 / self.cells as f64
        }
    }
}

pub fn agreement(stock: &Stock, stl: &[u8], tolerance: f64) -> Result<Agreement, String> {
    let tris = target::load_stl(stl)?;
    let theirs = target::heightfield(&tris, [0.0, 0.0, 0.0], stock);
    let mut out = Agreement {
        cells: 0,
        agree: 0,
        worst: 0.0,
    };
    for (idx, top) in theirs.top.iter().enumerate() {
        let Some(top) = top else { continue };
        let diff = (stock.h[idx] - top).abs() as f64;
        out.cells += 1;
        out.worst = out.worst.max(diff);
        if diff <= tolerance {
            out.agree += 1;
        }
    }
    Ok(out)
}
