#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/.." && pwd -P)
APP_COMMIT=685d84637c302976956d56e6bbb5c18a4503e7c8
ENUM_PATH=crates/halvo-capture/src/event.rs
KICKOFF_SHA256=a55aa0faaddb3bdecb710dc8d7cc77d6f1739c920a75432b21aae059181b46c3
OUT="$ROOT/contract/names/forbidden-names.json"

usage() {
  cat >&2 <<'EOF'
Writes the first file of forbidden names, version 0, from three sources: the serde names of the
enum Modality in crates/halvo-capture/src/event.rs at commit 685d846 of the app repository, the
field names of lines 147 to 149 of the kickoff plan, and the content words of its line 170.

  HALVO_APP_REPO=DIR HALVO_KICKOFF_PLAN=FILE sh scripts/names-v0.sh           print the file
  HALVO_APP_REPO=DIR HALVO_KICKOFF_PLAN=FILE sh scripts/names-v0.sh --write   replace
                                                    contract/names/forbidden-names.json

HALVO_APP_REPO is a clone of the app repository that holds the commit; the script reads it with
git show, so the state of its working tree does not matter. HALVO_KICKOFF_PLAN is the plan.md of
the kickoff run, which must have the sha256 that this script holds. Exit 64 when either is unset
or names nothing, or on bad usage, and nothing is written; exit 1 when git show fails, the plan
has another sha256 or a name is missing from its source.
EOF
  exit 64
}

write=
case $# in
  0) ;;
  1) [ "$1" = --write ] || usage; write=1 ;;
  *) usage ;;
esac
[ -n "${HALVO_APP_REPO:-}" ] && [ -d "$HALVO_APP_REPO" ] || usage
[ -n "${HALVO_KICKOFF_PLAN:-}" ] && [ -f "$HALVO_KICKOFF_PLAN" ] || usage

if command -v shasum > /dev/null 2>&1; then
  sha=$(shasum -a 256 < "$HALVO_KICKOFF_PLAN" | cut -d ' ' -f 1)
else
  sha=$(sha256sum < "$HALVO_KICKOFF_PLAN" | cut -d ' ' -f 1)
fi
if [ "$sha" != "$KICKOFF_SHA256" ]; then
  echo "names-v0: the kickoff plan has the sha256 $sha, not $KICKOFF_SHA256 of its 190 lines on 2026-11-10" >&2
  exit 1
fi
mkdir -p "$ROOT/.tmp"
scratch=$(mktemp -d "$ROOT/.tmp/names-v0.XXXXXX") || exit 1
if ! git -C "$HALVO_APP_REPO" show "$APP_COMMIT:$ENUM_PATH" > "$scratch/event.rs"; then
  echo "names-v0: git show $APP_COMMIT:$ENUM_PATH failed in HALVO_APP_REPO" >&2
  exit 1
fi
short=$(printf '%s' "$APP_COMMIT" | cut -c 1-7)
node --input-type=module - "$scratch/event.rs" "$HALVO_KICKOFF_PLAN" "$ENUM_PATH" "$short" > "$scratch/names.json" <<'EOF' || exit 1
import { readFileSync } from "node:fs";

const [enumFile, planFile, enumPath, short] = process.argv.slice(2);
const MODALITY = ["screen_frame", "keystroke", "pointer", "foreground_app", "window_title", "system_audio", "microphone", "camera"];
const FIELD_LINES = [147, 148, 149];
const CONTENT_LINE = 170;
const CONTENT_WORDS = ["screen", "keystroke", "pointer", "audio", "image", "transcript", "embedding"];
const sources = new Map();
const add = (name, source) => {
  if (!sources.has(name)) sources.set(name, []);
  if (!sources.get(name).includes(source)) sources.get(name).push(source);
};
const missing = (what) => {
  process.stderr.write("names-v0: " + what + "\n");
  process.exit(1);
};

const enumLines = readFileSync(enumFile, "utf8").split("\n");
const start = enumLines.findIndex((line) => /^pub enum Modality \{$/.test(line.trim()));
if (start < 0) missing("the enum Modality is not in " + enumPath);
const found = [];
for (let index = start + 1; index < enumLines.length && enumLines[index].trim() !== "}"; index += 1) {
  const variant = /^([A-Z][A-Za-z0-9]*),?$/.exec(enumLines[index].trim());
  if (!variant) continue;
  const name = variant[1].replace(/([a-z0-9])([A-Z])/g, "$1_$2").toLowerCase();
  found.push(name);
  add(name, "app@" + short + ":" + enumPath + ":" + (index + 1));
}
for (const name of MODALITY) if (!found.includes(name)) missing("the variant " + name + " is not in the enum Modality");
if (found.length !== MODALITY.length) missing("the enum Modality has " + found.length + " variants, not " + MODALITY.length);

const planLines = readFileSync(planFile, "utf8").split("\n");
for (const number of FIELD_LINES) {
  const line = planLines[number - 1] ?? "";
  let depth = 0;
  let word = "";
  let names = 0;
  for (let at = 0; at < line.length; at += 1) {
    const char = line[at];
    if (/[a-z0-9_]/.test(char)) {
      word += char;
      continue;
    }
    if (char === " " && line[at + 1] === "(" && depth === 0 && word !== "" && !/[A-Za-z0-9_]/.test(line[at - word.length - 1] ?? "")) {
      add(word, "kickoff:plan.md:" + number);
      names += 1;
    }
    if (char === "(") depth += 1;
    if (char === ")") depth -= 1;
    word = "";
  }
  if (names === 0) missing("line " + number + " of the kickoff plan holds no field name");
}
const content = planLines[CONTENT_LINE - 1] ?? "";
for (const name of CONTENT_WORDS) {
  if (!new RegExp("\\b" + name + "\\b").test(content)) missing("the word " + name + " is not on line " + CONTENT_LINE + " of the kickoff plan");
  add(name, "kickoff:plan.md:" + CONTENT_LINE);
}

const names = [...sources.keys()].sort((a, b) => Buffer.compare(Buffer.from(a), Buffer.from(b)));
const file = { version: 0, names: names.map((name) => ({ name, sources: sources.get(name) })) };
process.stdout.write(JSON.stringify(file, null, 2) + "\n");
EOF
if [ -n "$write" ]; then
  mkdir -p "$(dirname "$OUT")"
  cp "$scratch/names.json" "$OUT"
else
  cat "$scratch/names.json"
fi
