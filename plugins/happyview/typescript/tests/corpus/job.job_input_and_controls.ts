interface JobInput {
  collection: string;
}

interface JobContext {
  job: {
    id: string;
    progress(data: object): void;
    should_stop(): boolean;
    wait(seconds: number): Promise<void>;
  };
}

export default async function handle(input: JobInput, ctx: JobContext) {
  const collection: string = input.collection;
  ctx.job.progress({ done: 0 });
  if (ctx.job.should_stop()) {
    return { id: ctx.job.id, partial: true };
  }
  await ctx.job.wait(1);
  return { id: ctx.job.id, collection };
}
