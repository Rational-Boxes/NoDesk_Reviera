-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: An airport people mover
-- description: A dual-lane automated people mover 506 m long, its alignment bending round a 150 m curve, with an emergency walkway and each lane's dynamic envelope kept clear; three two-car stations with side platforms and screen doors stand on its straights. At a two-minute headway it carries 3,600 persons an hour each way, enough for a 3,000 peak; a 60 m curve or a station on the curve would be reported. An IFC 4.3 project.
--
-- Riviera sample model (test-data/samples): automated people movers (ARC-VT-025/026) on the
-- alignment foundation. Planning values from ACRP Report 37 (E-99); ASCE 21's own are not public.

riviera.code.set{ code = "ibc" }
local level = riviera.level.list()[1].id

-- Landside east 300 m, then north-east to the concourses round a 150 m curve; level throughout.
local line = riviera.alignment.create{
  level = level, name = "People mover",
  points = { { 0, 0 }, { 300, 0 }, { 450, 150 } },
  radii = { 150 },
}
local g = riviera.apm.guideway{ alignment_id = line.alignment }
print(string.format("guideway: %.1f m, deck %.2f m wide", g.length, g.deck_width))

-- Three stations on the straights: two-car trains, side platforms 30.4 m long, screen doors.
for _, s in ipairs({ { 30, "Landside" }, { 200, "Concourse A" }, { 480, "Concourse B" } }) do
  riviera.apm.station{ guideway_id = g.guideway, station = s[1], name = s[2] }
end

-- Two-car trains of 60 every two minutes: 3,600 an hour each way for a 3,000 peak. Nothing to
-- report, and the lanes' envelopes are clear.
local c = riviera.apm.check{ headway = 120, cars = 2, passengers = 60, demand = 3000 }
assert(c.capacity == 3600 and #c.findings == 0, "enough, and nothing to report")
assert(#riviera.clearance.check{} == 0, "the envelopes are clear")
