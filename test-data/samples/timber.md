# Mass timber frame: building it by hand

How to build `timber.ifc` in the window. Each step names the UI path; **[gap]** marks a step the
window cannot do yet, and the [gaps](#gaps) section lists them. The script `timber.lua` is the
reference: the same commands with exact values.

Start with **File › New**. Set the code to *DIN 18065* (Manage › Settings › **Model
Properties**): the European region, so Eurocode materials and EN 1990 combinations apply.

1. **Levels.** Structure › Datum › **Level**: Level 2 at 3500, Level 3 at 7000, Roof at 10500.
2. **Grids.** Level 1 plan, Structure › Datum › **Grid**: 1–4 at 6000 along x, A–C at 7500
   along y.
3. **Sections.** From *EN timber*: with the Column tool active, Properties › Type › *Load
   Section…* 300x300 and set Structural Material *Glulam GL24h (EN 14080)*; Structure › **Load
   Beam Section** 150x300 in *Glulam GL28h*; **Load Brace Section** 150x150 in *Glulam GL24h*.
4. **Columns.** Structure › Column › **At Grids**, storey by storey: Level 1 to Level 2 (top
   offset −200); Level 2 to Level 3 (base and top offset −200); Level 3 (base offset −200) to the
   Roof. Each storey's columns stand on the beams below.
5. **Beams.** In each upper plan, Structure › **Beam**: 150x300 between the columns along grids
   A, B and C, Start and End Level Offset −200 (under the CLT).
6. **CLT floors.** Structure › **Structural Floor** on Levels 2, 3 and the Roof: the outline,
   span along y. Duplicate its type as *CLT 200 (5 layers)*, one Structure layer of *Timber C24
   (EN 338)*, 200 (Properties › Type › *Edit Assembly…*).
7. **Bracing.** Elevation on grid A, Structure › **Brace**: 150x150 diagonals in the end bays,
   every storey.
8. **Columns through the CLT.** Modify › **Overlaps**: resolve each column × floor by cutting
   the floor; accept the braces' ends.
9. **Footings.** Select the Level 1 columns, Structure › Foundation › **At Columns**.
10. **Analytical model.** Analyze › Analytical › **Physical to Analytical** (columns split where
    the floors and beams meet them, panels for the CLT), Loads › **Column Base Supports**.
11. **Load cases and loads.** Analyze › Loads › **Load Cases**: G (permanent, self-weight), Q
    (imposed) and S (snow). In each upper plan, Loads › **Area Load** inside the CLT: G −1.5 kPa on
    every floor, Q −3.0 kPa on Levels 2 and 3, S −1.0 kPa on the roof.
12. **Combinations.** Analyze › Loads › **Generate Combinations** (EN 1990 6.10).
13. **Check.** Analyze › Checks › **Check Supports** and **Check Consistency** (none).

## Gaps

None: every step has its UI.
