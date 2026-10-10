import spaces from "happyview.spaces";

export default async function handle(input, ctx) {
  const created = await spaces.create({ type: "app.example.chat", skey: input.skey });
  const s = spaces.get(created.uri);
  await s.write_record({ collection: "app.example.message", record: { text: input.text } });
  return { uri: created.uri };
}
