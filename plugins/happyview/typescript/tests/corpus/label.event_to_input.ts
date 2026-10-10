interface Label {
  src: string;
  uri: string;
  val: string;
  neg?: boolean;
  cts?: string;
  exp?: string;
}

interface LabelInput {
  src: string;
  uri: string;
  val: string;
  cts?: string;
  exp?: string;
  raw: Label;
}

export default function handle(input: Label, ctx: unknown): LabelInput | undefined {
  if (input.neg) {
    return undefined;
  }
  return { src: input.src, uri: input.uri, val: input.val, cts: input.cts, exp: input.exp, raw: input };
}
