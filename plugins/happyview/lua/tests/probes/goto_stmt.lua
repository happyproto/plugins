case("continue", function() local out = {}; for i = 1, 5 do if i % 2 == 0 then goto continue end; out[#out + 1] = i; ::continue:: end; return out end)
case("nested_break", function() local found; for i = 1, 3 do for j = 1, 3 do if i * j == 4 then found = i .. "x" .. j; goto done end end end ::done:: return found end)
case("backward", function() local n = 0; ::top:: n = n + 1; if n < 3 then goto top end; return n end)
return report()
