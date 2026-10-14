# Observatory: building it by hand

How to build `observatory.ifc` in the window. Each step names the UI path; **[gap]** marks a
step the window cannot do yet ([gaps](#gaps)). The script `observatory.lua` and the turret's
answers in `samples.json` are the reference values.

Start with **File › New**, and open a Level 1 plan (View › Floor Plan).

1. **The drum.** Architecture › **Arc Wall**: click (-5000, 0), then the end (5000, 0), then a
   point on the arc at (0, 5000): the north half. Again from (5000, 0) to (-5000, 0) through (0,
   -5000): the south half. Select both and set Properties › Constraints › Unconnected Height
   to 5000.
2. **Door and windows on the curve.** Architecture › **Door**: hover the south arc at its
   lowest point and click. **Window**: three on the north arc (north-west, north-east, north),
   one on the south arc (south-west); set each Sill Height to 1600 in Properties.
3. **Floor.** Architecture › **Floor**: Pick Walls on the drum's inner faces.
   **[gap: Pick Walls takes a curved wall's chord, not its curve; and sketches have no circle
   or arc tool]**
4. **Room.** Architecture › **Room**: click inside the drum. Set its Name to *Observation* in
   Properties.
5. **The dome.** Architecture › **Dome** › *Circle*, *Hemisphere*: click the centre (0, 0) and a
   point 5350 from it. It springs from the drum's top.
   **[gap: the dome's centre and radius cannot be typed or edited in Properties]**
6. **Drum attached.** With the dome selected, Modify | Roofs › **Attach Walls**: the drum
   rises to the dome's underside.
7. **The observing slot (CSG).**
   - Draw a cutter: a wall from (0, -600) to (0, 6000). In Properties › Type choose Duplicate
     Type… (*Slot cutter 800*) and set Width 800; under Constraints set Base Offset 6400 and
     Unconnected Height 6000.
   - View › Coordination › **Check Overlaps**: pick *dome × cutter*, then *Cut … by …* (the dome
     by the cutter).
   - **[gap: the cutter's body should then be hidden (`tools: hide`); the resolve dialog cannot
     ask for it]**
8. **The annex.** Wall: (8000, -3000) up to (8000, 3000), along to (16000, 3000), down to (16000,
   -3000), back to the start; Unconnected Height 3200. A door in the south wall at its middle, windows either
   side.
9. **The vault.** Architecture › **Arched Roof**: Pick Walls on the annex (Tab for the chain),
   Finish, accept *Attach the walls*: the end walls fill up to the arch. In Properties › Arch:
   Rise 1600, Overhang 300, End Overhang 400, Profile *Circular Segment*.
10. **Link walls.** Wall: (5050, -1200)–(8000, -1200) and (8000, 1200)–(5050, 1200), 3000 high.
11. **Garden wall.** Architecture › **Spline Wall**: click (-7000, -4000), (-4000, -8500),
    (2000, -9500), (9000, -7500), (13000, -8500), (17000, -7000), Enter. Height 1200.
12. **The turret.** Run the *Castle Turret* plugin: centre (19000, 5500), radius 1600, 16 sides,
    6500 high, *Onion dome* 3000, 3 arrow slits.
    **[gap: the window has no Scripts menu to run a plugin, nor a dialog for its form]**
13. **Accessibility rules.** Set the project's rule set to ADA 2010 (door maneuvering
    clearances); View › Coordination › **Virtual Volumes** shows them.
    **[gap: the rule set has no UI]**

Check: View › Coordination › **Check Overlaps** reports none.

## Gaps

- **Sketching curves:** Pick Walls on a curved wall should take its curve; sketches need Arc and
  Circle tools.
- **Dome Properties:** centre and plan (radius, sides) are only set by clicking.
- **Resolve dialog:** cutting or intersecting cannot hide the tool's body.
- **Scripts:** no Scripts menu to run plugins and scripts, and no form dialog for their input.
- **Accessibility rule set:** no UI to choose it.
