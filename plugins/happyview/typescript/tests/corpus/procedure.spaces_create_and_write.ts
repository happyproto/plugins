import spaces from "happyview.spaces";

interface ChatInput {
  skey: string;
  text: string;
}

export default async function handle(input: ChatInput, ctx: unknown): Promise<{ uri: string }> {
  const created: { uri: string } = await spaces.create({ type: "app.example.chat", skey: input.skey });
  const s = spaces.get(created.uri);
  await s.write_record({ collection: "app.example.message", record: { text: input.text } });
  return { uri: created.uri };
}
