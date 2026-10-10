export default async function handle(input, ctx) {
  const collection = input.collection;
  ctx.job.progress({ done: 0 });
  if (ctx.job.should_stop()) {
    return { id: ctx.job.id, partial: true };
  }
  await ctx.job.wait(1);
  return { id: ctx.job.id, collection };
}
