# Structural model with problems: finding them

`structural_checks.ifc` is a small steel frame with faults made on purpose, for practising the
analytical model checks (STR-AM-007). The script `structural_checks.lua` builds it; each fault is
labelled in the Level 2 plan.

1. Open the sample and a 3D view; Analyze › Analytical › **Show Analytical**.
2. Analyze › Checks › **Check Supports**, then **Check Consistency**. The Analytical Checks pane
   lists, by severity:

| Fault (label) | What the checks report |
|---|---|
| 1. A ring of four beams with nothing under it | S2 circular support; C2 isolated part (an error) |
| 2. A cantilever whose far end rests on nothing | S1 unsupported member; C1 free member end |
| 3. A member drawn by hand 40 mm short of a column | C1 disconnected nodes, 40 mm against the 5 mm merge distance |
| 4. A member across the bay landing mid-span on the girders, and a beam there with no member | C1 end on span (twice); C2 isolated part; C5 no counterpart (the member, analytical-only, and the beam, with no analytical member) |
| 5. The south girder moved 250 mm after automation | C3 physical/analytical mismatch, 250 mm against 150 mm; C4 analytical outside physical |
| 6. The east girder's member pinned at both ends | C7 instability hint |
| 7. A slab 700 mm below the ring's beams | C6 overlap not reflected (four beams); S1 unsupported panel; C2 isolated part |

   C5 also lists the other hand-drawn member (fault 3) as analytical-only, for information.
3. Select an issue, **Zoom To** it and read its measured distance; **Ignore** one with a reason
   and see it stay ignored on **Re-check**.
4. Structural Analysis › **Run Analysis**: the C2 errors hold the run (*Run anyway* or review).
5. Fix them and re-check:
   - fault 1: put columns under the ring, or Loads › **Point Support** at its corners;
   - fault 2: a column under the cantilever's end, or delete it;
   - fault 3: Analyze › Analytical › **Adjust**, click the member's end by the column, click the
     same spot again (it snaps onto the column's node) and press Enter: the two nodes merge;
   - fault 4: delete the hand-drawn member and run **Physical to Analytical** for the beam (it
     splits the girders where the beam frames in);
   - fault 5: select the girder and click Analyze › Analytical › **Reset** to bring its
     analytical member to it, or move the girder back (Modify › **Move**, 250 mm);
   - fault 6: select the issue (**Select** in the pane) and set **Start Release** and **End
     Release** back to *Fixed* in Properties;
   - fault 7: set the slab's offset back to −300 mm.
6. Manage › **Structural Settings**: turn on C8 to see the hand-drawn members without a section,
   and the automatic checks to have them run after each change.

Selected analytical members, panels and nodes show their own rows in Properties: role, material,
releases and Analyze As; a panel's thickness and cladding; a node's support.

## Gaps

None: every step has its UI.
