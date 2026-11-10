# Changelog

## [0.1.0] - 2026-11-10

### Added
- `.gcam` planner, grbl, linuxcnc, marlin and fanuc posts, height-field simulation and CLI.
- G2 and G3 arcs in the posts, fitted from circular cuts and checked by the backplot.
- `cambench-ui`, an egui viewer with the stock, findings and G-code per post.
- `cambench camotics` writes a CAMotics project for the CI cross-check.
- `cambench agree` compares the simulated stock with a CAMotics result, and `scripts/camotics.sh` runs CAMotics 1.2.0 (camsim) in docker for the CI cross-check.
- A test that runs a gcad part through export, plan and simulation, skipped when gcad is missing.
