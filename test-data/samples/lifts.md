# Hospital, freight and small lifts: building it by hand

How to build `lifts.ifc` in the window. Each step names the UI path; **[gap]** marks a step
the window cannot do yet, and the [gaps](#gaps) section lists them. The script `lifts.lua` is the
reference: the same commands with exact values.

Start with **File › New** and leave the **Building Code** at *International Building Code (US)*
(**Manage › Model Properties**). Add Levels 2–4 at 3.5, 7.0 and 10.5 m, and a 12 × 10 m
**Architecture › Floor** from (0, 0) on every level.

Every lift below comes from **Architecture › Circulation › Elevator** in the Level 1 plan. Choose
its **Kind**, the level it **serves up to**, and type the entrance point (or click it). Leave the
load and speed empty: each kind takes the first car of its catalogue and its usual speed. The
lifts stand close together, so set **Controller and machine** to *None* for each (the sample
*Elevators in an office core* shows controllers and machine spaces).

1. **Vehicle elevator.** *Vehicle*, up to Level 2, drive *Hydraulic*, at `2.2, 1`: a class B
   8000 lb car, 2438 × 6426 mm. Its entrances are 8 ft openings, because vertical bi-parting doors
   are not modelled yet.
2. **Freight elevator.** *Freight*, up to Level 2, *Hydraulic*, at `6, 1`: a class A 2500 lb car,
   1626 × 2134 mm, with a 1524 mm pit and a 4496 mm overhead.
3. **Hospital elevator.** *Hospital or service*, up to Level 4, at `10, 1`: a 5000 lb car,
   1727 × 2743 mm, that takes a bed and the stretcher. Its 1372 mm two-speed doors stack inside
   the hoistway, so each entrance is an opening for now; the status bar says so.
4. **LULA.** *LULA (limited use)*, up to Level 2, at `6, 5`: 1400 lb, 48 × 54 in, with a 14 in
   pit. Its swinging entrance opens onto the landing.
5. **Home elevator.** *Home (private residence)*, up to Level 2, at `9, 6`: 1000 lb, 36 × 54 in.
6. **Platform lift.** *Vertical platform lift*, up to Level 2, at `6, 8`: 750 lb, 36 × 60 in, a
   3 in pit, 3.5 m of travel.
7. **Dumbwaiter.** *Dumbwaiter*, up to Level 3, at `11, 8.5`: 500 lb, 36 × 36 × 48 in, its
   openings 762 mm above each floor for loading from a counter.

**Architecture › Circulation › Check Elevators** then gives one note: the makers' 5000 lb hospital
car is 4.74 m², a little over the 4.64 m² A17.1's passenger table allows for its load. That rule
is advisory, because only restatements of A17.1 were read.

Try it: make the LULA serve Level 4 (a 7 m rise), and Check Elevators reports it against A17.1's
25 ft limit. Or give the freight car 1000 kg on a 2.4 × 3.0 m car, class C2: it needs at least
1757 kg.

## Gaps

- **[gap]** Vertical bi-parting freight doors and telescopic side-opening doors are drawn as
  openings.
- **[gap]** An elevator cannot be edited in place: delete it (`elevator.delete`) and make it
  again.
