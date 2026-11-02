// `node --test release/`: turning a GitHub release's notes into the
// CHANGELOG.md entry hpy publishes as the version's changelog document.

import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";

import { enabledFor, hasEntry, notesBody, withEntry } from "./changelog.mjs";

/** What semantic-release wrote for happyview-db 1.0.0-alpha.7. */
const NOTES = `# [happyview-db-v1.0.0-alpha.7](https://github.com/happyproto/plugins/compare/happyview-db-v1.0.0-alpha.6...happyview-db-v1.0.0-alpha.7) (2026-11-02)


### Bug Fixes

* pin the plugin sdk that carries the instruction count ([12f8cab](https://github.com/happyproto/plugins/commit/12f8cabff127473ada5526fbb95dcb11496c027d))
`;

const BODY = `### Bug Fixes

* pin the plugin sdk that carries the instruction count ([12f8cab](https://github.com/happyproto/plugins/commit/12f8cabff127473ada5526fbb95dcb11496c027d))`;

const HAND_WRITTEN = `## [1.0.0](https://github.com/happyproto/plugins/releases/tag/happyview-db-v1.0.0) (2026-10-07)

- The first release.
`;

/**
 * hpy's reading of a changelog (crates/hpy-plugin/src/changelog.rs): the
 * lines under the \`## \` heading naming exactly \`version\`, up to the next
 * \`## \` heading.
 */
function hpyEntry(changelog, version) {
  const lines = changelog.split("\n");
  const start = lines.findIndex((line) => line.startsWith("## ") && hasEntry(line, version));
  if (start === -1) return null;
  const end = lines.findIndex((line, i) => i > start && line.startsWith("## "));
  return lines.slice(start + 1, end === -1 ? undefined : end).join("\n").trim() || null;
}

test("the notes lose their tag heading and keep the rest", () => {
  assert.equal(notesBody(NOTES), BODY);
  assert.equal(notesBody("### Features\n\n* x"), "### Features\n\n* x");
  assert.equal(notesBody("# [t](u) (2026-11-02)\n\n"), "");
  assert.equal(notesBody(""), "");
});

test("the entry is the one hpy publishes for the version", () => {
  const changelog = withEntry(HAND_WRITTEN, "1.0.0-alpha.7", notesBody(NOTES));
  assert.equal(hpyEntry(changelog, "1.0.0-alpha.7"), BODY);
  assert.equal(hpyEntry(changelog, "1.0.0"), "- The first release.");
  assert.ok(changelog.startsWith("## 1.0.0-alpha.7\n\n"));
});

test("a version never matches a longer one", () => {
  assert.ok(!hasEntry(HAND_WRITTEN, "1.0"));
  assert.ok(!hasEntry("## 1.0.0-alpha.10\n", "1.0.0-alpha.1"));
  assert.ok(hasEntry(HAND_WRITTEN, "1.0.0"));
  assert.ok(hasEntry("## 2.0.0\n", "2.0.0"));
});

test("an existing entry or empty notes leave the changelog alone", () => {
  assert.equal(withEntry(HAND_WRITTEN, "1.0.0", "- again"), HAND_WRITTEN);
  assert.equal(withEntry(HAND_WRITTEN, "1.1.0", ""), HAND_WRITTEN);
  assert.equal(withEntry("", "1.1.0", "- x"), "## 1.1.0\n\n- x\n");
});

test("PUBLISH_CHANGELOGS names the plugins it is on for", () => {
  assert.ok(!enabledFor(undefined, "db"));
  assert.ok(!enabledFor("", "db"));
  assert.ok(enabledFor("all", "db"));
  assert.ok(enabledFor("db", "db"));
  assert.ok(enabledFor(" lua , db ", "db"));
  assert.ok(!enabledFor("db", "sql"));
  assert.ok(!enabledFor("dbx", "db"));
});

test("the script writes only for a plugin the variable names", () => {
  const dir = mkdtempSync(join(tmpdir(), "changelog-"));
  const notes = join(dir, "notes.md");
  const changelog = join(dir, "CHANGELOG.md");
  writeFileSync(notes, NOTES);
  writeFileSync(changelog, HAND_WRITTEN);
  const script = join(import.meta.dirname, "changelog.mjs");
  const run = (setting) =>
    execFileSync("node", [script, dir, "1.0.0-alpha.7", "db", notes], {
      env: { ...process.env, PUBLISH_CHANGELOGS: setting },
      encoding: "utf8",
    });

  assert.match(run(""), /doesn't name db/);
  assert.equal(readFileSync(changelog, "utf8"), HAND_WRITTEN);

  assert.match(run("db"), /wrote the 1.0.0-alpha.7 entry/);
  assert.equal(hpyEntry(readFileSync(changelog, "utf8"), "1.0.0-alpha.7"), BODY);

  assert.match(run("all"), /already has a 1.0.0-alpha.7 entry/);
});
