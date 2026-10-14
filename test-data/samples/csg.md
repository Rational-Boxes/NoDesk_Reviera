# Overlap resolutions: building it by hand

How to build `csg.ifc` in the window. Each step names the UI path; **[gap]** marks a step the
window cannot do yet ([gaps](#gaps)). The script `csg.lua` is the reference.

Start with **File › New**, and open a Level 1 plan (View › Floor Plan).

1. **The pier type.** Start the Wall tool; with nothing selected, Properties › Type chooses the
   type new walls get. Draw any wall, select it, and in Properties › Type choose Duplicate
   Type… as *Pier 600*, then set Width 600.
2. **Eight pairs.** For n = 0 … 7, at x = 7000 · n:
   - wall A: Architecture › **Wall** (type *Generic - 200mm*) from (x − 2000, 0) to (x + 2000,
     0), 3000 high (the tool's default);
   - pier B: the Wall tool with type *Pier 600*, from (x, −1000) to (x, 1000); then select the
     piers and set Unconnected Height 4500.
3. **Resolve each pair.** View › **Check Overlaps**, pick the pair, then in the
   *Resolve Overlap* dialog:
   1. leave the first unresolved;
   2. *Cut A by B*;
   3. *Cut B by A*;
   4. *Join (by precedence)*;
   5. *Keep only what A shares (intersect)*;
   6. *Fuse B into A (union)*;
   7. *Move B apart*;
   8. *Accept as intended*.
4. **Labels.** Annotate › **Text**: click below each pair and type its name (*1. Unresolved*,
   *2. Cut A by B*, …).

Check: View › **Check Overlaps** lists exactly one overlap (the unresolved pair).

## Gaps

None: every step has its UI.
