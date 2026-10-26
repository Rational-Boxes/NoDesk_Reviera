# Reinforced and precast concrete: building it by hand

How to build `concrete_reinforcement.ifc` in the window. Each step names the UI path; **[gap]**
marks a step the window cannot do yet ([gaps](#gaps)). The script `concrete_reinforcement.lua` is
the reference: the same commands with exact values.

Start with **File › New**. Set the code to *Approved Document K (England)* (Manage › Settings ›
**Model Properties**): metric bars and BS 8666 shape codes. Structure › Datum › **Level**: Level
2 at 3000; View › Create › **Floor Plan** for Level 1 and Level 2.

1. **Settings.** Concrete › Settings › **Reinforcement Settings**: *Round bar lengths*, bar
   length increment 10; add the cover types *Columns 40* (40) and *Walls 30* (30). Area and path
   reinforcement as real bars, so that an area system can be turned into sets (step 10): **[gap]**.
2. **Columns and footings.** Level 1 plan, Structure › Structure › **Column** (the default
   *Concrete Column 400 x 400*) at (0, 0) and (4000, 0), Level 1 to Level 2, top offset −750.
   Select both, Structure › Foundation › **At Columns**.
3. **The beam.** Duplicate the beam type (Properties › Type › *Duplicate…*) as *Concrete Beam 300
   x 500*: section rectangular, width 300, depth 500, material kind concrete. Level 2 plan,
   Structure › Framing › **Beam** from (0, 0) to (4000, 0), start and end offset −250.
4. **The slab.** Structure › Structure › **Floor** (structural) on Level 2 from (−200, −150) to
   (4200, 3100). Duplicate its type as *Concrete Slab 250*, one Structure layer of *Concrete*,
   250 (Properties › Type › *Edit Assembly…*).
5. **The wall.** Level 1 plan, Structure › Structure › **Wall** (structural) from (0, 3000) to
   (4000, 3000), unconnected height 2750. Duplicate its type as *Concrete Wall 200*, one
   Structure layer of *Concrete*, 200.
6. **Covers.** Select the columns, Concrete › Reinforcement › **Cover**: *Columns 40*; the wall,
   *Walls 30*.
7. **Column C1.** Select it, Concrete › Reinforcement › **Rebar**: shape 51, Ø10, section, maximum
   spacing 300. Select the links, Structural Rebar › Distribute › **Morphed**: four Ø20.
   **Column C2.** Concrete › Reinforcement › **Free Form**: aligned to the axis, Ø10, maximum
   spacing 300.
8. **The beam's bars.** Select it, **Rebar**: shape 51, Ø10, section, maximum spacing 300;
   **Morphed** through the links: four Ø16. Concrete › Reinforcement › **Sketch Rebar**: Ø16,
   corners (500, 150) and (3500, 150) in the beam's long section; Structural Rebar › Constraints ›
   **Edit Constraints**: the segment to the top cover, offset 12; its start and end to the start
   and end covers.
9. **The slab's bars.** Select the slab. Concrete › Reinforcement › **Area**: the whole host, Ø10
   at 200, the bottom major and minor layers only. **Path**: top face, spacing 200, primary Ø12
   1200 long; sketch the path from (300, −150) to (3700, −150), the bars to its left (into the
   slab). **Fabric Area**: *A252*, end and side laps 300, top face; sketch the rectangle
   (−200, 1400)–(1900, 3100). A Ø10 top layer at 200 in (2100, 1400)–(4200, 3100) with
   **Rebar**: the dialog lays a layer over the whole slab, not within a boundary **[gap]**.
10. **The wall's bars.** Select the wall. **Area**: the whole host, Ø12 at 200, the exterior
    major and minor layers only; select the system, **Remove Area System** (refused unless area
    reinforcement is real bars, step 1). **Fabric Sheet**: *B503*, cut by the cover, interior
    face, its first corner at (600, 300) and a point along x.
11. **Numbering partition.** Select the frame's rebar sets, Properties › Partition: *Frame*.
    Concrete › Settings › **Reinforcement Numbers** lists their numbers.
12. **Precast settings.** Concrete › Precast Settings › **Configuration**: solid walls' maximum
    length 2500.
13. **The precast wall.** Level 1 plan, Structure › Structure › **Wall** (structural, *Concrete
    Wall 200*) from (0, 7000) to (4500, 7000), unconnected height 2400; Architecture › Opening ›
    **Wall Opening** from 600 to 1400 along it. Select it, Concrete › Precast Detailing ›
    **Split**: pieces W1-1 and W1-2. Select both, **Auto Reinforce**: area type *A10-150*, edge
    type *U10-200*; **Mounting Parts**: two lifting anchors on each.
14. **The precast slab.** Structure › Structure › **Floor** (structural, *Concrete Slab 250*) on
    Level 1 from (0, 9000) to (4800, 11400). **Split** (solid): S1-1 and S1-2. **Custom Fabric**:
    a sheet at each face of each piece.
15. **Shop drawings.** Select the four pieces, Concrete › Precast Detailing › **Shop Drawings**,
    one sheet per piece (clear *One sheet per group of identical pieces*). **CAM Export** writes
    each piece to a file; it is not part of the model.
16. **Documentation.** Level 1 plan, select C1's links: Concrete › Documentation › **Rebar Tag**
    at (−1200, 800), **Bending Detail** at (−3000, −1000). Level 2 plan, select the slab's top
    layer: **Multi-Rebar Aligned** through (3800, 2200). Concrete › Documentation › **Bar Bending
    Schedule**.
17. **Labels.** Annotate › Text › **Text** for the notes in the Level 1 plan.

Check: View › **Check Overlaps** finds none. The tag reads *9 Ø10 @ 300*, the multi-rebar
annotation *10 Ø10 @ 200*. The schedule lists the Frame partition's numbers 1–9 and each precast
piece's sets under its mark.

## Gaps

- **Area and path reinforcement as real bars** (`reinforcement_settings.set`, key
  `area_path_rebar`): Concrete › Settings › Reinforcement Settings has rounding and cover types,
  not this switch, so Remove Area System is refused for systems made in the window.
- **A layer within a boundary** (`rebar.place`, placement `layer` with `boundary`): the Rebar
  dialog lays a floor's or wall's layer over the whole host.
