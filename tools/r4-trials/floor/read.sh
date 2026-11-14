#!/bin/sh
# usage: sh tools/r4-trials/floor/read.sh
# The reading lines of the floor trial (contracts.md I11.1, I11.4): what the builder read and did
# not measure. It starts no app. Exit: 0, or 2 on a refusal.
if [ "$#" -ne 0 ]; then
  echo "REFUSED usage: sh tools/r4-trials/floor/read.sh takes no argument" >&2
  exit 2
fi
cat <<'EOF'
reading trial=floor machine=mac fact=safari_of_macos14 value=17.0 date=2026-11-12
reading trial=floor machine=mac fact=safari17_build value=19616.1.27 date=2026-11-12
reading trial=floor machine=mac fact=compat_data value=8.1.8 date=2026-11-12
reading trial=floor machine=mac fact=segmenter_first_safari value=14.1 date=2026-11-12
reading trial=floor machine=mac fact=segmenter_first_edge value=87 date=2026-11-12
EOF
