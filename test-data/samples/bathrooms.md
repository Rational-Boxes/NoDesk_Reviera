# Bathrooms and a home spa: building it by hand

How to build `bathrooms.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet, and the [gaps](#gaps) section lists them. The script `bathrooms.lua` is
the reference: the same commands with exact values.

Start with **File › New** and open the Level 1 plan. In **Manage › Model Properties** set the
**Building Code** to *International Residential Code (US)*. The bathroom rule packs follow it: `us_plumbing` (the IPC and IRC's
clearances, shower falls and waterproofing) and `us_spa` (NEC 680 for hot tubs, the sauna heater
table). Each room below is four 200 mm walls (**Architecture › Wall**, typing the corners);
the sizes given are inside the walls, from the inside corner named.

1. **A full bath.** Walls round 1.525 × 2.44 m (5 × 8 ft) from (0, 0). **Architecture › Build ›
   Bathroom Layout**: choose *Full bath (US, 5 × 8 ft)* and click inside the room. The tub goes
   across one end, and the WC and a pedestal lavatory go along a long wall at the IPC's
   spacings: the WC's centre is 15 in from the tub. **View › Coordination › Check Bathrooms**
   then reports that the WC stands in the tub's 21 in clear space. The source gives that space no
   width, so Reviera takes the tub's whole length. **View › Coordination › Check
   Overlaps** lists the same intrusion; pick it and choose *Accept as intended*.
2. **A three-quarter bath.** Walls round 1.6 × 2.8 m from (3, 0). Bathroom Layout again:
   *Three-quarter bath (shower)*. A 900 mm shower tray, the WC and the lavatory go along the wet
   wall.
3. **A level-access shower room.** Walls round 2.4 × 2.5 m from (6, 0), and an
   **Architecture › Floor** over the inside.
   1. **Architecture › Build › Bathroom**: choose *Roll-in shower, 60 × 30 in* and click near the
      south wall at x = 7.2. The 13 mm former goes against the wall.
   2. Select the shower, then use **Architecture › Build › Shower Falls**. Leave the fall empty,
      for the IPC's ¼ in per foot, and choose *Linear, along the back*. A floor drain goes 100 mm
      in from the back wall. The floor under the shower now falls to it: the front edge stays
      level and the back drops 16 mm. The floor's shape-editing points show the warp.
   3. **Architecture › Build › Waterproofing** lists the shower's walls to 6 ft and its floor,
      with areas. Answer *Yes* to copy the list as CSV.
4. **A hot tub room.** Walls round 5 × 4 m from (10, 0), and a floor.
   1. **Bathroom**: *Hot tub, 2.0 × 2.0 m*. Place it away from the walls, at the room's middle
      (12.5, 2.0). It keeps 0.9 m clear all round for its service panels; set the view's
      **Virtual Volumes** to translucent (View › Coordination) to see that space in green.
   2. Its type carries a filled weight of 1800 kg and 5 seats, in the `SpaProperties` property
      set. Check Bathrooms works out the filled load, 5.4 kPa. The tub is on the lowest level, so
      the note asks for a 100 mm reinforced concrete pad. Upstairs, the same load would be
      reported against the 40 psf floors are commonly designed for.
5. **A sauna.** Walls 2.2 m high round 2.4 × 2.0 m from (17, 0).
   1. **Architecture › Build › Sauna**: two tiers, no uninsulated surface, insulated walls, and
      the heater left empty. Click inside the room.
   2. Bench modules 600 mm long go along a long wall in two tiers. The upper tops are 1.05 m up,
      1.15 m below the ceiling; the lower tops are 0.6 m. An 8 kW wall heater, sized for the
      room's 10.6 m³, goes on an end wall. The status bar gives the power. The script names the
      south wall as the bench wall (`bench_wall_id`), so the benches run along it.
   3. Put a door in the north wall (**Architecture › Door**). If Check Bathrooms says it opens
      into the sauna, select it and flip it (**Modify › Doors › Flip Facing**): sauna doors
      open outward, with no lock.

6. **A steam room.** Walls 2.4 m high round 2.4 × 1.8 m from (21, 0), and a floor.
   1. **Architecture › Build › Steam Room**: the ceiling's high side 2130 mm, ceramic tile, no
      outside wall. Click inside the room.
   2. The ceiling goes over the room: 2.13 m over the south wall, falling 1:6 to 1.83 m at the
      north wall.
   3. Four tiled bench modules go along the south wall. The steam head goes low on the east wall
      and the control 1.5 m up on the north wall.
   4. The floor falls ¼ in per foot to a centre drain.
   5. The status bar asks for a generator for 8.6 m³.
   
   The script names the south wall as the bench wall (`bench_wall_id`).

Check Bathrooms on the finished model gives one note: the hot tub's pad.

## Gaps

- **[gap]** The Sauna tool takes the longer wall for the benches. Its dialog has no choice of
  bench wall (`bench_wall_id`), and none of the heater's end.
