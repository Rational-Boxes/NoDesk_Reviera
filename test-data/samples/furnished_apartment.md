# A furnished apartment: building it by hand

How to build `furnished_apartment.ifc` in the window. Each step names the UI path; **[gap]** marks
a step the window cannot do yet, and the [gaps](#gaps) section lists them. The script
`furnished_apartment.lua` is the reference: the same commands with exact values.

Start with **File › New**. In **Manage › Model Properties**:
- set the **Accessibility rule set** to *2010 ADA Standards*;
- under **Interiors rules**, also tick *Interior planning practice*.

Room layouts fit each room with these packs' clear spaces.

1. **The walls.** Draw the outer walls, 200 mm thick and 2.7 m high, from (0, 0) to (15.8, 4.6)
   on their centre lines, so the inside runs 0.1–15.7 × 0.1–4.5. Add partitions at x = 4.2, 9.4
   and 13.0. That gives a 4.0 m bedroom, a 5.0 m living room, a 3.4 m dining room and a 2.6 m
   office, each 4.4 m deep.
2. **Furnish each room.** For each room, choose **Architecture › Build › Room Layout**, pick the
   template, then click inside the room:
   - *Bedroom, queen bed* in the bedroom: the bed's head on a long wall with a nightstand each
     side, and the dresser opposite. Each side of the bed keeps its 30 × 48 in space (ADA
     §806.2.3), and the foot keeps 24 in;
   - *Living room*: the sofa on a long wall, the coffee table 450 mm in front of it, an
     armchair each side, and the storage unit opposite;
   - *Dining room, 4 seats*: the table in the middle with 36 in clear round its edge, and two
     chairs on each long side;
   - *Home office*: the desk on a long wall, its chair, and a bookcase on the side wall.

   The pieces come from the bundled Interiors library's catalogue. The main piece goes against
   one of the longer walls; the layout may also name the wall.
3. **Rooms.** Place an **Architecture › Room** in each room and name it: *Bedroom*, *Living
   room*, *Dining room*, *Office*. **View › Coordination › Check Rules** finds nothing.
4. **Too small.** Try *Bedroom, queen bed* in the office. It is refused, with the shortfall: about
   180 mm across the room, because the bed and the 24 in at its foot need 2.78 m and the office
   is 2.6 m deep.
5. **The schedule.** Choose **View › Schedules › New Schedule** › *FF&E*. Each row is one type in
   one room, counted.

## Gaps

- **[gap]** A layout fits its own pieces only. Furniture already in the room is not avoided, so
  use *Replace it* for an earlier layout, or clear the room first.
- **[gap]** Rooms must be rectangles closed by four walls; L-shaped rooms and open plans come later.
- **[gap]** No doors are modelled here. A door's swing and maneuvering clearances are not yet
  part of a layout's fit.
