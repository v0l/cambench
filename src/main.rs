use std::path::{Path, PathBuf};
use std::process::ExitCode;

use cambench::gcode::{self, POSTS};

const USAGE: &str = "cambench: CAM toolpaths from a .gcam file, checked by simulation

usage:
  cambench check FILE.gcam            plan, simulate and verify every post; exit 1 on findings
  cambench post FILE.gcam POST [OUT]  write gcode for grbl, linuxcnc, marlin or fanuc
  cambench camotics FILE.gcam DIR     write plate.nc and a CAMotics project for the CI cross-check
  cambench agree FILE.gcam CAMSIM.stl  compare the simulated stock with a CAMotics result
  cambench view FILE.gcam OUT.svg     toolpath over the simulated stock
  cambench info FILE.gcam             time estimate and removed volume
  cambench materials                  the material table
  cambench posts                      the available posts
";

fn load(path: &str) -> Result<(String, PathBuf), String> {
    let p = Path::new(path);
    let src = std::fs::read_to_string(p).map_err(|e| format!("reading {path}: {e}"))?;
    Ok((src, p.parent().map(Path::to_path_buf).unwrap_or_default()))
}

fn fmt_time(s: f64) -> String {
    format!("{}m{:02}s", (s / 60.0) as u32, (s % 60.0) as u32)
}

fn real_main(args: &[String]) -> Result<bool, String> {
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["materials"] => {
            for m in cambench::materials::names() {
                println!("{m}");
            }
            Ok(true)
        }
        ["posts"] => {
            for (n, _) in POSTS {
                println!("{n}");
            }
            Ok(true)
        }
        ["check", file] => {
            let (src, base) = load(file)?;
            let (_, rep) = cambench::run(&src, &base)?;
            let mut ok = rep.sim.findings.ok();
            for l in rep.sim.findings.lines() {
                println!("{l}");
            }
            for (name, kind) in POSTS {
                let milling = rep.program.ops.iter().any(|o| o.rpm > 0.0);
                if *name == "marlin" && milling {
                    continue;
                }
                let text = gcode::emit(&rep.program, *kind)?;
                match gcode::backplot_check(&rep.program, &text) {
                    Ok(n) => println!("post {name}: {n} moves reproduce the toolpath"),
                    Err(e) => {
                        println!("post {name}: {e}");
                        ok = false;
                    }
                }
            }
            println!(
                "{} findings, estimated time {}",
                rep.sim.findings.count(),
                fmt_time(rep.program.estimate_seconds())
            );
            Ok(ok)
        }
        ["post", file, post] | ["post", file, post, _] => {
            let (src, base) = load(file)?;
            let kind = gcode::post_by_name(post).ok_or_else(|| format!("unknown post {post}"))?;
            let (_, rep) = cambench::run(&src, &base)?;
            let text = gcode::emit(&rep.program, kind)?;
            match a.get(3) {
                Some(out) => {
                    std::fs::write(out, text).map_err(|e| format!("writing {out}: {e}"))?
                }
                None => print!("{text}"),
            }
            Ok(true)
        }
        ["camotics", file, dir] => {
            let (src, base) = load(file)?;
            let (_, rep) = cambench::run(&src, &base)?;
            let text = gcode::emit(&rep.program, gcode::PostKind::LinuxCnc)?;
            std::fs::create_dir_all(dir).map_err(|e| format!("creating {dir}: {e}"))?;
            let nc = Path::new(dir).join("job.nc");
            std::fs::write(&nc, text).map_err(|e| format!("writing {}: {e}", nc.display()))?;
            let proj = Path::new(dir).join("job.camotics");
            std::fs::write(
                &proj,
                cambench::camotics::project(&rep.program, "job.nc", 0.25),
            )
            .map_err(|e| format!("writing {}: {e}", proj.display()))?;
            Ok(true)
        }
        ["agree", file, stl] => {
            let (src, base) = load(file)?;
            let (_, rep) = cambench::run(&src, &base)?;
            let bytes = std::fs::read(stl).map_err(|e| format!("reading {stl}: {e}"))?;
            let a = cambench::camotics::agreement(&rep.sim.stock, &bytes, 0.6)?;
            println!(
                "camotics agrees on {} of {} cells ({:.1}%), worst difference {:.2} mm",
                a.agree,
                a.cells,
                a.fraction() * 100.0,
                a.worst
            );
            Ok(a.fraction() >= 0.97)
        }
        ["view", file, out] => {
            let (src, base) = load(file)?;
            let (_, rep) = cambench::run(&src, &base)?;
            std::fs::write(out, cambench::view::svg(&rep.program, &rep.sim))
                .map_err(|e| format!("writing {out}: {e}"))?;
            Ok(true)
        }
        ["info", file] => {
            let (src, base) = load(file)?;
            let (_, rep) = cambench::run(&src, &base)?;
            println!("operations {}", rep.program.ops.len());
            println!("moves {}", rep.program.moves().len());
            println!("removed {:.0} mm3", rep.sim.removed_mm3);
            println!("time {}", fmt_time(rep.program.estimate_seconds()));
            Ok(true)
        }
        _ => Err(USAGE.trim_end().to_string()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match real_main(&args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
