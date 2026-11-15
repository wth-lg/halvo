"""Makes the block of docs/r3/trials/driver.md again from the raw files of its runs (ruling CP-173 E5 (c)).

usage: python3 -I tools/r3-trials/drive/rebuild.py --cells DIR --helper-log FILE --job-log FILE
           --run ID --sha HEX --since UTC [--compare FILE]
       python3 -I tools/r3-trials/drive/rebuild.py --self-test

DIR holds one file mac-<model>-<task>-<seed>.txt per cell of this Mac, as the cell wrapper wrote it;
FILE of --helper-log is the timed-run helper's log; FILE of --job-log the log of the hosted job that
ran the cells of hosted-win in run ID at commit HEX. The models are those of the verdict line of
docs/r3/trials/model-cells.md, in its order, and for each the cells of bill, order and reply and
seeds 1 to 3. The block is: the hosted line, the prompt lines of tools/r3-trials/drive/prompts/,
then for each model its mac lines (one timed line per helper call of the cell that started at or
after --since, then the load, attempt, drive, reply and notrun lines of the cell file) and its
hosted-win lines, and last the verdict line that verdict.py's rule gives. With --compare it exits 1
when the block of FILE differs from this one. Only this program on the raw files tells a typed value
from a run's; the raw files are not in the repository.

Exit 0: done (and the same with --compare); 1: the block differs; 2: a refusal, one line
"REFUSED <cause>: <detail>" on standard error, causes usage and source.
"""
import hashlib
import importlib.util
import os
import re
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.realpath(os.path.join(HERE, "..", "..", ".."))
PROMPTS = ("boundary.txt", "goal-bill.txt", "goal-order.txt", "goal-reply.txt")
KEEP = ("load ", "attempt ", "drive ", "reply ", "notrun ")


def load_verdict():
    spec = importlib.util.spec_from_file_location("verdict", os.path.join(HERE, "verdict.py"))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


V = load_verdict()
Refusal = V.Refusal


def read(path):
    try:
        with open(path, encoding="utf-8", errors="strict", newline="") as f:
            return f.read()
    except (OSError, UnicodeDecodeError) as error:
        raise Refusal("source", "%s cannot be read: %s" % (os.path.basename(path), error))


def helper_records(text):
    rows = []
    for line in text.splitlines():
        f = line.split("\t")
        if len(f) >= 6:
            rows.append((f[0], f[1], f[2], f[5]))
    return rows


def mac_lines(cells, model, helper, since):
    out = []
    for task in ("bill", "order", "reply"):
        for seed in ("1", "2", "3"):
            path = os.path.join(cells, "mac-%s-%s-%s.txt" % (model, task, seed))
            if not os.path.exists(path):
                raise Refusal("source", "no cell file mac-%s-%s-%s.txt" % (model, task, seed))
            label = "mac cell %s %s %s" % (model, task, seed)
            for start, end, name, load1 in sorted(helper):
                if start >= since and (label + " " in name or label + "," in name):
                    out.append("timed model=%s machine=mac task=%s seed=%s start=%s end=%s load1=%s" % (model, task, seed, start, end, load1))
            out += [l for l in read(path).splitlines() if l.startswith(KEEP)]
    return out


def hosted_lines(log, model):
    pat = re.compile(r"((?:attempt|drive|reply|notrun) model=%s machine=hosted-win .*)$" % re.escape(model))
    out = []
    for line in log.splitlines():
        m = pat.search(line.rstrip("\r"))
        if m:
            out.append(m.group(1).rstrip())
    return out


def prompt_lines(prompts):
    out = []
    for name in PROMPTS:
        with open(os.path.join(prompts, name), "rb") as f:
            out.append("prompt file=%s sha256=%s" % (name, hashlib.sha256(f.read()).hexdigest()))
    return out


def block(cells, helper_text, job_text, run, sha, since, models, prompts):
    helper = helper_records(helper_text)
    lines = ["hosted machine=hosted-win run=%s sha=%s" % (run, sha)] + prompt_lines(prompts)
    for m in models:
        lines += mac_lines(cells, m, helper, since) + hosted_lines(job_text, m)
    _, drives, replies = V.read_lines(lines)
    return lines + [V.verdict(models, drives, replies)]


