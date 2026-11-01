# Washrooms and a locker room: building it by hand

How to build `washrooms.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet, and the [gaps](#gaps) section lists them. The script `washrooms.lua` is
the reference: the same commands with exact values.

Start with **File › New** and open the Level 1 plan. In **Manage › Model Properties** set the
**Building Code** to *International Building Code (US)* with the occupancy *Other (public, commercial, common areas)*. In **View › Coordination ›
Accessibility Rules** choose *ADA 2010*. The bathroom rule packs follow: `us_plumbing` (the IPC's
clearances and Table 403.1's counts) and `ada_2010`. Each room is four 200 mm walls 3 m high
(**Architecture › Wall**, typing the corners); the sizes given are inside the walls.

1. **A men's room**, 9 × 4 m from (0, 0).
   1. Select its north wall and use **Architecture › Build › Toilet Core**. Set Room side to
      *Interior face*, Numbers to *Counted for an occupancy*, Occupancy *B*, Occupant load 150
      and Room for *Men*. The core runs along the wall's whole inside face, from (0, 3.9) to
      (9, 3.9).
   2. IPC Table 403.1 asks for three WCs for the 75 men, and one of them may be a urinal. The
      core lays out:
      - an accessible compartment, 60 in wide and as deep as the standard ones, with its WC
        18 in from the side partition and its side and rear grab bars;
      - a standard compartment;
      - a urinal;
      - two lavatories.
      
      The partitions stand 12 in off the floor. The compartment doors swing out: the accessible
      one is by the far partition (ADA 604.8.1.2), and the standard one is centred.
   3. The standard compartment and its door need not meet the ADA. The core marks them so
      (`bath.set_accessible`), and the door then keeps its swing but no manoeuvring clearances.
   4. **View › Coordination › Fixture Count** with the same occupancy and load gives the whole
      building's sums: the women's room, drinking fountains and a service sink are still short.
2. **An accessible single-user toilet**, 1.8 × 2.6 m from (11, 0). **Architecture › Build ›
   Bathroom Layout**: choose *Accessible single-user toilet*, with the west wall as the wet wall.
   Click inside the room. The WC goes 18 in from the side wall with the lavatory beside it, and
   the ADA's side and rear grab bars go on.
3. **A locker room**, 6 × 5 m from (0, 6).
   1. **Architecture › Build › Locker Room**: 40 lockers, two tier, six gang showers, accessible.
      Click inside the room.
   2. Lockers run along both long walls with the ADA's 30 in in front of them, and benches go
      down the middle. The showers go along one end wall, 750 mm apart, behind a drying zone.
      An accessible bench goes against the far wall.
   3. Two of the 40 lockers (5 %) are kept accessible, the first of each run.

**View › Coordination › Check Bathrooms** on the finished model finds nothing.

## Gaps

None: every step can be done in the window.
