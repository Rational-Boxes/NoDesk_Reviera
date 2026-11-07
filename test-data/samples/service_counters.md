# Service counters: building it by hand

How to build `service_counters.ifc` in the window. Each step names the UI path; **[gap]** marks a
step the window cannot do yet, and the [gaps](#gaps) section lists them. The script
`service_counters.lua` is the reference: the same commands with exact values.

Start with **File › New**. In **Manage › Model Properties**:
- set the **Building code** to the *IBC* and the **Accessibility rule set** to *2010 ADA Standards*;
- under **Interiors rules**, also tick *Interior planning practice*.

1. **The walls.** Draw four walls, 200 mm thick and 3.0 m high, round (0, 0)–(7.8, 5.2) on their
   centre lines. The inside is 7.6 × 5.0 m.
2. **The transaction counter.** In a plan, choose **Architecture › Build › Counter**, pick
   *Transaction counter, 42 in* and *Parallel approach at the end*, and click (2.0, 0.1) then
   (6.0, 0.1) on the south wall's face. The counter is a casework body on a 4 in toe kick under a
   1½ in top, 25 in deep. Its last 36 in is lowered to 36 in, ADA §904.4.1's longest and
   highest. It stops 1.7 m short of the east wall, so the 48 in clear floor space centred on the
   portion has room.
3. **The reception desk.** Choose **Counter** again with *Service counter, 36 in* and *Forward
   approach at the start*, and click (0.1, 2.0) then (0.1, 4.4) on the west wall. Its first 30 in
   has no body below: §306's knee clearance, 27 in high and 17 in deep, with a 30 × 48 in space
   in front (§904.4.2).
4. **A writing surface.** Choose *Work surface, 29 in* and click (4.0, 5.1) to (6.0, 5.1) on the
   north wall. It is open below.
5. **Check.** **View › Coordination › Check Rules** finds nothing. Raise the lowered portion
   (select its top and set its offset higher), or stand a chair under the reception's portion,
   and Check Rules reports §904.4 or §306.
6. **The room and the schedule.** Place an **Architecture › Room** named *Lobby*, and choose
   **View › Schedules › New Schedule** › *FF&E*: the bodies and tops are listed by type.

## Gaps

- **[gap]** The body under a forward-approach portion is left out, and work surfaces float; legs,
  brackets and modesty panels are not modelled.
- **[gap]** One accessible portion per counter. Bars' lowered sections for dining (§902) and
  check-out aisles (§904.3) are not made yet.
- **[gap]** Closets (shelves, rods and doors with the §308 reach check) come next.
