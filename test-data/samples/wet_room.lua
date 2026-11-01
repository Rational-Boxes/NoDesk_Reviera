-- SPDX-License-Identifier: GPL-3.0-or-later
-- Copyright (C) 2026 Reviera contributors

-- title: Family bathroom and wet room (England)
-- description: Two rooms of a house under England's Approved Documents: a family bathroom laid out from its template, and a wet room whose level-access shower falls 1:60 to a centre drain, its whole floor tanked, with the BS 7671 Section 701 electrical zones round the shower and a towel rail rated for zone 2.
--
-- Reviera sample model (test-data/samples): the wet-area tools (ARC-BATH-010, 011, 012) under
-- England's building code, which brings the `england_adm` pack (Approved Document M's spaces,
-- wet-room falls and tanking) and `uk_bs7671` (BS 7671 Section 701's zones). Each room is four
-- 200 mm walls; the comments give the rooms' inside sizes.

reviera.code.set{ code = "uk_adk", occupancy = "dwelling" }
local l1 = reviera.level.list()[1].id

-- Four walls round an inside `w` × `l` from (x, y), 2.4 m high; returns them south, east,
-- north, west.
local function room(x, y, w, l)
  local t = 0.1
  local x0, y0, x1, y1 = x - t, y - t, x + w + t, y + l + t
  local function wall(a, b)
    return reviera.wall.create{ curve = { line = { start = a, ["end"] = b } }, base_level = l1, top_constraint = { unconnected = 2.4 } }
  end
  return { wall({x0, y0}, {x1, y0}), wall({x1, y0}, {x1, y1}), wall({x1, y1}, {x0, y1}), wall({x0, y1}, {x0, y0}) }
end

-- 1. A family bathroom, 2.0 × 2.4 m: a 1700 × 700 bath across one end, the WC and a pedestal
--    basin along the wet wall (`bath.layout`). BS 7671's zones 0, 1 and 2 stand round the bath.
room(0, 0, 2.0, 2.4)
reviera.bath.layout{ template = "uk_family_bathroom", point = {1.0, 1.2}, level = l1 }

-- 2. A wet room, 2.6 × 2.4 m on a floor: a level-access shower area against the south wall
--    (a 60 × 30 in former), its floor falling 1:60 to a centre drain; Approved Document M's
--    1:40 at most is kept by sinking the drain no deeper than that over the shortest run
--    (`bath.shower_falls`). A level shower makes the room a wet room: its whole floor and the
--    shower's walls to 1.8 m are tanked (`bath.waterproofing`).
room(4, 0, 2.6, 2.4)
local c = { {4, 0}, {6.6, 0}, {6.6, 2.4}, {4, 2.4} }
local loop = {}
for i = 1, 4 do
  loop[i] = { line = { start = c[i], ["end"] = c[i % 4 + 1] } }
end
reviera.floor.create{ loops = { loop }, level_id = l1 }
local t = reviera.bath.load_components{ names = { "Roll-in shower, 60 × 30 in", "Heated towel rail, 500 × 800" } }
local shower = reviera.bath.place_component{ type_id = t["Roll-in shower, 60 × 30 in"], point = {5.3, 0.1}, level = l1 }
reviera.bath.shower_falls{ element_ids = { shower } }

-- A heated towel rail on the north wall, 2.4 m from the shower's head: outside its zones (a
-- shower with no tray has zone 1 within 1.2 m of its head), and rated IPX4 all the same, as
-- zones 1 and 2 would ask (`Pset_ElectricalDeviceCommon.IP_Code`). Moved within 1.2 m with a
-- lower rating, Check Bathrooms would report it.
local rail = reviera.bath.place_component{ type_id = t["Heated towel rail, 500 × 800"], point = {6.0, 2.3}, level = l1 }
reviera.pset.set{ element_id = rail, pset_name = "Pset_ElectricalDeviceCommon", property_name = "IP_Code", value = "IPX4" }
