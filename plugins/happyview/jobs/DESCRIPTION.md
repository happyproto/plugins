Some work is too slow to do inside a request, like data migrations, batch exports, or syncing with an outside API. This library lets a script queue that work for HappyView's job worker and check on it later. The worker runs whichever script has the trigger `job.run:<type>` for the job's type.

## Usage

```lua
local jobs = require("happyview.jobs")

function handle(input, ctx)
  local id = jobs.create("app.example.sync", {did = ctx.caller_did}, {auth = true})
  return {id = id}
end

function status(input, ctx)
  return jobs.get(input.id) -- the caller's own job, or nil
end
```

## What it exposes

- `create(job_type, input, opts)` enqueues a job as the calling user and returns its id. `opts.auth` carries the caller's PDS session into the job, so the job's script can act as that user with `caller:*` capabilities.
- `get(id)` returns one of the caller's own jobs, or `nil`. Someone else's job and a job that doesn't exist look the same.
- `get_any(id)` returns any user's job, and `list_any(opts)` lists jobs across every user, newest first, filtered by `status` and `job_type` (`limit` defaults to 50, capped at 200).

A job table carries its `status`, `input`, `progress`, `result`, `error` and timestamps, never the session it runs with. A running job reports its own progress through `ctx.job` in its script. This library is for every other script that wants to check on it.

Job types starting with `happyview.` are reserved for HappyView's own native jobs, so `create` refuses them.

## Capabilities

`jobs:create`, `jobs:read` and `jobs:read_any`, which are **Medium tier**. Scripts can enqueue jobs as the calling user (optionally with their session), and `jobs:read_any` lets them read every user's jobs.
