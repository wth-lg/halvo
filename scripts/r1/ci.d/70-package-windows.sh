#!/bin/sh
# jobs: windows
# rule: The one command npm run tauri build -- -- --locked, started with no set variable whose name starts with APPLE_ or TAURI_SIGNING_ or is HALVO_WINDOWS_SIGN_COMMAND and with no tracked file under src-tauri that names externalBin, signCommand or certificateThumbprint, and tried three times at most, makes exactly one installer *-setup.exe under release/bundle/nsis of the target directory, newer than the start of the build, whose manifest asks for the level asInvoker at each requestedExecutionLevel and for no other, and which the 7-Zip of the runner's image unpacks, with no install, into files that hold one halvo.exe with each helper that packaging/helpers.d names, with .exe, in its directory, each read for a number of bytes above zero before anything starts it, and no other file in that directory whose name ends in .exe but the uninstall.exe of the installer template, and 62-build.sh --files passes on those executables (no stand-in or test-seam marker, and the version by 61-version.sh --check, which starts each with --version, but for a helper whose file of packaging/helpers.d says version=none, which is read and not started), and that directory holds THIRD-PARTY-NOTICES.txt as a file with the bytes that scripts/notices/generate.js writes for the host triple in the same run, in which grep -i -c zetetic counts at least 1. Then src-tauri/windows/hooks.nsh waits for each helper of packaging/helpers.d and defines the box of the hook under another account, the sentence of a crash report that stays and the sentence of a data folder that the delete cannot use word for word as ruled and sets its exit level in a silent run alone, makes no stop of the capture service at an uninstall under another account, holds the running account's Run value Halvo, where the folder of its program is not the folder that goes, and its uninstall entry in its first hook of the uninstall behind the delete, and writes them back first in its last hook, src-tauri/windows/English.nsh gives the uninstaller's box the ruled label, and on a GitHub-hosted runner alone: the installer, run with /S into a scratch directory while the values pv of the WebView2 runtime that the template reads are moved aside, puts halvo.exe, each helper, the notices, uninstall.exe and WEBVIEW2-RUNTIME-MISSING.txt with sentence (1) there, with an uninstall entry under HKCU alone; the installed app started with --background on a provisioned scratch directory runs with exactly one halvo-capture.exe child from the install directory, ctl status reads store=open, and after ctl stop exits 0 no halvo-capture.exe from there runs for 10 s; an install over the copy while its halvo-owner.exe is held exits with a code other than 0 and writes no uninstall.exe; with a file at the name of the default data directory halvo-capture --delete-all exits 66 and uninstall.exe /S /DELETEALL exits 2 and takes nothing away; with two reports planted in the folder that halvo.exe --remove-crash-reports names and one of them held open, uninstall.exe /S exits 3, removes the program and the report that is not held, and an install again exits 0; an install into a second folder exits 0 and makes the uninstall entry name that folder; while the app started from the second folder with --background alone runs as below, the uninstall entry's InstallLocation turned to another folder makes the first copy's uninstall.exe /S /DELETEALL exit 2 with nothing taken away and the app running with its child, and the entry then names the second folder again; the first copy's uninstall.exe /S then exits 0, leaves no file in the first folder but uninstall.exe, and leaves the Run value, every value of the uninstall entry and the app of the second folder with the same halvo-capture.exe child as they stood before it; and while the app started from the second folder with --background alone runs with its child on the default data directory that halvo-capture --default-data-dir names, which the check makes and provisions, with a file planted in EBWebView beside it and the Run value Halvo planted, the second copy's uninstall.exe /S /DELETEALL exits 0 and leaves no process from the second folder, no file there but uninstall.exe, no notices, no Run value, no uninstall entry, no default data directory and no planted file.
set -u
name=70-package-windows
config_keys='externalBin signCommand certificateThumbprint'
attempts=3
# The uninstaller that the NSIS template of the Tauri bundler writes; 7-Zip unpacks it beside halvo.exe.
template_executables='uninstall.exe'

fail() {
  if [ -n "${cycling:-}" ]; then
    show_found
    cleanup
  fi
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

signing_defect() {
  environment=$(env) || { printf 'env could not list the environment\n'; return 1; }
  set_name=$(printf '%s\n' "$environment" | LC_ALL=C awk -F = '/^(APPLE_|TAURI_SIGNING_)[A-Za-z0-9_]*=./ || /^HALVO_WINDOWS_SIGN_COMMAND=./ { print $1; exit }')
  [ -z "$set_name" ] && return 0
  printf 'the signing variable %s is set, and this check builds unsigned\n' "$set_name"
  return 1
}

config_defect() {
  for key in $config_keys; do
    found=$(git -C "$1" grep -l -F -e "$key" -- src-tauri)
    status=$?
    case $status in
      0) ;;
      1) continue ;;
      *) printf 'git grep could not read the tracked files under src-tauri\n'; return 1 ;;
    esac
    case $key in
      externalBin) why='scripts/tauri.js gives the build its external binaries for that build alone, and cargo test fails on a clean checkout when a tracked configuration names one' ;;
      *) why='the installer is built unsigned, and sign_step in scripts/tauri.js is the place of signing' ;;
    esac
    printf 'the tracked file %s names %s; %s\n' "${found%%
*}" "$key" "$why"
    return 1
  done
}

helper_names() {
  for file in packaging/helpers.d/*; do
    if [ -f "$file" ]; then printf '%s\n' "${file##*/}"; fi
  done
}

installers() {
  for file in "$1"/*-setup.exe; do
    if [ -f "$file" ]; then printf '%s\n' "$file"; fi
  done
}

installer_defect() {
  found=$(installers "$1")
  count=$(printf '%s\n' "$found" | grep -c .)
  [ "$count" != 0 ] || { printf 'the build made no installer *-setup.exe under release/bundle/nsis\n'; return 1; }
  [ "$count" = 1 ] || { printf 'release/bundle/nsis holds %s installers *-setup.exe, and this check reads exactly one\n' "$count"; return 1; }
  newer=$(find "$found" -newer "$2") || { printf 'find could not compare the installer with the start of the build\n'; return 1; }
  [ -n "$newer" ] || { printf '%s is older than the start of this build\n' "${found##*/}"; return 1; }
  printf '%s\n' "$found"
}

manifest_defect() {
  elements=$(LC_ALL=C grep -a -o -E '<requestedExecutionLevel[^>]*>' "$1")
  [ -n "$elements" ] || { printf '%s carries no requestedExecutionLevel in its manifest\n' "${1##*/}"; return 1; }
  levels=$(printf '%s\n' "$elements" | sed -n 's/.*[[:space:]]level="\([^"]*\)".*/\1/p')
  element_count=$(printf '%s\n' "$elements" | grep -c .)
  level_count=$(printf '%s\n' "$levels" | grep -c .)
  [ "$level_count" = "$element_count" ] || { printf 'a requestedExecutionLevel of the manifest of %s names no level\n' "${1##*/}"; return 1; }
  other=$(printf '%s\n' "$levels" | grep -v -x -F asInvoker | head -n 1)
  [ -z "$other" ] || { printf 'the manifest of %s asks for the level %s, and a per-user installer asks for asInvoker alone\n' "${1##*/}" "$other"; return 1; }
  printf 'installer manifest: requestedExecutionLevel asInvoker, %s of %s\n' "$level_count" "$element_count"
}

content_defect() {
  content=$1
  shift
  apps=$(find "$content" -type f -name halvo.exe) || { printf 'find could not list the unpacked installer\n'; return 1; }
  count=$(printf '%s\n' "$apps" | grep -c .)
  if [ "$count" != 1 ]; then
    executables=$(cd "$content" && find . -type f -name '*.exe' | sed 's|^\./||' | sort | paste -s -d ' ' -)
    printf 'the installer holds %s files halvo.exe, and this check reads exactly one (its executables: %s)\n' "$count" "${executables:-none}"
    return 1
  fi
  for helper in "$@"; do
    [ -f "${apps%/*}/$helper.exe" ] || { printf 'the installer holds no %s.exe beside halvo.exe\n' "$helper"; return 1; }
  done
  # Every entry beside halvo.exe whose name ends in .exe in any case, of any type and with a dot name
  # too, is halvo.exe, a helper with .exe or a file of the installer template.
  for entry in "${apps%/*}"/* "${apps%/*}"/.[!.]* "${apps%/*}"/..?*; do
    [ -e "$entry" ] || [ -L "$entry" ] || continue
    case ${entry##*/} in
      *.[eE][xX][eE]) ;;
      *) continue ;;
    esac
    for executable in halvo "$@"; do
      [ "${entry##*/}" = "$executable.exe" ] && continue 2
    done
    for executable in $template_executables; do
      [ "${entry##*/}" = "$executable" ] && continue 2
    done
    printf 'the installer holds %s beside halvo.exe, which is neither halvo.exe, a helper of packaging/helpers.d with .exe nor a file of the installer template (%s)\n' "${entry##*/}" "$template_executables"
    return 1
  done
  printf '%s\n' "${apps%/*}"
}

bytes_of() {
  read_bytes=$(wc -c <"$1") || { printf 'wc could not read %s\n' "${1##*/}"; return 1; }
  read_bytes=$(printf '%s' "$read_bytes" | tr -d '[:blank:]')
  case $read_bytes in
    '' | *[!0-9]*) printf 'wc gave no number of bytes for %s\n' "${1##*/}"; return 1 ;;
  esac
  [ "$read_bytes" -gt 0 ] || { printf '%s holds no byte\n' "${1##*/}"; return 1; }
  printf '%s\n' "$read_bytes"
}

release_defect() {
  directory=$1
  shift
  names=$*
  set -- "$directory/halvo.exe"
  for helper in $names; do
    set -- "$@" "$directory/$helper.exe"
  done
  read_output=$(sh scripts/r1/ci.d/62-build.sh --files "$@")
  status=$?
  if [ "$status" -eq 0 ]; then
    printf '%s\n' "$read_output" | grep -v -E '^(PASS|FAIL) 62-build'
    return 0
  fi
  cause=$(printf '%s\n' "$read_output" | sed -n 's/^FAIL 62-build (\(.*\))$/\1/p')
  printf '62-build --files: %s\n' "${cause:-exit $status}"
  return 1
}

# The notices: the directory $1 of halvo.exe in the unpacked installer holds THIRD-PARTY-NOTICES.txt
# as a file with the bytes that scripts/notices/generate.js writes for the host triple now, into the
# file $2, and the file names Zetetic, the holder of SQLCipher, on at least one line.
notices_defect() {
  notices=$1/THIRD-PARTY-NOTICES.txt
  if [ -L "$notices" ] || [ ! -f "$notices" ]; then
    printf 'the installer holds no file THIRD-PARTY-NOTICES.txt beside halvo.exe\n'
    return 1
  fi
  host=$(rustc -vV | sed -n 's/^host: //p')
  [ -n "$host" ] || { printf 'rustc -vV names no host triple\n'; return 1; }
  node scripts/notices/generate.js --triple "$host" >"$2" 2>"$CI_D_ARTIFACTS/notices.err" || { printf 'scripts/notices/generate.js exited %s; see notices.err\n' "$?"; return 1; }
  cmp -s "$notices" "$2" || { printf 'the THIRD-PARTY-NOTICES.txt of the installer is not the file that scripts/notices/generate.js writes for %s\n' "$host"; return 1; }
  zetetic=$(grep -i -c zetetic "$notices")
  [ "$zetetic" -ge 1 ] || { printf 'the THIRD-PARTY-NOTICES.txt of the installer names no Zetetic, the holder of SQLCipher\n'; return 1; }
  bytes=$(wc -c <"$notices" | tr -d ' ')
  printf 'installer holds THIRD-PARTY-NOTICES.txt beside halvo.exe, bytes=%s, zetetic lines: %s\n' "$bytes" "$zetetic"
}

