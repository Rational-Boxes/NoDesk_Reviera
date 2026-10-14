# House with dormers: building it by hand

How to build `house.ifc` in the window. Each step names the UI path; **[gap]** marks a step the
window cannot do yet, and the [gaps](#gaps) section lists them. The script `house.lua` is the
reference: the same commands with exact values.

Start with **File › New**. The model has Level 1 at 0.

1. **Second level.** Architecture › Datum › **Level**: add a level at 3000. (Its plan comes
   with step 12.)
2. **Exterior walls.** Open the Level 1 plan (Project Browser, Floor Plans › Level 1).
   Architecture › **Wall**: click (0, 0) and type `10000` along x, Enter. Continue up 7000,
   back 10000, and down to the start. Click clockwise so the exterior arrow points out (Spacebar
   flips). Walls are 3 m high, up to the level above.
3. **Partition.** Wall: click from (5000, 0) to (5000, 7000).
4. **Doors and windows.**
   - Architecture › **Door**: hover the south wall, click at 7.5 m from its start; hover the
     partition and click at its middle.
   - Architecture › **Window**: south wall at 2.5 m, north wall at 2.5 m and 7.5 m, east wall at
     3.5 m.
   - The positions are set by clicking; to set an exact distance along the wall, select the
     insert and drag it. **[gap: no Properties row for the distance along the wall]**
5. **Roof.** Architecture › **Roof**. Pick Walls (Tab picks the joined chain) takes the
   exterior faces. Finish, and accept *Attach the walls*. In Properties: Roof Shape *Hip*,
   Base Offset 3000.
6. **Eaves.** With the roof selected, Modify | Roofs › **Eaves**. In Properties › Eaves set
   Overhang 600 and Fascia *Yes* (Bearing *Exterior Face*, Soffit *Boxed* are the defaults).
7. **Dormers.** Modify | Roofs › **Dormer** › *Gable (pitched)*: click the front slope at
   (5000, 500). Again › *Shed (flat lift)*: click the back slope at (5000, 6500). Select each
   dormer and set Front Height 700 in Properties.
8. **Stairs.** Architecture › **Stair**: click the run's start (13000, 0) and end (15240, 0),
   then the second run (15240, 1400) to (13000, 1400): a half-turn stair. Again for an L: (13000,
   4000)–(15240, 4000), then (15840, 4600)–(15840, 6840). Each gets its railings.
9. **Front railing with gates.** Architecture › **Railing**: click (-1000, -2500), (11000,
   -2500), (11000, 9000), Esc. Select it; Modify | Railings › **Gate** and click the railing 3 m
   from its start; **Double Gate** at 8 m; **Opening** at 17 m (on the east run).
10. **Deck.** Architecture › **Deck**: sketch a rectangle (18000, 0)–(22000, 3000), Finish.
11. **Deck railing.** Railing: click (18050, 50), (21950, 50), (21950, 2950), Esc. Sketched
    over the deck, it stands on the boards.
12. **Plans.** View › **Floor Plan**: Level 1 and Level 2.

Check: View › **Check Overlaps** reports none.

## Gaps

- **Exact distance of a door or window along its wall.** Clicks and drags only; Properties
  should show and edit it.
