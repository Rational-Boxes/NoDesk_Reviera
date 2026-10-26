# Energy house: building it by hand

How to build `energy_house.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet ([gaps](#gaps)). The script `energy_house.lua` is the reference.

Start with **File › New**, save it (**File › Save As**, so the weather file is kept beside it),
and open a Level 1 plan (View › Floor Plan).

1. **The site.** Sustainability › Site › **Location**: latitude 51.5, longitude 0, elevation 20,
   time zone 0, place *Synthetic Temperate*.
2. **Weather.** Sustainability › Site › **Weather**: import `reviera_synthetic.epw` from the
   samples folder (a synthetic year made by `make_weather.py`, not measured weather). It is
   copied into `energy_house.resources/weather`.
3. **The bungalow.** Architecture › **Wall**, 2700 high, clockwise round (0, 0), (0, 9000),
   (12000, 9000), (12000, 0). Partitions from (6000, 0) to (6000, 9000), then (0, 4500) to
   (6000, 4500) and (6000, 4500) to (12000, 4500), meeting at a T.
4. **Insulated walls.** Select an exterior wall, Properties › Type › *Duplicate Type…* *Exterior
   - Insulated Block 300*; Modify | Walls › **Edit Assembly**: *Lime Render* 15, *Wood Fibre
   Insulation* 160, *Concrete Block* 110 (Core), *Gypsum Plaster* 15. Give it to the four
   exterior walls.
5. **Windows.** Architecture › **Window**: one in the north wall of each northern room (x = 3000
   and 9000). Duplicate the type as *South Glazing 2400 x 1500* (Width 2400, Height 1500) and
   place two in the south wall of each southern room (x = 1700, 4300, 7700, 10300). A door in the
   west wall at y = 6800.
6. **Thermal values.** Sustainability › Energy › **Energy Model**, *Thermal Values…*: the
   exterior wall type U 0.18; *South Glazing* U 0.8, SHGC 0.5; *Window 1200 x 1200* U 1.1, SHGC
   0.6; the door type U 1.2.
7. **The slab.** Architecture › **Floor** on the walls' centrelines.
8. **Spaces.** Analyze › Spaces & Zones › **Place Spaces Automatically**; Limit Offset 2700.
   Manage › MEP Settings › **Building/Space Types**: add *Dwelling* (lighting 5 W/m², power 4
   W/m², 25 m² a person, 70 W sensible and 45 W latent a person, 8 L/s a person, 21 °C heating,
   25 °C cooling) and give it to the four spaces.
9. **Labels.** Annotate › **Text** for the two notes.

Then try:

- Sustainability › Energy › **Heating & Cooling Loads**: design loads by space.
- **Annual Energy** (heat pump, grid factor 0.2): about 51 kWh/m² a year, heating mostly in
  winter; **Energy Results** by month. With EnergyPlus installed, choose it as the *Engine*.
- Sustainability › Carbon › **Embodied Carbon**: the wall layers' materials.
- **File › Export gbXML…** and **Export HBJSON…**: four spaces, six windows and the door.

Check: View › **Check Overlaps** finds none.

## Gaps

None: every step has its UI.
