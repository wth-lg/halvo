#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd -P)
. "$ROOT/test/contract/probes/lib/common.sh"
DIR=$1
version="zod $(package_version zod), node $(node --version)"
cp "$ROOT/test/contract/probes/zod-enum-one/export.mjs" "$DIR/export.mjs"
(cd "$DIR" && node export.mjs) > "$DIR/export.json"
status=$?
if [ "$status" != 0 ]; then
  echo "FAIL probe zod-enum-one: the export script exited $status, $version"
  exit 1
fi
cat "$DIR/export.json" >&2
verdict=$(json_field "$(cat "$DIR/export.json")" '[r.alone, r.inObject.properties.result].every((s) => JSON.stringify(s.enum) === "[\"rejected\"]" && !("const" in s)) && !JSON.stringify(r).includes("\"const\"")')
if [ "$verdict" = true ]; then
  echo "PASS probe zod-enum-one: z.enum([\"rejected\"]) exports in input mode as enum [\"rejected\"] with no const, alone and as a property, $version"
  exit 0
fi
echo "FAIL probe zod-enum-one: the export is $(tr -d '\n ' < "$DIR/export.json"), $version"
exit 1
