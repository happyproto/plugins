import json from "internal.json";

export default function handle(input, ctx) {
  const body = json.encode({ ok: true });
  return json.decode(body);
}
