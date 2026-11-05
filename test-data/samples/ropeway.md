# A gondola up a hillside: building it by hand

How to build `ropeway.ifc` in the window. Each step names the UI path; **[gap]** marks a step the
window cannot do yet. The script `ropeway.lua` is the reference: the same commands with exact
values.

Start with **File › New**.

1. **The hillside.** **Massing & Site › Toposolid** from the points file in the script.
2. **The line.** **Massing & Site › Alignment**: `0, 0` and `850, 0`.
3. **The gondola.** Select the alignment, then **Architecture › Circulation › Ropeway** with the
   defaults:
   - towers 18.3 m high at most 152 m apart: six spans of 141.7 m;
   - the report finds the loaded cabins too close to the rising ground in the last two spans
     (RM2 asks 2.5 m on spans under 200 m), and below it in the fifth.
4. **Taller and closer.** Delete it, select the alignment again, and place a **Ropeway** with
   *Tower height* 22 and *Largest tower spacing* 120:
   - eight spans of 106 m, every one clear;
   - the cabins at most 27 m above the ground, which the report notes for the rescue concept;
   - 2,400 persons an hour each way: cabins of ten every 15 s at 6 m/s.

## Gaps

- **[gap]** One loaded sag per span; the empty, stopped and iced cases, the clearance envelope
  with swing, roads, buildings and other lines are not checked yet.
