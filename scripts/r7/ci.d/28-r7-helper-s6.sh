#!/bin/sh
# jobs: macos windows
# rule: S6 measures wall time and peak RSS for 30 calls of each real account helper command on loopback and over TLS with a test authority that the hosted runner trusts while the check runs; its key ends with the check, and a trust setting that the system keeps afterwards is named in the rule line. Before the authority is trusted, one control call over TLS must be refused for the certificate, as the stub's record of the client's TLS alert shows, or the check fails and nothing is trusted. S6 requires every TLS call to succeed in the stub's final record within 1,000 ms, confirms by a read of the process table that the TLS stub's process with its in-memory key has ended, and reports native trust left after undo.
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
import struct
import subprocess
import sys
import time
from contextlib import contextmanager
from urllib.parse import urlsplit


WORK = Path(__file__).resolve().parent
CA = WORK / "authority.pem"
ARMED = WORK / "trust-fingerprint"
KEY_EXIT = WORK / "tls-stub-exit"
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


def undo_command(action, call, *args):
    try:
        call(*args)
    except RuntimeError as exc:
        print(f"S6 trust {action} result={exc}", flush=True)
        return False
    print(f"S6 trust {action} result=ok", flush=True)
    return True


def undo():
    if not ARMED.exists():
        return True, False
    if not hosted(os.environ, sys.platform):
        raise RuntimeError("certificate-undo-outside-hosted-job")
    der = ssl.PEM_cert_to_DER_cert(CA.read_text("ascii"))
    fingerprint = ARMED.read_text("ascii")
    if fingerprint != der_fingerprint(der):
        raise RuntimeError("authority-identity")
    answers = []
    if sys.platform == "darwin":
        answers.append(undo_command("remove-trusted-cert", checked,
                                    ["sudo", "-n", "security", "remove-trusted-cert", "-d", str(CA)]))
        answers.append(undo_command("delete-certificate", checked,
                                    ["sudo", "-n", "security", "delete-certificate", "-Z", fingerprint,
                                     SYSTEM_KEYCHAIN]))
    else:
        answers.append(undo_command("delstore", certutil, "-delstore", "Root", hashlib.sha1(der).hexdigest()))
    remains = present(fingerprint)
    print("S6 trust certificate-after-undo=" + str(remains).lower(), flush=True)
    if sys.platform == "darwin":
        trust_setting = admin_trust_remains(der)
        print("S6 trust admin-setting-after-undo=" + str(trust_setting).lower(), flush=True)
        remains = remains or trust_setting
    print("S6 trust present-after-undo=" + str(remains).lower(), flush=True)
    print("S6 trust undone=" + str(not remains).lower(), flush=True)
    return all(answers), remains


CTL_KERN, KERN_PROC, KERN_PROC_PID = 1, 14, 1
KINFO_PID_AT, KINFO_STAT_AT, SZOMB = 40, 36, 5
QUERY_AND_WAIT, ERROR_INVALID_PARAMETER, WAIT_OBJECT_0, WAIT_TIMEOUT = 0x101000, 87, 0, 0x102


