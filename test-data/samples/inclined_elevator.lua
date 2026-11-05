-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: An inclined elevator up a hillside
-- description: A lower and an upper terrace 12 m apart joined by an inclined elevator 30 m across the hillside between them: the accessible 5 × 5 ft car on a double-drum drive, its track an alignment rising 12 m at 21.8°, with a level landing kept clear at each gate. The check finds nothing; on the smaller winding drum the same car would be too heavy for its drive.
--
-- Riviera sample model (test-data/samples): inclined elevators (ARC-VT-023), the first guideway
-- device on the alignment foundation (ARC-ALN-001). Car sizes, capacities and drive limits are
-- maker-style planning values (E-97).

riviera.code.set{ code = "ibc" }
local lower = riviera.level.list()[1].id
local upper = riviera.level.create{ elevation = 12.0 }

-- The two terraces: 10 × 8 m slabs, the lower west of the hillside, the upper east of it.
local function slab(x0, x1, level)
  local c = { { x0, -4 }, { x1, -4 }, { x1, 4 }, { x0, 4 } }
  local loop = {}
  for i = 1, 4 do
    loop[i] = { line = { start = c[i], ["end"] = c[i % 4 + 1] } }
  end
  riviera.floor.create{ loops = { loop }, level_id = level }
end
slab(-10, 0, lower)
slab(30, 40, upper)

-- The inclined elevator from the lower terrace's edge at (0, 0) to the upper one's at (30, 0):
-- 32.3 m of track at 21.8°, two minutes a trip at 0.25 m/s. The accessible car (5 × 5 ft inside,
-- 36 in door, 1,075 kg commercial) on the double-drum drive (≤ 3,800 lb).
local lift = riviera.inclined_elevator.create{
  bottom = { point = { 0, 0 }, level = lower }, top = { point = { 30, 0 }, level = upper },
  size = "ada", drive = "wdd", accessible = true,
}
print(string.format("inclined elevator: %.1f m at %.1f°, %.0f s a trip, %.0f kg", lift.length, lift.angle, lift.trip, lift.capacity))
assert(#riviera.inclined_elevator.check{} == 0, "nothing to report")
assert(#riviera.clearance.check{} == 0, "its landings are clear")

-- Its track is an alignment from 0 up to 12 m.
local track = riviera.alignment.list()[1]
assert(track.elevations[1] == 0 and math.abs(track.elevations[2] - 12) < 1e-9, "the track rises 12 m")
