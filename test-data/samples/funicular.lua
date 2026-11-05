-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: A funicular up a hillside
-- description: Two counterbalanced cars on a metre-gauge track with an Abt passing loop half way, running along an alignment that bends 300 m to the left round its corner and steepens from 20 % to 43.75 % over a 100 m vertical curve, rising 100 m. Its trip and its capacity each way are reported, and checked against 600 persons an hour. An IFC 4.3 project, so the track's alignment is saved as an IfcAlignment.
--
-- Riviera sample model (test-data/samples): funiculars (ARC-VT-024) on the alignment foundation
-- (ARC-ALN-001/002). Layouts and reference lines: research/16 Q4 C3 (E-98).

riviera.code.set{ code = "ibc" }
local level = riviera.level.list()[1].id

-- The line: 150 m east, then up the valley side to the north-east, a 300 m curve between; its
-- profile climbs at 20 %, then steepens over a 100 m vertical curve to 43.75 %.
local line = riviera.alignment.create{
  level = level, name = "Funicular line",
  points = { { 0, 0 }, { 150, 0 }, { 300, 60 } },
  radii = { 300 },
  profile = {
    { station = 0, elevation = 0 },
    { station = 150, elevation = 30, curve = 100 },
    { station = 310, elevation = 100 },
  },
}

-- Two cars of 60 on a metre gauge, an Abt loop half way, 5 m/s, a minute at each station, and a
-- walkway beside the track.
local fun = riviera.funicular.create{ alignment_id = line.alignment, gauge = 1.0, walkway = true }
print(string.format("funicular: %.1f m of track rising %.1f m, steepest %.1f %%, %.0f s a trip, %.0f persons an hour each way", fun.length, fun.rise, fun.max_grade, fun.trip, fun.capacity))
assert(fun.passing_loop ~= nil and math.abs(fun.max_grade - 43.75) < 0.01, "an Abt loop, steepest 43.75 %")
assert(#riviera.funicular.check{ demand = 600 } == 0, "it carries 600 an hour")
