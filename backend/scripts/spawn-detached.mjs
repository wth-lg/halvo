import { spawn } from "node:child_process";
import { openSync } from "node:fs";

const [log, command, ...args] = process.argv.slice(2);
if (!log || !command) {
  console.error("usage: node scripts/spawn-detached.mjs LOG COMMAND [ARGS]");
  process.exit(64);
}
const output = openSync(log, "a");
const child = spawn(command, args, { detached: true, stdio: ["ignore", output, output] });
child.on("error", (error) => {
  console.error(`spawn-detached: cannot start ${command}: ${error.message}`);
  process.exit(1);
});
child.on("spawn", () => {
  console.log(child.pid);
  child.unref();
  process.exit(0);
});
