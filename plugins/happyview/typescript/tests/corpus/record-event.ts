import log from "internal.logging";

interface RecordEvent {
  action: "create" | "update" | "delete";
  uri: string;
  did?: string;
  record?: Record<string, unknown>;
}

export default function handle(input: RecordEvent, ctx: unknown): Record<string, unknown> | true {
  if (input.action === "delete") {
    log.info("record deleted", { uri: input.uri });
  } else {
    log.info(`record ${input.action}`, { uri: input.uri, did: input.did });
  }

  // Returning nothing skips the operation, and a delete carries no record,
  // so `true` is what lets a delete through.
  return input.record ?? true;
}
