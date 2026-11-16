# shellcheck shell=sh

LIMIT=${LIMIT:-420}
RUNS=${RUNS:-10}
code=0
current_call=""
pend_key=""
pend_name=""
pend_had=no
pend_type=""
pend_data=""
pend_made=""
port=""
pypdf=""

reg_value() {
  MSYS_NO_PATHCONV=1 reg query "HKLM\\SOFTWARE\\Policies\\$1" /v "$2" 2>/dev/null | tr -d '\r' |
    awk -v n="$2" '$1 == n { t = $2; $1 = ""; $2 = ""; sub(/^ +/, ""); print t " " $0; f = 1 } END { exit f ? 0 : 1 }'
}

key_exists() {
  MSYS_NO_PATHCONV=1 reg query "HKLM\\SOFTWARE\\Policies\\$1" >/dev/null 2>&1
}

policy_set() {
  pend_key=""
  pend_had=no
  pend_type=""
  pend_data=""
  pend_made=""
  made_list=""
  sub=""
  for part in $(printf '%s\n' "$1" | tr '\134' ' '); do
    sub=${sub:+$sub\\}$part
    key_exists "$sub" || made_list="$sub${made_list:+ $made_list}"
  done
  had=no
  if prev=$(reg_value "$1" "$2"); then
    had=yes
    pend_type=${prev%% *}
    pend_data=${prev#* }
  fi
  pend_had=$had
  pend_made=$made_list
  pend_name=$2
  pend_key=$1
  MSYS_NO_PATHCONV=1 reg add "HKLM\\SOFTWARE\\Policies\\$1" /v "$2" /t "$3" /d "$4" /f >/dev/null
  made=0
  for m in $pend_made; do made=$((made + 1)); done
  echo "policy set browser=$k name=$2 key_existed=$(if [ "$made" -eq 0 ]; then echo yes; else echo no; fi) value_existed=$pend_had keys_made=$made"
}

policy_unset() {
  [ -n "$pend_key" ] || return 0
  if [ "$pend_had" = yes ]; then
    MSYS_NO_PATHCONV=1 reg add "HKLM\\SOFTWARE\\Policies\\$pend_key" /v "$pend_name" /t "$pend_type" /d "$pend_data" /f >/dev/null
  else
    MSYS_NO_PATHCONV=1 reg delete "HKLM\\SOFTWARE\\Policies\\$pend_key" /v "$pend_name" /f >/dev/null
  fi
  for m in $pend_made; do
    MSYS_NO_PATHCONV=1 reg delete "HKLM\\SOFTWARE\\Policies\\$m" /f >/dev/null
  done
  left=0
  for m in $pend_made; do key_exists "$m" && left=$((left + 1)); done
  removed=no
  if now=$(reg_value "$pend_key" "$pend_name"); then
    [ "$pend_had" = yes ] && [ "$now" = "$pend_type $pend_data" ] && removed=yes
  else
    [ "$pend_had" = no ] && removed=yes
  fi
  echo "policy removed=$removed name=$pend_name earlier=$pend_had keys_left=$left"
  pend_key=""
}

undo() {
  if [ -n "$current_call" ]; then
    kill -TERM "$current_call" 2>/dev/null
    wait "$current_call" 2>/dev/null
    current_call=""
  fi
  policy_unset
  echo "harness_end"
}

arm_undo() {
  trap 'undo' EXIT
  trap 'exit 129' HUP
  trap 'exit 130' INT
  trap 'exit 143' TERM
}

free_port() {
  p=$1
  n=0
  while [ "$n" -lt 20 ]; do
    if node tools/r3-trials/web/port-free.mjs "$p" >/dev/null 2>&1; then
      port=$p
      echo "port_check port=$1 free=$(if [ "$p" = "$1" ]; then echo yes; else echo no; fi) used=$p"
      return 0
    fi
    p=$((p + 1000))
    n=$((n + 1))
  done
  port=""
  echo "port_check port=$1 free=no used=none"
  return 1
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum <"$1" | awk '{ print $1 }'
  else
    shasum -a 256 <"$1" | awk '{ print $1 }'
  fi
}

pypdf_ready() {
  line=$(grep '^fetch file=pypdf-' "$1/fetch.txt")
  file=$(printf '%s\n' "$line" | sed -n 's/.* file=\([^ ]*\).*/\1/p')
  want_bytes=$(printf '%s\n' "$line" | sed -n 's/.* bytes=\([0-9]*\).*/\1/p')
  want_sha=$(printf '%s\n' "$line" | sed -n 's/.* sha256=\([0-9a-f]*\).*/\1/p')
  url=$(printf '%s\n' "$line" | sed -n 's/.* url=\([^ ]*\).*/\1/p')
  whl="$RUNNER_TEMP/$file"
  got_bytes=0
  got_sha=-
  if [ -n "$file" ] && [ -n "$url" ] && curl -fsSL --retry 2 -o "$whl" "$url"; then
    got_bytes=$(wc -c <"$whl" | tr -d ' ')
    got_sha=$(sha256_of "$whl")
  fi
  bytes_ok=no
  sha_ok=no
  [ "$got_bytes" = "$want_bytes" ] && bytes_ok=yes
  [ "$got_sha" = "$want_sha" ] && sha_ok=yes
  echo "pypdf_fetch file=$file bytes=$got_bytes sha256=$got_sha bytes_ok=$bytes_ok sha256_ok=$sha_ok"
  [ "$bytes_ok" = yes ] && [ "$sha_ok" = yes ] || return 1
  lic=$(python -I -c 'import sys, zipfile
z = zipfile.ZipFile(sys.argv[1])
meta = [n for n in z.namelist() if n.endswith(".dist-info/METADATA")]
text = z.read(meta[0]).decode("utf-8") if meta else ""
lic = [n for n in z.namelist() if ".dist-info/" in n and n.rsplit("/", 1)[-1] == "LICENSE"]
body = " ".join(z.read(lic[0]).decode("utf-8").split()) if lic else ""
bsd = "Classifier: License :: OSI Approved :: BSD License" in text or "License-Expression: BSD-3-Clause" in text
clauses = ("Redistributions of source code must retain", "Redistributions in binary form must reproduce",
           "endorse or promote products derived from this software")
three = all(c in body for c in clauses)
print("classifier_bsd=%s licence_file_bsd3=%s" % ("yes" if bsd else "no", "yes" if three else "no"))' "$whl" 2>/dev/null)
  echo "pypdf_licence ${lic:-classifier_bsd=no licence_file_bsd3=no}"
  [ "$lic" = "classifier_bsd=yes licence_file_bsd3=yes" ] || return 1
  python -m pip install --quiet --no-deps --no-index --target "$RUNNER_TEMP/pypdf" "$whl" >/dev/null 2>&1 || return 1
  pypdf="$RUNNER_TEMP/pypdf"
  echo "pypdf_install target=runner_temp"
}

page() {
  p=$1
  pn=unread
  pc=no_pdf
  if [ "$k" = edge ]; then url=edge://policy/; else url=chrome://policy/; fi
  mkdir -p "$p/prof"
  ended=$(powershell -NoProfile -ExecutionPolicy Bypass -File "$(cygpath -w "$hdir/page.ps1")" \
    -Browser "$(cygpath -w "$b")" -ProfileDir "$(cygpath -w "$p/prof")" -Url "$url" \
    -Pdf "$(cygpath -w "$p/page.pdf")" 2>/dev/null | tr -d '\r' | sed -n 's/^page_processes_ended=\([0-9]*\)$/\1/p')
  if [ -s "$p/page.pdf" ]; then
    pc=no_pdf_reader
    if [ -n "$pypdf" ]; then
      PYTHONPATH="$pypdf" python -c 'import sys, pypdf; r = pypdf.PdfReader(sys.argv[1]); open(sys.argv[2], "w", encoding="utf-8").write("\n".join((x.extract_text() or "") for x in r.pages))' \
        "$p/page.pdf" "$p/page.txt" && pc=-
      [ "$pc" = - ] && ! grep -q '[^[:space:]]' "$p/page.txt" && pc=empty_text
      [ -s "$p/page.txt" ] && [ "$pc" = - ] && pn=$("$bin" --policy-named "$2" "$p/page.txt" "$3" | sed -n 's/.*named=\([a-z]*\).*/\1/p')
    fi
  fi
  if [ -n "${KEEP_PAGES:-}" ]; then
    mkdir -p "$PWD/target/ci-artifacts/pages"
    [ -f "$p/page.pdf" ] && cp "$p/page.pdf" "$PWD/target/ci-artifacts/pages/$k-$2.pdf"
    [ -f "$p/page.txt" ] && cp "$p/page.txt" "$PWD/target/ci-artifacts/pages/$k-$2.txt"
  fi
  echo "policy page browser=$k name=$2 page_named=${pn:-no} cause=$pc page_processes_ended=${ended:-none}"
}

call() {
  s0=$(date +%s)
  timeout "$LIMIT" sh tools/r3-trials/web/run.sh --candidate cdp --browser "$b" --port "$port" "$@" &
  current_call=$!
  wait "$current_call"
  e=$?
  current_call=""
  echo "call_end exit=$e seconds=$(($(date +%s) - s0)) port=$port args=$(echo "$*" | tr ' ' '_')"
  [ "$e" -eq 0 ] || code=1
}

ctl() {
  c=$(timeout 300 "$bin" --org-control "$b" "$(cygpath -w "$1")" 2>/dev/null | tr -d '\r' | tail -1)
  echo "org_control at=$2 browser=$k step=$3 ${c:-reply=no cause=none}"
}

cell() {
  key=$1
  step=$2
  name=$3
  type=$4
  data=$5
  want=$6
  cell_port=$7
  w=$(mktemp -d "$RUNNER_TEMP/org.XXXXXX")
  before=$(ctl "$w" before "$step")
  echo "$before"
  case "$before" in *" reply=yes"*) cb=reply ;; *) cb=none ;; esac
  policy_set "$key" "$name" "$type" "$data"
  powershell -NoProfile -Command "(Get-Acl 'HKLM:\\SOFTWARE\\Policies\\$key').Access | ForEach-Object { 'policy_acl ' + \$_.IdentityReference.Value.Replace(' ','_') + ' ' + (\$_.RegistryRights.ToString().Replace(' ','')) }"
  raw=$(MSYS_NO_PATHCONV=1 reg query "HKLM\\SOFTWARE\\Policies\\$key" /v "$name" 2>/dev/null | tr -d '\r' | awk -v n="$name" '$1 == n {print $NF}')
  case "$raw" in 0x*) vr=$((raw)) ;; *) vr=$raw ;; esac
  echo "policy value browser=$k name=$name value_read=${vr:--} value_want=$want"
  pl=$(page "$w" "$name" "$vr")
  echo "$pl"
  pn=$(printf '%s\n' "$pl" | sed -n 's/.* page_named=\([a-z]*\).*/\1/p')
  pcause=$(printf '%s\n' "$pl" | sed -n 's/^policy page .* cause=\([a-z_-]*\) .*/\1/p')
  if free_port "$cell_port"; then
    R3_ORG_CONTROL_BEFORE=$cb R3_ORG_VALUE_READ=$vr R3_ORG_VALUE_WANT=$want R3_ORG_PAGE_NAMED=$pn \
      R3_ORG_PAGE_CAUSE=$pcause call --runs "$RUNS" --rows org --round 3 --relay --step "$step"
  else
    echo "notrun candidate=cdp machine=hosted-win browser=$k step=$step cause=port_taken"
    code=1
  fi
  policy_unset
  ctl "$w" after "$step"
}

win_main() {
  hdir=$1
  [ -n "${RUNNER_TEMP:-}" ] || {
    echo "REFUSED usage: RUNNER_TEMP is not set" >&2
    exit 2
  }
  arm_undo
  host=$(rustc -vV | sed -n 's/^host: //p')
  bin="$PWD/tools/r3-trials/web/target/$host/release/web-trial.exe"
  if [ -x "$bin" ]; then echo "bin built=yes"; else echo "bin built=no"; fi
  pypdf_ready "$hdir" || echo "pypdf_ready no"
  port0=18190
  for pair in "C:/Program Files/Google/Chrome/Application/chrome.exe|chrome" \
    "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe|edge"; do
    b=${pair%%|*}
    k=${pair##*|}
    [ -f "$b" ] || {
      echo "browser missing $k"
      code=1
      continue
    }
    echo "browser $k"
    if [ "$k" = chrome ]; then pk='Google\Chrome'; else pk='Microsoft\Edge'; fi
    cell "$pk" org_debugging RemoteDebuggingAllowed REG_DWORD 0 0 "$port0"
    port0=$((port0 + 10))
    cell "$pk" org_proxy ProxyMode REG_SZ direct direct "$port0"
    port0=$((port0 + 10))
  done
  exit "$code"
}
