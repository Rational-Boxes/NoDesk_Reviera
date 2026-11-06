# An accessible hotel guest room: building it by hand

How to build `guest_room.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet, and the [gaps](#gaps) section lists them. The script `guest_room.lua`
is the reference: the same commands with exact values.

The room is a guest room with mobility features under the 2010 ADA Standards. The rules come
from two interiors rule packs (ARC-RULE-001):
- `ada_2010_interiors`: a 30 × 48 in (762 × 1219 mm) clear floor space for a parallel approach
  along each side of a bed (§806.2.3);
- `practice_interiors`: planning practice, reported as notes. It asks 36 in round a dining table,
  30 in in front of a dresser and 24 in at the foot of a bed.

Start with **File › New**. In **Manage › Model Properties**:
- set the **Accessibility rule set** to *2010 ADA Standards*; that alone turns on the ADA
  interiors pack;
- under **Interiors rules**, also tick *Interior planning practice*.

1. **The room.** Draw four 200 mm **Architecture › Wall**s, 2.7 m high, round a 4.0 × 5.4 m inside
   from (0, 0). Draw an **Architecture › Floor** over the inside. Put an **Architecture › Door** in
   the south wall, 1.0 m along it.
2. **The furniture types.** Each piece is a box type for now. **[gap]**: the window has no box
   type maker yet, so import a box with **Architecture › Component › Import Object** in the
   *Furniture* category. In the type's **Properties**, set **IFC Type** or **Kind**; the packs
   choose furniture by these:

   | Type | Size (w × d × h) | IFC Type / Kind |
   |---|---|---|
   | Queen bed 60 × 80 in | 1524 × 2032 × 600 mm | BED |
   | Nightstand | 500 × 400 × 600 mm | USERDEFINED, Kind *Nightstand* |
   | Dresser | 1500 × 500 × 800 mm | USERDEFINED, Kind *Dresser* |
   | Desk | 1200 × 600 × 750 mm | DESK |
   | Desk chair | 500 × 500 × 850 mm | CHAIR |

   A type's front is its −y side: the foot of a bed, the drawers of a dresser.
3. **The bed and nightstands.** Place the bed with **Architecture › Component**, its head 20 mm off
   the north wall: centre (2.0, 4.364). Its use zones appear as green virtual volumes. Each is
   762 mm wide along a side, running 1219 mm from the foot. Place a nightstand at each side of
   the head, 20 mm from the bed: (0.968, 5.18) and (3.032, 5.18). They stand beyond the 48 in
   spaces, so nothing is reported.
4. **The dresser and desk.**
   - Place the dresser against the east wall at (3.73, 1.6), turned −90° so it faces west. Its
     30 in practice zone lies clear in front of it.
   - Place the desk against the west wall at (0.32, 2.45), turned 90°, clear of the door's
     maneuvering space.
5. **A chair in the way.** Drop the desk chair beside the bed at (0.85, 3.9) and choose
   **View › Coordination › Check Rules**. It reports one error: the chair is in the bed's left
   clear space (*ADA 2010 806.2.3 left*, 2010 ADA Standards §806.2.3, source S1354). Pick it to
   select and frame the bed and the chair.
6. **Clear it.** Move the chair in at the desk, to (0.9, 2.45) turned −90°, and run **Check
   Rules** again: no problems found.

## Gaps

- **[gap]** No box type maker in the window: the script uses `mep_type.create`, and by hand a
  box is imported. The interiors furniture library (`INTERIORS_PLAN.md` §4.1) will bring beds,
  dressers and desks with their IFC types already set.
- **[gap]** One clear space between two beds also meets §806.2.3. Riviera does not find that
  case: mark one bed not accessible (`rules.set_accessible`) when two share a space.
- **[gap]** The guest room's other ADA features (turning space, the accessible bathroom,
  reach ranges) and the Table 224.2 room counts come with later steps of the interiors plan.
