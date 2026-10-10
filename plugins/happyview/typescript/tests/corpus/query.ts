import log from "internal.logging";
import db from "happyview.db";

interface QueryInput {
  uri?: string;
  did?: string;
  limit?: string;
  cursor?: string;
}

interface QueryContext {
  collection: string;
}

interface Entry {
  uri: string;
  cid: string;
  record: unknown;
}

export default async function handle(input: QueryInput, ctx: QueryContext) {
  log.info("handling query", { uri: input.uri });

  if (input.uri) {
    const entry: Entry | null = await db.get(input.uri);
    if (!entry) {
      return { error: "NotFound", message: `no record at ${input.uri}` };
    }
    return { uri: entry.uri, cid: entry.cid, value: entry.record };
  }

  // Query params arrive as strings unless the lexicon types them, and a
  // chain step given undefined is skipped, so absent filters read as no
  // filter.
  return db
    .records(ctx.collection)
    .did(input.did)
    .limit(input.limit === undefined ? undefined : Number(input.limit))
    .cursor(input.cursor)
    .run();
}