def self_test():
    failures = []

    def expect(name, got, want):
        if got != want:
            failures.append("%s: got %r, want %r" % (name, got, want))

    tmp = tempfile.mkdtemp(prefix="r3-drive-rebuild.")
    cells = os.path.join(tmp, "cells")
    os.mkdir(cells)
    for task in ("bill", "order", "reply"):
        for seed in ("1", "2", "3"):
            with open(os.path.join(cells, "mac-m_a-%s-%s.txt" % (task, seed)), "w", encoding="utf-8") as f:
                f.write("start=x\nhead=y\n")
                if task == "reply":
                    f.write("load model=m_a machine=mac task=reply seed=%s kind=write i=1 load1=1.00\n" % seed)
                    f.write("attempt model=m_a machine=mac task=reply seed=%s kind=write i=1 pass=no steps=0 wall_s=1 tokens_in=1 tokens_out=1\n" % seed)
                    f.write("reply model=m_a machine=mac seed=%s bodies=1 of=2 wall_s=1 model_tokens=2\n" % seed)
                else:
                    for kind, i in V.RUNS:
                        f.write("attempt model=m_a machine=mac task=%s seed=%s kind=%s i=%s pass=no steps=1 wall_s=1 tokens_in=0 tokens_out=0\n" % (task, seed, kind, i))
                    f.write("drive model=m_a machine=mac task=%s seed=%s compile=miss replays_ok=0 of=3 recover=miss steps=1 wall_s=1 model_tokens=0 typed_credential=0 effects_without_confirm=0\n" % (task, seed))
                f.write("exit=0\n")
    helper = "2026-11-13T10:00:00Z\t2026-11-13T10:01:00Z\t[l] r3 U0.6: mac cell m_a bill 1 (x)\t60.0\t0\t9.50\n"
    helper += "2026-11-13T09:00:00Z\t2026-11-13T09:01:00Z\t[l] r3 U0.6: mac cell m_a bill 1 (x)\t60.0\t0\t8.00\n"
    helper += "2026-11-13T10:02:00Z\t2026-11-13T10:03:00Z\t[l] r3 U0.6: mac cell m_a bill 10 (x)\t60.0\t0\t7.00\n"
    job = "windows\tstep\t2026-11-13T10:00:00Z attempt model=m_a machine=hosted-win task=reply seed=1 kind=write i=1 pass=yes steps=0 wall_s=1 tokens_in=1 tokens_out=1\r\n"
    job += "windows\tstep\t2026-11-13T10:00:00Z reply model=m_a machine=hosted-win seed=1 bodies=2 of=2 wall_s=1 model_tokens=2\r\n"
    job += "windows\tstep\t2026-11-13T10:00:00Z drive model=m_b machine=hosted-win task=bill seed=1 compile=miss\r\n"
    for name in PROMPTS:
        with open(os.path.join(tmp, name), "wb") as f:
            f.write(name.encode())
    out = block(cells, helper, job, "1", "a" * 40, "2026-11-13T09:30:00Z", ["m_a"], tmp)
    expect("the hosted line first", out[0], "hosted machine=hosted-win run=1 sha=" + "a" * 40)
    expect("four prompt lines", sum(1 for l in out if l.startswith("prompt ")), 4)
    expect("the prompt hash", out[1], "prompt file=boundary.txt sha256=" + hashlib.sha256(b"boundary.txt").hexdigest())
    expect("one timed line, of the call since the round began, with its load", [l for l in out if l.startswith("timed ")],
           ["timed model=m_a machine=mac task=bill seed=1 start=2026-11-13T10:00:00Z end=2026-11-13T10:01:00Z load1=9.50"])
    expect("the cell lines and no other line of a cell file", sum(1 for l in out if l.startswith(("start=", "head=", "exit="))), 0)
    expect("the attempt lines", sum(1 for l in out if l.startswith("attempt ")), 34)
    expect("the hosted lines of the model alone", sum(1 for l in out if "machine=hosted-win" in l and not l.startswith("hosted ")), 2)
    expect("the verdict line last", out[-1], "verdict driver=none path=B writer=none")
    os.rename(os.path.join(cells, "mac-m_a-order-2.txt"), os.path.join(cells, "mac-m_a-order-2.txt.gone"))
    try:
        block(cells, helper, job, "1", "a" * 40, "2026-11-13T09:30:00Z", ["m_a"], tmp)
        failures.append("a missing cell file: not refused")
    except Refusal:
        pass
    if failures:
        for f in failures:
            print("self-test: " + f)
        print("FAIL self-test: rebuild.py (%d checks missed)" % len(failures))
        return 1
    print("PASS self-test: rebuild.py (10 checks)")
    return 0


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    opts = {}
    it = iter(argv)
    for flag in it:
        if flag not in ("--cells", "--helper-log", "--job-log", "--run", "--sha", "--since", "--compare") or flag in opts:
            raise Refusal("usage", "unknown or repeated argument " + flag)
        opts[flag] = next(it, None)
        if opts[flag] is None:
            raise Refusal("usage", flag + " has no value")
    need = ("--cells", "--helper-log", "--job-log", "--run", "--sha", "--since")
    if any(k not in opts for k in need):
        raise Refusal("usage", "rebuild.py needs " + " ".join(need))
    models = V.candidates(read(os.path.join(ROOT, "docs/r3/trials/model-cells.md")))
    out = block(opts["--cells"], read(opts["--helper-log"]), read(opts["--job-log"]), opts["--run"], opts["--sha"],
                opts["--since"], models, os.path.join(HERE, "prompts"))
    if "--compare" not in opts:
        print("\n".join(out))
        return 0
    body, _ = V.last_block(read(opts["--compare"]))
    same = body == out
    print("rebuild compare lines=%d same=%s" % (len(out), "yes" if same else "no"))
    return 0 if same else 1


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Refusal as refusal:
        sys.stderr.write("REFUSED %s: %s\n" % (refusal.cause, refusal.detail))
        sys.exit(2)
