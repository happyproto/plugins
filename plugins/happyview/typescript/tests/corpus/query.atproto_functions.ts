import atproto from "happyview.atproto";

interface Input {
  uri: string;
}

interface Context {
  caller_did: string;
}

export default async function handle(input: Input, ctx: Context) {
  // The three lookups depend on nothing of each other's, so they run at
  // once; the check needs the signature, so it waits for it.
  const [pds, labels, sig]: [unknown, unknown, unknown] = await Promise.all([
    atproto.resolve_service_endpoint(ctx.caller_did),
    atproto.get_labels(input.uri),
    atproto.sign({ ok: true }),
  ]);
  const ok: boolean = await atproto.verify_signature({ ok: true }, sig, ctx.caller_did);
  return { pds, labels, ok };
}
