pub mod camotics;
pub mod gcode;
pub mod geom;
pub mod materials;
pub mod model;
pub mod sim;
pub mod target;
pub mod toolpath;
pub mod view;

use std::path::Path;

pub struct Report {
    pub program: toolpath::Program,
    pub sim: sim::SimResult,
}

pub fn load_part_mesh(path: &Path) -> Result<Vec<[[f64; 3]; 3]>, String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    let stl = match ext {
        "stl" => path.to_path_buf(),
        "gcad" | "gasm" => {
            let out = std::env::temp_dir().join(format!("cambench-{}.stl", std::process::id()));
            let status = std::process::Command::new("gcad")
                .args(["export"])
                .arg(path)
                .arg(&out)
                .status()
                .map_err(|e| format!("running gcad export: {e}"))?;
            if !status.success() {
                return Err(format!("gcad export failed for {}", path.display()));
            }
            out
        }
        other => return Err(format!("part files are .stl, .gcad or .gasm, not .{other}")),
    };
    let bytes = std::fs::read(&stl).map_err(|e| format!("reading {}: {e}", stl.display()))?;
    target::load_stl(&bytes)
}

pub fn run(src: &str, base: &Path) -> Result<(model::Job, Report), String> {
    let job = model::parse(src)?;
    let program = toolpath::plan(&job)?;
    let mut sim = sim::simulate(&program, job.grid, job.tol);
    if let Some(part) = &job.part {
        let tris = load_part_mesh(&base.join(&part.path))?;
        let t = target::heightfield(&tris, part.at, &sim.stock);
        target::compare(&sim.stock, &t, job.tol, &mut sim.findings);
    }
    Ok((job, Report { program, sim }))
}
