#!/bin/sh
# jobs: macos windows
# rule: S6 measures wall time and peak RSS for 30 calls of each real account helper command on loopback and over TLS with a test authority that the hosted runner trusts for the length of the check, and requires every TLS call to succeed in the stub's final record within 1,000 ms.
# lane: alone
set -u
name=28-r7-helper-s6
case $#:${1:-} in
  0:) mode=measure ;;
  1:--self-test) mode=self-test ;;
  *) exit 64 ;;
esac
work=$(mktemp -d "${TMPDIR:?}/$name.XXXXXX") || exit 1
py=python3
if command -v cygpath >/dev/null 2>&1; then py=python; fi
cat >"$work/check.py" <<'PY' || exit 1
import ctypes
import hashlib
import http.client
import json
import os
from pathlib import Path
import plistlib
import re
import ssl
import statistics
import subprocess
import sys
import time
from contextlib import contextmanager
from urllib.parse import urlsplit


WORK = Path(__file__).resolve().parent
CA = WORK / "authority.pem"
ARMED = WORK / "trust-fingerprint"
REASONS = WORK / "failures"
NAME = "28-r7-helper-s6"
COUNT = 30
LIMIT_NS = 1_000_000_000
COMMANDS = {
    "subscribe": (["1", "week"], "POST", "/v1/checkout", "session"),
    "status": ([], "GET", "/v1/subscription", "active"),
    "set-cap": (["2", "week"], "PUT", "/v1/cap", "cap"),
    "cap": ([], "GET", "/v1/cap", "cap"),
    "manage": ([], "POST", "/v1/manage", "page"),
    "delete": ([], "DELETE", "/v1/account", "deleted"),
}


def hosted(env, platform):
    job = {"darwin": ("macos", "macOS"), "win32": ("windows", "Windows")}.get(platform)
    return (job is not None and env.get("GITHUB_ACTIONS") == "true"
            and env.get("RUNNER_ENVIRONMENT") == "github-hosted"
            and (env.get("CI_D_JOB"), env.get("RUNNER_OS")) == job)


def checked(command):
    action = next((str(arg).lstrip("-") for arg in command
                   if str(arg).lstrip("-") in {"find-certificate", "add-trusted-cert",
                                               "remove-trusted-cert", "delete-certificate",
                                               "addstore", "delstore"}), "read-machine-root")
    try:
        result = subprocess.run(command, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                                stderr=subprocess.DEVNULL, timeout=30)
    except subprocess.TimeoutExpired as exc:
        raise RuntimeError(f"certificate-tool-{action}-timeout-30s") from exc
    if result.returncode:
        raise RuntimeError(f"certificate-tool-{action}-exit-{result.returncode}")
    return result.stdout


def der_fingerprint(cert):
    return hashlib.sha256(cert).hexdigest().upper()


SYSTEM_KEYCHAIN = "/Library/Keychains/System.keychain"


def present(fingerprint):
    if sys.platform == "darwin":
        output = checked(["security", "find-certificate", "-a", "-p", SYSTEM_KEYCHAIN])
        pem = re.findall(rb"-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----", output, re.S)
        return any(der_fingerprint(ssl.PEM_cert_to_DER_cert(cert.decode("ascii"))) == fingerprint
                   for cert in pem)
    query = (r"$ErrorActionPreference = 'Stop'; Get-ChildItem -LiteralPath Cert:\LocalMachine\Root | "
             "ForEach-Object { $_.GetCertHashString([System.Security.Cryptography.HashAlgorithmName]::SHA256) }")
    output = checked(["pwsh", "-NoProfile", "-NonInteractive", "-Command", query])
    return fingerprint.encode("ascii") in output.splitlines()


def certutil(*args):
    exe = Path(os.environ["SystemRoot"]) / "System32" / "certutil.exe"
    return checked([str(exe), *args])


