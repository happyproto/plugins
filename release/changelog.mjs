// A released version's changelog entry, written into the workspace's
// CHANGELOG.md (never committed) so hpy publishes it as the version's
// changelog document. hpy reads the `## <version>` entry of the file that
// `[release] changelog` names; semantic-release writes the notes only to the
// GitHub release, under a `# [<tag>](…) (date)` heading hpy doesn't match.
//
//   node release/changelog.mjs DIR VERSION PLUGIN NOTES_FILE
//     with PUBLISH_CHANGELOGS set to `all` or a comma-separated list of
//     plugin names (e.g. `db,lua`), writes NOTES_FILE's body as DIR's
//     CHANGELOG.md entry for VERSION. For any other plugin, or with the
//     variable unset, it only prints the entry it would write, so the
//     release publishes no changelog document, as before.

import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { argv, env, exit, stderr, stdout } from "node:process";

/**
 * The release notes without their title, the level-1 or level-2 heading
 * semantic-release opens them with (`#` for a minor or major release, `##`
 * for a patch), which names the tag and the date rather than the version.
 * Section headings (`###`) stay. `""` when nothing is under the title.
 */
export function notesBody(notes) {
  const lines = notes.replace(/\r\n/g, "\n").split("\n");
  const first = lines.findIndex((line) => line.trim() !== "");
  if (first !== -1 && /^#{1,2} /.test(lines[first])) {
    lines.splice(0, first + 1);
  }
  return lines.join("\n").trim();
}

/**
 * Whether `changelog` already has an entry for `version`, under either
 * heading shape hpy reads: `## <version>` or `## [<version>](…)`.
 */
export function hasEntry(changelog, version) {
  return changelog.split("\n").some((line) => {
    const heading = line.startsWith("## ") ? line.slice(3).trimStart() : null;
    if (heading === null) return false;
    const rest = heading.startsWith("[") ? heading.slice(1) : heading;
    if (!rest.startsWith(version)) return false;
    const after = rest.slice(version.length);
    return after === "" || after.startsWith("]") || /^\s/.test(after);
  });
}

/**
 * `changelog` with a `## <version>` entry holding `body` at the top, or
 * unchanged when it has one already or `body` is empty.
 */
export function withEntry(changelog, version, body) {
  if (body === "" || hasEntry(changelog, version)) return changelog;
  const rest = changelog.trimStart();
  return `## ${version}\n\n${body}\n${rest === "" ? "" : `\n${rest}`}`;
}

/** Whether `PUBLISH_CHANGELOGS` turns changelogs on for `plugin`. */
export function enabledFor(setting, plugin) {
  const names = (setting ?? "")
    .split(",")
    .map((name) => name.trim())
    .filter(Boolean);
  return names.includes("all") || names.includes(plugin);
}

if (import.meta.url === `file://${argv[1]}`) {
  const [dir, version, plugin, notesFile] = argv.slice(2);
  try {
    if (!dir || !version || !plugin || !notesFile) {
      throw new Error("usage: changelog.mjs DIR VERSION PLUGIN NOTES_FILE");
    }
    const body = notesBody(readFileSync(notesFile, "utf8"));
    if (body === "") {
      stdout.write(`changelog: the ${version} release notes are empty, so ${plugin} publishes no changelog\n`);
      exit(0);
    }
    stdout.write(`changelog entry for ${plugin} ${version}:\n\n${body}\n\n`);
    if (!enabledFor(env.PUBLISH_CHANGELOGS, plugin)) {
      stdout.write(`changelog: PUBLISH_CHANGELOGS doesn't name ${plugin}, so this is not written\n`);
      exit(0);
    }
    const path = join(dir, "CHANGELOG.md");
    const before = existsSync(path) ? readFileSync(path, "utf8") : "";
    const after = withEntry(before, version, body);
    if (after === before) {
      stdout.write(`changelog: ${path} already has a ${version} entry\n`);
    } else {
      writeFileSync(path, after);
      stdout.write(`changelog: wrote the ${version} entry to ${path}\n`);
    }
  } catch (error) {
    stderr.write(`changelog: ${error.message}\n`);
    exit(1);
  }
}
