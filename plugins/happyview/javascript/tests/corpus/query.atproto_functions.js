import atproto from "happyview.atproto";

export default async function handle(input, ctx) {
  // The three lookups depend on nothing of each other's, so they run at
  // once; the check needs the signature, so it waits for it.
  const [pds, labels, sig] = await Promise.all([
    atproto.resolve_service_endpoint(ctx.caller_did),
    atproto.get_labels(input.uri),
    atproto.sign({ ok: true }),
  ]);
  const ok = await atproto.verify_signature({ ok: true }, sig, ctx.caller_did);
  return { pds, labels, ok };
}
