# cambench

CAM toolpaths from a text `.gcam` file, verified by simulation instead of a machine.

```
cargo run -- check examples/plate.gcam
cargo run -- post examples/plate.gcam grbl plate.nc
cargo run -- view examples/plate.gcam plate.svg
```

- Operations: face, pocket, adaptive, profile (tabs, lead-in), drill, chamfer, engrave, laser cut and raster.
- Posts: grbl, linuxcnc, marlin (laser), fanuc, each a small template in `src/gcode.rs`.
- Checks: height-field stock removal with gouge, remaining stock, rapid-through-material and holder checks; comparison against an STL of the gcad part; every post's G-code is parsed back and compared with the planned toolpath.
- Viewer: egui window (`cambench-ui`) and SVG of the toolpath over the simulated stock with a time estimate.
- Agent skill: `skills/cambench/SKILL.md`.

Circular cuts post as G2 and G3 arcs. `cambench-ui [FILE.gcam]` opens an egui viewer. CI writes the linuxcnc G-code and a CAMotics project (`cambench camotics FILE DIR`) and runs `camsim` on it.

Limits: the stock model is a 2.5D height field (no undercuts or 4/5 axis).
