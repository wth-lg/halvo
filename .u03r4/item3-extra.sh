#!/bin/sh
# sh item3-extra.sh SCRATCH REPO MACHINE SET: after one call of item 3, prints the count of the line
# "sandbox initialization failed" in the browser's kept standard error under SCRATCH, and for each
# line of the newest denial log of REPO/target/egress its protocol, its port and whether its host is
# loopback; never an address.
scratch=$1
repo=$2
machine=$3
set=$4
n=0
for f in "$scratch"/*-profile-egress.stderr; do
  [ -f "$f" ] || continue
  c=$(grep -c 'sandbox initialization failed' "$f")
  n=$((n + c))
done
echo "item3_stderr machine=$machine set=$set sandbox_init_failed_lines=$n"
last=$(find "$repo/target/egress" -mindepth 1 -maxdepth 1 -type d -name '[0-9]*' 2>/dev/null |
  awk -F/ '{ print $NF }' | sort -n | tail -1)
if [ -z "$last" ] || [ ! -f "$repo/target/egress/$last/denied.log" ]; then
  echo "item3_denied machine=$machine set=$set log=none"
  exit 0
fi
log="$repo/target/egress/$last/denied.log"
echo "item3_denied machine=$machine set=$set lines=$(wc -l <"$log" | tr -d ' ')"
LC_ALL=C awk -v m="$machine" -v s="$set" '{
  proto = "-"; dest = "-"
  for (i = 1; i <= NF; i++) {
    if ($i ~ /^proto=/) proto = substr($i, 7)
    if ($i ~ /^dest=/) dest = substr($i, 6)
  }
  port = dest; sub(/.*:/, "", port)
  host = dest; sub(/:[^:]*$/, "", host); gsub(/[][]/, "", host)
  if (host == "-" || host == "") loop = "unknown"
  else if (host ~ /^127\./ || host == "::1" || host == "localhost") loop = "yes"
  else loop = "no"
  print "item3_denied_line machine=" m " set=" s " proto=" proto " port=" port " loopback=" loop
}' "$log"
