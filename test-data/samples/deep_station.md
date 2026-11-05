# Waiting on the way up from a deep station: building it by hand

How to build `deep_station.ifc` in the window. Each step names the UI path; **[gap]** marks a
step the window cannot do yet. The script `deep_station.lua` is the reference: the same commands
with exact values.

Start with **File › New**, the **Building Code** at *International Building Code (US)*. Add
levels at 6.0 m (the concourse) and 12.0 m (the street), and a 60 × 12 m **Architecture › Floor**
from (0, 0) on each.

1. **Up from the platform.** **Architecture › Stair**, 2000 mm wide, from `4, 3` to `13.5, 3`
   and from `4, 8` to `13.5, 8`, each up to the concourse, then **Modify | Stairs › Stair
   Openings**. Together they pass 222 people a minute (0.0555 a minute a millimetre).
2. **Up to the street.** One stair 1200 mm wide from the concourse, from `40, 3` to `49.5, 3`,
   with its opening: 67 people a minute.
3. **Clearing.** **Analyze › Route Analysis › Platform Clearing**:
   - the platform level, a load of 400, the most remote point `58, 6` on the platform, and the
     point of safety `56, 9` on the street;
   - the platform clears in 1.80 minutes, but the narrow stair takes 6.01 minutes for the same
     people;
   - they wait 4.21 minutes on the concourse, and the time to safety goes over 6 minutes.
4. **A second stair up.** Another 1200 mm stair from `40, 9` to `49.5, 9`, with its opening.
   Platform Clearing again: 3.00 minutes up to the street, 1.20 minutes waiting, and the time to
   safety within 6.

## Gaps

- **[gap]** Only stairs and escalators count as a later level's exits. Doors, corridors and the
  concourse's own occupants beyond *Already on the concourse* are not modelled.
