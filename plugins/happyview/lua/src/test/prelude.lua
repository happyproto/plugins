-- internal.test for a test file: busted-style globals, the runner, the
-- assertions and the trigger calls. Loaded as a chunk of its own with the
-- host's bindings as its argument; only the globals it sets and the module
-- it returns reach the test file.
local host = ...

local Failure = {}
Failure.__tostring = function(failure)
  return failure.message
end

local root = { name = nil, parent = nil, before = {}, after = {} }
local current = root
local entries = {}
local running = false

-- Raise a failure placed at the test's own line rather than at this file's.
local function fail(message)
  local file, line = host.location()
  if not running and file and line then
    -- Outside a test nothing catches the table, so the run's own error would
    -- carry only its rendered text; a positioned string keeps the place.
    error(file .. ":" .. line .. ": " .. message, 0)
  end
  error(setmetatable({ message = message, file = file, line = line }, Failure), 0)
end

-- The xpcall handler: runs where the error was raised, so an error with no
-- position of its own can still be placed by the frames still on the stack.
local function capture(err)
  if getmetatable(err) == Failure then
    return err
  end
  local file, line, message = host.split(err)
  if not file then
    file, line = host.location()
  end
  return setmetatable({ message = message, file = file, line = line }, Failure)
end

local function full_name(block, name)
  local parts = { name }
  local b = block
  while b and b.name do
    table.insert(parts, 1, b.name)
    b = b.parent
  end
  return table.concat(parts, " > ")
end

local function declaring(kind, name)
  if type(name) ~= "string" then
    error(kind .. " needs a name as its first argument", 3)
  end
  if running then
    error(kind .. " cannot be called inside a running test", 3)
  end
end

local function needs_function(kind, fn)
  if type(fn) ~= "function" then
    error(kind .. " needs a function", 3)
  end
end

function describe(name, fn)
  declaring("describe", name)
  needs_function("describe", fn)
  local block = { name = name, parent = current, before = {}, after = {} }
  local outer = current
  current = block
  local ok, err = pcall(fn)
  current = outer
  if not ok then
    error(err, 0)
  end
end

