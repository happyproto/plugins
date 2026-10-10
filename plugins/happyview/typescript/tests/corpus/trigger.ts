// Trigger script: `input` is the trigger's payload; `ctx` describes this
// invocation (caller, environment, trigger id, and more). Return a
// transformed value, or `undefined` to skip the operation.
//
// `import` from "internal.*" and installed libraries provides the rest.

import log from "internal.logging";

interface Context {
  trigger: string;
  caller_did?: string;
}

export default function handle<T>(input: T, ctx: Context): T {
  log.info("script fired", { trigger: ctx.trigger, caller: ctx.caller_did });
  return input;
}
