## [1.1.0 (2026-10-08)](https://github.com/happyproto/plugins/releases/tag/happyview-jobs-v1.1.0)

- **Scripts can read jobs.** `get(id)` returns one of the caller's own jobs (or `nil`), `get_any(id)` returns anyone's, and `list_any(opts)` lists jobs across every user, filtered by `status` and `job_type`. They need two new capabilities, `jobs:read` and `jobs:read_any`. A job table never includes the job's session fields.

## [1.0.0 (2026-10-07)](https://github.com/happyproto/plugins/releases/tag/happyview-jobs-v1.0.0)

- **First release on HappyView v3's plugin API.** Ships `happyview.jobs` as a standalone library plugin, with a single export: `create(job_type, input, opts)`, which enqueues a job and returns its id. `opts.auth` carries the caller's PDS session into the job.
