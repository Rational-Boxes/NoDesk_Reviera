# Family bathroom and wet room (England): building it by hand

How to build `wet_room.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet, and the [gaps](#gaps) section lists them. The script `wet_room.lua` is
the reference: the same commands with exact values.

Start with **File › New** and open the Level 1 plan. In **Manage › Model Properties** set the
**Building Code** to *Approved Document K (England)* with the occupancy *Within a dwelling unit*. The bathroom
rule packs follow: `england_adm` (Approved Document M's spaces, and wet-room falls and tanking
from practice sources) and `uk_bs7671` (BS 7671 Section 701's electrical zones). Each room is
four 200 mm walls 2.4 m high (**Architecture › Wall**, typing the corners); the sizes given are
inside the walls.

1. **A family bathroom**, 2.0 × 2.4 m from (0, 0).
   1. **Architecture › Build › Bathroom Layout**: choose *Family bathroom (UK)* and click inside
      the room.
   2. A 1700 × 700 mm bath goes across one end. The WC and a pedestal basin go along the wet
      wall, the WC far enough from the bath that its 750 mm access zone (M4(1)) stays clear.
   3. Set the view's **Virtual Volumes** to translucent (View › Coordination). The bath's
      electrical zones show in yellow: zone 0 in the bath, zone 1 above it to 2.25 m, and zone 2
      a further 0.6 m out.
2. **A wet room**, 2.6 × 2.4 m from (4, 0), with an **Architecture › Floor** over the inside.
   1. **Architecture › Build › Bathroom**: choose *Roll-in shower, 60 × 30 in* and click near the
      south wall at x = 5.3.
   2. Select it and use **Architecture › Build › Shower Falls** with the fall left empty and a
      centre drain. Approved Document M's practice is 1:60, at least 1:80 and at most 1:40. A
      centre drain in a 1525 × 760 mm area cannot give 1:60 both ways without being steeper
      than 1:40 on the short runs. Left to its default, the tool sinks the drain only as far
      as 1:40 over the shortest run allows, so the long runs fall 1:80. Check Bathrooms finds
      the falls within the range. A linear drain along the back would fall evenly.
   3. The level shower makes the room a wet room. **Architecture › Build › Waterproofing**
      lists its whole floor (6.24 m²) and the shower's back wall to 1.8 m as tanked.
3. **A towel rail.** **Bathroom**: *Heated towel rail, 500 × 800*, on the north wall at
   x = 6.0. In its properties, set `Pset_ElectricalDeviceCommon` › **IP_Code** to *IPX4*.
   - The rail stands 2.4 m from the shower's head, beyond the 1.2 m zone 1 of a shower with no
     tray.
   - Move it within 1.2 m and set IP_Code to IP20: Check Bathrooms reports it. With no rating at
     all, it gives a note asking for one.

Check Bathrooms on the finished model finds nothing.

## Gaps

None: every step can be done in the window.
