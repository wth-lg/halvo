#!/bin/sh
# jobs: macos windows
# rule: In a second copy of the tree, the engine-alone copy of the window main reads 0 connections, requests and datagrams beside the no-layer copy above 0 on the ip forms of one link step (s03) and one peer step (s18), and s20 on Windows, and the ip form of s05 (window.open) reads control 0 with no route found; a control of 0 fails as not measured.
# self-test: alone
# lane: alone
# The tracked check of ruling AP-11 point 5 (b) in the smallest form of units/app-2.md line 192 (a)
# (ruling AP-14 L14): the engine-alone and no-layer copies of the window main, each built in a
# second copy of the tree and never in the job's checkout, run the steps above in one step. It
# prints the image, the system's version and the engine's version. Outside a hosted job
# (GITHUB_ACTIONS is not true) it changes nothing, builds nothing and starts nothing: it skips
# (ruling AP-15 point 3). In a hosted job on Windows it turns off each firewall profile that is
# on, and a trap that also runs on a signal and at an early exit ends the listener and a running
# launch by their recorded process ids and turns those profiles alone on again; it prints the
# state before and after. Each copy is checked to hold its change and builds into a target
# directory under the check's own work directory (ruling AP-17 G8). It changes no hosts file,
# resolver or packet filter.
set -u
name=38-app-engine-routes

keys_of() {
  if [ "$1" = windows ]; then echo "s03 ip
s05 ip
s18 ip
s20 ip"; else echo "s03 ip
s05 ip
s18 ip"; fi
}

verdict() {
  judged=$1
  job=$2
  bad=
  for key in $(keys_of "$job" | tr ' ' '-'); do
    step=${key%-ip}
    if [ "$step" = s05 ]; then
      control0=$(grep -c "^measure row: s05 ip no-layer 0 0 0 0 0 [a-z-]* control 0, as expected for a cell with no route found\$" "$judged")
      noroute=$(grep -c "^measure row: s05 ip engine-alone 0 0 0 0 0 [a-z-]* no route found: " "$judged")
      [ "$control0" = 1 ] && [ "$noroute" = 1 ] || bad="$bad s05-ip:route-found-or-unread"
      continue
    fi
    zero=$(grep -c "^measure row: $step ip engine-alone 0 0 0 0 0 true ok 0 beside its control\$" "$judged")
    control=$(awk -v s="$step" '$1 == "measure" && $2 == "row:" && $3 == s && $4 == "ip" && $5 == "no-layer" && ($6 + $7 + $8 + $9 + $10) > 0 && / control above 0$/ { n++ } END { print n + 0 }' "$judged")
    if [ "$control" != 1 ]; then
      bad="$bad $step-ip:not-measured"
    elif [ "$zero" != 1 ]; then
      bad="$bad $step-ip:open-or-unread"
    fi
  done
  if [ -n "$bad" ]; then
    printf 'FAIL %s (%s)\n' "$name" "${bad# }"
    return 1
  fi
  printf 'PASS %s\n' "$name"
}

told() {
  [ -f "$work/ready-$1" ] || return 0
  tr -d '\r' < "$work/ready-$1" > "$work/ready-$1.lf"
  asked=$(sed -n 's/^asked //p' "$work/ready-$1.lf")
  refused=$(grep -c -v -e '^asked ' -e '^interface ' -e '^interfaces ' "$work/ready-$1.lf")
  echo "$name listeners $1: asked ${asked:-no address}, $refused refused"
  relay "$work/ready-$1.lf" "at a refused bind of $1"
}

relay() {
  sed -n -e "s/^interface /$name interface $2: /p" -e "s/^interfaces /$name interfaces $2: /p" "$1"
}

self_test() {
  work=$(mktemp -d "${TMPDIR:-/tmp}/$name.XXXXXX") || exit 2
  rejected=0
  wrong=
  row() { printf 'measure row: %s\n' "$1"; }
  good() {
    row "s03 ip engine-alone 0 0 0 0 0 true ok 0 beside its control"
    row "s03 ip no-layer 1 1 0 0 0 false control above 0"
    row "s05 ip engine-alone 0 0 0 0 0 true no route found: with no layer the step reached nothing, and wry shows why; not measured as closed"
    row "s05 ip no-layer 0 0 0 0 0 true control 0, as expected for a cell with no route found"
    row "s18 ip engine-alone 0 0 0 0 0 true ok 0 beside its control"
    row "s18 ip no-layer 0 0 0 230 230 true control above 0"
    row "s20 ip engine-alone 0 0 0 0 0 true ok 0 beside its control"
    row "s20 ip no-layer 1 2 0 0 1 true control above 0"
  }
  case_of() {
    want=$1 label=$2 job=$3 file=$4
    got=PASS
    verdict "$file" "$job" > "$work/$label.out" || got=FAIL
    shown=$(cat "$work/$label.out")
    printf 'case %s/%s want=%s got=%s %s\n' "$name" "$label" "$want" "$got" "$shown"
    [ "$got" = "$want" ] || wrong="$wrong $label"
    [ "$want" = FAIL ] && [ "$got" = FAIL ] && rejected=$((rejected + 1))
  }
  good > "$work/pass.txt"
  case_of PASS pass-windows windows "$work/pass.txt"
  case_of PASS pass-macos macos "$work/pass.txt"
  good | sed 's/^measure row: s18 ip engine-alone 0 0 0 0 0 true ok 0 beside its control$/measure row: s18 ip engine-alone 0 0 0 5 0 true UNEXPECTED above 0/' > "$work/open.txt"
  case_of FAIL peer-open windows "$work/open.txt"
  good | sed 's/^measure row: s03 ip no-layer 1 1 0 0 0 false control above 0$/measure row: s03 ip no-layer 0 0 0 0 0 true control 0/' > "$work/control0.txt"
  case_of FAIL control-0 macos "$work/control0.txt"
  good | grep -v '^measure row: s20 ' > "$work/nos20.txt"
  case_of FAIL s20-missing-on-windows windows "$work/nos20.txt"
  case_of PASS s20-not-asked-on-macos macos "$work/nos20.txt"
  good | sed 's/^measure row: s03 ip engine-alone 0 0 0 0 0 true ok 0 beside its control$/measure row: s03 ip engine-alone 0 0 0 0 0 false UNEXPECTED window false/' > "$work/away.txt"
  case_of FAIL window-away macos "$work/away.txt"
  good | sed 's/^measure row: s18 ip engine-alone 0 0 0 0 0 true ok 0 beside its control$/measure row: s18 ip engine-alone 0 0 0 0 0 true UNEXPECTED not measured (listener)/' > "$work/listener.txt"
  case_of FAIL listener-unbound windows "$work/listener.txt"
  good | sed 's/^measure row: s05 ip no-layer 0 0 0 0 0 true control 0, as expected for a cell with no route found$/measure row: s05 ip no-layer 1 1 0 0 0 true UNEXPECTED control above 0 for a cell with no route found/' > "$work/route.txt"
  case_of FAIL s05-route-found windows "$work/route.txt"
  good | grep -v '^measure row: s05 ' > "$work/nos05.txt"
  case_of FAIL s05-missing macos "$work/nos05.txt"
  : > "$work/empty.txt"
  case_of FAIL no-output macos "$work/empty.txt"
  good | sed 's/^measure row: s03 ip no-layer 1 1 0 0 0 false control above 0$/measure row: s03 ip no-layer 0 0 0 0 0 false control above 0/' > "$work/control0-accepted.txt"
  case_of FAIL control-0-written-as-above-0 macos "$work/control0-accepted.txt"
  good | grep -v '^measure row: s05 ip engine-alone ' > "$work/s05-unread.txt"
  case_of FAIL s05-engine-alone-row-unread windows "$work/s05-unread.txt"
  told_cases
  helper_cases
  run_cases
  if [ -n "$wrong" ]; then
    printf 'FAIL self-test:%s (wrong verdict in:%s)\n' "$name" "$wrong"
    exit 1
  fi
  printf 'PASS self-test:%s (%s failing fixtures rejected)\n' "$name" "$rejected"
}

told_cases() {
  printf "s03-ip port 47030: [Errno 49] Can't assign requested address\r\ns18-ip port 47180: [Errno 49] Can't assign requested address\r\nasked 203.0.113.9\r\ninterface lo0 8049<UP,LOOPBACK,RUNNING,MULTICAST> inet 127.0.0.1\r\ninterface en0 8863<UP,BROADCAST> inet 10.0.0.5 status: active\r\n" > "$work/ready-refused-steps"
  printf '%s\n' "$name listeners refused-steps: asked 203.0.113.9, 2 refused" "$name interface at a refused bind of refused-steps: lo0 8049<UP,LOOPBACK,RUNNING,MULTICAST> inet 127.0.0.1" "$name interface at a refused bind of refused-steps: en0 8863<UP,BROADCAST> inet 10.0.0.5 status: active" > "$work/told-refused.want"
  printf 'asked 10.0.0.5\n' > "$work/ready-taken-steps"
  printf '%s\n' "$name listeners taken-steps: asked 10.0.0.5, 0 refused" > "$work/told-taken.want"
  printf 'resolver port 5354: [Errno 48] Address already in use\n' > "$work/ready-unasked-steps"
  printf '%s\n' "$name listeners unasked-steps: asked no address, 1 refused" > "$work/told-unasked.want"
  printf "s03-ip port 47030: [Errno 49] Can't assign requested address\nasked 203.0.113.9\ninterfaces read 3, reader failed false, asked address among them false, error numbers 49\n" > "$work/ready-elsewhere-steps"
  printf '%s\n' "$name listeners elsewhere-steps: asked 203.0.113.9, 1 refused" "$name interfaces at a refused bind of elsewhere-steps: read 3, reader failed false, asked address among them false, error numbers 49" > "$work/told-elsewhere.want"
  : > "$work/told-unwritten.want"
  for label in refused taken unasked elsewhere unwritten; do
    told "$label-steps" > "$work/told-$label.got" 2>&1
    got=mis-told
    cmp -s "$work/told-$label.want" "$work/told-$label.got" && got=told
    printf 'case %s/told-%s want=told got=%s\n' "$name" "$label" "$got"
    if [ "$got" = told ]; then rejected=$((rejected + 1)); else wrong="$wrong told-$label"; fi
  done
}

helper_cases() {
  py=python3
  command -v python3 > /dev/null 2>&1 || py=python
  mkdir -p "$work/helper" || exit 2
  write_helpers "$work/helper"
  write_cases "$work/helper"
  "$py" "$work/helper/cases.py" "$work/helper" > "$work/helper-cases.txt" 2>&1
  code=$?
  cat "$work/helper-cases.txt"
  right=$(grep -c ' right$' "$work/helper-cases.txt")
  rejected=$((rejected + right))
  bad=$(sed -n 's/^case [^ /]*\/\([^ ]*\) .* wrong$/\1/p' "$work/helper-cases.txt" | paste -s -d ' ' -)
  [ -z "$bad" ] || wrong="$wrong $bad"
  if [ "$code" != 0 ] || [ "$right" != 9 ]; then wrong="$wrong helper-cases"; fi
  shown=$(verdict "$work/helper/judged.txt" macos)
  got=green
  [ "$shown" = "FAIL $name (s03-ip:open-or-unread s18-ip:open-or-unread)" ] && got=red
  printf 'case %s/unbound-listener-judged-red want=red got=%s %s\n' "$name" "$got" "$shown"
  if [ "$got" = red ]; then rejected=$((rejected + 1)); else wrong="$wrong unbound-listener-judged-red"; fi
}

