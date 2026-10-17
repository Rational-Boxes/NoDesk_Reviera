# Steel-framed office: building it by hand

How to build `steel_frame.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet, and the [gaps](#gaps) section lists them. The script `steel_frame.lua`
is the reference: the same commands with exact values.

Start with **File › New**. Set the code to *IBC* (Manage › Settings › **Model Properties**).

1. **Levels.** Elevation view, Structure › Datum › **Level**: Level 2 at 14'-0", Roof at 28'-0".
2. **Grids.** Level 1 plan, Structure › Datum › **Grid**: A–D at 30'-0" along x, 1–3 at 25'-0"
   along y.
3. **Sections.** Structure › **Load Beam Section**: W18X35, W16X26 and W12X26 from *AISC W*;
   **Load Brace Section**: HSS6X6X3/8 from *AISC HSS (rectangular)*; with the Column tool
   active, Properties › Type › *Load Section…*: W12X53.
4. **Columns.** Structure › Column › **At Grids**: pick all grids; W12X53, base Level 1, top Roof.
5. **Girders and beams.** Level 2 plan, Structure › **Beam**: W18X35 between the columns along
   grids 1, 2 and 3; W16X26 along grids A–D. Select them all and set Start and End Level Offset
   to −6" so the slab sits on top. Repeat in the Roof plan.
6. **Infill beams.** Structure › **Beam System** in each bay: W12X26, layout *Maximum Spacing*
   10'-0", spanning along y (between the girders), offset −6". Repeat in the Roof plan.
7. **Bracing.** South elevation, Structure › **Brace**: HSS6X6X3/8 crossing bay A–B on grid 1,
   both storeys (each end 6" below its level, except at Level 1).
8. **Slabs.** Level 2 plan, Structure › **Structural Floor**: the building outline, span along
   y. Duplicate its type as *Concrete Slab 6"* with one Structure layer of *Concrete f'c 4000
   psi*, 6" (Properties › Type › *Edit Assembly…*). Repeat in the Roof plan.
9. **Columns through slabs.** View › Coordination › **Clash Detection** (or Modify ›
   **Overlaps**): resolve each column × slab by *Cut B by A*, and accept each brace's ends.
10. **Footings.** Select the columns, Structure › Foundation › **At Columns**.
11. **Analytical model.** Analyze › Analytical › **Physical to Analytical** (members split where
    the infill beams frame into the girders, panels for the slabs), Loads › **Column Base
    Supports**.
12. **Load cases and loads.** **[gap]** Create cases DL (dead, self-weight), LL (live), LR (roof
    live), WIND; put 9.1 kN/m dead and live line loads on the Level 2 girders along grid 2, and
    15 kN along x at the tops of the grid A columns (`load_case.create`, `load.create_line`,
    `load.create_point` from the Lua console or MCP).
13. **Combinations.** Analyze › Loads › **Generate Combinations** (ASCE 7).
14. **Check and run.** Analyze › Checks › **Check Supports** and **Check Consistency** (none);
    Structural Analysis › **Run Analysis**.

## Gaps

- Load cases and applied loads have no dialog or placement tool yet: create them with the
  `load_case.*` and `load.*` commands.
