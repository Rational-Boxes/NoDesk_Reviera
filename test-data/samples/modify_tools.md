# Editing tools: building it by hand

How to build `modify_tools.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet ([gaps](#gaps)). The script `modify_tools.lua` is the reference.

Start with **File › New**, and open a Level 1 plan (View › Floor Plan). Walls are Architecture ›
**Wall** (type *Generic - 200mm*, 3000 high, the tool's default), drawn by typing their points;
columns are Architecture › **Column**, Unconnected Height 3000; grids are Architecture › Datum
& Work Plane › **Grid** (rename one by editing its Name in Properties). The Modify tools are on
Modify › Modify; select first, then start the tool, unless the step says otherwise.

1. **Align.** Grid *A* from (−500, 3000) to (8500, 3000). Walls from (0, 500) to (2000, 500),
   (2800, 1200) to (4800, 1200) and (5600, 1900) to (7600, 1900). **Align** (AL): click grid A,
   then each wall's north (exterior) face: all three faces land on the grid.
2. **Offset.** A wall from (12000, 500) to (18000, 500). **Offset** (OF), type 2000, click the
   wall with the cursor north of it: a copy at y = 2500.
3. **Mirror – Pick Axis.** Grid *1* from (27000, −500) to (27000, 4500). A wall from (24500, 0)
   to (24500, 4000), with a door 1500 along it (Architecture › **Door**). Select the wall,
   **Mirror – Pick Axis** (MM), click grid 1: a copy at x = 29500, its door turned.
4. **Mirror – Draw Axis.** Walls from (36000, 0) to (41000, 0) and on to (41000, 1200), with a
   window 2500 along the first (Architecture › **Window**). Select both walls, **Mirror – Draw
   Axis** (DM), click (35000, 2000) and (42000, 2000).
5. **Move.** A column at (1000, 13000); **Move** (MV), click it and type 4000 east.
6. **Copy.** A wall from (12000, 12000) to (17000, 12000) with a window 2500 along it.
   **Copy** (CO) north 1500; **Copy** again north 3000.
7. **Rotate.** A wall from (24000, 12500) to (29000, 12500). **Rotate** (RO), type 30: it turns
   about the selection's centre, the wall's middle.
8. **Array.** A column at (36000, 13000). **Array** (AR): 5 members, Move To *2nd*, click the
   column's centre and type 1500 east.
9. **Radial Array.** A column at (7000, 28000). **Radial Array**: 8 members, Move To *Last*,
   type 360. The tool turns the copies about the selection's centre, here the column's own, so
   the circle's centre at (4000, 28000) cannot be picked **[gap]**.
10. **Array along Path.** In the Component Library (Architecture › Component › **Component
    Library**), open `test-data/library` and click *Classical/Columns/parthenon_column*: Place
    Component starts holding it. **Array along Path** (AP) takes the held column; in Properties
    choose *Arc (3 points)*, Number 5, Orientation *Same*, then click (12000, 25000), (16000,
    29000) and (20000, 25000). The script names the array *Arc colonnade*; the window has no
    Name field for a path array **[gap]**.
11. **Unarray.** Array along Path again with the same column, Number 4, a polyline from (24000,
    26000) to (30000, 26000), Enter. Select the array and **Unarray**: four independent objects.
    Select one of them and set Scale (type) 0.3 in Properties: every copy of the column, the arc
    array's too, follows.
12. **Create Component.** Walls from (36000, 24000) to (40000, 24000) and on to (40000, 26000),
    a door 2000 along the first. In the Component Library, **New…** makes a library in a folder
    of your choice. Select the two walls and **Create Component**, named *Corner Bay* (the door,
    type and level come along). Then Architecture › **Component**, pick *Corner Bay* and click
    (36000, 28000).
13. **Trim/Extend to Corner.** Walls from (0, 36000) to (6000, 36000) and from (5000, 37000) to
    (5000, 40000). **Trim/Extend to Corner** (TR): click the first near (2000, 36000), the second
    near (5000, 39000). The first is trimmed and the second extended to an L at (5000, 36000).
14. **Trim/Extend Single Element.** Grid *2* from (16000, 35000) to (16000, 41000). Walls from
    (12000, 37000) to (14000, 37000) and from (12000, 39000) to (19000, 39000). **Trim/Extend
    Single Element**: click grid 2, then each wall near x = 13000: the first extends to the
    grid, the second is trimmed back to it.
15. **Split.** A wall from (24000, 36000) to (30000, 36000); **Split Element** (SL), click at
    (27000, 36000). Another from (24000, 38500) to (30000, 38500); select it and Modify | Walls ›
    **Split with Gap**, click at (27000, 38500) and type 100.
16. **Pin and Unpin.** Walls from (36000, 36000) to (41000, 36000) and from (36000, 38000) to
    (41000, 38000). Select the first, **Pin**. Select the second, **Pin**, then **Unpin**, then
    **Move** it 1000 north.
17. **A layered wall.** Draw any wall, select it, in Properties › Type choose Duplicate Type… as
    *Cavity Wall 265*, then Modify | Walls › **Edit Assembly**: Brick 100 (Finish 1), Insulation
    50 (Thermal/Air), Block 100 (Structure), Plaster 15 (Finish 2). Delete the trial wall and draw
    one of the new type from (0, 48000) to (10000, 48000).
18. **Parts.** Select the wall and Modify › Parts › **Create Parts**: four parts, one per layer.
    - Set the work plane on the wall's face (Architecture › Datum & Work Plane › **Set Work
      Plane**) and draw three vertical model lines (**Model Line**) at x = 2500, 5000 and 7500.
      Select the brick part and the lines, **Divide Parts**, Gap 10: four brick panels.
    - The same with lines at x = 3000 and 6000 for the plaster part, no gap. Select the first two
      plaster pieces and **Merge Parts**.
    - Select the insulation part and **Exclude Parts**.
19. **Labels.** Annotate › **Text**: click below each cell and type its name (*Align: three
    walls' exterior faces onto grid A*, …).

Check: View › **Check Overlaps** finds none. The Model Tree lists 30 walls, 18 columns (14
columns and the 4 unarrayed objects) and one path array with five members.

## Gaps

- **Radial Array's centre.** The tool always turns about the selection's centre (as Rotate does);
  there is no way to move the centre, so columns round a circle about another point need the
  script (`element.array_radial` takes a `center`).
- **Array and Radial Array make independent copies.** *Group and Associate* (an array element
  that keeps its members and count editable) needs groups as array members and is not available
  yet; only Array along Path keeps an array element.
- **Naming a path array.** `array.along_path` takes a `name`, but the array's Properties have
  no Name field.
- **Parts of layered floors and roofs** are not available yet (a floor gives one part), so the
  parts are shown on a wall.