def process_start(pid):
    if sys.platform == "darwin":
        libc = ctypes.CDLL(None, use_errno=True)
        mib = (ctypes.c_int * 4)(CTL_KERN, KERN_PROC, KERN_PROC_PID, pid)
        info = ctypes.create_string_buffer(1024)
        size = ctypes.c_size_t(len(info))
        if libc.sysctl(mib, 4, info, ctypes.byref(size), None, 0) != 0:
            raise RuntimeError("stub-process-read")
        if size.value == 0:
            return None
        if size.value <= KINFO_PID_AT + 4 or struct.unpack_from("=i", info.raw, KINFO_PID_AT)[0] != pid:
            raise RuntimeError("stub-process-read")
        if info.raw[KINFO_STAT_AT] == SZOMB:
            return None
        seconds, micros = struct.unpack_from("=qi", info.raw, 0)
        return f"{seconds}.{micros:06d}"
    kernel = ctypes.WinDLL("kernel32", use_last_error=True)
    kernel.OpenProcess.argtypes = [ctypes.c_uint32, ctypes.c_int, ctypes.c_uint32]
    kernel.OpenProcess.restype = ctypes.c_void_p
    kernel.GetProcessTimes.argtypes = [ctypes.c_void_p] + [ctypes.POINTER(ctypes.c_uint64)] * 4
    kernel.GetProcessTimes.restype = ctypes.c_int
    kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_uint32]
    kernel.WaitForSingleObject.restype = ctypes.c_uint32
    kernel.CloseHandle.argtypes = [ctypes.c_void_p]
    handle = kernel.OpenProcess(QUERY_AND_WAIT, 0, pid)
    if not handle:
        if ctypes.get_last_error() == ERROR_INVALID_PARAMETER:
            return None
        raise RuntimeError("stub-process-read")
    try:
        times = [ctypes.c_uint64() for _ in range(4)]
        if not kernel.GetProcessTimes(handle, *(ctypes.byref(value) for value in times)):
            raise RuntimeError("stub-process-read")
        state = kernel.WaitForSingleObject(handle, 0)
        if state == WAIT_OBJECT_0:
            return None
        if state != WAIT_TIMEOUT:
            raise RuntimeError("stub-process-read")
        return str(times[0].value)
    finally:
        kernel.CloseHandle(handle)


def stub_record(pid, born, status):
    try:
        gone = born is not None and process_start(pid) != born
    except RuntimeError:
        gone = False
    return json.dumps({"exit": status, "gone": gone, "pid": pid, "start": born}, sort_keys=True)


def key_record():
    try:
        record = json.loads(KEY_EXIT.read_text("ascii")) if KEY_EXIT.is_file() else None
    except (OSError, ValueError):
        record = None
    if (not isinstance(record, dict) or type(record.get("exit")) is not int
            or type(record.get("gone")) is not bool):
        return "unknown", False
    return str(record["exit"]), record["gone"]


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
    born = None
    try:
        if tls:
            born = process_start(proc.pid)
        ready = json.loads(proc.stdout.readline())
        for key, scheme in [("app", "https" if tls else "http"), ("control", "http")]:
            url = urlsplit(ready[key])
            if url.scheme != scheme or url.hostname != "127.0.0.1" or not url.port:
                raise RuntimeError("stub-address")
        yield ready
    finally:
        try:
            proc.stdin.close()
        finally:
            status = proc.wait()
            if tls:
                KEY_EXIT.write_text(stub_record(proc.pid, born, status), encoding="ascii")
        if status != 0:
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


CONTROL_WAIT_S = 10
CERTIFICATE_ALERTS = ("ERR_SSL_TLSV1_ALERT_UNKNOWN_CA", "ERR_SSL_SSLV3_ALERT_BAD_CERTIFICATE",
                      "ERR_SSL_SSLV3_ALERT_CERTIFICATE_UNKNOWN", "ERR_SSL_SSLV3_ALERT_UNSUPPORTED_CERTIFICATE")


def control_window(log, before):
    deadline = time.monotonic() + CONTROL_WAIT_S
    while True:
        data = log.read_bytes()[before:]
        if data.endswith(b"\n") or time.monotonic() >= deadline:
            break
        time.sleep(0.05)
    rows = []
    for line in data.splitlines():
        try:
            row = json.loads(line)
        except ValueError:
            row = None
        rows.append(row if isinstance(row, dict) else {"error": "unreadable-line"})
    return rows


def control_cause(code, answer, rows):
    if code == 0 or any("status" in row for row in rows):
        return "answered"
    if code != 1:
        return f"helper-exit-{code}"
    if answer != [b"error=failed-call"]:
        return "helper-answer"
    if not rows:
        return "no-alert"
    if len(rows) != 1:
        return f"lines-{len(rows)}"
    if rows[0].get("error") not in CERTIFICATE_ALERTS:
        return "alert-other"
    return None


