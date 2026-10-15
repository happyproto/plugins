// internal.test: Vitest-style describe/it/expect and the trigger calls for a
// test file. The host's bindings arrive on import.meta, which no other
// module can read.
const host = import.meta.host;

// Kept as plain Error by name, so a failure's message is shown without a
// prefix; the class is what tells an assertion from anything else thrown.
class AssertionError extends Error {}

const root = { name: null, parent: null, before: [], after: [] };
let current = root;
let running = false;
const entries = [];

function fullName(block, name) {
  const parts = [name];
  for (let b = block; b !== null && b.name !== null; b = b.parent) parts.unshift(b.name);
  return parts.join(" > ");
}

function declaring(kind, name) {
  if (typeof name !== "string") throw new TypeError(`${kind} needs a name as its first argument`);
  if (running) throw new Error(`${kind} cannot be called inside a running test`);
}

function needsFunction(kind, fn) {
  if (typeof fn !== "function") throw new TypeError(`${kind} needs a function`);
}

export function describe(name, fn) {
  declaring("describe", name);
  needsFunction("describe", fn);
  const block = { name, parent: current, before: [], after: [] };
  const outer = current;
  current = block;
  try {
    const returned = fn();
    if (returned && typeof returned.then === "function") {
      throw new TypeError("describe's function must not be async: every test is collected before any runs");
    }
  } finally {
    current = outer;
  }
}

export function it(name, fn) {
  declaring("it", name);
  needsFunction("it", fn);
  entries.push({ name: fullName(current, name), fn, block: current, skip: false });
}

it.skip = function skip(name) {
  declaring("it.skip", name);
  entries.push({ name: fullName(current, name), block: current, skip: true });
};

export function beforeEach(fn) {
  needsFunction("beforeEach", fn);
  current.before.push(fn);
}

export function afterEach(fn) {
  needsFunction("afterEach", fn);
  current.after.push(fn);
}

// From the outermost block in, so an outer beforeEach runs first.
function hooks(block, field) {
  const blocks = [];
  for (let b = block; b !== null; b = b.parent) blocks.unshift(b);
  return blocks.flatMap((b) => b[field]);
}

async function runOne(entry) {
  let failed = false;
  let failure;
  for (const hook of hooks(entry.block, "before")) {
    try {
      await hook();
    } catch (e) {
      failed = true;
      failure = e;
      break;
    }
  }
  if (!failed) {
    try {
      await entry.fn();
    } catch (e) {
      failed = true;
      failure = e;
    }
  }
  // Innermost first, and always: a hook that releases what the test took
  // has to run however the test ended.
  for (const hook of hooks(entry.block, "after").reverse()) {
    try {
      await hook();
    } catch (e) {
      if (!failed) {
        failed = true;
        failure = e;
      }
    }
  }
  return { failed, failure };
}

async function run() {
  running = true;
  const counts = { passed: 0, failed: 0, skipped: 0 };
  for (const entry of entries) {
    if (entry.skip) {
      host.report({ name: entry.name, status: "skip", durationMs: 0 });
      counts.skipped += 1;
      continue;
    }
    const started = Date.now();
    const { failed, failure } = await runOne(entry);
    host.report({
      name: entry.name,
      status: failed ? "fail" : "pass",
      failure,
      durationMs: Date.now() - started,
    });
    if (failed) counts.failed += 1;
    else counts.passed += 1;
  }
  return counts;
}

host.register(run);
