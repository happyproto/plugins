# happyview-jobs

Library plugin exposing `happyview.jobs`: enqueue background jobs for the
job worker. Its one export is a thin translator over one SDK host wrapper;
the host validates the job type and enqueues the row.

## Lua

```lua
local jobs = require("happyview.jobs")

function handle(input, ctx)
  local id = jobs.create("app.example.reindex", {collection = "app.bsky.feed.post"})
  local id_with_auth = jobs.create("app.example.sync", {did = ctx.caller_did}, {auth = true})
  return {id = id, id_with_auth = id_with_auth}
end
```

## Surface

- `create(job_type, input[, opts])` — enqueues a job and returns its id.
  `input` is any table; a nil `input` sends `{}`. `opts.auth` (default
  `false`) carries the calling user's PDS session into the job, so its
  script can act with `caller:*` capabilities as that user.

Once running, a job controls itself and reports progress through
`ctx.job` inside its own script (`ctx.job.id`, `ctx.job.progress`,
`ctx.job.should_stop`, `ctx.job.wait`) — this library only enqueues, from
any script context, and has no view into a job once it exists.

## Errors

- `BAD_INPUT` — a missing `job_type`, a non-table `opts`, a `job_type`
  starting with the reserved `happyview.` prefix (native job types; only
  internal Rust callers may enqueue them), or no caller to enqueue as (a
  label script or an unauthenticated request; a record-event script acts as
  the record's author, so it can enqueue) — checked before `opts.auth` even
  matters.
- `NO_SESSION` — `opts.auth` is set, there is a caller, but its script
  context has no DPoP session to carry (a cookie-authenticated runner).
- `FORBIDDEN` — the plugin lacks `jobs:create`.
- `HOST_ERROR` — a database failure inside the host.

## Capabilities

`jobs:create`. This library is **Medium tier**: it can enqueue background
jobs as the calling user, optionally carrying that user's PDS session into
the job.

Imports exactly `host_jobs_create`.

## Build

```bash
cargo test -p happyview-jobs
cargo build --release --target wasm32-unknown-unknown -p happyview-jobs
```
