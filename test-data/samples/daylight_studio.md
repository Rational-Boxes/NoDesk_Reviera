# Daylight studios: building it by hand

How to build `daylight_studio.ifc` in the window. Each step names the UI path; **[gap]** marks a
step the window cannot do yet ([gaps](#gaps)). The script `daylight_studio.lua` is the reference.

Start with **File › New**, save it (**File › Save As**), and open a Level 1 plan.

1. **The site and weather.** Sustainability › Site › **Location** (51.5, 0, elevation 20, time
   zone 0) and **Weather**: `reviera_synthetic.epw` from the samples folder.
2. **A roof level.** Architecture › Datum › **Level** at 3000, named *Roof*.
3. **The studios.** Architecture › **Wall**, 3000 high, clockwise round (0, 0), (0, 6000),
   (12000, 6000), (12000, 0); a party wall from (6000, 0) to (6000, 6000), 2700 high.
4. **Glazing.** Architecture › **Window** in the south wall; duplicate its type as *Studio
   Glazing 2400 x 1500* (Width 2400, Height 1500), give it to that window, and place three more:
   two in each studio (x = 1700, 4300, 7700, 10300). Doors in the north wall at x = 3000 and
   9000.
5. **The roof slab.** On the Roof plan, Architecture › **Floor** inside the walls' faces, (100,
   100) to (11900, 5900).
6. **The neighbouring block.** Architecture › **Wall**, 8000 high, from (13500, −5000) to (6500,
   −5000).
7. **Spaces.** Analyze › Spaces & Zones › **Place Spaces Automatically**; Limit Offset 2700.
8. **Labels.** Annotate › **Text** for the three notes.

Then try:

- Sustainability › Daylight & Solar › **Daylight Factor**, then **Daylight Results**: about 3.9%
  on average in the west studio and 2.5% in the east one, where the block hides the low sky; *Show
  on Model* colours each sensor in the 3D view.
- **Solar Study** for the roof's sun and PV potential, and **PVGIS Check** to compare (it sends
  the site's coordinates to PVGIS).

Check: View › **Check Overlaps** finds none.

## Gaps

- The glazing's visible transmittance is the study's default (0.7): a window type's
  `Pset_DoorWindowGlazingType.VisibleLightTransmittance` cannot yet be set from the window or by
  `param.set` on the type.