# The hooks of the installer (src-tauri/windows/hooks.nsh): the file $1 waits for each helper of
# the list after it, by a line !insertmacro HALVO_HELD "<helper>.exe" of its stop.
hooks_defect() {
  hooks=$1
  shift
  [ -f "$hooks" ] || { printf 'there is no file %s\n' "$hooks"; return 1; }
  for helper in "$@"; do
    found=$(sed 's/^[[:space:]]*//' "$hooks" | grep -c -x -F "!insertmacro HALVO_HELD \"$helper.exe\"")
    [ "$found" -ge 1 ] || { printf '%s waits for no %s.exe of packaging/helpers.d before an uninstall\n' "$hooks" "$helper"; return 1; }
  done
}

# The texts that a person reads, to the letter of their rulings: in the hooks file $1 the box of
# the hook under another account, the sentence of a crash report that stays and the sentence of a
# data folder that the delete cannot use; in the language file $2 the label of the uninstaller's
# box, which the template draws in one line.
texts_defect() {
  for key in OTHER_ACCOUNT REPORT_STAYS DATA_FOLDER_UNUSABLE; do
    case $key in
      OTHER_ACCOUNT) text=$sentence_other_account ;;
      REPORT_STAYS) text=$sentence_report_stays ;;
      *) text=$sentence_data_folder_unusable ;;
    esac
    found=$(grep -c -x -F -e "!define HALVO_SENTENCE_$key \"$text\"" "$1")
    [ "$found" = 1 ] || { printf '%s defines HALVO_SENTENCE_%s in other words than its ruling\n' "$1" "$key"; return 1; }
  done
  # shellcheck disable=SC2016
  label_line='LangString deleteAppData ${LANG_ENGLISH} "'"$label_of_the_box"'"'
  found=$(grep -c -x -F -e "$label_line" "$2")
  [ "$found" = 1 ] || { printf '%s gives the box of the uninstaller another label than its ruling, %s\n' "$2" "$label_of_the_box"; return 1; }
}

# The exit level of the hooks file $1: at least one line sets it, and each line that sets it sets 3
# in a silent run alone, in the form of level_line, since the installer of the template reads every
# exit but 0 and 1 of an uninstall with windows as a failed uninstall.
# shellcheck disable=SC2016
level_line='${IfThen} ${Silent} ${|} SetErrorLevel ${HALVO_EXIT_REPORT_STAYS} ${|}'
level_defect() {
  code_lines=$(grep -v '^[[:space:]]*;' "$1") || { printf 'there is no code in %s\n' "$1"; return 1; }
  set_lines=$(printf '%s\n' "$code_lines" | grep -c 'SetErrorLevel')
  silent_lines=$(printf '%s\n' "$code_lines" | sed 's/^[[:space:]]*//' | grep -c -x -F -e "$level_line")
  if [ "$set_lines" -lt 1 ] || [ "$silent_lines" != "$set_lines" ]; then
    printf '%s sets an exit level outside a silent run (%s lines set one, %s in a silent run alone)\n' "$1" "$set_lines" "$silent_lines"
    return 1
  fi
}

# The folder that the hooks file $1 names where the form does not start or its line names no
# folder: the folder of Halvo's window alone, by the template's expression, on the one line that
# names a folder by $LOCALAPPDATA for the report's sentence; never the folder of the app's id, which
# holds the data that a person who did not tick the box kept.
# shellcheck disable=SC2016
window_folder_line='StrCpy $HalvoReportFolder "$LOCALAPPDATA\${BUNDLEID}\EBWebView"'
window_folder_defect() {
  # shellcheck disable=SC2016
  named=$(sed 's/^[[:space:]]*//' "$1" | grep -c -F -e 'StrCpy $HalvoReportFolder "$LOCALAPPDATA')
  window=$(sed 's/^[[:space:]]*//' "$1" | grep -c -x -F -e "$window_folder_line")
  if [ "$named" != 1 ] || [ "$window" != 1 ]; then
    printf '%s names another folder than the folder of Halvo'"'"'s window where the form names none\n' "$1"
    return 1
  fi
}

# The lines of the macro $2 of the hooks file $1 in their order, each without the blanks around it,
# with no comment line and no empty line.
macro_lines() {
  awk -v m="$2" '
    { t = $0; sub(/^[ \t]+/, "", t); sub(/[ \t\r]+$/, "", t) }
    !inside && (t == "!macro " m || index(t, "!macro " m " ") == 1) { inside = 1; next }
    inside && t == "!macroend" { exit }
    inside && t != "" && substr(t, 1, 1) != ";" { print t }
  ' "$1"
}

# Exit 0 where the lines of standard input hold the lines $1, $2, ... one right after the other,
# exactly once.
holds_once() {
  WANT=$(printf '%s\n' "$@") awk '
    BEGIN { n = split(ENVIRON["WANT"], w, "\n") }
    { line[NR] = $0 }
    END {
      for (i = 1; i + n - 1 <= NR; i++) {
        k = 0
        while (k < n && line[i + k] == w[k + 1]) k++
        if (k == n) found++
      }
      exit found == 1 ? 0 : 1
    }'
}

# No stop of the capture service under another account: in the hooks file $1 the one line of code
# that calls ctl stop stands in the macro HALVO_STOP_SERVICE, and the first hook of the uninstall
# inserts that macro once, inside the test of the account's own install. The call names no data
# directory, so it reaches the capture service of the account that runs the uninstaller.
# shellcheck disable=SC2016
stop_line='nsExec::Exec '"'"'"$INSTDIR\halvo-capture.exe" ctl stop'"'"
stop_defect() {
  calls=$(grep -v '^[[:space:]]*;' "$1" | grep -c -F -e 'ctl stop')
  inserts=$(macro_lines "$1" NSIS_HOOK_PREUNINSTALL | grep -c -x -F -e '!insertmacro HALVO_STOP_SERVICE')
  # shellcheck disable=SC2016
  if [ "$calls" = 1 ] && [ "$inserts" = 1 ] \
    && macro_lines "$1" HALVO_STOP_SERVICE | holds_once "$stop_line" \
    && macro_lines "$1" NSIS_HOOK_PREUNINSTALL | holds_once '${If} $HalvoOwnInstall = 1' '!insertmacro HALVO_STOP_SERVICE' '${EndIf}'; then
    return 0
  fi
  printf '%s stops the capture service at an uninstall under another account\n' "$1"
  return 1
}

# The order of the hooks: the first hook of the uninstall holds the running account's entries
# behind its delete and its form, before the template's steps take them away, and the last hook
# writes them back first, behind those steps; each macro is inserted there alone.
write_back_defect() {
  holds=$(grep -c -x -E '[[:space:]]*!insertmacro HALVO_HOLD_ENTRIES[[:space:]]*' "$1")
  writes=$(grep -c -x -E '[[:space:]]*!insertmacro HALVO_WRITE_BACK_ENTRIES[[:space:]]*' "$1")
  first=$(macro_lines "$1" NSIS_HOOK_POSTUNINSTALL | sed -n 1p)
  if [ "$holds" = 1 ] && [ "$writes" = 1 ] && [ "$first" = '!insertmacro HALVO_WRITE_BACK_ENTRIES' ] \
    && macro_lines "$1" NSIS_HOOK_PREUNINSTALL | holds_once '!insertmacro HALVO_DELETE_EVERYTHING' '!insertmacro HALVO_REMOVE_CRASH_REPORTS' '!insertmacro HALVO_HOLD_ENTRIES'; then
    return 0
  fi
  printf '%s does not hold the running account'"'"'s entries in its first hook of the uninstall and write them back first in its last\n' "$1"
  return 1
}

# The Run value comes back only where it names no program in the folder that goes: in the hooks
# file $1 the mark that holds it stands once, behind the test that the folder of the program that
# the value names, in one spelling, is not the install folder in that spelling.
run_value_defect() {
  # shellcheck disable=SC2016
  marks=$(grep -c -F -e 'StrCpy $HalvoRunHeld 1' "$1")
  # shellcheck disable=SC2016
  if [ "$marks" = 1 ] \
    && macro_lines "$1" HALVO_HOLD_ENTRIES | holds_once 'StrCpy $HalvoRunValue $R9' '!insertmacro HALVO_FOLDER_OF_THE_RUN_VALUE' 'StrCpy $R7 $INSTDIR' '!insertmacro HALVO_ONE_SPELLING $R7' '${If} $R4 == ""' '${OrIf} $R4 != $R7' 'StrCpy $HalvoRunHeld 1' '${EndIf}'; then
    return 0
  fi
  printf '%s holds the Run value also where it names the program in the folder that goes\n' "$1"
  return 1
}

# The install, start, status, stop and uninstall of the built installer. Each helper below is the
# hosted runner's own; the fixture of the self-test puts stand-ins in their place.
sentence_runtime='Halvo cannot open its window yet: the Microsoft Edge WebView2 Runtime is not on this computer. This installer downloads nothing. Install the runtime from Microsoft, then start Halvo.'
runtime_file=WEBVIEW2-RUNTIME-MISSING.txt
# The sentences of the hooks that a person reads and the label of the uninstaller's box, to the
# letter of their rulings.
sentence_other_account='This account has no record of installing this copy of Halvo, so the uninstaller deletes no Halvo data. The Halvo data and the start entry of the person who installed it stay on this computer, and so does any Halvo data of your own. Choose OK to remove the program only, or Cancel to stop.'
sentence_report_stays='Halvo was uninstalled, but a crash report of its own window could not be removed: {folder}. It can hold what the window showed. Remove it yourself.'
sentence_data_folder_unusable='Halvo could not use its data folder, so nothing was deleted and Halvo is still installed: {folder}. You can remove that folder yourself and run the uninstaller again, or run it again without the box to remove the program only.'
label_of_the_box="Also delete Halvo's data. Copies next to your files stay."
run_key='HKCU\Software\Microsoft\Windows\CurrentVersion\Run'
uninstall_key='Software\Microsoft\Windows\CurrentVersion\Uninstall\Halvo'
# The two values that the template's section WebView2 reads on a 64-bit Windows, in its order.
runtime_keys='HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5} HKCU\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
start_s=60
answer_s=30
lease_s=30
stop_hold_s=10
end_s=10
poll_s=1

native_path() {
  cygpath -w -l "$1"
}

posix_path() {
  cygpath -u "$1"
}

