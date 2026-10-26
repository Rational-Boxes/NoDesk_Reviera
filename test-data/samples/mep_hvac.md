# Office air system: building it by hand

How to build `mep_hvac.ifc` in the window. Each step names the UI path; **[gap]** marks a step the
window cannot do yet ([gaps](#gaps)). The script `mep_hvac.lua` is the reference.

Start with **File › New**, and open a Level 1 plan (View › Floor Plan).

1. **The office.** Architecture › **Wall**, 3400 high, clockwise round (0, 0), (0, 9000),
   (14000, 9000), (14000, 0). Three windows in each long wall (Architecture › **Window**) and a
   door in the west wall (Architecture › **Door**).
2. **Two spaces.** Analyze › Spaces & Zones › **Space Separator** from (7000, 0) to (7000, 9000);
   then **Place Spaces Automatically**. Select both spaces and set Limit Offset 3400 in
   Properties.
3. **Office loads.** Manage › MEP Settings › **Building/Space Types**: add *Office* (lighting 8
   W/m², power 10 W/m², 10 m² a person, 75 W sensible and 55 W latent a person, 10 L/s a person
   of outdoor air, 20 °C heating and 24 °C cooling) and give it to both spaces.
4. **Rooms on the same line.** Architecture › Room & Area › **Room Separator** on the same line,
   then **Room** in each half.
5. **Types.** Systems › Components › **Load MEP Types**: *Air Handling Unit*, *Supply Diffuser
   600*, *Return Grille 600*.
6. **The air handler.** Systems › Components › **Mechanical Equipment**, *Air Handling Unit* at
   (5500, 1800), rotated 90°, so its supply outlet faces north.
7. **The supply main.** Systems › HVAC & Piping › **Duct**, round 400, offset 3100, from (1200,
   5500) to (12800, 5500).
8. **Connect the unit.** Select the unit, Systems › Components › **Connect Into**, its Supply
   connector, then click the main: out, up and into it, a tee where it lands.
9. **Diffusers.** Systems › Components › **Air Terminal**, *Supply Diffuser 600*, offset 2500,
   flow 150 L/s, at (2500, 3000), (4000, 7500), (9500, 3000) and (11500, 7500); **Connect Into**
   the main for each. They are staggered along the main so the tees do not meet.
10. **Attenuator and return.** Systems › Insulation & Accessories › **Duct Accessory**, *Sound
    attenuator* 600 long on the main at (7600, 5500); a *Return Grille 600* at (7000, 8000),
    offset 2500, 400 L/s.
11. **The system.** Select the four diffusers, Systems › Systems › **Create System** (*Supply
    Air*).
12. **System-zones.** Analyze › Spaces & Zones › **System-Zone**: sketch the west half, then the
    east half; in Analyze › Spaces & Zones › **System-Zones**, *Edit…* to name them *West* and
    *East* and *Assign…* the air handler to each.
13. **Labels.** Annotate › **Text** for the two notes.

Check: View › **Check Overlaps** finds none. Each room's Supply Airflow (a room schedule's field)
reads 300 L/s. Sustainability › Energy › **Heating & Cooling Loads** runs on the two spaces.

## Gaps

None: every step has its UI.
