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
  error(setmetatable({ message = message, file = file, line = line }, Failure), 0)
end

-- The xpcall handler: runs where the error was raised, so an error with no
-- position of its own can still be placed by the frames still on the stack.
local function capture(err)
  if getmetatable(err) == Failure then
    return err
  end
  local file, line, message = host.split(tostring(err))
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
  needs_function("before_each", fn)
  current.before[#current.before + 1] = fn
end

function after_each(fn)
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

local test = {}

return run, test
