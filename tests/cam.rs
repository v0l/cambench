use std::path::Path;

use cambench::gcode::{self, POSTS, PostKind};
use cambench::sim;
use cambench::toolpath::Item;
use cambench::{model, run, toolpath};

const HEAD: &str = "machine grid=0.25 tol=0.05\nstock 60 40 10 material=aluminium\ntool 1 endmill 6 flutes=2 flute_len=20 holder=12\ntool 2 vbit 12 angle=90\n";

fn run_src(body: &str) -> Result<cambench::Report, String> {
    run(&format!("{HEAD}{body}"), Path::new(".")).map(|(_, r)| r)
}

fn clean(body: &str) -> cambench::Report {
    let r = run_src(body).expect("plan");
    assert!(r.sim.findings.ok(), "{:?}", r.sim.findings.lines());
    r
}

fn every_post_reproduces(r: &cambench::Report) {
    for (name, kind) in POSTS {
        if *kind == PostKind::Marlin {
            continue;
        }
        let text = gcode::emit(&r.program, *kind).unwrap();
        gcode::backplot_check(&r.program, &text).unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn pocket_reaches_its_floor_and_stays_inside() {
    let r = clean("pocket rect 10 10 40 20 tool=1 depth=4\n");
    let s = &r.sim.stock;
    let at = |x: f64, y: f64| s.h[(y / s.grid) as usize * s.nx + (x / s.grid) as usize];
    assert!((at(30.0, 20.0) + 4.0).abs() < 0.01);
    assert!(at(5.0, 5.0).abs() < 0.001);
    assert!(at(9.5, 20.0).abs() < 0.001);
    every_post_reproduces(&r);
}

#[test]
fn adaptive_clears_a_circle_and_leaves_the_wall() {
    let r = clean("adaptive circle 30 20 12 tool=1 depth=6\n");
    let s = &r.sim.stock;
    let at = |x: f64, y: f64| s.h[(y / s.grid) as usize * s.nx + (x / s.grid) as usize];
    assert!((at(30.0, 20.0) + 6.0).abs() < 0.01);
    assert!(at(30.0, 33.0).abs() < 0.001);
    every_post_reproduces(&r);
}

#[test]
fn profile_with_tabs_cuts_through_except_at_tabs() {
    let r = clean("profile rect 10 10 30 20 tool=1 depth=10 side=outside tabs=2 tab_h=2\n");
    let s = &r.sim.stock;
    let ring = (r.program.ops[0].items.len(), 0);
    assert!(ring.0 > 50);
    let tab_cells = s.h.iter().filter(|h| (**h + 8.0).abs() < 0.01).count();
    let through = s.h.iter().filter(|h| (**h + 10.0).abs() < 0.01).count();
    assert!(tab_cells > 0, "tabs hold material at 2 mm above the bottom");
    assert!(through > tab_cells);
    every_post_reproduces(&r);
}

#[test]
fn drill_cuts_a_hole_of_the_tool_diameter() {
    let r = clean("drill 20,20 tool=1 depth=5\n");
    let s = &r.sim.stock;
    let at = |x: f64, y: f64| s.h[(y / s.grid) as usize * s.nx + (x / s.grid) as usize];
    assert!((at(20.1, 20.1) + 5.0).abs() < 0.01);
    assert!(at(24.0, 20.0).abs() < 0.001);
    every_post_reproduces(&r);
}

#[test]
fn chamfer_makes_a_45_degree_edge() {
    let r = clean("chamfer rect 10 10 30 20 tool=2 width=1\n");
    let s = &r.sim.stock;
    let at = |x: f64, y: f64| s.h[(y / s.grid) as usize * s.nx + (x / s.grid) as usize];
    assert!((at(10.0, 20.0) + 1.0).abs() < 0.3);
    assert!(at(12.0, 20.0).abs() < 0.001);
    every_post_reproduces(&r);
}

#[test]
fn engrave_follows_the_path() {
    let r = clean("engrave 10,10 40,30 tool=2 depth=0.5\n");
    assert!(r.sim.removed_mm3 > 1.0);
    every_post_reproduces(&r);
}

#[test]
fn face_levels_the_whole_top() {
    let r = clean("face tool=1 depth=1\n");
    assert!(r.sim.stock.h.iter().all(|h| (*h + 1.0).abs() < 0.01));
    every_post_reproduces(&r);
}

#[test]
fn full_job_is_clean_on_every_post() {
    let src = std::fs::read_to_string("examples/plate.gcam").unwrap();
    let (_, r) = run(&src, Path::new("examples")).unwrap();
    assert!(r.sim.findings.ok(), "{:?}", r.sim.findings.lines());
    every_post_reproduces(&r);
}

#[test]
fn gouge_is_reported_when_a_pass_digs_outside_its_pocket() {
    let job = model::parse(&format!("{HEAD}pocket rect 10 10 40 20 tool=1 depth=4\n")).unwrap();
    let mut prog = toolpath::plan(&job).unwrap();
    for it in prog.ops[0].items.iter_mut() {
        if let Item::Feed(p, _) = it {
            p[0] += 6.0;
        }
    }
    let res = sim::simulate(&prog, 0.25, 0.05);
    assert!(
        !res.findings.gouges.is_empty(),
        "{:?}",
        res.findings.lines()
    );
}

#[test]
fn rapid_through_material_is_reported() {
    let job = model::parse(&format!("{HEAD}pocket rect 10 10 20 20 tool=1 depth=4\n")).unwrap();
    let mut prog = toolpath::plan(&job).unwrap();
    let mut seen_feed = false;
    for it in prog.ops[0].items.iter_mut() {
        match it {
            Item::Feed(..) => seen_feed = true,
            Item::Rapid(p) if seen_feed && p[2] > 0.0 => {
                p[2] = -2.0;
                break;
            }
            _ => {}
        }
    }
    prog.ops[0].items.push(Item::Rapid([50.0, 38.0, -2.0]));
    prog.ops[0].items.push(Item::Rapid([5.0, 38.0, -2.0]));
    let res = sim::simulate(&prog, 0.25, 0.05);
    assert!(
        !res.findings.rapid_hits.is_empty(),
        "{:?}",
        res.findings.lines()
    );
}

#[test]
fn holder_collision_is_reported_for_a_short_flute() {
    let src = "machine grid=0.25\nstock 60 40 10 material=aluminium\ntool 1 endmill 4 flutes=2 flute_len=4 holder=16\npocket rect 10 10 40 20 tool=1 depth=6 stepdown=2\n";
    let (_, r) = run(src, Path::new(".")).unwrap();
    assert!(
        !r.sim.findings.holder_hits.is_empty(),
        "{:?}",
        r.sim.findings.lines()
    );
}

#[test]
fn a_tool_too_big_for_the_pocket_is_refused() {
    let e = run_src("pocket rect 10 10 4 4 tool=1 depth=2\n")
        .err()
        .unwrap();
    assert!(e.contains("does not fit"), "{e}");
}

#[test]
fn a_cut_deeper_than_the_stock_is_refused() {
    let e = run_src("pocket rect 10 10 20 20 tool=1 depth=11\n")
        .err()
        .unwrap();
    assert!(e.contains("depth"), "{e}");
}

#[test]
fn unknown_operations_and_missing_stock_are_refused() {
    assert!(model::parse("bogus 1\n").is_err());
    assert!(
        model::parse("tool 1 endmill 3\nface\n")
            .unwrap_err()
            .contains("stock")
    );
}

#[test]
fn feeds_come_from_the_material_and_tool_table() {
    let r = clean("pocket rect 10 10 20 20 tool=1 depth=1\n");
    let rpm = r.program.ops[0].rpm;
    assert!((rpm - (200_000.0 / (std::f64::consts::PI * 6.0)).round()).abs() < 1.0);
    let m = r.program.moves().into_iter().find(|m| !m.rapid).unwrap();
    assert!(m.feed > 50.0 && m.feed < 3000.0);
}

#[test]
fn each_post_has_its_own_dialect() {
    let r = clean("pocket rect 10 10 20 20 tool=1 depth=1\n");
    let grbl = gcode::emit(&r.program, PostKind::Grbl).unwrap();
    let lcnc = gcode::emit(&r.program, PostKind::LinuxCnc).unwrap();
    let fanuc = gcode::emit(&r.program, PostKind::Fanuc).unwrap();
    assert!(grbl.contains("M0") && !grbl.contains("M6"));
    assert!(lcnc.contains("T1 M6") && lcnc.contains("G43 H1") && !lcnc.contains("N10"));
    assert!(fanuc.starts_with('%') && fanuc.contains("N10 ") && fanuc.trim_end().ends_with('%'));
    assert!(gcode::emit(&r.program, PostKind::Marlin).is_err());
}

#[test]
fn backplot_catches_a_tampered_post() {
    let r = clean("pocket rect 10 10 20 20 tool=1 depth=1\n");
    let text = gcode::emit(&r.program, PostKind::Grbl).unwrap();
    let bad = text.replacen("G1 Y", "G1 Y1", 1);
    assert!(gcode::backplot_check(&r.program, &bad).is_err());
}

const LASER: &str = "machine grid=0.2\nstock 60 40 3 material=plywood\ntool 1 laser 0.2\n";

#[test]
fn laser_cut_goes_through_and_marlin_reproduces_it() {
    let (_, r) = run(
        &format!("{LASER}cut circle 30 20 10 tool=1 side=outside\n"),
        Path::new("."),
    )
    .unwrap();
    assert!(r.sim.findings.ok(), "{:?}", r.sim.findings.lines());
    let text = gcode::emit(&r.program, PostKind::Marlin).unwrap();
    assert!(text.contains("M3 S") && text.contains("M5"));
    gcode::backplot_check(&r.program, &text).unwrap();
    let s = &r.sim.stock;
    let at = |x: f64, y: f64| s.h[(y / s.grid) as usize * s.nx + (x / s.grid) as usize];
    assert!((at(40.05, 20.0) + 3.0).abs() < 0.01);
    assert!(at(30.0, 20.0).abs() < 0.001);
}

#[test]
fn laser_cut_that_is_too_fast_does_not_cut_through() {
    let (_, r) = run(
        &format!("{LASER}cut circle 30 20 10 tool=1 feed=3000 power=200\n"),
        Path::new("."),
    )
    .unwrap();
    assert!(
        !r.sim.findings.remaining.is_empty(),
        "{:?}",
        r.sim.findings.lines()
    );
}

#[test]
fn laser_raster_stays_shallow_and_inside() {
    let (_, r) = run(
        &format!("{LASER}raster rect 10 10 20 10 tool=1 line=0.2 depth=0.3\n"),
        Path::new("."),
    )
    .unwrap();
    assert!(r.sim.findings.ok(), "{:?}", r.sim.findings.lines());
    let s = &r.sim.stock;
    let at = |x: f64, y: f64| s.h[(y / s.grid) as usize * s.nx + (x / s.grid) as usize];
    let d = at(20.0, 15.0);
    assert!(d < -0.05 && d > -1.0, "{d}");
    assert!(at(40.0, 30.0).abs() < 0.001);
}

#[test]
fn laser_ops_need_a_laser_tool_and_a_laser_material() {
    assert!(run_src("cut circle 30 20 5 tool=1\n").is_err());
    let e = run("machine grid=0.2\nstock 60 40 3 material=aluminium\ntool 1 laser 0.2\ncut circle 30 20 5\n", Path::new(".")).err().unwrap();
    assert!(e.contains("laser"), "{e}");
}

fn box_stl(x0: f64, y0: f64, x1: f64, y1: f64, top: f64) -> String {
    let v = |x: f64, y: f64| format!("vertex {x} {y} {top}\n");
    let mut s = String::from("solid t\n");
    for tri in [
        [(x0, y0), (x1, y0), (x1, y1)],
        [(x0, y0), (x1, y1), (x0, y1)],
    ] {
        s.push_str("facet normal 0 0 1\nouter loop\n");
        for (x, y) in tri {
            s.push_str(&v(x, y));
        }
        s.push_str("endloop\nendfacet\n");
    }
    s + "endsolid t\n"
}

#[test]
fn part_comparison_flags_gouges_and_remaining_stock() {
    let dir = std::env::temp_dir().join(format!("cambench-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("floor.stl"), box_stl(11.0, 11.0, 29.0, 29.0, -4.0)).unwrap();
    std::fs::write(
        dir.join("deeper.stl"),
        box_stl(11.0, 11.0, 29.0, 29.0, -5.0),
    )
    .unwrap();
    std::fs::write(
        dir.join("shallow.stl"),
        box_stl(11.0, 11.0, 29.0, 29.0, -3.0),
    )
    .unwrap();
    let job = |part: &str| format!("{HEAD}part {part}\npocket rect 10 10 20 20 tool=1 depth=4\n");
    let (_, ok) = run(&job("floor.stl"), &dir).unwrap();
    assert!(ok.sim.findings.ok(), "{:?}", ok.sim.findings.lines());
    let (_, gouge) = run(&job("shallow.stl"), &dir).unwrap();
    assert!(!gouge.sim.findings.gouges.is_empty());
    let (_, left) = run(&job("deeper.stl"), &dir).unwrap();
    assert!(!left.sim.findings.remaining.is_empty());
    std::fs::remove_dir_all(dir).ok();
}

#[test]
fn time_estimate_grows_with_work() {
    let a = clean("pocket rect 10 10 20 20 tool=1 depth=1\n")
        .program
        .estimate_seconds();
    let b = clean("pocket rect 10 10 40 20 tool=1 depth=4\n")
        .program
        .estimate_seconds();
    assert!(b > a && a > 1.0);
}

#[test]
fn view_draws_the_toolpath_over_the_stock() {
    let r = clean("pocket rect 10 10 20 20 tool=1 depth=1\n");
    let svg = cambench::view::svg(&r.program, &r.sim);
    assert!(svg.starts_with("<svg") && svg.contains("<line") && svg.contains("findings"));
}

#[test]
fn circular_cuts_are_posted_as_arcs_and_still_reproduce_the_toolpath() {
    let r = clean(
        "pocket circle 30 20 12 tool=1 depth=2\nprofile circle 30 20 15 tool=1 depth=3 side=outside\n",
    );
    for kind in [PostKind::Grbl, PostKind::LinuxCnc, PostKind::Fanuc] {
        let text = gcode::emit(&r.program, kind).unwrap();
        let arcs = text
            .lines()
            .filter(|l| l.contains("G2") || l.contains("G3"))
            .count();
        assert!(arcs > 4, "{kind:?} posted {arcs} arcs");
        assert!(
            text.lines().count() < r.program.moves().len(),
            "arcs shorten the program"
        );
        gcode::backplot_check(&r.program, &text).unwrap();
    }
}

#[test]
fn a_hand_written_arc_is_expanded_into_lines_on_the_circle() {
    let moves =
        gcode::parse_gcode("G21 G90\nG0 X10 Y0\nG3 X0 Y10 I-10 J0 F300\nG2 X10 Y0 I0 J-10\n")
            .unwrap();
    assert!(moves.len() > 40);
    for m in &moves[1..] {
        let r = (m.to[0] * m.to[0] + m.to[1] * m.to[1]).sqrt();
        assert!(
            (r - 10.0).abs() < 0.01
                || m.to[0] == 10.0 && m.to[1] == 0.0
                || (m.to[0] - 0.0).abs() < 1e-9
        );
    }
    assert_eq!(moves[0].to, [10.0, 0.0, 0.0]);
    assert_eq!(moves.last().unwrap().to, [10.0, 0.0, 0.0]);
}

#[test]
fn backplot_catches_a_tampered_arc() {
    let r = clean("pocket circle 30 20 12 tool=1 depth=2\n");
    let text = gcode::emit(&r.program, PostKind::Grbl).unwrap();
    let flipped = if text.contains("G2") {
        text.replacen("G2", "G3", 1)
    } else {
        text.replacen("G3", "G2", 1)
    };
    assert_ne!(flipped, text);
    assert!(gcode::backplot_check(&r.program, &flipped).is_err());
}

#[test]
fn camotics_project_lists_the_stock_tools_and_gcode_file() {
    let r =
        clean("pocket rect 10 10 20 20 tool=1 depth=1\nchamfer rect 10 10 20 20 tool=2 width=1\n");
    let json = cambench::camotics::project(&r.program, "job.nc", 0.25);
    assert!(json.contains("\"files\": [\"job.nc\"]"));
    assert!(json.contains("\"number\": 1") && json.contains("\"number\": 2"));
    assert!(json.contains("\"max\": [60, 40, 0]") && json.contains("\"min\": [0, 0, -10]"));
}

#[test]
fn camotics_agreement_counts_cells_within_tolerance() {
    let r = clean("pocket rect 10 10 20 20 tool=1 depth=2\n");
    let s = &r.sim.stock;
    let mut stl = String::from("solid t\n");
    for j in 0..s.ny {
        for i in 0..s.nx {
            let c = s.centre(i, j);
            let z = s.h[j * s.nx + i];
            let g = s.grid;
            let (x0, y0, x1, y1) = (
                c[0] - g / 2.0,
                c[1] - g / 2.0,
                c[0] + g / 2.0,
                c[1] + g / 2.0,
            );
            for t in [
                [(x0, y0), (x1, y0), (x1, y1)],
                [(x0, y0), (x1, y1), (x0, y1)],
            ] {
                stl.push_str("facet normal 0 0 1\nouter loop\n");
                for (x, y) in t {
                    stl.push_str(&format!("vertex {x} {y} {z}\n"));
                }
                stl.push_str("endloop\nendfacet\n");
            }
        }
    }
    let a = cambench::camotics::agreement(s, stl.as_bytes(), 0.05).unwrap();
    assert!(
        a.cells > 0 && a.fraction() > 0.999,
        "{} of {}",
        a.agree,
        a.cells
    );
    let shifted = stl.replace("vertex 20 ", "vertex 21 ");
    let b = cambench::camotics::agreement(s, shifted.as_bytes(), 0.05).unwrap();
    assert!(b.fraction() <= a.fraction());
}
