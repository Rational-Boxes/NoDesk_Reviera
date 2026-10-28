# Greenhouses: building it by hand

How to build `greenhouses.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet, and the [gaps](#gaps) section lists them. The script `greenhouses.lua`
is the reference: the same commands with exact values.

Start with **File › New**. The model has Level 1 at 0. Open the Level 1 plan.

1. **A Venlo range.** Architecture › **Glazed Enclosure**. Form *Venlo*; Length 24 m, Truss
   width 8 m, Trusses 3, Bay spacing 4 m, Eave height 5.5 m, Roof pitch 22°, Knee wall 600 mm.
   Untick *Place by clicking* to place it at the origin, or leave it ticked and click (0, 0),
   then a point east of it. The range is 24 × 24 m: six 4 m glass roofs with a valley gutter
   between each pair, glass walls on a knee wall, the end walls' glass filling up into all six
   gables and dropping under the valley gutters, and two eaves gutters.
2. **Change it.** With the range selected, Properties › Glazed Enclosure shows its form and
   sizes; set Trusses to 4 and back to 3, or the Eave Height, and the parts are made again.
   (The script leaves it at 3 trusses.)
3. **A gutter-connected range.** Glazed Enclosure again: Form *Gutter-connected multispan*,
   Length 16 m, Span 9.6 m, Spans 3, Bay spacing 4 m, Eave height 4.5 m, Roof pitch 22.6°
   (5:12), Knee wall 0. Click (60, 0), then a point due north: the length runs north and the
   three spans step west.
4. **A hobby greenhouse.** Glazed Enclosure: Form *Gable*, Length 6 m, Span 3.6 m, Bay spacing
   1.2 m, Eave height 2.2 m, Roof pitch 25°. Place it at (0, 32).
5. **Look at it.** In a 3D view the glass is translucent; select a roof to see its glazing grid,
   or an end wall to see the glass clipped to the gables. Each part says which enclosure it
   belongs to; Modify › **Detach from Enclosure** **[gap]** (`enclosure.detach`) keeps a part
   when the enclosure changes.

Check: three enclosures (category *Glazed Enclosures*); the Venlo range has 6 roofs, 4 glass
walls, 4 knee walls and 7 gutters; the multispan 3 roofs and 4 gutters; the hobby house 1 roof
and 2 gutters. Saved, each is an `IfcElementAssembly` (ObjectType *GlazedEnclosure*) aggregating
its parts.

## Gaps

- **Detach from Enclosure** has no menu entry yet; `enclosure.detach` does it.
- **Frame members, doors and vents.** The enclosures have no posts, rafters or trusses (the
  glazing bars stand in), no doors in the glass walls and no vents yet (ARC-GLZ-001, -004).
- **Hoop, gothic and sawtooth forms**, glazed vaults and geodesic domes (ARC-GLZ-002, -006).
- **Headhouse**, growing layouts (ARC-GLZ-005), and the glazing and frame type library
  (ARC-GLZ-003): the two glass types are made on first use with provisional values.
- **Keeping edits.** A change rebuilds every part, so hand edits to a part are lost unless the
  part is detached first.
