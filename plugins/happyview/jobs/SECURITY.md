## Reporting a vulnerability

Please report vulnerabilities privately through [GitHub's advisory form](https://github.com/happyproto/plugins/security/advisories/new). Don't open a public issue for a vulnerability.

Only the latest release of this plugin gets fixes.

## What this plugin can do

It declares `jobs:create`, `jobs:read` and `jobs:read_any`, which together are **Medium tier**. A script can enqueue background jobs as the calling user, optionally carrying that user's PDS session into the job, and with `jobs:read_any` it can read every user's jobs.

It imports exactly `host_jobs_create`, `host_jobs_get`, `host_jobs_get_any` and `host_jobs_list_any`. The loader refuses a module whose imports need more than it declares.

The plugin runs sandboxed in WASM with no filesystem access, and it only talks to HappyView through the host functions it imports. HappyView validates the job type, enqueues the row and decides who may read which job.

## What's worth reporting

- Enqueueing a job as someone other than the caller, or with no caller at all.
- `get` returning another user's job, or anything that tells it apart from a job that doesn't exist.
- A job table that includes the job's session fields.
- Enqueueing a reserved `happyview.` job type.
