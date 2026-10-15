# Greek temple: building it by hand

How to build `temple.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet, and the [gaps](#gaps) section lists them. The script `temple.lua` is
the reference: the same commands with exact values.

Start with **File › New**. The model has Level 1 at 0. Open the sample Component Library,
`test-data/library` (its column is a public-domain scan; see its README).

1. **Levels.** Architecture › **Level**: *Stylobate* at 1500 and *Cornice* at 15000.
2. **The steps.** Level 1 plan, Architecture › **Floor**, three rectangles, each a type
   *Marble Step 500* (duplicate the default floor type, Thickness 500):
   - (-1500, -1500) to (32380, 71000), Height Offset 500;
   - (-750, -750) to (31630, 70250), Height Offset 1000;
   - (0, 0) to (30880, 69500), Height Offset 1500: the stylobate.
3. **The peristyle.** In the Component Library, *Classical › Columns › Parthenon Doric Column*;
   **Array along Path** with a drawn closed rectangle through the column axes, 1150 inside the
   stylobate's edge, on the Stylobate level: Distribution *Number* with 8 per front and 17 per
   flank (counts 8, 17), Corners *At every corner*, Orientation *Outward*. 46 columns. **[gap]**
4. **The cella.** Stylobate plan, Architecture › **Wall**, type *Marble Wall 1000* (duplicate
   Generic 200, Width 1000), Unconnected Height 10170: a rectangle from (7500, 9500) to
   (23380, 60000) and a cross wall at y = 40000.
5. **Doors.** Architecture › **Door**, type *Bronze Doors 4900 x 9000*, in the middle of the east
   and west walls.
6. **The porches.** Six columns before each end: **Array along Path** of the column along a drawn
   line from (7500, 62000) to (23380, 62000), and from (7500, 7500) to (23380, 7500), Number 6,
   Orientation *Fixed*. **[gap]**
7. **The entablature.** Cornice plan, Architecture › **Floor**, type *Entablature 3320*
   (Thickness 3320): the stylobate's rectangle with a hole 2400 inside it.
8. **The roof.** Architecture › **Roof** on the stylobate's rectangle at the Cornice level;
   Roof Shape *Gable*, Overhang 300, Slope 13.5°.
9. **Pediments.** Architecture › **Wall** at the Cornice level, Marble Wall 1000, Unconnected
   Height 5000, across each end 600 inside the roof's edge; then Modify | Roofs › **Attach
   Walls**.
10. **Views.** View › **Floor Plan** for Level 1, and the 3D view.

## Gaps

- **Array along Path** and the **Component Library** panel are built as commands
  (`array.along_path`, `library.*`, `component.place`; scripts and MCP) but not yet as window
  tools.