def admin_trust_remains(der):
    path = WORK / "admin-trust-after.plist"
    try:
        result = subprocess.run(["security", "trust-settings-export", "-d", str(path)],
                                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
    except subprocess.TimeoutExpired as exc:
        raise RuntimeError("admin-trust-readback-timeout-30s") from exc
    if result.returncode:
        if b"No Trust Settings were found" in result.stderr:
            return False
        raise RuntimeError("admin-trust-readback")
    trust = plistlib.loads(path.read_bytes())["trustList"]
    return hashlib.sha1(der).hexdigest().upper() in {key.upper() for key in trust}


def install():
    if not hosted(os.environ, sys.platform):
        raise RuntimeError("certificate-install-outside-hosted-job")
    der = ssl.PEM_cert_to_DER_cert(CA.read_text("ascii"))
    fingerprint = der_fingerprint(der)
    if present(fingerprint):
        raise RuntimeError("authority-already-present")
    ARMED.write_text(fingerprint, encoding="ascii")
    if sys.platform == "darwin":
        checked(["sudo", "-n", "security", "add-trusted-cert", "-d", "-r", "trustRoot",
                 "-p", "ssl", "-k", SYSTEM_KEYCHAIN, str(CA)])
    else:
        certutil("-addstore", "Root", str(CA))
    if not present(fingerprint):
        raise RuntimeError("authority-install-readback")
    print("S6 trust installed=true", flush=True)


def undo():
    if not ARMED.exists():
        return
    if not hosted(os.environ, sys.platform):
        raise RuntimeError("certificate-undo-outside-hosted-job")
    der = ssl.PEM_cert_to_DER_cert(CA.read_text("ascii"))
    fingerprint = ARMED.read_text("ascii")
    if fingerprint != der_fingerprint(der):
        raise RuntimeError("authority-identity")
    errors = []
    if sys.platform == "darwin":
        try:
            checked(["sudo", "-n", "security", "remove-trusted-cert", "-d", str(CA)])
        except RuntimeError as exc:
            errors.append(str(exc))
        if present(fingerprint):
            try:
                checked(["sudo", "-n", "security", "delete-certificate", "-Z", fingerprint,
                         SYSTEM_KEYCHAIN])
            except RuntimeError as exc:
                errors.append(str(exc))
    else:
        try:
            certutil("-delstore", "Root", hashlib.sha1(der).hexdigest())
        except RuntimeError as exc:
            errors.append(str(exc))
    remains = present(fingerprint)
    if sys.platform == "darwin":
        trust_setting = admin_trust_remains(der)
        print("S6 trust admin-setting-after-undo=" + str(trust_setting).lower(), flush=True)
        remains = remains or trust_setting
    print("S6 trust present-after-undo=" + str(remains).lower(), flush=True)
    if errors or remains:
        raise RuntimeError("-".join(["authority-undo", *errors]))
    print("S6 trust undone=true", flush=True)


STUB = """
import { startStub } from './account/stub/src/stub.mjs';
const [record, tlsCa] = process.argv.slice(1);
const stub = await startStub({record, tlsCa});
process.stdin.once('end', () => stub.close());
process.stdin.resume();
console.log(JSON.stringify({app:stub.app, control:stub.control}));
await stub.stopped;
"""


@contextmanager
def stub(record, tls):
    record.mkdir()
    args = ["node", "--input-type=module", "-e", STUB, str(record)]
    if tls:
        if not hosted(os.environ, sys.platform):
            raise RuntimeError("certificate-outside-hosted-job")
        args.append(str(CA))
    proc = subprocess.Popen(args, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                            stderr=subprocess.DEVNULL)
    try:
        ready = json.loads(proc.stdout.readline())
        for key, scheme in [("app", "https" if tls else "http"), ("control", "http")]:
            url = urlsplit(ready[key])
            if url.scheme != scheme or url.hostname != "127.0.0.1" or not url.port:
                raise RuntimeError("stub-address")
        yield ready
    finally:
        proc.stdin.close()
        if proc.wait() != 0:
            raise RuntimeError("stub-stop")


def control(address, path):
    url = urlsplit(address)
    conn = http.client.HTTPConnection(url.hostname, url.port, timeout=10)
    try:
        conn.request("POST", path, body=b"")
        response = conn.getresponse()
        return response.status == 200 and json.loads(response.read()) == {"changed": 1}
    finally:
        conn.close()


class MemoryCounters(ctypes.Structure):
    _fields_ = [("cb", ctypes.c_uint32), ("faults", ctypes.c_uint32),
                ("peak_working_set", ctypes.c_size_t), ("counters", ctypes.c_size_t * 7)]


def reap(proc):
    if sys.platform == "darwin":
        pid, status, usage = os.wait4(proc.pid, 0)
        proc.returncode = os.waitstatus_to_exitcode(status)
        return time.perf_counter_ns(), usage.ru_maxrss
    proc.wait()
    stopped = time.perf_counter_ns()
    read_memory = ctypes.WinDLL("kernel32", use_last_error=True).K32GetProcessMemoryInfo
    read_memory.argtypes = [ctypes.c_void_p, ctypes.POINTER(MemoryCounters), ctypes.c_uint32]
    read_memory.restype = ctypes.c_int
    counters = MemoryCounters()
    counters.cb = ctypes.sizeof(counters)
    if not read_memory(int(proc._handle), ctypes.byref(counters), counters.cb):
        raise RuntimeError("peak-working-set")
    return stopped, counters.peak_working_set


def one_call(helper, command, env):
    args = [str(helper), "--print-url", command, *COMMANDS[command][0]]
    start = time.perf_counter_ns()
    with subprocess.Popen(args, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                          stderr=subprocess.DEVNULL) as child:
        end, peak = reap(child)
        output = child.stdout.read().split()
    result = COMMANDS[command][3]
    ok = child.returncode == 0 and output[:1] == [("result=" + result).encode()]
    return end - start, peak, ok


def answered(rows, forced, command):
    if len(rows) != 1:
        return False
    row = rows[0]
    _, method, path, result = COMMANDS[command]
    connection = row.get("connection")
    return (type(connection) is int and connection > 0 and connection not in forced
            and row.get("method") == method and row.get("target") == path
            and row.get("status") == 200 and row.get("result") == result)


def summary(command, samples, record, forced, tls, report=True):
    wall = []
    peaks = []
    successes = 0
    mode = "TLS" if tls else "loopback"
    for number, (elapsed, peak, ok, first, last) in enumerate(samples, 1):
        rows = [json.loads(line) for line in record[first:last].splitlines()]
        success = ok and answered(rows, forced, command)
        successes += success
        wall.append(elapsed)
        peaks.append(peak)
        if report:
            print(f"S6 sample {mode} {command} call={number} wall_ns={elapsed} "
                  f"peak_rss_bytes={peak} success={str(success).lower()}")
    if report:
        print(f"S6 {mode} {command} calls={len(wall)} successes={successes} "
              f"median_ms={statistics.median(wall) / 1e6:.6f} worst_ms={max(wall) / 1e6:.6f} "
              f"peak_rss_bytes={max(peaks)}", flush=True)
    return (len(wall) == COUNT and min(wall) > 0 and min(peaks) > 0
            and (not tls or (successes == COUNT and max(wall) <= LIMIT_NS)))


def forced_connections(record):
    rows = (json.loads(line) for line in record.splitlines())
    return {row["connection"] for row in rows if row.get("error") == "forced-close"}


def measure(tls):
    mode = "tls" if tls else "loopback"
    folder = WORK / mode
    folder.mkdir()
    record = folder / "record"
    log = record / "requests.jsonl"
    helper = Path("target/account/debug/halvo-account" + (".exe" if sys.platform == "win32" else "")).resolve()
    if not helper.is_file():
        raise RuntimeError("missing-10-r7-account-build")
    samples = {name: [] for name in COMMANDS}
    with stub(record, tls) as ready:
        if tls:
            install()
        for round_id in range(COUNT):
            env = dict(os.environ, HALVO_ACCOUNT_BACKEND=ready["app"],
                       HALVO_ACCOUNT_STORE_DIR=str(folder / f"store-{round_id}"))
            for command in COMMANDS:
                before = log.stat().st_size
                elapsed, peak, ok = one_call(helper, command, env)
                after = log.stat().st_size
                samples[command].append((elapsed, peak, ok, before, after))
                if command == "subscribe" and not control(ready["control"], "/complete"):
                    print(f"S6 {mode} setup-complete=false round={round_id + 1}", flush=True)
    final = log.read_bytes()
    forced = forced_connections(final)
    return [mode + "/" + command for command in COMMANDS
            if not summary(command, samples[command], final, forced, tls)]


def self_test():
    import io
    from contextlib import redirect_stdout
    from unittest.mock import patch

    env = {"RUNNER_ENVIRONMENT": "github-hosted", "GITHUB_ACTIONS": "true",
           "RUNNER_OS": "macOS", "CI_D_JOB": "macos"}
    if not hosted(env, "darwin") or not hosted(dict(env, RUNNER_OS="Windows", CI_D_JOB="windows"), "win32"):
        raise RuntimeError("hosted-control")
    refused = 0
    for key, value in [("RUNNER_ENVIRONMENT", "self-hosted"), ("GITHUB_ACTIONS", "false"),
                       ("RUNNER_OS", "Linux"), ("CI_D_JOB", "windows")]:
        if hosted(dict(env, **{key: value}), "darwin"):
            raise RuntimeError("host-gate-" + key)
        refused += 1
    row = {"connection": 7, "method": "GET", "target": "/v1/subscription", "status": 200, "result": "active"}
    def check(rows, *, elapsed=LIMIT_NS, peak=100, ok=True, count=30, tls=True, late=None):
        data = b"".join(json.dumps(r).encode() + b"\n" for r in rows)
        final = data + (json.dumps(late).encode() + b"\n" if late else b"")
        samples = [(elapsed, peak, ok, 0, len(data))] * count
        return summary("status", samples, final, forced_connections(final), tls, False)

    if not (check([row]) and check([row], elapsed=LIMIT_NS + 1, tls=False)
            and check([], ok=False, tls=False)
            and check([row], late={"connection": 8, "error": "forced-close"})):
        raise RuntimeError("sample-control")
    bad = {"no-record": check([]), "two-records": check([row, row]),
           "wrong-method": check([dict(row, method="POST")]), "wrong-route": check([dict(row, target="/v1/cap")]),
           "wrong-status": check([dict(row, status=400)]), "wrong-result": check([dict(row, result="pending")]),
           "missing-connection": check([dict(row, connection=None)]), "zero-connection": check([dict(row, connection=0)]),
           "failed-helper": check([row], ok=False), "short-count": check([row], count=29),
           "long-count": check([row], count=31), "missing-rss": check([row], peak=0),
           "zero-time": check([row], elapsed=0), "over-limit": check([row], elapsed=LIMIT_NS + 1),
           "forced-after-status": check([row], late={"connection": 7, "error": "forced-close"})}
    for case, accepted in bad.items():
        if accepted:
            raise RuntimeError(case)
    refused += len(bad)
    for remains, settings, failed_tool, want_failure in [
        (False, False, False, False), (True, False, False, True),
        (False, True, False, True), (False, False, True, True)
    ]:
        identity = der_fingerprint(b"fixture")
        with patch("sys.platform", "darwin"), patch.dict(os.environ, env), \
             patch("__main__.ARMED") as armed, patch("__main__.CA"), \
             patch("ssl.PEM_cert_to_DER_cert", return_value=b"fixture"), \
             patch("__main__.checked") as tool, patch("__main__.present", side_effect=[True, remains]) as read, \
             patch("__main__.admin_trust_remains", return_value=settings) as trust, redirect_stdout(io.StringIO()):
            armed.read_text.return_value = identity
            tool.side_effect = [RuntimeError("fixture") if failed_tool else b"", b""]
            try:
                undo()
                failed = False
            except RuntimeError:
                failed = True
            if (failed != want_failure or read.call_count != 2 or trust.call_count != 1
                    or [call.args[0][3] for call in tool.call_args_list] != ["remove-trusted-cert", "delete-certificate"]):
                raise RuntimeError("macos-undo-fixture")
        refused += want_failure
    with patch("sys.platform", "win32"), patch.dict(os.environ, dict(env, CI_D_JOB="windows", RUNNER_OS="Windows")), \
         patch("__main__.ARMED") as armed, patch("__main__.CA"), \
         patch("ssl.PEM_cert_to_DER_cert", return_value=b"fixture"), patch("__main__.certutil") as tool, \
         patch("__main__.present", return_value=True) as read, redirect_stdout(io.StringIO()):
        armed.read_text.return_value = identity
        try:
            undo()
        except RuntimeError as exc:
            if str(exc) != "authority-undo":
                raise
        else:
            raise RuntimeError("windows-undo-fixture")
        tool.assert_called_once_with("-delstore", "Root", hashlib.sha1(b"fixture").hexdigest())
        read.assert_called_once_with(identity)
        refused += 1
    with patch.dict(os.environ, {"SystemRoot": "system"}), patch("__main__.checked") as native:
        for operation in ("-addstore", "-delstore"):
            certutil(operation, "Root", "fixture")
            if native.call_args.args[0][1:] != [operation, "Root", "fixture"]:
                raise RuntimeError("windows-machine-store-fixture")
    with patch("sys.platform", "win32"), patch("ssl.enum_certificates", create=True, return_value=[]), \
         patch("__main__.checked", return_value=(identity + "\r\n").encode()) as native:
        if not present(identity) or present(der_fingerprint(b"other")):
            raise RuntimeError("windows-readback-fixture")
        if r"Cert:\LocalMachine\Root" not in native.call_args.args[0][-1]:
            raise RuntimeError("windows-readback-store-fixture")
    for operation in ["-addstore", "-delstore", "add-trusted-cert", "remove-trusted-cert", "delete-certificate"]:
        command = ["tool", operation]
        with patch("subprocess.run", side_effect=subprocess.TimeoutExpired(command, 30)) as native:
            try:
                checked(command)
            except RuntimeError as exc:
                if str(exc) != "certificate-tool-" + operation.lstrip("-") + "-timeout-30s":
                    raise RuntimeError("store-timeout-fixture") from exc
            except subprocess.TimeoutExpired as exc:
                raise RuntimeError("store-timeout-fixture") from exc
            else:
                raise RuntimeError("store-timeout-fixture")
            if native.call_args.kwargs.get("timeout") != 30:
                raise RuntimeError("store-timeout-fixture")
            refused += 1
    print(f"PASS self-test:{NAME} ({refused} failing fixtures rejected)")


def finish(code):
    try:
        undo()
    except Exception as exc:
        print(f"S6 trust undone=false cause={reason(exc)}")
        code = 1
    reasons = REASONS.read_text().strip() if REASONS.exists() else ""
    if code or reasons:
        print("S6 real-helper: MISSED " + (reasons or "setup-or-cleanup"))
        print(f"FAIL {NAME}")
        return 1
    if hosted(os.environ, sys.platform):
        print("S6 real-helper: MET")
    else:
        print("S6 real-helper: loopback measured")
    print(f"PASS {NAME}")
    return 0


def reason(exc):
    return str(exc) if type(exc) is RuntimeError else type(exc).__name__


def run():
    mode = sys.argv[1]
    if mode == "finish":
        return finish(int(sys.argv[2]))
    if mode == "self-test":
        self_test()
        return 0
    failures = measure(False)
    if hosted(os.environ, sys.platform):
        failures.extend(measure(True))
    else:
        print("S6 TLS skipped: requires a GitHub-hosted runner")
    REASONS.write_text(" ".join(failures), encoding="ascii")
    return int(bool(failures))


try:
    sys.exit(run())
except Exception as error:
    cause = reason(error)
    print(f"S6 error={cause}", flush=True)
    REASONS.write_text(cause, encoding="ascii")
    sys.exit(1)
PY
if [ "$mode" = self-test ]; then
  exec "$py" -I "$work/check.py" self-test
fi
finish() {
  code=$?
  trap - EXIT HUP INT TERM
  "$py" -I "$work/check.py" finish "$code"
  exit $?
}
trap finish EXIT
trap 'exit 1' HUP INT TERM
"$py" -I "$work/check.py" measure
