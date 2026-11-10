---
name: cambench
description: Write and check CAM toolpaths with cambench, where a `.gcam` file lists one operation per line (stock, tools, face, pocket, adaptive, profile with tabs and lead-in, drill, chamfer, engrave, laser cut and raster) and the `cambench` CLI simulates the stock removal, compares it with a gcad part exported to STL, and writes grbl, linuxcnc, marlin (laser) or fanuc G-code. Use when writing or editing `*.gcam` files, turning a gcad part into G-code, or checking feeds, gouges, holder clearance and tabs without a machine.
---

# cambench

A `.gcam` file runs top to bottom, one operation per line, `key=value` options after the positional words. Lines starting with `#` are skipped. Units are mm, Z=0 is the stock top, cuts go negative, X/Y origin is the stock corner.

```
machine rapid=3000 clearance=5 grid=0.25 tol=0.05
stock 60 40 10 material=aluminium
tool 1 endmill 6 flutes=2 flute_len=20 holder=20
tool 2 vbit 12 angle=90
part bracket.stl at=0,0,0
face tool=1 depth=0.5
pocket rect 10 10 40 20 tool=1 depth=4
adaptive circle 30 20 6 tool=1 depth=6
drill 5,5 55,5 tool=1 depth=3
chamfer rect 10 10 40 20 tool=2 width=1
engrave 12,35 48,35 tool=2 depth=0.5
profile rect 0 0 60 40 tool=1 depth=10 side=outside tabs=4 lead=3
```

Shapes: `rect X Y W H`, `circle CX CY R`, `poly X,Y X,Y X,Y ...`. Tools: `endmill`, `ball`, `vbit angle=`, `laser`. Materials: `cambench materials`.

Operations and options:

- `face depth=`
- `pocket SHAPE depth=` and `adaptive SHAPE depth=` (adaptive uses concentric rings with a small stepover; rect and circle only)
- `profile SHAPE depth= side=outside|inside|on tabs=N tab_w= tab_h= lead=`
- `drill X,Y ... depth= peck=`
- `chamfer SHAPE width=` (vbit), `engrave X,Y ... depth= | width= closed=1` (vbit)
- laser: `cut SHAPE side= power= feed= passes= kerf=`, `raster SHAPE line= depth= power= feed=`
- every milling op accepts `tool= feed= plunge= rpm= stepdown= stepover=`; defaults come from the material and tool table.

Workflow:

1. `cambench check part.gcam` plans, simulates, and back-plots every post. Exit code 1 means findings: gouge, remaining stock, rapid through material, holder collision, laser on during rapid, depth.
2. Fix the `.gcam` (shorter stepdown, longer `flute_len`, deeper cut, slower laser feed) and rerun.
3. `cambench post part.gcam grbl out.nc` (also `linuxcnc`, `marlin` for lasers only, `fanuc`).
4. `cambench view part.gcam out.svg` for the toolpath over the simulated stock, `cambench info` for time and removed volume.

To compare with a gcad part: `gcad export part.gcad part.stl`, then add `part part.stl at=X,Y,Z` (place the part in stock coordinates, top at Z=0). A `part` of `.gcad` or `.gasm` runs `gcad export` itself. The comparison is a top-surface height field, so it checks 2.5D work (pockets, profiles, faces), not undercuts.