write_cases() {
  cat > "$1/cases.py" <<'CASESEOF'
import contextlib
import io
import json
import os
import socket
import sys

directory = sys.argv[1]
sys.path.insert(0, directory)
import measure

NAME = "38-app-engine-routes"
REFUSED = "[Errno 49] Can't assign requested address"
MAC_TEXT = (
    "lo0: flags=8049<UP,LOOPBACK,RUNNING,MULTICAST> mtu 16384\n"
    "\toptions=1203<RXCSUM,TXCSUM,TXSTATUS,SW_TIMESTAMP>\n"
    "\tinet 127.0.0.1 netmask 0xff000000\n"
    "\tinet6 ::1 prefixlen 128 \n"
    "en0: flags=8863<UP,BROADCAST,SMART,RUNNING,SIMPLEX,MULTICAST> mtu 1500\n"
    "\tether 02:00:00:00:00:01\n"
    "\tinet6 fe80::1%en0 prefixlen 64 secured scopeid 0x4 \n"
    "\tinet 10.0.0.5 netmask 0xffffff00 broadcast 10.0.0.255\n"
    "\tstatus: active\n"
    "utun0: flags=8051<UP,POINTOPOINT,RUNNING,MULTICAST> mtu 1380\n"
)
MAC_LINES = [
    "lo0 8049<UP,LOOPBACK,RUNNING,MULTICAST> inet 127.0.0.1 inet6 ::1",
    "en0 8863<UP,BROADCAST,SMART,RUNNING,SIMPLEX,MULTICAST> inet6 fe80::1%en0 inet 10.0.0.5 status: active",
    "utun0 8051<UP,POINTOPOINT,RUNNING,MULTICAST>",
]
WIN_TEXT = (
    "\r\nWindows IP Configuration\r\n\r\n\r\n"
    "Ethernet adapter Ethernet:\r\n\r\n"
    "   Connection-specific DNS Suffix  . : example.invalid\r\n"
    "   Link-local IPv6 Address . . . . . : fe80::5%6\r\n"
    "   IPv4 Address. . . . . . . . . . . : 10.1.0.4\r\n"
    "   Subnet Mask . . . . . . . . . . . : 255.255.0.0\r\n"
    "   Default Gateway . . . . . . . . . : 10.1.0.1\r\n\r\n"
    "Ethernet adapter vEthernet (nat):\r\n\r\n"
    "   Media State . . . . . . . . . . . : Media disconnected\r\n"
)
WIN_LINES = ["Ethernet adapter Ethernet fe80::5%6 10.1.0.4", "Ethernet adapter vEthernet (nat) Media disconnected"]
held = {"127.0.0.1", "::1", "10.0.0.5"}
route = ["10.0.0.5"]
reads = []
refused = {}
denied = set()
binds = []


class MadeUpSocket:
    def __init__(self, family=socket.AF_INET, kind=socket.SOCK_STREAM, *rest):
        self.family = family
        self.kind = kind

    def setsockopt(self, *rest):
        pass

    def bind(self, address):
        binds.append((self.kind, address[0], address[1]))
        if (address[0], address[1]) in denied:
            raise PermissionError(13, "Permission denied")
        if address[1] in refused:
            raise OSError(*refused[address[1]])
        if address[0] not in held:
            raise OSError(49, "Can't assign requested address")

    def listen(self, backlog):
        pass

    def accept(self):
        raise OSError("a made-up socket takes no connection")

    def recvfrom(self, size):
        raise OSError("a made-up socket takes no datagram")

    def connect(self, address):
        pass

    def getsockname(self):
        return (route[0], 9)

    def close(self):
        pass


def text_of_the_minute():
    reads.append(1)
    return MAC_TEXT


measure.socket.socket = MadeUpSocket
measure.time.sleep = lambda seconds: None
measure.interface_text = text_of_the_minute


def lines_of(name):
    with open(os.path.join(directory, name), encoding="utf-8") as f:
        return f.read().splitlines()


def quiet(call, *args):
    with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
        return call(*args)


def listen(label, where=directory):
    open(os.path.join(where, f"stop-{label}"), "w").close()
    measure.listen(where, label)
    with open(os.path.join(where, f"ready-{label}"), encoding="utf-8") as f:
        return f.read().splitlines()


def machine(value):
    if value is None:
        os.environ.pop("GITHUB_ACTIONS", None)
    else:
        os.environ["GITHUB_ACTIONS"] = value


def refused_binds_are_written_and_told():
    os.environ["HALVO_MEASURE_TEST_IP"] = "203.0.113.9"
    quiet(measure.plan, directory)
    os.environ.pop("HALVO_MEASURE_TEST_IP")
    with open(os.path.join(directory, "plan.json"), encoding="utf-8") as f:
        targets = json.load(f)["targets"]
    expected = [f"{t['step']}-{t['form']} port {t['port']}: {REFUSED}" for t in targets if not t.get("pf") and not t.get("loop")]
    machine("true")
    lines = listen("engine-alone-steps")
    with open(os.path.join(directory, "counts-engine-alone-steps.json"), encoding="utf-8") as f:
        failures = json.load(f)["listen_failures"]
    told = [line[len("interface "):] for line in lines if line.startswith("interface ")]
    if lines[:len(expected)] != expected or failures != expected:
        return f"refusals-{len(failures)}-of-{len(expected)}"
    if lines[len(expected):] != ["asked 203.0.113.9"] + [f"interface {line}" for line in MAC_LINES]:
        return f"no-print-{len(told)}-interfaces"
    return "told"


def taken_binds_tell_the_address_alone():
    held.add("203.0.113.9")
    del reads[:]
    lines = listen("no-layer-steps")
    held.discard("203.0.113.9")
    if reads:
        return "interfaces-read"
    return "asked-alone" if lines == ["asked 203.0.113.9"] else f"lines-{len(lines)}"


def one_refused_bind_is_told_with_the_interfaces():
    with open(os.path.join(directory, "plan.json"), encoding="utf-8") as f:
        port = json.load(f)["dns_port"]
    held.add("203.0.113.9")
    refused[port] = (48, "Address already in use")
    machine("true")
    try:
        lines = listen("one-refusal-steps")
    finally:
        held.discard("203.0.113.9")
        del refused[port]
    want = [f"resolver port {port}: [Errno 48] Address already in use", "asked 203.0.113.9"] + [f"interface {line}" for line in MAC_LINES]
    return "told" if lines == want else f"lines-{len(lines)}"


def each_refused_bind_is_asked_once():
    denied.add(("203.0.113.9", 443))
    del binds[:]
    machine("true")
    try:
        listen("once-steps")
    finally:
        denied.discard(("203.0.113.9", 443))
    asked = {}
    for call in binds:
        if call[2] != 0:
            asked[call] = asked.get(call, 0) + 1
    twice = [call for call, n in asked.items() if n > 1]
    if twice:
        return f"asked-twice-{len(twice)}"
    if asked.get((socket.SOCK_STREAM, "203.0.113.9", 443)) != 1 or asked.get((socket.SOCK_STREAM, "0.0.0.0", 443)) != 1:
        return "permission-branch-misread"
    return "once"


def refused_binds_elsewhere_are_told_by_counts():
    with open(os.path.join(directory, "plan.json"), encoding="utf-8") as f:
        port = json.load(f)["dns_port"]
    refusals = [line for line in lines_of("ready-engine-alone-steps") if line.endswith(REFUSED)]
    count = "interfaces read 3, reader failed false, asked address among them false, error numbers 49"
    machine(None)
    lines = listen("elsewhere-unset-steps")
    if lines != refusals + ["asked 203.0.113.9", count]:
        return f"unset-{sum(line.startswith('interface ') for line in lines)}-interface-lines"
    near = os.path.join(directory, "near")
    os.environ["HALVO_MEASURE_TEST_IP"] = "10.0.0.5"
    quiet(measure.plan, near)
    os.environ.pop("HALVO_MEASURE_TEST_IP")
    machine("false")
    refused[port] = (48, "Address already in use")
    try:
        lines = listen("elsewhere-false-steps", near)
    finally:
        del refused[port]
    if lines != [f"resolver port {port}: [Errno 48] Address already in use", "asked 10.0.0.5", "interfaces read 3, reader failed false, asked address among them true, error numbers 48"]:
        return f"false-{sum(line.startswith('interface ') for line in lines)}-interface-lines"
    machine(None)

    def broken():
        raise OSError("no reader")

    measure.interface_text = broken
    try:
        lines = listen("elsewhere-unread-steps")
    finally:
        measure.interface_text = text_of_the_minute
    if lines != refusals + ["asked 203.0.113.9", "interfaces read 0, reader failed true, asked address among them false, error numbers 49"]:
        return "unread-misread"
    return "counted"


def windows_interfaces_are_read():
    measure.interface_text = lambda: WIN_TEXT
    try:
        lines = measure.interfaces()
    finally:
        measure.interface_text = text_of_the_minute
    return "read" if lines == WIN_LINES else "misread"


def an_unread_list_is_told():
    def broken():
        raise OSError("no reader")

    def late():
        raise measure.subprocess.TimeoutExpired("ifconfig", 30)

    told = []
    for reader in (broken, late, lambda: "", lambda: "\tinet 10.9.9.9 netmask 0xff000000\n"):
        measure.interface_text = reader
        try:
            told.append(measure.interfaces())
        finally:
            measure.interface_text = text_of_the_minute
    late_line = "not read: Command 'ifconfig' timed out after 30 seconds"
    return "told" if told == [["not read: no reader"], [late_line], ["none read"], ["none read"]] else "untold"


def an_unbound_listener_reads_not_measured():
    with open(os.path.join(directory, "counts-no-layer-steps.json"), "w", encoding="utf-8") as f:
        json.dump({"counts": {"s03-ip": {"connections": 1, "requests": 1}, "s18-ip": {"packets": 21, "marked": 21}}, "seen": [], "listen_failures": []}, f)
    for label in ("engine-alone-steps", "no-layer-steps"):
        with open(os.path.join(directory, f"{label}.err"), "w", encoding="utf-8") as f:
            f.write("".join(f"halvo-measure {key} at-app-origin=true\n" for key in ("s03-ip", "s05-ip", "s18-ip")) + "halvo-measure done\n")
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        code = measure.judge(directory, "macos")
    with open(os.path.join(directory, "judged.txt"), "w", encoding="utf-8", newline="\n") as f:
        f.write(out.getvalue())
    rows = [line for line in out.getvalue().splitlines() if line.startswith(("measure row: s03 ip engine-alone ", "measure row: s18 ip engine-alone "))]
    want = [f"measure row: {step} ip engine-alone 0 0 0 0 0 true UNEXPECTED not measured (listener)" for step in ("s03", "s18")]
    if code != 1:
        return f"judge-exit-{code}"
    return "not-measured" if rows == want else "measured"


def a_loopback_read_is_refused():
    elsewhere = os.path.join(directory, "loopback")
    route[0] = "127.0.0.1"
    try:
        quiet(measure.plan, elsewhere)
        refused = False
    except AssertionError:
        refused = not os.path.exists(os.path.join(elsewhere, "plan.json"))
    finally:
        route[0] = "10.0.0.5"
    quiet(measure.plan, elsewhere)
    with open(os.path.join(elsewhere, "plan.json"), encoding="utf-8") as f:
        ip = json.load(f)["ip"]
    if ip != "10.0.0.5":
        return f"read-{ip}"
    return "refused" if refused else "let-through"


def case(label, want, run):
    try:
        got = run()
    except Exception as e:
        got = f"error-{type(e).__name__}"
    print(f"case {NAME}/{label} want={want} got={got} {'right' if got == want else 'wrong'}")


case("refused-binds-written-and-told", "told", refused_binds_are_written_and_told)
case("taken-binds-tell-the-address-alone", "asked-alone", taken_binds_tell_the_address_alone)
case("one-refused-bind-told-with-the-interfaces", "told", one_refused_bind_is_told_with_the_interfaces)
case("refused-binds-elsewhere-told-by-counts", "counted", refused_binds_elsewhere_are_told_by_counts)
case("each-refused-bind-asked-once", "once", each_refused_bind_is_asked_once)
case("windows-interfaces-read", "read", windows_interfaces_are_read)
case("unread-interfaces-told", "told", an_unread_list_is_told)
case("unbound-listener-reads-not-measured", "not-measured", an_unbound_listener_reads_not_measured)
case("loopback-read-refused", "refused", a_loopback_read_is_refused)
CASESEOF
}

write_stubs() {
  mkdir -p "$1" || exit 2
  cat > "$1/record" <<'STUBEOF'
#!/bin/sh
tool=${0##*/}
printf '%s %s\n' "$tool" "$*" >> "$FIXTURE_CALLS"
case "$tool:$*" in
  'netsh:advfirewall show allprofiles state')
    for profile in Domain Private Public; do
      printf '%s Profile Settings:\n----------------------------------------------------------------------\nState                                 %s\n\n' "$profile" "$(sed -n "s/^$profile //p" "$FIXTURE_STATE")"
    done
    ;;
  'netsh:advfirewall set '*'profile state '*)
    case $3 in
      domainprofile) profile=Domain ;;
      privateprofile) profile=Private ;;
      publicprofile) profile=Public ;;
      *) exit 1 ;;
    esac
    value=$(printf '%s' "$5" | tr '[:lower:]' '[:upper:]')
    sed "s/^$profile .*/$profile $value/" "$FIXTURE_STATE" > "$FIXTURE_STATE.new" && mv "$FIXTURE_STATE.new" "$FIXTURE_STATE"
    echo Ok.
    ;;
  cargo:*)
    if [ -n "${FIXTURE_STOP:-}" ]; then kill "-$FIXTURE_STOP" "$PPID"; fi
    if [ "${FIXTURE_LAUNCH:-}" = stop ]; then
      mkdir -p "$CARGO_TARGET_DIR/debug"
      printf '#!/bin/sh\necho "$$" >> "$FIXTURE_PIDS"\nkill -TERM "$PPID"\nexec sleep 30\n' > "$CARGO_TARGET_DIR/debug/halvo"
      chmod +x "$CARGO_TARGET_DIR/debug/halvo"
      exit 0
    fi
    if [ "${FIXTURE_LAUNCH:-}" = run ]; then
      mkdir -p "$CARGO_TARGET_DIR/debug"
      printf '#!/bin/sh\necho page=loaded\n' > "$CARGO_TARGET_DIR/debug/halvo"
      chmod +x "$CARGO_TARGET_DIR/debug/halvo"
      exit 0
    fi
    exit 1
    ;;
  git:archive*)
    if [ "${FIXTURE_ARCHIVE:-}" = fail ]; then exit 1; fi
    tar -cf "$3" -C "$FIXTURE_TREE" .
    ;;
  python*:*copies.py*)
    if [ "${FIXTURE_COPIES:-}" = fail ]; then exit 1; fi
    if [ "${FIXTURE_COPIES:-}" = keeps-layer ] && [ "$2" = engine-alone ]; then exit 0; fi
    sed '/on_navigation/d;/initialization_script_for_all_frames/d' src-tauri/src/app.rs > src-tauri/src/app.rs.new && mv src-tauri/src/app.rs.new src-tauri/src/app.rs
    if [ "$2" = no-layer ]; then echo HALVO_MEASURE_NEVER_SET >> src-tauri/src/engine.rs; fi
    ;;
  python*:*measure.py\ plan*)
    if [ "${FIXTURE_PLAN:-}" = fail ]; then exit 1; fi
    if [ "${FIXTURE_PLAN:-}" = fail-second ] && [ "$(grep -c 'measure\.py plan ' "$FIXTURE_CALLS")" = 2 ]; then exit 1; fi
    if [ "${FIXTURE_LAUNCH:-}" = run ]; then printf 's03-ip\tplan %s\n' "$(grep -c 'measure\.py plan ' "$FIXTURE_CALLS")" > "$3/steps.tsv"; fi
    ;;
  python*:*measure.py\ listen*)
    if [ "${FIXTURE_LAUNCH:-}" = stop ]; then
      : > "$3/ready-$4"
      echo "$$" >> "$FIXTURE_PIDS"
      exec sleep 30
    fi
    if [ -n "${FIXTURE_READY_LATE:-}" ]; then
      while [ ! -f "$3/stop-$4" ]; do sleep 0.1; done
      cp "$FIXTURE_READY_LATE" "$3/ready-$4"
    fi
    if [ -n "${FIXTURE_READY:-}" ]; then cp "$FIXTURE_READY" "$3/ready-$4"; fi
    ;;
  python*:*measure.py\ judge*)
    if [ -n "${FIXTURE_JUDGED:-}" ]; then cat "$FIXTURE_JUDGED"; fi
    ;;
  sw_vers:*) echo 15.8 ;;
  defaults:*) echo 20621 ;;
