# Fire protection: building it by hand

How to build `mep_fire.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet ([gaps](#gaps)). The script `mep_fire.lua` is the reference.

Start with **File › New**, and open a Level 1 plan (View › Floor Plan).

1. **The floor.** Architecture › Build › **Wall**, 3600 high, clockwise round (0, 0), (0, 8000),
   (16000, 8000), (16000, 0); a **Door** in the west wall at y = 7000.
2. **Fire-rated walls.** Draw **Wall** from (10000, 0) to (10000, 8000) and from (0, 6000) to
   (10000, 6000). Select one, and in Properties pick **Duplicate Type…** in the type list: *Fire
   Wall 200 EI 60*, and give both walls that type. Set its **Fire Rating** in Properties to *EI
   60* (a type parameter: Pset_WallCommon.FireRating).
3. **Fire doors.** **Door** in the corridor wall at x = 8500 and in the store wall at y = 7000;
   **Duplicate Type…** *Fire Door 900 x 2100 FD30S* for both, **Fire Rating** *FD30S*.
4. **Ceilings.** Architecture › Build › **Ceiling**, **Automatic Ceiling**: click in the office,
   Height Offset From Level 2700; click in the corridor, 2600. The store has none.
5. **Types.** Systems › Components › **Load MEP Types**: *Pendent Sprinkler*, *Upright
   Sprinkler*, *Sidewall Sprinkler*, *Smoke Detector*, *Manual Call Point*, *Distribution
   Board*.
6. **Head data.** **[gap]** On the sprinkler types: pendent K 80, 68 °C, quick response, bulb,
   12 m²; sidewall K 80, 57 °C, quick, bulb, 9 m²; upright K 115, 93 °C, standard, bulb, 9 m².
   The bulb colours follow (red, orange, green). **[gap]** The upright's inlet connector to
   *Fire Protection Dry*.
7. **The wet riser and main.** Systems › HVAC & Piping › **Pipe**, *Fire Protection Wet*: 40 mm
   at offset 3000 from (500, 1500) to (9500, 1500); 65 mm from (10600, 7300) at offset 300, **E**
   to 3000 (the riser), to (600, 7300); 50 mm from (9200, 7300) on the main to (9200, 1500) on the
   first line; 40 mm from (500, 4500) to (9200, 4500). Runs starting or ending on a pipe's side
   are teed in.
8. **Riser valves.** Systems › HVAC & Piping › **Pipe Accessory**: *Butterfly valve* and *Check
   valve* on the wet riser, low down.
9. **Pendent heads.** Systems › HVAC & Piping › **Sprinkler**, *Pendent Sprinkler*, no height:
   at x = 1250, 3750, 6250, 8750 on y = 1500 and 4500; each hangs from the office ceiling.
   Systems › Components › **Connect Into** each into the line above it.
10. **Sidewall heads.** **Sprinkler**, *Sidewall Sprinkler*, clicked just north of the corridor
    wall at x = 2500 and 7000: they go on its face, 2450 up. **Connect Into** the y = 4500 line:
    the run goes through the wall and up through the office ceiling.
11. **Wet system.** Select the ten heads, Systems › Systems › **Create System**, *Fire Protection
    Wet* (*Wet System*).
12. **The dry riser.** **Pipe**, *Fire Protection Dry*, at offset 2700: 40 mm lines (11600, 800)
    to (11600, 7200) and (14400, 800) to (14400, 7200); 50 mm (11600, 3500) to (14400, 3500); 65
    mm from (15400, 4000) at offset 300, **E** to 2700, to (14400, 4000). **Pipe Accessory**:
    *Gate valve* and *Check valve* on its riser.
13. **Upright heads.** **Sprinkler**, *Upright Sprinkler*, at (11600, 2000), (11600, 6000),
    (14400, 2000), (14400, 6000): 3000 up, as there is no ceiling. **Connect Into** the line
    below each. Select them, **Create System**, *Fire Protection Dry* (*Dry System*).
14. **Duct and fire damper.** Systems › HVAC & Piping › **Duct**, rectangular 400 × 250, offset
    3350, from (5000, 2500) to (5000, 7500). Systems › Insulation & Accessories › **Duct
    Accessory**, *Fire damper*, clicked on the duct at the corridor wall. View › Coordination ›
    **Check Overlaps**: pick the wall and damper, **Cut** the wall by the damper. The openings
    for the pipes and the duct are made by themselves (Systems › Check Systems › **Penetrations**
    lists them).
15. **Fire alarm devices.** (The starter smoke detector and call point have 24 V *Fire Alarm*
    connectors.) Systems › Electrical › **Device**: *Smoke Detector* off the walls
    at (2500, 3000) and (7500, 3000) at 2650, (3500, 7000) and (8000, 7000) at 2550, (13000, 5000)
    at 3300; *Manual Call Point* at 1400 on the west wall's face at (150, 6400) and the store
    wall's at (10150, 6200).
16. **The fire alarm circuit.** **Electrical Equipment**, *Distribution Board* at (1600, 7750);
    **Panel Settings**: name it *FACP*. Select the seven devices, **Create Circuit**, *Fire
    Alarm*; **Select Panel** FACP: it is FACP-1.
17. **Labels.** Annotate › **Text** for the six notes.

Check: View › **Check Overlaps** finds none. Systems › Systems lists the Wet System (ten heads)
and the Dry System (four).

## Gaps

- Sprinkler heads' data (K-factor, rating, response, activation, coverage) are set with
  `sprinkler_type.set`; the window has no sprinkler type properties dialog yet.
- A component type's connectors cannot be edited in the window (`connector.set`): the upright
  head's inlet classification (dry).
- There is no fire alarm control panel: a distribution board named FACP stands in. No sounders,
  beacons or heat detectors among the starter types.
- No alarm, dry pipe or pre-action valve, flow switch or fire department connection: an isolating
  and a check valve stand in on each riser.
- Penetrations open walls and ceilings for runs, not for inline accessories: a fire damper set in
  the wall's plane overlaps it until the wall is cut by it.
- The systems hold the heads; the pipes, fittings and valves are not members of the
  IfcDistributionSystem in IFC.
- No hydraulic calculation (MEP-PIPE-007, P2).
