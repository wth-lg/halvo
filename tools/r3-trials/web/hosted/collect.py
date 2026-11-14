import io
import os
import re
import sys
import tempfile

KEEP = re.compile(r"^(cell |relay_cell |kill |control |way |check machine=|check_named |notrun |extras |call_stopped )")
STAMP = re.compile(r"^\d{4}-\d\d-\d\dT[0-9:.]+Z ")
CELLS = [("mac", "chrome"), ("hosted-mac", "chrome"), ("hosted-mac", "edge"),
         ("hosted-win", "chrome"), ("hosted-win", "edge")]
LIST = ["list_refuse", "list_hops", "list_close"]
SIX = ["payment_frame_isolated", "changed_page_1_isolated", "block_popup", "block_frame_post",
       "block_websocket", "cert_error"]
NINE = ["signin_form", "table_read", "form_fill", "payment_frame", "file_upload", "session_expiry",
        "changed_page_1", "block_request", "credential_mark"]
REACH = ["reach_page", "reach_opened", "reach_frame", "reach_worker", "reach_shared", "reach_service"]
NEW = ["pipe_inherit", "relay_foreign", "start_probe"]
TWENTY = ["driver_killed_pipe", "driver_killed_pipe_relay"]
ORG = ["org_debugging", "org_proxy"]
SAME = ["same_site_pref", "same_site_rule"]
POLICY_STEP = {"RemoteDebuggingAllowed": "org_debugging", "ProxyMode": "org_proxy"}
CALL_LIMIT_EXITS = ("142", "143", "137")
CHECK_RULES = ("port_flag", "tcp_listen", "active_port_file", "executor_listener", "pipe_holder")
ORG_MACHINES = ("hosted-mac", "hosted-win")
USAGE = """usage: python3 -I tools/r3-trials/web/hosted/collect.py [--hosted MACHINE RUN SHA LOG]...
           [--hosted-steps MACHINE RUN SHA LOG STEP,...]... [--mac FILE...]
       python3 -I tools/r3-trials/web/hosted/collect.py --self-test
       python3 -I tools/r3-trials/web/hosted/collect.py --help
Builds the third round's block of docs/r3/trials/web-isolation.md from the web trial's lines.
  --hosted        a job log of MACHINE (mac, hosted-mac or hosted-win) from run RUN at commit SHA
  --hosted-steps  a later job log whose lines of the named steps (STEP, or BROWSER:STEP for one
                  browser) replace the earlier logs' lines of each cell that it holds
  --mac FILE...   this Mac's capture files, the rest of the arguments; a later file's cell of a
                  browser and a step replaces the earlier files' lines of that browser and step
Prints the block and its verdict line on standard output and a summary on standard error.
--self-test runs the named cases of the collector's rules on fixtures of a few lines and prints
one line per case and "self-test cases=<n> failed=<n>"; exit 0 when none failed.
Exit 64 for a bad argument.
"""


class UsageError(Exception):
    pass


def field(line, key):
    m = re.search(r"(?:^| )" + key + r"=(\S+)", line)
    return m.group(1) if m else None


