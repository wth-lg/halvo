#!/bin/sh
set -u
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd -P)
. "$ROOT/test/contract/probes/lib/common.sh"
DIR=$1
version="wrangler $(package_version wrangler)"
mkdir -p "$DIR/src" "$DIR/out"
cat > "$DIR/src/other.ts" <<'EOF'
export function answer(): Response {
  return new Response("probe");
}
EOF
cat > "$DIR/src/index.ts" <<'EOF'
import { answer } from "./other.ts";

export default {
  fetch(): Response {
    return answer();
  },
};
EOF
wrangler_config "$DIR/wrangler.jsonc" src/index.ts '{}'
under_outbound_profile "$DIR/outbound.log" env XDG_CONFIG_HOME="$HALVO_WRANGLER_CONFIG_HOME" \
  "$ROOT/node_modules/.bin/wrangler" deploy --dry-run --config "$DIR/wrangler.jsonc" \
  --outdir "$DIR/out" --metafile "$DIR/out/meta.json"
if ! outbound_was_clean; then
  echo "FAIL probe dry-run-modules: the dry run under the outbound profile was not clean ($(outbound_state)), $version"
  exit 1
fi
inputs=$(node -e 'const m = JSON.parse(require("fs").readFileSync(process.argv[1], "utf8")); process.stdout.write(Object.keys(m.inputs).join(" "));' "$DIR/out/meta.json")
case " $inputs " in
  *"src/index.ts "*"src/other.ts "* | *"src/other.ts "*"src/index.ts "*)
    echo "PASS probe dry-run-modules: the metafile's inputs hold both source files of the bundle ($inputs), $version"
    exit 0
    ;;
esac
echo "FAIL probe dry-run-modules: the metafile's inputs are \"$inputs\", without both src/index.ts and src/other.ts, $version"
exit 1
