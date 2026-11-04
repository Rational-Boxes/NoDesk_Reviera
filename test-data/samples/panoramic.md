# Panoramic lifts in a hotel atrium: building it by hand

How to build `panoramic.ifc` in the window. Each step names the UI path; the script
`panoramic.lua` is the reference: the same commands with exact values.

Start with **File › New** and leave the **Building Code** at *International Building Code (US)*
(**Manage › Model Properties**). Add Levels 2 to 4 at 4.5, 8.0 and 11.5 m. Draw a 30 × 20 m
**Architecture › Floor** from (0, 0) on every level.

Every elevator comes from **Architecture › Circulation › Elevator** in the Level 1 plan. Type the
middle of its entrance, or click it.

1. **The atrium's panoramic elevator.**
   - *Serves up to* Level 4, *Car glass* "Rear and sides", *Hoistway* "Glass at the sides and
     rear, on steel posts", controller on the left, at `8, 6`.
   - The car's panels take 10.76 mm laminated glass (5 + 5 + 0.76), weighed at 2.5 kg per m² per
     mm.
   - Its counterweight goes to the side, so the hoistway is 300 mm wider than the plain car's.
   - The sides and rear are curtain walls of 10.8 mm laminated glass on 50 × 110 mm steel posts,
     at most 1.3 m apart, with transoms at most 2.5 m apart. The front is a wall with the landing
     entrances.
2. **A rear-glazed car in a walled hoistway.** *Car glass* "Rear (panoramic)", *Hoistway*
   "Walls", at `18, 6`.
3. **A glass lift to the mezzanine.**
   - *Serves up to* Level 2, drive *Hydraulic*, speed 0.5, *Car glass* "Rear and sides",
     *Hoistway* "Glass at the sides and rear…", at `24, 12`.
   - A hydraulic car has no counterweight, so its hoistway keeps its size.
4. **Check.**
   - **Architecture › Circulation › Check Elevators** gives one note. IBC 2409: the atrium
     elevator's glass hoistway serves four levels, so it needs fire-rated glazing unless the
     authority accepts it unenclosed, as in an atrium.
   - **View › Coordination › Check Clearances** finds nothing in the hoistways, landings or
     machine spaces.

Try this: switch to the EN packs (`vt.set_rule_packs` with `en81`, `en81_70` and `practice_en`)
and place another glazed car. Its glass then follows EN 81-20 Table 9 for each panel's inscribed
circle. Its sizing notes (`elevator.size`, or the elevator's data) ask for a handrail 0.9–1.1 m
above the car floor, fixed independently of the glass.

## Gaps

- **[gap]** Only the sides and rear can be glass. A glazed front with glass landing doors, and
  partially enclosed (open) hoistways with their guard heights, are not built yet.
- **[gap]** There is no solar gain or ventilation estimate for exterior glass shafts, and no
  counterweight screen.
