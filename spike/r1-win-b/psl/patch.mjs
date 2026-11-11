// Adds one reachable site-cut call to the daemon's main for the S8 size probe.
// Mode "stub": the same branch without the list; mode "psl": the registrable domain from psl.
import { readFileSync, writeFileSync } from "node:fs";

const [mode, file] = process.argv.slice(2);
const anchor = "    let args: Vec<String> = std::env::args().skip(1).collect();\n";
const text = readFileSync(file, "utf8").replace(/\r\n/g, "\n");
if (!text.includes(anchor)) {
  console.log("S8 patch anchor_found=false");
  process.exit(1);
}
const body =
  mode === "psl"
    ? "psl::domain_str(&host).map(|d| d.len()).unwrap_or(0)"
    : "host.rfind('.').map(|i| host.len() - i).unwrap_or(0)";
const block =
  anchor +
  '    if args.first().map(String::as_str) == Some("--r1spike-site") {\n' +
  "        let host = args.get(1).cloned().unwrap_or_default();\n" +
  `        let n: usize = ${body};\n` +
  '        println!("{n}");\n' +
  "        return ExitCode::SUCCESS;\n" +
  "    }\n";
writeFileSync(file, text.replace(anchor, block));
console.log(`S8 patch mode=${mode} anchor_found=true`);
