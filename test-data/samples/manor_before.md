# Manor before coordination: finding and resolving clashes

`manor_before.ifc` is the [Manor with tower](manor.md) as first modelled: built by the same
script, `manor.lua`, with *Coordinate overlaps* off, so the last step, which resolves where the
parts meet, is left out. Its 19 clashes are where real coordination work starts: the castle
towers' walls run through the house's eaves, slabs and walls, and the gallery's barrel vault and
floor run into the round tower. `manor.ifc` is the same model after coordination, with no
clashes. Together they show clash detection from start to finish (spec COL-CLASH-001, 002).

1. **Open it.** Help › Sample models › *Manor before coordination*. Open the 3D view (View ›
   Default 3D View).
2. **Run a clash test.** View › Coordination › **Clash Detection** opens the pane at the bottom.
   Click **Run Test…**, tick nothing for set A and set B (every element against every element)
   and OK both. The test runs outside Reviera, on the compute host, with progress in the pane;
   then the list shows 19 *new* clashes, largest first, grouped by category pair (IfcRoof ×
   IfcSlab, IfcRoof × IfcWall, …).
3. **Navigate.** Click a clash: both elements are selected and the view zooms to the clash. Use
   **Next** and **Previous** to step through them. **Isolate** shows only the two elements, and
   keeps doing so as you step; **Section Box** boxes the clash; **Reset View** shows everything
   again.
4. **Review.** For a clash that is intended, choose *Reviewed* (or *Approved*) in the status
   list, type a comment, and click **Apply**. It leaves the *Open* list; choose *All* in Show to
   see it.
5. **Resolve.** For a tower wall through the roof eave, click **Resolve…** and choose *Cut
   IfcRoof … by IfcWall …* (answer *No* to hiding the wall). The roof is notched round the wall,
   as in the coordinated Manor.
6. **Run again.** Click **Run Again**: clashes found again become *active*, reviewed and
   approved ones keep their status, and the one you resolved becomes *resolved*.
7. **Report.** **Export…** writes the results as CSV or HTML. The results are kept with the
   model: save it, reopen it, and the pane lists them with their statuses and comments.

To build the coordinated version yourself, resolve every clash this way, or run `manor.lua` with
*Coordinate overlaps* on (Scripts › Run Script…).

The same steps work from the command shell, Lua or MCP: `clash.run_basic`, `compute.status`,
`clash.collect`, `clash.last_report`, `clash.set_status`, `clash.refresh`, `clash.export_report`.
