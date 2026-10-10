// A script with no default export is refused by both exports, as its
// counterparts are.
export function handle(input: unknown, ctx: unknown): { ok: boolean } {
  return { ok: true };
}
