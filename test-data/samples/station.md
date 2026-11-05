# Escalators in a transit station: building it by hand

How to build `station.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet. The script `station.lua` is the reference: the same commands with
exact values.

Start with **File › New**, the **Building Code** at *International Building Code (US)*. Add a
level at 7.0 m (the concourse) above the platform, and a 70 × 24 m **Architecture › Floor** from
(0, 0) on each.

1. **The pair.** In the platform's plan, **Architecture › Circulation › Escalator**:
   - *Up to* the concourse, *Setting* "A station", at `20, 10`.
   - A station's escalator is heavy duty and runs at 0.65 m/s. A 6 m *station run-off* zone is
     kept beyond each end, besides the code's landing zone. Check Clearances reports anything
     in it.
   - The second at `20, 11.64`: a transit truss is 1640 mm wide (the step and 640 mm), so the
     pair stands a truss width apart.
2. **Units for the peak.** **[gap]** The window has no escalator traffic dialog. Use
   `escalator.traffic` from a script or the agent: 9,000 people an hour up needs three units,
   one of them for the unit assumed out.
3. **Egress.** **[gap]** Also `escalator.traffic`, with `egress`:
   - under NFPA 130 the escalators running the way out count, at 120 people a minute per metre
     of tread with the worst unit out, up to half the platform's required capacity;
   - under the IBC they never count.

4. **A stair.** **Architecture › Stair**, 1500 mm wide, from `4, 19` to `14.64, 19` up to the
   concourse, then **Modify | Stairs › Stair Openings**.
5. **The way out.** **Analyze › Route Analysis › Egress Route**:
   - from the platform at `64, 4` to the concourse at `64, 20`;
   - *Escalators* "Not counted (IBC)": the route walks to the west end, climbs the stair and
     comes back east;
   - "Counted (NFPA 130)": it takes the up escalator, and is quicker.
6. **Drawn in the plans.** **View › Create › Floor Plan** for both levels, then Egress Route again with
   *Plans* ticked: each leg becomes a path of travel in its level's floor plan, showing its
   length and time, and **Paths of Travel** lists them.
7. **Clearing the platform.** **Analyze › Route Analysis › Platform Clearing**:
   - the platform, a load of 500, the most remote point `64, 4`, and the point of safety on the
     concourse at `64, 20`; *Escalators* "Counted (NFPA 130)";
   - the stair gives 83 people a minute (1500 mm at 0.0555); of the escalators one is out and the
     other counts up to the stair's share, 83 more;
   - the platform clears in 3.00 minutes (at most 4), and the far end reaches the exit in 4.22
     (2.43 walking, 1.79 waiting at the escalator; at most 6);
   - "Not counted": the stair alone takes 6.0 minutes and fails.

## Gaps

- **[gap]** No escalator traffic or egress dialog yet.
- **[gap]** The occupant load is typed, not worked out from trains and headways; waits beyond the platform (concourse, fare gates) are not added.
