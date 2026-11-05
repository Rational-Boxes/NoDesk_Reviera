# An inclined elevator up a hillside: building it by hand

How to build `inclined_elevator.ifc` in the window. Each step names the UI path; **[gap]** marks a
step the window cannot do yet. The script `inclined_elevator.lua` is the reference: the same
commands with exact values.

Start with **File › New**. Add a level at 12.0 m (the upper terrace). Add a 10 × 8 m
**Architecture › Floor** from (−10, −4) on Level 1, and another from (30, −4) on the upper level.

1. **The inclined elevator.** **Architecture › Circulation › Inclined Elevator**:
   - bottom landing on Level 1 at `0, 0`, top landing on the upper level at `30, 0`, or leave
     them empty and click them in the two plans;
   - *Car* 5 × 5 ft (accessible), *Use* commercial, enclosed, *Drive* double drum on a slab,
     *Accessible route* ticked.
   - The track is 32.3 m long at 21.8°: two minutes a trip at 0.25 m/s. The car carries
     1,075 kg.
   - Its track is an alignment rising 12 m. A level landing 1.5 m deep is kept clear in front of
     each gate.
2. **Nothing to report.** No problems are found. **Analyze › Check Clearances** finds both
   landings clear.
3. **The wrong drive.** With the *winding drum on a slab* instead, which lifts at most 2,000 lb
   (907 kg), the check would report the accessible car as too heavy for it. Try it on a copy.

## Gaps

- **[gap]** Supports (posts, piers, piles), landing gates and the drive's slab are not modelled;
  the car stays parked at the bottom.
- **[gap]** A17.1's end clearances, runby and gate rules on an incline are not checked.
