# Managed project: building it by hand

How to build `managed_project.ifc` in the window. Each step names the UI path; **[gap]** marks a
step the window cannot do yet ([gaps](#gaps)). The script `managed_project.lua` is the reference.

Start with **File › New**, and open a Level 1 plan (View › Floor Plan).

1. **Model Properties.** Manage › Settings › **Model Properties**: Units *Metric (mm)*, Decimal
   symbol `.`, Digit grouping *1 234*, Code *International Building Code (US)*, Occupancy
   *Other*, Rule set *ADA 2010*. (Manage › Settings › **Accessibility Rules** sets the rule set
   on its own.)
2. **The building.** Architecture › **Wall**, 3000 high, clockwise round (0, 0), (0, 8000),
   (12000, 8000), (12000, 0); a partition from (8000, 0) to (8000, 8000). A door in the south
   wall 6.5 m from its start (Architecture › **Door**) and one in the middle of the partition; a
   window in the south wall at 9.5 m and three in the north wall at 2, 5.5 and 10.2 m
   (Architecture › **Window**). Set Properties › Placement › From Wall Start for the exact
   values.
3. **A shared parameter.** Manage › Settings › **Parameter Libraries**: **New…** makes a library
   file (*Practice Parameters*); **Definitions…** › **New…** adds *Acoustic Rating* (Text);
   **Add to Project…** binds it to Walls as an instance parameter. The project keeps a full copy
   with the library's name and version. (The window gives the definition a new GUID; the script
   uses a fixed one.) Select the four exterior walls and set Acoustic Rating *Rw 50 dB* in
   Properties.
4. **Dimensions.** Annotate › **Aligned Dimension**: the east wall's centreline, then the
   partition's, and click at y 6000 to place it; again from the west wall's centreline to the
   partition's.
5. **Global parameters.** Manage › Settings › **Global Parameters**, **New…** for each:
   - *StoreWidth*, Length, 3600, group *Layout*;
   - *OfficeWidth*, Length, Reporting, group *Layout*;
   - *OfficeArea*, Area, formula `OfficeWidth * 8 m`, group *Layout*;
   - *SillHeight*, Length, 1000, group *Openings*;
   - *PartitionAcoustic*, Text, `Rw 45 dB`, group *Acoustics*.
6. **Label the dimensions.** Select the east dimension, Manage › Settings › **Label with
   Global** (also Modify | Dimensions), *StoreWidth*: the partition moves to 3600 from the east
   wall, its door with it. Label the west dimension with *OfficeWidth*: it reads 8400, and
   OfficeArea 67.2 m².
7. **Associate globals.** Select the four windows, Manage › Settings › **Associate Global**,
   *Sill Height*, *SillHeight*: the sills rise to 1000. Select the partition, Associate Global,
   *Acoustic Rating*, *PartitionAcoustic*.
8. **Classification.** Write a CSV file with `code,parent_code,title` lines: `EF_25,,Wall and
   barrier elements`, `EF_25_10,EF_25,Walls`, `EF_25_30,EF_25,Doors and windows`. Manage ›
   Settings › **Classification Systems** › Load…, named *Uniclass 2015*. **[gap]** Its source
   (*NBS*) and location (`https://uniclass.thenbs.com`) cannot be given. Select the five walls,
   Manage › Settings › **Classify**, *Uniclass 2015*, *EF_25_10*; the doors and windows,
   *EF_25_30*.
9. **Keynotes.** Save the project, and beside it write `keynotes.txt`, a tab-separated key,
   text and parent key a line: `08` Openings; `08 11 13` Hollow metal doors and frames (parent
   `08`); `08 51 13` Aluminium windows (`08`); `09` Finishes; `09 21 16` Gypsum board assemblies
   (`09`). Manage › Settings › **Load Keynotes**. Select the front door, Edit Type, and set
   Keynote to `08 11 13`; the south-wall window's type to `08 51 13`. Annotate › Tag ›
   **Element Keynote**: click the front door, then just below it, right of its swing (a boxed
   08 11 13 with a leader); the same for the window. **User Keynote**: choose *09 21 16 — Gypsum
   board assemblies*, click the partition, then a point in the store; in Properties set Style
   *Number*. View › **Keynote Legend**, *Yes* to Filter by sheet; View › **Sheet** (*A3*, named
   *Plan and Keynotes*), Place View the Level 1 plan and Place Schedule the legend: it lists the
   three keys with their text.
10. **Saved selections.** Select the four exterior walls, Manage › Selection › **Save
    Selection** (*Exterior walls*); the doors and windows (*Doors and windows*). Manage ›
    Selection › **Sets as IfcGroups**: *Yes*.
11. **Location.** Manage › Project Location › **Location**: Place *Portland, Oregon*, Latitude
    45.52, Longitude −122.68, Elevation 15, Time zone −8, Daylight saving on; Angle to true
    north 12; Survey point (0, 0, 0) with Shared coordinates 525000, 5040800, 15; CRS
    *EPSG:32610*. **Report Coordinates** at a point reads its easting and northing.
12. **MEP settings.** Manage › MEP Settings › **Building/Space Types**: **New…** *Office*
    (lighting 8 W/m², power 10 W/m², 10 m² a person, 75 W sensible and 55 W latent a person,
    10 L/s a person of outdoor air, 20 °C heating and 24 °C cooling). Manage › MEP Settings ›
    **Pipe Segment**: Material *Copper*, Schedule *Type L*, Roughness 0.0015, Sizes `12.7 13.843
    15.875`, `19.05 19.939 22.225`, `25.4 26.035 28.575`.
13. **Labels.** Annotate › **Text** for the two notes.

Check: View › **Check Overlaps** finds none, and no door clearance is intruded. Global
Parameters lists StoreWidth 3600, OfficeWidth 8400 and OfficeArea 67.2 m²; changing StoreWidth
moves the partition, and OfficeWidth and OfficeArea follow. Saved, the selections are IfcGroups,
the walls, doors and windows carry IfcClassificationReferences, and the site has its latitude,
longitude and an IfcMapConversion to EPSG:32610.

## Gaps

- **Classification source and location:** Classification Systems › Load… asks only for the
  system's name, though `classification.load` takes its source and edition; a location (the
  publisher's URL) only `classification.create` sets, and no dialog edits a loaded system.
