# shellcheck shell=sh

LIMIT=${LIMIT:-420}
RUNS=${RUNS:-10}
code=0

call() {
  s0=$(date +%s)
  timeout "$LIMIT" sh tools/r3-trials/web/run.sh --candidate cdp --browser "$b" --port 18190 "$@"
  e=$?
  echo "call_end exit=$e seconds=$(($(date +%s) - s0)) args=$(echo "$*" | tr ' ' '_')"
  [ "$e" -eq 0 ] || code=1
}

page() {
  p=$1
  pn=unread
  pc=no_pdf
  if [ "$k" = edge ]; then url=edge://policy/; else url=chrome://policy/; fi
  mkdir -p "$p/prof"
  timeout 60 "$b" --headless=new --user-data-dir="$p/prof" --allow-chrome-scheme-url --no-pdf-header-footer \
    --timeout=4000 --print-to-pdf="$p/page.pdf" "$url" >/dev/null 2>&1
  if [ -s "$p/page.pdf" ]; then
    pc=no_pdf_reader
    if python -c "import pypdf" 2>/dev/null || python -m pip install --quiet pypdf==4.3.1 >/dev/null 2>&1; then
      python -c 'import sys, pypdf; r = pypdf.PdfReader(sys.argv[1]); open(sys.argv[2], "w", encoding="utf-8").write("\n".join((x.extract_text() or "") for x in r.pages))' \
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
  prof=$(cygpath -w "$p/prof" | sed 's/\\/\\\\/g')
  left=$(powershell -NoProfile -Command "\$n = 0; Get-CimInstance Win32_Process | Where-Object { \$_.CommandLine -like '*$prof*' } | ForEach-Object { Stop-Process -Id \$_.ProcessId -Force -ErrorAction SilentlyContinue; \$n++ }; \$n" 2>/dev/null | tr -d '\r')
  echo "policy page browser=$k name=$2 page_named=${pn:-no} cause=$pc page_processes_ended=${left:-0}"
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
  w=$(mktemp -d "$RUNNER_TEMP/org.XXXXXX")
  before=$(ctl "$w" before "$step")
  echo "$before"
  case "$before" in *" reply=yes"*) cb=reply ;; *) cb=none ;; esac
  had=no
  MSYS_NO_PATHCONV=1 reg query "HKLM\\SOFTWARE\\Policies\\$key" >/dev/null 2>&1 && had=yes
  MSYS_NO_PATHCONV=1 reg add "HKLM\\SOFTWARE\\Policies\\$key" /v "$name" /t "$type" /d "$data" /f >/dev/null
  echo "policy set browser=$k name=$name key_existed=$had"
  powershell -NoProfile -Command "(Get-Acl 'HKLM:\\SOFTWARE\\Policies\\$key').Access | ForEach-Object { 'policy_acl ' + \$_.IdentityReference.Value.Replace(' ','_') + ' ' + (\$_.RegistryRights.ToString().Replace(' ','')) }"
  raw=$(MSYS_NO_PATHCONV=1 reg query "HKLM\\SOFTWARE\\Policies\\$key" /v "$name" 2>/dev/null | tr -d '\r' | awk -v n="$name" '$1 == n {print $NF}')
  case "$raw" in 0x*) vr=$((raw)) ;; *) vr=$raw ;; esac
  echo "policy value browser=$k name=$name value_read=${vr:--} value_want=$want"
  pl=$(page "$w" "$name" "$vr")
  echo "$pl"
  pn=$(printf '%s\n' "$pl" | sed -n 's/.* page_named=\([a-z]*\).*/\1/p')
  pcause=$(printf '%s\n' "$pl" | sed -n 's/^policy page .* cause=\([a-z_-]*\) .*/\1/p')
  R3_ORG_CONTROL_BEFORE=$cb R3_ORG_VALUE_READ=$vr R3_ORG_VALUE_WANT=$want R3_ORG_PAGE_NAMED=$pn \
    R3_ORG_PAGE_CAUSE=$pcause call --runs "$RUNS" --rows org --round 3 --relay --step "$step"
  MSYS_NO_PATHCONV=1 reg delete "HKLM\\SOFTWARE\\Policies\\$key" /v "$name" /f >/dev/null
  [ "$had" = no ] && MSYS_NO_PATHCONV=1 reg delete "HKLM\\SOFTWARE\\Policies\\$key" /f >/dev/null
  if MSYS_NO_PATHCONV=1 reg query "HKLM\\SOFTWARE\\Policies\\$key" /v "$name" >/dev/null 2>&1; then
    echo "policy removed=no name=$name"
  else
    echo "policy removed=yes name=$name"
  fi
  ctl "$w" after "$step"
}

win_main() {
  [ -n "${RUNNER_TEMP:-}" ] || {
    echo "REFUSED usage: RUNNER_TEMP is not set" >&2
    exit 2
  }
  host=$(rustc -vV | sed -n 's/^host: //p')
  bin="$PWD/tools/r3-trials/web/target/$host/release/web-trial.exe"
  if [ -x "$bin" ]; then echo "bin built=yes"; else echo "bin built=no"; fi
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
    cell "$pk" org_debugging RemoteDebuggingAllowed REG_DWORD 0 0
    cell "$pk" org_proxy ProxyMode REG_SZ direct direct
  done
  exit "$code"
}
