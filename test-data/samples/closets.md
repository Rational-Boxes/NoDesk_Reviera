# Closets: building it by hand

How to build `closets.ifc` in the window. Each step names the UI path; **[gap]** marks a step the
window cannot do yet, and the [gaps](#gaps) section lists them. The script `closets.lua` is the
reference: the same commands with exact values.

Start with **File › New**. In **Manage › Model Properties**:
- set the **Building code** to the *IBC* and the **Accessibility rule set** to *2010 ADA Standards*;
- under **Interiors rules**, also tick *Interior planning practice*.

1. **The walls.** Draw a wall, 200 mm thick and 2.7 m high, from (0, 0) to (4.2, 0). Its face is
   at y = 0.1. Add partitions at x = 0, 1.4, 2.8 and 4.2, from y = 0.1 to 0.71 (24 in deep): three
   closets, each 1.2 m between the partitions' faces.
2. **A single rod.** In a plan, choose **Architecture › Build › Closet**, pick *Single rod at
   66 in, shelf at 68 in*, and click the first closet's back wall face from (0.1, 0.1) to
   (1.3, 0.1). The rod is centred 12 in out from the wall; the shelf is 12 in deep on the wall
   (practice heights, S1489).
3. **Double hang.** In the second closet, (1.5, 0.1) to (2.7, 0.1), choose *Double hang at 40 and
   80 in, shelf at 84 in*.
4. **Accessible.** In the third, (2.9, 0.1) to (4.1, 0.1), choose *Accessible: rod at 42 in,
   shelf at 46 in*. Reaching sideways over the closet's 24 in depth, the 2010 ADA Standards allow
   46 in at most (§308.3.2); a 66 in rod would fail.
5. **Check.** **View › Coordination › Check Rules** finds nothing. Select the accessible shelf
   and raise its offset by 2 in: Check Rules reports §308.3.2.

## Gaps

- **[gap]** Closet doors, their swing and the 30 × 48 in clear floor space in front of an
  accessible closet are not made by the tool; place the door and check its clearances.
- **[gap]** Walk-in closets with rods on three walls, drawers, shoe shelves and closet lighting
  (NEC 410.16 storage zone) come later.
