-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Annotated plan
-- description: A three-room bungalow documented with the Annotate tab: dimension strings, notes, door, window and room tags, Tag All, door and room schedules, a section and an elevation with their own dimensions, tags and notes, spot elevations and a spot coordinate, and a sheet.
--
-- Reviera sample model (test-data/samples): a 10 × 6 m bungalow (living room, bedroom, kitchen)
-- with three doors and five windows, documented in its Level 1 plan: aligned dimension strings
-- to wall faces and partitions, and to the openings' jambs (DOC-ANN); a title note and a note
-- with a leader; a door tag with a leader and a window tag (Tag by Category), a room tag, then
-- Tag All Not Tagged for the rest (DOC-ANN-010); door and room schedules (DOC-SCH); a section
-- through the bedroom and the south elevation, each with its own dimensions, a tag and a note;
-- spot elevations in the plan and the elevation, and a spot coordinate;
-- sheet A101 with the plan, section, elevation and both schedules placed (DOC-SHT).

local l1 = reviera.level.list()[1].id
reviera.view.create_plan{ level_ids = { l1 } }
local plan
for _, v in ipairs(reviera.view.list()) do
  if v.name == "Level 1" and v.kind ~= "3d" then plan = v.id end
end
local function note(x, y, text, size)
  return reviera.text.create{ view_id = plan, position = { x, y }, content = text, type_id = size }
end
local function wall(a, b)
  return reviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = 3.0 } }
end

-- The bungalow: 10 × 6 m on the walls' centrelines, clockwise (exteriors outside); a partition
-- at x = 6 m, and one at y = 3 m splitting the east side into bedroom and kitchen.
local c = { {0, 0}, {10, 0}, {10, 6}, {0, 6} }
local outer = {}
for i = 1, 4 do
  outer[i] = wall(c[i % 4 + 1], c[i])
end
local south, east, north, west = outer[1], outer[2], outer[3], outer[4]
local cross = wall({6, 0}, {6, 6})
local split = wall({6, 3}, {10, 3})

-- Openings at exact sizes, so the dimension strings can pick their jambs. Doors 900 wide,
-- windows 1200. The south wall runs from (10, 0) to (0, 0), the north from (0, 6) to (10, 6).
local front = reviera.door.place{ host_wall_id = south, point_along = 6.0, width = 0.9 }
local d_bed = reviera.door.place{ host_wall_id = cross, point_along = 1.5, width = 0.9 }
local d_kit = reviera.door.place{ host_wall_id = cross, point_along = 4.5, width = 0.9 }
local w_living_s = reviera.window.place{ host_wall_id = south, point_along = 8.0, width = 1.2 }
local w_bed_s = reviera.window.place{ host_wall_id = south, point_along = 2.0, width = 1.2 }
reviera.window.place{ host_wall_id = east, point_along = 4.5, width = 1.2 }
reviera.window.place{ host_wall_id = north, point_along = 3.0, width = 1.2 }
reviera.window.place{ host_wall_id = north, point_along = 8.0, width = 1.2 }

local living = reviera.room.place{ level_id = l1, point = { 3, 3 }, name = "Living", number = "1" }
reviera.room.place{ level_id = l1, point = { 8, 1.5 }, name = "Bedroom", number = "2" }
reviera.room.place{ level_id = l1, point = { 8, 4.5 }, name = "Kitchen", number = "3" }

-- Dimensions. Wall references follow the walls; jambs are fixed lines on the opening's edges.
local function face(w, sub, p) return { element = w, sub = sub, point = p } end
local function jamb(e, x) return { element = e, sub = "line", point = { x, 0 }, direction = { 0, 1 } } end
-- Overall, south side: west face, partition, east face.
reviera.dim.create_aligned{ view_id = plan, line_point = { 5, -2.0 }, refs = {
  face(west, "face:ext", { 0, 1 }), face(cross, "axis", { 6, 1 }), face(east, "face:ext", { 10, 1 }) } }
-- Openings in the south wall: the jambs of each window and the door, between the end faces.
reviera.dim.create_aligned{ view_id = plan, line_point = { 5, -1.2 }, refs = {
  face(west, "face:ext", { 0, 1 }),
  jamb(w_living_s, 1.4), jamb(w_living_s, 2.6), jamb(front, 3.55), jamb(front, 4.45),
  face(cross, "axis", { 6, 1 }),
  jamb(w_bed_s, 7.4), jamb(w_bed_s, 8.6),
  face(east, "face:ext", { 10, 1 }) } }
-- Overall, east side: south face, partition, north face.
reviera.dim.create_aligned{ view_id = plan, line_point = { 12.0, 3 }, refs = {
  face(south, "face:ext", { 9, 0 }), face(split, "axis", { 9, 3 }), face(north, "face:ext", { 9, 6 }) } }
-- Inside the living room: clear width between the wall faces.
reviera.dim.create_aligned{ view_id = plan, line_point = { 3, 5.0 }, refs = {
  face(west, "face:int", { 0, 5 }), face(cross, "face:ext", { 6, 5 }) } }

