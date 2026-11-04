# Stairlifts and platform stairlifts in a parish hall and its house: building it by hand

How to build `stairlifts.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet, and the [gaps](#gaps) section lists them. The script `stairlifts.lua`
is the reference: the same commands with exact values.

Start with **File › New** and leave the **Building Code** at *International Building Code (US)*
(**Manage › Model Properties**). The vertical-transport rule packs follow it:
- the IBC's stair widths beside a folded unit (20 in on a dwelling's stair, otherwise the stair's
  required width);
- A18.1's 1524 mm head box over a platform's deck and its 0.15 m/s cap, as makers quote them;
- ADA's 30 × 48 in wheelchair space;
- the makers' generic sizes.

Add Level 2 at 3.0 m. Draw a 32 × 16 m **Architecture › Floor** from (0, 0) on Level 1. On
Level 2, draw the same floor with two inner loops for the hall's stairwells: (13, 1.8) to
(17.48, 3.0), and (24, 1.6) to (27.4, 4.86).

Every lift comes from **Architecture › Circulation › Stairlift** with its stair selected.

1. **The house's stair and its seated lift.**
   - Draw an **Architecture › Stair** on Level 1, 914 mm wide, from `3, 3` to `7.48, 3`: 17
     risers of 176 mm on 280 mm treads.
   - Put **Modify | Stairs › Railings** on it and delete the left one. **[gap]**: the window
     places both sides.
   - Use **Modify | Stairs › Stair Openings** to open Level 2 over the stair for its headroom.
   - Select the stair. Choose **Stairlift**: *Seated stairlift*, *Left*, stair *In a dwelling*.
   - It folds to 320 mm from the wall, leaving 594 mm of the stair. The IBC asks 508 mm on a
     dwelling's stair.
2. **The hall's straight stair and its platform lift.**
   - Draw a stair 1200 mm wide from `13, 2.4` to `17.48, 2.4`, with a railing on its left only.
   - Select it. Choose **Stairlift**: *Wheelchair platform lift*, *Right*, *The family's
     largest*, stair *Other*, 40 people.
   - It takes the straight family: an 800 × 1250 mm platform folding to 260 mm, which leaves
     940 mm. The stair needs 914 mm, since it serves fewer than 50 people.
   - Its head box rides 0.92 m above the nosings at the stair's 32.7°. The stairwell lets it
     clear Level 2.
3. **The hall's back stair, which turns.**
   - Draw a stair 1300 mm wide with two runs: from `24, 2.25` to `26.24, 2.25`, then from
     `26.75, 2.9` to `26.75, 4.86`. It turns left at a landing.
   - Select it. Choose **Stairlift**: *Wheelchair platform lift*, *Right* (the outside of the
     turn), 40 people.
   - A stair that turns takes the curved family: an 800 × 1220 mm platform on tubes that follow
     the landing. It needs 1225 mm on the inside of the bend, and the stair has 1300 mm.
4. **Check.**
   - **Architecture › Circulation › Check Stairlifts** finds no problems.
   - **View › Coordination › Check Clearances** finds nothing in the running envelopes, head
     boxes, run-outs or wheelchair spaces.
   - Set a 3D view's virtual volumes to *Translucent* to see them.

Try these:
- Cut the hall stair's opening with **Stair Openings** instead of the stairwell. Check Clearances
  then finds Level 2 in the platform's head box: the opening was made for walking headroom only.
- Make the house's stair *Other*. Check Stairlifts then reports that 594 mm is short of the
  stair's required width.
- Under the England packs (`vt.set_rule_packs` with `england_adm` and `practice_en`), choose an
  800 × 1000 platform. ADM's 800 × 1250 minimum flags it.

## Gaps

- **[gap]** Railings on one side only: the window's **Railings** places both sides. Delete the
  one on the lift's side.
- **[gap]** The lift is made for its stair as it is. If the stair changes, delete the lift and
  place it again.
- **[gap]** There are no running envelopes over winders, spirals or a turning stair's landings
  yet.
