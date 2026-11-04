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

## Gaps

- **[gap]** No escalator traffic or egress dialog yet.
- **[gap]** Platform clearing times are not worked out yet, and a route is reported, not drawn.
