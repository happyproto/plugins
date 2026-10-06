-- Lua 5.4 local attributes.
case("gc_close", function() local closed = false; do local x <close> = setmetatable({}, { __close = function() closed = true end }) end; return closed end)
case("const", function() local x <const> = 5; return x end)
return report()