# The processes of the machine as the system names them, read with the directory $1 (native), both
# paths in one spelling, the long form that GetLongPathName gives: with $2 = in, every process
# whose executable lies in $1, one line each with its name, its process id, the id of its parent
# and its command line; with $2 = all, the long form of $1 and then every process whose name starts
# with halvo, with its executable's path as the system gives it and in its long form.
list_ps() {
  script=$work/list.ps1
  [ -f "$script" ] || cat >"$script" <<'EOF' || return 1
param([string]$Dir, [string]$Mode)
$ErrorActionPreference = "Stop"
Add-Type -Namespace HalvoCheck -Name Paths -MemberDefinition '[DllImport("kernel32.dll", CharSet = CharSet.Unicode)] public static extern uint GetLongPathName(string shortPath, System.Text.StringBuilder longPath, uint size);'
function Long([string]$Path) {
  $buffer = New-Object System.Text.StringBuilder 32768
  $n = [HalvoCheck.Paths]::GetLongPathName($Path, $buffer, 32768)
  if ($n -gt 0 -and $n -lt 32768) { return $buffer.ToString() }
  return $Path
}
$dirLong = (Long $Dir).TrimEnd('\')
$prefix = $dirLong + '\'
if ($Mode -eq 'all') { [Console]::Out.WriteLine(("dir given={0} long={1}" -f $Dir, $dirLong)) }
foreach ($p in Get-CimInstance -ClassName Win32_Process) {
  $exe = $p.ExecutablePath
  $long = ''
  if ($exe) { $long = Long $exe }
  if ($Mode -eq 'all') {
    if ($p.Name -like 'halvo*') {
      [Console]::Out.WriteLine(("{0} {1} {2} path={3} long={4}" -f $p.Name, $p.ProcessId, $p.ParentProcessId, $exe, $long))
    }
  } elseif ($long -and $long.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    [Console]::Out.WriteLine(("{0} {1} {2} {3}" -f $p.Name, $p.ProcessId, $p.ParentProcessId, $p.CommandLine))
  }
}
EOF
  native=$(native_path "$script") || return 1
  powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$native" -Dir "$1" -Mode "$2" >"$work/list.raw" || return 1
  tr -d '\r' <"$work/list.raw"
}

list_from() {
  list_ps "$1" in
}

list_all() {
  list_ps "$1" all
}

# The Windows process id of the process $1 that this shell started, from MSYS's /proc.
winpid_of() {
  tries=0
  while [ "$tries" -lt 20 ]; do
    if [ -r "/proc/$1/winpid" ]; then
      cat "/proc/$1/winpid"
      return 0
    fi
    sleep 0.1
    tries=$((tries + 1))
  done
  return 1
}

# Ends the process $1 with every process below it.
end_tree() {
  taskkill //F //T //PID "$1" >/dev/null 2>&1
}

# The shortcuts that an install makes for a person, where the uninstall takes them away.
shortcuts() {
  desktop=$(cygpath -D) || return 1
  programs=$(cygpath -P) || return 1
  printf '%s\n' "$desktop/Halvo.lnk" "$programs/Halvo.lnk"
}

# Where a stage fails: every process whose name starts with halvo, with its path as the system gives
# it and in its long form, the install directory as this check spells it, and the end of each
# app's standard error, or that the app wrote no line.
show_found() {
  printf 'found: the install directory as this check spells it: %s\n' "${native_install:-none}"
  if list_all "${native_install:-.}" >"$work/found" 2>"$work/found.err"; then
    sed 's/^/found: /' "$work/found"
  else
    cause=$(head -n 3 "$work/found.err" | tr '\n' ' ')
    printf 'found: the list of processes could not be read: %s\n' "$cause"
  fi
  for label in app-1 app-2; do
    [ -f "$work/$label.out" ] || continue
    if [ -s "$work/$label.out" ]; then
      tail -n 20 "$work/$label.out" | tr -d '\r' | LC_ALL=C tr -c '[:print:]\n' '?' | sed "s/^/found: $label err: /"
    else
      printf 'found: %s err: no line\n' "$label"
    fi
  done
}

# Ends each app that this check started, by its process id with the processes below it, where the
# system still names that id halvo.exe.
end_started() {
  [ -n "${started:-}" ] || return 0
  list_all "${native_install:-.}" >"$work/all" 2>/dev/null || return 1
  for pid in $started; do
    if awk -v p="$pid" '$1 == "halvo.exe" && $2 == p { found = 1 } END { exit !found }' "$work/all"; then
      end_tree "$pid"
    fi
  done
}

end_process() {
  taskkill //F //PID "$1" >/dev/null 2>&1
}

take_away() {
  rm -rf "$@"
}

reg_data() {
  MSYS_NO_PATHCONV=1 reg query "$1" /v "$2" >"$work/reg.out" 2>/dev/null || return 1
  tr -d '\r' <"$work/reg.out" | sed -n "s/^[[:space:]]*$2[[:space:]]*REG_SZ[[:space:]]*//p"
}

reg_put() {
  MSYS_NO_PATHCONV=1 reg add "$1" /v "$2" /t REG_SZ /d "$3" /f >/dev/null 2>&1
}

reg_drop() {
  MSYS_NO_PATHCONV=1 reg delete "$1" /v "$2" /f >/dev/null 2>&1
}

# Whether the uninstall entry of Halvo stands under the hive $1 (HKCU or HKLM).
uninstall_entry() {
  MSYS_NO_PATHCONV=1 reg query "$1\\$uninstall_key" >/dev/null 2>&1
}

uninstall_entry_drop() {
  MSYS_NO_PATHCONV=1 reg delete "HKCU\\$uninstall_key" /f >/dev/null 2>&1
}

# Every value of the uninstall entry of Halvo under HKCU, one line each with its name, kind and
# data, sorted; exit 1 where the entry does not stand.
entry_values() {
  MSYS_NO_PATHCONV=1 reg query "HKCU\\$uninstall_key" >"$work/entry.out" 2>/dev/null || return 1
  tr -d '\r' <"$work/entry.out" | grep '^    ' | sort
}

# Moves the values pv that the template reads aside, into $work/runtime-moved, for the next install.
runtime_hide() {
  : >"$work/runtime-moved" || return 1
  for key in $runtime_keys; do
    data=$(reg_data "$key" pv) || continue
    [ -n "$data" ] || continue
    printf '%s|%s\n' "$key" "$data" >>"$work/runtime-moved" || return 1
    reg_drop "$key" pv || return 1
  done
}

runtime_put_back() {
  [ -f "$work/runtime-moved" ] || return 0
  while IFS='|' read -r key data; do
    reg_put "$key" pv "$data" || return 1
    back=$(reg_data "$key" pv) || return 1
    [ "$back" = "$data" ] || return 1
  done <"$work/runtime-moved"
  mv "$work/runtime-moved" "$work/runtime-put-back"
}

# Holds the file $1 open for reading alone, so that no other process opens it for writing, until
# release_file; the holder writes its process id to $work/holder when it holds.
hold_file() {
  script=$work/hold.ps1
  cat >"$script" <<'EOF' || return 1
param([string]$File, [string]$Ready)
$held = [System.IO.File]::Open($File, 'Open', 'Read', 'Read')
[System.IO.File]::WriteAllText($Ready, "$PID")
Start-Sleep -Seconds 120
$held.Close()
EOF
  native=$(native_path "$script") || return 1
  native_file=$(native_path "$1") || return 1
  native_work=$(native_path "$work") || return 1
  powershell.exe -NoLogo -NoProfile -NonInteractive -ExecutionPolicy Bypass -File "$native" -File "$native_file" -Ready "$native_work\\holder" >/dev/null 2>&1 &
}

release_file() {
  [ -s "$work/holder" ] || return 0
  holder=$(tr -d '\r' <"$work/holder") || return 1
  end_process "$holder"
  mv "$work/holder" "$work/holder-ended"
}

# The halvo.exe of the install directory and its halvo-capture.exe children of the listing in
# $work/procs whose command line holds $1: it sets apps, children and pair (the app and its one
# child, or empty).
pair_of() {
  apps=$(awk -v t="$1" '$1 == "halvo.exe" && index($0, t) { print $2 }' "$work/listing")
  children=$(awk -v t="$1" '$1 == "halvo-capture.exe" && index($0, t) { print $2 " " $3 }' "$work/listing")
  pair=
  n_apps=$(printf '%s\n' "$apps" | grep -c .)
  n_children=$(printf '%s\n' "$children" | grep -c .)
  [ "$n_apps" = 1 ] && [ "$n_children" = 1 ] || return 0
  [ "${children#* }" = "$apps" ] && pair="$apps ${children% *}"
}

# Waits up to start_s for the app started with $2 in its command line to run with exactly one
# halvo-capture.exe child from the install directory, and up to answer_s more for ctl status, on
# the data directory $3 or without --data-dir where $3 is empty, to read the store open.
wait_pair() {
  began=$(date +%s) || fail "the clock cannot be read"
  while :; do
    list_from "$native_install" >"$work/listing" || fail "the list of processes cannot be read"
    pair_of "$2"
    [ -n "$pair" ] && break
    now=$(date +%s) || fail "the clock cannot be read"
    [ $((now - began)) -lt "$start_s" ] || fail "the installed app ($1) runs with $n_children halvo-capture.exe children from the install directory after $start_s s, not one ($n_apps apps from there)"
    sleep "$poll_s"
  done
  printf 'cycle %s: the app %s runs with one halvo-capture.exe child %s from the install directory\n' "$1" "${pair% *}" "${pair#* }"
  while :; do
    if [ -n "$3" ]; then
      "$install/halvo-capture.exe" ctl --data-dir "$3" status >"$work/status.out" 2>&1
    else
      "$install/halvo-capture.exe" ctl status >"$work/status.out" 2>&1
    fi && tr -d '\r' <"$work/status.out" | grep -q -x 'store=open' && break
    now=$(date +%s) || fail "the clock cannot be read"
    [ $((now - began)) -lt $((start_s + answer_s)) ] || fail "ctl status of the installed app ($1) did not read store=open"
    sleep "$poll_s"
  done
  printf 'cycle %s: ctl status reads store=open\n' "$1"
}

# Waits up to lease_s for ctl status, on the data directory $2 or without --data-dir where $2 is
# empty, to read lease=fresh: the app has read a Status of its child and takes it as its own, so a
# stop is no longer followed by a child that the app starts again. At the bound the stage fails with
# the status's lines.
wait_lease() {
  waited_from=$(date +%s) || fail "the clock cannot be read"
  while :; do
    if [ -n "$2" ]; then
      "$install/halvo-capture.exe" ctl --data-dir "$2" status >"$work/lease.out" 2>&1
    else
      "$install/halvo-capture.exe" ctl status >"$work/lease.out" 2>&1
    fi
    tr -d '\r' <"$work/lease.out" | grep -q -x 'lease=fresh' && break
    now=$(date +%s) || fail "the clock cannot be read"
    if [ $((now - waited_from)) -ge "$lease_s" ]; then
      tr -d '\r' <"$work/lease.out" | sed 's/^/found: status: /'
      fail "ctl status of the installed app ($1) did not read lease=fresh within $lease_s s"
    fi
    sleep "$poll_s"
  done
  now=$(date +%s) || fail "the clock cannot be read"
  printf 'cycle %s: ctl status read lease=fresh after %s s, the app takes its child as its own\n' "$1" "$((now - waited_from))"
}

# Ends each process from the install directory by its id and waits up to end_s for none.
end_installed() {
  waited=0
  while [ "$waited" -le "$end_s" ]; do
    list_from "$native_install" >"$work/listing" 2>/dev/null || return 1
    [ -s "$work/listing" ] || return 0
    awk '{ print $2 }' "$work/listing" | while read -r pid; do end_process "$pid"; done
    sleep 1
    waited=$((waited + 1))
  done
  return 1
}

# Takes away what the cycle planted, also where it fails: the apps it started, by their ids, and
# every process from the install directory, the held file, the values pv, the Run value with its
# own text, the uninstall entry, the shortcuts that its install made, its install and the default
# data directory it made; and it keeps the standard output and error of each app that it started
# among the artifacts.
cleanup() {
  trap - EXIT
  [ -n "${cycling:-}" ] || return 0
  cycling=
  end_started || printf 'cycle: the apps that the check started could not be read\n'
  end_installed || printf 'cycle: a process from the install directory still runs at the end\n'
  release_file
  runtime_put_back || printf 'cycle: the values pv of the WebView2 runtime could not all be put back\n'
  if [ -n "${planted_run:-}" ] && run_now=$(reg_data "$run_key" Halvo) && [ "$run_now" = "$planted_run" ]; then
    reg_drop "$run_key" Halvo
  fi
  if [ -d "$install" ] && uninstall_entry HKCU; then
    uninstall_entry_drop
  fi
  if [ -f "$work/made-links" ]; then
    while IFS= read -r link; do
      [ -f "$link" ] && take_away "$link"
    done <"$work/made-links"
  fi
  [ -n "${made_default:-}" ] && [ -d "$made_default" ] && take_away "$made_default"
  [ -n "${planted:-}" ] && [ -f "$planted" ] && take_away "$planted"
  for file in "${unusable:-}" "${report_goes:-}" "${report_stays:-}"; do
    [ -n "$file" ] && [ -f "$file" ] && take_away "$file"
  done
  if [ -n "${aside:-}" ] && [ -e "$aside" ] && [ ! -e "$default" ]; then
    mv "$aside" "$default"
  fi
  for label in app-1 app-2; do
    if [ -n "${CI_D_ARTIFACTS:-}" ] && [ -f "$work/$label.out" ]; then
      cp "$work/$label.out" "$CI_D_ARTIFACTS/$label.out" || printf 'cycle: %s.out could not be kept among the artifacts\n' "$label"
    fi
  done
  take_away "$work/installed"
}

cycle() {
  installer=$1
  work=$2
  install=$work/installed/Halvo
  mkdir -p "$install" || fail "mkdir failed"
  native_install=$(native_path "$install") || fail "the install directory has no native path"
  planted_run=
  made_default=
  planted=
  unusable=
  aside=
  report_goes=
  report_stays=
  started=
  cycling=yes
  trap cleanup EXIT
  shortcuts >"$work/links" || fail "the folders of the shortcuts cannot be named"
  : >"$work/made-links" || fail "the list of the shortcuts cannot be written"
  while IFS= read -r link; do
    [ -e "$link" ] || printf '%s\n' "$link" >>"$work/made-links"
  done <"$work/links"
  runtime_hide || fail "the values pv of the WebView2 runtime could not be moved aside"
  MSYS_NO_PATHCONV=1 "$installer" /S "/D=$native_install" >"$work/install.out" 2>&1
  code=$?
  runtime_put_back || fail "the values pv of the WebView2 runtime could not be put back"
  [ "$code" = 0 ] || fail "the silent install exited $code"
  for file in halvo.exe $helpers THIRD-PARTY-NOTICES.txt uninstall.exe; do
    case $file in *.*) ;; *) file=$file.exe ;; esac
    [ -f "$install/$file" ] || fail "the silent install put no $file into the install directory"
  done
  [ -f "$install/$runtime_file" ] || fail "the silent install with no WebView2 runtime wrote no $runtime_file into the install directory"
  text=$(tr -d '\r' <"$install/$runtime_file") || fail "$runtime_file cannot be read"
  [ "$text" = "$sentence_runtime" ] || fail "$runtime_file holds another text than sentence (1)"
  n_helpers=$(printf '%s\n' "$helpers" | grep -c .)
  printf 'cycle install: halvo.exe, %s helpers, the notices, uninstall.exe and %s with sentence (1)\n' "$n_helpers" "$runtime_file"
  uninstall_entry HKCU || fail "the silent install wrote no uninstall entry under HKCU"
  ! uninstall_entry HKLM || fail "the silent install wrote an uninstall entry under HKLM"
  printf 'cycle install: the uninstall entry stands under HKCU alone\n'

  data=$(mktemp -d "$work/data.XXXXXX") || fail "mktemp failed"
  printf '{}\n' >"$data/daemon-state.json" || fail "the state file of a provisioned directory cannot be written"
  native_data=$(native_path "$data") || fail "the data directory has no native path"
  "$install/halvo.exe" --background --data-dir "$native_data" >"$work/app-1.out" 2>&1 &
  pid=$(winpid_of "$!") && started="$started $pid"
  wait_pair "--data-dir" "${data##*/}" "$native_data"
  stopped_child=${pair#* }
  wait_lease "--data-dir" "$native_data"
  "$install/halvo-capture.exe" ctl --data-dir "$native_data" stop >"$work/stop.out" 2>&1 || fail "ctl stop of the installed app exited $?"
  stopped=$(date +%s) || fail "the clock cannot be read"
  now=$stopped
  while [ $((now - stopped)) -lt "$stop_hold_s" ]; do
    list_from "$native_install" >"$work/listing" || fail "the list of processes cannot be read"
    awk '$1 == "halvo-capture.exe" { print $2 }' "$work/listing" | while read -r pid; do
      [ "$pid" = "$stopped_child" ] || printf '%s\n' "$pid"
    done >"$work/again"
    [ ! -s "$work/again" ] || fail "a second halvo-capture.exe from the install directory started after ctl stop"
    sleep "$poll_s"
    now=$(date +%s) || fail "the clock cannot be read"
  done
  awk '$1 == "halvo-capture.exe" { print $2 }' "$work/listing" | grep -q . && fail "the stopped halvo-capture.exe still runs $stop_hold_s s after ctl stop"
  printf 'cycle --data-dir: ctl stop exited 0 and no halvo-capture.exe from the install directory ran for %s s behind it\n' "$stop_hold_s"
  end_installed || fail "the processes from the install directory could not be ended"

  # An install over the copy while a file of it is held goes no further than the hook's wait.
  hold_file "$install/halvo-owner.exe"
  waited=0
  until [ -s "$work/holder" ]; do
    [ "$waited" -lt "$end_s" ] || fail "halvo-owner.exe of the install directory could not be held"
    sleep 1
    waited=$((waited + 1))
  done
  mv "$install/uninstall.exe" "$work/uninstall.exe.aside" || fail "uninstall.exe could not be moved aside"
  MSYS_NO_PATHCONV=1 "$installer" /S "/D=$native_install" >"$work/install-2.out" 2>&1
  code=$?
  release_file
  [ "$code" != 0 ] && [ ! -e "$install/uninstall.exe" ] || fail "the install over a copy of which halvo-owner.exe is held went on (exit $code)"
  mv "$work/uninstall.exe.aside" "$install/uninstall.exe" || fail "uninstall.exe could not be put back"
  printf 'cycle install over a held copy: exit %s, nothing installed\n' "$code"

  # Where the delete cannot use the data directory, here a file at its name, it exits 66, and a
  # silent uninstall with /DELETEALL stops before anything goes (exit 2).
  default_native=$("$install/halvo-capture.exe" --default-data-dir) || fail "halvo-capture.exe --default-data-dir exited $?"
  default_native=$(printf '%s' "$default_native" | tr -d '\r')
  default=$(posix_path "$default_native") || fail "the default data directory has no path here"
  folder=${default%/*}
  if [ -e "$default" ]; then
    aside=$default.check-70-aside
    mv "$default" "$aside" || fail "the default data directory cannot be moved aside"
  fi
  mkdir -p "$folder" || fail "the folder of the default data directory cannot be made"
  unusable=$default
  printf 'a file at the name of the data directory, planted by check 70\n' >"$unusable" || fail "no file can be planted at the name of the default data directory"
  "$install/halvo-capture.exe" --delete-all --data-dir "$default_native" >"$work/delete-66.out" 2>&1
  code=$?
  [ "$code" = 66 ] || fail "halvo-capture.exe --delete-all on a file at the name of the data directory exited $code, not 66"
  MSYS_NO_PATHCONV=1 "$install/uninstall.exe" /S /DELETEALL "_?=$native_install" >"$work/uninstall-66.out" 2>&1
  code=$?
  printf 'cycle data folder unusable: the delete exits 66; uninstall /S /DELETEALL: exit %s\n' "$code"
  went=
  for file in halvo.exe uninstall.exe THIRD-PARTY-NOTICES.txt; do
    [ -e "$install/$file" ] || went="$went, $file"
  done
  uninstall_entry HKCU || went="$went, the uninstall entry"
  [ -f "$unusable" ] || went="$went, the file at the name of the data directory"
  [ -z "$went" ] || fail "the silent uninstall with /DELETEALL went on where the delete could not use the data directory; gone:${went#,}"
  [ "$code" = 2 ] || fail "the silent uninstall with /DELETEALL where the delete could not use the data directory exited $code, not 2"
  take_away "$unusable"
  unusable=
  if [ -n "$aside" ]; then
    mv "$aside" "$default" || fail "the default data directory cannot be put back"
    aside=
  fi
  printf 'cycle data folder unusable (sentence 8): exit 2, nothing gone\n'

  # An uninstall without the box calls the app's form, which removes the crash reports of the
  # window from the folder that its line names; where one stays, here one that the check holds
  # open, the uninstall goes to its end and exits 3.
  "$install/halvo.exe" --remove-crash-reports >"$work/form.out" 2>&1
  code=$?
  line=$(tr -d '\r' <"$work/form.out" | grep -m 1 '^crash_reports folder=')
  [ "$code" = 0 ] && [ -n "$line" ] || fail "halvo.exe --remove-crash-reports exited $code with the line: ${line:-none}"
  reports_native=${line#crash_reports folder=}
  reports_native=${reports_native% removed=*}
  reports=$(posix_path "$reports_native") || fail "the folder of the crash reports has no path here"
  mkdir -p "$reports" || fail "the folder of the crash reports cannot be made"
  report_goes=$reports/check-70-report-goes.dmp
  report_stays=$reports/check-70-report-stays.dmp
  for file in "$report_goes" "$report_stays"; do
    printf 'planted by check 70\n' >"$file" || fail "no report can be planted in $reports_native"
  done
  hold_file "$report_stays"
  waited=0
  until [ -s "$work/holder" ]; do
    [ "$waited" -lt "$end_s" ] || fail "the planted report could not be held"
    sleep 1
    waited=$((waited + 1))
  done
  MSYS_NO_PATHCONV=1 "$install/uninstall.exe" /S "_?=$native_install" >"$work/uninstall-report.out" 2>&1
  code=$?
  release_file
  printf 'cycle uninstall /S without the box, one report held: exit %s\n' "$code"
  if [ -e "$install/halvo.exe" ] || uninstall_entry HKCU; then
    fail "the silent uninstall without the box did not remove the program (exit $code)"
  fi
  [ ! -e "$report_goes" ] || fail "the report planted in the folder that the form names stays after an uninstall without the box"
  printf 'cycle uninstall without the box (sentence 5): the report planted in %s is gone\n' "$reports_native"
  [ -e "$report_stays" ] || fail "the report that the check held is gone"
  [ "$code" = 3 ] || fail "the silent uninstall without the box that left a report exited $code, not 3"
  printf 'cycle uninstall without the box (sentence 6): the held report stays, exit 3, the program is gone\n'
  take_away "$report_stays"
  report_stays=
  MSYS_NO_PATHCONV=1 "$installer" /S "/D=$native_install" >"$work/install-3.out" 2>&1
  code=$?
  if [ "$code" != 0 ] || [ ! -f "$install/halvo.exe" ] || ! uninstall_entry HKCU; then
    fail "the install after the uninstall without the box exited $code"
  fi
  printf 'cycle install again: exit 0\n'

  # The account's own Halvo in a second folder: its install makes the uninstall entry name that
  # folder, and its app, started below, writes the Run value of its own path. From here on the
  # cycle runs that copy, and the copy in the first folder reads as another account's install.
  second_from=$(date +%s) || fail "the clock cannot be read"
  first=$install
  native_first=$native_install
  install=$work/installed/second/Halvo
  mkdir -p "$install" || fail "mkdir failed"
  native_install=$(native_path "$install") || fail "the second folder has no native path"
  MSYS_NO_PATHCONV=1 "$installer" /S "/D=$native_install" >"$work/install-second.out" 2>&1
  code=$?
  if [ "$code" != 0 ] || [ ! -f "$install/halvo.exe" ] || ! uninstall_entry HKCU; then
    fail "the install into a second folder exited $code"
  fi
  location=$(reg_data "HKCU\\$uninstall_key" InstallLocation)
  location=${location#\"}
  location=${location%\"}
  location=${location%\\}
  # The last uninstall below reads the entry as naming the second folder by the hook's own
  # compare; here the entry no longer names the first folder.
  named=$(printf '%s' "$location" | tr '[:upper:]' '[:lower:]')
  was=$(printf '%s' "$native_first" | tr '[:upper:]' '[:lower:]')
  [ -n "$named" ] && [ "$named" != "$was" ] || fail "the uninstall entry names ${location:-no folder} after the install into a second folder"
  now=$(date +%s) || fail "the clock cannot be read"
  second_s=$((now - second_from))
  printf 'cycle install into a second folder: exit 0, the uninstall entry names %s\n' "$location"

  [ -e "$default" ] || made_default=$default
  mkdir -p "$default" "$folder/EBWebView" || fail "the default data directory cannot be made"
  printf '{}\n' >"$default/daemon-state.json" || fail "the state file of the default data directory cannot be written"
  planted=$folder/EBWebView/check-70-planted.txt
  printf 'planted by check 70\n' >"$planted" || fail "no file can be planted in the web view's folder"
  planted_run="\"$native_install\\halvo.exe\" --background"
  reg_put "$run_key" Halvo "$planted_run" || fail "the Run value Halvo cannot be planted"
  printf 'cycle default: %s made, a file planted in %s, the Run value planted\n' "$default_native" "${folder##*/}/EBWebView"
  "$install/halvo.exe" --background >"$work/app-2.out" 2>&1 &
  pid=$(winpid_of "$!") && started="$started $pid"
  wait_pair "default" "halvo" ""
  wait_lease "default" ""

  # The uninstall entry of this account turned to another folder reads as another account's
  # install to the first copy: its silent uninstall with /DELETEALL stops before anything goes
  # (exit 2), and the app of the second folder still runs with its child. Then the entry names the
  # second folder again.
  entry_key="HKCU\\$uninstall_key"
  location=$(reg_data "$entry_key" InstallLocation)
  [ -n "$location" ] || fail "the uninstall entry names no InstallLocation"
  reg_put "$entry_key" InstallLocation "$native_first-of-another-account" || fail "the folder of the uninstall entry cannot be turned"
  # The app writes the Run value of its own path at its start, so the value is read as it stands
  # now, not as the check planted it.
  run_before=$(reg_data "$run_key" Halvo)
  MSYS_NO_PATHCONV=1 "$first/uninstall.exe" /S /DELETEALL "_?=$native_first" >"$work/uninstall-other.out" 2>&1
  code=$?
  printf 'cycle uninstall /S /DELETEALL, the entry naming another folder: exit %s\n' "$code"
  went=
  for file in halvo.exe uninstall.exe THIRD-PARTY-NOTICES.txt; do
    [ -e "$first/$file" ] || went="$went, $file"
  done
  uninstall_entry HKCU || went="$went, the uninstall entry"
  [ -e "$default" ] || went="$went, the default data directory"
  [ -e "$planted" ] || went="$went, the file planted in EBWebView"
  run_now=$(reg_data "$run_key" Halvo)
  [ "$run_now" = "$run_before" ] || went="$went, the Run value"
  list_from "$native_install" >"$work/listing" || fail "the list of processes cannot be read"
  pair_of halvo
  [ -n "$pair" ] || went="$went, the app with its child"
  [ -z "$went" ] || fail "the silent uninstall with /DELETEALL went on while the uninstall entry names another folder; gone:${went#,}"
  [ "$code" = 2 ] || fail "the silent uninstall with /DELETEALL while the uninstall entry names another folder exited $code, not 2"
  # The folder goes back without the template's quotes, which the hook takes off as well, so that
  # no quote has to pass to reg.exe.
  location=${location#\"}
  location=${location%\"}
  reg_put "$entry_key" InstallLocation "$location" || fail "the folder of the uninstall entry cannot be put back"
  back=$(reg_data "$entry_key" InstallLocation)
  [ "$back" = "$location" ] || fail "the uninstall entry names $back after the check put $location back"
  printf 'cycle another account (sentence 3): exit 2, nothing gone, the app runs with its child; the entry names the second folder again, without quotes\n'

  # The first copy's uninstall without the box goes to its end under another account, and behind
  # it the running account's Run value and uninstall entry stand as they stood, value for value,
  # and the capture service of the second copy runs on: the stop that the hook makes no longer
  # under another account names no data directory, so the service on the default data directory,
  # this app's child, is the one that it would reach.
  stage_from=$(date +%s) || fail "the clock cannot be read"
  run_before=$(reg_data "$run_key" Halvo) || fail "the Run value Halvo cannot be read before the uninstall of the first copy"
  entry_before=$(entry_values) || fail "the uninstall entry cannot be read before the uninstall of the first copy"
  n_values=$(printf '%s\n' "$entry_before" | grep -c .)
  list_from "$native_install" >"$work/listing" || fail "the list of processes cannot be read"
  pair_of halvo
  [ -n "$pair" ] || fail "the app of the second folder runs with no child before the uninstall of the first copy"
  pair_before=$pair
  MSYS_NO_PATHCONV=1 "$first/uninstall.exe" /S "_?=$native_first" >"$work/uninstall-first.out" 2>&1
  code=$?
  printf 'cycle uninstall /S of the first copy, the entry and the Run value naming the second folder: exit %s\n' "$code"
  [ "$code" = 0 ] || fail "the silent uninstall of the first copy under another account exited $code, not 0"
  left=$(cd "$first" && find . ! -name . | sed 's|^\./||' | grep -v -x -F uninstall.exe | sort | paste -s -d ' ' -)
  [ -z "$left" ] || fail "the uninstall of the first copy left in its folder: $left"
  went=
  run_now=$(reg_data "$run_key" Halvo)
  [ "$run_now" = "$run_before" ] || went="$went, the Run value"
  entry_now=$(entry_values)
  [ "$entry_now" = "$entry_before" ] || went="$went, the uninstall entry"
  list_from "$native_install" >"$work/listing" || fail "the list of processes cannot be read"
  pair_of halvo
  [ "$pair" = "$pair_before" ] || went="$went, the capture service of the second copy"
  [ -z "$went" ] || fail "the uninstall of the first copy under another account took what the account holds for its copy in the second folder; gone:${went#,}"
  now=$(date +%s) || fail "the clock cannot be read"
  printf 'cycle a copy of the account'"'"'s own in a second folder (write-back): the first copy is gone, the Run value and the %s values of the uninstall entry stand as they stood, and the app %s of the second folder runs with the same child %s; seconds: the install into the second folder %s, this stage %s\n' "$n_values" "${pair% *}" "${pair#* }" "$second_s" "$((now - stage_from))"

  MSYS_NO_PATHCONV=1 "$install/uninstall.exe" /S /DELETEALL "_?=$native_install" >"$work/uninstall.out" 2>&1
  code=$?
  printf 'cycle uninstall /S /DELETEALL: exit %s\n' "$code"
  [ "$code" = 0 ] || fail "the silent uninstall with /DELETEALL exited $code"
  list_from "$native_install" >"$work/listing" || fail "the list of processes cannot be read"
  names=$(awk '{ print $1 }' "$work/listing" | paste -s -d ' ' -)
  [ ! -s "$work/listing" ] || fail "a process from the install directory runs after the uninstall: $names"
  [ ! -e "$install/THIRD-PARTY-NOTICES.txt" ] || fail "the uninstall left THIRD-PARTY-NOTICES.txt"
  left=$(cd "$install" && find . ! -name . | sed 's|^\./||' | grep -v -x -F uninstall.exe | sort | paste -s -d ' ' -)
  [ -z "$left" ] || fail "the uninstall left in the install directory: $left"
  ! reg_data "$run_key" Halvo >/dev/null || fail "the Run value Halvo stays after the uninstall"
  ! uninstall_entry HKCU || fail "the uninstall entry under HKCU stays after the uninstall"
  [ ! -e "$default" ] || fail "the uninstall went on while the default data directory stays"
  [ ! -e "$planted" ] || fail "the file planted in the web view's folder stays after an uninstall with /DELETEALL"
  printf 'cycle uninstall: no process, no file but uninstall.exe, no notices, no Run value, no uninstall entry, no default data directory, no planted file\n'
  cleanup
}

# The cycle on the stand-ins of the self-test, in the directory $1: STUB_MODE names the fault,
# STUB_BIN holds the stand-in programs, and the stand-ins keep their processes in procs/ (one file
# per process id), the registry in reg/ (one file per value) and what they take away in gone/. It
# keeps no output of a stand-in among the artifacts.
fixture() {
  CI_D_ARTIFACTS=
  start_s=10
  answer_s=10
  lease_s=3
  stop_hold_s=2
  end_s=3
  poll_s=0.2
  taken=0
  helpers=$(helper_names)
  native_path() {
    printf '%s\n' "$1"
  }
  posix_path() {
    printf '%s\n' "$1"
  }
  list_from() {
    for file in "$STUB_DIR/procs"/*; do
      [ -f "$file" ] || continue
      kill -0 "${file##*/}" 2>/dev/null && cat "$file"
    done
    return 0
  }
  end_process() {
    kill -9 "$1" 2>/dev/null
    return 0
  }
  if [ "$STUB_MODE" = listing-blind ]; then
    list_from() {
      return 0
    }
  fi
  list_all() {
    printf 'dir given=%s long=%s\n' "$1" "$1"
    for file in "$STUB_DIR/procs"/*; do
      [ -f "$file" ] || continue
      kill -0 "${file##*/}" 2>/dev/null || continue
      read -r kind pid ppid exe rest <"$file"
      printf '%s %s %s path=%s long=%s\n' "$kind" "$pid" "$ppid" "$exe" "$exe"
    done
    return 0
  }
  winpid_of() {
    printf '%s\n' "$1"
  }
  # As taskkill /T: the process and each stand-in whose parent it is.
  end_tree() {
    for file in "$STUB_DIR/procs"/*; do
      [ -f "$file" ] || continue
      read -r kind pid ppid rest <"$file"
      [ "$ppid" = "$1" ] && kill -9 "$pid" 2>/dev/null
    done
    kill -9 "$1" 2>/dev/null
    return 0
  }
  shortcuts() {
    printf '%s\n' "$STUB_DIR/Desktop/Halvo.lnk" "$STUB_DIR/Start Menu/Programs/Halvo.lnk"
  }
  take_away() {
    for path in "$@"; do
      taken=$((taken + 1))
      if [ -e "$path" ]; then mv "$path" "$STUB_DIR/gone/taken-$taken"; fi
    done
    return 0
  }
  reg_file() {
    flat=$(printf '%s %s' "$1" "$2" | tr -c 'A-Za-z0-9' '_')
    printf '%s/reg/%s\n' "$STUB_DIR" "$flat"
  }
  reg_data() {
    file=$(reg_file "$1" "$2")
    [ -f "$file" ] && cat "$file"
  }
  reg_put() {
    printf '%s' "$3" >"$(reg_file "$1" "$2")"
  }
  reg_drop() {
    file=$(reg_file "$1" "$2")
    take_away "$file"
  }
  uninstall_entry() {
    [ -f "$STUB_DIR/reg/uninstall-$1" ]
  }
  uninstall_entry_drop() {
    take_away "$STUB_DIR/reg/uninstall-HKCU"
  }
  entry_values() {
    [ -f "$STUB_DIR/reg/uninstall-HKCU" ] || return 1
    for file in "$STUB_DIR/reg/HKCU_Software_Microsoft_Windows_CurrentVersion_Uninstall_Halvo_"*; do
      [ -f "$file" ] || continue
      value=$(cat "$file") || return 1
      printf '%s %s\n' "${file##*/}" "$value"
    done
    return 0
  }
  # The held file's path goes into held, where the stand-in form reads it.
  hold_file() {
    printf '%s\n' "$1" >"$STUB_DIR/held" && printf 'stand-in\n' >"$work/holder"
  }
  release_file() {
    [ -s "$work/holder" ] || return 0
    take_away "$STUB_DIR/held"
    mv "$work/holder" "$work/holder-ended"
  }
  mkdir -p "$STUB_DIR/procs" "$STUB_DIR/reg" "$STUB_DIR/gone" || exit 2
  # The runner's WebView2 runtime: its two values pv.
  for key in $runtime_keys; do
    reg_put "$key" pv 146.0.0.1 || exit 2
  done
  cycle "$STUB_BIN/setup.exe" "$STUB_DIR"
  printf 'PASS %s\n' "$name"
}

