case("index_table", function() local base = { greet = "hi" }; local t = setmetatable({}, { __index = base }); return t.greet, t.missing, rawget(t, "greet") end)
case("index_function", function() local t = setmetatable({}, { __index = function(_, k) return k .. "!" end }); return t.x, t[1] end)
case("index_chain", function() local a = { v = 1 }; local b = setmetatable({}, { __index = a }); local c = setmetatable({}, { __index = b }); return c.v end)
case("newindex_function", function() local log = {}; local t = setmetatable({}, { __newindex = function(t, k, v) log[#log + 1] = k; rawset(t, k, v) end }); t.a = 1; t.a = 2; t.b = 3; return log, t.a end)
case("newindex_table", function() local store = {}; local t = setmetatable({}, { __newindex = store }); t.x = 1; return rawget(t, "x"), store.x end)
case("newindex_error", function() local t = setmetatable({}, { __newindex = function(_, k) error("cannot assign to internal field '" .. k .. "'") end }); t._uri = 1 end)
case("call", function() local t = setmetatable({}, { __call = function(self, a, b) return a + b, self ~= nil end }); return t(1, 2) end)
case("call_varargs", function() local t = setmetatable({}, { __call = function(_, ...) return select("#", ...), ... end }); return t(), t(nil, nil) end)
case("call_chain", function() local inner = setmetatable({}, { __call = function() return "inner" end }); local outer = setmetatable({}, { __call = inner }); return pcall(outer) end)
case("tostring", function() local t = setmetatable({}, { __tostring = function() return "Record(x)" end }); return tostring(t), "v=" .. tostring(t) end)
case("tostring_nonstring", function() return tostring(setmetatable({}, { __tostring = function() return 42 end })) end)
case("name", function() local s = tostring(setmetatable({}, { __name = "MyType" })); return string.sub(s, 1, 8) end)
case("concat", function() local t = setmetatable({}, { __concat = function(a, b) return "cat" end }); return t .. "x", "x" .. t, 1 .. t end)
case("eq", function() local mt = { __eq = function() return true end }; local a, b = setmetatable({}, mt), setmetatable({}, mt); return a == b, a ~= b, a == 1, rawequal(a, b) end)
case("lt_le", function() local mt = { __lt = function(a, b) return a.v < b.v end, __le = function(a, b) return a.v <= b.v end }; local a, b = setmetatable({ v = 1 }, mt), setmetatable({ v = 2 }, mt); return a < b, a <= b, a > b, a >= b end)
case("le_without_le", function() local mt = { __lt = function(a, b) return a.v < b.v end }; local a, b = setmetatable({ v = 1 }, mt), setmetatable({ v = 2 }, mt); return a <= b end)
case("arith", function() local mt = {}; mt.__add = function(a, b) return "add" end; mt.__unm = function() return "unm" end; mt.__idiv = function() return "idiv" end; mt.__mod = function() return "mod" end; local t = setmetatable({}, mt); return t + 1, 1 + t, -t, t // 1, t % 1 end)
case("len", function() return #setmetatable({}, { __len = function() return 7 end }) end)
case("metatable_field", function() local t = setmetatable({}, { __metatable = "locked" }); return getmetatable(t), pcall(setmetatable, t, {}) end)
case("getmetatable_string", function() return getmetatable("").__index == string, ("x").upper == string.upper end)
case("index_nil_error", function() local t = nil; return t.x end)
case("index_nil_field_error", function() local t = {}; return t.a.b end)
case("call_nil_error", function() local t = {}; return t.nope() end)
case("call_nil_method_error", function() local t = {}; return t:nope() end)
case("index_number_error", function() local n = 5; return n.x end)
case("inheritance", function()
  local Base = {}; Base.__index = Base
  function Base.new(v) return setmetatable({ v = v }, Base) end
  function Base:get() return self.v end
  local Derived = setmetatable({}, { __index = Base }); Derived.__index = Derived
  function Derived.new(v) local o = Base.new(v); return setmetatable(o, Derived) end
  function Derived:double() return self:get() * 2 end
  return Derived.new(21):double()
end)
case("global_guard", function()
  local env = setmetatable({}, { __index = function(_, k) if k == "db" then error("the 'db' global was removed") end return nil end })
  return pcall(function() return env.db end), env.other
end)
return report()
