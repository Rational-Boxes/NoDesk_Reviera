-- SPDX-License-Identifier: MIT
-- Copyright (C) 2026 Reviera contributors

-- A sample connection design provider (STR-STL-009): simple bolt shear, bearing, bolt tension and
-- fillet weld checks, written as an example of the provider interface. It is not a complete
-- connection design: it ignores eccentricity, prying, block shear, plate yielding and the other
-- limit states a real design needs.
--
-- `check(input)` gets the connection's code (`aisc360` or `en1993`), bolt group, plies, welds and
-- design forces in SI units (N, m, Pa) and returns check items, each with its action, resistance,
-- ratio, pass or fail, and the parameter that would strengthen it (for Presize).
--
-- AISC 360-16 (LRFD): bolt shear φ Fnv Ab, φ 0.75 (J3.6); bearing and tear-out
-- φ min(2.4 d t Fu, 1.2 lc t Fu), φ 0.75 (J3.10); bolt tension φ 0.75 Fu Ab, φ 0.75 (J3.6, Fnt
-- 0.75 Fu); fillet welds φ 0.6 FEXX 0.707 w L, φ 0.75 (J2.4).
-- EN 1993-1-8: bolt shear αv fub As / γM2, αv 0.6, As about 0.78 A; bearing
-- k1 αb fu d t / γM2, k1 2.5, αb min(e1 / 3d0, fub / fu, 1); tension 0.9 fub As / γM2; fillet welds
-- fu / (√3 βw γM2) a L, βw 0.8, 0.85 or 0.9 by grade; γM2 1.25.

local function item(name, group, action, resistance, parameter, note)
  local ratio = resistance > 0 and action / resistance or math.huge
  return {
    name = name, group = group, action = action, resistance = resistance,
    ratio = ratio, ok = ratio <= 1.0, parameter = parameter, note = note,
  }
end

local function checks(input, f)
  local out = {}
  local en = input.code == "en1993"
  local g2 = 1.25
  local v = math.sqrt((f.v or 0) ^ 2 + (f.vh or 0) ^ 2)
  local n = f.n or 0
  local b = input.bolts
  if b and b.count > 0 then
    local per = v / b.count
    local as = 0.78 * b.area
    local shear = en and (0.6 * b.fub * as / g2) or (0.75 * b.fnv * b.area)
    out[#out + 1] = item("Bolt shear", f.group, per, shear * (b.shear_planes or 1), "bolt_diameter")
    for _, ply in ipairs(input.plies) do
      local d0 = b.hole_diameter
      local edge = b.edge or (2.0 * d0)
      local r
      if en then
        local ab = math.min(edge / (3 * d0), b.fub / ply.fu, 1.0)
        r = 2.5 * ab * ply.fu * b.diameter * ply.thickness / g2
      else
        local lc = math.max(edge - d0 / 2, 0)
        r = 0.75 * math.min(2.4 * b.diameter * ply.thickness * ply.fu, 1.2 * lc * ply.thickness * ply.fu)
      end
      -- A thin plate grows; on a thin member web, more bolts share the force (bigger ones would
      -- only shorten the edge distance tear-out works on).
      local grow = "plate_thickness"
      if ply.what ~= "plate" then
        grow = (input.params.rows and "rows") or (input.params.bolts and "bolts") or "bolt_diameter"
      end
      out[#out + 1] = item("Bearing on the " .. ply.what, f.group, per, r, grow)
    end
    if n > 0 then
      local tension = en and (0.9 * b.fub * as / g2) or (0.75 * 0.75 * b.fub * b.area)
      out[#out + 1] = item("Bolt tension", f.group, n / b.count, tension, "bolt_diameter")
    end
  end
  local welds = 0
  for _, w in ipairs(input.welds) do
    if en then
      local bw = w.fu < 400e6 and 0.8 or (w.fu < 470e6 and 0.85 or 0.9)
      welds = welds + w.fu / (math.sqrt(3) * bw * g2) * 0.707 * w.size * w.length
    else
      welds = welds + 0.75 * 0.6 * w.fexx * 0.707 * w.size * w.length
    end
  end
  if #input.welds > 0 then
    out[#out + 1] = item("Fillet welds", f.group, math.sqrt(v ^ 2 + n ^ 2), welds, "weld_size")
  end
  return out
end

return {
  title = "Sample connection checks",
  description = "A sample connection design provider: bolt shear, bearing, bolt tension and fillet welds to AISC 360 or EN 1993-1-8 (simplified).",
  capability = "connection_check",
  codes = { "aisc360", "en1993" },
  run = function()
    reviera.ui.message("This plugin is a connection design provider: use Structure › Steel › Check Connections.")
  end,
  check = function(input)
    -- The governing (largest) ratio of each check over the load groups.
    local worst = {}
    local order = {}
    for _, f in ipairs(input.forces) do
      for _, it in ipairs(checks(input, f)) do
        local w = worst[it.name]
        if not w then
          order[#order + 1] = it.name
        end
        if not w or it.ratio > w.ratio then
          worst[it.name] = it
        end
      end
    end
    local items = {}
    for _, name in ipairs(order) do
      items[#items + 1] = worst[name]
    end
    return {
      items = items,
      note = "Sample provider: simplified checks, not a complete connection design.",
    }
  end,
}
