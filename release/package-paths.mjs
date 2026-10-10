// A semantic-release shareable config that counts a commit toward a plugin's
// release when it touches any of a list of paths, rather than only the
// plugin's own directory.
//
// `semantic-release-monorepo` counts the commits that touch the directory the
// release runs in, which is right for a plugin that is all in its own
// directory and wrong for one built on a crate beside it: a fix in the
// QuickJS engine touches neither `javascript/` nor `typescript/`, so it would
// release neither interpreter, and the modules people install would never
// carry it. This does what that plugin does — wraps each configured plugin's
// `analyzeCommits`, `generateNotes`, `success` and `fail` and hands them only
// the commits that count — with the paths listed in `.releaserc.json`:
//
//   "extends": "../../../release/package-paths.mjs",
//   "packagePaths": ["plugins/happyview/javascript", "plugins/happyview/quickjs"]
//
// Paths are relative to the repository root, which is what `git diff-tree`
// names files by whatever directory the release runs in.

import { execFileSync } from "node:child_process";

/** Whether any of `files` is one of `paths` or lies beneath one. */
export function touches(files, paths) {
  return files.some((file) =>
    paths.some((path) => file === path || file.startsWith(`${path}/`)),
  );
}

/** The files a commit changed, relative to the repository root. */
export function filesOf(hash) {
  return execFileSync(
    "git",
    ["diff-tree", "--root", "--no-commit-id", "--name-only", "-r", hash],
    { encoding: "utf8" },
  )
    .split("\n")
    .filter(Boolean);
}

/** The commits among `commits` that touch any of `paths`. */
export function onlyPathCommits(commits, paths, files = filesOf) {
  return commits.filter((commit) => touches(files(commit.hash), paths));
}

function packagePaths(config) {
  const paths = config.packagePaths;
  if (!Array.isArray(paths) || paths.length === 0) {
    throw new Error(
      "release/package-paths.mjs: list the paths whose commits release this " +
        'plugin as "packagePaths" in its .releaserc.json',
    );
  }
  return paths.map((path) => path.replace(/\/+$/, ""));
}

// semantic-release accepts an array of functions for a step and runs them as
// if each were a plugin, but cannot say how many plugins are configured until
// it runs. So, as `semantic-release-plugin-decorators` does for
// `semantic-release-monorepo`, each step is ten slots, and the slot at index
// `i` runs the step of the `i`th configured plugin, if it has one.
const SLOTS = 10;

function wrapStep(step, transform) {
  return Array.from({ length: SLOTS }, (_, index) => {
    const slot = async (globalConfig, context) => {
      const { plugins } = context.options;
      if (index >= plugins.length) {
        return undefined;
      }
      const [name, config] = Array.isArray(plugins[index])
        ? plugins[index]
        : [plugins[index], {}];
      const plugin = await import(name);
      if (typeof plugin[step] !== "function") {
        return undefined;
      }
      return plugin[step](
        { ...globalConfig, ...config },
        transform(globalConfig, context),
      );
    };
    Object.defineProperty(slot, "name", { value: "package-paths" });
    return slot;
  });
}

/**
 * The four wrapped steps, reading each commit's files through `files`.
 *
 * Notes and comments name the release by its tag rather than its bare
 * version, as `semantic-release-monorepo` makes them, so every plugin's
 * release notes read the same whichever of the two filtered its commits.
 */
export function steps(files = filesOf) {
  const filtered = (config, context) => ({
    ...context,
    commits: onlyPathCommits(context.commits, packagePaths(config), files),
  });
  const tagged = (config, context) => {
    const next = filtered(config, context);
    if (next.nextRelease?.version && next.nextRelease.gitTag) {
      next.nextRelease = {
        ...next.nextRelease,
        version: next.nextRelease.gitTag,
      };
    }
    return next;
  };
  return {
    analyzeCommits: wrapStep("analyzeCommits", filtered),
    generateNotes: wrapStep("generateNotes", tagged),
    success: wrapStep("success", tagged),
    fail: wrapStep("fail", tagged),
  };
}

// The shareable config itself. semantic-release takes a module's default
// export when it has one, so the helpers above, exported for the tests, never
// become options of a release.
export default steps();
