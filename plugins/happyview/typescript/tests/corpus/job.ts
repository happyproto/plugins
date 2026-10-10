// Job runner: executes as a background job. `input` is the job's input;
// `ctx.job` exposes the job's id, progress(data), should_stop(), and
// wait(seconds). The return value becomes the job's result.
//
// `import` from "internal.*" and installed libraries provides the rest.

import log from "internal.logging";

interface JobContext {
  job: {
    id: string;
    progress(data: object): void;
    should_stop(): boolean;
  };
}

type Result = { done: true } | { partial: true };

export default async function handle(input: unknown, ctx: JobContext): Promise<Result> {
  log.info("job started", { job_id: ctx.job.id });

  ctx.job.progress({ status: "working" });

  if (ctx.job.should_stop()) {
    return { partial: true };
  }

  return { done: true };
}
