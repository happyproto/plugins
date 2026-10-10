import { create } from "internal.tids";

export default function handle(input: unknown, ctx: unknown): { rkey: string } {
  return { rkey: create() };
}
