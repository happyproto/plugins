import { list, get } from "happyview.linked_repos";

interface Input {
  did: string;
}

export default async function handle(input: Input, ctx: unknown): Promise<{ grants: unknown }> {
  const grants: unknown = await list();
  const repo = get(input.did);
  await repo.create_record({ collection: "app.example.post", record: { text: "hi" } });
  return { grants };
}
