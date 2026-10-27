# Annotated plan: building it by hand

How to build `annotated_plan.ifc` in the window. Each step names the UI path; **[gap]** marks a
step the window cannot do yet ([gaps](#gaps)). The script `annotated_plan.lua` is the reference.

Start with **File › New**, and open a Level 1 plan (View › **Floor Plan**).

1. **The bungalow.** Architecture › **Wall**, 3000 high, clockwise round (0, 0), (0, 6000),
   (10000, 6000), (10000, 0). Partitions from (6000, 0) to (6000, 6000) and from (6000, 3000)
   to (10000, 3000).
2. **Doors and windows.** Architecture › **Door** (900 wide, the default): the south wall at
   6000 from its start (x = 4000), the partition at x = 6000 at 1500 and 4500 from its start.
   Architecture › **Window** (1200 wide): the south wall at 8000 and 2000 from its start (x =
   2000 and 8000), the east wall at 4500 (y = 1500), the north wall at 3000 and 8000. Set each
   one's exact place in Properties › Placement › From Wall Start. Doors and windows get their
   Marks (1, 2, 3 …) as they are placed.
3. **Rooms.** Architecture › Room & Area › **Room**: click at (3000, 3000), (8000, 1500) and
   (8000, 4500); in Properties name them *Living*, *Bedroom* and *Kitchen* (numbers 1, 2, 3).
4. **Dimension strings.** Annotate › **Aligned Dimension** (DI), clicking each reference in turn
   and then the place for the line:
   - South, overall: the west wall's exterior face, the partition's centreline, the east wall's
     exterior face; the line 2000 outside the wall (y = −2000). It reads 6100 and 4100.
   - South, openings: the west face, the jambs of the living-room window and the front door, the
     partition's centreline, the jambs of the bedroom window, the east face; the line at
     y = −1200 (1500, 1200, 950, 900, 1550, 1400, 1200, 1500).
   - East, overall: the south wall's exterior face, the bedroom–kitchen partition's centreline,
     the north wall's exterior face; the line at x = 12000 (3100, 3100).
   - Living room: the west wall's interior face and the partition's living-room face, the line
     at y = 5000 (5800).
5. **Notes.** Annotate › **Text** (TX): click (0, −3000) and type *GROUND FLOOR PLAN 1 : 100*;
   set its type to *Text 5 mm* in Properties. Again at (1000, 8200): *100 mm stud partitions,
   plasterboard both sides* (two lines). **[gap]** Its leader, through (5000, 7600) to the
   partition at (6000, 5600): Properties show a note's leaders but cannot add one.
6. **Tags.** Annotate › **Tag by Category** (TG): click the front door, drag its tag to (4000,
   1200) and set Leader *Attached* in Properties; click the living-room window and drag its tag
   inside, to (2000, 800). Annotate › **Room Tag** (RT): click inside the living room.
7. **Tag All.** Annotate › **Tag All Not Tagged**: tick Doors, Windows and Rooms. The other two
   doors, four windows and two rooms are tagged (8 tags); the ones tagged in step 6 are not
   tagged again.
8. **Schedules.** View › **Schedules**, *Doors*, then again for *Rooms*. Each opens in its tab:
   three doors (900 × 2100), and the three rooms with their areas (33.64, 10.64 and 10.64 m²).
9. **Section.** View › **Section**: in the plan click (11000, 1500) then (−1000, 1500), so it
   looks north through the bedroom door and window. In Properties set Far Clip Offset 8000;
   rename it *Section A* (Project Browser, Rename).
10. **Elevation.** View › **Building Elevations** makes North, South, East and West; keep
    *South* and delete the other three in the Project Browser. **[gap]** The script places one
    elevation by its point and width (5 m south of the house, 14 m wide); the window places all
    four from the model's extents.
11. **Section and elevation annotation.** Open *South* (Project Browser). Annotate › **Aligned
    Dimension** (DI): click the Level 1 line, then the wall's top edge, then click in empty space
    left of the house (3000); again from the west wall's outer edge to the east wall's, placed
    above the roof line (10200). **Tag by Category** (TG): click the living-room window and drag
    its tag up beside it. **Text** (TX): click on the wall between the door and the east window
    and type *RENDERED BLOCKWORK*. Open *Section A*: dimension the Level 1 line to the partition's
    top (3000) left of the house, and the living room's west wall's inner face to the partition's
    face (5800); tag the bedroom door; and put a text note *BEDROOM* above the bedroom. In the
    Default 3D View, View › Annotation › **Annotation Visibility…** › *From*: tick Sections and
    Elevations to see their annotation float in 3D; the elevation's note sits on the south wall.
12. **Spots.** In the Level 1 plan, Annotate › **Spot Elevation** (EL): click the north wall's
    top near its west end, then click above the wall for the text (+3000). **Spot Coordinate**:
    click the north-east corner, then up and to the right (N 6000, E 10000). In *South*, Spot
    Elevation on the wall between the east window and the east end, text to the right of the
    house; in Properties set Display Elevations to *Top & Bottom Elevations* (T +3000, B ±0).
13. **Sheet.** View › **Sheet**, *A2*: it is numbered A101. In Properties name it *Ground
    Floor*, Drawn By *Reviera*, Issue Date *2026-10-26*. Its Sheet tab › **Place View**: the
    Level 1 plan, then *Section A*, then *South*; **Place Schedule**: the door and room
    schedules. Drag them into place: the plan at the left, the section and elevation to its
    right, the schedules below.

Check: View › **Check Overlaps** finds none. The tags read the Marks and room names live: change
a door's Mark and its tag and the schedule follow. File › **Export PDF** with the sheet open
prints it with its title block.

## Gaps

- **Leaders on text notes** (step 5): `text.add_leader` has no UI; Properties list a note's
  leaders read-only.
- **One elevation at a chosen place** (step 10): `view.create_elevation` takes a point, a
  direction and a width; the window has only Building Elevations, all four round the model.
- **Keynotes**: this sample has none; the Managed project sample shows keynote tags.
