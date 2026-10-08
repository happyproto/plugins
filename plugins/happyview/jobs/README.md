# happyview-jobs

Library plugin exposing `happyview.jobs`: enqueue and read background jobs
for the job worker. Each export is a thin translator over one SDK host
wrapper; the host validates the job type, enqueues the row and enforces who
may read which job.

## Lua

```lua
local jobs = require("happyview.jobs")

function handle(input, ctx)
  local id = jobs.create("app.example.reindex", {collection = "app.bsky.feed.post"})
  local id_with_auth = jobs.create("app.example.sync", {did = ctx.caller_did}, {auth = true})
  return {id = id, id_with_auth = id_with_auth}
end

function status(input, ctx)
  local mine = jobs.get(input.id)         -- the caller's own job, or nil
  local any = jobs.get_any(input.id)      -- any user's job, or nil
  local running = jobs.list_any({status = {"running"}, job_type = "app.example.sync", limit = 20})
  return {mine = mine, any = any, running = running}
end
```

## Surface

- `create(job_type, input[, opts])` — enqueues a job and returns its id.
  `input` is any table; a nil `input` sends `{}`. `opts.auth` (default
  `false`) carries the calling user's PDS session into the job, so its
  script can act with `caller:*` capabilities as that user.

- `get(id)` — returns the job as a table, or `nil` when it does not exist
  or belongs to another user (the two are indistinguishable). Needs
  `jobs:read`.
- `get_any(id)` — returns any user's job, or `nil` when it does not exist.
  Needs `jobs:read_any`.
- `list_any([opts])` — returns an array of jobs across every user, newest
  first. `opts` is `{status = {..}?, job_type = string?, limit = number?}`;
  an empty `status` matches every status, and `limit` defaults to 50 on the
  host, which caps it at 200. Needs `jobs:read_any`.

A job table has `id`, `job_type`, `status`, `input`, `progress`, `result`,
`error`, `created_by`, `created_at`, `started_at` and `completed_at`; the
optional fields are absent when unset. It never carries the job's session
fields.

Once running, a job controls itself and reports progress through
`ctx.job` inside its own script (`ctx.job.id`, `ctx.job.progress`,
`ctx.job.should_stop`, `ctx.job.wait`); this library enqueues and reads
jobs from any script context.

## Errors

- `BAD_INPUT` — a missing `job_type`, a non-table `opts` (for `list_any` too), a missing `id`, a `job_type`
  starting with the reserved `happyview.` prefix (native job types; only
  internal Rust callers may enqueue them), or no caller to enqueue as (a
  label script or an unauthenticated request; a record-event script acts as
  the record's author, so it can enqueue) — checked before `opts.auth` even
  matters.
- `NO_SESSION` — `opts.auth` is set, there is a caller, but its script
  context has no DPoP session to carry (a cookie-authenticated runner).
- `FORBIDDEN` — the plugin lacks the capability the call needs
  (`jobs:create`, `jobs:read` or `jobs:read_any`).
- `HOST_ERROR` — a database failure inside the host.

## Capabilities

`jobs:create`, `jobs:read` and `jobs:read_any`. This library is **Medium
tier**: it can enqueue background jobs as the calling user, optionally
carrying that user's PDS session into the job, and `jobs:read_any` reads
every user's jobs.

Imports exactly `host_jobs_create`, `host_jobs_get`, `host_jobs_get_any` and
`host_jobs_list_any`.

## Build

```bash
cargo test -p happyview-jobs
cargo build --release --target wasm32-unknown-unknown -p happyview-jobs
```
