# Massing and site: building it by hand

How to build `massing_site.ifc` in the window. Each step names the UI path; **[gap]** marks a
step the window cannot do yet ([gaps](#gaps)). The script `massing_site.lua` is the reference.

Start with **File › New**, and open a Level 1 plan (View › Floor Plan).

1. **Levels.** Architecture › Datum & Work Plane › **Level** at 4000, 8000 and 12000; a Floor
   Plan for each (View › **Floor Plan**).
2. **The podium mass.** In the Level 1 plan, Architecture › Datum & Work Plane › **Model Line**:
   a closed rectangle (0, 0), (24000, 0), (24000, 14000), (0, 14000). Select the four lines,
   Massing & Site › Conceptual Mass › **In-Place Mass**: name *Podium*, height 8000.
3. **The tower mass.** The same from a rectangle (28000, 2000), (38000, 2000), (38000, 12000),
   (28000, 12000): name *Tower*, height 12000. Delete the model lines if you like.
4. **Mass floors.** Select each mass, Massing & Site › Conceptual Mass › **Mass Floors**, tick
   all four levels: the podium has two mass floors of 336 m², the tower three of 100 m² (none at
   its top face).
5. **Podium walls.** Select the podium, Massing & Site › Model by Face › **Wall by Face**, tick
   *form0.side0* … *form0.side3* (facing south, east, north, west). **[gap]** The script stands
   them on their interior finish face, outside the mass, so the floors fill the mass inside them;
   the window always uses the exterior finish face, with the walls inside the faces.
6. **Podium floors and roof.** With the podium selected, Model by Face › **Floor by Face** (a
   floor on each mass floor), then **Roof by Face** on *form0.top* (300 thick, on the face).
7. **Tower curtain system and roof.** Select the tower, Model by Face › **Curtain System** on
   its four sides (1500 grids each way, a mullion shared at each corner), then **Roof by Face**
   on its top.
8. **Stretch the tower.** **[gap]** The script raises the tower's form to 16000
   (`mass.set_form`); the window has no grip or property for a form's height. Then select the
   curtain system and the roof, Model by Face › **Update to Face**: both follow the new faces.
9. **Tower floors.** Select the tower, **Mass Floors** shows four now; **Floor by Face**.
10. **Site settings.** Massing & Site › Model Site › **Site Settings**: contour interval 250,
    passing through 0.
11. **The existing ground.** Save a text file of four rows `x y z`, in metres: `-10 -12 -1.26`,
    `50 -12 -1.26`, `50 30 0.24`, `-10 30 0.24`. In the Level 1 plan, with nothing selected,
    Massing & Site › Model Site › **Toposolid** and open the file: 60 × 42 m, rising 1.5 m to
    the north, below the slabs everywhere under the buildings.
12. **The building pad.** Select the toposolid, Model Site › **Graded Region**, *Copy every
    point*. **[gap]** The script then levels a pad 46 × 24 m, (-4000, -6000) to (42000, 18000), at
    -300 (the slabs' underside), with a ring of points 2 m outside it at the existing heights
    (`toposolid.set_points`); the window cannot add or move a toposolid's points.
13. **Cut and fill.** Select the graded region, Model Site › **Cut/Fill**: cut 13.85 m³, fill
    409.50 m³, net 395.65 m³ of fill.
14. **Path and lawn.** Model Lines on Level 1: a rectangle (10000, -12000) to (13000, -300) for
    the path, and (0, 22000) to (38000, 28000) for the lawn. Select the graded region and one
    loop, Model Site › **Sub-Divide**, height 50; again for the other.
15. **Site components.** **[gap]** The script makes two box types, *Shrub 1200* (planting, 1200
    × 1200 × 1500) and *Bench 1800* (furniture, 1800 × 600 × 450), with `mep_type.create`,
    which has no window command; Architecture › Component › **Import Object** brings in a mesh
    of a shrub or bench instead. Then Model Site › **Site Component**: shrubs at (3000, -3500),
    (7000, -3500), (17000, -3500) and (21000, -3500), benches at (26000, -2500) and (33000,
    -2500), on the pad.
16. **Property line.** Model Lines: a rectangle (-8000, -10000), (-8000, 28000), (48000,
    28000), (48000, -10000); select them, Massing & Site › Property Line › **Property Line**:
    2128 m². (With nothing selected, it takes a table of rows `38000 N 0°E`, `56000 N 90°E`,
    `38000 S 0°E`, `56000 S 90°W`, as the script does; **[gap]** the table starts at (0, 0), and
    the window has no field for its starting point, (-8000, -10000).)
17. **Labels.** Annotate › **Text** for the notes in the Level 1 plan.

Check: View › **Check Overlaps** finds none: masses are conceptual and left out, and the existing ground is hidden under its graded copy (kept for Cut/Fill).

## Gaps

- **Wall by Face location line.** `wall.create_by_face` takes `location_line`; the window
  always uses the exterior finish face. (Properties › Location Line keeps the walls' bodies
  where they are, so it does not move them out.)
- **Stretching a mass form.** `mass.set_form` (a form's start and end) has no grip or
  property.
- **Editing toposolid points.** `toposolid.set_points` (add, move, remove and lock points) has no
  window command, so a graded region cannot be graded in the window.
- **Box component types.** `mep_type.create` has no window command; site components come from
  Import Object.
- **Property line table origin.** The table dialog has no starting point (`origin`).
