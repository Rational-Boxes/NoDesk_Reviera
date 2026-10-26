# Analysis tools: building it by hand

How to build `analysis_tools.ifc` in the window. Each step names the UI path; **[gap]** marks a
step the window cannot do yet ([gaps](#gaps)). The script `analysis_tools.lua` is the reference:
the same commands with exact values.

Start with **File › New**. Set the code to *IBC* (Manage › Settings › **Model Properties**).

1. **Levels and views.** Elevation view, Structure › Datum › **Level**: *Canopy* at 3600. View ›
   Create › **Floor Plan** for Level 1 and Canopy, and **3D View**.
2. **The room.** Level 1 plan, Architecture › **Wall**, 4000 high, clockwise round (0, 0), (0,
   8000), (12000, 8000), (12000, 0). Architecture › **Door** in the east wall at y = 4000: the
   exit.
3. **Sections.** Structure › Framing › **Load Beam Section**: W12X26 from *AISC W*; with the
   Column tool active, Properties › Type › *Load Section…*: W8X31.
4. **The canopy.** Structure › **Column**: W8X31, base Level 1, top Canopy, at (12600, 1000),
   (16600, 1000), (16600, 7000) and (12600, 7000). Canopy plan, Structure › **Beam**: W12X26 round
   the four columns.
5. **Analytical model.** Analyze › Analytical › **Physical to Analytical**: a member on each
   column and beam, nodes where they meet.
6. **Roof panel and rooflight.** Canopy plan, Analyze › Panels › **Panel by Boundary**: sketch
   the bay's outline through the beams' end nodes; in Properties, thickness 100 and material
   *Concrete f'c 4000 psi*. Analyze › Panels › **Analytical Opening**: sketch the rooflight inside
   it, (13850, 3250) to (15350, 4750).
7. **Supports.** Level 1 plan, Analyze › Loads › **Point Support**, *Fixed*: click near each
   column's foot node. (**Column Base Supports** would pin them instead.)
8. **Load cases and loads.** Analyze › Loads › **Load Cases**, Load Cases tab: *D* (dead,
   self-weight), *Lr* (roof live), *W* (wind). Canopy plan, Loads › **Area Load**: D −1.0 kPa and
   Lr −1.0 kPa inside the panel. Loads › **Point Load**: W, Fx 5 kN, at the north-west column's
   top node (12600, 7000).
9. **The combination.** **Load Cases**, Combinations tab, **Add**: *U1*, formula `1.2*D + 1.6*Lr +
   0.5*W`.
10. **Route settings.** Analyze › Route Analysis › **Route Settings**: walking speed 1.2 m/s,
    clearance 300 (the zone, 200 to 2000 above the level, as it is).
11. **Path of travel.** Level 1 plan, Analyze › Route Analysis › **Path of Travel**: click
    (600, 7400) in the north-west corner, then (13600, 4000) under the canopy. The route keeps
    300 clear of the walls and goes out through the exit door; its length (about 13.5 m) and time
    (about 11 s) show at its end.
12. **Types.** Systems › Components › **Load MEP Types**: *Air Handling Unit*, *Supply Diffuser
    600*, *Distribution Board*, *Receptacle*.
13. **The air handler.** Systems › Components › **Mechanical Equipment**, *Air Handling Unit* at
    (2000, 1200), not rotated: its supply outlet faces east.
14. **Diffusers and their system.** Systems › Components › **Air Terminal**, *Supply Diffuser
    600*, offset 2700, flow 100 L/s, at (6000, 4000), (8000, 4000) and (10000, 4000). Select the
    three, Systems › Systems › **Create System** (*Supply Air*), then **Select Equipment**: the
    air handler.
15. **The main.** Systems › HVAC & Piping › **Duct**, round 400, *Supply Air*, middle elevation
    900: from the unit's outlet at (3500, 1200) to (5000, 1200); press **E**, 3500, and on to
    (10600, 1200).
16. **Branches.** **Duct**, round 250, elevation 3500: from the main's side at (6000, 1200) to
    (6000, 4000), **E** 2850 to drop onto the diffuser's neck; the same at x = 8000 and 10000. A
    tee joins each to the main. Select the main and **Cap Open Ends** for its east end.
17. **Sizing.** Select a duct of the system, Analyze › Sizing › **Duct/Pipe Sizing**: *Equal
    Friction* (1 Pa/m). The main becomes Ø315, then Ø250 past the first branch and Ø200 past the
    second; the branches Ø200. The main's capped stub past the last branch carries no flow and is
    left as it is (the report says so).
18. **Pressure drop.** Analyze › Pressure Drop › **Calculate Networks**; **Analytical
    Segments** lists each segment and fitting, the critical path to the east diffuser and about
    44 Pa at the unit.
19. **Duct legend.** Level 1 plan, Analyze › Color Fill › **Duct Legend**: click at (−4500, 7500),
    a new scheme *Duct Velocity*, Velocity by range, rows at 0, 3.5 and 4 m/s. The ducts are
    coloured by their velocity.
20. **The board and its circuits.** Systems › Electrical › **Electrical Equipment**, *Distribution
    Board* at (6000, 7750); **Panel Settings**: name it *LP1*. **Device**, *Receptacle*, offset
    300, at (1500, 7850) and (3500, 7850): **Create Circuit**, **Select Panel** LP1 (LP1-1); and
    at (8500, 7850) and (10500, 7850) for LP1-2.
21. **Panel schedule.** Select LP1, Analyze › Reports & Schedules › **Panel Schedule**.
22. **Labels.** Level 1 plan, Annotate › **Text** for the five notes.

Check: View › **Check Overlaps** finds none. Analyze › Checks › **Check Supports** and **Check
Consistency** report only that the roof panel has no physical element (allowed). Analyze ›
Structural Analysis › **Run Analysis**, then **Results Explorer**: U1's largest moment is in the
6 m east beam, about 20 kN·m, and the vertical reactions total about 84 kN. The rooflight is not
taken off the area loads yet: the members carry the panel's gross 24 m² (the analysis notes it).

## Gaps

None: every step has its UI.