def lines_of(path, machine=None):
    out = []
    channel = "pipe"
    unset = None
    plist = None
    pending = None
    for raw in open(path, encoding="utf-8", errors="replace"):
        line = STAMP.sub("", raw.rstrip("\n")).rstrip("\r")
        if line.startswith("start candidate="):
            channel = field(line, "channel") or "pipe"
        if line.startswith("policy set "):
            empty = " mode=" in line and not field(line, "mode")
            if " seen=" in line:
                empty = field(line, "seen") != "yes"
                keys = [("owner", "plist_owner"), ("mode", "plist_mode"), ("value", "plist_value"),
                        ("page_named", "page_named"), ("page_platform", "page_platform"),
                        ("control_platform", "control_platform")]
                extra = " ".join(f"{k2}={field(line, k)}" for k, k2 in keys if field(line, k))
                plist = (POLICY_STEP.get(field(line, "name")), extra) if extra else None
                if field(line, "seen") == "yes":
                    pending = {"browser": field(line, "browser"), "step": POLICY_STEP.get(field(line, "name")),
                               "ok": 0, "done": 0, "refused": False}
            unset = POLICY_STEP.get(field(line, "name")) if empty else None
        if pending and line.startswith("run candidate=") and field(line, "step") == pending["step"]:
            pending["done"] += 1
            pending["ok"] += field(line, "result") == "ok"
        if pending and line.startswith("REFUSED port"):
            pending["refused"] = True
        if pending and line.startswith("call_stopped ") and field(line, "step") == pending["step"]:
            pending["stopped"] = True
        if (pending and line.startswith("notrun ") and field(line, "cause") == "signal"
                and field(line, "step") == pending["step"]):
            pending["stopped"] = True
            continue
        if pending and line.startswith("cell ") and field(line, "step") == pending["step"]:
            pending = None
        if pending and line.startswith("call_end "):
            e = field(line, "exit")
            cause = ("call_time_limit" if e in CALL_LIMIT_EXITS or pending.get("stopped")
                     else ("port_taken" if pending["refused"] else f"call_exit_{e}"))
            line = (f"notrun candidate=cdp machine={machine} browser={pending['browser']} step={pending['step']} "
                    f"cause={cause} runs_ok={pending['ok']} runs_done={pending['done']}")
            pending = None
        if line.startswith("policy removed="):
            unset = None
        if plist and line.startswith(("cell ", "notrun ")) and field(line, "step") == plist[0]:
            line = line + " " + plist[1]
            plist = None
        if unset and line.startswith("cell ") and field(line, "step") == unset:
            line = (f"notrun candidate=cdp machine={field(line, 'machine')} "
                    f"browser={field(line, 'browser')} step={unset} cause=policy_not_set "
                    f"ran_without_policy_ok={field(line, 'ok')}_of_{field(line, 'of')}")
        if line.startswith("org_control at=after "):
            k = (field(line, "browser"), field(line, "step"))
            for idx in range(len(out) - 1, -1, -1):
                o = out[idx]
                if o.startswith("cell ") and (field(o, "browser"), field(o, "step")) == k:
                    reply = "reply" if " reply=yes" in line else "none"
                    out[idx] = o + f" control_after={reply}"
                    break
            continue
        if KEEP.match(line) or (line.startswith("run candidate=") and " step=relay_port_free " in line):
            if machine and field(line, "machine") not in (machine, None):
                continue
            if line.startswith("check machine="):
                b = " browser=" + field(line, "browser")
                line = line.replace(b, b + f" form={channel}_relay", 1)
            out.append(line)
    return out


def values(args, i, n, form):
    v = args[i + 1:i + 1 + n]
    if len(v) < n or any(x.startswith("--") for x in v):
        raise UsageError(f"{args[i]} takes {form}")
    return v


