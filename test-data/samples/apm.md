# An airport people mover: building it by hand

How to build `apm.ifc` in the window. Each step names the UI path; **[gap]** marks a step the
window cannot do yet. The script `apm.lua` is the reference: the same commands with exact values.

Start with **File › New IFC 4.3 Project**.

1. **The line.** **Massing & Site › Alignment**, its points one a row, no profile:
   ```
   0, 0
   300, 0, 150
   450, 150
   ```
   It runs east, then bends north-east round a 150 m curve: 506 m in all.
2. **The guideway.** Select the alignment, then **Architecture › Circulation › APM Guideway**:
   two lanes 4.75 m apart with a walkway. The deck is 9.02 m wide, and each lane's 3.66 m dynamic
   envelope is kept clear for **Check Clearances**.
3. **The stations.** Select the guideway, then **APM Station** three times, at chainages 30
   (*Landside*), 200 (*Concourse A*) and 480 (*Concourse B*), or click each beside the guideway.
   Each has two-car trains, side platforms 30.4 m long and 4 m wide, and screen doors.
4. **The service.** **[gap]** `apm.check` with a 120 s headway, two cars of 60 and a 3,000 peak:
   3,600 persons an hour each way, and nothing to report. A 60 m curve, an 8 % grade or a station
   on the curve would be.

## Gaps

- **[gap]** Vehicles are boxes standing at the stations. Guideway spans and columns, crossovers,
  the power rail, maintenance and storage, and IFC 4.3's railway classes are not modelled.
- **[gap]** Platform clearing (ANA-ROUTE-005) does not yet read APM station platforms.
