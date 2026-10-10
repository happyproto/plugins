// A script with no default export is refused by both exports, as its Lua
// counterpart is.
export function handle(input, ctx) {
  return { ok: true };
}