def tls_control(helper, ready, folder, log):
    env = dict(os.environ, HALVO_ACCOUNT_BACKEND=ready["app"],
               HALVO_ACCOUNT_STORE_DIR=str(folder / "store-control"))
    before = log.stat().st_size
    with subprocess.Popen([str(helper), "--print-url", "subscribe", *COMMANDS["subscribe"][0]], env=env,
                          stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL) as child:
        answer = child.stdout.read().split()[:1]
    rows = control_window(log, before)
    cause = control_cause(child.returncode, answer, rows)
    said = answer[0].decode("ascii", "replace") if answer else "none"
    alert = rows[0].get("error", "none") if len(rows) == 1 else "none"
    print(f"S6 TLS control command=subscribe exit={child.returncode} answer={said} stub-lines={len(rows)} "
          f"alert={alert} certificate-refused={str(cause is None).lower()}", flush=True)
    if cause is not None:
        raise RuntimeError("tls-control-" + cause)


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
            tls_control(helper, ready, folder, log)
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
    from unittest.mock import MagicMock, call, patch

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
    for remains, settings, failed_tool in [
        (False, False, False), (True, False, False), (False, True, False), (False, False, True)
    ]:
        identity = der_fingerprint(b"fixture")
        with patch("sys.platform", "darwin"), patch.dict(os.environ, env), \
             patch("__main__.ARMED") as armed, patch("__main__.CA"), \
             patch("ssl.PEM_cert_to_DER_cert", return_value=b"fixture"), \
             patch("__main__.checked") as tool, patch("__main__.present", return_value=remains) as read, \
             patch("__main__.admin_trust_remains", return_value=settings) as trust, redirect_stdout(io.StringIO()):
            armed.read_text.return_value = identity
            tool.side_effect = [RuntimeError("fixture") if failed_tool else b"", b""]
            if (undo() != (not failed_tool, remains or settings) or read.call_count != 1 or trust.call_count != 1
                    or [call.args[0][3] for call in tool.call_args_list] != ["remove-trusted-cert", "delete-certificate"]):
                raise RuntimeError("macos-undo-fixture")
    with patch("sys.platform", "win32"), patch.dict(os.environ, dict(env, CI_D_JOB="windows", RUNNER_OS="Windows")), \
         patch("__main__.ARMED") as armed, patch("__main__.CA"), \
         patch("ssl.PEM_cert_to_DER_cert", return_value=b"fixture"), patch("__main__.certutil") as tool, \
         patch("__main__.present", return_value=True) as read, redirect_stdout(io.StringIO()):
        armed.read_text.return_value = identity
        if undo() != (True, True):
            raise RuntimeError("windows-undo-fixture")
        tool.assert_called_once_with("-delstore", "Root", hashlib.sha1(b"fixture").hexdigest())
        read.assert_called_once_with(identity)
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
    with patch("__main__.hosted", return_value=True), patch("subprocess.Popen") as start, \
         patch("__main__.KEY_EXIT") as witness:
        proc = start.return_value
        proc.pid = os.getpid()
        proc.stdout.readline.return_value = b'{"app":"https://127.0.0.1:1","control":"http://127.0.0.1:2"}'
        def waited():
            witness.write_text.assert_not_called()
            return 0
        proc.wait.side_effect = waited
        with stub(WORK / "witness-fixture", True):
            witness.write_text.assert_not_called()
        proc.wait.assert_called_once_with()
        if witness.write_text.call_args is None:
            raise RuntimeError("key-wait-fixture")
        witness.write_text.assert_called_once_with(witness.write_text.call_args.args[0], encoding="ascii")
        if json.loads(witness.write_text.call_args.args[0])["exit"] != 0:
            raise RuntimeError("key-wait-fixture")
    gone = json.dumps({"exit": 0, "gone": True, "pid": 7, "start": "1.000000"})
    running = json.dumps({"exit": 0, "gone": False, "pid": 7, "start": "1.000000"})
    for evidence, initial, reasons, cleanup, code, expected in [
        (None, 0, "", (True, False), 1, "MISSED setup-or-cleanup"),
        ("invalid", 0, "", (True, False), 1, "MISSED setup-or-cleanup"),
        (gone, 0, "", (True, False), 0, "MET"),
        (gone, 0, "", (True, True), 0, "MET trust-left=true, key-gone=true, hosted"),
        (gone, 0, "", (False, True), 0, "MET trust-left=true, key-gone=true, hosted"),
        (gone, 1, "tls/status", (False, True), 1, "MISSED tls/status trust-left=true, key-gone=true, hosted"),
        (gone, 0, "", (False, False), 0, "MET trust-left=false, key-gone=true, hosted"),
        (None, 0, "", (False, True), 1, "MISSED setup-or-cleanup"),
        ("invalid", 0, "", (False, True), 1, "MISSED setup-or-cleanup"),
        ("0", 0, "", (True, False), 1, "MISSED setup-or-cleanup"),
        (running, 0, "", (True, False), 1, "MISSED setup-or-cleanup"),
        (running, 0, "", (False, True), 1, "MISSED setup-or-cleanup"),
        (json.dumps({"exit": 0, "pid": 7}), 0, "", (True, False), 1, "MISSED setup-or-cleanup"),
        (json.dumps({"exit": 0, "gone": "true"}), 0, "", (True, False), 1, "MISSED setup-or-cleanup"),
        (json.dumps({"exit": "0", "gone": True}), 0, "", (True, False), 1, "MISSED setup-or-cleanup"),
    ]:
        captured = io.StringIO()
        with patch("__main__.hosted", return_value=True), patch("__main__.KEY_EXIT") as witness, \
             patch("__main__.REASONS") as faults, patch("__main__.undo", return_value=cleanup) as clean, \
             redirect_stdout(captured):
            witness.is_file.return_value = evidence is not None
            witness.read_text.return_value = evidence
            faults.exists.return_value = bool(reasons)
            faults.read_text.return_value = reasons
            try:
                result = finish(initial)
            except Exception as exc:
                raise RuntimeError("key-verdict-fixture") from exc
            clean.assert_called_once_with()
        lines = captured.getvalue().splitlines()
        if result != code or "S6 real-helper: " + expected not in lines:
            raise RuntimeError("key-verdict-fixture")
        if (any(line.startswith("S6 TLS stub-exit=") and line.endswith(" key-gone=false") for line in lines)
                and any("key-gone=true" in line for line in lines)):
            raise RuntimeError("key-verdict-fixture")
        if evidence == gone and "S6 TLS stub-exit=0 key-gone=true" not in lines:
            raise RuntimeError("key-exit-line-fixture")
        if evidence == running and "S6 TLS stub-exit=0 key-gone=false" not in lines:
            raise RuntimeError("key-exit-line-fixture")
        refused += int(code != 0)
    with patch("__main__.hosted", return_value=False), patch("__main__.KEY_EXIT") as witness, \
         patch("__main__.undo") as native, patch("__main__.REASONS") as faults, redirect_stdout(io.StringIO()):
        faults.exists.return_value = False
        if finish(0):
            raise RuntimeError("local-verdict-fixture")
        witness.is_file.assert_not_called()
        native.assert_not_called()
    captured = io.StringIO()
    with patch("__main__.hosted", return_value=True), patch("__main__.KEY_EXIT") as witness, \
         patch("__main__.REASONS") as faults, patch("__main__.undo", side_effect=RuntimeError("authority-identity")), \
         redirect_stdout(captured):
        witness.read_text.return_value = gone
        faults.exists.return_value = False
        if finish(0) != 1 or "S6 real-helper: MISSED setup-or-cleanup" not in captured.getvalue().splitlines():
            raise RuntimeError("cleanup-identity-fixture")
        refused += 1
    captured = io.StringIO()
    with patch("sys.platform", "darwin"), patch("__main__.hosted", return_value=True), \
         patch("__main__.ARMED") as armed, patch("__main__.CA"), \
         patch("ssl.PEM_cert_to_DER_cert", return_value=b"fixture"), patch("__main__.checked") as native, \
         patch("__main__.present", return_value=False), patch("__main__.admin_trust_remains", return_value=True), \
         redirect_stdout(captured):
        armed.read_text.return_value = identity
        native.side_effect = [RuntimeError("certificate-tool-remove-trusted-cert-timeout-30s"), b""]
        try:
            outcome = undo()
        except RuntimeError as exc:
            raise RuntimeError("undo-operations-fixture") from exc
    reports = captured.getvalue().splitlines()
    if (outcome != (False, True) or [used.args[0][3] for used in native.call_args_list]
            != ["remove-trusted-cert", "delete-certificate"]
            or "S6 trust remove-trusted-cert result=certificate-tool-remove-trusted-cert-timeout-30s" not in reports
            or "S6 trust delete-certificate result=ok" not in reports
            or "S6 trust certificate-after-undo=false" not in reports):
        raise RuntimeError("undo-operations-fixture")
    with patch("__main__.reap") as reaped, patch("subprocess.Popen") as start:
        child = start.return_value.__enter__.return_value
        for code, answer, good in [(0, b"result=active\n", True), (3, b"result=active\n", False),
                                   (0, b"result=pending\n", False)]:
            child.stdout.read.return_value = answer
            def ended(proc, code=code):
                proc.returncode = code
                return 5, 100
            reaped.side_effect = ended
            if one_call("helper", "status", {})[2] != good:
                raise RuntimeError("helper-exit-fixture")
            refused += int(not good)
    with patch("__main__.hosted", return_value=False), patch("subprocess.Popen") as start, \
         patch("__main__.KEY_EXIT") as witness:
        try:
            with stub(WORK / "guard-stub", True):
                pass
        except Exception as exc:
            if type(exc) is not RuntimeError or str(exc) != "certificate-outside-hosted-job":
                raise RuntimeError("stub-guard-fixture") from exc
        else:
            raise RuntimeError("stub-guard-fixture")
        if start.called or witness.write_text.called:
            raise RuntimeError("stub-guard-fixture")
        refused += 1
    for action, refusal, case in [(install, "certificate-install-outside-hosted-job", "install-guard-fixture"),
                                  (undo, "certificate-undo-outside-hosted-job", "undo-guard-fixture")]:
        with patch("__main__.hosted", return_value=False), patch("__main__.ARMED") as armed, patch("__main__.CA"), \
             patch("ssl.PEM_cert_to_DER_cert", return_value=b"fixture"), patch("__main__.present", return_value=False), \
             patch("__main__.checked") as native, patch("subprocess.Popen") as start, patch("subprocess.run") as ran, \
             redirect_stdout(io.StringIO()):
            armed.exists.return_value = True
            try:
                action()
            except Exception as exc:
                if type(exc) is not RuntimeError or str(exc) != refusal:
                    raise RuntimeError(case) from exc
            else:
                raise RuntimeError(case)
            if native.called or start.called or ran.called:
                raise RuntimeError(case)
            refused += 1
    with patch("subprocess.run", side_effect=subprocess.TimeoutExpired(["security"], 30)) as native:
        try:
            admin_trust_remains(b"fixture")
        except RuntimeError as exc:
            if str(exc) != "admin-trust-readback-timeout-30s":
                raise RuntimeError("admin-readback-limit-fixture") from exc
        else:
            raise RuntimeError("admin-readback-limit-fixture")
        if (native.call_args.args[0][:3] != ["security", "trust-settings-export", "-d"]
                or native.call_args.kwargs.get("timeout") != 30):
            raise RuntimeError("admin-readback-limit-fixture")
        refused += 1
    captured = io.StringIO()
    with patch("__main__.hosted", return_value=False), patch("__main__.measure", return_value=[]) as measured, \
         patch("__main__.REASONS"), patch("sys.argv", ["check.py", "measure"]), redirect_stdout(captured):
        if run() != 0:
            raise RuntimeError("run-guard-fixture")
    if (measured.call_args_list != [call(False)]
            or "S6 TLS skipped: requires a GitHub-hosted runner" not in captured.getvalue().splitlines()):
        raise RuntimeError("run-guard-fixture")
    refused += 1
    for platform, case, expected in [
        ("darwin", "macos-add-fixture", ["sudo", "-n", "security", "add-trusted-cert", "-d", "-r", "trustRoot",
                                         "-p", "ssl", "-k", "/Library/Keychains/System.keychain"]),
        ("win32", "windows-add-fixture", [str(Path("system") / "System32" / "certutil.exe"), "-addstore", "Root"]),
    ]:
        with patch("sys.platform", platform), patch("__main__.hosted", return_value=True), \
             patch.dict(os.environ, {"SystemRoot": "system"}), patch("__main__.ARMED"), patch("__main__.CA") as authority, \
             patch("ssl.PEM_cert_to_DER_cert", return_value=b"fixture"), patch("__main__.present", side_effect=[False, True]), \
             patch("__main__.checked") as native, redirect_stdout(io.StringIO()):
            install()
        if native.call_args_list != [call(expected + [str(authority)])]:
            raise RuntimeError(case)
    alive = WORK / "key-alive-fixture"
    alive.mkdir()
    captured = io.StringIO()
    with patch("__main__.hosted", return_value=True), patch("subprocess.Popen") as start, \
         patch("__main__.KEY_EXIT", alive / "exit"), patch("__main__.REASONS") as faults, \
         patch("__main__.undo", return_value=(True, False)), redirect_stdout(captured):
        proc = start.return_value
        proc.pid = os.getpid()
        proc.wait.return_value = 0
        proc.stdout.readline.return_value = b'{"app":"https://127.0.0.1:1","control":"http://127.0.0.1:2"}'
        with stub(alive / "record", True):
            pass
        faults.exists.return_value = False
        result = finish(0)
    lines = captured.getvalue().splitlines()
    if (result != 1 or "S6 TLS stub-exit=0 key-gone=false" not in lines
            or "S6 real-helper: MISSED setup-or-cleanup" not in lines):
        raise RuntimeError("key-alive-fixture")
    refused += 1
    own = process_start(os.getpid())
    if (own is None or process_start(os.getpid()) != own
            or json.loads(stub_record(os.getpid(), own, 0))["gone"] is not False
            or json.loads(stub_record(os.getpid(), own + "1", 0))["gone"] is not True
            or json.loads(stub_record(os.getpid(), None, 0))["gone"] is not False):
        raise RuntimeError("process-read-fixture")
    with patch("__main__.process_start", side_effect=RuntimeError("stub-process-read")):
        if json.loads(stub_record(7, "1.000000", 0))["gone"] is not False:
            raise RuntimeError("process-read-fixture")
    ended = WORK / "key-ended-fixture"
    ended.mkdir()
    captured = io.StringIO()
    with patch("__main__.hosted", return_value=True), patch("subprocess.Popen") as start, \
         patch("__main__.KEY_EXIT", ended / "exit"), patch("__main__.REASONS") as faults, \
         patch("__main__.undo", return_value=(True, False)), \
         patch("__main__.process_start", side_effect=["1.000000", None]) as read, redirect_stdout(captured):
        proc = start.return_value
        proc.pid = 7
        proc.wait.return_value = 0
        proc.stdout.readline.return_value = b'{"app":"https://127.0.0.1:1","control":"http://127.0.0.1:2"}'
        with stub(ended / "record", True):
            pass
        faults.exists.return_value = False
        result = finish(0)
    if (result != 0 or read.call_args_list != [call(7), call(7)]
            or "S6 TLS stub-exit=0 key-gone=true" not in captured.getvalue().splitlines()):
        raise RuntimeError("key-ended-fixture")
    answered_row = {"seq": 1, "connection": 1, "method": "POST", "target": "/v1/checkout", "status": 200,
                    "result": "session"}
    alert_row = {"seq": 1, "error": "ERR_SSL_TLSV1_ALERT_UNKNOWN_CA"}
    for label, code, answer, rows, expected in [
        ("answered", 0, b"result=session url=https://127.0.0.1:1/c\n", [answered_row], "tls-control-answered"),
        ("no-alert", 1, b"error=failed-call\n", [], "tls-control-no-alert"),
        ("refused", 1, b"error=failed-call\n", [alert_row], "fixture-install-reached"),
    ]:
        base = WORK / ("control-" + label)
        base.mkdir()
        log = base / "tls" / "record" / "requests.jsonl"

        @contextmanager
        def stand_in(record, tls):
            record.mkdir()
            log.write_bytes(b"")
            yield {"app": "https://127.0.0.1:1", "control": "http://127.0.0.1:2"}

        def helper_call(args, rows=rows, code=code, answer=answer, **kwargs):
            with log.open("ab") as out:
                out.write(b"".join(json.dumps(row).encode() + b"\n" for row in rows))
            child = start.return_value.__enter__.return_value
            child.stdout.read.return_value = answer
            child.returncode = code
            return start.return_value

        captured = io.StringIO()
        with patch("__main__.WORK", base), patch("__main__.Path"), patch("__main__.stub", stand_in), \
             patch("__main__.CONTROL_WAIT_S", 0, create=True), patch("subprocess.Popen") as start, \
             patch("__main__.install", side_effect=RuntimeError("fixture-install-reached")) as added, \
             redirect_stdout(captured):
            start.side_effect = helper_call
            try:
                measure(True)
            except RuntimeError as exc:
                if str(exc) != expected:
                    raise RuntimeError("tls-control-fixture") from exc
            else:
                raise RuntimeError("tls-control-fixture")
        lines = captured.getvalue().splitlines()
        if added.called != (label == "refused") or len([line for line in lines if line.startswith("S6 TLS control ")]) != 1:
            raise RuntimeError("tls-control-fixture")
        if label == "refused" and lines != ["S6 TLS control command=subscribe exit=1 answer=error=failed-call "
                                            "stub-lines=1 alert=ERR_SSL_TLSV1_ALERT_UNKNOWN_CA certificate-refused=true"]:
            raise RuntimeError("tls-control-fixture")
        refused += int(label != "refused")
    for code, answer, rows, cause in [
        (1, [b"error=failed-call"], [alert_row], None),
        (1, [b"error=failed-call"], [dict(alert_row, error="ERR_SSL_SSLV3_ALERT_BAD_CERTIFICATE")], None),
        (0, [b"result=session"], [], "answered"),
        (1, [b"error=failed-call"], [answered_row], "answered"),
        (3, [b"error=no-credential"], [], "helper-exit-3"),
        (4, [b"error=store-fault"], [alert_row], "helper-exit-4"),
        (1, [b"result=session"], [alert_row], "helper-answer"),
        (1, [b"error=failed-call"], [], "no-alert"),
        (1, [b"error=failed-call"], [alert_row, alert_row], "lines-2"),
        (1, [b"error=failed-call"], [dict(alert_row, error="ERR_SSL_TLSV1_ALERT_PROTOCOL_VERSION")], "alert-other"),
        (1, [b"error=failed-call"], [{"seq": 1, "connection": 2, "error": "forced-close"}], "alert-other"),
    ]:
        if control_cause(code, answer, rows) != cause:
            raise RuntimeError("control-cause-fixture")
        refused += int(cause is not None)
    window = WORK / "control-window"
    window.write_bytes(b'{"seq":1,"error":"ERR_SSL_TLSV1_ALERT_UNKNOWN_CA"}\n7\n{"seq":3,"err')
    with patch("__main__.CONTROL_WAIT_S", 0):
        if (control_window(window, 0) != [alert_row, {"error": "unreadable-line"}, {"error": "unreadable-line"}]
                or control_window(window, window.stat().st_size) != []):
            raise RuntimeError("control-window-fixture")
    growing = MagicMock()
    growing.read_bytes.side_effect = [b"", b'{"seq":1,"err', b'{"seq":1,"error":"ERR_SSL_TLSV1_ALERT_UNKNOWN_CA"}\n']
    with patch("time.sleep") as paused:
        if control_window(growing, 0) != [alert_row] or growing.read_bytes.call_count != 3 or paused.call_count != 2:
            raise RuntimeError("control-window-fixture")
    print(f"PASS self-test:{NAME} ({refused} failing fixtures rejected)")


def finish(code):
    cleanup_failed = False
    limit = ""
    on_runner = hosted(os.environ, sys.platform)
    if on_runner:
        status, key_gone = key_record()
        print(f"S6 TLS stub-exit={status} key-gone={str(key_gone).lower()}", flush=True)
        cleanup_failed = not key_gone
        try:
            accepted, remains = undo()
            if key_gone and (remains or not accepted):
                limit = f" trust-left={str(remains).lower()}, key-gone=true, hosted"
        except Exception as exc:
            print(f"S6 trust undone=false cause={reason(exc)}")
            cleanup_failed = True
    try:
        reasons = REASONS.read_text().split() if REASONS.exists() else []
    except (OSError, ValueError):
        reasons = []
        cleanup_failed = True
    if cleanup_failed or (code and not reasons):
        reasons.append("setup-or-cleanup")
    if reasons:
        print("S6 real-helper: MISSED " + " ".join(reasons) + limit)
        print(f"FAIL {NAME}")
        return 1
    if on_runner:
        print("S6 real-helper: MET" + limit)
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
