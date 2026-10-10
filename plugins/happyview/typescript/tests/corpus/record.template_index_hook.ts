import log from "internal.logging";

interface RecordEvent {
  action: "create" | "update" | "delete";
  uri: string;
}

export default function handle(input: RecordEvent, ctx: unknown): void {
  if (input.action === "delete") {
    // record was deleted
    log.info(`deleted ${input.uri}`);
  } else {
    // record was created or updated
    log.info(`${input.action} ${input.uri}`);
  }
}
