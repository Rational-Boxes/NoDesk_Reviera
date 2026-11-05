# A drive set out by chainage: building it by hand

How to build `alignment_drive.ifc` in the window. Each step names the UI path; **[gap]** marks a
step the window cannot do yet. The script `alignment_drive.lua` is the reference: the same
commands with exact values.

Start with **File › New IFC 4.3 Project**: in an IFC 4.3 project the drive is written as an
`IfcAlignment`, with its horizontal and vertical layouts.

1. **The alignment.** **Massing & Site › Alignment** with nothing selected. Type its points of
   intersection, one a row, with the curve radius at the inner corners:
   ```
   0, 0
   60, 0, 25
   110, 50, 40
   170, 50
   ```
   - It turns left round a 25 m curve, then right round a 40 m one.
   - Or draw the three legs as **Model Lines** and select them first: the window joins them end to
     end and asks for the radii.
2. **Its profile.** Next, the vertical profile, one point a row (chainage, elevation, vertical
   curve length):
   ```
   0, 0
   120, 7.2, 80
   175, 8.3
   ```
   - It climbs at 6 %, then eases to 2 % over an 80 m crest curve.
   - **[gap]** The window's chainage starts at 0; the script starts it at 1+000
     (`start_station`).
3. **In the plan.** The drive is drawn as a dash-dot centreline in blue, with a tick across it
   every 20 m of chainage and a longer one every 100 m.
4. **Reading it.** **[gap]** Chainage and offset are read in a script or by the agent:
   - `alignment.point_at` at 1+040 is (40, 0), 2.4 m up at 6 %. The first curve starts 10.36 m
     before its corner, at 1+049.64;
   - a lamp post 3 m to the right of 1+100 comes back from `alignment.station_of` as chainage
     1+100, offset −3.

## Gaps

- **[gap]** No chainage labels in the plan yet, and no grips to move the points.
- **[gap]** Spirals, cant and cross sections come later, and so do the `IfcAlignment`'s geometry
  and its start chainage as a referent.
