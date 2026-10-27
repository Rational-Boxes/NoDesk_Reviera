-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Views and inspection
-- description: A two-storey house with a set of saved views: plans, ceiling plans, sections, elevations, a scope box and 3D views cut open by section boxes and slices.
--
-- Reviera sample model (test-data/samples), the View tab's showcase: a floor plan and a ceiling
-- plan of each level (DOC-VIEW-001/002), a long and a cross section, the four building
-- elevations (DOC-VIEW-003/004), a scope box with a copy of the Level 1 plan
-- assigned to it (ARC-SCOPE-001), and 3D views each saved with its own camera and inspection
-- state: a section box round the east room, a Selection Box round the upper storey (DOC-VIEW-008),
-- a horizontal and a vertical slice (DOC-VIEW-014), virtual volumes shown translucent
-- (ARC-MOD-031), an orthographic view and the default 3D view's Home (UI-NAV-001/002).
-- Temporary hide, isolate, ghost and translucency (DOC-GFX-008) are applied to four more 3D
-- views as the script runs; they are never saved, so the opened sample shows those views whole.

local l1 = reviera.level.list()[1].id
local l2 = reviera.level.create{ elevation = 3.0 }
local function wall(level, a, b, height)
  return reviera.wall.create{
    curve = { line = { start = a, ["end"] = b } },
    base_level = level, top_constraint = { unconnected = height or 3.0 },
  }
end
local function square(x0, y0, x1, y1)
  local p = { {x0, y0}, {x1, y0}, {x1, y1}, {x0, y1} }
  local out = {}
  for i = 1, 4 do
    out[i] = { line = { start = p[i], ["end"] = p[i % 4 + 1] } }
  end
  return out
end

-- ---- The house: 10 × 7 m on the walls' centrelines, two storeys ----
local W, D = 10.0, 7.0
local c = { {0, 0}, {W, 0}, {W, D}, {0, D} }
local ground, upper = {}, {}
for i = 1, 4 do
  -- Clockwise, so each wall's exterior faces out: [1] south, [2] east, [3] north, [4] west.
  ground[i] = wall(l1, c[i % 4 + 1], c[i])
  upper[i] = wall(l2, c[i % 4 + 1], c[i])
end
-- Partitions; the one downstairs stops under the Level 2 floor (300 mm deep).
local hall = wall(l1, {5, 0}, {5, D}, 2.7)
local split = wall(l2, {5, 0}, {5, D})
reviera.door.place{ host_wall_id = ground[1], point_along = 7.5 }
reviera.door.place{ host_wall_id = hall, point_along = 3.5 }
reviera.door.place{ host_wall_id = split, point_along = 3.5 }
for _, a in ipairs({ 2.5, 5.0 }) do
  reviera.window.place{ host_wall_id = ground[1], point_along = a }
end
for _, a in ipairs({ 2.5, 7.5 }) do
  reviera.window.place{ host_wall_id = upper[1], point_along = a }
  reviera.window.place{ host_wall_id = ground[3], point_along = a }
  reviera.window.place{ host_wall_id = upper[3], point_along = a }
end
for _, w in ipairs({ ground[2], ground[4], upper[2], upper[4] }) do
  reviera.window.place{ host_wall_id = w, point_along = 3.5 }
end

-- Floors inside the walls, rooms and their ceilings.
reviera.floor.create{ loops = { square(0.1, 0.1, W - 0.1, D - 0.1) }, level_id = l1 }
reviera.floor.create{ loops = { square(0.1, 0.1, W - 0.1, D - 0.1) }, level_id = l2 }
local rooms = {
  { l1, {2.5, 3.5}, "Living" }, { l1, {7.5, 3.5}, "Kitchen" },
  { l2, {2.5, 3.5}, "Bedroom 1" }, { l2, {7.5, 3.5}, "Bedroom 2" },
}
for _, r in ipairs(rooms) do
  reviera.room.place{ level_id = r[1], point = r[2], name = r[3] }
  reviera.ceiling.create_auto{ level_id = r[1], point = r[2] }
end

-- A hipped roof on the upper walls' exterior faces, with eaves.
local o = 0.1
local f = { {-o, -o}, {W + o, -o}, {W + o, D + o}, {-o, D + o} }
local curves = {}
for i = 1, 4 do
  curves[i] = { line = { start = f[i], ["end"] = f[i % 4 + 1] } }
end
local roof = reviera.roof.create_footprint{ curves = curves, level_id = l2, base_offset = 3.0 }
reviera.roof.set_eaves{ roof_id = roof, overhang = 0.6, fascia = true }

