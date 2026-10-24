// A handler written against the generated declarations. The check fails if
// they do not type-check, or if any line marked `@ts-expect-error` stops
// being an error, which is how a wrong type that still compiles is caught.
import type { Params, Output } from "happyview:lexicons/com.example.feed.getPosts";
import type { Record as Post, Event } from "happyview:lexicons/com.example.feed.post";
import type { QueryCtx, JobCtx, RecordEvent } from "happyview:ctx";
import { chain } from "happyview.objects";
import * as spaces from "happyview.spaces";
import * as http from "happyview.http";

export default async function handle(input: Params, ctx: QueryCtx): Promise<Output> {
  const actor: string = input.actor;
  const limit: number | undefined = input.limit;
  // @ts-expect-error params has no such field
  input.nope;
  const caller: string | undefined = ctx.caller_did;
  const env: string | undefined = ctx.env["API"];

  const response = await http.get("https://example.com", { headers: {} });
  const status: number = response.status;

  const doc: unknown = await chain().add().add().doc();
  await spaces.delete();

  const post: Post = {
    text: `${actor} ${limit} ${caller} ${env} ${status} ${String(doc)}`,
    createdAt: "2026-10-24T00:00:00Z",
    kind: "anything else",
    visibility: "public",
  };
  // @ts-expect-error a closed enum refuses other values
  const bad: Post = { text: "", createdAt: "", visibility: "secret" };
  void bad;
  return { posts: [post] };
}

export async function job(_input: unknown, ctx: JobCtx): Promise<void> {
  ctx.job.progress({ done: 1 });
  if (ctx.job.should_stop()) return;
  await ctx.job.wait(5);
}

export function onPost(event: Event): string | undefined {
  const generic: RecordEvent = event;
  void generic;
  return event.record?.text;
}