function it(name, fn)
  declaring("it", name)
  needs_function("it", fn)
  entries[#entries + 1] = { name = full_name(current, name), fn = fn, block = current }
end

function pending(name)
  declaring("pending", name)
  entries[#entries + 1] = { name = full_name(current, name), block = current, pending = true }
end

function before_each(fn)
  if running then
    error("before_each cannot be called inside a running test", 2)
  end
  needs_function("before_each", fn)
  current.before[#current.before + 1] = fn
end

function after_each(fn)
  if running then
    error("after_each cannot be called inside a running test", 2)
  end
  needs_function("after_each", fn)
  current.after[#current.after + 1] = fn
end

-- Hooks from the outermost block in, so an outer before_each runs first.
local function hooks(block, field)
  local blocks = {}
  local b = block
  while b do
    table.insert(blocks, 1, b)
    b = b.parent
  end
  local found = {}
  for _, each in ipairs(blocks) do
    for _, fn in ipairs(each[field]) do
      found[#found + 1] = fn
    end
  end
  return found
end

local function run_one(entry)
  local failure
  for _, hook in ipairs(hooks(entry.block, "before")) do
    local ok, err = xpcall(hook, capture)
    if not ok then
      failure = err
      break
    end
  end
  if not failure then
    local ok, err = xpcall(entry.fn, capture)
    if not ok then
      failure = err
    end
  end
  -- Innermost first, and always: a hook that releases what the test took
  -- has to run however the test ended.
  local after = hooks(entry.block, "after")
  for i = #after, 1, -1 do
    local ok, err = xpcall(after[i], capture)
    if not ok and not failure then
      failure = err
    end
  end
  return failure
end

local function run()
  running = true
  local counts = { passed = 0, failed = 0, skipped = 0 }
  for _, entry in ipairs(entries) do
    if entry.pending then
      host.report({ name = entry.name, status = "skip", duration_ms = 0 })
      counts.skipped = counts.skipped + 1
    else
      local started = host.now()
      local failure = run_one(entry)
      local report = { name = entry.name, status = "pass", duration_ms = host.now() - started }
      if failure then
        report.status = "fail"
        report.message = failure.message
        report.file = failure.file
        report.line = failure.line
        counts.failed = counts.failed + 1
      else
        counts.passed = counts.passed + 1
      end
      host.report(report)
    end
  end
  return counts
end

local function show(value, depth)
  depth = depth or 0
  local kind = type(value)
  if kind == "string" then
    return string.format("%q", value)
  end
  if kind ~= "table" then
    return tostring(value)
  end
  if depth >= 3 then
    return "{...}"
  end
  local parts = {}
  local n = #value
  for i = 1, n do
    parts[#parts + 1] = show(value[i], depth + 1)
  end
  local keys = {}
  for k in pairs(value) do
    if not (math.type(k) == "integer" and k >= 1 and k <= n) then
      keys[#keys + 1] = k
    end
  end
  table.sort(keys, function(a, b)
    return tostring(a) < tostring(b)
  end)
  for _, k in ipairs(keys) do
    parts[#parts + 1] = tostring(k) .. " = " .. show(value[k], depth + 1)
  end
  if #parts == 0 then
    return "{}"
  end
  return "{ " .. table.concat(parts, ", ") .. " }"
end

local function same(a, b, seen)
  if a == b then
    return true
  end
  if type(a) ~= "table" or type(b) ~= "table" then
    return false
  end
  seen = seen or {}
  if seen[a] == b then
    return true
  end
  seen[a] = b
  for k, v in pairs(a) do
    if not same(v, b[k], seen) then
      return false
    end
  end
  for k in pairs(b) do
    if a[k] == nil then
      return false
    end
  end
  return true
end

local function differ(expected, actual)
  fail("expected " .. show(expected) .. ", got " .. show(actual))
end

local checks = { are = {} }

function checks.are.equal(expected, actual)
  if expected ~= actual then
    differ(expected, actual)
  end
end

function checks.are.same(expected, actual)
  if not same(expected, actual) then
    differ(expected, actual)
  end
end

function checks.is_true(value)
  if value ~= true then
    differ(true, value)
  end
end

function checks.is_false(value)
  if value ~= false then
    differ(false, value)
  end
end

function checks.is_nil(value)
  if value ~= nil then
    differ(nil, value)
  end
end

function checks.has_error(fn, expected)
  if type(fn) ~= "function" then
    fail("has_error needs a function to call")
  end
  local ok, err = pcall(fn)
  if ok then
    fail("expected an error, got none")
  end
  if expected == nil then
    return
  end
  local message
  if getmetatable(err) == Failure then
    message = err.message
  else
    local _, _, text = host.split(err)
    message = text
  end
  if message ~= expected then
    fail("expected the error " .. show(expected) .. ", got " .. show(message))
  end
end

-- Callable like Lua's own assert, so a test file that uses it plainly still
-- reads as it would anywhere else.
assert = setmetatable(checks, {
  __call = function(_, value, message, ...)
    if not value then
      fail(message ~= nil and tostring(message) or "assertion failed!")
    end
    return value, message, ...
  end,
})

local function options(opts)
  return opts or {}
end

local test = {}

function test.query(nsid, params, opts)
  return host.trigger({ kind = "query", target = nsid, input = params or {}, as = options(opts).as })
end

function test.procedure(nsid, input, opts)
  return host.trigger({ kind = "procedure", target = nsid, input = input or {}, as = options(opts).as })
end

local function record(kind, collection, opts, with_record)
  opts = options(opts)
  return host.trigger({
    kind = kind,
    target = collection,
    input = with_record and (opts.record or {}) or nil,
    did = opts.did,
    rkey = opts.rkey,
    as = opts.as,
  })
end

function test.record_create(collection, opts)
  return record("record_create", collection, opts, true)
end

function test.record_update(collection, opts)
  return record("record_update", collection, opts, true)
end

function test.record_delete(collection, opts)
  return record("record_delete", collection, opts, false)
end

function test.job(job_type, input, opts)
  return host.trigger({ kind = "job", target = job_type, input = input or {}, as = options(opts).as })
end

function test.label(uri, label)
  return host.trigger({ kind = "label", target = uri, input = label or {} })
end

function test.mock(namespace, responses)
  host.mock(namespace, responses)
end

function test.calls(namespace)
  return host.calls(namespace)
end

function test.pds_writes()
  return host.pds_writes()
end

return run, test