# The stand-in programs of the fixture, into the directory $1.
stand_in_programs() {
  mkdir -p "$1" || exit 2
  cat >"$1/setup.exe" <<'EOF'
#!/bin/sh
[ "$1" = /S ] || exit 98
case $2 in /D=*) dir=${2#/D=} ;; *) exit 98 ;; esac
[ -f "$STUB_DIR/held" ] && [ "$STUB_MODE" != install-goes-on ] && exit 2
mkdir -p "$dir" || exit 3
cp "$STUB_BIN/app" "$dir/halvo.exe" || exit 3
cp "$STUB_BIN/capture" "$dir/halvo-capture.exe" || exit 3
for helper in $STUB_HELPERS; do
  [ -f "$dir/$helper.exe" ] || printf '#!/bin/sh\nexit 0\n' >"$dir/$helper.exe"
done
printf 'notices\n' >"$dir/THIRD-PARTY-NOTICES.txt"
cp "$STUB_BIN/uninstall" "$dir/uninstall.exe" || exit 3
if ! ls "$STUB_DIR/reg" | grep -q EdgeUpdate && [ "$STUB_MODE" != no-runtime-file ]; then
  printf '%s\r\n' "$STUB_SENTENCE" >"$dir/WEBVIEW2-RUNTIME-MISSING.txt"
fi
: >"$STUB_DIR/reg/uninstall-HKCU"
printf '"%s"' "$dir" >"$STUB_DIR/reg/HKCU_Software_Microsoft_Windows_CurrentVersion_Uninstall_Halvo_InstallLocation"
exit 0
EOF
  # The app: --background with its child, and the form --remove-crash-reports on the folder of the
  # window's reports beside the default data directory, which leaves the file that held names.
  cat >"$1/app" <<'EOF'
#!/bin/sh
if [ "${1:-}" = --remove-crash-reports ]; then
  folder=${STUB_DEFAULT%/*}/EBWebView/Crashpad/reports
  held=
  [ -f "$STUB_DIR/held" ] && held=$(cat "$STUB_DIR/held")
  removed=0
  left=0
  for file in "$folder"/*; do
    [ -f "$file" ] || continue
    if [ "$file" = "$held" ]; then
      left=$((left + 1))
    else
      mv "$file" "$STUB_DIR/gone/report-$$-$removed" && removed=$((removed + 1))
    fi
  done
  printf 'crash_reports folder=%s removed=%s left=%s\n' "$folder" "$removed" "$left"
  [ "$left" = 0 ]
  exit
fi
[ "$1" = --background ] || exit 98
dir=$STUB_DEFAULT
[ "${2:-}" = --data-dir ] && dir=$3
here=$(cd "$(dirname "$0")" && pwd)
# As the app at a start on its default data directory: the Run value of its own path.
if [ "${2:-}" != --data-dir ]; then
  printf '"%s" --background' "$here/halvo.exe" >"$STUB_DIR/reg/HKCU_Software_Microsoft_Windows_CurrentVersion_Run_Halvo"
fi
printf 'halvo.exe %s %s %s --background --data-dir %s\n' "$$" "$PPID" "$here/halvo.exe" "$dir" >"$STUB_DIR/procs/$$"
if [ -f "$dir/daemon-state.json" ] && [ "$STUB_MODE" != no-child ]; then
  sh "$here/halvo-capture.exe" --service-run --lifeline --data-dir "$dir" &
fi
while :; do sleep 1; done
EOF
  cat >"$1/capture" <<'EOF'
#!/bin/sh
here=$(cd "$(dirname "$0")" && pwd)
case $1 in
  --default-data-dir)
    [ $# = 1 ] || exit 64
    printf '%s\n' "$STUB_DEFAULT"
    exit 0
    ;;
  --delete-all)
    [ $# = 3 ] && [ "$2" = --data-dir ] || exit 64
    printf '%s\n' "$3" >>"$STUB_DIR/deleted"
    [ -f "$3" ] && exit 66
    if [ "$STUB_MODE" = delete-fails ]; then
      printf '%s\n' "$3" >"$STUB_DIR/stuck"
      exit 1
    fi
    [ -d "$3" ] && mv "$3" "$STUB_DIR/gone/deleted-$$"
    exit 0
    ;;
  ctl)
    shift
    dir=$STUB_DEFAULT
    if [ "$1" = --data-dir ]; then
      dir=$2
      shift 2
    fi
    pids=
    for file in "$STUB_DIR/procs"/*; do
      [ -f "$file" ] || continue
      kill -0 "${file##*/}" 2>/dev/null || continue
      case $(cat "$file") in
        "halvo-capture.exe "*" --data-dir $dir") pids="$pids ${file##*/}" ;;
      esac
    done
    [ -n "$pids" ] || exit 69
    case $1 in
      status)
        printf 'store=open\n'
        if [ "$STUB_MODE" = no-fresh-lease ]; then printf 'lease=stale\n'; else printf 'lease=fresh\n'; fi
        ;;
      stop) kill $pids ;;
      *) exit 64 ;;
    esac
    exit 0
    ;;
  --service-run)
    [ "$2" = --lifeline ] && [ "$3" = --data-dir ] || exit 98
    printf 'halvo-capture.exe %s %s %s --service-run --lifeline --data-dir %s\n' "$$" "$PPID" "$here/halvo-capture.exe" "$4" >"$STUB_DIR/procs/$$"
    while kill -0 "$PPID" 2>/dev/null; do sleep 0.2; done
    exit 0
    ;;
