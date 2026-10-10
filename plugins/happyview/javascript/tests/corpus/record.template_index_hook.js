import log from "internal.logging";

export default function handle(input, ctx) {
  if (input.action === "delete") {
    // record was deleted
    log.info(`deleted ${input.uri}`);
  } else {
    // record was created or updated
    log.info(`${input.action} ${input.uri}`);
  }
}