-- Notes: a title, and one with a leader to the partition.
note(0, -3.0, "GROUND FLOOR PLAN  1 : 100", "Text 5 mm")
local n = note(1.0, 8.2, "100 mm stud partitions,\nplasterboard both sides")
reviera.text.add_leader{ annotation_id = n, points = { { 5.0, 7.6 }, { 6.0, 5.6 } } }

-- Tags: the front door with a leader and the living window by Tag by Category, the living
-- room by Room Tag; Tag All Not Tagged does the other doors, windows and rooms.
reviera.tag.create{ view_id = plan, host_ref = front, position = { 4.0, 1.2 }, leader = true }
reviera.tag.create{ view_id = plan, host_ref = w_living_s, position = { 2.0, 0.8 } }
reviera.tag.create_room{ view_id = plan, room_id = living }
local rest = reviera.tag.tag_all{ view_id = plan, categories = { "cat.doors", "cat.windows", "cat.rooms" } }
assert(#rest == 8, "Tag All tagged " .. #rest .. " elements, not 8")

-- Schedules of the doors and rooms.
local doors = reviera.schedule.create{ category = "cat.doors" }
local rooms = reviera.schedule.create{ category = "cat.rooms" }

-- A section through the bedroom door, looking north, and the south elevation.
local sec = reviera.view.create_section{ start = { 11, 1.5 }, ["end"] = { -1, 1.5 }, far_offset = 8, name = "Section A" }
local elev = reviera.view.create_elevation{ point = { 5, -5 }, look = { 0, 1 }, width = 14, name = "South" }

-- Their annotation, in each view's own coordinates: x to the viewer's right from the view's left
-- end, and height (the elevation starts at x = -2 m, the section at x = -1 m). In the south
-- elevation, the wall's height above Level 1, the overall width between the end walls' outer
-- faces (seen edge on), a tag on the living window and a note on the facade; in Section A, the
-- partition's height, the living room's clear width where the section cuts it, a tag on the
-- bedroom door and a note. A 3D view floats them too when chosen (DOC-ANN-025).
local function at_level(x) return { element = l1, sub = "level", point = { x, 0 } } end
reviera.dim.create_aligned{ view_id = elev, line_point = { 0.8, 1.5 }, refs = { at_level(1), face(south, "top", { 1, 3 }) } }
reviera.dim.create_aligned{ view_id = elev, line_point = { 7, 4.0 }, refs = {
  face(west, "face:ext", { 2, 1 }), face(east, "face:ext", { 12, 1 }) } }
reviera.tag.create{ view_id = elev, host_ref = w_living_s, position = { 4.0, 2.6 } }
reviera.text.create{ view_id = elev, position = { 7.0, 2.8 }, content = "RENDERED BLOCKWORK" }
reviera.dim.create_aligned{ view_id = sec, line_point = { 0.3, 1.5 }, refs = { at_level(4), face(cross, "top", { 7, 3 }) } }
reviera.dim.create_aligned{ view_id = sec, line_point = { 4, 2.4 }, refs = {
  face(west, "face:int", { 1, 2 }), face(cross, "face:ext", { 7, 2 }) } }
reviera.tag.create{ view_id = sec, host_ref = d_bed, position = { 7.6, 2.4 } }
reviera.text.create{ view_id = sec, position = { 7.4, 3.6 }, content = "BEDROOM" }

-- Spot elevations and a spot coordinate (DOC-ANN-006/007), from the project base point: the
-- north wall's top in the plan; the north-east corner's coordinates; and, in the elevation, the
-- south wall's top and bottom by the east end (picked on the elevation's plane: the spot finds
-- the wall's face along the view, and its top and bottom straight up and down there).
reviera.spot.create_elevation{ view_id = plan, ref = north, point = { 1.5, 6, 3 }, leader_point = { 2.2, 6.9 } }
reviera.spot.create_coordinate{ view_id = plan, ref = east, point = { 10, 6, 3 }, leader_point = { 11.2, 7.4 } }
reviera.spot.create_elevation{ view_id = elev, ref = south, point = { 9.3, -5, 1.0 }, leader_point = { 12.6, 1.6 }, display = "top_and_bottom" }

-- Sheet A101 (A2): the plan, section and elevation at 1 : 100, and both schedules.
local sheet = reviera.sheet.create{ name = "Ground Floor", size = "A2" }
reviera.sheet.set_parameter{ sheet_id = sheet, parameters = { ["Drawn By"] = "Reviera", ["Issue Date"] = "2026-10-26" } }
reviera.sheet.place_view{ sheet_id = sheet, view_id = plan, position = { 130, 290 } }
reviera.sheet.place_view{ sheet_id = sheet, view_id = sec, position = { 400, 340 } }
reviera.sheet.place_view{ sheet_id = sheet, view_id = elev, position = { 400, 240 } }
reviera.sheet.place_schedule{ sheet_id = sheet, schedule_id = doors, position = { 40, 160 } }
reviera.sheet.place_schedule{ sheet_id = sheet, schedule_id = rooms, position = { 250, 160 } }

print("Annotated plan built")
return true
