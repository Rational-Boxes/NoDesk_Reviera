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
   x (across the infill beams). Duplicate its type as *Concrete Slab 6"* with one Structure layer of *Concrete f'c 4000
   psi*, 6" (Properties › Type › *Edit Assembly…*). Repeat in the Roof plan.
9. **Columns through slabs.** View › Coordination › **Clash Detection** (or Modify ›
   **Overlaps**): resolve each column × slab by *Cut B by A*, and accept each brace's ends.
10. **Footings.** Select the columns, Structure › Foundation › **At Columns**.
11. **Analytical model.** Analyze › Analytical › **Physical to Analytical** (members split where
    the infill beams frame into the girders, panels for the slabs), Loads › **Column Base
    Supports**.
12. **Load cases and loads.** Analyze › Loads › **Load Cases**, Load Cases tab: DL (dead,
    self-weight), LL (live), LR (roof live), WIND. Level 2 plan, Loads › **Area Load**: DL −2.39
    kPa and LL −2.39 kPa inside the slab; Roof plan: DL and LR −0.96 kPa. Roof plan, Loads ›
    **Point Load**: WIND, Fx 15 kN, at each grid A column top.
13. **Combinations.** Analyze › Loads › **Generate Combinations** (ASCE 7).
14. **Check and run.** Analyze › Checks › **Check Supports** and **Check Consistency** (none);
    Structural Analysis › **Run Analysis**, then **Results Explorer** (for example the *Beam
    moments with deformation* preset).
15. **Connections.** Select every column, Structure › **Connection**, *Base Plate*: a pinned
    base plate with four anchor rods under each. At Level 2, select a column and a W18X35
    girder that frames into it, **Connection**, *Shear Plate*; repeat at each girder end. At each
    brace's foot, select the column then the brace, **Connection**, *Gusset*. Set the 3D view's
    Detail Level to *Fine* to see their plates, bolts and welds; a connection's Properties show
    its type and parameters. Accept the brace tops against the girders' shear plates (Modify ›
    **Overlaps**).

## Gaps

None: every step has its UI.
