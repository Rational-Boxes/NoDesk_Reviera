-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: A drive set out by chainage
-- description: An S-shaped drive up a slope as an alignment: three straights joined by a 25 m curve to the left and a 40 m curve to the right, chainage from 1+000, and a vertical profile climbing at 6 % then easing to 2 % over an 80 m crest curve. The sample reads it back by chainage and offset, finds the chainage of a point beside it, and lists its curves. It is an IFC 4.3 project, so the drive is saved as an IfcAlignment with its horizontal and vertical layouts.
--
-- Riviera sample model (test-data/samples): alignments (ARC-ALN-001), the shared foundation for
-- roads, drives and guideway devices. Drawn in plans as a dash-dot centreline with a tick every
-- 20 m of chainage and a longer one every 100 m.

riviera.code.set{ code = "ibc" }
local level = riviera.level.list()[1].id

-- Points of intersection: east, then north-east, then east again. A 25 m curve turns left at the
-- second, a 40 m curve right at the third. Chainage starts at 1000 m (1+000).
local drive = riviera.alignment.create{
  level = level, name = "Drive",
  points = { { 0, 0 }, { 60, 0 }, { 110, 50 }, { 170, 50 } },
  radii = { 25, 40 },
  start_station = 1000,
  profile = {
    { station = 1000, elevation = 0 },
    { station = 1120, elevation = 7.2, curve = 80 },
    { station = 1000 + 175, elevation = 8.3 },
  },
}
print(string.format("drive: %.2f m, chainage %.2f to %.2f", drive.length, drive.start_station, drive.end_station))

-- Its two curves, one each way.
local info = riviera.alignment.list()[1]
assert(#info.curves == 2 and info.curves[1].left and not info.curves[2].left, "a left then a right curve")

-- At chainage 1+040, still on the first straight (the curve starts 10.36 m before the corner,
-- at 1+049.64) and climbing at 6 %: (40, 0), 2.4 m up.
local at = riviera.alignment.point_at{ alignment_id = drive.alignment, station = 1040 }
assert(math.abs(at.point[1] - 40) < 1e-9 and math.abs(at.point[2]) < 1e-9 and math.abs(at.elevation - 2.4) < 1e-9 and math.abs(at.grade - 0.06) < 1e-9, "on the first straight")

-- A lamp post 3 m to the right of chainage 1+100, and back: its chainage and offset.
local post = riviera.alignment.point_at{ alignment_id = drive.alignment, station = 1100, offset = -3 }
local back = riviera.alignment.station_of{ alignment_id = drive.alignment, point = post.point }
assert(math.abs(back.station - 1100) < 1e-6 and math.abs(back.offset + 3) < 1e-6, "the post's chainage and offset")
print(string.format("post at (%.2f, %.2f), elevation %.2f m on the crest curve", post.point[1], post.point[2], post.elevation))
