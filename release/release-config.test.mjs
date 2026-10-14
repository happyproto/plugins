// `node --test release/`: what the release workflows read from each plugin's
// `.releaserc.json`, and that every plugin is set up to be published.

import assert from "node:assert/strict";
import { readdirSync } from "node:fs";
import { join } from "node:path";
import { test } from "node:test";

import { exportsData, readConfig, versionOfTag, wasmPath } from "./release-config.mjs";

const ROOT = join(import.meta.dirname, "..", "plugins", "happyview");
const RELEASED = readdirSync(ROOT).filter((name) => {
  try {
    readConfig(join(ROOT, name));
    return true;
  } catch {
    return false; // a crate with no release of its own, e.g. quickjs
  }
});

test("every released plugin uploads exactly one module", () => {
  assert.ok(RELEASED.length > 0);
  for (const name of RELEASED) {
    assert.match(wasmPath(readConfig(join(ROOT, name))), /\.wasm$/, name);
  }
});

test("every released plugin exports its release data", () => {
  for (const name of RELEASED) {
    assert.ok(exportsData(readConfig(join(ROOT, name))), `${name} lacks semantic-release-export-data`);
  }
});

test("the version comes from the tag, not the plugin's name", () => {
  const config = { tagFormat: "happyview-sql-v${version}" };
  assert.equal(versionOfTag(config, "happyview-sql-v1.2.0"), "1.2.0");
  assert.equal(versionOfTag(config, "happyview-sql-v2.0.0-beta.1"), "2.0.0-beta.1");
  assert.throws(() => versionOfTag(config, "happyview-db-v1.2.0"), /does not match/);
});

test("a module path is read from string and object assets alike", () => {
  const config = (assets) => ({ plugins: [["@semantic-release/github", { assets }]] });
  assert.equal(wasmPath(config(["a/b.wasm", "manifest.json"])), "a/b.wasm");
  assert.equal(wasmPath(config([{ path: "a/b.wasm" }, { path: "manifest.json" }])), "a/b.wasm");
  assert.throws(() => wasmPath(config([{ path: "manifest.json" }])), /found 0/);
});