def collect(args, out, err):
    hosted, mac, partial = [], [], []
    i = 0
    while i < len(args):
        a = args[i]
        if a == "--hosted":
            hosted.append(tuple(values(args, i, 4, "MACHINE RUN SHA LOG")))
            i += 5
        elif a == "--hosted-steps":
            m, run, sha, log, steps = values(args, i, 5, "MACHINE RUN SHA LOG STEP,...")
            partial.append((m, run, sha, log, steps.split(",")))
            i += 6
        elif a == "--mac":
            mac.extend(args[i + 1:])
            break
        else:
            raise UsageError(f"unknown argument {a}")
    block = []
    for m, run, sha, _ in hosted:
        block.append(f"hosted machine={m} run={run} sha={sha}")
    for m, run, sha, _, steps in partial:
        block.append(f"hosted machine={m} run={run} sha={sha} steps={','.join(steps)}")
    def cells_in(ls):
        return {(field(l, "browser"), field(l, "step")) for l in ls
                if l.startswith(("cell ", "notrun ")) and field(l, "browser")}

    mac_files = [lines_of(f, "mac") for f in mac]
    for i, ls in enumerate(mac_files):
        later = [cells_in(x) for x in mac_files[i + 1:]]
        block.extend(l for l in ls if not any(
            (field(l, "browser"), field(l, "step")) in c
            or (field(l, "browser") is None and any(x[1] == field(l, "step") for x in c))
            for c in later))

    def names(l, steps):
        return field(l, "step") in steps or f"{field(l, 'browser')}:{field(l, 'step')}" in steps

    ran = []
    for m, _, _, log, steps in partial:
        own = [l for l in lines_of(log, m) if names(l, steps)]
        cells_ran = {(field(l, "browser"), field(l, "step")) for l in own
                     if l.startswith(("cell ", "notrun ")) and field(l, "browser")}
        checks_ran = {(field(l, "browser"), field(l, "form"), field(l, "step")) for l in own
                      if l.startswith("check machine=")}
        ran.append((m, own, cells_ran, checks_ran))

    def taken_by(l, m, later):
        b, st = field(l, "browser"), field(l, "step")
        if l.startswith("check machine=") and any(
                pm == m and (b, field(l, "form"), st) in ck for pm, _, _, ck in later):
            return True
        return any(pm == m and ((b, st) in cr or (b is None and any(x[1] == st for x in cr)))
                   for pm, _, cr, _ in later)

    for m, _, _, log in hosted:
        block.extend(l for l in lines_of(log, m) if not taken_by(l, m, ran))
    for i, (m, own, _, _) in enumerate(ran):
        block.extend(l for l in own if not taken_by(l, m, ran[i + 1:]))
    block = [l for l in block if not l.startswith("extras ")]
    def wants_relay(step):
        return None if step == "driver_killed_pipe" else "pipe_relay"

    cell_lines = {}
    notruns = {}
    ways = {}
    checks = []
    for l in block:
        key = (field(l, "machine"), field(l, "browser"), field(l, "step"))
        if l.startswith("cell ") and field(l, "relay") == wants_relay(key[2]):
            cell_lines.setdefault(key, []).append(l)
        if l.startswith("notrun ") and key[1] and key[2]:
            notruns.setdefault(key, []).append(l)
        if l.startswith("way row=frame_doc"):
            ways[(field(l, "machine"), field(l, "browser"), field(l, "form"))] = l
        if l.startswith("check machine=") and field(l, "form") == "pipe_relay":
            checks.append(l)
    missing, missed, notes = [], [], []

    def judge(m, b, step, want):
        lines = cell_lines.get((m, b, step), [])
        if not lines:
            missing.append(f"{m}/{b}/{step}")
            return
        bad = [l for l in lines if field(l, "ok") != want or field(l, "of") != want]
        if bad:
            missed.append(f"{m}/{b}/{step}:{field(bad[0], 'ok')}/{field(bad[0], 'of')}")
        elif (m, b, step) in notruns:
            missing.append(f"{m}/{b}/{step}:notrun")

    def row_result(line, m):
        r = field(line, "result")
        if field(line, "cause") is not None and r in ("ok", "miss", "open"):
            return r
        g, f = field(line, "gone_ms"), field(line, "free_ms")
        if g in (None, "-"):
            return "open"
        if f not in (None, "-"):
            if int(f) < int(g):
                return "miss"
            if int(f) == int(g):
                return "open"
        lb, la = field(line, "left_before"), field(line, "left_after")
        if lb is None or la is None:
            return "ok" if m == "hosted-mac" else "open"
        return "ok" if lb == "0" and la == "0" else "miss"

    def judge_row(m, b):
        step = "relay_port_free"
        rows = [l for l in block if l.startswith("run ") and field(l, "machine") == m
                and field(l, "browser") == b and field(l, "step") == step]
        if (m, b, step) in notruns:
            missing.append(f"{m}/{b}/{step}:notrun")
        if not rows:
            missing.append(f"{m}/{b}/{step}")
            return
        res = [row_result(l, m) for l in rows]
        if "miss" in res:
            missed.append(f"{m}/{b}/{step}:runs_miss_{res.count('miss')}_of_{len(res)}")
        elif "open" in res:
            missing.append(f"{m}/{b}/{step}:runs_open_{res.count('open')}_of_{len(res)}")
        elif len(res) < 10:
            missing.append(f"{m}/{b}/{step}:runs_{len(res)}")

    cells = {k: v[-1] for k, v in cell_lines.items()}
    for m, b in CELLS:
        for step in LIST + SIX + NINE + REACH + NEW + TWENTY:
            judge(m, b, step, "20" if step in TWENTY else "10")
        judge_row(m, b)
        fd = ways.get((m, b, "pipe_relay"))
        if fd is None:
            missing.append(f"{m}/{b}/frame_doc")
        elif field(fd, "told") != "10" or field(fd, "runs") != "10":
            missed.append(f"{m}/{b}/frame_doc:{field(fd, 'told')}/{field(fd, 'runs')}")
        for step in ORG:
            if m not in ORG_MACHINES:
                continue
            judge(m, b, step, "10")
            for l in cell_lines.get((m, b, step), []):
                if m == "hosted-win" or field(l, "control_before"):
                    if field(l, "control_before") != "reply" or field(l, "control_after") != "reply":
                        missing.append(f"{m}/{b}/{step}:control")
    for c in checks:
        where = f"check/{field(c, 'machine')}/{field(c, 'browser')}/{field(c, 'step')}"
        rules = {k: field(c, k) for k in CHECK_RULES}
        unread = [k for k, v in rules.items() if v is None]
        if unread:
            missing.append(f"{where}:no_{','.join(unread)}")
        bad = []
        for k, v in rules.items():
            if v in ("0", None):
                continue
            if k in ("port_flag", "pipe_holder"):
                passing, opened = field(c, k + "_passing"), field(c, k + "_open")
                if passing is None or opened is None:
                    missing.append(f"{where}:{k}_unclassed")
                    continue
                if int(v) - int(passing) - int(opened) > 0:
                    bad.append(k)
                elif int(opened) > 0:
                    missing.append(f"{where}:{k}_open")
                continue
            bad.append(k)
        if field(c, "table_unread") not in ("0", None):
            missing.append(f"{where}:table_unread")
        if bad:
            missed.append(f"{where}:{','.join(bad)}")
        if field(c, "udp_wide") not in ("0", None):
            notes.append(f"udp_wide/{field(c, 'machine')}/{field(c, 'browser')}/{field(c, 'step')}={field(c, 'udp_wide')}")
    for m, b in CELLS:
        if not any(field(c, "machine") == m and field(c, "browser") == b for c in checks):
            missing.append(f"check/{m}/{b}:none")
    product = "no" if missed else ("open" if missing else "yes")
    relay_bad, relay_missing = [], []
    for m, b in CELLS:
        ks = [l for l in block if l.startswith("kill ") and field(l, "machine") == m
              and field(l, "browser") in (b, None) and field(l, "form") == "pipe_relay"]
        if m in ("mac", "hosted-win") and b == "chrome" and not ks:
            relay_missing.append(f"kill/{m}/pipe_relay")
        for k in ks:
            if field(k, "arrived") != "0":
                relay_bad.append(k)
            elif field(k, "kills") != "20":
                relay_missing.append(f"kill/{m}/pipe_relay:kills_{field(k, 'kills')}")
        for step in NINE:
            rc = [l for l in block if l.startswith("relay_cell ") and field(l, "machine") == m
                  and field(l, "browser") == b and field(l, "form") == "pipe_relay"
                  and field(l, "step") == step]
            if not rc:
                if m in ("mac", "hosted-win") and b == "chrome":
                    relay_missing.append(f"relay_cell/{m}/{b}/{step}")
            else:
                relay_bad.extend(l for l in rc if field(l, "ok") != "10" or field(l, "of") != "10")
    relay = "no" if relay_bad else ("open" if relay_missing else "yes")
    pref = [cells.get((m, b, "same_site_pref")) for m, b in CELLS]
    rule = [cells.get((m, b, "same_site_rule")) for m, b in CELLS]

    def every_ok(ls):
        if any(l is None for l in ls):
            return "open"
        return "yes" if all(field(l, "ok") == field(l, "of") == "10" for l in ls) else "no"

    p, r = every_ok(pref), every_ok(rule)
    writes = "yes" if p == "yes" else ("open" if p == "open" else "no")
    block.append(f"same_site pref={p} rule={r} start_writes_setting={writes}")
    for l in block:
        print(l, file=out)
    print(f"verdict executor=cdp version=0.9.1 form=pipe product={product} relay={relay} library=protocol_types", file=out)
    print(f"summary missing={len(missing)} missed={len(missed)} relay_bad={len(relay_bad)} "
          f"relay_missing={len(relay_missing)} udp_notes={len(notes)}", file=err)
    for x in missing:
        print(f"missing {x}", file=err)
    for x in missed:
        print(f"missed {x}", file=err)
    for x in relay_bad + relay_missing:
        print(f"relay {x}", file=err)
    for x in notes:
        print(f"note {x}", file=err)


