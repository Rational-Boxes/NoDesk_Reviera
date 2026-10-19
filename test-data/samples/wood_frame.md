# Wood-frame house: building it by hand

How to build `wood_frame.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet, and the [gaps](#gaps) section lists them. The script `wood_frame.lua`
is the reference: the same commands with exact values.

Start with **File › New**. Set the code to *IRC* (Manage › Settings › **Model Properties**).

1. **Levels.** Structure › Datum › **Level**: Level 2 at 9'-1", Roof Plate at 18'-2".
2. **Walls.** Level 1 plan, Structure › **Wall**: a 32'-0" × 24'-0" rectangle clockwise from
   (0, 0), structural (Bearing), top Level 2 with Top Offset −10" (under the joists and
   subfloor). Level 2 plan: the same walls up to the Roof Plate.
3. **Stud wall type.** Select the walls; Properties › Type › *Duplicate Type…* as *Exterior - 2x6
   Stud*, and *Edit Assembly…*: siding 5/16", OSB sheathing 7/16", Structure *Wood
   Spruce-Pine-Fir No.1/No.2* 5 1/2", gypsum board 1/2".
4. **Strip footings.** Select the Level 1 walls, Structure › Foundation › **Wall**.
5. **Sections.** From *NA lumber*: Structure › **Load Beam Section** 2x10, 2x6, 2x4 and 4x12 (set
   its Structural Material to *Wood Douglas Fir-Larch No.2*); with the Column tool active,
   Properties › Type › *Load Section…* 4x6 in Douglas Fir-Larch.
6. **Posts and flush beam.** Level 1 plan, Structure › **Column**: 4x6 at 8', 16' and 24' on the
   house's centre line, top Level 2 offset −1'-0" (under the beam). Level 2 plan, Structure ›
   **Beam**: 4x12 along the centre line, Start and End Level Offset −3/4". Select the posts,
   Structure › Foundation › **At Columns**.
7. **Joists.** Level 2 plan, Structure › **Beam System** in each half (wall to beam): 2x10,
   layout *Fixed Distance* 16", spanning across the house, offset −3/4".
8. **Subfloor.** Structure › **Structural Floor**: the outline, span along x (across the joists);
   duplicate its type as *Subfloor - 3/4" Plywood*, one Structure layer of Douglas Fir-Larch,
   3/4".
9. **Slab on grade.** Level 1 plan, Structure › Foundation › **Slab**: the outline.
10. **Trusses.** Roof Plate plan, Structure › **Truss**: across the 24' width every 24" (17
    trusses); in a member's Properties set Layout *Fink*, Profile *Gable*, Truss Height 6'-0"
    (a 6:12 roof), chords 2x6 and webs 2x4.
11. **Bearings.** Modify › **Overlaps**: accept the truss members at their joints and on the
    walls, and the footings under the slab.
12. **Analytical model.** Analyze › Analytical › **Physical to Analytical**, Loads › **Column
    Base Supports**. Loads › **Point Support** (pinned): click the wall panels' bottom corners in
    the Level 1 plan, and the joist, beam and truss ends over the exterior walls in the Level 2
    and Roof Plate plans.
13. **Load cases and loads.** Analyze › Loads › **Load Cases**: DL (dead, self-weight), LL
    (live), SNOW. Level 2 plan, Loads › **Area Load** inside the subfloor: DL −0.48 kPa, LL −1.92
    kPa.
14. **Combinations.** Analyze › Loads › **Generate Combinations** (ASCE 7).
15. **Check.** Analyze › Checks › **Check Consistency**: seven warnings where the wall panels'
    top corners sit a joist's depth below the joist ends (the joists bear on the walls through
    their supports, not through shared nodes).

## Gaps

None: every step has its UI (clicking many supports one by one is slow: the script does it in
a loop).
