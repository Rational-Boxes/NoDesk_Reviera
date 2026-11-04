# Elevators in an office core: building it by hand

How to build `elevators.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet, and the [gaps](#gaps) section lists them. The script `elevators.lua` is
the reference: the same commands with exact values.

Start with **File › New**. In **Manage › Model Properties** leave the **Building Code** at
*International Building Code (US)*. The vertical-transport rule packs follow it:
- `us_ibc_ada`: the ADA's car and door sizes, and the IBC 3002.4 stretcher from four storeys;
- `us_a17`: ASME A17.1's maximum car area for the load, advisory because only restatements were
  read;
- `practice_us`: generic, maker-style sizes for cars, hoistways, pits and overheads.

1. **Levels and floors.** Add Levels 2–5 at 4.0, 7.5, 11.0 and 14.5 m (**Architecture › Level**).
   On each level draw an **Architecture › Floor** over 24 × 14 m from (0, 0).
2. **Two passenger elevators.** Open the Level 1 plan. **Architecture › Circulation › Elevator**:
   1. Choose *Serves up to* Level 5 and leave the rated load empty.
   2. Leave the drive at *Traction, no machine room*, the speed at 1.0 m/s, the doors *As the car
      comes* and the rotation at 0.
   3. Type the entrance point `10, 7` (or leave it empty and click there).

   Five storeys need the stretcher, so the 3500 lb car comes: 2032 × 1651 mm for 21 persons, in
   a 2591 × 2108 mm hoistway with a 1524 mm pit and a 4115 mm overhead. The status bar gives
   these sizes. The shaft cuts all five floors, and the walls go from the pit floor to the
   overhead, with a sliding entrance on every level. Place the second elevator the same way at
   `13.041, 7`: one hoistway and two walls along, 50 mm clear.
3. **A hydraulic elevator to Level 2.** Elevator again, *Serves up to* Level 2, drive
   *Hydraulic*, speed 0.5, rotation −90, entrance point `4, 7`. Two storeys need no stretcher, so
   the ADA's side-door minimum gives the 2000 lb car (1727 × 1295 mm), with a 1219 mm pit and a
   3734 mm overhead. Its landing faces west.
4. **Check.** **Architecture › Circulation › Check Elevators** finds no problems. **View ›
   Coordination › Check Clearances** finds nothing in the hoistways. Switch a 3D view's virtual
   volumes to *Translucent* to see each hoistway (violet) from pit to overhead.

Try it: draw a wall across a hoistway on Level 3, and Check Clearances reports it in the
hoistway. Or make an elevator with a 1500 × 1200 mm car (`elevator.create` with `load`,
`car_width` and `car_depth`), and Check Elevators reports it against ADA 407.4.1 and IBC
3002.4.

## Gaps

- **[gap]** An elevator cannot be edited in place yet: delete it (`elevator.delete`, given the
  car or any wall) and make it again.
- **[gap]** The rule packs are chosen by the building code or with `vt.set_rule_packs`; there is
  no Model Properties page for them yet.
