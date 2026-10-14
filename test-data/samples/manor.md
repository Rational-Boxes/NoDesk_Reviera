# Manor with tower: building it by hand

How to build `manor.ifc` in the window. Each step names the UI path; **[gap]** marks a step the
window cannot do yet, and the [gaps](#gaps) section lists them. The script `manor.lua` is the
reference: the same commands with exact values.

Start with **File › New**. The model has Level 1 at 0.

1. **Code.** Manage › Settings › **Model Properties**: Building code *International Residential
   Code (US)*, Occupancy *Within a dwelling unit*. OK.
2. **Levels.** Architecture › Datum › **Level**: add levels at 3000 and 6000.
3. **Ground-floor walls.** Level 1 plan, Architecture › **Wall** (3000 high): a 14000 × 9000
   rectangle clockwise from (0, 0). Then the hall partition from (7000, 0) to (7000, 9000);
   select it and set Unconnected Height 2700 (it stops under the floor above).
4. **Upper walls.** Level 2 plan, Wall: the same rectangle, and a partition from (7000, 3200) to
   (7000, 9000).
5. **Types.** Place one door and one window (they make the default types), then select them and
   in Properties › Type choose *Duplicate Type…* for each new type, setting its values in Type
   Parameters:
   - *Arched Door 1000 x 2400*: Width 1000, Height 2400, Shape *Round Top*.
   - *Round Top 1000 x 1800*: Width 1000, Height 1800, Default Sill Height 600, Shape *Round
     Top*, Glazing Bars 3.
   - *Gothic 800 x 1800* (from Round Top): Width 800, Head Profile *Gothic*.
   - *Oculus 900*: Shape *Round*, Width 900, Glazing Bars 4.
6. **Doors and windows.** With the Door and Window tools (Properties › Type picks the type), on
   the south front: the arched door at 10.5 m from the wall's start (its east end), round-top
   windows at 12.8, 8.0, 4.5 and 2.2 m, and upstairs at 12.0, 9.0, 5.0 and 2.0 m (Sill 800), the
   oculus at 7.0 m (Sill 1200). North: windows at 3.5 and 10.5 m on both floors, and the back
   door at 7.6 m with Flip Facing (it swings in). East and west: a Gothic window at 4.5 m on
   both floors. The hall partition: a door at 6.0 m. Set exact positions with Properties ›
   Placement › From Wall Start.
7. **Floors.** Architecture › **Floor** on Level 1 and Level 2: a rectangle from (100, 100) to
   (13900, 8900).
8. **Rooms and ceilings.** Architecture › **Room** on Level 1: click in each half (*Living*,
   *Hall and Kitchen*). Architecture › **Ceiling**: click in each room. On Level 2, rooms
   *Bedroom* and *Study*.
9. **Main stair.** Level 1 plan, Architecture › **Stair**: click (8000, 1000) and move east: past
   about 4.2 m it snaps to Level 2 ("reaches Level 2"); click. (Drawn shorter, Esc completes
   it.) Set Run Width 1100 in Properties. The stair offers to open the ceiling and the Level 2
   floor over it for the IRC's 6 ft 8 in: OK.
10. **Guard.** Level 2 plan, Architecture › **Railing**: sketch along the opening's north edge,
    50 mm onto the floor.
11. **Roof.** Architecture › **Roof** on the upper walls' exterior faces, 100 mm out, Base Offset
    6000, Shape *Hip*. Modify | Roofs › **Eaves**: Overhang 600, Fascia *Yes*. **Dormer** ›
    *Gable*: click the south slope at (7000, 600); Front Height 800.
12. **Tower walls.** Level 1 plan, Architecture › **Arc Wall**: two half circles of radius 2600
    about (19500, 4500), 9400 high, clockwise (north half from west to east, south half back).
13. **Tower doors and windows.** The arched door on the south half at its middle; Gothic windows
    at Sill 1000 and 4000 on the north-west, at 1400 and 4400 on the south-east, at 7000 on the
    south-west; the oculus at the north, Sill 7400.
14. **Tower floors.** Architecture › **Floor** on each level: a circle of radius 2500 about the
    centre (Sketch › Circle).
15. **Spiral stairs.** Level 1 plan, Architecture › **Spiral Stair**: click the centre, then the
    walking line 1200 east of it. Set Run Width 900. Accept the floor opening. Repeat on the
    Level 2 plan.
16. **Tower room.** Architecture › **Room** in the tower: *Tower*.
17. **Dome.** Architecture › **Dome**: centre (19500, 4500), radius 2900, Base Offset 9400,
    Profile *Onion*; accept *Attach the walls*.
18. **Deck.** Architecture › **Deck**: a rectangle from (4000, 9200) to (12000, 12500).
    Architecture › **Railing**: sketch its west, north and east edges 50 mm in, then Modify |
    Railings › **Gate** at 7.2 m along it.
19. **Views.** View › **Floor Plan** for each level, and the 3D view.

## Gaps

None: every step has its UI.
