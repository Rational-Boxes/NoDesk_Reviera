# A funicular up a hillside: building it by hand

How to build `funicular.ifc` in the window. Each step names the UI path; **[gap]** marks a step the
window cannot do yet. The script `funicular.lua` is the reference: the same commands with exact
values.

Start with **File › New IFC 4.3 Project**, so the funicular's line is saved as an `IfcAlignment`.

1. **The line.** **Massing & Site › Alignment**, its points of intersection one a row:
   ```
   0, 0
   150, 0, 300
   300, 60
   ```
   then its profile:
   ```
   0, 0
   150, 30, 100
   310, 100
   ```
   It bends left round a 300 m curve, and steepens from 20 % to 43.75 % over a 100 m vertical
   curve.
2. **The funicular.** Select the alignment, then **Architecture › Circulation › Funicular**:
   - *Track* two rails with an Abt passing loop half way; *Gauge* 1000 mm;
   - cars 12 m long for 60 persons, 5 m/s, a minute at each station, level floors;
   - *Walkway* ticked.
   - The status bar gives 327 m of track rising 100 m, steepest 44 %, 75 s a trip, and 1,594
     persons an hour each way.
3. **Its capacity.** **[gap]** `funicular.check` with a demand of 600 persons an hour finds
   nothing: two cars of 60 every 75 s trip and minute's stop carry 1,594 an hour.

## Gaps

- **[gap]** The stations (platforms, stepped or level), the haul rope and its pulleys, the
  walkway itself, tunnels and viaducts are not modelled. The track is one object, not separate
  rails and vehicles yet.
- **[gap]** Three-rail layouts and bottom tail ropes come later.
