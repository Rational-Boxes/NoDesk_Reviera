# Washroom plumbing and sprinklers: building it by hand

How to build `mep_plumbing.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet ([gaps](#gaps)). The script `mep_plumbing.lua` is the reference.

Start with **File › New**, and open a Level 1 plan (View › Floor Plan).

1. **The washroom.** Architecture › **Wall**, 3300 high, clockwise round (0, 0), (0, 4500),
   (7000, 4500), (7000, 0); a door in the south wall.
2. **Types.** Systems › Components › **Load MEP Types**: *Lavatory*, *Water Closet*, *Urinal*,
   *Floor Drain*, *Water Heater*, *Pendent Sprinkler*.
3. **Fixtures.** Systems › HVAC & Piping › **Plumbing Fixture**: lavatories at x = 800 and 1700,
   a urinal at 2700, WCs at 4000 and 5200, each clicked just inside the north wall (y = 4200):
   they go on the wall's face. A floor drain at (2200, 2200).
4. **The water heater.** Systems › Components › **Mechanical Equipment**, *Water Heater* at (600,
   600).
5. **The sanitary main.** Systems › HVAC & Piping › **Pipe**, 100 mm, system *Sanitary*, slope
   1%, offset −600, from (600, 3000) to (6400, 3000): it falls 58 mm to the east.
6. **Sanitary system.** Select the fixtures and the drain, Systems › Systems › **Create System**,
   *Sanitary*.
7. **Cold water.** **Pipe**, 28 mm, *Domestic Cold Water*, offset 2600, from (300, 4000) to (6600,
   4000); Systems › HVAC & Piping › **Pipe Accessory**, *Gate valve*, on it. Select the
   fixtures, **Create System**, *Domestic Cold Water* (*Cold Water*).
8. **Sprinklers.** **Pipe**, 32 mm, *Fire Protection Wet*, offset 3000, from (600, 2000) to
   (6400, 2000). Systems › HVAC & Piping › **Sprinkler**, *Pendent Sprinkler*, offset 2600, at
   (1500, 1600), (3500, 1600) and (5500, 1600); **Connect Into** the branch for each (tees and
   elbows are made).
9. **Labels.** Annotate › **Text** for the two notes.

Check: View › **Check Overlaps** finds none.

## Gaps

- Connect Into joins only into a level pipe, so the fixtures are not piped into the sloped
  sanitary main; they are in its system logically.
