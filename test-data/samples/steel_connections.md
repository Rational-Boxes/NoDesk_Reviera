# Steel connection detailing: building it by hand

How to build `steel_connections.ifc` in the window. Each step names the UI path; **[gap]** marks a
step the window cannot do yet, and the [gaps](#gaps) section lists them. The script
`steel_connections.lua` is the reference: the same commands with exact values.

Start with **File › New**. Set the code to *IBC* (Manage › Settings › **Model Properties**), so
connections and bolts take US sizes. Coordinates are in mm; plates at Level 2 are sketched in the
Level 2 plan (its level is the work plane, z = 4000), and XY types a corner's coordinates.

1. **Level and views.** Elevation view, Structure › Datum › **Level**: Level 2 at 4000. View ›
   **Floor Plan** for Level 2, and a 3D view.
2. **Sections.** Structure › **Load Beam Section**: W18X35, W16X26 and W12X26 from *AISC W*;
   **Load Brace Section**: HSS6X6X3/8 from *AISC HSS (rectangular)*; with the Column tool active,
   Properties › Type › *Load Section…*: W12X53.
3. **The bay.** Structure › **Column**, W12X53, base Level 1, top Level 2, at (0, 0), (6000, 0),
   (6000, 6000) and (0, 6000). Level 2 plan, Structure › **Beam**: W18X35 girders from (0, 0) to
   (6000, 0) and from (0, 6000) to (6000, 6000); W16X26 beams along x = 0 and x = 6000; a W12X26
   infill beam from (3000, 0) to (3000, 6000). South elevation, Structure › **Brace**:
   HSS6X6X3/8 from (0, 0) at Level 1 to (6000, 0) at Level 2.
4. **Custom connection.** Select the column at (0, 0), then the south girder, Steel ›
   Connections › **Connection**, *End Plate*. Select the column, Steel › Detailing › **Plate**,
   face *web_2* (the web's west face): corners (3500, −100), (3900, −100), (3900, 100), (3500,
   100) on the face; Properties › Thickness 10. Select the end plate and the doubler, Steel ›
   Custom › **Customize**, named *End plate and doubler*.
5. **Placed again.** Select the column at (0, 6000), then the north girder, **Connection**, *End
   plate and doubler*: an end plate and a doubler in the same place relative to the join. Select
   either instance, Steel › Custom › **Custom Parameter**, *Plate 1.thickness*, 12: both doublers
   follow.
6. **Coped infill.** The infill beam stops clear of the girders' flanges. Select it, Steel ›
   Modifiers › **Shorten**: End *Start*, Length −71.9 (extends it to 13 mm off the girder's web);
   again with End *End*. Select the infill then the south girder, Steel › Cuts › **Cope**; the
   same with the north girder.
7. **Automation.** With nothing selected, Steel › Connections › **Automate Connections**, rule set
   *Default*, *Preview* then *Apply*: base plates under the four columns, shear plates at the
   other beam and girder ends and at the infill's ends, gussets at the brace's ends; the two
   custom connections' joins are skipped as already connected.
8. **Check and presize.** Select the shear plate at the north girder's east end; in Properties,
   Design forces (N, V, Vh kN; M kN·m): `ASCE7 2.3.2: 0, 246`. Steel › Connections › **Check**,
   the sample provider with *AISC 360*: bolt shear fails (three 3/4" bolts). **Presize** with the
   same provider: four rows of 7/8" bolts, OK. **Check Report** writes them as HTML.
9. **Bolted splice.** Level 2 plan, Steel › Detailing › **Plate** (nothing selected, on the level):
   corners (9800, 850), (10200, 850), (10200, 1150), (9800, 1150); Thickness 15. Select it,
   **Plate** on its *top* face: a 300 × 200 cover plate centred on it, Thickness 10. Select both,
   Steel › Detailing › **Bolts**: *Bolts*, square to the cover plate's top face, rectangular, 2
   rows, 3 columns, spacing 75 × 75, diameter blank (3/4"); click the cover plate's centre.
10. **Stiffener and weld.** Structure › Datum › **Ref Plane** from (10180, 900) to (10180, 1100),
    named in Properties; Architecture › Datum & Work Plane › **Set** to it. In a section through
    it, **Plate**: 200 wide, 100 high, standing on the 15 mm plate's top (z 4015 to 4115);
    Thickness 10, Justification *Centre*. Select the 15 mm plate and the stiffener, Steel ›
    Detailing › **Weld**, click the edge on the stiffener's east side in the 3D view: *Fillet*,
    throat 6.
11. **Corner and contour cuts.** Level 2 plan, **Plate** with corners (12000, 1000), (12300,
    1000), (12300, 1200), (12000, 1200), Thickness 10. Select it, Steel › Modifiers › **Corner
    Cut**: corner 0, *Straight*, 50; again, corner 2, *Convex (rounded)*, 50. **Contour Cut** on
    its *top* face: a 50 mm square from (125, 75) to (175, 125) on the face.
12. **Cut by.** **Plate** from (14000, 1000) to (14300, 1200), Thickness 10. A vertical plate
    across it on a ref plane at x = 14250 (as in step 10): y 1020 to 1060, z 3950 to 4050,
    Thickness 10, *Centre*. Select the first plate then the crossing one, Steel › Cuts › **Cut
    By**.
13. **Miter.** Level 2 plan, **Beam** W12X26 from (10000, 3000) to (12000, 3000) and from (12000,
    3000) to (12000, 5000). Select both, Steel › Cuts › **Miter**.
14. **Saw cut.** **Beam** W12X26 from (14000, 3000) to (14000, 5000) and W16X26 from (13000, 5000)
    to (15000, 5000). Select the W12X26 then the W16X26, Steel › Cuts › **Saw Cut**, *Flange*.
15. **Cut through.** **Beam** W12X26 from (10000, 7000) to (13000, 7000). A diaphragm on a ref
    plane at x = 11500: 400 wide (y 6800 to 7200), 500 high (z 3550 to 4050), Thickness 20,
    *Centre*. Select the diaphragm then the beam, Steel › Cuts › **Cut Through**, gap 10.
16. **Shortened end.** **Beam** W12X26 from (14000, 7000) to (17000, 7000). Select it, Steel ›
    Modifiers › **Shorten**: End *End*, Length 200, angle across the height 30°.
17. **Numbering.** Steel › Fabrication › **Numbering**: part marks (c1, b1…, br1, p1…) and
    assembly marks in each element's Tag. Steel › Fabrication › **NC Export** writes a DSTV NC1
    file for each part mark to a folder you choose (the script does not write files).
18. **Overlaps.** View › Coordination › **Check Overlaps**: the brace meets the columns, its
    gussets and a gusset bolt; *Accept as intended* each.
19. **Labels.** Level 2 plan, Annotate › **Text** for each feature's note.

Check: View › Coordination › **Check Overlaps** finds none. Properties of the north-east shear
plate show its check *OK*; a part's Properties show its Tag (part mark).

## Gaps

None: every step has its UI.
