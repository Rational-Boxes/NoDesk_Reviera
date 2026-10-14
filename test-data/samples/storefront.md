# Storefront doors: building it by hand

How to build `storefront.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet, and the [gaps](#gaps) section lists them. The script `storefront.lua`
is the reference: the same commands with exact values.

Start with **File › New**. The model has Level 1 at 0.

1. **Rules.** Manage › Settings › **Model Properties**: Accessibility rule set *ADA 2010*.
2. **Walls.** Level 1 plan, Architecture › **Wall**: a 24000 × 10000 rectangle clockwise from
   (0, 0); select the walls and set Unconnected Height 4000.
3. **Back door and window.** Architecture › **Door** on the north wall at 3.0 m, **Window** at
   12.0 m (these make the default types).
4. **Door types.** Select the back door; Properties › Type › *Duplicate Type…* for each, then in
   Type Parameters:
   - *Revolving 2200*: Width 2200, Height 2400, Operation *Revolving*.
   - *Automatic Sliding 2000*: Width 2000, Height 2300, Operation *Sliding (Bi-parting)*,
     Leaves *Glass (Storefront)*, Automatic *Yes*.
   - *Storefront Pair 1800*: Width 1800, Height 2300, Operation *Double Swing*, Leaves *Glass*.
   - *Automatic Storefront 1000*: Width 1000, Height 2300, Leaves *Glass*, Automatic *Yes*.
5. **Shop window type.** Select the window; duplicate it as *Shop Window 1800 x 2400*, Width
   1800, Height 2400.
6. **The front.** Architecture › **Door** with each type on the south wall, measured from its
   east end (the wall's start): the revolving door at 4.0 m, the sliding door at 10.0 m (it
   claims the wall its leaves slide over, 1 m each side), the pair at 16.0 m and the automatic
   door at 20.5 m. **Window** with the shop window type at 6.6, 13.6 and 22.6 m, Sill 400. Set
   exact positions with Properties › Placement › From Wall Start.
7. **Floor.** Architecture › **Floor**: a rectangle from (100, 100) to (23900, 9900).
8. **Roof.** Architecture › **Roof** on the walls' exterior faces, 100 mm out, Base Offset 4000;
   Roof Shape *Flat*, Overhang 300.
9. **Room.** Architecture › **Room**: click inside, name *Shop*.
10. **Views.** View › **Floor Plan** for Level 1, and the 3D view.

## Gaps

None: every step has its UI.
