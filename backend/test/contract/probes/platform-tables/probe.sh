#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd -P)
. "$ROOT/test/contract/probes/lib/common.sh"
DIR=$1
version="wrangler $(package_version wrangler)"
runtime="miniflare $(node -p 'require(require.resolve("miniflare/package.json", { paths: [require("path").dirname(require.resolve("wrangler/package.json", { paths: [process.argv[1]] }))] })).version' "$ROOT")"
LOCAL_RUNTIME_TABLE=__miniflare_do_name
cp "$ROOT/test/contract/probes/platform-tables/worker.js.in" "$DIR/index.js"
wrangler_config "$DIR/wrangler.jsonc" index.js \
  '{"durable_objects":{"bindings":[{"name":"PROBE","class_name":"Probe"}]},"migrations":[{"tag":"v1","new_sqlite_classes":["Probe"]}]}'
under_outbound_profile "$DIR/outbound.log" node "$LIB/serve.mjs" "$DIR/wrangler.jsonc" "$DIR/state" \
  '[{"vars":[],"paths":["/names"]}]'
if ! outbound_was_clean; then
  echo "FAIL probe platform-tables: the run under the outbound profile was not clean ($(outbound_state)), $version"
  exit 1
fi
answer=$(served_result)
status=$(json_field "$answer" 'r.runs[0].answers[0].status')
if [ "$status" != 200 ]; then
  echo "FAIL probe platform-tables: the object answered $status: $(json_field "$answer" 'r.runs[0].answers[0].body'), $version"
  exit 1
fi
names=$(json_field "$answer" 'JSON.parse(r.runs[0].answers[0].body).names.join(" ")')
columns=$(json_field "$answer" 'JSON.parse(r.runs[0].answers[0].body).columns.join(",")')
rows=$(json_field "$answer" 'JSON.parse(r.runs[0].answers[0].body).rows')
other=
named=
for name in $names; do
  case "$name" in
    probe_own | sqlite_* | _cf_*) ;;
    "$LOCAL_RUNTIME_TABLE") named=1 ;;
    *) other="$other $name" ;;
  esac
done
case " $names " in
  *" probe_own "*) ;;
  *) other="$other (no probe_own)" ;;
esac
if [ -n "$other" ]; then
  echo "FAIL probe platform-tables: the tables are $names; these are neither the object's own, nor start with sqlite_ or _cf_, nor are $LOCAL_RUNTIME_TABLE:$other, $version"
  exit 1
fi
if [ -n "$named" ] && { [ "$columns" != "id,name" ] || [ "$rows" != '[{"id":1,"name":"one"}]' ]; }; then
  echo "FAIL probe platform-tables: $LOCAL_RUNTIME_TABLE has the columns $columns and the rows $rows, not id,name and one row with the name one by which the probe addressed its object, $version"
  exit 1
fi
if [ -n "$named" ]; then
  echo "PASS probe platform-tables: the tables are $names; every one but probe_own starts with sqlite_ or _cf_ or is $LOCAL_RUNTIME_TABLE, whose columns are $columns and whose one row is $rows, the name by which the probe addressed its object, $version, $runtime"
  exit 0
fi
echo "PASS probe platform-tables: the tables are $names; every one but probe_own starts with sqlite_ or _cf_, and $LOCAL_RUNTIME_TABLE is absent, $version, $runtime"
exit 0
