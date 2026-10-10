// `node --test release/`: the path filter, on fixtures shaped like what
// `git log` and `git diff-tree` give semantic-release, and the wrapped steps
// on a plugin of the test's own.

import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import config, { onlyPathCommits, steps, touches } from "./package-paths.mjs";

const JAVASCRIPT = ["plugins/happyview/javascript", "plugins/happyview/quickjs"];
const TYPESCRIPT = ["plugins/happyview/typescript", "plugins/happyview/quickjs"];

/** A stretch of history, newest first, each commit with the files it changed. */
const HISTORY = [
  {
    hash: "a1",
    message: "fix: catch runaway recursion in the engine",
    files: ["plugins/happyview/quickjs/src/budget.rs"],
  },
  {
    hash: "b2",
    message: "feat: refuse export = in typescript",
    files: [
      "plugins/happyview/typescript/src/lib.rs",
      "plugins/happyview/typescript/README.md",
    ],
  },
  {
    hash: "c3",
    message: "docs: say what the javascript plugin prepares",
    files: ["plugins/happyview/javascript/README.md"],
  },
  {
    hash: "d4",
    message: "fix: name the library bridge in lua tracebacks",
    files: ["plugins/happyview/lua/src/lib.rs"],
  },
  {
    hash: "e5",
    message: "chore: bump the lockfile",
    files: ["Cargo.lock", "package-lock.json"],
  },
  {
    // A sibling whose name merely starts with an engine's.
    hash: "f6",
    message: "feat: add a quickjs-tools crate",
    files: ["plugins/happyview/quickjs-tools/src/lib.rs"],
  },
  {
    hash: "g7",
    message: "refactor: move the engine and both front ends",
    files: [
      "plugins/happyview/javascript/src/lib.rs",
      "plugins/happyview/quickjs/src/lib.rs",
      "plugins/happyview/typescript/src/lib.rs",
    ],
  },
];

const commits = HISTORY.map(({ hash, message }) => ({ hash, message, subject: message }));
const filesOf = (hash) => HISTORY.find((commit) => commit.hash === hash).files;
const hashes = (list) => list.map((commit) => commit.hash);

test("a path counts itself and everything beneath it, and nothing that only shares a prefix", () => {
  assert.equal(touches(["plugins/happyview/quickjs/src/lib.rs"], ["plugins/happyview/quickjs"]), true);
  assert.equal(touches(["plugins/happyview/quickjs"], ["plugins/happyview/quickjs"]), true);
  assert.equal(touches(["plugins/happyview/quickjs-tools/x"], ["plugins/happyview/quickjs"]), false);
  assert.equal(touches(["plugins/happyview/quick"], ["plugins/happyview/quickjs"]), false);
  assert.equal(touches([], ["plugins/happyview/quickjs"]), false);
});

test("an engine commit counts for both interpreters, and each one's own only for it", () => {
  assert.deepEqual(hashes(onlyPathCommits(commits, JAVASCRIPT, filesOf)), ["a1", "c3", "g7"]);
  assert.deepEqual(hashes(onlyPathCommits(commits, TYPESCRIPT, filesOf)), ["a1", "b2", "g7"]);
});

test("a plugin with only its own directory listed filters as semantic-release-monorepo does", () => {
  assert.deepEqual(hashes(onlyPathCommits(commits, ["plugins/happyview/lua"], filesOf)), ["d4"]);
});

/** A plugin module written to a directory of its own, loaded by path. */
function plugin(source) {
  const dir = mkdtempSync(join(tmpdir(), "package-paths-"));
  const path = join(dir, "plugin.mjs");
  writeFileSync(path, source);
  return path;
}

/** A plugin that records what each step was handed. */
function recordingPlugin() {
  return plugin(
    `export const seen = [];
     export async function analyzeCommits(config, context) {
       seen.push({ step: "analyzeCommits", config, commits: context.commits.map((c) => c.hash) });
       return "minor";
     }
     export async function generateNotes(config, context) {
       seen.push({ step: "generateNotes", version: context.nextRelease.version });
       return "notes";
     }`,
  );
}

function context(plugins) {
  const logged = [];
  return {
    commits,
    nextRelease: { version: "1.2.0", gitTag: "happyview-javascript-v1.2.0" },
    options: { plugins },
    logger: { log: (...args) => logged.push(args) },
  };
}

test("each wrapped step hands the configured plugin only the commits that count", async () => {
  const recording = recordingPlugin();
  const { seen } = await import(recording);
  const wrapped = steps(filesOf);
  const plugins = [
    [recording, { preset: "angular" }],
    plugin("export async function prepare() {}"),
  ];

  const results = await Promise.all(
    wrapped.analyzeCommits.map((slot) =>
      slot({ packagePaths: JAVASCRIPT }, context(plugins)),
    ),
  );
  // The first slot is the recording plugin's; the second is a plugin with no
  // such step, and the rest are past the end of the list.
  assert.equal(results[0], "minor");
  assert.ok(results.slice(1).every((result) => result === undefined));
  assert.deepEqual(seen[0].commits, ["a1", "c3", "g7"]);
  // The plugin's own options are laid over the release's, as semantic-release does.
  assert.equal(seen[0].config.preset, "angular");

  await wrapped.generateNotes[0]({ packagePaths: JAVASCRIPT }, context(plugins));
  assert.equal(seen[1].version, "happyview-javascript-v1.2.0");
});

test("a release that lists no paths fails saying what to add", async () => {
  await assert.rejects(
    steps(filesOf).analyzeCommits[0]({}, context([recordingPlugin()])),
    /"packagePaths" in its \.releaserc\.json/,
  );
});

test("the shareable config defines the four steps and nothing else", () => {
  assert.deepEqual(Object.keys(config).sort(), ["analyzeCommits", "fail", "generateNotes", "success"]);
  for (const slots of Object.values(config)) {
    assert.equal(slots.length, 10);
  }
});
