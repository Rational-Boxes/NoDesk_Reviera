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
2. **The bed and nightstands.** Open **Riviera Interiors** in the **Component Library** panel.
   It ships with Riviera, beside your own libraries. From *Bedroom*, place *Bed, queen 60 × 80
   in* with its head 20 mm off the north wall: centre (2.0, 4.314). It is 1624 × 2132 mm on
   its frame. Its use zones appear as green virtual volumes, each 762 mm wide along a side and
   running 1219 mm from the foot. Place a *Nightstand* each side of the head, 20 mm from the bed:
   (0.889, 5.177) and (3.111, 5.177). They stand beyond the 48 in spaces, so nothing is
   reported.
3. **The dresser and desk.**
   - Place *Dresser* against the east wall at (3.7385, 1.6), turned −90° so it faces west. Its
     30 in practice zone lies clear in front of it.
   - From *Office*, place *Desk 60 × 30 in* against the west wall at (0.401, 2.4), turned 90°,
     clear of the door's maneuvering space.

   Each piece comes in typed: the bed as BED, the desk as DESK, the nightstands and dresser as
   USERDEFINED with their kind. That is how the packs know them.
4. **A chair in the way.** Place a *Task chair* beside the bed at (0.85, 3.9). It is in the
   *Office* folder, or comes from the catalogue by script (`furnishing.load`). Choose **View ›
   Coordination › Check Rules**. It reports one error: the chair is in the bed's left clear
   space (*ADA 2010 806.2.3 left*, 2010 ADA Standards §806.2.3, source S1354). Pick it to select
   and frame the bed and the chair.
5. **Clear it.** Move the chair to the desk, at (1.107, 2.3) turned −90°, facing it, and run
   **Check Rules** again: no problems found.
6. **Keep the result.** Answer *Yes* to *Save a report of this check?*, or choose **Manage ›
   Settings › Save Report** › *Interiors rules*. Save to this computer, or to a FileEngine folder
   when one is connected. The `.html` page names the model, the date and the rule packs' check.
   A `.csv` gives the findings as a table, `.md` as Markdown, and `.json` as Riviera returns them
   (`report.write`).

## Gaps

- **[gap]** One clear space between two beds also meets §806.2.3. Riviera does not find that
  case: mark one bed not accessible (`rules.set_accessible`) when two share a space.
- **[gap]** The guest room's other ADA features (turning space, the accessible bathroom,
  reach ranges) and the Table 224.2 room counts come with later steps of the interiors plan.