esac
exit 0
STUBEOF
  for tool in netsh cargo git powershell python3 python sw_vers defaults; do
    cp "$1/record" "$1/$tool" && chmod +x "$1/$tool" || exit 2
  done
}

run_cases() {
  stubs="$work/stubs"
  write_stubs "$stubs"
  mkdir -p "$work/repo" "$work/tree/src-tauri/src" || exit 2
  printf '        .on_navigation(|url| may_navigate(url.as_str()))\n        .initialization_script_for_all_frames(NO_PEER_SCRIPT)\n' > "$work/tree/src-tauri/src/app.rs"
  printf 'pub fn guard<R: Runtime>(window: &WebviewWindow<R>, site: EngineSite, answer: Sender<GuardAnswer>) {\n' > "$work/tree/src-tauri/src/engine.rs"
  good > "$work/judged-good.txt"
  run_fixture() {
    label=$1 hosted=$2 job=$3
    shift 3
    : > "$work/$label.calls"
    : > "$work/$label.pids"
    printf 'Domain ON\nPrivate OFF\nPublic ON\n' > "$work/$label.state"
    if [ "$hosted" = unset ]; then
      env -u GITHUB_ACTIONS PATH="$stubs:$PATH" FIXTURE_CALLS="$work/$label.calls" FIXTURE_STATE="$work/$label.state" CI_D_REPO="$work/repo" CI_D_JOB="$job" CI_D_ARTIFACTS= RUNNER_TEMP="$work" sh "$self" "$job" > "$work/$label.out" 2>&1
    else
      env PATH="$stubs:$PATH" GITHUB_ACTIONS="$hosted" FIXTURE_CALLS="$work/$label.calls" FIXTURE_STATE="$work/$label.state" FIXTURE_PIDS="$work/$label.pids" FIXTURE_TREE="$work/tree" CI_D_REPO="$work/repo" CI_D_JOB="$job" CI_D_ARTIFACTS= RUNNER_TEMP="$work" "$@" sh "$self" "$job" > "$work/$label.out" 2>&1
    fi
    echo "$?" > "$work/$label.code"
  }
  calls_of() { grep -c "^$2" "$work/$1.calls"; }
  is() { [ "$1" = "$2" ] || got=$3; }
  report() {
    label=$1 got=$2
    code=$(cat "$work/$label.code")
    calls=$(grep -c '' "$work/$label.calls")
    last=$(tail -n 1 "$work/$label.out")
    printf 'case %s/%s want=refused got=%s exit=%s calls=%s last: %s\n' "$name" "$label" "$got" "$code" "$calls" "$last"
    if [ "$got" = refused ]; then rejected=$((rejected + 1)); else wrong="$wrong $label"; fi
  }
  for label in outside-a-hosted-job unset-outside-a-hosted-job; do
    if [ "$label" = outside-a-hosted-job ]; then run_fixture "$label" false windows; else run_fixture "$label" unset windows; fi
    got=refused
    code=$(cat "$work/$label.code")
    is "$code" 77 ran
    last=$(tail -n 1 "$work/$label.out")
    case $last in "SKIP $name (outside a hosted job: "*) ;; *) got=ran ;; esac
    [ -s "$work/$label.calls" ] && got=ran
    report "$label" "$got"
  done
  before="$name firewall before: domain=ON private=OFF public=ON"
  after="$name firewall after: domain=ON private=OFF public=ON (on again: domain public)"
  put_back_holds() {
    grep -qxF "$before" "$work/$1.out" || got=left
    grep -qxF "$after" "$work/$1.out" || got=left
    n=$(calls_of "$1" 'netsh advfirewall set domainprofile state on$')
    is "$n" 1 left
    n=$(calls_of "$1" 'netsh advfirewall set publicprofile state on$')
    is "$n" 1 left
    n=$(calls_of "$1" 'netsh advfirewall set privateprofile')
    is "$n" 0 left
    n=$(calls_of "$1" 'netsh advfirewall set allprofiles')
    is "$n" 0 left
    state=$(sed -n 's/^Domain //p;s/^Private //p;s/^Public //p' "$work/$1.state" | paste -s -d ' ' -)
    is "$state" 'ON OFF ON' left
  }
  run_fixture stopped-in-the-middle true windows FIXTURE_STOP=TERM
  got=refused
  put_back_holds stopped-in-the-middle
  code=$(cat "$work/stopped-in-the-middle.code")
  is "$code" 143 left
  builds=$(calls_of stopped-in-the-middle 'cargo ')
  is "$builds" 1 left
  report stopped-in-the-middle "$got"
  run_fixture normal-path true windows
  got=refused
  put_back_holds normal-path
  code=$(cat "$work/normal-path.code")
  is "$code" 1 left
  builds=$(calls_of normal-path 'cargo ')
  is "$builds" 2 left
  last=$(tail -n 1 "$work/normal-path.out")
  case $last in "FAIL $name ("*) ;; *) got=left ;; esac
  at=$(grep -n -xF "$after" "$work/normal-path.out" | cut -d: -f1)
  lines=$(grep -c '' "$work/normal-path.out")
  [ "${at:-$lines}" -lt "$lines" ] || got=left
  report normal-path "$got"
  for row in "archive-fails FIXTURE_ARCHIVE=fail git archive HEAD failed for the copy engine-alone" "copy-maker-fails FIXTURE_COPIES=fail the copy maker failed for the copy engine-alone" "engine-alone-keeps-a-page-layer FIXTURE_COPIES=keeps-layer the copy engine-alone does not hold the change that it names"; do
    label=${row%% *}
    rest=${row#* }
    setting=${rest%% *}
    reason=${rest#* }
    run_fixture "$label" true windows "$setting"
    got=refused
    put_back_holds "$label"
    code=$(cat "$work/$label.code")
    is "$code" 1 ran
    builds=$(calls_of "$label" 'cargo ')
    is "$builds" 0 ran
    last=$(tail -n 1 "$work/$label.out")
    is "$last" "FAIL $name ($reason)" ran
    report "$label" "$got"
  done
  run_fixture stopped-during-a-launch true windows FIXTURE_LAUNCH=stop
  got=refused
  put_back_holds stopped-during-a-launch
  code=$(cat "$work/stopped-during-a-launch.code")
  is "$code" 143 left
  started=$(grep -c '' "$work/stopped-during-a-launch.pids")
  is "$started" 2 left
  while read -r pid; do
    if kill -0 "$pid" 2>/dev/null; then
      got=left
      kill "$pid" 2>/dev/null
    fi
  done < "$work/stopped-during-a-launch.pids"
  report stopped-during-a-launch "$got"
  printf 'measure row: s20 ip engine-alone 0 0 0 0 0 false UNEXPECTED window not-read\nmeasure row: s20 ip no-layer 0 0 0 0 0 true control 0\n' >> "$work/judged-good.txt"
  run_fixture macos-asks-no-s20 true macos FIXTURE_JUDGED="$work/judged-good.txt"
  got=refused
  code=$(cat "$work/macos-asks-no-s20.code")
  is "$code" 0 left
  printed=$(grep -c "^$name measure row: s20 " "$work/macos-asks-no-s20.out")
  is "$printed" 0 left
  shown=$(grep -c "^$name measure row: s03 " "$work/macos-asks-no-s20.out")
  is "$shown" 2 left
  n=$(calls_of macos-asks-no-s20 'netsh ')
  is "$n" 0 left
  report macos-asks-no-s20 "$got"
  printf "s03-ip port 47030: [Errno 49] Can't assign requested address\r\nasked 203.0.113.9\r\ninterface en0 8863<UP,BROADCAST> inet 10.0.0.5\r\n" > "$work/ready-made-up"
  mkdir -p "$work/after-each-build" || exit 2
  run_fixture address-read-after-each-build true macos FIXTURE_LAUNCH=run FIXTURE_READY="$work/ready-made-up" FIXTURE_JUDGED="$work/judged-good.txt" RUNNER_TEMP="$work/after-each-build"
  got=refused
  code=$(cat "$work/address-read-after-each-build.code")
  is "$code" 0 left
  order=$(sed -n -e 's/^cargo .*/build/p' -e 's/^python[0-9]* .*measure\.py plan .*/plan/p' -e 's/^python[0-9]* .*measure\.py listen .*/listen/p' "$work/address-read-after-each-build.calls" | paste -s -d ' ' -)
  is "$order" "build plan listen build plan listen" left
  for phase in engine-alone no-layer; do
    n=$(grep -c -x -F -e "$name steps of $phase: s03-ip" -e "$name listeners $phase-steps: asked 203.0.113.9, 1 refused" -e "$name interface at a refused bind of $phase-steps: en0 8863<UP,BROADCAST> inet 10.0.0.5" "$work/address-read-after-each-build.out")
    is "$n" 3 left
    at=$(grep -n -x -F "$name listeners $phase-steps: asked 203.0.113.9, 1 refused" "$work/address-read-after-each-build.out" | cut -d: -f1)
    launched=$(grep -n "^$name launch $phase: " "$work/address-read-after-each-build.out" | cut -d: -f1)
    [ "${at:-0}" -gt 0 ] && [ "${at:-0}" -lt "${launched:-0}" ] || got=left
  done
  routes=$(cat "$work/after-each-build/$name".*/routes.tsv)
  tab=$(printf '\t')
  is "$routes" "s03-ip${tab}plan 2" left
  report address-read-after-each-build "$got"
  run_fixture a-failed-plan-fails-the-check true macos FIXTURE_LAUNCH=run FIXTURE_PLAN=fail
  got=refused
  code=$(cat "$work/a-failed-plan-fails-the-check.code")
  is "$code" 1 ran
  last=$(tail -n 1 "$work/a-failed-plan-fails-the-check.out")
  is "$last" "FAIL $name (the plan could not read an address of the runner for the copy engine-alone)" ran
  n=$(grep -c 'measure\.py listen ' "$work/a-failed-plan-fails-the-check.calls")
  is "$n" 0 ran
  report a-failed-plan-fails-the-check "$got"
  run_fixture second-copy-failed-plan-fails-the-check true macos FIXTURE_LAUNCH=run FIXTURE_PLAN=fail-second FIXTURE_READY="$work/ready-made-up"
  got=refused
  code=$(cat "$work/second-copy-failed-plan-fails-the-check.code")
  is "$code" 1 ran
  last=$(tail -n 1 "$work/second-copy-failed-plan-fails-the-check.out")
  is "$last" "FAIL $name (the plan could not read an address of the runner for the copy no-layer)" ran
  order=$(sed -n -e 's/^cargo .*/build/p' -e 's/^python[0-9]* .*measure\.py plan .*/plan/p' -e 's/^python[0-9]* .*measure\.py listen .* engine-alone-steps$/listen-engine-alone/p' -e 's/^python[0-9]* .*measure\.py listen .*/listen-other/p' "$work/second-copy-failed-plan-fails-the-check.calls" | paste -s -d ' ' -)
  is "$order" "build plan listen-engine-alone build plan" ran
  report second-copy-failed-plan-fails-the-check "$got"
  mkdir -p "$work/fast" || exit 2
  printf '#!/bin/sh\nexit 0\n' > "$work/fast/sleep"
  chmod +x "$work/fast/sleep" || exit 2
  run_fixture late-ready-file-printed-once true macos FIXTURE_LAUNCH=run FIXTURE_READY_LATE="$work/ready-made-up" FIXTURE_JUDGED="$work/judged-good.txt" PATH="$work/fast:$stubs:$PATH"
  got=refused
  code=$(cat "$work/late-ready-file-printed-once.code")
  is "$code" 0 left
  for phase in engine-alone-steps no-layer-steps; do
    n=$(grep -c -x -F -e "$name listeners $phase: asked 203.0.113.9, 1 refused" -e "$name interface at a refused bind of $phase: en0 8863<UP,BROADCAST> inet 10.0.0.5" "$work/late-ready-file-printed-once.out")
    is "$n" 2 left
  done
  report late-ready-file-printed-once "$got"
}

hosted_only() {
  [ "${GITHUB_ACTIONS:-}" = true ] && return 0
  printf 'SKIP %s (outside a hosted job: GITHUB_ACTIONS is not true, so the check turns no firewall profile off, builds nothing and starts no app; the hosted jobs run it)\n' "$name"
  exit 77
}

firewall_state() {
  netsh advfirewall show allprofiles state | tr -d '\r' | awk '
    /^Domain Profile/ { p = "domain" }
    /^Private Profile/ { p = "private" }
    /^Public Profile/ { p = "public" }
    /^State[ \t]/ && p != "" { printf "%s%s=%s", sep, p, $NF; sep = " "; p = "" }
    END { print "" }'
}

firewall_put_back() {
  [ "${firewall_taken:-0}" = 1 ] || return 0
  for profile in ${firewall_off:-}; do
    netsh advfirewall set "${profile}profile" state on > /dev/null
  done
  firewall_after=$(firewall_state)
  echo "$name firewall after: $firewall_after (on again:${firewall_off:- none})"
  firewall_taken=0
}

own_pids=

end_own() {
  for pid in ${own_pids:-}; do
    kill "$pid" 2>/dev/null
  done
  own_pids=
}

forget_own() {
  left=
  for pid in ${own_pids:-}; do
    [ "$pid" = "$1" ] || left="$left $pid"
  done
  own_pids=$left
}

put_back() {
  end_own
  firewall_put_back
}

stop_with() {
  trap '' HUP INT TERM
  put_back
  trap - EXIT
  exit "$1"
}

take_traps() {
  trap 'put_back' EXIT
  trap 'stop_with 129' HUP
  trap 'stop_with 130' INT
  trap 'stop_with 143' TERM
}

fail() {
  put_back
  printf 'FAIL %s (%s)\n' "$name" "$1"
  exit 1
}

copy_made() {
  app="$2/src-tauri/src/app.rs"
  engine="$2/src-tauri/src/engine.rs"
  [ -f "$app" ] && [ -f "$engine" ] || return 1
  navigation=$(grep -c '\.on_navigation(|url| may_navigate(url\.as_str()))' "$app")
  script=$(grep -c '\.initialization_script_for_all_frames(NO_PEER_SCRIPT)' "$app")
  off=$(grep -c 'HALVO_MEASURE_NEVER_SET' "$engine")
  want=0
  [ "$1" = no-layer ] && want=1
  [ "$navigation" = 0 ] && [ "$script" = 0 ] && [ "$off" = "$want" ]
}

firewall_take() {
  firewall_before=$(firewall_state)
  echo "$name firewall before: $firewall_before"
  firewall_off=
  firewall_taken=1
  for profile in domain private public; do
    case " $firewall_before " in
      *" $profile=ON "*)
        firewall_off="$firewall_off $profile"
        netsh advfirewall set "${profile}profile" state off > /dev/null
        ;;
    esac
  done
  echo "$name firewall turned off:${firewall_off:- none}"
}