esac
exit 98
EOF
  # The uninstaller with the hooks' steps in their order and the template's own steps; where the
  # delete exits 1 a file of the data directory stays, and the removal of the folder leaves it.
  # First whose install it removes: where this account's uninstall entry names another folder, the
  # box deletes nothing and a silent run with /DELETEALL stops (exit 2). The stop of the capture
  # service at the account's own install alone; the app's end, as the template's, of the app of
  # this folder alone. Behind the delete, at the account's own uninstall, the app's form; a report
  # that stays ends a run without the box with 3. Under another account the Run value, where it
  # names no program of this folder, and the uninstall entry come back behind the removals.
  cat >"$1/uninstall" <<'EOF'
#!/bin/sh
here=$(cd "$(dirname "$0")" && pwd -P)
here_named=$(cd "$(dirname "$0")" && pwd)
[ "$1" = /S ] || exit 98
delete=no
[ "${2:-}" = /DELETEALL ] && delete=yes
reg=$STUB_DIR/reg
location_file=$reg/HKCU_Software_Microsoft_Windows_CurrentVersion_Uninstall_Halvo_InstallLocation
run_file=$reg/HKCU_Software_Microsoft_Windows_CurrentVersion_Run_Halvo
location=
[ -f "$location_file" ] && location=$(cat "$location_file")
location=${location#\"}
location=${location%\"}
own=no
if [ -n "$location" ] && [ -d "$location" ] && [ "$(cd "$location" && pwd -P)" = "$here" ]; then
  own=yes
fi
[ "$STUB_MODE" = other-deletes ] && own=yes
[ "$own" = no ] && [ "$delete" = yes ] && exit 2
if [ "$own" = yes ] || [ "$STUB_MODE" = no-write-back ]; then
  sh "$here/halvo-capture.exe" ctl stop
fi
if [ "$STUB_MODE" != left ]; then
  for file in "$STUB_DIR/procs"/*; do
    [ -f "$file" ] || continue
    read -r kind pid ppid exe rest <"$file"
    [ "$kind" = halvo.exe ] && [ "$exe" = "$here_named/halvo.exe" ] && kill -9 "$pid" 2>/dev/null
  done
fi
sleep 1
if [ "$delete" = yes ]; then
  dir=$(sh "$here/halvo-capture.exe" --default-data-dir) || exit 2
  sh "$here/halvo-capture.exe" --delete-all --data-dir "$dir"
  code=$?
  case $STUB_MODE:$code in
    *:0 | delete-fails:1 | goes-on-at-66:66) ;;
    *) exit 2 ;;
  esac
fi
report=no
if [ "$own" = yes ] && [ "$STUB_MODE" != report-left ]; then
  sh "$here/halvo.exe" --remove-crash-reports >/dev/null || report=yes
fi
n=0
gone() {
  n=$((n + 1))
  mv "$1" "$STUB_DIR/gone/uninstall-$$-$n"
}
held_run=no
held_entry=no
if [ "$own" = no ] && [ "$STUB_MODE" != no-write-back ]; then
  if [ -f "$run_file" ]; then
    program=$(cat "$run_file")
    program=${program#\"}
    program=${program%%\"*}
    folder=$(cd "${program%/*}" 2>/dev/null && pwd -P)
    [ "$folder" != "$here" ] && cp "$run_file" "$STUB_DIR/held-run" && held_run=yes
  fi
  if [ -f "$reg/uninstall-HKCU" ]; then
    mkdir -p "$STUB_DIR/held-entry" && cp "$reg/uninstall-HKCU" "$reg"/HKCU_Software_Microsoft_Windows_CurrentVersion_Uninstall_Halvo_* "$STUB_DIR/held-entry/" && held_entry=yes
  fi
fi
if [ "$STUB_MODE" != run-value-left ]; then
  for file in "$STUB_DIR/reg/"*_Run_Halvo; do
    [ -f "$file" ] && gone "$file"
  done
fi
for file in "$here"/*; do
  [ "${file##*/}" = uninstall.exe ] || gone "$file"
done
if [ "$delete" = yes ]; then
  folder=${STUB_DEFAULT%/*}
  for entry in "$folder"/*; do
    [ -e "$entry" ] || continue
    [ -f "$STUB_DIR/stuck" ] && [ "$entry" = "$STUB_DEFAULT" ] && continue
    gone "$entry"
  done
fi
[ -f "$STUB_DIR/reg/uninstall-HKCU" ] && gone "$STUB_DIR/reg/uninstall-HKCU"
[ -f "$location_file" ] && gone "$location_file"
[ "$held_run" = yes ] && mv "$STUB_DIR/held-run" "$run_file"
if [ "$held_entry" = yes ]; then
  mv "$STUB_DIR/held-entry"/* "$reg/" && mv "$STUB_DIR/held-entry" "$STUB_DIR/gone/held-entry-$$"
fi
[ "$delete" = no ] && [ "$report" = yes ] && exit 3
exit 0
EOF
  chmod +x "$1/setup.exe" "$1/app" "$1/capture" "$1/uninstall" || exit 2
}

build() {
  : >"$CI_D_ARTIFACTS/build.txt"
  attempt=1
  while :; do
    printf 'build attempt %s of %s\n' "$attempt" "$attempts" >>"$CI_D_ARTIFACTS/build.txt"
    npm run tauri build -- -- --locked >>"$CI_D_ARTIFACTS/build.txt" 2>&1
    status=$?
    printf 'build attempt %s of %s: exit %s\n' "$attempt" "$attempts" "$status"
    [ "$status" = 0 ] && return 0
    [ "$attempt" -lt "$attempts" ] || return 1
    attempt=$((attempt + 1))
  done
}

measure() {
  cd "$CI_D_REPO" || exit 2
  [ "${RUNNER_ENVIRONMENT:-}" = github-hosted ] || fail "this check installs Halvo for the account that runs it and deletes that account's Halvo data, so it runs on a GitHub-hosted runner alone"
  found=$(signing_defect) || fail "$found"
  found=$(config_defect .) || fail "$found"
  helpers=$(helper_names)
  [ -n "$helpers" ] || fail "packaging/helpers.d names no helper"
  # shellcheck disable=SC2086
  found=$(hooks_defect src-tauri/windows/hooks.nsh $helpers) || fail "$found"
  printf 'hooks: src-tauri/windows/hooks.nsh waits for each helper of packaging/helpers.d\n'
  found=$(texts_defect src-tauri/windows/hooks.nsh src-tauri/windows/English.nsh) || fail "$found"
  printf 'texts: the three sentences of the hooks and the label of the box are their rulings, word for word\n'
  found=$(level_defect src-tauri/windows/hooks.nsh) || fail "$found"
  printf 'hooks: each exit level is set in a silent run alone\n'
  found=$(window_folder_defect src-tauri/windows/hooks.nsh) || fail "$found"
  printf 'hooks: where the form names no folder, the sentence names the folder of the window alone\n'
  found=$(stop_defect src-tauri/windows/hooks.nsh) || fail "$found"
  printf 'hooks: under another account the uninstall makes no stop of the capture service\n'
  found=$(write_back_defect src-tauri/windows/hooks.nsh) || fail "$found"
  printf 'hooks: the running account'"'"'s entries are held in the first hook of the uninstall and written back first in the last\n'
  found=$(run_value_defect src-tauri/windows/hooks.nsh) || fail "$found"
  printf 'hooks: the Run value is held only where it names no program in the folder that goes\n'
  sevenzip=$(command -v 7z) || sevenzip='/c/Program Files/7-Zip/7z.exe'
  [ -x "$sevenzip" ] || fail "no 7z on PATH and no $sevenzip; the windows runner's image holds 7-Zip"
  banner=$("$sevenzip" i | grep -m 1 '7-Zip')
  printf '7-Zip: %s\n' "${banner:-unknown}"
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  : >"$work/before-build" || exit 2
  build || {
    cause=$(grep -m 1 -E '^(error|Error|scripts/tauri.js:)' "$CI_D_ARTIFACTS/build.txt")
    fail "the build call failed $attempts times: ${cause:-see build.txt in the artifacts}"
  }
  installer=$(installer_defect "${CARGO_TARGET_DIR:-target}/release/bundle/nsis" "$work/before-build") || fail "$installer"
  bytes=$(wc -c <"$installer" | tr -d ' ')
  printf 'installer: %s bytes=%s\n' "${installer##*/}" "$bytes"
  found=$(manifest_defect "$installer") || fail "$found"
  printf '%s\n' "$found"
  cp "$installer" "$CI_D_ARTIFACTS/" || fail "cp could not copy the installer to the artifacts"
  cp "$installer" "$work/installer.exe" || fail "cp could not copy the installer"
  (cd "$work" && "$sevenzip" x -y -bd -ocontent installer.exe) >"$CI_D_ARTIFACTS/7z.txt" 2>&1
  status=$?
  case $status in
    0) printf '7-Zip unpacked the installer: exit 0\n' ;;
    1)
      warning=$(grep -m 1 -i 'warning' "$CI_D_ARTIFACTS/7z.txt")
      printf '7-Zip unpacked the installer with a warning, exit 1: %s\n' "${warning:-see 7z.txt in the artifacts}"
      ;;
    *)
      cause=$(grep -m 1 -i -E 'error|cannot|unsupported' "$CI_D_ARTIFACTS/7z.txt")
      fail "7-Zip could not unpack the installer, exit $status: ${cause:-see 7z.txt in the artifacts}"
      ;;
  esac
  (cd "$work/content" && find . -type f | sed 's|^\./||' | sort) >"$CI_D_ARTIFACTS/installer-files.txt" || fail "find could not list the unpacked installer"
  file_count=$(grep -c . "$CI_D_ARTIFACTS/installer-files.txt")
  printf 'installer files unpacked by 7-Zip: %s\n' "$file_count"
  # shellcheck disable=SC2086
  directory=$(content_defect "$work/content" $helpers) || fail "$directory"
  for executable in halvo $helpers; do
    bytes=$(bytes_of "$directory/$executable.exe") || fail "$bytes"
    printf 'installer holds %s.exe bytes=%s\n' "$executable" "$bytes"
  done
  # shellcheck disable=SC2086
  read_output=$(release_defect "$directory" $helpers) || fail "$read_output"
  printf '%s\n' "$read_output"
  found=$(notices_defect "$directory" "$work/notices.txt") || fail "$found"
  printf '%s\n' "$found"
  cycled=$(date +%s) || fail "the clock cannot be read"
  cycle "$work/installer.exe" "$work"
  now=$(date +%s) || fail "the clock cannot be read"
  printf 'cycle seconds: %s\n' "$((now - cycled))"
  printf 'PASS %s\n' "$name"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  verdict() {
    printf 'case %s/%s want=%s got=%s\n' "$name" "$1" "$2" "$3"
    if [ "$3" != "$2" ]; then wrong="$wrong $1"; elif [ "$2" = FAIL ]; then rejected=$((rejected + 1)); fi
  }
  stand_in() {
    mkdir -p "$work/$1" || exit 2
    printf '%s\n' '#!/bin/sh' 'exit 0' >"$work/$1/$2" || exit 2
    chmod +x "$work/$1/$2" || exit 2
  }
  answer() {
    got=unexpected-exit-$2
    [ "$2" = 0 ] && got=PASS
    [ "$2:$3" = "1:$4" ] && got=FAIL
    verdict "$1" FAIL "$got"
  }

  mkdir -p "$work/tree/target/release/bundle/nsis" || exit 2
  : >"$work/tree/before-build" || exit 2
  output=$(installer_defect "$work/tree/target/release/bundle/nsis" "$work/tree/before-build" 2>"$work/tree.err")
  answer build-tree-without-an-installer $? "$output" 'the build made no installer *-setup.exe under release/bundle/nsis'

  stand_in content-without-the-daemon/Halvo halvo.exe
  output=$(content_defect "$work/content-without-the-daemon" halvo-capture 2>"$work/content.err")
  answer installer-content-without-the-daemon $? "$output" 'the installer holds no halvo-capture.exe beside halvo.exe'

  # The list of packaging/helpers.d as the tree holds it, and a content with every helper of it but the runner.
  helpers=$(helper_names)
  stand_in content-without-the-runner/Halvo halvo.exe
  for helper in $(printf '%s\n' "$helpers" | grep -v -x -F halvo-runner); do
    stand_in content-without-the-runner/Halvo "$helper.exe"
  done
  # shellcheck disable=SC2086
  output=$(content_defect "$work/content-without-the-runner" $helpers 2>"$work/runner.err")
  answer installer-content-without-the-runner $? "$output" 'the installer holds no halvo-runner.exe beside halvo.exe'

  # The template's uninstall.exe stands there as in a real installer, and the one more file sorts after it.
  for executable in halvo $helpers uninstall updater; do
    stand_in content-with-one-more-executable/Halvo "$executable.exe"
  done
  # shellcheck disable=SC2086
  output=$(content_defect "$work/content-with-one-more-executable" $helpers 2>"$work/more.err")
  answer installer-content-with-one-more-executable $? "$output" 'the installer holds updater.exe beside halvo.exe, which is neither halvo.exe, a helper of packaging/helpers.d with .exe nor a file of the installer template (uninstall.exe)'

  stand_in daemon-with-the-test-seam halvo.exe
  stand_in daemon-with-the-test-seam halvo-capture.exe
  printf '%s\n' '# halvo-capture:test-seam:this-build-has-it!' >>"$work/daemon-with-the-test-seam/halvo-capture.exe" || exit 2
  output=$(release_defect "$work/daemon-with-the-test-seam" halvo-capture 2>"$work/seam.err")
  answer daemon-with-the-test-seam $? "$output" '62-build --files: halvo-capture.exe holds the marker halvo-capture:test-seam:this-build-has-it!'

  # Fixture 4: an unpacked installer without the notices file beside halvo.exe.
  stand_in content-without-the-notices/Halvo halvo.exe
  output=$(notices_defect "$work/content-without-the-notices/Halvo" "$work/notices.txt" 2>"$work/notices.err")
  answer installer-content-without-the-notices $? "$output" 'the installer holds no file THIRD-PARTY-NOTICES.txt beside halvo.exe'

  config_repo=$work/config
  mkdir -p "$config_repo/src-tauri" || exit 2
  git init -q "$config_repo" || exit 2
  printf '%s\n' '{"bundle":{"windows":{"signCommand":"stand-in-sign %1"}}}' >"$config_repo/src-tauri/tauri.windows.conf.json" || exit 2
  git -C "$config_repo" add -- src-tauri/tauri.windows.conf.json || exit 2
  output=$(config_defect "$config_repo")
  code=$?
  got=unexpected-exit-$code
  [ "$code" = 0 ] && got=PASS
  case $code:$output in
    "1:the tracked file src-tauri/tauri.windows.conf.json names signCommand; "*) got=FAIL ;;
  esac
  verdict configuration-with-a-sign-command FAIL "$got"

  # The hooks of the tree wait for every helper; a hooks file that lacks the runner's line fails.
  # shellcheck disable=SC2086
  output=$(hooks_defect src-tauri/windows/hooks.nsh $helpers 2>"$work/hooks.err")
  code=$?
  got=FAIL
  [ "$code" = 0 ] && got=PASS
  verdict hooks-of-the-tree PASS "$got"
  mkdir -p "$work/hooks" || exit 2
  for helper in $(printf '%s\n' "$helpers" | grep -v -x -F halvo-runner); do
    printf '    !insertmacro HALVO_HELD "%s.exe"\n' "$helper" >>"$work/hooks/hooks.nsh" || exit 2
  done
  # shellcheck disable=SC2086
  output=$(hooks_defect "$work/hooks/hooks.nsh" $helpers 2>"$work/hooks.err")
  answer hooks-without-the-runner $? "$output" "$work/hooks/hooks.nsh waits for no halvo-runner.exe of packaging/helpers.d before an uninstall"

  # The texts of the tree are their rulings; a language file with another label for the box fails.
  output=$(texts_defect src-tauri/windows/hooks.nsh src-tauri/windows/English.nsh 2>"$work/texts.err")
  code=$?
  got=FAIL
  [ "$code" = 0 ] && got=PASS
  verdict texts-of-the-tree PASS "$got"
  mkdir -p "$work/label" || exit 2
  # shellcheck disable=SC2016
  sed 's/^LangString deleteAppData .*/LangString deleteAppData ${LANG_ENGLISH} "Also delete everything that Halvo has stored on this computer"/' src-tauri/windows/English.nsh >"$work/label/English.nsh" || exit 2
  output=$(texts_defect src-tauri/windows/hooks.nsh "$work/label/English.nsh" 2>"$work/label.err")
  answer language-file-with-another-label $? "$output" "$work/label/English.nsh gives the box of the uninstaller another label than its ruling, $label_of_the_box"

  # The hooks of the tree set their exit level in a silent run alone; a hooks file that sets it in a
  # run with windows too fails.
  output=$(level_defect src-tauri/windows/hooks.nsh 2>"$work/level.err")
  code=$?
  got=FAIL
  [ "$code" = 0 ] && got=PASS
  verdict exit-level-of-the-tree PASS "$got"
  mkdir -p "$work/level" || exit 2
  # shellcheck disable=SC2016
  awk -v line="$level_line" '{ text = $0; sub(/^[ \t]+/, "", text) } text == line && !done { print "    SetErrorLevel ${HALVO_EXIT_REPORT_STAYS}"; done = 1; next } { print }' src-tauri/windows/hooks.nsh >"$work/level/hooks.nsh" || exit 2
  output=$(level_defect "$work/level/hooks.nsh" 2>"$work/level.err")
  answer hooks-with-an-exit-level-in-a-run-with-windows $? "$output" "$work/level/hooks.nsh sets an exit level outside a silent run (2 lines set one, 1 in a silent run alone)"

  # Where the form names no folder, the hooks of the tree name the folder of Halvo's window; a hooks
  # file that names the folder of the app's id there fails.
  output=$(window_folder_defect src-tauri/windows/hooks.nsh 2>"$work/window.err")
  code=$?
  got=FAIL
  [ "$code" = 0 ] && got=PASS
  verdict window-folder-of-the-tree PASS "$got"
  mkdir -p "$work/window" || exit 2
  # shellcheck disable=SC2016
  sed 's/StrCpy \$HalvoReportFolder "\$LOCALAPPDATA\\\${BUNDLEID}\\EBWebView"/StrCpy $HalvoReportFolder "$LOCALAPPDATA\\${BUNDLEID}"/' src-tauri/windows/hooks.nsh >"$work/window/hooks.nsh" || exit 2
  output=$(window_folder_defect "$work/window/hooks.nsh" 2>"$work/window.err")
  answer hooks-naming-the-folder-of-the-app-id $? "$output" "$work/window/hooks.nsh names another folder than the folder of Halvo's window where the form names none"

  # The hooks of the tree make no stop of the capture service under another account; a hooks file
  # whose first hook of the uninstall stops it ahead of the test of the account's own install, as
  # the base did, fails.
  output=$(stop_defect src-tauri/windows/hooks.nsh 2>"$work/stop.err")
  code=$?
  got=FAIL
  [ "$code" = 0 ] && got=PASS
  verdict no-stop-under-another-account-of-the-tree PASS "$got"
  mkdir -p "$work/stop" || exit 2
  # shellcheck disable=SC2016
  awk '{ t = $0; sub(/^[ \t]+/, "", t); line[NR] = $0; text[NR] = t }
    END {
      for (i = 1; i <= NR; i++) {
        if (text[i] == "${If} $HalvoOwnInstall = 1" && text[i + 1] == "!insertmacro HALVO_STOP_SERVICE" && text[i + 2] == "${EndIf}") { print line[i + 1]; i += 2; continue }
        print line[i]
      }
    }' src-tauri/windows/hooks.nsh >"$work/stop/hooks.nsh" || exit 2
  output=$(stop_defect "$work/stop/hooks.nsh" 2>"$work/stop.err")
  answer hooks-stopping-the-service-under-another-account $? "$output" "$work/stop/hooks.nsh stops the capture service at an uninstall under another account"

  # The hooks of the tree hold the running account's entries in the first hook of the uninstall and
  # write them back first in the last; a hooks file that writes them back in the first hook, before
  # the template's steps take them away, fails.
  output=$(write_back_defect src-tauri/windows/hooks.nsh 2>"$work/order.err")
  code=$?
  got=FAIL
  [ "$code" = 0 ] && got=PASS
  verdict write-back-order-of-the-tree PASS "$got"
  mkdir -p "$work/order" || exit 2
  awk '{ t = $0; sub(/^[ \t]+/, "", t) }
    t == "!insertmacro HALVO_WRITE_BACK_ENTRIES" { next }
    { print }
    t == "!insertmacro HALVO_HOLD_ENTRIES" { print "  !insertmacro HALVO_WRITE_BACK_ENTRIES" }' src-tauri/windows/hooks.nsh >"$work/order/hooks.nsh" || exit 2
  output=$(write_back_defect "$work/order/hooks.nsh" 2>"$work/order.err")
  answer hooks-writing-back-before-the-template $? "$output" "$work/order/hooks.nsh does not hold the running account's entries in its first hook of the uninstall and write them back first in its last"

  # The hooks of the tree hold the Run value only where the folder of its program is not the folder
  # that goes; a hooks file whose test holds it where that folder is the one that goes fails.
  output=$(run_value_defect src-tauri/windows/hooks.nsh 2>"$work/run.err")
  code=$?
  got=FAIL
  [ "$code" = 0 ] && got=PASS
  verdict run-value-of-the-tree PASS "$got"
  mkdir -p "$work/run" || exit 2
  # shellcheck disable=SC2016
  sed 's/\${OrIf} \$R4 != \$R7/${OrIf} $R4 == $R7/' src-tauri/windows/hooks.nsh >"$work/run/hooks.nsh" || exit 2
  output=$(run_value_defect "$work/run/hooks.nsh" 2>"$work/run.err")
  answer hooks-holding-the-run-value-of-the-folder-that-goes $? "$output" "$work/run/hooks.nsh holds the Run value also where it names the program in the folder that goes"

  # The cycle on stand-ins; the default data directory's path holds a space and a letter beyond
  # ASCII, so the good one passes only where the uninstall hands the printed path on whole.
  here=${0%/*}
  [ "$here" = "$0" ] && here=.
  here=$(cd "$here" && pwd) || exit 2
  self=$here/${0##*/}
  stand_in_programs "$work/stand-in-bin"
  fixture_limit_s=120
  zoe=$(printf 'Zo\303\253')
  for mode in good left delete-fails no-runtime-file run-value-left no-child install-goes-on listing-blind no-fresh-lease other-deletes report-left goes-on-at-66 no-write-back; do
    mkdir -p "$work/cycle-$mode" || exit 2
    STUB_MODE=$mode STUB_DIR=$work/cycle-$mode STUB_BIN=$work/stand-in-bin STUB_DEFAULT="$work/cycle-$mode/home/$zoe Halvo/com.halvo.desktop/data" STUB_HELPERS=$helpers STUB_SENTENCE=$sentence_runtime sh "$self" --fixture "$work/cycle-$mode" >"$work/cycle-$mode.out" 2>&1 &
    fixture=$!
    began=$(date +%s) || exit 2
    now=$began
    while kill -0 "$fixture" 2>/dev/null && [ $((now - began)) -lt "$fixture_limit_s" ]; do
      sleep 0.2
      now=$(date +%s) || exit 2
    done
    if kill -0 "$fixture" 2>/dev/null; then
      kill -9 "$fixture" 2>/dev/null
      printf 'the fixture ran past %s s and was ended\n' "$fixture_limit_s" >>"$work/cycle-$mode.out"
    fi
    wait "$fixture"
    printf '%s\n' "$?" >"$work/cycle-$mode.code"
    for file in "$work/cycle-$mode/procs"/*; do
      if [ -f "$file" ] && kill -0 "${file##*/}" 2>/dev/null; then
        printf 'left %s\n' "${file##*/}" >>"$work/cycle-$mode.left"
        kill -9 "${file##*/}" 2>/dev/null
      fi
    done
  done
  cycle_verdict() {
    out=$(cat "$work/cycle-$1.out")
    code=$(cat "$work/cycle-$1.code")
    last=$(printf '%s\n' "$out" | grep -E "^(PASS|FAIL) $name( |\$)" | tail -n 1)
    case $code:$last in
      "0:PASS $name") got=PASS ;;
      "1:FAIL $name ($3)") got=FAIL ;;
      *) got=exit-$code ;;
    esac
    [ ! -s "$work/cycle-$1.left" ] || got=$got-with-processes-left
    [ "$got" = "$2" ] || printf '%s\n' "$out" | sed "s/^/  $1: /"
    verdict "cycle-$1" "$2" "$got"
  }
  cycle_verdict good PASS ''
  grep -q -x -F "$work/cycle-good/home/$zoe Halvo/com.halvo.desktop/data" "$work/cycle-good/deleted" || { printf 'the stand-in delete got another path than the one printed\n'; wrong="$wrong cycle-good-path"; }
  cycle_verdict left FAIL 'a process from the install directory runs after the uninstall: halvo.exe'
  cycle_verdict delete-fails FAIL 'the uninstall went on while the default data directory stays'
  cycle_verdict no-runtime-file FAIL "the silent install with no WebView2 runtime wrote no $runtime_file into the install directory"
  cycle_verdict run-value-left FAIL 'the Run value Halvo stays after the uninstall'
  cycle_verdict no-child FAIL 'the installed app (--data-dir) runs with 0 halvo-capture.exe children from the install directory after 10 s, not one (1 apps from there)'
  cycle_verdict install-goes-on FAIL 'the install over a copy of which halvo-owner.exe is held went on (exit 0)'
  # A listing that sees nothing from the install directory, as the first hosted run's did: the
  # stage fails, and the cleanup ends the apps that the check started by their ids all the same.
  cycle_verdict listing-blind FAIL 'the installed app (--data-dir) runs with 0 halvo-capture.exe children from the install directory after 10 s, not one (0 apps from there)'
  # An app that never takes its child as its own: the stage fails at the bound before its stop.
  cycle_verdict no-fresh-lease FAIL 'ctl status of the installed app (--data-dir) did not read lease=fresh within 3 s'
  # An uninstall that reads no uninstall entry and deletes under another account's install.
  cycle_verdict other-deletes FAIL 'the silent uninstall with /DELETEALL went on while the uninstall entry names another folder; gone: halvo.exe, THIRD-PARTY-NOTICES.txt, the uninstall entry, the default data directory, the file planted in EBWebView, the Run value, the app with its child'
  # An uninstall that does not call the app's form, so the crash report stays.
  cycle_verdict report-left FAIL 'the report planted in the folder that the form names stays after an uninstall without the box'
  # An uninstall that goes on where the delete could not use the data directory (exit 66).
  cycle_verdict goes-on-at-66 FAIL 'the silent uninstall with /DELETEALL went on where the delete could not use the data directory; gone: halvo.exe, THIRD-PARTY-NOTICES.txt, the uninstall entry, the file at the name of the data directory'
  # An uninstall under another account that does what the base did: it stops the capture service
  # and writes neither the Run value nor the uninstall entry back.
  cycle_verdict no-write-back FAIL 'the uninstall of the first copy under another account took what the account holds for its copy in the second folder; gone: the Run value, the uninstall entry, the capture service of the second copy'

  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

case ${1:-} in
  --self-test) self_test ;;
  --fixture) fixture ;;
  '') measure ;;
  *) exit 64 ;;
esac
