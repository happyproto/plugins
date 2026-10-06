-- Legal in Lua 5.4, a compile error in Lua 5.5 (loop variables are const).
case("assign_loop_var", function() local out = {}; for i = 1, 3 do i = i * 2; out[#out + 1] = i end; return out end)
return report()
