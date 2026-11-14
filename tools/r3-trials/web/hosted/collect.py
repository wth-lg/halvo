import re
import sys

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
            cause = ("call_time_limit" if e in ("142", "143", "137") or pending.get("stopped")
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


def main():
    hosted, mac, partial = [], [], []
    it = iter(sys.argv[1:])
    for a in it:
        if a == "--hosted":
            hosted.append((next(it), next(it), next(it), next(it)))
        elif a == "--hosted-steps":
            partial.append((next(it), next(it), next(it), next(it), next(it).split(",")))
        elif a == "--mac":
            mac.extend(it)
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
            if m == "mac":
                continue
            judge(m, b, step, "10")
            for l in cell_lines.get((m, b, step), []):
                if m == "hosted-win" or field(l, "control_before"):
                    if field(l, "control_before") != "reply" or field(l, "control_after") != "reply":
                        missing.append(f"{m}/{b}/{step}:control")
    for c in checks:
        where = f"check/{field(c, 'machine')}/{field(c, 'browser')}/{field(c, 'step')}"
        rules = {k: field(c, k) for k in ("port_flag", "tcp_listen", "active_port_file",
                                           "executor_listener", "pipe_holder")}
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
        print(l)
    print(f"verdict executor=cdp version=0.9.1 form=pipe product={product} relay={relay} library=protocol_types")
    print(f"summary missing={len(missing)} missed={len(missed)} relay_bad={len(relay_bad)} "
          f"relay_missing={len(relay_missing)} udp_notes={len(notes)}", file=sys.stderr)
    for x in missing:
        print(f"missing {x}", file=sys.stderr)
    for x in missed:
        print(f"missed {x}", file=sys.stderr)
    for x in relay_bad + relay_missing:
        print(f"relay {x}", file=sys.stderr)
    for x in notes:
        print(f"note {x}", file=sys.stderr)


main()
