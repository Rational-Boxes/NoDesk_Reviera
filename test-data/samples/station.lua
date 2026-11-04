-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: Escalators in a transit station
-- description: A concourse over a platform 7 m below, joined by an up-and-down pair of transit escalators (heavy duty, 0.65 m/s) each with a 6 m station run-off zone beyond its ends; escalator.traffic checks the units a 9,000-an-hour peak needs with one out, and what the escalators add to the platform's egress capacity under NFPA 130 (at most half) against the IBC (nothing).
--
-- Reviera sample model (test-data/samples): escalators in stations (ARC-VT-020, ARC-VT-021).
-- `practice_us` gives the transit speed and the 6 m run-off (SCDF, LTA, Network Rail); the unit
-- flow (100 persons a minute) and the evacuation rate (120 a minute per metre of tread, TfL) are
-- planning values (E-90).

reviera.code.set{ code = "ibc" }
local platform = reviera.level.list()[1].id
local concourse = reviera.level.create{ elevation = 7.0 }

-- A 70 × 24 m slab on each level.
local c = { {0, 0}, {70, 0}, {70, 24}, {0, 24} }
local loop = {}
for i = 1, 4 do
  loop[i] = { line = { start = c[i], ["end"] = c[i % 4 + 1] } }
end
for _, l in ipairs({ platform, concourse }) do
  reviera.floor.create{ loops = { loop }, level_id = l }
end

-- An up-and-down pair from the platform to the concourse: a station's escalators are heavy duty
-- (a truss 640 mm wider than the step, so the pair stands a truss width apart), run at 0.65 m/s
-- and keep 6 m clear beyond each end.
local up = reviera.escalator.create{ point = { 20.0, 10.0 }, base_level = platform, top_level = concourse, context = "transit" }
local down = reviera.escalator.create{ point = { 20.0, 10.0 + up.truss_width }, base_level = platform, top_level = concourse, context = "transit" }
assert(up.speed == 0.65 and up.duty == "transit", "transit defaults")

-- A peak of 9,000 people an hour up (150 a minute) needs two units at 100 a minute, and a third
-- for the one assumed out: this pair is one short.
local t = reviera.escalator.traffic{ up = { up.escalator }, down = { down.escalator }, demand_up = 9000, demand_down = 3000 }
assert(t.up.units_needed == 3, "three units up")

-- Evacuating the platform up: it needs 300 people a minute and its stairs give 200. The up
-- escalator alone runs that way; with it out, nothing is left, so under NFPA 130 the stairs
-- must do it all. Running both up for evacuation, one counts (120 a minute), within half the
-- need (150): enough. Under the IBC escalators never count.
local both = { up.escalator, down.escalator }
local nfpa = reviera.escalator.traffic{ up = both, demand_up = 0, egress = { mode = "nfpa130", required = 300, stairs = 200 } }.egress
assert(nfpa.counted == 120 and nfpa.meets, "NFPA 130 counts one unit")
local ibc = reviera.escalator.traffic{ up = both, demand_up = 0, egress = { mode = "ibc", required = 300, stairs = 200 } }.egress
assert(ibc.counted == 0 and not ibc.meets, "the IBC counts none")
