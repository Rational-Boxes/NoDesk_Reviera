# Views and inspection: building it by hand

How to build `views_inspection.ifc` in the window. Each step names the UI path; **[gap]** marks a
step the window cannot do yet ([gaps](#gaps)). The script `views_inspection.lua` is the
reference.

Start with **File › New**. The model has Level 1 at 0.

1. **Second level.** Architecture › Datum › **Level**: add a level at 3000 (its floor plan comes
   with it).
2. **Walls.** Level 1 plan, Architecture › **Wall** (3000 high): click clockwise round (0, 0),
   (0, 7000), (10000, 7000), (10000, 0). A partition from (5000, 0) to (5000, 7000), Unconnected
   Height 2700 in Properties (it stops under the floor above). On the Level 2 plan the same
   rectangle and partition, 3000 high.
3. **Doors and windows.** Architecture › **Door**: the south wall at 7.5 m from its start, each
   partition at 3.5 m. Architecture › **Window**: south wall at 2.5 and 5.0 m on Level 1, 2.5 and
   7.5 m on Level 2; north walls at 2.5 and 7.5 m on both; east and west walls at 3.5 m on both.
   Set exact positions with Properties › Placement › From Wall Start.
4. **Floors, rooms, ceilings.** Architecture › **Floor** on Level 1 and Level 2: a rectangle from
   (100, 100) to (9900, 6900). Architecture › **Room** in each half: *Living* and *Kitchen* on
   Level 1, *Bedroom 1* and *Bedroom 2* on Level 2. Architecture › **Ceiling**: click in each
   room.
5. **Roof.** Level 2 plan, Architecture › **Roof** on the upper walls' exterior faces (Pick
   Walls), Base Offset 3000, Shape *Hip*. Modify | Roofs › **Eaves**: Overhang 600, Fascia
   *Yes*.
6. **Plans and ceiling plans.** View › **Floor Plan**: Level 1 (Level 2 has its plan). View ›
   **Ceiling Plan**: tick both levels.
7. **Sections.** Level 1 plan, View › **Section**: click (-2000, 3000) and (12000, 3000), looking
   south. Again from (7500, 9000) to (7500, -2000), looking west; Properties › Far Clip Offset
   10000. In the Project Browser right-click each, **Rename**: *Long Section*, *Cross Section*.
8. **Elevations.** View › **Building Elevations**: North, South, East and West, 5 m outside the
   model.
9. **Scope box.** Level 1 plan, View › **Scope Box**: Name *East Half*, two corners (4600, -1000)
   and (11000, 8000). Select it; Properties › Height 9000.
10. **A plan for the scope box.** Project Browser, right-click Floor Plans › Level 1 ›
    **Duplicate**, then **Rename** it *Level 1 - East Half*. Assign it the scope box **[gap]**,
    so that its crop follows the box **[gap]**.
11. **Default 3D view and Home.** View › **Default 3D View**; click the Orientation Cube's
    south-east top corner, then View › **Set as Home** (Alt+Home returns to it).
12. **Orthographic.** View › **New 3D View**, renamed *Orthographic* in the Project Browser;
    south-east corner on the cube, then View › **Perspective / Isometric** (PI).
13. **Section box.** A new 3D view *Section Box - Kitchen*, south-east corner. View › **Section
    Box** (SX) and drag its orange grips to (4900, -500, -400)–(10500, 7500, 2000): the east room
    on Level 1, cut at 2 m through its windows. The grips give the extents only roughly; they
    cannot be typed **[gap]**.
14. **Selection Box.** A new 3D view *Selection Box - Level 2*: select the five Level 2 walls
    (Ctrl+click), View › **Selection Box** (BX). The box fits round them with 300 mm to spare.
15. **Horizontal slice.** A new 3D view *Horizontal Slice*: View › **Horizontal Slice**, click a
    Level 1 window halfway up (1500): the model above is cut away, a plan in 3D. Drag the
    slice's grip to 1500 exactly; its height cannot be typed **[gap]**.
16. **Vertical slice.** A new 3D view *Vertical Slice*, the cube's Front face: View › **Vertical
    Slice**, click the south wall, then drag the slice's grip north to y = 3500 **[gap]** (as
    in step 15): the south half is cut away, a section in 3D.
17. **Virtual volumes.** A new 3D view *Virtual Volumes*: View › **Virtual Volumes** once (hidden
    to translucent): the doors' swings show.
18. **Temporary hide and isolate.** Four more 3D views, each with one state:
    - *Hide Roof*: select the roof, View › **Hide Category** (HC).
    - *Isolate South Wall* (Front face): select the Level 1 south wall, its door and windows
      (Ctrl+click), View › **Isolate, Ghost Others** (HG).
    - *Translucent Walls*: select every wall, View › **Translucent** (HT).
    - *Isolate Partition*: select the Level 1 partition, View › **Isolate Element** (HI).

    These are temporary: they are not saved with the views **[gap]**.
19. **BCF viewpoint.** Open *Section Box - Kitchen*, View › Coordination › **Export
    Viewpoint…** and save `kitchen.bcfv`: its camera, the box's six faces as clipping planes and
    any hidden or isolated elements. Open the *Orthographic* view and **Show Viewpoint…** the same
    file: it takes the kitchen's camera and box (Undo puts it back). The views sample is not
    changed by this; it is a check that the file round-trips.
20. **Cropped plan.** Project Browser › right-click *Level 1* › Duplicate View, rename it *Level 1 -
    Cropped*. In Properties › Extents tick **Crop View** and **Crop Region Visible**: the crop
    region fits the model. Drag its edges to (−1000, −1000)–(5000, 8000), or tick **Size Crop…**
    and type 6000 × 9000 (the script sets the region with `view.set_crop`). Tick **Annotation Crop**.

Check: View › **Check Overlaps** finds none. The Project Browser lists 3 floor plans, 2 ceiling
plans, 2 sections, 4 elevations and 11 3D views; reopened, each 3D view keeps its camera, Home,
section box, slices and virtual volumes.

## Gaps

- **Scope boxes on views.** `view.assign_scope_box` (step 10) has no UI: no Properties row on a
  view chooses its scope box (the assigned plan is cropped to its box, and grids and levels follow
  it).
- **Exact section box and slice positions.** The section box and slice planes are placed by
  grips and clicks only; `view.set_section_box` and `view.set_slice_planes` take exact values
  (steps 13, 15, 16), the window has no typed extents or heights.
- **Saved hide and isolate.** Temporary hide, isolate, ghost and translucency (step 18) are kept
  per view for the session only; nothing saves them with a view (there are no view-level
  visibility settings yet). The script applies them, but the saved sample shows those four views
  whole.
- **BCF topics.** A viewpoint is exported and shown on its own (`bcf.export_viewpoint`,
  `bcf.import_viewpoint`); issues with their viewpoints in a `.bcf` archive are not built yet.
