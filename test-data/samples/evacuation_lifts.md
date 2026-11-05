# Evacuation lifts in a tower: building it by hand

How to build `evacuation_lifts.ifc` in the window. Each step names the UI path; **[gap]** marks
a step the window cannot do yet. The script `evacuation_lifts.lua` is the reference: the same
commands with exact values.

Start with **File › New**, the **Building Code** at *International Building Code (US)*. Add
levels at 3.5, 7.0, 10.5, 14.0 and 17.5 m, and a 32 × 16 m **Architecture › Floor** from (0, 0)
on each.

1. **The stairs.** On each floor but the top, **Architecture › Stair**, 1200 mm wide, up to the
   floor above: from `2, 4` to `7.5, 4` on Levels 1, 3 and 5, and from `2, 7` to `7.5, 7` on
   Levels 2 and 4. After each, **Modify | Stairs › Stair Openings**.
2. **The elevator.** **Architecture › Circulation › Elevator** from Level 1 to the top level, at
   `26, 9`, its landing toward the south.
3. **The way down by the stairs.** **Analyze › Route Analysis › Egress Route**:
   - from the top level at `28, 3` to Level 1 at `28, 3`, *Lifts* ticked;
   - the elevator is not marked for evacuation, so the route walks west and takes the five
     flights.
4. **An evacuation elevator.** Select the elevator's car, then **Architecture › Circulation ›
   Egress Role** and *Occupant evacuation elevator*.
   - Egress Route again with *Lifts* ticked: the route walks to the elevator's landing and rides
     it down.
   - The ride counts a 60 s wait for the car, 10 s of doors, and 17.5 m at the rated speed with
     the time lost speeding up and slowing down: 97 s in all, against 116 s by the stairs.
   - Without *Lifts*, the stairs again.
5. **Drawn in the plans.** **View › Create › Floor Plan** for every level, then Egress Route with
   *Lifts* and *Plans* ticked: a path of travel on the top floor to the lift, and one on the
   ground floor from it.
6. **For firefighters.** *Egress Role* › *Fire service access elevator*: routes leave it alone,
   and its IFC FireFightingLift property is set.

7. **Its lobbies.** On the top floor, three **Walls** carrying the hoistway's side walls on 3 m
   from its front wall, and one across, enclosing the landing. Put a **Door** in the middle of the
   wall across, then a **Room** inside them.
   - **Architecture › Circulation › Check Lobbies**, *Occupants a floor* empty: 512 m² at
     13.94 m² a person is 37 people.
   - The top lobby must hold a quarter of them, 10, at 0.28 m², and one wheelchair space: 3.7 m².
     At 7.5 m² it is large enough.
   - The nearest stair's end is 17.5 m away at the other end of the floor, so there's no direct
     access to one (IBC 3008.6.2).
   - The five lower floors have no lobby at all (IBC 3008.6).

## Gaps

- **[gap]** The wait for the car is typed in a script (`lift_wait`); the dialog uses 60 s. It is
  not worked out from the elevators' traffic or the number of people waiting.
- **[gap]** The lobby's 1-hour smoke barrier, standby power and the protected hoistway are not
  checked; *direct access* is a stair's end within 3 m of the lobby.
