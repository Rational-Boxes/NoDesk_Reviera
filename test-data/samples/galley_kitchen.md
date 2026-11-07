# A galley kitchen: building it by hand

How to build `galley_kitchen.ifc` in the window. Each step names the UI path; **[gap]** marks a
step the window cannot do yet, and the [gaps](#gaps) section lists them. The script
`galley_kitchen.lua` is the reference: the same commands with exact values.

Start with **File › New**. In **Manage › Model Properties**, set the **Building code** to the
*IRC*: cabinet runs then use US sizes.

1. **The walls.** Draw four walls, 200 mm thick and 2.7 m high, round (0, 0)–(4.0, 2.7) on their
   centre lines. The inside is 3.8 × 2.5 m.
2. **The south base run.** In a plan, choose **Architecture › Build › Cabinet Run**, pick *Base
   units*, and click the run's start and end on the south wall's inside face, (0.1, 0.1) to
   (3.9, 0.1). The 3.8 m is filled with standard widths, 36 in units and one or two of 9–48 in
   (3 in steps), to leave the least filler: 36 + 36 + 36 + 39 in and a 66 mm filler. The units face away from
   the wall. Each is 24 in deep and 34½ in high on a 4 in toe kick.
3. **Wall units.** Choose **Cabinet Run** again with *Wall units* and click the same two points.
   They are 12 in deep and 30 in high, their bottoms at 54 in: 18 in over the counter.
4. **The north side.** On the north wall's face (y = 2.6), make a base run from x = 0.1 to 3.1,
   then a run of *Tall units* from 3.1 to 3.9: one 30 in, 84 in high pantry and a 38 mm
   filler.
5. **Countertops.** Choose **Architecture › Build › Countertop** with nothing selected. Each base
   run gets a top 1½ in thick, its back on the wall and its front 1 in past the units, so 25 in
   deep, with its surface at 36 in. Between the two counters 1.23 m is left clear.
6. **The room.** Place an **Architecture › Room** inside and name it *Kitchen*. **Analyze › Find
   Overlaps** finds nothing: no unit, filler or top stands in another or in a wall.

The units are *Casework* (IfcFurniture with their own types); the tops are IfcSystemFurnitureElement
*WORKSURFACE*. Both are listed in **View › Schedules › New Schedule** › *FF&E*.

## Gaps

- **[gap]** Runs are straight. Corner units (blind or lazy-susan) where two runs meet are not
  made yet, so leave the corner to a filler or draw the runs to stop short of each other.
- **[gap]** Appliances (range, sink base, dishwasher, refrigerator) are not yet placed in a run;
  give the run explicit widths and place the appliance in the gap.
- **[gap]** Countertops are rectangles per run; L and U tops, sink and cooktop cut-outs and
  backsplashes come later.
