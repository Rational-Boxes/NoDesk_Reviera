-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Riviera contributors

-- title: Greek temple
-- description: A Parthenon-style Doric temple: 46 columns from the sample Component Library repeated round the stylobate by a path array, porches, the cella and a pedimented roof.
--
-- Riviera sample model (test-data/samples), a demonstration of components from a Component
-- Library (DM-LIB-002 … 004) and arrays along a path (ARC-MOD-033), at the Parthenon's size:
-- - the crepidoma: three marble steps up to a 30.88 × 69.5 m stylobate;
-- - the peristyle: 8 × 17 Doric columns round the stylobate, a column on every corner, made by
--   one path array of the library's "Parthenon Doric Column" (a public-domain scanned shaft with
--   a modelled capital, test-data/library);
-- - the cella: the naos (east) and the opisthodomos (west) with great doors, each fronted by a
--   porch of four columns (path arrays along a line);
-- - the entablature over the columns, and a gable roof at 13.5° whose pediments are walls
--   attached to it.
--
-- The column is placed from the library as a custom object type the first time and shared by
-- every column after: the saved IFC holds its mesh once.

local library = riviera.script_dir .. "/../library"
local column = "Classical/Columns/parthenon_column.obj"

local l1 = riviera.level.list()[1].id
local stylobate_level = riviera.level.create{ elevation = 1.5, name = "Stylobate" }
local cornice = riviera.level.create{ elevation = 15.0, name = "Cornice" }

local function rect(x0, y0, x1, y1)
  local p = { {x0, y0}, {x1, y0}, {x1, y1}, {x0, y1} }
  local out = {}
  for i = 1, 4 do
    out[i] = { line = { start = p[i], ["end"] = p[i % 4 + 1] } }
  end
  return out
end
local function type_named(name)
  for _, t in ipairs(riviera.type.list()) do
    if t.name == name then return t.id end
  end
end

-- ---- The crepidoma: three steps of 0.5 m, the top one the stylobate ----
local W, L = 30.88, 69.5
-- (The project's default floor type exists once a floor does: a first floor makes it, and gives
-- way to the steps.)
local first = riviera.floor.create{ loops = { rect(-1.5, -1.5, W + 1.5, L + 1.5) }, level_id = l1, offset = 0.5 }
local step = riviera.type.duplicate{ type_id = type_named("Generic Floor - 300mm"), new_name = "Marble Step 500" }
riviera.type.set_param{ type_id = step, param = "thickness", value = 0.5 }
riviera.element.delete{ element_ids = { first } }
riviera.floor.create{ loops = { rect(-1.5, -1.5, W + 1.5, L + 1.5) }, level_id = l1, offset = 0.5, type_id = step }
riviera.floor.create{ loops = { rect(-0.75, -0.75, W + 0.75, L + 0.75) }, level_id = l1, offset = 1.0, type_id = step }
local stylobate = riviera.floor.create{ loops = { rect(0, 0, W, L) }, level_id = l1, offset = 1.5, type_id = step }

-- ---- The peristyle: 8 columns across each front, 17 along each flank, corners shared ----
-- The column axes run 1.15 m inside the stylobate's edge.
local i = 1.15
riviera.array.along_path{
  name = "Peristyle",
  sources = { { library = library, item = column } },
  path = { points = { {i, i}, {W - i, i}, {W - i, L - i}, {i, L - i} }, closed = true },
  level = stylobate_level,
  distribution = { mode = "number", value = 8, counts = { 8, 17 } },
  corners = "every_corner",
  -- Every column turned the same way: square capitals stay square to the building, the corner
  -- ones too (facing outward would turn those 45°).
  orientation = "same",
}

-- ---- The cella: naos to the east, opisthodomos to the west, 1 m marble walls ----
local marble
local function wall(a, b)
  return riviera.wall.create{
    curve = { line = { start = a, ["end"] = b } }, base_level = stylobate_level,
    top_constraint = { unconnected = 10.17 }, type_id = marble,
  }
end
local x0, x1, y0, y1 = 7.5, W - 7.5, 9.5, 60.0
-- The first wall makes the default wall type; the marble type is made from it.
local east = wall({x1, y1}, {x0, y1})
marble = riviera.type.duplicate{ type_id = type_named("Generic - 200mm"), new_name = "Marble Wall 1000" }
riviera.type.set_param{ type_id = marble, param = "width", value = 1.0 }
riviera.element.set_type{ element_ids = { east }, type_id = marble }
wall({x1, y0}, {x1, y1})
local west = wall({x0, y0}, {x1, y0})
wall({x0, y1}, {x0, y0})
wall({x0, 40.0}, {x1, 40.0}) -- between the naos and the opisthodomos
-- Likewise a first door makes the default door type.
local trial = riviera.door.place{ host_wall_id = east, point_along = 2.0 }
local great_door = riviera.type.duplicate{ type_id = type_named("Single Door 900 x 2100"), new_name = "Bronze Doors 4900 x 9000" }
riviera.element.delete{ element_ids = { trial } }
riviera.type.set_param{ type_id = great_door, param = "width", value = 4.9 }
riviera.type.set_param{ type_id = great_door, param = "height", value = 9.0 }
riviera.door.place{ type_id = great_door, host_wall_id = east, point_along = (x1 - x0) / 2 }
riviera.door.place{ type_id = great_door, host_wall_id = west, point_along = (x1 - x0) / 2 }

-- ---- The porches: four columns before each end of the cella ----
for _, y in ipairs({ 62.0, 7.5 }) do
  riviera.array.along_path{
    name = y > 30 and "East Porch" or "West Porch",
    sources = { { library = library, item = column } },
    path = { points = { {x0, y}, {x1, y} } },
    level = stylobate_level,
    distribution = { mode = "number", value = 4 },
    orientation = "same",
  }
end

-- ---- The entablature over the peristyle: a 3.32 m ring from the columns' tops to the cornice ----
local entablature = riviera.type.duplicate{ type_id = type_named("Generic Floor - 300mm"), new_name = "Entablature 3320" }
riviera.type.set_param{ type_id = entablature, param = "thickness", value = 3.32 }
riviera.floor.create{
  loops = { rect(0, 0, W, L), rect(2.4, 2.4, W - 2.4, L - 2.4) },
  level_id = cornice, type_id = entablature,
}

-- ---- The roof: a gable over the whole temple at 13.5°, the pediments walls attached to it ----
local roof = riviera.roof.create_footprint{ curves = rect(0, 0, W, L), level_id = cornice }
riviera.roof.set_shape{ roof_id = roof, shape = "gable", overhang = 0.3 }
riviera.roof.set_slope{ roof_id = roof, slope = "13.5°" }
local function pediment(a, b)
  return riviera.wall.create{
    curve = { line = { start = a, ["end"] = b } }, base_level = cornice,
    top_constraint = { unconnected = 5.0 }, type_id = marble,
  }
end
pediment({W - 0.6, 0.6}, {0.6, 0.6})
pediment({0.6, L - 0.6}, {W - 0.6, L - 0.6})
riviera.roof.attach_walls{ roof_id = roof }

-- Views: a plan of each level and a 3D view.
riviera.view.create_plan{ level_ids = { l1 } }
riviera.view.create_3d{}
