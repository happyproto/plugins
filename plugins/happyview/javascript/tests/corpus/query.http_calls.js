import http from "happyview.http";

export default async function handle(input, ctx) {
  const resp = await http.get("https://example.com/data");
  await http.post("https://example.com/hook", { body: resp.body });
  return { status: resp.status };
}
