use std::path::Path;
use std::process::Command;

use cambench::gcode::{self, POSTS, PostKind};
use cambench::run;

fn gcad_available() -> bool {
    Command::new("gcad")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

const JOB: &str = "machine grid=0.25 tol=0.1
stock 60 40 10 material=aluminium
tool 1 endmill 6 flutes=2 flute_len=20 holder=12
tool 3 endmill 4 flutes=2 flute_len=14 holder=10
part plate.gcad at=30,20,-10
pocket circle 30 20 5 tool=1 depth=4
drill 8,8 52,8 8,32 52,32 tool=3 depth=10
";

#[test]
fn a_gcad_part_is_exported_planned_simulated_and_compared() {
    if !gcad_available() {
        eprintln!(
            "skipped: gcad is not on PATH, install it with `cargo install gcad` to run this test"
        );
        return;
    }
    let (_, r) = run(JOB, Path::new("tests/data")).expect("gcad export, plan and simulate");
    assert!(r.sim.findings.ok(), "{:?}", r.sim.findings.lines());
    for (name, kind) in POSTS {
        if *kind == PostKind::Marlin {
            continue;
        }
        let text = gcode::emit(&r.program, *kind).unwrap();
        gcode::backplot_check(&r.program, &text).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn a_gcad_part_that_needs_a_deeper_pocket_is_flagged() {
    if !gcad_available() {
        eprintln!(
            "skipped: gcad is not on PATH, install it with `cargo install gcad` to run this test"
        );
        return;
    }
    let shallow = JOB.replace("depth=4\n", "depth=3\n");
    let (_, r) = run(&shallow, Path::new("tests/data")).expect("plan");
    assert!(
        !r.sim.findings.remaining.is_empty(),
        "{:?}",
        r.sim.findings.lines()
    );
}
