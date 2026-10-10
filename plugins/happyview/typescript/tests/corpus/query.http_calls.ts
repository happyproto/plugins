import http from "happyview.http";

interface Response {
  status: number;
  body: string;
}

export default async function handle(input: unknown, ctx: unknown): Promise<{ status: number }> {
  const resp: Response = await http.get("https://example.com/data");
  await http.post("https://example.com/hook", { body: resp.body });
  return { status: resp.status };
}
