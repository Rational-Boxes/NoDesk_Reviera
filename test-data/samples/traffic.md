# Elevator traffic in an office tower: building it by hand

How to build `traffic.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet. The script `traffic.lua` is the reference: the same commands with
exact values.

Start with **File › New** and leave the **Building Code** at *International Building Code (US)*.
Add Levels 2 to 11, 4.0 m apart. Draw a 36 × 24 m **Architecture › Floor** from (0, 0) on every
level.

1. **Bank A.** In the Level 1 plan, place four elevators with **Architecture › Circulation ›
   Elevator**: *Serves up to* Level 11, speed empty (1.0 m/s), controller *None*, at `6, 10`,
   `9.2, 10`, `12.4, 10` and `15.6, 10`. Eleven storeys need the stretcher, so each is the
   3500 lb car.
2. **Bank B.** Six of the same cars, drive *Traction, machine room*, speed 2.5, rotation 180, at `6, 18` and every 3.2 m
   along to `22, 18`.
3. **Escalators.** **Architecture › Circulation › Escalator**, up to Level 2, at `22, 3` and
   `22, 4.544`.
4. **Traffic.** **Architecture › Circulation › Elevator Traffic**:
   - *Use* "Office", population empty. Each bank is a group: cars serving the same levels at
     the same speed with the same car.
   - Both banks serve the same floors, so each needs 120 % of the 12 % target, for migration.
   - The population is estimated from the floors at 10 m² a person: about 830 people.
   - The report gives each group's round trip, interval, handling and waiting time against the
     targets. Bank A misses them at 1.0 m/s: an interval of about 47 s.
   - The script checks bank B on its own for a given 1,200 people: an interval of about 24 s
     and about 12.7 %, which meets them.
5. **Escalator capacity.** **[gap]** The window has no escalator traffic dialog yet. Use
   `escalator.traffic` from a script or the agent, giving the units up and down and each way's
   demand.

## Gaps

- **[gap]** The escalator traffic check has no dialog.
- **[gap]** Lunchtime and two-way traffic, and destination dispatch, need a simulation, which
  is not built in. The up-peak is a planning check.
