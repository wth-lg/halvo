import { sweepMarks } from "./process-group.ts";

/** The global set-up of the node tests: before any test starts, it ends what an earlier run of this checkout left (sweepMarks) and prints a line for each group. */
export default function setup(): void {
  for (const line of sweepMarks()) process.stderr.write(`${line}\n`);
}