-- ---- Plans, ceiling plans, sections, elevations ----
reviera.view.create_plan{ level_ids = { l1, l2 } }
reviera.view.create_plan{ kind = "ceiling_plan", level_ids = { l1, l2 } }
reviera.view.create_section{ start = {-2, 3.0}, ["end"] = {12, 3.0}, name = "Long Section" }
reviera.view.create_section{ start = {7.5, 9}, ["end"] = {7.5, -2}, far_offset = 10, name = "Cross Section" }
-- The building elevations, 5 m outside the eaves (as View › Building Elevations places them).
local lo, hi, m = { -o - 0.6, -o - 0.6 }, { W + o + 0.6, D + o + 0.6 }, 5.0
local mid = { (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2 }
local wx, wy = hi[1] - lo[1] + 2 * m, hi[2] - lo[2] + 2 * m
for _, e in ipairs({
  { "North", { mid[1], hi[2] + m }, { 0, -1 }, wx }, { "South", { mid[1], lo[2] - m }, { 0, 1 }, wx },
  { "East", { hi[1] + m, mid[2] }, { -1, 0 }, wy }, { "West", { lo[1] - m, mid[2] }, { 1, 0 }, wy },
}) do
  reviera.view.create_elevation{ name = e[1], point = e[2], look = e[3], width = e[4] }
end

-- A scope box round the east half, from Level 1 up past the roof, and a copy of the Level 1
-- plan assigned to it.
local plan1
for _, v in ipairs(reviera.view.list{ kind = "view.plan" }) do
  if v.name == "Level 1" then plan1 = v.id end
end
local east = reviera.scopebox.create{
  name = "East Half", rectangle = { corner_a = {4.6, -1.0}, corner_b = {11.0, 8.0} },
  level_id = l1, height = 9.0,
}
local plan_east = reviera.view.duplicate{ view_id = plan1, name = "Level 1 - East Half" }
reviera.view.assign_scope_box{ view_ids = { plan_east }, scope_box_id = east }
-- A cropped plan (DOC-GFX-015): the west half, its crop region shown, the annotation crop on
-- (5 mm on paper round it), so what lies outside is not drawn.
local plan_crop = reviera.view.duplicate{ view_id = plan1, name = "Level 1 - Cropped" }
reviera.view.set_crop{ view_id = plan_crop, shape = { min = { -1.0, -1.0 }, max = { 5.0, 8.0 } }, visible = true }
reviera.view.set_annotation_crop{ view_id = plan_crop, enabled = true, offsets = { 5, 5, 5, 5 } }

-- ---- 3D views, each with its camera and inspection state ----
-- Cameras look at the house's middle from the south-east (iso) or the south (front).
local centre = { W / 2, D / 2, 3.0 }
local function camera(view, from, projection)
  reviera.view.set_camera{
    view_id = view, target = centre, projection = projection or "perspective",
    eye = { centre[1] + from[1], centre[2] + from[2], centre[3] + from[3] },
  }
end
local se, front = { 18, -18, 14 }, { 0, -26, 6 }
local function view3d(name, from, projection)
  local v = reviera.view.create_3d{ kind = "named", name = name }
  camera(v, from, projection)
  return v
end

-- The default 3D view, its Home the south-east iso it opens at.
local default3d = reviera.view.create_3d{}
camera(default3d, se)
reviera.view.set_home{ view_id = default3d }
-- The same in parallel projection.
view3d("Orthographic", se, "orthographic")
-- A section box round the east room on Level 1, cut at 2 m through the windows.
local box = view3d("Section Box - Kitchen", se)
reviera.view.set_section_box{ view_id = box, enabled = true, min = {4.9, -0.5, -0.4}, max = {10.5, 7.5, 2.0} }
-- Selection Box: fitted round the upper storey's walls (300 mm margin).
local fitted = view3d("Selection Box - Level 2", se)
local upper_ids = { split }
for i = 1, 4 do upper_ids[#upper_ids + 1] = upper[i] end
reviera.view.fit_section_box{ view_id = fitted, element_ids = upper_ids }
-- A horizontal slice at 1.5 m (the side below kept): a plan in 3D.
local hslice = view3d("Horizontal Slice", se)
reviera.view.set_slice_planes{ view_id = hslice, planes = { { origin = {0, 0, 1.5}, normal = {0, 0, -1} } } }
-- A vertical slice on y = 3.5 (the north side kept), seen from the south: a section in 3D.
local vslice = view3d("Vertical Slice", front)
reviera.view.set_slice_planes{ view_id = vslice, planes = { { origin = {0, 3.5, 0}, normal = {0, 1, 0} } } }
-- Virtual volumes (door swings) shown translucent.
local volumes = view3d("Virtual Volumes", se)
reviera.view.set_virtual_volumes{ view_id = volumes, mode = "translucent" }

-- Temporary visibility (not saved): the roof hidden by category, the south wall isolated with
-- its doors and windows and the rest ghosted, the walls translucent, the Level 1 partition
-- isolated.
local noroof = view3d("Hide Roof", se)
reviera.view.temp_hide{ view_id = noroof, element_ids = { roof }, by = "category" }
local ghost = view3d("Isolate South Wall", front)
reviera.view.temp_isolate{ view_id = ghost, element_ids = { ground[1] }, by = "subtree", ghost_others = true }
local glass = view3d("Translucent Walls", se)
reviera.view.temp_translucent{ view_id = glass, element_ids = { ground[1] }, by = "category" }
local only = view3d("Isolate Partition", se)
reviera.view.temp_isolate{ view_id = only, element_ids = { hall } }

print("Views and inspection built")
return true
