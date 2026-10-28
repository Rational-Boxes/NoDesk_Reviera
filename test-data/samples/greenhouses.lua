-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Greenhouses
-- description: A Venlo glasshouse range on a knee wall, a three-span gutter-connected range turned to run north–south, and a small gable greenhouse, each one Glazed Enclosure of glazed roofs, glazed walls and gutters.
--
-- Reviera sample model (test-data/samples): the Glazed Enclosure tool (ARC-GLZ-001/002). Each
-- greenhouse is one element (`enclosure.create`) that lays out ordinary Reviera elements and
-- builds them again when a parameter changes:
-- - its roofs: a sloped-glazing footprint roof per span (Greenhouse Roof Glass, 1,125 mm panes);
-- - its walls: curtain walls of Greenhouse Wall Glass (800 mm panes), the end walls attached to
--   every span's roof so the glass fills the gables;
-- - a knee wall under the glass where one is set;
-- - gutters on the outer eaves and a valley gutter between each pair of roofs.
-- The sizes are the forms' provisional defaults where none is given (EXPERT_REVIEW E-44).

local l1 = reviera.level.list()[1].id

-- A Venlo range: 3 trusses of 8 m, each with two 4 m roofs at 22°, 24 m long in 4 m bays, the
-- gutters at 5.5 m on a 600 mm knee wall.
local venlo = reviera.enclosure.create{
  form = "venlo", level_id = l1, origin = {0, 0},
  span = "8 m", spans = 3, length = "24 m", bay = "4 m", eave = "5.5 m", pitch = "22°",
  knee = "600 mm",
}

-- A gutter-connected range of three 9.6 m gable spans at 5:12, its length turned to run
-- north from (60, 0): the spans step west from there.
local range = reviera.enclosure.create{
  form = "multispan", level_id = l1, origin = {60, 0}, rotation = "90°",
  span = "9.6 m", spans = 3, length = "16 m", bay = "4 m", eave = "4.5 m",
}

-- A hobby greenhouse: one 3.6 m gable span, 6 m long, eaves at 2.2 m, roof at 25°.
local hobby = reviera.enclosure.create{
  form = "gable", level_id = l1, origin = {0, 32},
  span = "3.6 m", length = "6 m", bay = "1.2 m", eave = "2.2 m", pitch = "25°",
}

for _, e in ipairs({ venlo, range, hobby }) do
  local info = reviera.enclosure.get{ enclosure_id = e }
  print(string.format("%s: %.0f m², ridge %.2f m, %d roofs, %d gutters",
    info.recipe.form, info.floor_area, info.ridge, #info.roofs, #info.gutters))
end
print("Greenhouses built")
return true
