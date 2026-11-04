# Escalators and moving walks in a department store: building it by hand

How to build `escalators.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet, and the [gaps](#gaps) section lists them. The script `escalators.lua`
is the reference: the same commands with exact values.

Start with **File › New** and leave the **Building Code** at *International Building Code (US)*
(**Manage › Model Properties**). The vertical-transport rule packs follow it: A17.1's escalator
rules (at most 30° and 0.5 m/s, 2130 mm headroom, a safety zone at each end, 100 mm clear beside
the handrails) and the makers' generic sizes. Add Levels 2 and 3 at 5.0 and 10.0 m, and a
40 × 24 m **Architecture › Floor** from (0, 0) on every level.

Every escalator and walk comes from **Architecture › Circulation › Escalator** in the plan of
its lower level. Type the point of the middle of its lower end, or click it. At rotation 0 it
rises toward the right.

1. **An up-and-down pair.** In the Level 1 plan: *Escalator*, up to Level 2, 1000 mm, speed 0.5,
   at `4, 4`. It comes out 30°, with 2 flat steps, 13.36 m long and good for about 6000 persons an
   hour. It cuts a pit in Level 1 and an opening in Level 2 that runs from its upper pit down
   the incline as far as its headroom reaches the slab. The second goes at `4, 5.744`, leaving
   200 mm between the handrails' outer edges.
2. **A single escalator above.** In the Level 2 plan, up to Level 3, at `4, 10`.
3. **An inclined walk.** In the Level 1 plan: *Moving walk*, up to Level 2, at `4, 16`. It comes
   out at 12°.
4. **A horizontal walk.** In the Level 3 plan: *Moving walk*, *Up to* "None", 1400 mm, walk
   length 30 m, *On the floor*, at `5, 20`.
5. **Check.** **Architecture › Circulation › Check Escalators** finds no problems, and **View ›
   Coordination › Check Clearances** finds nothing in their headroom, side clearance or landing
   zones. Set a 3D view's virtual volumes to *Translucent* to see them.

Try it: place the second escalator of the pair 50 mm from the first, and Check Clearances
reports it inside the first one's 100 mm side clearance. Or make a 35° escalator: under the IBC
Check Escalators notes it as steeper than 30°; under the EN packs (`vt.set_rule_packs`) it is
allowed up to 6 m of rise at 0.5 m/s.

## Gaps

- **[gap]** Floor openings keep the size they were made with: changing the rule packs to a
  greater headroom leaves them short (Check Clearances says so). Delete and make the escalator
  again.
- **[gap]** No wellway protection yet (draft curtains, shutters), no intersection guards, no
  deck barricades.
