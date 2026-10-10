import log from "internal.logging";
import record from "happyview.record";

export default async function handle(input, ctx) {
  const ref = await record.create("COLLECTION", input);
  log.info("record saved", { uri: ref.uri });
  return { uri: ref.uri, cid: ref.cid };
}
