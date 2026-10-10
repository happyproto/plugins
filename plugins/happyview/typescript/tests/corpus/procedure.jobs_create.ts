import jobs from "happyview.jobs";

interface Context {
  collection: string;
}

export default async function handle(input: unknown, ctx: Context): Promise<{ job_id: string }> {
  const id: string = await jobs.create("app.example.reindex", { collection: ctx.collection }, { auth: true });
  return { job_id: id };
}
