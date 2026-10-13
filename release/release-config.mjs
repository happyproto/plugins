// What the release workflows read from a plugin's `.releaserc.json`, so they
// and the tests share one reading of it.
//
//   node release/release-config.mjs wasm plugins/happyview/sql
//     prints the module `@semantic-release/github` uploads, relative to the
//     plugin's directory: ../../../target/wasm32-unknown-unknown/release/happyview_sql.wasm
//   node release/release-config.mjs version plugins/happyview/sql happyview-sql-v1.2.0
//     prints the version a release tag names: 1.2.0

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { argv, exit, stderr, stdout } from "node:process";

/** A plugin's `.releaserc.json`. */
export function readConfig(dir) {
  return JSON.parse(readFileSync(join(dir, ".releaserc.json"), "utf8"));
}

/** The configured plugins, each as `[name, options]`. */
function plugins(config) {
  return (config.plugins ?? []).map((p) => (Array.isArray(p) ? p : [p, {}]));
}

/** The one `.wasm` asset the GitHub release uploads. */
export function wasmPath(config) {
  const assets = plugins(config)
    .filter(([name]) => name === "@semantic-release/github")
    .flatMap(([, options]) => options.assets ?? [])
    .map((asset) => (typeof asset === "string" ? asset : asset.path))
    .filter((path) => path.endsWith(".wasm"));
  if (assets.length !== 1) {
    throw new Error(`expected one .wasm asset in the github plugin's assets, found ${assets.length}`);
  }
  return assets[0];
}

/** The version a release tag names, given the plugin's `tagFormat`. */
export function versionOfTag(config, tag) {
  const [prefix, suffix] = config.tagFormat.split("${version}");
  if (!tag.startsWith(prefix) || !tag.endsWith(suffix ?? "")) {
    throw new Error(`${tag} does not match the tag format ${config.tagFormat}`);
  }
  return tag.slice(prefix.length, tag.length - (suffix ?? "").length);
}

/** Whether the release exports its data for the workflow's later steps. */
export function exportsData(config) {
  return plugins(config).some(([name]) => name === "semantic-release-export-data");
}

if (import.meta.url === `file://${argv[1]}`) {
  const [command, dir, tag] = argv.slice(2);
  try {
    const config = readConfig(dir);
    if (command === "wasm") {
      stdout.write(`${wasmPath(config)}\n`);
    } else if (command === "version" && tag) {
      stdout.write(`${versionOfTag(config, tag)}\n`);
    } else {
      throw new Error("usage: release-config.mjs wasm DIR | version DIR TAG");
    }
  } catch (error) {
    stderr.write(`release-config: ${error.message}\n`);
    exit(1);
  }
}