write_helpers() {
  cat > "$1/measure.py" <<'HELPEREOF'
"""The measurement of ruling AP-9 point 7 for app-2, run by a scratch workflow step on a hosted
runner. Never shipped.

usage:
  python measure.py plan <dir>            writes <dir>/plan.json, <dir>/steps.tsv, <dir>/init.js,
                                          <dir>/hosts.txt and prints the runner's address
  python measure.py listen <dir> <label>  serves every listener until <dir>/stop exists, then writes
                                          <dir>/counts-<label>.json
  python measure.py judge <dir> <job>     prints the table and exits 1 on a count that differs
                                          from the expected one
"""
import json
import os
import re
import socket
import struct
import subprocess
import sys
import threading
import time

BASE = 47000
FORMS = ("ip", "host", "dns", "off4", "off6", "ll", "port")
LL_STEPS = ("s03", "s12", "s20")
HINT_STEPS = ("s06", "s07")
REFUSED_WITH_PORT = "http://xn--zz.com:{port}/p?q#f"
A2_CONTROL_HOST = "a2control.halvo-hosts.test"
OFF4 = "198.51.100.7"
OFF6 = "2001:db8::7"
OFF_STEPS = ("s03", "s04", "s10")
LOOP_BASE = 48000
LOOP_FORMS = ("lo4", "lname", "lo6", "tauri", "ipc")
LOOP_HOSTS = {"lo4": "127.0.0.1", "lname": "localhost", "lo6": "[::1]", "tauri": "tauri.localhost", "ipc": "ipc.localhost"}
LOOP_STEPS = ("s01", "s03", "s08", "s11", "s12", "s13", "s14", "s15", "s16", "s20")
HOOK_STEPS = ("s13", "s14", "s15", "s06")
PROBE_PORTS = {"setup": 9999, "before": 9998, "after": 9997}
PEER_STEPS = ("s16", "s18", "s19", "s20", "s21", "s22")
MARKS = (b"HALVOMARK", b"HALVOUSER")
HOSTS_DOMAIN = "halvo-hosts.test"
DNS_DOMAIN = "halvo-measure.test"
REFUSED_FORMS = (("http", "http://xn--zz.com/p?q#f", 80), ("https", "https://xn--zz.com/p?q#f", 443))
REFUSED_HOST = "xn--zz.com"
DNS_PORT = 53 if os.name == "nt" else 5354

STEPS = [
    ("s01", "navigation refused with a query", "location.href = U('/nav?d=x');"),
    ("s02", "navigation to a form the reader refuses", None),
    ("s03", "link clicked by script", "var a = document.createElement('a'); a.href = U('/link?d=x'); document.body.appendChild(a); a.click();"),
    ("s04", "link with a target", "var a = document.createElement('a'); a.href = U('/target?d=x'); a.target = '_blank'; document.body.appendChild(a); a.click();"),
    ("s05", "window.open", "window.open(U('/open?d=x'));"),
    ("s06", "preconnect hint", "var l = document.createElement('link'); l.rel = 'preconnect'; l.href = U('/'); document.head.appendChild(l);"),
    ("s07", "dns-prefetch hint", "var l = document.createElement('link'); l.rel = 'dns-prefetch'; l.href = U('/'); document.head.appendChild(l);"),
    ("s08", "frame", "var f = document.createElement('iframe'); f.src = U('/frame?d=x'); document.body.appendChild(f);"),
    ("s09", "form post with a body", "var f = document.createElement('form'); f.method = 'post'; f.action = U('/form'); var i = document.createElement('input'); i.name = 'd'; i.value = 'x'; f.appendChild(i); document.body.appendChild(f); f.submit();"),
    ("s10", "download from another host", "var a = document.createElement('a'); a.href = U('/download?d=x'); a.download = 'halvo-measure.bin'; document.body.appendChild(a); a.click();"),
    ("s11", "image", "var m = new Image(); m.src = U('/img?d=x'); document.body.appendChild(m);"),
    ("s12", "fetch", "fetch(U('/fetch?d=x'), {method: 'POST', body: 'x', mode: 'no-cors'}).catch(function () {});"),
    ("s13", "sendBeacon", "try { navigator.sendBeacon(U('/beacon'), 'd=x'); } catch (e) {}"),
    ("s14", "WebSocket", "try { new WebSocket(U('/ws?d=x').replace('http:', 'ws:')); } catch (e) {}"),
    ("s15", "worker's fetch", "try { new Worker(URL.createObjectURL(new Blob([\"fetch('\" + U('/worker?d=x') + \"', {mode: 'no-cors'}).catch(function () {});\"], {type: 'text/javascript'}))); } catch (e) {}"),
    ("s16", "peer connection", "try { var p = new RTCPeerConnection({iceServers: [{urls: 'stun:' + H + ':' + P}]}); p.onicecandidate = function (e) { if (e.candidate) { window.__halvoNote = (window.__halvoNote || '') + e.candidate.type + '/' + (e.candidate.protocol || '') + ','; } }; p.createDataChannel('d'); p.createOffer().then(function (o) { return p.setLocalDescription(o); }).catch(function () {}); window.__halvoPeer16 = p; } catch (e) { window.__halvoNote = 'error ' + e.name; }"),
    ("s17", "navigation by an initialization script before the page's first script", None),
    ("s18", "peer connection with a remote description whose fragment is a marker", "try { var p = new RTCPeerConnection({iceServers: []}); p.onicecandidate = function (e) { if (e.candidate) { window.__halvoNote = (window.__halvoNote || '') + e.candidate.type + '/' + (e.candidate.protocol || '') + ','; } }; p.createDataChannel('d'); p.createOffer().then(function (o) { return p.setLocalDescription(o).then(function () { var s = o.sdp.replace(/a=ice-ufrag:[^\\r\\n]*/g, 'a=ice-ufrag:HALVOMARK18').replace(/a=ice-pwd:[^\\r\\n]*/g, 'a=ice-pwd:halvomarkhalvomarkhalvo18').replace(/a=setup:actpass/g, 'a=setup:active'); return p.setRemoteDescription({type: 'answer', sdp: s}); }).then(function () { return p.addIceCandidate({candidate: 'candidate:1 1 udp 2122260223 ' + H + ' ' + P + ' typ host', sdpMid: '0', sdpMLineIndex: 0}); }); }).catch(function (e) { window.__halvoNote = (window.__halvoNote || '') + 'error ' + e.name; }); window.__halvoPeer18 = p; } catch (e) { window.__halvoNote = 'error ' + e.name; }"),
    ("s19", "peer connection with a turn: address and a user name over UDP", "try { var p = new RTCPeerConnection({iceServers: [{urls: 'turn:' + H + ':' + P + '?transport=udp', username: 'HALVOUSER19', credential: 'halvo'}]}); p.onicecandidate = function (e) { if (e.candidate) { window.__halvoNote = (window.__halvoNote || '') + e.candidate.type + '/' + (e.candidate.protocol || '') + ','; } }; p.createDataChannel('d'); p.createOffer().then(function (o) { return p.setLocalDescription(o); }).catch(function () {}); window.__halvoPeer19 = p; } catch (e) { window.__halvoNote = 'error ' + e.name; }"),
    ("s20", "peer connection with a turn: address and a user name over TCP", "try { var p = new RTCPeerConnection({iceServers: [{urls: 'turn:' + H + ':' + P + '?transport=tcp', username: 'HALVOUSER20', credential: 'halvo'}]}); p.onicecandidate = function (e) { if (e.candidate) { window.__halvoNote = (window.__halvoNote || '') + e.candidate.type + '/' + (e.candidate.protocol || '') + ','; } }; p.createDataChannel('d'); p.createOffer().then(function (o) { return p.setLocalDescription(o); }).catch(function () {}); window.__halvoPeer20 = p; } catch (e) { window.__halvoNote = 'error ' + e.name; }"),
    ("s22", "WebTransport", "try { var t = new WebTransport('https://' + H + ':' + P + '/wt?d=x'); t.ready.catch(function (e) { window.__halvoNote = 'wt ' + (e && e.name); }); window.__halvoWT = t; } catch (e) { window.__halvoNote = 'error ' + e.name; }"),
    ("s23", "prefetch link", "var l = document.createElement('link'); l.rel = 'prefetch'; l.href = U('/prefetch?d=x'); document.head.appendChild(l);"),
    ("s24", "speculation rules", "try { var s = document.createElement('script'); s.type = 'speculationrules'; s.textContent = JSON.stringify({prefetch: [{source: 'list', urls: [U('/spec?d=x')]}]}); document.head.appendChild(s); } catch (e) { window.__halvoNote = 'error ' + e.name; }"),
    ("s21", "peer connection from the constructor of a new empty frame", "try { var f = document.createElement('iframe'); document.body.appendChild(f); var C = f.contentWindow.RTCPeerConnection; var p = new C({iceServers: [{urls: 'stun:' + H + ':' + P}]}); p.onicecandidate = function (e) { if (e.candidate) { window.__halvoNote = (window.__halvoNote || '') + e.candidate.type + '/' + (e.candidate.protocol || '') + ','; } }; p.createDataChannel('d'); p.createOffer().then(function (o) { return p.setLocalDescription(o); }).catch(function () {}); window.__halvoPeer21 = p; } catch (e) { window.__halvoNote = 'error ' + e.name; }"),
]


def runner_address():
    probe = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        probe.connect(("192.0.2.1", 9))
        return probe.getsockname()[0]
    finally:
        probe.close()


def port_of(step, form):
    if form in LOOP_FORMS:
        return LOOP_BASE + int(step[1:]) * 10 + LOOP_FORMS.index(form)
    return BASE + int(step[1:]) * 10 + FORMS.index(form)


def has_v6_loopback():
    try:
        s = socket.socket(socket.AF_INET6, socket.SOCK_STREAM)
        s.bind(("::1", 0))
        s.close()
        return True
    except OSError:
        return False


def loop_forms():
    forms = ["lo4", "lname"]
    if has_v6_loopback():
        forms.append("lo6")
    if os.name == "nt":
        forms += ["tauri", "ipc"]
    return forms


def loop_binds(form):
    v6 = has_v6_loopback()
    if form == "lo4":
        return [("127.0.0.1", socket.AF_INET)]
    if form == "lo6":
        return [("::1", socket.AF_INET6)]
    return [("127.0.0.1", socket.AF_INET)] + ([("::1", socket.AF_INET6)] if v6 else [])


def forms_of(step):
    if step == "s17":
        return ("ip",)
    forms = ["ip", "host", "dns"]
    if step in OFF_STEPS:
        forms += ["off4", "off6"]
    if step in LL_STEPS and os.environ.get("HALVO_MEASURE_LL"):
        forms.append("ll")
    return tuple(forms)


def host_of(step, form, ip):
    if form in LOOP_FORMS:
        return LOOP_HOSTS[form]
    if form == "off4":
        return OFF4
    if form == "off6":
        return f"[{OFF6}]"
    if form == "ll":
        return os.environ["HALVO_MEASURE_LL"]
    if form == "ip":
        return ip
    if form == "host":
        return f"{step}.{HOSTS_DOMAIN}"
    return f"{step}.{DNS_DOMAIN}"


def plan(directory):
    ip = os.environ.get("HALVO_MEASURE_TEST_IP") or runner_address()
    assert not ip.startswith("127.") or os.environ.get("HALVO_MEASURE_TEST_IP"), ip
    no_filter = os.name == "nt"
    targets = []
    lines = []
    off_lines = []
    hint_lines = []
    loop_lines = []
    hook_lines = []
    for step, label, script in STEPS:
        if step == "s02":
            for form, address, port in REFUSED_FORMS:
                if os.name == "nt" and port == 80:
                    print(f"measure s02-{form} is not measurable on Windows: port 80 is held by the system (WinError 10013, every run of this harness)", file=sys.stderr)
                    continue
                targets.append({"step": step, "form": form, "host": REFUSED_HOST, "port": port})
                lines.append(f"{step}-{form}\tlocation.href = {json.dumps(address)};")
            port = port_of("s02", "port")
            targets.append({"step": step, "form": "port", "host": REFUSED_HOST, "port": port})
            lines.append(f"{step}-port\tlocation.href = {json.dumps(REFUSED_WITH_PORT.format(port=port))};")
            continue
        for form in forms_of(step):
            host = host_of(step, form, ip)
            port = port_of(step, form)
            target = {"step": step, "form": form, "host": host, "port": port, "pf": form.startswith("off")}
            if form == "ll":
                target["bind"] = host
            targets.append(target)
            if step == "s17":
                continue
            prefix = f"var H = {json.dumps(host)}, P = {port}; var U = function (p) {{ return 'http://' + H + ':' + P + p; }}; "
            line = f"{step}-{form}\t(function () {{ {prefix}{script} }})();"
            if form.startswith("off") and no_filter:
                off_lines.append(line)
            else:
                lines.append(line)
            if step in HINT_STEPS:
                hint_lines.append(line)
            if step in HOOK_STEPS and form in ("ip", "host"):
                hook_lines.append(line)
        if step in LOOP_STEPS or step == "s06":
            for form in loop_forms():
                host = host_of(step, form, ip)
                port = port_of(step, form)
                targets.append({"step": step, "form": form, "host": host, "port": port, "loop": True})
                prefix = f"var H = {json.dumps(host)}, P = {port}; var U = function (p) {{ return 'http://' + H + ':' + P + p; }}; "
                line = f"{step}-{form}\t(function () {{ {prefix}{script} }})();"
                if step == "s06":
                    hint_lines.append(line)
                else:
                    loop_lines.append(line)
    init_js = (
        "(function () { if (/localhost$/.test(location.hostname)) { "
        f"location.href = 'http://{ip}:{port_of('s17', 'ip')}/init?d=x'; }} }})();"
    )
    pairs = []
    for step, rel in (("s06", "preconnect"), ("s07", "dns-prefetch")):
        for form in ("ip", "host", "dns"):
            pairs.append([rel, f"http://{host_of(step, form, ip)}:{port_of(step, form)}/"])
    for form in loop_forms():
        pairs.append(["preconnect", f"http://{host_of('s06', form, ip)}:{port_of('s06', form)}/"])
    hints_js = (
        "(function () { if (!/localhost$/.test(location.hostname)) { return; } var pairs = " + json.dumps(pairs) + "; "
        "var add = function () { pairs.forEach(function (p) { var l = document.createElement('link'); l.rel = p[0]; l.href = p[1]; document.head.appendChild(l); }); }; "
        "if (document.head) { add(); } else { document.addEventListener('DOMContentLoaded', add); } })();"
    )
    a2_lines = [f"a2-00\tlocation.href = {json.dumps('http://' + A2_CONTROL_HOST + '/first')};"]
    a2_hosts = []
    for n, form in enumerate(REFUSED_BY_THE_READER, 1):
        host = form.split("://", 1)[1].split("/", 1)[0]
        if host and host not in a2_hosts:
            a2_hosts.append(host)
        a2_lines.append(f"a2-{n:02d}\tlocation.href = {json.dumps(form)};")
    a2_lines.append(f"a2-71\tlocation.href = {json.dumps('http://' + A2_CONTROL_HOST + '/last')};")
    hosts = [f"{ip} {REFUSED_HOST}", f"{ip} {A2_CONTROL_HOST}"] + [f"{ip} {h}" for h in a2_hosts if h != REFUSED_HOST] + [
        f"{ip} {host_of(step, 'host', ip)}" for step, _, _ in STEPS if step != "s02"
    ]
    os.makedirs(directory, exist_ok=True)
    files = {
        "steps.tsv": lines, "offsteps.tsv": off_lines, "hintsteps.tsv": hint_lines,
        "loopsteps.tsv": loop_lines, "hooksteps.tsv": hook_lines,
        "reload.tsv": [
            "reload-1\t(function () { window.__halvoBefore = 1; Halvo.loadAgain('after-delete'); })();",
            "reload-2\t(function () { window.__halvoNote = 'before=' + (window.__halvoBefore ? 1 : 0) + ' hash=' + (location.hash || '-') + ' replied=' + Halvo.ipc.replied().slice().sort().join(','); })();",
        ],
        "a2.tsv": a2_lines, "hosts.txt": hosts,
    }
    for name, rows in files.items():
        with open(os.path.join(directory, name), "w", encoding="utf-8", newline="\n") as f:
            f.write("\n".join(rows) + "\n")
    with open(os.path.join(directory, "plan.json"), "w", encoding="utf-8") as f:
        json.dump({"ip": ip, "targets": targets, "dns_port": DNS_PORT, "loop_forms": loop_forms()}, f, indent=1)
    with open(os.path.join(directory, "init.js"), "w", encoding="utf-8", newline="\n") as f:
        f.write(init_js)
    with open(os.path.join(directory, "hints.js"), "w", encoding="utf-8", newline="\n") as f:
        f.write(hints_js)
    with open(os.path.join(directory, "a2-hosts.json"), "w", encoding="utf-8") as f:
        json.dump(a2_hosts, f)
    print(ip)


class Counts:
    def __init__(self):
        self.lock = threading.Lock()
        self.data = {}
        self.seen = []

    def add(self, key, field, note=None):
        with self.lock:
            entry = self.data.setdefault(key, {"connections": 0, "requests": 0, "lookups": 0, "packets": 0, "marked": 0})
            entry[field] += 1
            if note:
                self.seen.append(f"{key} {field} {note}")


REFUSED_BY_THE_READER = [
    'http://xn--a.com/p?q#f',
    'http://xn--a.com/',
    'http://xn--.com/p?q#f',
    'http://xn--.com/',
    'http://xn--zz.com/p?q#f',
    'http://xn--zz.com/',
    'http://xn--pokxncvks/p?q#f',
    'http://xn--pokxncvks/',
    'http://xn--a-ecp.ru/p?q#f',
    'http://xn--a-ecp.ru/',
    'http://xn--xn--a.com/p?q#f',
    'http://xn--xn--a.com/',
    'http://a.xn--b-/p?q#f',
    'http://a.xn--b-/',
    'https://xn--a.com/p?q#f',
    'https://xn--a.com/',
    'https://xn--.com/p?q#f',
    'https://xn--.com/',
    'https://xn--zz.com/p?q#f',
    'https://xn--zz.com/',
    'https://xn--pokxncvks/p?q#f',
    'https://xn--pokxncvks/',
    'https://xn--a-ecp.ru/p?q#f',
    'https://xn--a-ecp.ru/',
    'https://xn--xn--a.com/p?q#f',
    'https://xn--xn--a.com/',
    'https://a.xn--b-/p?q#f',
    'https://a.xn--b-/',
    'ws://xn--a.com/p?q#f',
    'ws://xn--a.com/',
    'ws://xn--.com/p?q#f',
    'ws://xn--.com/',
    'ws://xn--zz.com/p?q#f',
    'ws://xn--zz.com/',
    'ws://xn--pokxncvks/p?q#f',
    'ws://xn--pokxncvks/',
    'ws://xn--a-ecp.ru/p?q#f',
    'ws://xn--a-ecp.ru/',
    'ws://xn--xn--a.com/p?q#f',
    'ws://xn--xn--a.com/',
    'ws://a.xn--b-/p?q#f',
    'ws://a.xn--b-/',
    'ftp://xn--a.com/p?q#f',
    'ftp://xn--a.com/',
    'ftp://xn--.com/p?q#f',
    'ftp://xn--.com/',
    'ftp://xn--zz.com/p?q#f',
    'ftp://xn--zz.com/',
    'ftp://xn--pokxncvks/p?q#f',
    'ftp://xn--pokxncvks/',
    'ftp://xn--a-ecp.ru/p?q#f',
    'ftp://xn--a-ecp.ru/',
    'ftp://xn--xn--a.com/p?q#f',
    'ftp://xn--xn--a.com/',
    'ftp://a.xn--b-/p?q#f',
    'ftp://a.xn--b-/',
    'file://xn--a.com/p?q#f',
    'file://xn--a.com/',
    'file://xn--.com/p?q#f',
    'file://xn--.com/',
    'file://xn--zz.com/p?q#f',
    'file://xn--zz.com/',
    'file://xn--pokxncvks/p?q#f',
    'file://xn--pokxncvks/',
    'file://xn--a-ecp.ru/p?q#f',
    'file://xn--a-ecp.ru/',
    'file://xn--xn--a.com/p?q#f',
    'file://xn--xn--a.com/',
    'file://a.xn--b-/p?q#f',
    'file://a.xn--b-/',
]
assert len(REFUSED_BY_THE_READER) == 70

COOKIE = 0x2112A442


def stun_reply(data, peer):
    if len(data) < 20 or struct.unpack(">I", data[4:8])[0] != COOKIE:
        return None
    kind, _ = struct.unpack(">HH", data[:4])
    txid = data[8:20]
    attrs = b""
    if kind == 0x0001:
        host = socket.inet_aton(peer[0]) if ":" not in peer[0] else None
        if host is None:
            return None
        port = peer[1] ^ (COOKIE >> 16)
        address = struct.unpack(">I", host)[0] ^ COOKIE
        value = struct.pack(">BBHI", 0, 1, port, address)
        attrs = struct.pack(">HH", 0x0020, len(value)) + value
        reply_kind = 0x0101
    elif kind == 0x0003:
        reason = b"Unauthorized"
        value = struct.pack(">HBB", 0, 4, 1) + reason
        attrs = struct.pack(">HH", 0x0009, len(value)) + value + b"\0" * (-len(value) % 4)
        for code, text in ((0x0014, b"halvo"), (0x0015, b"halvononce")):
            attrs += struct.pack(">HH", code, len(text)) + text + b"\0" * (-len(text) % 4)
        reply_kind = 0x0113
    else:
        return None
    return struct.pack(">HHI", reply_kind, len(attrs), COOKIE) + txid + attrs


def note_bytes(data, key, counts, how):
    counts.add(key, "packets" if how == "udp" else "requests", f"{how} first bytes {data[:24].hex()}")
    for mark in MARKS:
        if mark in data:
            counts.add(key, "marked", f"{how} carries {mark.decode()}")


def serve_tcp(sock, key, counts):
    while True:
        try:
            conn, _ = sock.accept()
        except OSError:
            return
        counts.add(key, "connections")
        threading.Thread(target=answer_tcp, args=(conn, key, counts), daemon=True).start()


def answer_tcp(conn, key, counts):
    conn.settimeout(3)
    try:
        data = conn.recv(4096)
        if len(data) >= 20 and struct.unpack(">I", data[4:8])[0] == COOKIE:
            for _ in range(3):
                note_bytes(data, key, counts, "tcp")
                reply = stun_reply(data, conn.getpeername())
                if reply is None:
                    break
                conn.sendall(reply)
                data = conn.recv(4096)
                if not data:
                    break
            return
        if data.split(b" ")[0] in (b"GET", b"POST", b"HEAD", b"OPTIONS", b"PUT"):
            first = data.split(b"\r\n")[0][:100].decode("ascii", "replace")
            counts.add(key, "requests", first)
            if b"/download" in data.split(b"\r\n")[0]:
                head = b"HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ncontent-disposition: attachment; filename=halvo-measure.bin\r\ncontent-length: 2\r\n\r\nok"
            else:
                head = b"HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: 2\r\n\r\nok"
            conn.sendall(head)
    except OSError:
        pass
    finally:
        conn.close()


def serve_udp(sock, key, counts):
    while True:
        try:
            data, peer = sock.recvfrom(4096)
        except OSError:
            return
        note_bytes(data, key, counts, "udp")
        reply = stun_reply(data, peer)
        if reply is not None:
            try:
                sock.sendto(reply, peer)
            except OSError:
                pass


def dns_name(data):
    labels = []
    i = 12
    while i < len(data) and data[i] != 0:
        n = data[i]
        labels.append(data[i + 1:i + 1 + n].decode("ascii", "replace"))
        i += n + 1
    return ".".join(labels), i + 1


def serve_dns(sock, ip, targets, counts):
    by_name = {t["host"].lower(): t for t in targets if t["form"] == "dns"}
    while True:
        try:
            data, peer = sock.recvfrom(4096)
        except OSError:
            return
        if len(data) < 17:
            continue
        name, end = dns_name(data)
        qtype = struct.unpack(">H", data[end:end + 2])[0] if end + 2 <= len(data) else 0
        target = by_name.get(name.lower().rstrip("."))
        key = f"{target['step']}-dns" if target else "other-dns"
        counts.add(key, "lookups", f"{name} type {qtype}")
        question = data[12:end + 4]
        if target and qtype == 1:
            answer = b"\xc0\x0c" + struct.pack(">HHIH", 1, 1, 5, 4) + socket.inet_aton(ip)
            reply = data[:2] + b"\x81\x80" + struct.pack(">HHHH", 1, 1, 0, 0) + question + answer
        else:
            reply = data[:2] + b"\x81\x80" + struct.pack(">HHHH", 1, 0, 0, 0) + question
        try:
            sock.sendto(reply, peer)
        except OSError:
            pass


def bound_socket(kind, address):
    s = socket.socket(socket.AF_INET, kind)
    if os.name != "nt":
        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    try:
        s.bind(address)
    except PermissionError:
        s.bind(("0.0.0.0", address[1]))
    return s


def interface_text():
    command = ["ipconfig"] if os.name == "nt" else ["/sbin/ifconfig", "-a"]
    return subprocess.run(command, capture_output=True, text=True, errors="replace", timeout=30, check=False).stdout


def interface_rows(text):
    rows = []
    for line in text.splitlines():
        if not line.strip():
            continue
        if not line[0].isspace():
            head = line.split(": flags=", 1)
            if len(head) == 2:
                rows.append([head[0], head[1].split(" ", 1)[0]])
            elif line.rstrip().endswith(":"):
                rows.append([line.rstrip()[:-1]])
            continue
        if not rows:
            continue
        words = line.split()
        key, _, value = line.partition(" : ")
        if words[0] in ("inet", "inet6", "status:"):
            rows[-1].append(" ".join(words[:2]))
        elif "Address" in key or "Media State" in key:
            rows[-1].append(value)
    return rows


def interfaces():
    try:
        text = interface_text()
    except (OSError, subprocess.SubprocessError) as e:
        return [f"not read: {e}"]
    return [" ".join(row) for row in interface_rows(text)] or ["none read"]


def told_lines(ip, errnos):
    if os.environ.get("GITHUB_ACTIONS") == "true":
        return [f"interface {line}" for line in interfaces()]
    try:
        rows = interface_rows(interface_text())
        failed = "false"
    except (OSError, subprocess.SubprocessError):
        rows, failed = [], "true"
    words = " ".join(" ".join(row) for row in rows).split()
    asked = "no address asked" if ip is None else f"asked address among them {str(ip in words).lower()}"
    numbers = " ".join(sorted({str(n) for n in errnos}))
    return [f"interfaces read {len(rows)}, reader failed {failed}, {asked}, error numbers {numbers}"]


def failed_keys(failures, keys):
    marked = set()
    for line in failures:
        head = line.split(" ", 1)[0]
        if head == "resolver":
            marked.update(k for k in keys if k.endswith("-dns"))
        elif head in keys:
            marked.add(head)
    return marked


def listen(directory, label):
    with open(os.path.join(directory, "plan.json"), encoding="utf-8") as f:
        planned = json.load(f)
    ip = planned["ip"]
    counts = Counts()
    sockets = []
    failures = []
    errnos = []
    for t in planned["targets"]:
        key = f"{t['step']}-{t['form']}"
        if t.get("pf"):
            continue
        if t.get("loop"):
            for address, family in loop_binds(t["form"]):
                try:
                    s = socket.socket(family, socket.SOCK_STREAM)
                    if os.name != "nt":
                        s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
                    s.bind((address, t["port"]))
                    s.listen(64)
                    sockets.append(s)
                    threading.Thread(target=serve_tcp, args=(s, key, counts), daemon=True).start()
                    if t["step"] in PEER_STEPS:
                        u = socket.socket(family, socket.SOCK_DGRAM)
                        u.bind((address, t["port"]))
                        sockets.append(u)
                        threading.Thread(target=serve_udp, args=(u, key, counts), daemon=True).start()
                except OSError as e:
                    failures.append(f"{key} port {t['port']} at {address}: {e}")
                    errnos.append(e.errno)
            continue
        try:
            s = bound_socket(socket.SOCK_STREAM, (t.get("bind", ip), t["port"]))
            s.listen(64)
            sockets.append(s)
            threading.Thread(target=serve_tcp, args=(s, key, counts), daemon=True).start()
            if t["step"] in PEER_STEPS:
                u = bound_socket(socket.SOCK_DGRAM, (t.get("bind", ip), t["port"]))
                sockets.append(u)
                threading.Thread(target=serve_udp, args=(u, key, counts), daemon=True).start()
        except OSError as e:
            failures.append(f"{key} port {t['port']}: {e}")
            errnos.append(e.errno)
    try:
        d = bound_socket(socket.SOCK_DGRAM, ("127.0.0.1", planned["dns_port"]))
        sockets.append(d)
        threading.Thread(target=serve_dns, args=(d, ip, planned["targets"], counts), daemon=True).start()
    except OSError as e:
        failures.append(f"resolver port {planned['dns_port']}: {e}")
        errnos.append(e.errno)
    told = failures + [f"asked {ip}"]
    if failures:
        told += told_lines(ip, errnos)
    with open(os.path.join(directory, f"ready-{label}"), "w", encoding="utf-8") as f:
        f.write("\n".join(told) + "\n")
    stop = os.path.join(directory, f"stop-{label}")
    deadline = time.time() + 1800
    while not os.path.exists(stop) and time.time() < deadline:
        time.sleep(0.2)
    time.sleep(1)
    with counts.lock:
        result = {"counts": counts.data, "seen": counts.seen, "listen_failures": failures}
    with open(os.path.join(directory, f"counts-{label}.json"), "w", encoding="utf-8") as f:
        json.dump(result, f, indent=1)
    for s in sockets:
        s.close()


ZERO_COPIES = ("head", "no-script", "engine-alone")
ORDER = ("head", "no-script", "engine-alone", "no-layer", "guard-off", "late", "no-switch", "no-proxy",
         "no-resolver-map", "no-bypass", "bypass", "hook-alone", "allowed-hosts", "no-allowed-hosts",
         "phint-no-layer", "phint-head", "phint-engine-alone",
         "hint-no-layer", "hint-head", "hint-no-script", "hint-engine-alone",
         "prefetch-no-layer", "prefetch-head", "prefetch-no-script", "prefetch-engine-alone",
         "chrome-switch", "rules-only", "proxy-only", "delegate-only", "features-only", "connect-proxy", "csp-sandbox")
NO_ROUTE_LINES = {
    "windows": "wry 0.57.1 src/webview2/mod.rs lines 861 to 863 and tauri-runtime-wry 2.12.3 src/lib.rs line 4829, with no new-window handler under src-tauri/src",
    "macos": "wry 0.57.1 src/wkwebview/class/wry_web_view_ui_delegate.rs lines 288 to 290 and tauri-runtime-wry 2.12.3 src/lib.rs line 4829, with no new-window handler under src-tauri/src",
}


HINT_PREFIXES = ("hint-", "prefetch-", "phint-")


def control_of(copy):
    for prefix in HINT_PREFIXES:
        if copy.startswith(prefix):
            return prefix + "no-layer"
    return "no-layer"


def is_zero_copy(copy):
    base = copy.split("-", 1)[1] if copy.startswith(HINT_PREFIXES) else copy
    return base in ZERO_COPIES


def no_route(key, job):
    step, form = key.split("-", 1)
    return step == "s05" or (job == "windows" and step == "s04" and form in ("ip", "host", "dns"))


def no_count(key):
    return key in ("s07-ip", "s07-host")
A2_PORTS = (80, 443, 21)


def answer_a2(conn, port, counts):
    conn.settimeout(3)
    try:
        data = conn.recv(4096)
        name = ""
        for line in data.split(b"\r\n"):
            if line.lower().startswith(b"host:"):
                name = line[5:].strip().decode("ascii", "replace")
        if not name and data[:1] == b"\x16":
            name = "tls " + "".join(chr(b) if 32 < b < 127 else "." for b in data[:300])[-80:]
        counts.add(f"a2-port{port}", "requests", f"{data[:60]!r} {name}")
        if port != 443 and data:
            conn.sendall(b"HTTP/1.1 200 OK\r\ncontent-type: text/html\r\ncontent-length: 2\r\n\r\nok")
    except OSError:
        pass
    finally:
        conn.close()


def serve_a2(sock, port, counts):
    while True:
        try:
            conn, _ = sock.accept()
        except OSError:
            return
        counts.add(f"a2-port{port}", "connections")
        threading.Thread(target=answer_a2, args=(conn, port, counts), daemon=True).start()


def listen_a2(directory, label):
    counts = Counts()
    sockets = []
    failures = []
    for port in A2_PORTS:
        try:
            s = bound_socket(socket.SOCK_STREAM, ("0.0.0.0", port))
            s.listen(64)
            sockets.append(s)
            threading.Thread(target=serve_a2, args=(s, port, counts), daemon=True).start()
        except OSError as e:
            failures.append(f"a2-port{port} port {port}: {e}")
    with open(os.path.join(directory, f"ready-{label}"), "w", encoding="utf-8") as f:
        f.write("\n".join(failures) + "\n")
    stop = os.path.join(directory, f"stop-{label}")
    deadline = time.time() + 1800
    while not os.path.exists(stop) and time.time() < deadline:
        time.sleep(0.2)
    time.sleep(1)
    with counts.lock:
        result = {"counts": counts.data, "seen": counts.seen, "listen_failures": failures}
    with open(os.path.join(directory, f"counts-{label}.json"), "w", encoding="utf-8") as f:
        json.dump(result, f, indent=1)
    for s in sockets:
        s.close()


def origins_of(path):
    origins = {}
    notes = {}
    if os.path.exists(path):
        with open(path, encoding="utf-8", errors="replace") as f:
            for line in f:
                parts = line.split()
                if len(parts) >= 3 and parts[0] == "halvo-measure":
                    origins[parts[1]] = parts[2].split("=", 1)[1]
                if len(parts) >= 3 and parts[0] == "halvo-measure-note":
                    notes[parts[1]] = " ".join(parts[2:])[:200]
    return origins, notes


def judge_a2(directory):
    verdict = 0
    for label in ("no-layer-a2",):
        path = os.path.join(directory, f"counts-{label}.json")
        if not os.path.exists(path):
            print(f"measure a2 {label}: not run")
            verdict = 1
            continue
        with open(path, encoding="utf-8") as f:
            data = json.load(f)
        origins, _ = origins_of(os.path.join(directory, f"{label}.err"))
        seen = data["seen"]
        first = sum(1 for line in seen if A2_CONTROL_HOST in line and "/first" in line)
        last = sum(1 for line in seen if A2_CONTROL_HOST in line and "/last" in line)
        others = [line for line in seen if A2_CONTROL_HOST not in line]
        forms = {k: v for k, v in origins.items() if k not in ("a2-00", "a2-71")}
        away = sorted(k for k, v in forms.items() if v != "true")
        print(f"measure a2 {label}: lines read {len(origins)} of 72, control first {first}, control last {last}, refused forms away {away or 'none'}, refused forms at the listeners {len(others)}, listener failures {data['listen_failures'] or 'none'}")
        for line in seen:
            print(f"measure a2 seen {label}: {line}")
        if label == "no-layer-a2":
            if len(origins) == 72 and first > 0 and last > 0 and not data["listen_failures"]:
                print(f"measure a2 no layer: {len(away)} of 70 refused forms left the app, {len(others)} reached the listeners, beside both controls above 0")
            else:
                print("measure a2 no layer: not measured (a control read 0, a line was not read or a listener did not bind)")
                verdict = 1
        elif away or others:
            print(f"measure a2 head: a refused form left or reached a listener: {away} {others[:3]}")
    return verdict


def pf_counts(directory, label, planned):
    path = os.path.join(directory, f"pflog-{label}.txt")
    fwpath = os.path.join(directory, f"fwlog-{label}.txt")
    if not os.path.exists(path) and os.path.exists(fwpath):
        path = fwpath
    if not os.path.exists(path):
        return None
    by_port = {}
    for t in planned["targets"]:
        if t.get("pf"):
            by_port[t["port"]] = f"{t['step']}-{t['form']}"
    attempts = {}
    probes = {}
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            m = re.search(r" (\S+)\.(\d+) > (198\.51\.100\.7|2001:db8::7)\.(\d+): (Flags \[S\]|UDP)", line)
            w = re.search(r"DROP (TCP|UDP) (\S+) (198\.51\.100\.7|2001:db8::7) (\d+) (\d+)", line)
            if m:
                source, source_port, port = m.group(1), m.group(2), int(m.group(4))
            elif w:
                source, source_port, port = w.group(2), w.group(4), int(w.group(5))
            else:
                continue
            target = (m or w).group(3)
            for moment, probe_port in PROBE_PORTS.items():
                if port == probe_port:
                    probes.setdefault("v4" if target == OFF4 else "v6", set()).add(moment)
            key = by_port.get(port)
            if key:
                attempts.setdefault(key, set()).add((source, source_port))
    return {k: len(v) for k, v in attempts.items()}, {family: sorted(m) for family, m in probes.items()}


def probe(directory):
    with open(os.path.join(directory, "plan.json"), encoding="utf-8") as f:
        planned = json.load(f)
    by_key = {f"{t['step']}-{t['form']}": t for t in planned["targets"]}
    for key in ("s01-ip", "s01-host", "s01-dns", "s16-ip"):
        t = by_key[key]
        try:
            if key == "s16-ip":
                u = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
                u.sendto(b"probe", (t["host"], t["port"]))
                u.close()
                print(f"measure probe {key}: sent one datagram")
                continue
            address = socket.getaddrinfo(t["host"], t["port"], socket.AF_INET, socket.SOCK_STREAM)[0][4]
            c = socket.create_connection(address, timeout=3)
            c.sendall(b"GET /probe HTTP/1.1\r\nhost: probe\r\n\r\n")
            c.recv(100)
            c.close()
            print(f"measure probe {key}: connected to {address[0] == planned['ip']} the runner address")
        except OSError as e:
            print(f"measure probe {key}: {e}")


def judge_head(directory, label):
    with open(os.path.join(directory, f"counts-{label}.json"), encoding="utf-8") as f:
        data = json.load(f)
    above = {k: v for k, v in data["counts"].items() if k != "other-dns" and sum(v.values()) > 0}
    failures = data["listen_failures"]
    origins = {}
    done = False
    with open(os.path.join(directory, f"{label}.err"), encoding="utf-8", errors="replace") as f:
        for line in f:
            parts = line.split()
            if len(parts) >= 3 and parts[0] == "halvo-measure":
                origins[parts[1]] = parts[2].split("=", 1)[1]
            if line.startswith("halvo-measure done"):
                done = True
    away = sorted(k for k, v in origins.items() if v != "true")
    ok = done and not above and not away and not failures and len(origins) > 0
    print(f"measure loop {label}: steps read {len(origins)}, done {done}, above 0 {sorted(above) or 'none'}, window away {away or 'none'}, listener failures {failures or 'none'}: {'ok' if ok else 'FAIL'}")
    return 0 if ok else 1


def judge(directory, job):
    with open(os.path.join(directory, "plan.json"), encoding="utf-8") as f:
        planned = json.load(f)
    keys = [f"{t['step']}-{t['form']}" for t in planned["targets"]]
    pf_keys = {f"{t['step']}-{t['form']}" for t in planned["targets"] if t.get("pf")}
    pf_status = open(os.path.join(directory, "pf-status.txt"), encoding="utf-8").read().strip() if os.path.exists(os.path.join(directory, "pf-status.txt")) else "not set up"
    print(f"measure packet filter: {pf_status}")
    copies = [c for c in ORDER if os.path.exists(os.path.join(directory, f"counts-{c}-steps.json"))]
    print(f"measure copies read: {' '.join(copies)}")
    zero = {"connections": 0, "requests": 0, "lookups": 0, "packets": 0, "marked": 0}
    results = {}
    unbound = {}
    filtered = set()
    probed = {}
    planned_keys = {}
    for copy in copies:
        merged = {}
        origins = {}
        notes = {}
        unbound[copy] = set()
        planned_keys[copy] = set()
        for launch in ("steps", "init", "fw", "lo"):
            label = f"{copy}-{launch}"
            path = os.path.join(directory, f"counts-{label}.json")
            keys_path = os.path.join(directory, f"{label}.keys")
            if os.path.exists(keys_path):
                with open(keys_path, encoding="utf-8") as f:
                    planned_keys[copy] |= {line.strip() for line in f if line.strip()}
            elif os.path.exists(path) and launch == "steps":
                planned_keys[copy] |= {k for k in keys if not k.split("-", 1)[1] in LOOP_FORMS}
            elif os.path.exists(path) and launch == "init":
                planned_keys[copy].add("s17-ip")
            if os.path.exists(path):
                with open(path, encoding="utf-8") as f:
                    data = json.load(f)
                for key, entry in data["counts"].items():
                    merged[key] = {**zero, **entry}
                unbound[copy] |= failed_keys(data["listen_failures"], keys)
                print(f"measure listeners {label}: failures {data['listen_failures'] or 'none'}")
                for line in data["seen"]:
                    print(f"measure seen {label}: {line}")
            pf = pf_counts(directory, label, planned) if pf_status.startswith("set up") else None
            if pf is not None:
                pf, probes = pf
                whole = sorted(family for family, moments in probes.items() if "before" in moments and "after" in moments)
                print(f"measure packet filter launch {label}: probes seen {probes or 'none'}, families with both probes {whole or 'none'}")
                probed[label] = set(whole)
                filtered.add(copy)
                for key, n in pf.items():
                    merged[key] = {**zero, "connections": n}
            o, n = origins_of(os.path.join(directory, f"{label}.err"))
            origins.update(o)
            notes.update(n)
        results[copy] = (merged, origins, notes)
    unexpected = []
    print("measure table: step form copy connections requests lookups packets marked window verdict")
    for key in keys:
        for copy in copies:
            if copy.startswith(HINT_PREFIXES) and key.split("-", 1)[0] not in HINT_STEPS:
                continue
            if key not in planned_keys[copy] and key not in pf_keys:
                continue
            control = results.get(control_of(copy), ({}, {}, {}))[0].get(key, zero)
            control_sum = sum(control.values())
            entry = {**zero, **results[copy][0].get(key, {})}
            window = results[copy][1].get(key, "not-read")
            total = sum(entry.values())
            verdict = ""
            if no_count(key):
                verdict = "no count can exist for this form; it is no 0"
            elif key in pf_keys and (f"{'v4' if key.endswith('off4') else 'v6'}=yes" not in pf_status or copy not in filtered or control_of(copy) not in filtered
                                     or ('v4' if key.endswith('off4') else 'v6') not in probed.get(f"{copy}-steps", set())
                                     or ('v4' if key.endswith('off4') else 'v6') not in probed.get(f"{control_of(copy)}-steps", set())):
                verdict = "not measurable (packet filter: a probe of the launch or of its control's launch was not seen)"
            elif no_route(key, job) and copy == control_of(copy):
                verdict = "UNEXPECTED control above 0 for a cell with no route found" if total > 0 else "control 0, as expected for a cell with no route found"
            elif no_route(key, job) and is_zero_copy(copy):
                verdict = ("UNEXPECTED control above 0 for a cell with no route found" if control_sum > 0
                           else f"no route found: with no layer the step reached nothing, and {NO_ROUTE_LINES[job]} shows why; not measured as closed")
            elif key in unbound[copy] or (is_zero_copy(copy) and key in unbound.get(control_of(copy), set())):
                verdict = "not measured (listener)"
                if is_zero_copy(copy):
                    verdict = "UNEXPECTED " + verdict
            elif is_zero_copy(copy):
                if total > 0:
                    verdict = "UNEXPECTED above 0"
                elif window != "true":
                    verdict = f"UNEXPECTED window {window}"
                elif control_sum == 0:
                    verdict = "not measured (control 0)"
                else:
                    verdict = "ok 0 beside its control"
            elif copy == control_of(copy):
                verdict = "control above 0" if total > 0 else "control 0"
            print(f"measure row: {key.replace('-', ' ')} {copy} {entry['connections']} {entry['requests']} {entry['lookups']} {entry['packets']} {entry['marked']} {window} {verdict}")
            note = results[copy][2].get(key)
            if note:
                print(f"measure note: {key.replace('-', ' ')} {copy} {note}")
            if verdict.startswith("UNEXPECTED"):
                unexpected.append(f"{key} {copy}: {verdict}")
    if "guard-off" in results:
        refused = {form: results["guard-off"][1].get(f"s02-{form}") for form, _, _ in REFUSED_FORMS}
        reached = {form: sum(results.get("no-layer", ({}, {}, {}))[0].get(f"s02-{form}", zero).values()) for form, _, _ in REFUSED_FORMS}
        print(f"measure guard-off s02 window at the app origin: {refused}; no-layer s02 at its listener: {reached}")
        if job == "windows" and all(value == "true" for value in refused.values()):
            unexpected.append(f"guard-off s02: no refused form left the app (window {refused})")
    if "late" in results:
        late = sum(results["late"][0].get("s17-ip", zero).values())
        print(f"measure late s17 at its listeners: {late}")
        if late == 0:
            unexpected.append("late s17: the first-script navigation did not reach its listeners")
    for name in sorted(os.listdir(directory)):
        if name.startswith("http80-") and name.endswith(".txt"):
            with open(os.path.join(directory, name), encoding="utf-8", errors="replace") as f:
                rows = [line.strip() for line in f if line.strip()]
            print(f"measure http80 {name[7:-4]}: requests at the system's HTTP service {len(rows)}, connections not counted; {rows[:3]}")
    other = results.get("head", ({}, {}, {}))[0].get("other-dns", zero)["lookups"]
    print(f"measure head lookups of other names at the resolver: {other}")
    for line in unexpected:
        print(f"measure UNEXPECTED {line}")
    print(f"measure verdict {job}: {'FAIL' if unexpected else 'PASS'} ({len(unexpected)} unexpected)")
    return 1 if unexpected else 0


if __name__ == "__main__":
    command = sys.argv[1]
    if command == "plan":
        plan(sys.argv[2])
    elif command == "listen":
        listen(sys.argv[2], sys.argv[3])
    elif command == "offprobe":
        moment = sys.argv[2] if len(sys.argv) > 2 else "setup"
        for family, host in ((socket.AF_INET, OFF4), (socket.AF_INET6, OFF6)):
            s = socket.socket(family, socket.SOCK_STREAM)
            s.settimeout(1)
            try:
                s.connect((host, PROBE_PORTS[moment]))
            except OSError as e:
                print(f"measure packet filter probe {moment} {host}: {type(e).__name__} {e}")
            s.close()
    elif command == "probe":
        probe(sys.argv[2])
    elif command == "listen-a2":
        listen_a2(sys.argv[2], sys.argv[3])
    elif command == "judge-a2":
        sys.exit(judge_a2(sys.argv[2]))
    elif command == "judge-head":
        sys.exit(judge_head(sys.argv[2], sys.argv[3]))
    elif command == "judge":
        sys.exit(judge(sys.argv[2], sys.argv[3]))
    else:
        sys.exit(64)
HELPEREOF
  cat > "$1/copies.py" <<'HELPEREOF'
"""Makes one copy of app-2 for the measurement of rulings AP-9 point 7 and AP-10, in the checkout of
a hosted job. Never shipped; the step restores the files with git checkout after each copy.

usage: python copies.py <copy>
Every copy gets the probe: the check mode runs the steps of HALVO_MEASURE_STEPS after its line and
prints each step's note (window.__halvoNote), and the window main gets the initialization script of
HALVO_MEASURE_INIT.
"""
import json
import re
import sys

COPIES = {
    "head": "every means and layer of the head",
    "no-script": "the head without the script that takes the peer constructors away",
    "engine-alone": "the engine means alone: content policy, the runtime's navigation rule and the script out",
    "no-layer": "every layer and means out, the configuration at creation too",
    "guard-off": "the engine means and the configuration at creation out, the content policy, the runtime's rule and the script in",
    "late": "page layers, start arguments and the configuration at creation out, the engine means installed 4 s after the load",
    "no-switch": "Windows: the head without the script layer and without --webrtc-ip-handling-policy",
    "no-proxy": "Windows: the engine means alone without --proxy-server",
    "no-resolver-map": "Windows: the engine means alone without --host-resolver-rules",
    "no-bypass": "Windows: the engine means alone without --proxy-bypass-list=<-loopback>",
    "hook-alone": "Windows: the engine means alone without --proxy-server and without --host-resolver-rules: the request hook and the switch",
    "phint-no-layer": "Windows: no layer, the profile's private mode on as shipped; hints put in at load",
    "phint-head": "Windows: the head, the profile's private mode on as shipped; hints put in at load",
    "phint-engine-alone": "Windows: the engine means alone, the profile's private mode on as shipped; hints put in at load",
    "hint-no-layer": "Windows: no layer, the profile's private mode off; hints put in at load",
    "hint-head": "Windows: the head, the profile's private mode off; hints put in at load",
    "hint-no-script": "Windows: the head without the script layer, the profile's private mode off; hints put in at load",
    "hint-engine-alone": "Windows: the engine means alone, the profile's private mode off; hints put in at load",
    "allowed-hosts": "macOS: the configuration at creation that allows no network host and keeps nothing, alone",
    "no-allowed-hosts": "macOS: the head without the configuration at creation and without the guard's reading of it",
    "prefetch-no-layer": "macOS: no layer, the engine's DNS prefetching on",
    "prefetch-head": "macOS: the head, the engine's DNS prefetching on",
    "prefetch-no-script": "macOS: the head without the script layer, the engine's DNS prefetching on",
    "prefetch-engine-alone": "macOS: the engine means alone, the engine's DNS prefetching on",
    "peer-proof": "macOS: the head with the peer-connection preference left on (the script in)",
}
copy = sys.argv[1]
assert copy in COPIES, copy


def sub(path, old, new):
    text = open(path, encoding="utf-8").read()
    assert text.count(old) == 1, (path, old, text.count(old))
    open(path, "w", encoding="utf-8", newline="\n").write(text.replace(old, new))


PROBE = '''if let Ok(path) = std::env::var("HALVO_MEASURE_STEPS") {
                let wait = std::env::var("HALVO_MEASURE_WAIT_MS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(1500u64);
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                for row in text.lines() {
                    let Some((id, script)) = row.split_once('\\t') else {
                        continue;
                    };
                    let Some(window) = app.get_webview_window("main") else {
                        eprintln!("halvo-measure {id} at-app-origin=no-window");
                        continue;
                    };
                    let _ = window.eval(script);
                    std::thread::sleep(Duration::from_millis(wait));
                    let at = match window.url() {
                        Ok(url) if crate::app::is_app_url(&url) => "true",
                        Ok(_) => "false",
                        Err(_) => "unreadable",
                    };
                    eprintln!("halvo-measure {id} at-app-origin={at}");
                    let (noted, note) = mpsc::channel();
                    if window
                        .eval_with_callback("String(window.__halvoNote || '')", move |seen| {
                            let _ = noted.send(seen);
                        })
                        .is_ok()
                    {
                        if let Ok(seen) = note.recv_timeout(Duration::from_secs(2)) {
                            if seen.len() > 2 {
                                eprintln!("halvo-measure-note {id} {seen}");
                            }
                        }
                    }
                    let _ = window.eval("window.__halvoNote = ''");
                    if at != "true" {
                        let before = app.try_state::<PageLoads>().map(|l| l.count()).unwrap_or(0);
                        let _ = window.navigate(crate::app::app_url());
                        let deadline = Instant::now() + Duration::from_secs(15);
                        while Instant::now() < deadline
                            && app.try_state::<PageLoads>().map(|l| l.count()).unwrap_or(0) <= before
                        {
                            std::thread::sleep(Duration::from_millis(100));
                        }
                        std::thread::sleep(Duration::from_millis(300));
                    }
                }
                eprintln!("halvo-measure done");
            }
            app.exit(Exit::PageLoaded.code());'''

sub("src-tauri/src/check_page.rs", "            app.exit(Exit::PageLoaded.code());", PROBE)
sub(
    "src-tauri/src/check_page.rs",
    "    std::thread::spawn(move || watch(handle, started));\n",
    "    if std::env::var_os(\"HALVO_MEASURE_INIT\").is_some() {\n"
    "        let reader = handle.clone();\n"
    "        std::thread::spawn(move || {\n"
    "            std::thread::sleep(Duration::from_secs(15));\n"
    "            let at = match reader.get_webview_window(\"main\").map(|w| w.url()) {\n"
    "                Some(Ok(url)) if crate::app::is_app_url(&url) => \"true\",\n"
    "                Some(Ok(_)) => \"false\",\n"
    "                Some(Err(_)) => \"unreadable\",\n"
    "                None => \"no-window\",\n"
    "            };\n"
    "            eprintln!(\"halvo-measure s17-ip at-app-origin={at}\");\n"
    "            reader.exit(Exit::PageLoaded.code());\n"
    "        });\n"
    "    }\n"
    "    std::thread::spawn(move || watch(handle, started));\n",
)
sub(
    "src-tauri/src/app.rs",
    "        .on_download(|_, _| false)\n",
    "        .on_download(|_, _| false)\n"
    "        .initialization_script(std::env::var(\"HALVO_MEASURE_INIT\").unwrap_or_default() + &std::env::var(\"HALVO_MEASURE_HINTS\").unwrap_or_default())\n",
)

ENGINE_OFF = '''pub fn guard<R: Runtime>(window: &WebviewWindow<R>, site: EngineSite, answer: Sender<GuardAnswer>) {
    if std::env::var_os("HALVO_MEASURE_NEVER_SET").is_none() {
        let _ = answer.send(Ok(GuardInstalled(())));
        return;
    }
'''

LATE = '''pub fn guard<R: Runtime>(window: &WebviewWindow<R>, site: EngineSite, answer: Sender<GuardAnswer>) {
    let _ = answer.send(Ok(GuardInstalled(())));
    let later = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(4));
        let window = later.clone();
        let _ = later.run_on_main_thread(move || {
            let (unread, _) = std::sync::mpsc::channel();
            guard_now(&window, site, unread);
        });
    });
}

'''

CONFIG = (
    "    #[cfg(target_os = \"macos\")]\n"
    "    let builder = match crate::engine::configuration_at_creation() {\n"
    "        Some(configuration) => builder.with_webview_configuration(configuration),\n"
    "        None => builder,\n"
    "    };\n"
)
READING = "            .and_then(|()| read_the_store_and_the_hosts(&view))\n"

PREFETCH = '''    let window = builder.build()?;
    #[cfg(target_os = "macos")]
    let _ = window.with_webview(|webview| {
        use objc2::runtime::AnyObject;
        use objc2::{msg_send, sel};
        let view = webview.inner() as *mut AnyObject;
        let config: *mut AnyObject = unsafe { msg_send![view, configuration] };
        let prefs: *mut AnyObject = unsafe { msg_send![config, preferences] };
        let mut how = String::from("no DNS-prefetch feature and no DNS-prefetch property");
        let features: *mut AnyObject = unsafe { msg_send![objc2::class!(WKPreferences), _features] };
        let count: usize = unsafe { msg_send![features, count] };
        for i in 0..count {
            let feature: *mut AnyObject = unsafe { msg_send![features, objectAtIndex: i] };
            let key: *mut AnyObject = unsafe { msg_send![feature, key] };
            let utf8: *const std::ffi::c_char = unsafe { msg_send![key, UTF8String] };
            if unsafe { std::ffi::CStr::from_ptr(utf8) }.to_bytes() == b"DNSPrefetchingEnabled" {
                let _: () = unsafe { msg_send![prefs, _setEnabled: true, forFeature: feature] };
                let on: bool = unsafe { msg_send![prefs, _isEnabledForFeature: feature] };
                how = format!("feature DNSPrefetchingEnabled reads {on}");
            }
        }
        let has: bool = unsafe { msg_send![prefs, respondsToSelector: sel!(_setDNSPrefetchingEnabled:)] };
        if has {
            let _: () = unsafe { msg_send![prefs, _setDNSPrefetchingEnabled: true] };
            let on: bool = unsafe { msg_send![prefs, _dnsPrefetchingEnabled] };
            how = format!("{how}; property _dnsPrefetchingEnabled reads {on}");
        }
        eprintln!("halvo-measure-config dns prefetch: {how}");
    });
    Ok(window)
'''


def engine_text():
    return open("src-tauri/src/engine.rs", encoding="utf-8").read()


def write_engine(text):
    open("src-tauri/src/engine.rs", "w", encoding="utf-8", newline="\n").write(text)


def guard_head(text):
    head = re.search(r"pub fn guard<R: Runtime>\([^{]*\{\n", text)
    assert head, "the guard's head"
    return head


def args_out():
    sub("src-tauri/src/app.rs", "    let builder = builder.additional_browser_args(ENGINE_ARGS);\n", "    let builder = builder;\n")


def config_out():
    sub("src-tauri/src/app.rs", CONFIG, "")


def engine_out(keep_config=False):
    text = engine_text()
    head = guard_head(text)
    write_engine(text[: head.start()] + ENGINE_OFF + text[head.end():])
    args_out()
    if not keep_config:
        config_out()


def script_out():
    sub("src-tauri/src/app.rs", "        .initialization_script_for_all_frames(NO_PEER_SCRIPT)\n", "")


def set_conf(key, value):
    path = "src-tauri/tauri.conf.json"
    conf = json.load(open(path, encoding="utf-8"))
    if key == "csp":
        conf["app"]["security"]["csp"] = value
    else:
        window = [w for w in conf["app"]["windows"] if w["label"] == "main"][0]
        window[key] = value
    open(path, "w", encoding="utf-8", newline="\n").write(json.dumps(conf, indent=2) + "\n")


def page_layers_out():
    set_conf("csp", None)
    sub("src-tauri/src/app.rs", "        .on_navigation(|url| may_navigate(url.as_str()))\n", "")
    script_out()


def arg_out(word):
    text = open("src-tauri/src/app.rs", encoding="utf-8").read()
    line = [l for l in text.split("\n") if l.startswith("pub const ENGINE_ARGS: &str = ")]
    assert len(line) == 1
    assert line[0].count(word) == 1, word
    sub("src-tauri/src/app.rs", line[0], line[0].replace(word, ""))


SWITCH = " --webrtc-ip-handling-policy=disable_non_proxied_udp"
RESOLVER = " --host-resolver-rules=\\\"MAP * ~NOTFOUND\\\""
PROXY = " --proxy-server=http://255.255.255.255:9"
BYPASS = " --proxy-bypass-list=<-loopback>"

base = copy.split("-", 1)[1] if copy.startswith(("hint-", "prefetch-", "phint-")) else copy
if copy.startswith("hint-"):
    set_conf("incognito", False)
if copy.startswith("prefetch-"):
    sub("src-tauri/src/app.rs", "    builder.build()\n}\n", PREFETCH + "}\n")

if base == "no-script":
    script_out()
elif base == "guard-off":
    engine_out()
elif base == "engine-alone":
    page_layers_out()
elif base == "no-layer":
    engine_out()
    page_layers_out()
elif base == "late":
    page_layers_out()
    args_out()
    config_out()
    text = engine_text()
    head = re.search(r"pub fn guard<R: Runtime>\(", text)
    assert head, "the guard's head"
    write_engine(text[: head.start()] + LATE + "pub fn guard_now<R: Runtime>(" + text[head.end():])
elif base == "no-switch":
    script_out()
    arg_out(SWITCH)
elif base == "no-proxy":
    page_layers_out()
    arg_out(PROXY)
elif base == "no-resolver-map":
    page_layers_out()
    arg_out(RESOLVER)
elif base == "no-bypass":
    page_layers_out()
    arg_out(BYPASS)
elif base == "hook-alone":
    page_layers_out()
    arg_out(PROXY)
    arg_out(RESOLVER)
elif base == "allowed-hosts":
    engine_out(keep_config=True)
    page_layers_out()
elif base == "no-allowed-hosts":
    config_out()
    sub("src-tauri/src/engine.rs", READING, "")
elif base == "peer-proof":
    sub("src-tauri/src/engine.rs", "let _: () = unsafe { msg_send![&*prefs, _setPeerConnectionEnabled: false] };", "let _: () = unsafe { msg_send![&*prefs, _setPeerConnectionEnabled: true] };")
    sub("src-tauri/src/engine.rs", "        if peer_on {\n", "        if !peer_on {\n")
else:
    assert base == "head", base
print(f"copy {copy} made: {COPIES[copy]}")
HELPEREOF
}