WIN = "machine=hosted-win browser=chrome"
MACH = "machine=hosted-mac browser=chrome"
START = f"start candidate=cdp {WIN} channel=pipe"
RULES0 = "port_flag=0 tcp_listen=0 active_port_file=0 executor_listener=0 pipe_holder=0"


def cell(machine, step, ok="10", extra=" relay=pipe_relay"):
    return f"cell candidate=cdp {machine} major=156 step={step} ok={ok} of=10 median_ms=1{extra}"


def check(step, rules):
    return f"check {WIN} step={step} ok=10 of=10 {rules}"


SEEN = "policy set browser=chrome name=RemoteDebuggingAllowed owner=root:wheel mode=-rw-r--r-- value=0 seen=yes"
RUN_ORG = f"run candidate=cdp {MACH} step=org_debugging i=1 result=ok"
PRF = f"run candidate=cdp {WIN} step=relay_port_free i=1"

# name, the DELETE line of the comment pass, the logs ("hosted-win:" or "hosted-mac:" or "steps:" or
# "mac:"), and what must hold: ("out" or "err", "has" or "lacks", text)
CASES = [
    ("a_policy_file_missing_means_the_cell_measured_no_policy", 52,
     {"hosted-mac": ["policy set browser=chrome name=RemoteDebuggingAllowed file_existed=no owner= mode= value=",
                     cell(MACH, "org_debugging")]},
     [("out", "has", "step=org_debugging cause=policy_not_set ran_without_policy_ok=10_of_10")]),
    ("a_policy_page_that_did_not_show_it_means_no_policy", 56,
     {"hosted-mac": ["policy set browser=chrome name=RemoteDebuggingAllowed owner=root:wheel mode=-rw-r--r-- value=0 seen=no",
                     cell(MACH, "org_debugging")]},
     [("out", "has", "step=org_debugging cause=policy_not_set")]),
    ("a_signal_notrun_folds_into_the_call_time_limit", 78,
     {"hosted-mac": [SEEN, RUN_ORG, "call_stopped browser=chrome step=org_debugging limit_s=420 runs_ok=1 runs_done=1 group_left=0",
                     f"notrun candidate=cdp {MACH} step=org_debugging cause=signal",
                     "call_end exit=1 seconds=420 port=18190 args=-"]},
     [("out", "has", "step=org_debugging cause=call_time_limit runs_ok=1 runs_done=1"), ("out", "lacks", "cause=signal")]),
    ("an_alarm_exit_is_the_call_time_limit", 91,
     {"hosted-mac": [SEEN, RUN_ORG, "call_end exit=142 seconds=300 port=18190 args=-"]},
     [("out", "has", "step=org_debugging cause=call_time_limit runs_ok=1 runs_done=1")]),
    ("the_control_after_the_removal_goes_onto_its_cell", 107,
     {"hosted-win": [cell(WIN, "org_proxy", extra=" relay=pipe_relay control_before=reply"),
                     "org_control at=after browser=chrome step=org_proxy reply=yes cause=-"]},
     [("out", "has", "control_before=reply control_after=reply")]),
    ("the_run_lines_of_relay_port_free_are_kept", 117,
     {"hosted-win": [f"{PRF} result=ok cause=- gone_ms=5 free_ms=6 left_before=0 left_after=0"]},
     [("out", "has", f"{PRF} result=ok cause=-")]),
    ("a_later_mac_file_takes_the_cell", 143,
     {"mac": [cell("machine=mac browser=chrome", "signin_form", ok="9")],
      "mac2": [cell("machine=mac browser=chrome", "signin_form")]},
     [("out", "has", "step=signin_form ok=10 of=10"), ("out", "lacks", "step=signin_form ok=9 of=10")]),
    ("a_steps_log_replaces_only_the_cells_that_it_holds", 161,
     {"hosted-win": [cell(WIN, "signin_form", ok="9"), cell("machine=hosted-win browser=edge", "signin_form", ok="8"),
                     "call_stopped step=signin_form limit_s=420"],
      "steps": [cell(WIN, "signin_form")]},
     [("out", "has", f"{WIN} major=156 step=signin_form ok=10"), ("out", "lacks", "step=signin_form ok=9"),
      ("out", "has", "browser=edge major=156 step=signin_form ok=8"), ("out", "lacks", "call_stopped")]),
    ("a_later_check_line_replaces_the_earlier", 169,
     {"hosted-win": [START, check("frame_doc", RULES0.replace("port_flag=0", "port_flag=7"))],
      "steps": [START, check("frame_doc", RULES0)]},
     [("out", "has", "step=frame_doc ok=10 of=10 port_flag=0"), ("out", "lacks", "port_flag=7")]),
    ("every_line_of_a_cell_counts", 188,
     {"hosted-win": [cell(WIN, "signin_form", ok="9"), cell(WIN, "signin_form")]},
     [("err", "has", "missed hosted-win/chrome/signin_form:9/10")]),
    ("a_cell_line_counts_only_in_its_steps_relay_form", 188,
     {"hosted-win": [cell(WIN, "signin_form", extra="")]},
     [("err", "has", "missing hosted-win/chrome/signin_form\n")]),
    ("a_notrun_line_holds_its_cell_open", 188,
     {"hosted-win": [cell(WIN, "signin_form"), f"notrun candidate=cdp {WIN} step=signin_form cause=browser_start"]},
     [("err", "has", "missing hosted-win/chrome/signin_form:notrun")]),
    ("relay_port_free_with_the_port_free_first_is_a_miss", 222,
     {"hosted-win": [f"{PRF} result=ok gone_ms=100 free_ms=50 left_before=0 left_after=0"]},
     [("err", "has", "missed hosted-win/chrome/relay_port_free:runs_miss_1_of_1")]),
    ("relay_port_free_without_left_counts_is_open_off_the_hosted_mac", 222,
     {"hosted-win": [f"{PRF} result=ok gone_ms=100 free_ms=200"]},
     [("err", "has", "missing hosted-win/chrome/relay_port_free:runs_open_1_of_1")]),
    ("the_mac_org_cells_hold_no_verdict_open", 273,
     {"hosted-win": [cell(WIN, "signin_form")]},
     [("err", "lacks", "missing mac/chrome/org_"), ("err", "has", "missing hosted-win/chrome/org_debugging")]),
    ("a_hosted_org_cell_needs_a_reply_before_and_after", 279,
     {"hosted-win": [cell(WIN, "org_debugging", extra=" relay=pipe_relay control_before=reply")]},
     [("err", "has", "missing hosted-win/chrome/org_debugging:control")]),
    ("a_check_line_without_one_of_the_five_rules_is_open", 289,
     {"hosted-win": [START, check("signin_form", RULES0.replace("tcp_listen=0 ", ""))]},
     [("err", "has", "missing check/hosted-win/chrome/signin_form:no_tcp_listen")]),
    ("a_count_above_its_passing_and_open_is_a_miss", 298,
     {"hosted-win": [START, check("signin_form", RULES0.replace("port_flag=0", "port_flag=3 port_flag_passing=1 port_flag_open=0"))]},
     [("err", "has", "missed check/hosted-win/chrome/signin_form:port_flag")]),
    ("an_open_count_holds_the_check_open", 298,
     {"hosted-win": [START, check("signin_form", RULES0.replace("port_flag=0", "port_flag=3 port_flag_passing=1 port_flag_open=2"))]},
     [("err", "has", "missing check/hosted-win/chrome/signin_form:port_flag_open"), ("err", "lacks", "missed check/")]),
    ("a_count_without_its_two_keys_is_no_read", 298,
     {"hosted-win": [START, check("signin_form", RULES0.replace("port_flag=0", "port_flag=3"))]},
     [("err", "has", "missing check/hosted-win/chrome/signin_form:port_flag_unclassed")]),
    ("a_kill_line_of_another_browser_does_not_stand_for_chrome", 304,
     {"hosted-win": ["kill machine=hosted-win browser=edge form=pipe_relay kills=20 arrived=0",
                     "kill machine=hosted-win browser=chrome form=pipe kills=20 arrived=0"]},
     [("err", "has", "relay kill/hosted-win/pipe_relay\n")]),
    ("a_kill_line_without_a_browser_counts_for_its_machine", 304,
     {"hosted-win": ["kill machine=hosted-win form=pipe_relay kills=20 arrived=0"]},
     [("err", "lacks", "relay kill/hosted-win/pipe_relay")]),
    ("a_miss_decides_the_verdict_before_an_open_cell", 321,
     {"hosted-win": [cell(WIN, "signin_form", ok="9")]},
     [("out", "has", "product=no relay="), ("err", "has", "missing hosted-win/edge/")]),
]


