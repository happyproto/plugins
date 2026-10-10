import json from "internal.json";

export default function handle(input: unknown, ctx: unknown): { ok: boolean } {
  const body: string = json.encode({ ok: true });
  return json.decode(body);
}
