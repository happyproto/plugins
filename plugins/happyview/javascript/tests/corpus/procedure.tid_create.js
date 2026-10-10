import { create } from "internal.tids";

export default function handle(input, ctx) {
  return { rkey: create() };
}
