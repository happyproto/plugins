export default function handle(input, ctx) {
  if (input.neg) {
    return undefined;
  }
  return { src: input.src, uri: input.uri, val: input.val, cts: input.cts, exp: input.exp, raw: input };
}
