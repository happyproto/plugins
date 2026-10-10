import jobs from "happyview.jobs";

export default async function handle(input, ctx) {
  const id = await jobs.create("app.example.reindex", { collection: ctx.collection }, { auth: true });
  return { job_id: id };
}
