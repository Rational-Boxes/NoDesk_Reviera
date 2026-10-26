-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Editing tools
-- description: The Modify tab's tools side by side, each result labelled in the plan: align, offset, mirror, move, copy, rotate, linear, radial and path arrays, unarray, create component, trim/extend, split, pin, and parts of a layered wall.
--
-- Reviera sample model (test-data/samples): one cell per tool of Modify › Modify and Modify ›
-- Parts (ARC-MOD-*, ARC-PART-*), laid out in rows 12 m apart so no result touches another.
-- Row 1: align (three walls' exterior faces onto a grid), offset (a wall copied 2 m across),
-- mirror about a picked grid and about a drawn axis (doors turn with their walls). Row 2: move,
-- copy, rotate, a linear array of five columns. Row 3: a radial array of eight columns round a
-- circle, a path array of five columns along an arc (parametric: one array element), a path
-- array unarrayed into four independent objects, and a component made from an L of walls with
-- a door, placed again beside it. Row 4: trim/extend to corner, trim/extend to a grid, split
-- and split with gap, a pinned wall and one pinned then unpinned and moved. Row 5: a cavity
-- wall made into parts, its brick divided with 10 mm joints, its plaster divided and two pieces
-- merged, its insulation excluded. The component is written to modify_tools.resources/library.

local l1 = reviera.level.list()[1].id
local plan = reviera.view.create_plan{ level_ids = { l1 } }[1]
reviera.view.create_3d{}
local function label(x, y, text)
  reviera.text.create{ view_id = plan, position = { x, y }, content = text }
end
local function wall(a, b, type_id)
  return reviera.wall.create{
    type_id = type_id,
    curve = { line = { start = a, ["end"] = b } },
    base_level = l1, top_constraint = { unconnected = 3.0 },
  }
end
local function grid(a, b, name)
  return reviera.grid.create{ name = name, curve = { line = { start = a, ["end"] = b } } }
end
local function column(x, y)
  return reviera.column.place_architectural{ point = { x, y }, base_level = l1, top_constraint = { unconnected = 3.0 } }
end

-- ---- Row 1 (y = 0): align, offset, mirror ----
-- Align: three 2 m walls at different depths, their exterior faces (the +y side for walls
-- drawn toward +x) brought onto grid A at y = 3.
local ga = grid({ -0.5, 3 }, { 8.5, 3 }, "A")
for i, y in ipairs({ 0.5, 1.2, 1.9 }) do
  local x = (i - 1) * 2.8
  local w = wall({ x, y }, { x + 2, y })
  reviera.element.align{
    reference = { element = ga, sub = "axis", point = { x + 1, 3 } },
    targets = { { element = w, sub = "face:ext", point = { x + 1, y + 0.1 } } },
  }
end
label(0, -1.5, "Align: three walls' exterior faces onto grid A")

-- Offset: a wall copied parallel to itself, 2 m toward the north.
local off = wall({ 12, 0.5 }, { 18, 0.5 })
reviera.element.offset{ element_ids = { off }, distance = 2.0, side_point = { 15, 5 } }
label(12, -1.5, "Offset: wall copied 2 m north")

-- Mirror – Pick Axis: a wall with a door mirrored about grid 1 (x = 27).
local g1 = grid({ 27, -0.5 }, { 27, 4.5 }, "1")
local mp = wall({ 24.5, 0 }, { 24.5, 4 })
reviera.door.place{ host_wall_id = mp, point_along = 1.5 }
reviera.element.mirror{ element_ids = { mp }, axis = { element = g1 } }
label(24, -1.5, "Mirror – Pick Axis: wall and door about grid 1")

-- Mirror – Draw Axis: an L of walls with a window mirrored about a line drawn at y = 2.
local ma = wall({ 36, 0 }, { 41, 0 })
local mb = wall({ 41, 0 }, { 41, 1.2 })
reviera.window.place{ host_wall_id = ma, point_along = 2.5 }
reviera.element.mirror{ element_ids = { ma, mb }, axis = { start = { 35, 2 }, ["end"] = { 42, 2 } } }
label(36, -1.5, "Mirror – Draw Axis: L of walls about y = 2")

-- ---- Row 2 (y = 12): move, copy, rotate, linear array ----
-- Move: a column placed at (1, 13) and moved 4 m east.
local mv = column(1, 13)
reviera.element.move{ element_ids = { mv }, vector = { 4, 0, 0 } }
label(0, 10.5, "Move: column moved 4 m east from (1, 13)")

-- Copy: a wall with a window copied twice, 1.5 m and 3 m north.
local cp = wall({ 12, 12 }, { 17, 12 })
reviera.window.place{ host_wall_id = cp, point_along = 2.5 }
reviera.element.copy{ element_ids = { cp }, vectors = { { 0, 1.5, 0 }, { 0, 3, 0 } } }
label(12, 10.5, "Copy: wall and window, two copies north")

-- Rotate: a 5 m wall drawn along x, turned 30° about its middle.
local rt = wall({ 24, 12.5 }, { 29, 12.5 })
reviera.element.rotate{ element_ids = { rt }, center = { 26.5, 12.5 }, angle = "30°" }
label(24, 10, "Rotate: wall turned 30° about its middle")

-- Linear array: a column and four copies 1.5 m apart (five members).
local la = column(36, 13)
reviera.element.array_linear{ element_ids = { la }, vector = { 1.5, 0, 0 }, count = 5 }
label(36, 10.5, "Array: five columns at 1.5 m")

-- ---- Row 3 (y = 24): radial array, path arrays, unarray, component ----
-- Radial array: eight columns spaced round a full turn of radius 3 m about (4, 28).
local ra = column(7, 28)
reviera.element.array_radial{ element_ids = { ra }, center = { 4, 28 }, angle = "360°", count = 8, move_to = "last" }
label(0, 23.5, "Radial Array: eight columns round a 3 m circle")

-- Array along Path: five columns (the library's Doric column at 30 %) along an arc; one
-- parametric array element, its members listed by array.members.
local library = reviera.script_dir .. "/../library"
local arc = reviera.array.along_path{
  name = "Arc colonnade",
  sources = { { library = library, item = "Classical/Columns/parthenon_column.obj" } },
  path = { points = { { 12, 25 }, { 16, 29 }, { 20, 25 } }, shape = "arc" },
  level = l1,
  distribution = { mode = "number", value = 5 },
  orientation = "same",
}
for _, t in ipairs(reviera.mesh_object.types()) do
  reviera.mesh_object.set_type{ type_id = t.id, scale = 0.3 }
end
label(12, 23.5, "Array along Path: five columns on an arc")

-- Unarray: a straight path array of four, turned into four independent objects.
local un = reviera.array.along_path{
  name = "Unarrayed row",
  sources = { { library = library, item = "Classical/Columns/parthenon_column.obj" } },
  path = { points = { { 24, 26 }, { 30, 26 } } },
  level = l1,
  distribution = { mode = "number", value = 4 },
  orientation = "same",
}
reviera.array.unarray{ array_id = un }
label(24, 23.5, "Unarray: path array of four made independent")

-- Create Component: an L of walls with a door written as a component, then placed 4 m north.
local ca = wall({ 36, 24 }, { 40, 24 })
local cb = wall({ 40, 24 }, { 40, 26 })
reviera.door.place{ host_wall_id = ca, point_along = 2 }
-- Written to the sample's own folder (a Component Library made there), by path so a rebuild
-- overwrites it.
local lib = reviera.script_dir .. "/modify_tools.resources/library"
reviera.library.create{ path = lib, name = "Editing tools sample" }
local bay = reviera.component.create{
  element_ids = { ca, cb }, name = "Corner Bay", path = lib .. "/Corner Bay.ifc", base_point = { 36, 24 },
}
reviera.component.place_fragment{ path = bay.path, point = { 36, 28 }, level = l1 }
label(36, 22.5, "Create Component: L with door, placed again north")

-- ---- Row 4 (y = 36): trim/extend, split, pin ----
-- Trim/Extend to Corner: one wall overshoots (trimmed), the other falls short (extended).
local ta = wall({ 0, 36 }, { 6, 36 })
local tb = wall({ 5, 37 }, { 5, 40 })
reviera.element.trim_corner{
  a = { element_id = ta, keep_point = { 2, 36 } },
  b = { element_id = tb, keep_point = { 5, 39 } },
}
label(0, 34.5, "Trim/Extend to Corner: L at (5, 36)")

-- Trim/Extend: grid 2 (x = 16) the boundary; one wall extended to it, one trimmed back.
local g2 = grid({ 16, 35 }, { 16, 41 }, "2")
local te = wall({ 12, 37 }, { 14, 37 })
local tt = wall({ 12, 39 }, { 19, 39 })
reviera.element.trim_extend{ boundary = g2, target = { element_id = te, keep_point = { 13, 37 } } }
reviera.element.trim_extend{ boundary = g2, target = { element_id = tt, keep_point = { 13, 39 } } }
label(12, 34.5, "Trim/Extend: to grid 2, extended and trimmed")

-- Split: a 6 m wall split in two at its middle; Split with Gap: the same with a 100 mm gap.
local sp = wall({ 24, 36 }, { 30, 36 })
reviera.element.split{ element_id = sp, point = { 27, 36 } }
local sg = wall({ 24, 38.5 }, { 30, 38.5 })
reviera.element.split_with_gap{ element_id = sg, point = { 27, 38.5 }, gap = 0.1 }
label(24, 34.5, "Split (y = 36) and Split with Gap, 100 mm (y = 38.5)")

-- Pin: one wall pinned (moves and deletes refused); another pinned, unpinned and moved 1 m north.
local pn = wall({ 36, 36 }, { 41, 36 })
reviera.element.pin{ element_ids = { pn } }
local up = wall({ 36, 38 }, { 41, 38 })
reviera.element.pin{ element_ids = { up } }
reviera.element.unpin{ element_ids = { up } }
reviera.element.move{ element_ids = { up }, vector = { 0, 1, 0 } }
label(36, 34.5, "Pin (y = 36); unpinned and moved (y = 39)")

-- ---- Row 5 (y = 48): parts of a layered wall ----
-- A cavity wall type: brick 100, insulation 50, block 100, plaster 15 (outside to inside).
local generic
for _, t in ipairs(reviera.type.list()) do
  if t.name == "Generic - 200mm" then generic = t.id end
end
local cavity = reviera.type.duplicate{ type_id = generic, new_name = "Cavity Wall 265" }
reviera.compound.set_layers{ type_id = cavity, layers = {
  { ["function"] = "finish1", material = "Brick", thickness = 0.1 },
  { ["function"] = "thermal_air", material = "Insulation", thickness = 0.05 },
  { ["function"] = "structure", material = "Block", thickness = 0.1 },
  { ["function"] = "finish2", material = "Plaster", thickness = 0.015 },
} }
local pw = wall({ 0, 48 }, { 10, 48 }, cavity)
-- One part per layer: brick, insulation, block, plaster.
local parts = reviera.parts.create{ host_ids = { pw } }
-- The brick divided by three lines across its face into four panels with 10 mm joints.
local across = function(xs)
  local lines = {}
  for _, x in ipairs(xs) do
    lines[#lines + 1] = { start = { x, 48, 0 }, ["end"] = { x, 48, 3 } }
  end
  return lines
end
local bricks = reviera.parts.divide{ part_ids = { parts[1] }, lines = across({ 2.5, 5, 7.5 }), gap = 0.01 }.parts
-- The plaster divided into three; the first two merged back into one.
local plaster = reviera.parts.divide{ part_ids = { parts[4] }, lines = across({ 3, 6 }) }.parts
local merged = reviera.parts.merge{ part_ids = { plaster[1], plaster[2] } }
-- The insulation excluded: kept, but out of views and the IFC export.
reviera.parts.exclude{ part_ids = { parts[2] } }
label(0, 46.5, "Parts: brick in four with 10 mm joints, plaster divided and merged, insulation excluded")

print("Editing tools built")
return true