measure() {
  hosted_only
  job=${CI_D_JOB:-$1}
  started=$(date +%s)
  repo=${CI_D_REPO:-}
  [ -n "$repo" ] || repo=$(git rev-parse --show-toplevel) || exit 2
  cd "$repo" || exit 2
  py=python3
  command -v python3 >/dev/null 2>&1 || py=python
  native() { if command -v cygpath >/dev/null 2>&1; then cygpath -w "$1"; else printf '%s\n' "$1"; fi; }
  base=${RUNNER_TEMP:-${TMPDIR:-/tmp}}
  if command -v cygpath >/dev/null 2>&1; then base=$(cygpath -u "$base"); fi
  work=$(mktemp -d "$base/$name.XXXXXX") || exit 2
  take_traps
  echo "$name image: ${ImageOS:-unknown} ${ImageVersion:-unknown}"
  if [ "$job" = macos ]; then
    system=$(sw_vers -productVersion)
    build=$(sw_vers -buildVersion)
    webkit=$(defaults read /System/Library/Frameworks/WebKit.framework/Versions/A/Resources/Info CFBundleVersion 2>&1)
    echo "$name system: macOS $system build $build"
    echo "$name engine: WebKit ${webkit%%
*}"
  else
    powershell -NoProfile -Command "\$os = Get-CimInstance Win32_OperatingSystem; '$name system: ' + \$os.Caption + ' ' + \$os.Version; '$name engine: WebView2 runtime ' + (Get-ItemProperty -Path 'HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}' -ErrorAction SilentlyContinue).pv" | tr -d '\r'
    firewall_take
  fi
  write_helpers "$work"
  keys_of "$job" | tr ' ' '-' > "$work/keys.txt"
  for copy in engine-alone no-layer; do
    tree="$work/tree-$copy"
    mkdir -p "$tree" || fail "the directory of the copy $copy cannot be made"
    git archive -o "$work/head-$copy.tar" HEAD || fail "git archive HEAD failed for the copy $copy"
    tar -x -f "$work/head-$copy.tar" -C "$tree" || fail "tar could not unpack the tree of the copy $copy"
    (cd "$tree" && "$py" "$work/copies.py" "$copy" > /dev/null) || fail "the copy maker failed for the copy $copy"
    copy_made "$copy" "$tree" || fail "the copy $copy does not hold the change that it names"
    if ! CARGO_TARGET_DIR="$work/target" cargo build -p halvo --locked --manifest-path "$tree/Cargo.toml" > "$work/build-$copy.txt" 2>&1; then
      echo "$name build $copy failed"
      tail -n 5 "$work/build-$copy.txt"
      continue
    fi
    "$py" "$work/measure.py" plan "$work" > /dev/null || fail "the plan could not read an address of the runner for the copy $copy"
    grep -F -f "$work/keys.txt" "$work/steps.tsv" | awk -F'\t' 'NR==FNR {k[$1]=1; next} ($1 in k)' "$work/keys.txt" - > "$work/routes.tsv"
    steps=$(cut -f1 "$work/routes.tsv" | paste -s -d ' ' -)
    echo "$name steps of $copy: $steps"
    exe="$work/target/debug/halvo"
    [ -f "$exe.exe" ] && exe=$exe.exe
    "$py" "$work/measure.py" listen "$work" "$copy-steps" &
    listener=$!
    own_pids="$own_pids $listener"
    tick=0
    while [ ! -f "$work/ready-$copy-steps" ] && [ "$tick" -lt 100 ]; do sleep 0.2; tick=$((tick + 1)); done
    late=yes
    if [ -f "$work/ready-$copy-steps" ]; then
      told "$copy-steps"
      late=no
    fi
    data=$(mktemp -d "$work/data-XXXXXX")
    routes=$(native "$work/routes.tsv")
    data_dir=$(native "$data")
    cd "$tree" || fail "the tree of the copy $copy cannot be entered"
    HALVO_MEASURE_STEPS="$routes" HALVO_MEASURE_WAIT_MS=1500 "$exe" --check-page --data-dir "$data_dir" --within-s 180 > "$work/$copy-steps.out" 2> "$work/$copy-steps.err" &
    launch=$!
    own_pids="$own_pids $launch"
    cd "$repo" || fail "the checkout cannot be entered again"
    wait "$launch"
    launched=$?
    forget_own "$launch"
    first=$(head -n 1 "$work/$copy-steps.out")
    echo "$name launch $copy: exit $launched, $first"
    sleep 2
    : > "$work/stop-$copy-steps"
    wait "$listener"
    forget_own "$listener"
    [ "$late" = no ] || told "$copy-steps"
  done
  "$py" "$work/measure.py" judge "$work" "$job" > "$work/judged.txt"
  [ -n "${CI_D_ARTIFACTS:-}" ] && cp "$work/judged.txt" "$CI_D_ARTIFACTS/$name-judged.txt"
  asked=$(keys_of "$job" | cut -d ' ' -f 1 | paste -s -d '|' -)
  grep -E '^measure (row|listeners)' "$work/judged.txt" | grep -E " ($asked) ip |listeners" | sed "s/^/$name /"
  verdict "$work/judged.txt" "$job" > "$work/verdict.txt"
  code=$?
  put_back
  echo "$name hosts file, resolver and packet filter: not changed"
  ended=$(date +%s)
  echo "$name measured in $(( ended - started )) s"
  cat "$work/verdict.txt"
  return "$code"
}

case $0 in
  /*) self=$0 ;;
  *) self=$PWD/$0 ;;
esac
case ${1:-} in
  --self-test) self_test ;;
  '') measure "${CI_D_JOB:-}" ;;
  macos | windows) measure "$1" ;;
  *) exit 64 ;;
esac
