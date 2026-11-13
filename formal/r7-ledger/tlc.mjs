import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = process.env.HALVO_ROOT ?? join(dirname(fileURLToPath(import.meta.url)), "..", "..", "backend");
export const MODEL = join(ROOT, "..", "formal", "r7-ledger");
export const STATE = process.env.GATE_STATE ?? join(ROOT, ".tmp");
export const OUTPUT_LIMIT = 1 << 28;
export const WORKERS = process.env.MODEL_WORKERS ?? "12";
const JAR = join(ROOT, ".tools", "tla2tools.jar");
const JAR_PIN = "tla2tools-1.7.4.jar";
const LAUNCHER = join(MODEL, "LoopbackTlc.java");
const LONGEST_RUN_MS = 1_800_000;
const JVM = [
  "-XX:+UseParallelGC",
  "-Xmx4g",
  "-Djava.rmi.server.hostname=127.0.0.1",
  "-Djdk.net.hosts.file=/dev/null",
  "-Dutil.ExecutionStatisticsCollector.id=NO_STATISTICS",
];

export function missingTool() {
  if (!/^[1-9][0-9]?$/.test(WORKERS)) return `MODEL_WORKERS is ${WORKERS}, not a number of worker threads from 1 to 99`;
  if (!existsSync(JAR)) return "there is no .tools/tla2tools.jar; sh scripts/prerequisites.sh fetches it";
  const pin = readFileSync(join(ROOT, "scripts", "tools.sha256"), "utf8")
    .split("\n")
    .map((each) => each.split(/\s+/))
    .find(([, name]) => name === JAR_PIN);
  const sha256 = createHash("sha256").update(readFileSync(JAR)).digest("hex");
  if (pin === undefined || pin[0] !== sha256) return `.tools/tla2tools.jar has the sha256 ${sha256}, not the one that scripts/tools.sha256 pins for ${JAR_PIN}`;
  if (spawnSync("java", ["-version"], { encoding: "utf8" }).status !== 0) return "java does not run; the checker needs a JDK of version 17 or later";
  return null;
}

export function scratch(name) {
  mkdirSync(STATE, { recursive: true });
  return mkdtempSync(join(STATE, `${name}.`));
}

export function tlc(work, name, args, longest = LONGEST_RUN_MS) {
  const run = spawnSync("java", [...JVM, "-cp", JAR, LAUNCHER, "-metadir", join(work, `meta-${name}`), ...args], {
    cwd: work,
    encoding: "utf8",
    maxBuffer: OUTPUT_LIMIT,
    timeout: longest,
    killSignal: "SIGKILL",
  });
  const output = `${run.stdout ?? ""}${run.stderr ?? ""}`;
  const file = join(work, `${name}.out`);
  writeFileSync(file, output);
  const states = /^(\d+) states generated, (\d+) distinct states found, (\d+) states left on queue\.$/m.exec(output);
  return {
    status: run.status,
    output,
    file,
    violated: [...new Set([...output.matchAll(/^Error: (?:Invariant|Action property) (\S+) is violated/gm)].map((match) => match[1]))],
    distinct: states === null ? null : states[2],
    complete: run.status === 0 && states !== null && states[3] === "0" && output.includes("Model checking completed. No error has been found."),
  };
}
