import log from "internal.logging";
import record from "happyview.record";

interface StrongRef {
  uri: string;
  cid: string;
}

export default async function handle(input: Record<string, unknown>, ctx: unknown): Promise<StrongRef> {
  const ref: StrongRef = await record.create("COLLECTION", input);
  log.info("record saved", { uri: ref.uri });
  return { uri: ref.uri, cid: ref.cid };
}
