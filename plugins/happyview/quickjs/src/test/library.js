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

function show(value) {
  if (value === undefined) return "undefined";
  if (typeof value === "function") return `[Function ${value.name || "anonymous"}]`;
  if (typeof value === "bigint") return `${value}n`;
  if (value instanceof Error) return `${value.name}: ${value.message}`;
  try {
    return JSON.stringify(value) ?? String(value);
  } catch {
    return String(value);
  }
}

function isPlain(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

// toEqual ignores a property whose value is undefined, as Vitest's does.
function equal(a, b) {
  if (Object.is(a, b)) return true;
  if (Array.isArray(a) || Array.isArray(b)) {
    return Array.isArray(a) && Array.isArray(b) && a.length === b.length && a.every((v, i) => equal(v, b[i]));
  }
  if (isPlain(a) && isPlain(b)) {
    const keys = (o) => Object.keys(o).filter((k) => o[k] !== undefined);
    const ka = keys(a);
    const kb = keys(b);
    return ka.length === kb.length && ka.every((k) => equal(a[k], b[k]));
  }
  return false;
}

function matches(actual, expected) {
  if (isPlain(expected)) {
    return isPlain(actual) && Object.keys(expected).every((k) => matches(actual[k], expected[k]));
  }
  if (Array.isArray(expected)) {
    return Array.isArray(actual) && actual.length === expected.length && expected.every((v, i) => matches(actual[i], v));
  }
  return Object.is(actual, expected);
}

function fail(message) {
  throw new AssertionError(message);
}

function matchers(actual, rejected) {
  return {
    toBe(expected) {
      if (!Object.is(actual, expected)) fail(`expected ${show(expected)}, got ${show(actual)}`);
    },
    toEqual(expected) {
      if (!equal(actual, expected)) fail(`expected ${show(expected)}, got ${show(actual)}`);
    },
    toBeTruthy() {
      if (!actual) fail(`expected a truthy value, got ${show(actual)}`);
    },
    toBeNull() {
      if (actual !== null) fail(`expected null, got ${show(actual)}`);
    },
    toContain(item) {
      const found = typeof actual === "string"
        ? typeof item === "string" && actual.includes(item)
        : Array.isArray(actual) && actual.some((v) => Object.is(v, item));
      if (!found) fail(`expected ${show(actual)} to contain ${show(item)}`);
    },
    toMatchObject(expected) {
      if (!matches(actual, expected)) fail(`expected ${show(actual)} to match ${show(expected)}`);
    },
    // Under rejects, the value is already what was thrown.
    toThrow(expected) {
      let threw = rejected;
      let thrown = actual;
      if (!rejected) {
        if (typeof actual !== "function") fail("toThrow needs a function to call");
        try {
          actual();
        } catch (e) {
          threw = true;
          thrown = e;
        }
      }
      if (!threw) fail("expected a throw, got none");
      if (expected === undefined) return;
      const message = thrown instanceof Error ? thrown.message : String(thrown);
      const ok = typeof expected === "string"
        ? message.includes(expected)
        : expected instanceof RegExp
          ? expected.test(message)
          : typeof expected === "function" && thrown instanceof expected;
      if (!ok) fail(`expected a throw matching ${show(expected)}, got ${show(message)}`);
    },
  };
}

const MATCHERS = ["toBe", "toEqual", "toBeTruthy", "toBeNull", "toContain", "toMatchObject", "toThrow"];

// Each matcher settles first. The test's own frame is on the stack only
// when the matcher is called, before any await, so the place a failure is
// reported at is taken then and given to the failure.
function settled(settle) {
  const out = {};
  for (const name of MATCHERS) {
    out[name] = (...args) => {
      const site = new Error();
      return settle()
        .then((m) => m[name](...args))
        .catch((e) => {
          if (e instanceof AssertionError) e.stack = site.stack;
          throw e;
        });
    };
  }
  return out;
}

export function expect(actual) {
  const now = matchers(actual, false);
  now.resolves = settled(async () => {
    let value;
    try {
      value = await actual;
    } catch (e) {
      fail(`expected the promise to resolve, it rejected with ${show(e)}`);
    }
    return matchers(value, false);
  });
  now.rejects = settled(async () => {
    let value;
    try {
      value = await actual;
    } catch (e) {
      return matchers(e, true);
    }
    fail(`expected the promise to reject, it resolved with ${show(value)}`);
  });
  return now;
}

// Each trigger is called before its promise is returned, so a refusal is
// raised while the test's frame is still on the stack and is placed there.
export const test = {
  query: async (nsid, params = {}, opts = {}) =>
    host.trigger({ kind: "query", target: nsid, input: params, as: opts.as }),
  procedure: async (nsid, input = {}, opts = {}) =>
    host.trigger({ kind: "procedure", target: nsid, input, as: opts.as }),
  recordCreate: async (collection, opts = {}) =>
    host.trigger({ kind: "record_create", target: collection, input: opts.record ?? {}, did: opts.did, rkey: opts.rkey, as: opts.as }),
  recordUpdate: async (collection, opts = {}) =>
    host.trigger({ kind: "record_update", target: collection, input: opts.record ?? {}, did: opts.did, rkey: opts.rkey, as: opts.as }),
  recordDelete: async (collection, opts = {}) =>
    host.trigger({ kind: "record_delete", target: collection, did: opts.did, rkey: opts.rkey, as: opts.as }),
  job: async (type, input = {}, opts = {}) =>
    host.trigger({ kind: "job", target: type, input, as: opts.as }),
  label: async (uri, label = {}) => host.trigger({ kind: "label", target: uri, input: label }),
  mock: (namespace, responses) => host.mock({ namespace, responses }),
  calls: (namespace) => host.calls({ namespace }),
  pdsWrites: () => host.pdsWrites(),
};

host.register(run);
