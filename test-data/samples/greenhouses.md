# Greenhouses: building it by hand

How to build `greenhouses.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet, and the [gaps](#gaps) section lists them. The script `greenhouses.lua`
is the reference: the same commands with exact values.

Start with **File › New**. The model has Level 1 at 0. Open the Level 1 plan.

1. **A Venlo range.** Architecture › **Glazed Enclosure**. Form *Venlo*; Length 24 m, Truss
   width 8 m, Trusses 2, Bay spacing 4 m, Eave height 5.5 m, Roof pitch 22°, Knee wall 600 mm.
   Untick *Place by clicking* to place it at the origin, or leave it ticked and click (0, 0),
   then a point east of it. The range is 24 × 16 m: four 4 m glass roofs with a valley gutter
   between each pair, glass side walls on a knee wall, end walls glazed to the ground with a door
   in the middle of each, their glass filling up into all four gables and dropping under the
   valley gutters, two eaves gutters, and ridge vents (hung open) on alternate sides of each
   ridge, 18 % of the floor area.
2. **Change it.** With the range selected, Properties › Glazed Enclosure shows its form and
   sizes; set Trusses to 3 and back to 2, or the Eave Height, and the parts are made again.
   (The script leaves it at 2 trusses.)
3. **A gutter-connected range.** Glazed Enclosure again: Form *Gutter-connected multispan*,
   Length 16 m, Span 9.6 m, Spans 3, Bay spacing 4 m, Eave height 4.5 m, Roof pitch 22.6°
   (5:12), Knee wall 0. Click (60, 0), then a point due north: the length runs north and the
   three spans step west. In Properties set **Roof Glazing** to *Diffuse AR Glass 4 mm*.
4. **A hobby greenhouse.** Glazed Enclosure: Form *Gable*, Length 6 m, Span 3.6 m, Bay spacing
   1.2 m, Eave height 2.2 m, Roof pitch 25°. Place it at (0, 32), and set its Roof Glazing to
   *Twin-Wall Polycarbonate 10 mm* and its **Frame System** to *Timber Glazing Bar 50×100*. Each
   glazing type carries its light transmission, U-value, g-value and safety class
   (`Reviera_GlazingUnit`), and its frame's material, bar section and deflection limit
   (`Reviera_FrameSystem`); Frame Shading shows the share of the roof the bars shade.
5. **A sawtooth range.** Glazed Enclosure: Form *Sawtooth*, Length 8 m, Span 6.4 m, Spans 3,
   Bay spacing 4 m (the eave height 4 m and slope 25° are the form's own when the Venlo's
   values are left). Place it at (30, 32). Each span is one glass slope rising from its gutter
   to a glazed vertical face at the high side; the top band of each face is vents, sized to
   Properties › **Face Vents** (18 % of the span's floor, so 1.15 m), and the next span's gutter
   runs along the face's foot. Turn the faces away from the prevailing wind or the sun.
6. **Two tunnels.** Glazed Enclosure: Form *Hoop tunnel*, Length 12 m, Span 9.1 m, Eave
   (sidewall) height 1.2 m; place it at (12, 40), its length running east. Then Form *Gothic
   tunnel* with the same sizes at (12, 52). Each is one roof by extrusion, its arch in 16 straight
   facets, under *Greenhouse Tunnel Film* with a hoop every 1.5 m at most; the gothic one's two
   arcs (radius 0.6 × the span) meet in a pointed crown. Their end walls rise into the arches,
   each with a door; tunnels have no gutters, vents or posts.
7. **Growing layouts.** Select the Venlo range, Architecture › **Growing Layout**: System *NFT
   tables*, Runs *Along the length*; the tables stand clear of the posts, in runs of at most 12 m
   falling 2 %. Then the multispan with *Rolling benches* along its length, the hobby house with
   *Benches* across it (width 0.9 m, aisle 0.6 m, main aisle 0.9 m), and the hoop tunnel with
   *Ebb-and-flow tables* along it. Properties › Growing Layout shows each one's growing area and
   share of the floor; change a size there, or the enclosure's, and the layout is laid out again.
8. **Look at it.** In a 3D view the glass is translucent; select a roof to see its glazing grid,
   or an end wall to see the glass clipped to the gables. Each part says which enclosure it
   belongs to; Modify › **Detach from Enclosure** **[gap]** (`enclosure.detach`) keeps a part
   when the enclosure changes.

Check: six enclosures (category *Glazed Enclosures*); the Venlo range has 4 roofs, 4 glass
walls (a door in each end wall), 2 knee walls, 5 gutters, 21 steel posts and 3 gutter beams; the multispan 3 roofs and 4 gutters; the hobby house 1 roof
and 2 gutters; the sawtooth 3 roofs, 6 glass walls (two of them inner vertical faces) and 3 gutters; the tunnels 1 roof by extrusion and 4 glass walls each. Saved, each is an `IfcElementAssembly` (ObjectType *GlazedEnclosure*) aggregating
its parts.

## Gaps

- **Detach from Enclosure** has no menu entry yet; `enclosure.detach` does it.
- **Frame members and vents.** The frame is posts and gutter beams only: no rafters, purlins or trusses yet
  (the glazing bars stand in), and only ridge vents (no side vents or roll-up sides, ARC-GLZ-004); the doors are plain hinged curtain-wall
  doors (no sliding doors or door types yet).
- Gutter-connected arch ranges, glazed smooth vaults and geodesic domes
  (ARC-GLZ-002, -006); the tunnels' arches are faceted, and they have no roll-up sides yet.
- **Headhouse**, high-wire gutters, layouts in rooms, and the glazing and frame type library
  (ARC-GLZ-003): the two glass types are made on first use with provisional values.
- **Keeping edits.** A change rebuilds every part, so hand edits to a part are lost unless the
  part is detached first.
