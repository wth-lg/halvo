import { spawnSync } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const [mutantPath, copy] = process.argv.slice(2);
const mutant = JSON.parse(readFileSync(mutantPath, "utf8"));
for (const edit of mutant.edits) {
  const path = join(copy, edit.file);
  const text = readFileSync(path, "utf8");
  const at = text.indexOf(edit.find);
  if (at < 0 || text.indexOf(edit.find, at + 1) >= 0) {
    console.error(`the text to change is not in ${edit.file} exactly once: ${edit.find}`);
    process.exit(1);
  }
  writeFileSync(path, `${text.slice(0, at)}${edit.replace}${text.slice(at + edit.find.length)}`);
}
for (const command of mutant.then ?? []) {
  const result = spawnSync(command[0], command.slice(1), { cwd: copy, stdio: "inherit" });
  if (result.status !== 0) {
    console.error(`the step ${command.join(" ")} after the mutant exited ${result.status}`);
    process.exit(1);
  }
}