def run_case(logs, work):
    args, macs = [], []
    for name, lines in logs.items():
        path = os.path.join(work, name + ".log")
        with open(path, "w", encoding="utf-8") as f:
            f.write("".join(l + "\n" for l in lines))
        if name in ("hosted-win", "hosted-mac"):
            args += ["--hosted", name, "1", "0" * 40, path]
        elif name == "steps":
            steps = sorted({field(l, "step") for l in lines if field(l, "step")})
            args += ["--hosted-steps", "hosted-win", "2", "1" * 40, path, ",".join(steps)]
        else:
            macs.append(path)
    if macs:
        args += ["--mac"] + macs
    out, err = io.StringIO(), io.StringIO()
    collect(args, out, err)
    return {"out": out.getvalue(), "err": err.getvalue()}


def self_test():
    failed = 0
    count = 0
    with tempfile.TemporaryDirectory(prefix="r3-collect-self-test-") as work:
        for name, line, logs, wants in CASES:
            got = run_case(logs, work)
            bad = [f"{stream}_{how}" for stream, how, text in wants if (text in got[stream]) != (how == "has")]
            count += 1
            failed += bool(bad)
            print(f"case {name} rule_line={line} ok={'no' if bad else 'yes'} failed_on={','.join(bad) or '-'}")
        for name, args in [("an_unknown_flag_is_refused", ["--host", "x"]),
                           ("a_missing_value_is_refused", ["--hosted", "hosted-win", "1", "x"]),
                           ("a_flag_in_place_of_a_value_is_refused", ["--hosted-steps", "hosted-win", "1", "x", "--mac", "y"])]:
            sink = io.StringIO()
            code = main(args, sink, sink)
            ok = code == 64 and "usage: " in sink.getvalue() and "REFUSED usage: " in sink.getvalue()
            count += 1
            failed += not ok
            print(f"case {name} exit={code} ok={'yes' if ok else 'no'}")
    print(f"self-test cases={count} failed={failed}")
    return 0 if failed == 0 else 1


def main(argv, out, err):
    if argv[:1] == ["--help"]:
        print(USAGE, end="", file=out)
        return 0
    if argv[:1] == ["--self-test"]:
        return self_test()
    try:
        collect(argv, out, err)
    except UsageError as e:
        print(f"REFUSED usage: {e}", file=err)
        print(USAGE, end="", file=err)
        return 64
    return 0


sys.exit(main(sys.argv[1:], sys.stdout, sys.stderr))
