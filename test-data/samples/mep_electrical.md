# Office electrical and lighting: building it by hand

How to build `mep_electrical.ifc` in the window. Each step names the UI path; **[gap]** marks a
step the window cannot do yet ([gaps](#gaps)). The script `mep_electrical.lua` is the reference.

Start with **File › New**, and open a Level 1 plan (View › Floor Plan).

1. **The office.** Architecture › **Wall**, 3300 high, clockwise round (0, 0), (0, 7000),
   (10000, 7000), (10000, 0); a door in the west wall, three windows in the south wall.
2. **The space.** Analyze › Spaces & Zones › **Place Spaces Automatically**; Limit Offset 3300.
3. **Types.** Systems › Components › **Load MEP Types**: *Switchboard*, *Distribution Board*,
   *Ceiling Luminaire*, *Receptacle*, *Light Switch*, *Smoke Detector*.
4. **Boards.** Systems › Electrical › **Electrical Equipment**: the switchboard at (8400, 6500),
   the distribution board at (6600, 6750). **Panel Settings**: name them *MDB* and *LP1*.
   Select LP1, **Select Supply**, MDB: the feeder.
5. **Lighting data.** Select the luminaire type, Systems › Electrical › **Lighting Data**: 5000
   lm, a CU table of 0.6 at RCR 0 and 10 (reflectances 80/50/20).
6. **Luminaires.** Systems › Electrical › **Lighting Fixture**, offset 3000, on a 3 × 3 grid: x
   = 1800, 5000, 8200; y = 1400, 3500, 5600. A **Device**, *Light Switch*, offset 1100, by the
   door at (150, 4600).
7. **The lighting circuit.** Select the nine luminaires, **Create Circuit**, then **Select
   Panel** LP1: it is LP1-1.
8. **Receptacles.** **Device**, *Receptacle*, offset 300, on the wall faces at (150, 1500), (2500,
   150), (4500, 150) — circuit LP1-2 — and (6500, 150), (9850, 2000), (9850, 4500) — LP1-3.
9. **Smoke detector.** **Device**, *Smoke Detector*, offset 3200, at (3400, 2400).
10. **Labels.** Annotate › **Text** for the two notes.

Check: Analyze › Spaces & Zones › **Illumination** gives the space about 324 lx from nine
luminaires. LP1's panel schedule lists LP1-1 to LP1-3.

## Gaps

None: every step has its UI.
