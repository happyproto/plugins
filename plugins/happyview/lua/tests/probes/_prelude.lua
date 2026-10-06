-- Shared by every probe. Deliberately uses only the base library so that a
-- candidate missing string/table functions still reports the cases it can run.
local function sorted_keys(t)
  local keys = {}
  for k in pairs(t) do keys[#keys + 1] = k end
  for i = 2, #keys do
    local v = keys[i]
    local j = i - 1
    while j >= 1 and tostring(keys[j]) > tostring(v) do
      keys[j + 1] = keys[j]
      j = j - 1
    end
    keys[j + 1] = v
  end
  return keys
end

local function ser(v, depth)
  depth = depth or 0
  local t = type(v)
  if t == "string" then return '"' .. v .. '"' end
  if t == "number" then
    local mt = math and math.type and math.type(v) or "?"
    return tostring(v) .. "<" .. mt .. ">"
  end
  if t ~= "table" then return tostring(v) end
  if depth > 4 then return "{...}" end
  local out = "{"
  local first = true
  for _, k in ipairs(sorted_keys(v)) do
    if not first then out = out .. "," end
    first = false
    out = out .. tostring(k) .. "=" .. ser(v[k], depth + 1)
  end
  return out .. "}"
end

local __lines = {}
local function collect(...)
  return select("#", ...), { ... }
end

local function case(name, fn)
  local n, res = collect(pcall(fn))
  if res[1] then
    local out = ""
    for i = 2, n do
      if i > 2 then out = out .. " | " end
      out = out .. ser(res[i])
    end
    __lines[#__lines + 1] = name .. " = " .. out
  else
    __lines[#__lines + 1] = name .. " ! " .. ser(res[2])
  end
end

local function report()
  local out = ""
  for i = 1, #__lines do out = out .. __lines[i] .. "\n" end
  return out
end
