import { list, get } from "happyview.linked_repos";

export default async function handle(input, ctx) {
  const grants = await list();
  const repo = get(input.did);
  await repo.create_record({ collection: "app.example.post", record: { text: "hi" } });
  return { grants };
}
