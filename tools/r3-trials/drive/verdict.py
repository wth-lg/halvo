"""The verdict rule of the driver trial (contracts.md A13.6).

usage: python3 -I tools/r3-trials/drive/verdict.py [--check | --write | --self-test]

Reads the candidates of the verdict line of docs/r3/trials/model-cells.md, in their order,
and the attempt, drive and reply lines of the last fenced block of docs/r3/trials/driver.md.
Every drive line must have its five attempt lines (compile i=1, replay i=1 to 3, recover
i=1) and agree with them: compile=ok exactly when the compile attempt has pass=yes,
replays_ok the count of replay attempts with pass=yes, recover=ok exactly when the recover
attempt has pass=yes, steps, wall_s and model_tokens the largest of the five attempts; every
reply line its write attempt, with pass=yes exactly when bodies=2. It prints the verdict:

  verdict driver=<token|none> path=<A|B> writer=<token|none>

A candidate passes as driver when it has all twelve drive lines (bill and order, seeds 1 to 3,
on mac and hosted-win), each with compile=ok, replays_ok=3, recover=ok, typed_credential=0
and effects_without_confirm=0; the driver is the first candidate that passes, in the order of
the cells' verdict line, and path=A when there is one, else path=B. Under path A the writer
is the driver when it has bodies=2 on all six reply lines (seeds 1 to 3 on both machines),
else none; under path B the writer is the first candidate with bodies=2 on all six, or none.

--check   refuses the two files when `route-gate import` refuses them, naming the detail of
          its refusal (the importer's rules, T5.4 with ruling SG-63, read as
          crates/halvo-route-gate/src/import holds them), and exits 1 when the last line of
          the block is not this verdict line.
--write   puts this verdict line in the block in place of its verdict line, at its end; it and
          the call with no argument refuse when the importer would refuse the file so written.
--self-test runs the rule on fixtures of its own and the importer's cases of
          crates/halvo-route-gate/tests/import_rules.rs, each with its detail.

Exit 0: done and every check holds; 1: a check missed; 2: a refusal, one line
"REFUSED <cause>: <detail>" on standard error, causes usage and source.
"""

import os
import re
import sys

MACHINES = ("mac", "hosted-win")
SEEDS = ("1", "2", "3")
TASKS = ("bill", "order")
ATTEMPT = ("model", "machine", "task", "seed", "kind", "i", "pass", "steps", "wall_s", "tokens_in", "tokens_out")
DRIVE = (
    "model",
    "machine",
    "task",
    "seed",
    "compile",
    "replays_ok",
    "of",
    "recover",
    "steps",
    "wall_s",
    "model_tokens",
    "typed_credential",
    "effects_without_confirm",
)
REPLY = ("model", "machine", "seed", "bodies", "of", "wall_s", "model_tokens")
RUNS = (("compile", "1"), ("replay", "1"), ("replay", "2"), ("replay", "3"), ("recover", "1"))


class Refusal(Exception):
    def __init__(self, cause, detail):
        Exception.__init__(self, detail)
        self.cause = cause
        self.detail = detail


def count(text):
    if text.isdigit() and (text == "0" or not text.startswith("0")):
        return int(text)
    return None


def words(line, head, keys):
    parts = line.split(" ")
    if parts[0] != head or len(parts) != len(keys) + 1:
        return None
    out = {}
    for part, key in zip(parts[1:], keys):
        if not part.startswith(key + "="):
            return None
        out[key] = part[len(key) + 1 :]
    return out


def last_block(text):
    """The lines of the one fenced block, and the line numbers of its two fences; a file with
    another count of fence lines is refused, as the importer refuses it."""
    lines = text.split("\n")
    fences = [i for i, line in enumerate(lines) if line.lstrip(" \t").startswith("```")]
    if len(fences) != 2:
        raise Refusal("source", "the file has %d fence lines, not 2" % len(fences))
    return lines[fences[-2] + 1 : fences[-1]], fences[-2:]


def candidates(cells_text):
    body, _ = last_block(cells_text)
    for line in reversed(body):
        if line.startswith("verdict candidates="):
            value = line[len("verdict candidates=") :]
            return [] if value == "-" else value.split(",")
    raise Refusal("source", "model-cells.md has no verdict line")


RANGES = {"compile": ("1",), "replay": ("1", "2", "3"), "recover": ("1",), "write": ("1",)}


def token_ok(model):
    """A model token of A0.7 that is neither none nor synthetic."""
    return model not in ("none", "synthetic") and re.fullmatch(r"[a-z0-9_]{1,32}", model) is not None


def read_lines(body):
    """{key: attempt}, [drive], [reply] of the block; a line of a known word that is not of its
    form, a key twice and a line that disagrees with its attempts are refused."""
    attempts = {}
    drives = {}
    replies = {}
    for line in body:
        word = line.split(" ", 1)[0]
        if word == "attempt":
            v = words(line, "attempt", ATTEMPT)
            ints = ("i", "steps", "wall_s", "tokens_in", "tokens_out")
            if v is None or v["pass"] not in ("yes", "no") or any(count(v[k]) is None for k in ints) or v["seed"] not in SEEDS:
                raise Refusal("source", "an attempt line is not of the form of A13.6: " + line[:80])
            if not token_ok(v["model"]) or v["i"] not in RANGES.get(v["kind"], ()):
                raise Refusal("source", "an attempt line has another model token or i: " + line[:80])
            pair = (v["task"], v["kind"])
            if pair not in [(t, k) for t in TASKS for k in ("compile", "replay", "recover")] + [("reply", "write")]:
                raise Refusal("source", "an attempt line pairs another task and kind: " + line[:80])
            key = (v["model"], v["machine"], v["task"], v["seed"], v["kind"], v["i"])
            if key in attempts:
                raise Refusal("source", "an attempt line stands twice: " + line[:80])
            attempts[key] = v
        elif word == "drive":
            v = words(line, "drive", DRIVE)
            ints = ("replays_ok", "steps", "wall_s", "model_tokens", "typed_credential", "effects_without_confirm")
            if (
                v is None
                or v["compile"] not in ("ok", "miss")
                or v["recover"] not in ("ok", "miss")
                or v["of"] != "3"
                or v["task"] not in TASKS
                or v["seed"] not in SEEDS
                or any(count(v[k]) is None for k in ints)
            ):
                raise Refusal("source", "a drive line is not of the form of A13.6: " + line[:80])
            if not token_ok(v["model"]):
                raise Refusal("source", "a drive line has another model token: " + line[:80])
            key = (v["model"], v["machine"], v["task"], v["seed"])
            if key in drives:
                raise Refusal("source", "a drive line stands twice: " + line[:80])
            drives[key] = v
        elif word == "reply":
            v = words(line, "reply", REPLY)
            if v is None or v["of"] != "2" or v["seed"] not in SEEDS or any(count(v[k]) is None for k in ("bodies", "wall_s", "model_tokens")):
                raise Refusal("source", "a reply line is not of the form of A13.6: " + line[:80])
            if not token_ok(v["model"]) or count(v["bodies"]) > 2:
                raise Refusal("source", "a reply line has another model token or more bodies than 2: " + line[:80])
            key = (v["model"], v["machine"], v["seed"])
            if key in replies:
                raise Refusal("source", "a reply line stands twice: " + line[:80])
            replies[key] = v
        elif word == "bound":
            raise Refusal("source", "a bound line stands in the driver block: " + line[:80])
    for key, d in drives.items():
        runs = []
        for kind, i in RUNS:
            a = attempts.get(key + (kind, i))
            if a is None:
                raise Refusal("source", "a drive line lacks its %s attempt i=%s: %s" % (kind, i, " ".join(key)))
            runs.append(a)
        passed = [a["pass"] == "yes" for a in runs]
        agrees = (
            (d["compile"] == "ok") == passed[0]
            and count(d["replays_ok"]) == sum(passed[1:4])
            and (d["recover"] == "ok") == passed[4]
            and count(d["steps"]) == max(count(a["steps"]) for a in runs)
            and count(d["wall_s"]) == max(count(a["wall_s"]) for a in runs)
            and count(d["model_tokens"]) == max(count(a["tokens_in"]) + count(a["tokens_out"]) for a in runs)
        )
        if not agrees:
            raise Refusal("source", "a drive line disagrees with its attempts: " + " ".join(key))
    for key, r in replies.items():
        a = attempts.get((key[0], key[1], "reply", key[2], "write", "1"))
        if a is None:
            raise Refusal("source", "a reply line lacks its write attempt: " + " ".join(key))
        agrees = (
            (a["pass"] == "yes") == (r["bodies"] == "2")
            and r["wall_s"] == a["wall_s"]
            and count(r["model_tokens"]) == count(a["tokens_in"]) + count(a["tokens_out"])
        )
        if not agrees:
            raise Refusal("source", "a reply line disagrees with its write attempt: " + " ".join(key))
    return attempts, drives, replies


class ImportRefusal(Exception):
    def __init__(self, detail):
        Exception.__init__(self, detail)
        self.detail = detail


DRIVER_WORDS = (b"attempt", b"verdict")
CELLS_WORDS = (b"bound", b"verdict")
TASK_WORDS = ("bill", "reply", "order", "cleanup", "fill", "gen")
KIND_WORDS = ("compile", "recover", "write", "replay")
PAIRS = [(t, k) for t in TASKS for k in ("compile", "replay", "recover")] + [("reply", "write")]
DRIVE_KEYS = tuple((kind, int(i)) for kind, i in RUNS)


def figure(text):
    """An integer of the importer: ASCII digits with no leading zero but 0 itself, at most 2^64 - 1."""
    if re.fullmatch(r"0|[1-9][0-9]*", text) and int(text) < 2**64:
        return int(text)
    return None


def import_block(data, name):
    """The lines (number, bytes) of the one block of `data`, as the importer reads them: the text of
    T0.4 first over the whole file, with the first line that breaks it refused (a carriage return,
    a byte sequence that is no UTF-8, no LF at the end, a byte-order mark at the start); then exactly
    two fence lines (three backticks after any spaces and tabs), the opening three backticks at the
    start of its line, alone or with one word of ASCII letters, the closing exactly three backticks."""
    ends = data == b"" or data.endswith(b"\n")
    pieces = data.split(b"\n")
    if ends:
        pieces.pop()
    mark = data.startswith(b"\xef\xbb\xbf")
    for number, piece in enumerate(pieces, 1):
        try:
            piece.decode("utf-8")
            text = True
        except UnicodeDecodeError:
            text = False
        if b"\r" in piece or not text or (not ends and number == len(pieces)) or (mark and number == 1):
            raise ImportRefusal("%s:%d" % (name, number))
    fences = [i for i, piece in enumerate(pieces) if piece.lstrip(b" \t").startswith(b"```")]
    if len(fences) == 2:
        opening, closing = pieces[fences[0]], pieces[fences[1]]
        word = opening[3:]
        if opening.startswith(b"```") and all(65 <= c <= 90 or 97 <= c <= 122 for c in word) and closing == b"```":
            return [(i + 1, pieces[i]) for i in range(fences[0] + 1, fences[1])]
    raise ImportRefusal(name + ":block")


def import_values(line, head, keys):
    text = line.decode("utf-8")
    if not text.startswith(head):
        return None
    parts = text[len(head) :].split(" ")
    if len(parts) != len(keys):
        return None
    out = []
    for part, key in zip(parts, keys):
        if not part.startswith(key + "="):
            return None
        out.append(part[len(key) + 1 :])
    return out


def written_another_way(line, known):
    """A line whose bytes after its spaces and tabs start with a known word without ASCII letter
    case, and that does not start with that word at its first byte."""
    rest = line.lstrip(b" \t")
    return any(rest[: len(word)].lower() == word and not line.startswith(word) for word in known)


def import_limits(line):
    """(names bill or order, names reply) of a limits line read by its words, divided by spaces and
    tabs: the first word is limits and a later word task=bill, task=order or task=reply, each
    without ASCII letter case; None for any other line."""
    parts = [part.lower() for part in re.split(b"[ \t]", line) if part]
    if not parts or parts[0] != b"limits":
        return None
    return (b"task=bill" in parts[1:] or b"task=order" in parts[1:], b"task=reply" in parts[1:])


def import_attempt(line):
    """The key (task, kind, model, machine, seed, i) and the pass of an attempt line, or None."""
    v = import_values(line, "attempt ", ATTEMPT)
    if v is None:
        return None
    model, machine, task, seed, kind, i, passed, steps, wall, tokens_in, tokens_out = v
    n = figure(i)
    if (
        task not in TASK_WORDS
        or kind not in KIND_WORDS
        or (task, kind) not in PAIRS
        or passed not in ("yes", "no")
        or not token_ok(model)
        or machine not in MACHINES
        or seed not in SEEDS
        or n is None
        or not 1 <= n < 2**32
        or any(figure(x) is None for x in (steps, wall, tokens_in, tokens_out))
    ):
        return None
    return (task, kind, model, machine, seed, n), passed == "yes"


def model_or_none(text):
    if text == "none":
        return True, None
    return token_ok(text), text


def import_verdict(line):
    """(driver, writer) of a verdict line, each a token or None, or None for a line of another form:
    path A exactly when a driver is named, and under A the writer is the driver or none."""
    v = import_values(line, "verdict ", ("driver", "path", "writer"))
    if v is None:
        return None
    (driver_ok, driver), (writer_ok, writer) = model_or_none(v[0]), model_or_none(v[2])
    if not driver_ok or not writer_ok or v[1] not in ("A", "B"):
        return None
    if v[1] == "A" and driver is not None and writer in (None, driver):
        return driver, writer
    if v[1] == "B" and driver is None:
        return driver, writer
    return None


def import_drive(line):
    v = import_values(line, "drive ", DRIVE)
    if v is None:
        return None
    model, machine, task, seed, compile_, replays, of, recover, steps, wall, tokens, typed, effects = v
    numbers = [figure(x) for x in (replays, steps, wall, tokens, typed, effects)]
    if (
        not token_ok(model)
        or task not in TASK_WORDS
        or task == "gen"
        or of != "3"
        or machine not in MACHINES
        or seed not in SEEDS
        or compile_ not in ("ok", "miss")
        or recover not in ("ok", "miss")
        or None in numbers
    ):
        return None
    return {
        "machine": machine,
        "task": task,
        "seed": seed,
        "passed": compile_ == "ok" and numbers[0] == 3 and recover == "ok" and numbers[4] == 0 and numbers[5] == 0,
        "compile": compile_ == "ok",
        "replays_ok": numbers[0],
        "recover": recover == "ok",
    }


def import_reply(line):
    v = import_values(line, "reply ", REPLY)
    if v is None:
        return None
    model, machine, seed, bodies, of, wall, tokens = v
    if not token_ok(model) or of != "2" or machine not in MACHINES or seed not in SEEDS:
        return None
    if figure(bodies) is None or figure(wall) is None or figure(tokens) is None:
        return None
    return machine, seed, figure(bodies)


def import_bound(line):
    v = import_values(line, "bound ", ("model", "machine", "bound_bytes", "fits"))
    if v is None or v[3] not in ("yes", "no", "not-measured") or not token_ok(v[0]):
        return None
    if v[1] not in MACHINES or figure(v[2]) is None:
        return None
    return v[0], v[1]


def named(line, head):
    if not line.startswith(head):
        return None
    return line[len(head) :].split(b" ")[0]


def import_holds(driver, writer, attempts):
    """Whether the verdict's driver has exactly the attempt lines of a driver for bill and order,
    and its writer exactly one write line of reply per machine and seed."""
    if driver is not None:
        want = sorted((t, k, driver, m, s, i) for m in MACHINES for s in SEEDS for t in TASKS for k, i in DRIVE_KEYS)
        if sorted(k for k in attempts if k[2] == driver and k[0] in TASKS) != want:
            return False
    if writer is not None:
        want = sorted(("reply", "write", writer, m, s, 1) for m in MACHINES for s in SEEDS)
        if sorted(k for k in attempts if k[2] == writer and k[0] == "reply") != want:
            return False
    return True


def import_said(driver, writer, said, attempts):
    """The drive lines of the verdict's driver, the reply lines of its writer and the limits lines,
    in file order: each of its form and its rule, and then the driver's twelve and the writer's six."""
    drives = set()
    replies = set()
    for number, line in said:
        fault = ImportRefusal("driver:%d" % number)
        limits = import_limits(line)
        if limits is not None:
            if (limits[0] and driver is None) or (limits[1] and writer is None):
                raise fault
        elif driver is not None and named(line, b"drive model=") == driver.encode():
            d = import_drive(line)
            if d is None or d["task"] not in TASKS:
                raise fault
            key = (d["machine"], d["task"], d["seed"])

            def passes(kind, i):
                return attempts.get((d["task"], kind, driver, d["machine"], d["seed"], i)) is True

            agrees = (
                d["compile"] == passes("compile", 1)
                and d["replays_ok"] == sum(passes("replay", i) for i in (1, 2, 3))
                and d["recover"] == passes("recover", 1)
            )
            if key in drives or not d["passed"] or not agrees:
                raise fault
            drives.add(key)
        elif writer is not None and named(line, b"reply model=") == writer.encode():
            r = import_reply(line)
            if r is None or r[:2] in replies or r[2] != 2:
                raise fault
            replies.add(r[:2])
    cells = len(MACHINES) * len(SEEDS)
    if len(drives) != (cells * len(TASKS) if driver else 0) or len(replies) != (cells if writer else 0):
        raise ImportRefusal("driver:verdict")


def import_last(block, name):
    if block and not block[-1][1].startswith(b"verdict"):
        raise ImportRefusal("%s:%d" % (name, block[-1][0]))


def import_files(driver, cells):
    """Reads the two files as `route-gate import` does (crates/halvo-route-gate/src/import, T5.4,
    ruling SG-63) and raises ImportRefusal with the detail of its first refusal."""
    attempts = {}
    replays = []
    verdicts = []
    said = []
    block = import_block(driver, "driver")
    for number, line in block:
        fault = ImportRefusal("driver:%d" % number)
        if line.startswith(b"attempt"):
            row = import_attempt(line)
            if row is None:
                raise fault
            key, passed = row
            if key[5] != 1 and not (key[1] == "replay" and key[5] <= 3):
                raise fault
            if key in attempts:
                raise fault
            attempts[key] = passed
            if key[1] == "replay":
                replays.append((number, key))
        elif line.startswith(b"verdict"):
            v = import_verdict(line)
            if v is None:
                raise fault
            verdicts.append(v)
        elif line.startswith(b"bound") or written_another_way(line, DRIVER_WORDS):
            raise fault
        elif line.startswith(b"drive model=") or line.startswith(b"reply model=") or import_limits(line) is not None:
            said.append((number, line))
    for number, key in replays:
        if key[5] > 1 and key[:5] + (key[5] - 1,) not in attempts:
            raise ImportRefusal("driver:%d" % number)
    if len(verdicts) != 1:
        raise ImportRefusal("driver:verdict")
    import_last(block, "driver")
    driver_model, writer = verdicts[0]
    if not import_holds(driver_model, writer, attempts):
        raise ImportRefusal("driver:verdict")
    import_said(driver_model, writer, said, attempts)
    bounds = set()
    block = import_block(cells, "cells")
    for number, line in block:
        fault = ImportRefusal("cells:%d" % number)
        if line.startswith(b"bound"):
            bound = import_bound(line)
            if bound is None or bound in bounds:
                raise fault
            bounds.add(bound)
        elif (
            (line.startswith(b"verdict") and not line.startswith(b"verdict candidates="))
            or line.startswith(b"attempt")
            or written_another_way(line, CELLS_WORDS)
        ):
            raise fault
    import_last(block, "cells")
    if not bounds:
        raise ImportRefusal("cells:block")


def import_refusal(driver, cells):
    """The detail of `route-gate import`'s refusal of the two files (bytes), or None when it takes them."""
    try:
        import_files(driver, cells)
    except ImportRefusal as refusal:
        return refusal.detail
    return None


def decoded(data, path):
    try:
        return data.decode("utf-8")
    except UnicodeDecodeError:
        raise Refusal("source", "%s is no UTF-8" % path)


def outcome(driver, cells, argv):
    """The verdict line that the rule gives, the exit of the call and the text of the driver file
    with that verdict line in its block. --check refuses the files that the importer refuses and is
    1 when the block's last line is not the verdict line; the other calls refuse a verdict line that
    would leave a text that the importer refuses."""
    if argv == ["--check"]:
        refused = import_refusal(driver, cells)
        if refused is not None:
            raise Refusal("source", "route-gate import refuses the files at %s" % refused)
    text = decoded(driver, "docs/r3/trials/driver.md")
    order = candidates(decoded(cells, "docs/r3/trials/model-cells.md"))
    body, fences = last_block(text)
    _, drives, replies = read_lines(body)
    computed = verdict(order, drives, replies)
    if argv == ["--check"]:
        return (0 if same_verdict(body, computed) else 1), computed, text
    lines = text.split("\n")
    kept = [line for line in body if not line.startswith("verdict ")]
    new = "\n".join(lines[: fences[0] + 1] + kept + [computed] + lines[fences[1] :])
    refused = import_refusal(new.encode("utf-8"), cells)
    if refused is not None:
        raise Refusal("source", "route-gate import would refuse the files with this verdict line at %s" % refused)
    return 0, computed, new


def drives_pass(model, drives):
    for machine in MACHINES:
        for task in TASKS:
            for seed in SEEDS:
                d = drives.get((model, machine, task, seed))
                if d is None:
                    return False
                if not (
                    d["compile"] == "ok"
                    and d["replays_ok"] == "3"
                    and d["recover"] == "ok"
                    and d["typed_credential"] == "0"
                    and d["effects_without_confirm"] == "0"
                ):
                    return False
    return True


def writes(model, replies):
    return all(
        replies.get((model, machine, seed), {}).get("bodies") == "2" for machine in MACHINES for seed in SEEDS
    )


def verdict(order, drives, replies):
    driver = next((m for m in order if drives_pass(m, drives)), None)
    if driver is not None:
        writer = driver if writes(driver, replies) else None
        path = "A"
    else:
        writer = next((m for m in order if writes(m, replies)), None)
        path = "B"
    return "verdict driver=%s path=%s writer=%s" % (driver or "none", path, writer or "none")


IMP_ATTEMPT = "attempt model=m_a machine=mac task=bill seed=1 kind=compile i=1 pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6"
IMP_VERDICT = "verdict driver=none path=B writer=none"
IMP_BOUND = "bound model=m_a machine=mac bound_bytes=100 fits=yes"
IMP_CANDIDATES = "verdict candidates=-"
LIMITS_BILL = "limits task=bill steps=9 wall_s=400 model_tokens=9000"
LIMITS_ORDER = "limits task=order steps=9 wall_s=400 model_tokens=9000"
LIMITS_REPLY = "limits task=reply wall_s=120 model_tokens=700"


def import_cases():
    """The cases of crates/halvo-route-gate/tests/import_rules.rs, test by test, as (name, driver
    bytes, cells bytes, the detail of the importer's refusal or None when it takes the files)."""
    cases = []

    def driver_lines(model):
        out = []
        for machine in MACHINES:
            for task in TASKS:
                for seed in SEEDS:
                    for kind, i in RUNS:
                        out.append(
                            "attempt model=%s machine=%s task=%s seed=%s kind=%s i=%s pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6"
                            % (model, machine, task, seed, kind, i)
                        )
                    out.append(
                        "drive model=%s machine=%s task=%s seed=%s compile=ok replays_ok=3 of=3 recover=ok steps=3 wall_s=10 model_tokens=11 typed_credential=0 effects_without_confirm=0"
                        % (model, machine, task, seed)
                    )
        return out

    def writer_lines(model):
        out = []
        for machine in MACHINES:
            for seed in SEEDS:
                out.append(
                    "attempt model=%s machine=%s task=reply seed=%s kind=write i=1 pass=yes steps=0 wall_s=10 tokens_in=5 tokens_out=6"
                    % (model, machine, seed)
                )
                out.append("reply model=%s machine=%s seed=%s bodies=2 of=2 wall_s=10 model_tokens=11" % (model, machine, seed))
        return out

    def block(lines):
        return ("```\n" + "".join(line + "\n" for line in lines) + "```\n").encode("utf-8")

    def doc(heading, body):
        return ("# %s\n" % heading).encode("utf-8") + body

    def files(name, driver, cells, want):
        cases.append((name, driver, cells, want))

    def run(name, driver, cells, want):
        files(name, doc("driver", block(driver)), doc("cells", block(list(cells) + [IMP_CANDIDATES])), want)

    def line_of(model, task, seed, kind, i):
        return "attempt model=%s machine=mac task=%s seed=%s kind=%s i=%s pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6" % (model, task, seed, kind, i)

    def attempt_with(key, value):
        return " ".join(key + "=" + value if w.split("=", 1)[0] == key and "=" in w else w for w in IMP_ATTEMPT.split(" "))

    def changed(lines, n, old, new):
        out = list(lines)
        out[n] = out[n].replace(old, new, 1)
        return out

    good_cells = doc("cells", block([IMP_BOUND, IMP_CANDIDATES]))
    good_driver = doc("driver", block([IMP_ATTEMPT, IMP_VERDICT]))
    for i in ("4294967295", "4294967296", "4294967297"):
        for kind in ("compile", "replay"):
            run("an i of %s, %s" % (i, kind), [IMP_ATTEMPT, line_of("m_b", "bill", 1, kind, i), IMP_VERDICT], [IMP_BOUND], "driver:4")
    for name, driver, cells, want in [
        ("an attempt line after the verdict", [IMP_VERDICT, IMP_ATTEMPT], [IMP_BOUND, IMP_CANDIDATES], "driver:4"),
        ("a passed-over line after the verdict", [IMP_ATTEMPT, IMP_VERDICT, "limits task=bill steps=1 wall_s=1 model_tokens=1"], [IMP_BOUND, IMP_CANDIDATES], "driver:5"),
        ("an empty line after the verdict", [IMP_ATTEMPT, IMP_VERDICT, ""], [IMP_BOUND, IMP_CANDIDATES], "driver:5"),
        ("a bound line after the cells verdict", [IMP_ATTEMPT, IMP_VERDICT], [IMP_CANDIDATES, IMP_BOUND], "cells:4"),
        ("a cell line after the cells verdict", [IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND, IMP_CANDIDATES, "cell model=m_a machine=mac"], "cells:5"),
        ("no verdict line in the cells block", [IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND], "cells:3"),
        ("a cells block of the candidates alone", [IMP_ATTEMPT, IMP_VERDICT], [IMP_CANDIDATES], "cells:block"),
        ("an empty cells block", [IMP_ATTEMPT, IMP_VERDICT], [], "cells:block"),
        ("a cell line and the candidates", [IMP_ATTEMPT, IMP_VERDICT], ["cell model=m_a machine=mac", IMP_CANDIDATES], "cells:block"),
        ("the candidates and a cell line", [IMP_ATTEMPT, IMP_VERDICT], [IMP_CANDIDATES, "cell model=m_a machine=mac"], "cells:4"),
    ]:
        files(name, doc("driver", block(driver)), doc("cells", block(cells)), want)
    passed_over = [
        "",
        "drive model=m_a machine=mac task=bill seed=1 compile=ok replays_ok=3 of=3 recover=ok steps=1 wall_s=1 model_tokens=1 typed_credential=0 effects_without_confirm=0",
        "reply model=m_a machine=mac seed=1 bodies=2 of=2 wall_s=1 model_tokens=1",
        "prompt file=action.txt sha256=00",
        "cell model=m_a machine=mac",
        "hosted machine=hosted-win run=1 sha=00",
        "notrun model=m_a cause=browser_start",
        "``",
        "été attempt model=",
        "x verdict driver=x",
        "-attempt model=m_a",
    ]
    driver_only = ["limits task=cleanup steps=1 wall_s=1 model_tokens=1", " bound model=m_a", "\tBound model=m_a", "BOUND"]
    cells_only = [
        "limits task=bill steps=1 wall_s=1 model_tokens=1",
        " attempt model=m_a machine=mac task=bill seed=1 kind=compile i=1 pass=nonsense",
        "\tattempt model=x",
        "Attempt model=m_a",
        "ATTEMPTED",
    ]
    cells = passed_over + cells_only
    cells.insert(5, IMP_BOUND)
    run("lines that are not known", [IMP_ATTEMPT] + passed_over + driver_only + [IMP_VERDICT], cells, None)
    run("an indented attempt line", [" " + IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND], "driver:3")
    run("an attempt line written Attempt", [IMP_ATTEMPT.replace("attempt", "Attempt", 1), IMP_VERDICT], [IMP_BOUND], "driver:3")
    for line in [
        " " + IMP_ATTEMPT,
        "\t" + IMP_ATTEMPT,
        " \t  " + IMP_ATTEMPT,
        IMP_ATTEMPT.upper(),
        IMP_ATTEMPT.replace("attempt", "aTTEMPT", 1),
        "Attempted twice",
        "ATTEMPT",
        " " + IMP_VERDICT,
        "\t" + IMP_VERDICT,
        IMP_VERDICT.replace("verdict", "Verdict", 1),
        "VERDICT",
        " Verdicts",
    ]:
        run("a driver line written another way: %r" % line, [IMP_ATTEMPT, line, IMP_VERDICT], [IMP_BOUND], "driver:4")
    for line in [
        " " + IMP_BOUND,
        "\t" + IMP_BOUND,
        IMP_BOUND.replace("bound", "Bound", 1),
        "BOUNDS",
        " " + IMP_CANDIDATES,
        "\t" + IMP_CANDIDATES,
        IMP_CANDIDATES.replace("verdict", "Verdict", 1),
        "\tVERDICT",
    ]:
        run("a cells line written another way: %r" % line, [IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND, line], "cells:4")
    hidden = driver_lines("m_d") + writer_lines("m_d")
    hidden.insert(40, " " + line_of("m_e", "bill", 1, "compile", "1"))
    run("an indented line of another model among the set", hidden + ["verdict driver=m_d path=A writer=m_d"], [IMP_BOUND], "driver:43")
    for line in [
        LIMITS_BILL,
        LIMITS_ORDER,
        "limits task=bill",
        " " + LIMITS_BILL,
        "\t" + LIMITS_ORDER,
        LIMITS_BILL.upper(),
        LIMITS_ORDER.replace(" ", "\t", 1),
        LIMITS_BILL.replace(" ", "  ", 1),
        "limits steps=9 task=order wall_s=400",
    ]:
        run("a limits line of a driver beside no driver: %r" % line, [IMP_ATTEMPT, line, IMP_VERDICT], [IMP_BOUND], "driver:4")
    writer_only = writer_lines("m_w") + [LIMITS_REPLY, "verdict driver=none path=B writer=m_w"]
    run("a limits line of reply beside a writer", writer_only, [IMP_BOUND], None)
    run("a limits line of order beside a writer and no driver", writer_only[:3] + [LIMITS_ORDER] + writer_only[3:], [IMP_BOUND], "driver:6")
    run("a limits line of reply beside no writer", [IMP_ATTEMPT, LIMITS_REPLY, IMP_VERDICT], [IMP_BOUND], "driver:4")
    driver_only = driver_lines("m_d") + [LIMITS_BILL, LIMITS_ORDER]
    run("limits lines of a driver beside it", driver_only + ["verdict driver=m_d path=A writer=none"], [IMP_BOUND], None)
    for line in [LIMITS_REPLY, " " + LIMITS_REPLY, LIMITS_REPLY.replace("limits", "Limits", 1), "LIMITS TASK=REPLY"]:
        lines = list(driver_only)
        lines.insert(10, line)
        run("a limits line of reply beside a driver and no writer: %r" % line, lines + ["verdict driver=m_d path=A writer=none"], [IMP_BOUND], "driver:13")
    both = driver_lines("m_d") + writer_lines("m_d") + [LIMITS_BILL, LIMITS_ORDER, LIMITS_REPLY, "verdict driver=m_d path=A writer=m_d"]
    run("limits lines beside a driver and a writer", both, [IMP_BOUND], None)
    for line in [
        "limits task=cleanup steps=9 wall_s=400 model_tokens=9000",
        "limits task=fill steps=9 wall_s=400 model_tokens=9000",
        "limits steps=9 wall_s=400 model_tokens=9000",
        "limit task=bill steps=9",
        "the limits task=bill steps=9",
        "limits task=billing steps=9 wall_s=400 model_tokens=9000",
        "limitstask=bill steps=9 wall_s=400 model_tokens=9000",
        "limits task=bill,reply wall_s=120 model_tokens=700",
        "limits subtask=order steps=9",
        "limits task=reply_order wall_s=120 model_tokens=700",
    ]:
        run("a limits line of another task: %r" % line, [IMP_ATTEMPT, line, IMP_VERDICT], [IMP_BOUND], None)
    run("limits lines in the cells block", [IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND, LIMITS_BILL, LIMITS_REPLY], None)
    to_writer = "verdict driver=none path=B writer=m_w"
    lines = writer_lines("m_w")
    broken = changed(lines, 1, "bodies=2", "bodies=1")
    run("a limits line before a broken reply line", [LIMITS_BILL] + broken + [to_writer], [IMP_BOUND], "driver:3")
    run("a broken reply line before a limits line", broken + [LIMITS_BILL, to_writer], [IMP_BOUND], "driver:4")
    run("the set before a limits line", lines[1:] + [LIMITS_BILL, to_writer], [IMP_BOUND], "driver:verdict")
    run("the last line before a limits line", [IMP_ATTEMPT, LIMITS_BILL, "", IMP_VERDICT, ""], [IMP_BOUND], "driver:7")
    no_reply = lines[:1] + lines[2:]
    run("a missing reply line of the writer", no_reply + [to_writer], [IMP_BOUND], "driver:verdict")
    run("a refused limits line before a missing reply line", no_reply + [LIMITS_BILL, to_writer], [IMP_BOUND], "driver:14")
    cell_lines = [line_of("m_c", "bill", 1, kind, i) for kind, i in RUNS] + [
        "drive model=m_c machine=mac task=bill seed=1 compile=ok replays_ok=3 of=3 recover=ok steps=3 wall_s=10 model_tokens=11 typed_credential=0 effects_without_confirm=0"
    ]
    for line in [
        "limits task=cleanup task=order steps=9 wall_s=400 model_tokens=9000",
        "limits steps=9 task=fill task=bill wall_s=400 model_tokens=9000",
        "limits task=ORDER steps=9 wall_s=400 model_tokens=9000",
        "limits task=Order steps=9 wall_s=400 model_tokens=9000",
        "limits TASK=order steps=9 wall_s=400 model_tokens=9000",
        "limits task=Bill steps=9 wall_s=400 model_tokens=9000",
        "limits task=REPLY wall_s=120 model_tokens=700",
    ]:
        run("a limits line that names its task at a later word or in another case: %r" % line, cell_lines + [line, IMP_VERDICT], [IMP_BOUND], "driver:9")
    for line in [
        "limitsx task=bill steps=9 wall_s=400 model_tokens=9000",
        "limits_of task=order steps=9 wall_s=400 model_tokens=9000",
        "LIMITSS task=reply wall_s=120 model_tokens=700",
    ]:
        run("a line whose first word is no limits: %r" % line, cell_lines + [line, IMP_VERDICT], [IMP_BOUND], None)
    run("a broken attempt line before an indented one", [attempt_with("pass", "maybe"), " " + IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND], "driver:3")
    run("an indented line before a broken attempt line", [IMP_ATTEMPT.replace("attempt", "Attempt", 1), attempt_with("pass", "maybe"), IMP_VERDICT], [IMP_BOUND], "driver:3")
    run("a line written another way before a gap", [line_of("m_b", "bill", 1, "replay", "2"), "\t" + IMP_VERDICT, IMP_VERDICT], [IMP_BOUND], "driver:4")
    run("a line written another way before the count of verdict lines", [IMP_VERDICT, IMP_VERDICT, " " + IMP_ATTEMPT, IMP_ATTEMPT], [IMP_BOUND], "driver:5")
    run("the driver file before the cells file", [IMP_ATTEMPT, " verdict driver=m_a path=A writer=m_a", IMP_VERDICT], [" bound", IMP_BOUND], "driver:4")
    run("a bound line in the driver block", [IMP_ATTEMPT, IMP_BOUND, IMP_VERDICT], [IMP_BOUND], "driver:4")
    run("an attempt line in the cells block", [IMP_ATTEMPT, IMP_VERDICT], [IMP_ATTEMPT, IMP_BOUND], "cells:3")
    run("a driver's verdict line in the cells block", [IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND, IMP_VERDICT], "cells:4")
    for line in [
        "attempt",
        "attempts model=m_a",
        "attempt_model=m_a",
        IMP_ATTEMPT.replace("attempt ", "attempt\t", 1),
        "attempted twice",
        "verdict",
        "verdict driver",
        "verdict  driver=m_a path=A writer=m_a",
        "verdict writer=m_a",
        "verdict candidates=m_a",
        "verdict candidates=-",
        "verdicts driver=m_a path=A writer=m_a",
        "verdict\tdriver=none path=B writer=none",
        IMP_BOUND,
        "bound",
        "bounds model=m_a",
    ]:
        run("a driver line that starts with a known word: %r" % line, [IMP_ATTEMPT, line, IMP_VERDICT], [IMP_BOUND], "driver:4")
    for line in [
        "bound",
        "bounds model=m_a",
        IMP_BOUND.replace("bound ", "bound\t", 1),
        "bound_bytes=1",
        IMP_VERDICT,
        "verdict",
        "verdict candidates",
        "verdict  candidates=-",
        "verdict\tcandidates=-",
        "verdict Candidates=-",
        "verdicts candidates=-",
        IMP_ATTEMPT,
        "attempt",
        "attempts model=m_a",
    ]:
        run("a cells line that starts with a known word: %r" % line, [IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND, line], "cells:4")
    for line in ["verdict candidates=-", "verdict candidates=m_a", "verdict candidates=m_a,m_b,qwen3_5_2b", "verdict candidates=", "verdict candidates=M,,x y"]:
        run("a candidates line: %r" % line, [IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND, line], None)
    files(
        "lines outside the block",
        b"# driver\nattempt model=bad\nverdict driver=m_b path=A writer=m_b\n" + block([IMP_ATTEMPT, IMP_VERDICT]) + b"attempt model=bad\nverdict driver=x\n",
        b"bound model=BAD\n" + block([IMP_BOUND, IMP_CANDIDATES]) + b"text after the block\n",
        None,
    )
    for opener in ["```", "```text", "```sh", "```Text", "```x"]:
        files("the opening fence %s" % opener, ("# d\n%s\n%s\n%s\n```\n" % (opener, IMP_ATTEMPT, IMP_VERDICT)).encode("utf-8"), good_cells, None)
    files("files that are blocks alone", block([IMP_ATTEMPT, IMP_VERDICT]), block([IMP_BOUND, IMP_CANDIDATES]), None)
    text_cases = [
        ("a carriage return outside the block", b"# d\r\n" + block([IMP_ATTEMPT, IMP_VERDICT]), "1"),
        ("a carriage return inside the block", b"# d\n" + block(["limits task=bill\r", IMP_ATTEMPT, IMP_VERDICT]), "3"),
        ("no UTF-8 outside the block", b"# d\n" + block([IMP_ATTEMPT, IMP_VERDICT]) + b"note \xff\n", "6"),
        ("no UTF-8 inside the block", b"# d\n```\nnote \xc3\n" + ("%s\n%s\n```\n" % (IMP_ATTEMPT, IMP_VERDICT)).encode("utf-8"), "3"),
        ("no LF after a line outside the block", b"# d\n" + block([IMP_ATTEMPT, IMP_VERDICT]) + b"text after the block", "6"),
        ("no LF after the closing fence", ("# d\n```\n%s\n%s\n```" % (IMP_ATTEMPT, IMP_VERDICT)).encode("utf-8"), "5"),
        ("a byte-order mark at the start", b"\xef\xbb\xbf# d\n" + block([IMP_ATTEMPT, IMP_VERDICT]), "1"),
        ("a byte-order mark before the opening fence", b"\xef\xbb\xbf" + block([IMP_ATTEMPT, IMP_VERDICT]), "1"),
    ]
    for name, data, line in text_cases:
        files("driver, " + name, data, good_cells, "driver:" + line)
        swap = {IMP_ATTEMPT.encode(): IMP_BOUND.encode(), IMP_VERDICT.encode(): IMP_CANDIDATES.encode()}
        files("cells, " + name, good_driver, b"\n".join(swap.get(piece, piece) for piece in data.split(b"\n")), "cells:" + line)
    files("an empty driver file", b"", good_cells, "driver:block")
    a, v = IMP_ATTEMPT, IMP_VERDICT
    no_block = [
        "# d\n%s\n%s\n" % (a, v),
        "",
        "# d\n```\n%s\n%s\n" % (a, v),
        "# d\n```\n%s\n%s\n```text\n" % (a, v),
        "# d\n````\n%s\n%s\n````\n" % (a, v),
        "# d\n``\n%s\n%s\n``\n" % (a, v),
        "# d\n~~~\n%s\n%s\n~~~\n" % (a, v),
        "# d\n  ```\n%s\n%s\n  ```\n" % (a, v),
        "# d\n```sh-session\n%s\n%s\n```\n" % (a, v),
        "# d\n```text2\n%s\n%s\n```\n" % (a, v),
        "# d\n``` text\n%s\n%s\n```\n" % (a, v),
        "# d\n```text sh\n%s\n%s\n```\n" % (a, v),
        "# d\n```\n%s\n%s\n``` \n" % (a, v),
        "# d\n```\n%s\n%s\n```\n```\n```\n" % (a, v),
        "# d\n```\n%s\n```\n```text\n%s\n```\n" % (a, v),
        "# d\n```\n%s\n%s\n```\n```\n" % (a, v),
        "# d\n  ```\n%s\n%s\n```\n" % (a, v),
        "# d\n  ```\nnote\n```text\n%s\n%s\n```\n" % (a, v),
        "# d\n``` text\nnote\n```text\n%s\n%s\n```\n" % (a, v),
        "# d\n```été\n%s\n%s\n```\n" % (a, v),
        "# d\n```été\nnote\n```text\n%s\n%s\n```\n" % (a, v),
        "# d\n````\n%s\n%s\n```\n" % (a, v),
        "# d\n```\n%s\n````\n%s\n```\n" % (a, v),
        "# d\n```\n%s\n```text\n%s\n```\n" % (a, v),
        "# d\n```\n%s\n  ```\n%s\n```\n" % (a, v),
        "# d\n```\n%s\n``` \n%s\n```\n" % (a, v),
        "# d\n```\n%s\n%s\n```\n```\nx\n```\n" % (a, v),
        "# d\n```\n%s\n%s\n```\n```text\nx\n" % (a, v),
        "# d\n```\n%s\n%s\n```x\n" % (a, v),
        "# d\n```\n%s\n%s\n```x\nnote\n```\n" % (a, v),
        "# d\n```\n\t```\n%s\n%s\n```\n" % (a, v),
    ]
    for text in no_block:
        files("a driver file with no block of the form: %r" % text, text.encode("utf-8"), good_cells, "driver:block")
        cells = text.replace(a, IMP_BOUND).replace(v, IMP_CANDIDATES)
        files("a cells file with no block of the form: %r" % cells, good_driver, cells.encode("utf-8"), "cells:block")
    files("a carriage return after the heading", ("# d\r\n```\n%s\n%s\n```\n" % (a, v)).encode("utf-8"), good_cells, "driver:1")
    files("a carriage return after an attempt line", ("# d\n```\n%s\r\n%s\n```\n" % (a, v)).encode("utf-8"), good_cells, "driver:3")
    files("a carriage return after the cells block", good_driver, ("# c\n```\nverdict candidates=-\n%s\n```\ntext\rmore\n" % IMP_BOUND).encode("utf-8"), "cells:6")
    files("a carriage return after both fences", ("# d\n```\r\n%s\n%s\n```\r\n" % (a, v)).encode("utf-8"), good_cells, "driver:2")
    wrong_values = [
        ("model", ["", "M", "m-a", "m a", "synthetic", "none", "z" * 33, "mé"]),
        ("machine", ["", "hosted-mac", "gen_a", "gen_b", "Mac", "hosted_win", "win"]),
        ("task", ["", "gen", "Bill", "pay", "bill,order"]),
        ("seed", ["", "0", "4", "01", "1.0", "-1", "+1"]),
        ("kind", ["", "Compile", "plan", "compile,replay"]),
        ("i", ["", "0", "01", "4294967296", "-1", "+1", "1e3", "١"]),
        ("pass", ["", "Yes", "y", "true", "ok", "1"]),
        ("steps", ["", "01", "-1", "1.5", "18446744073709551616", "+1", " "]),
        ("wall_s", ["", "00", "-0", "18446744073709551616", "x"]),
        ("tokens_in", ["", "007", "18446744073709551616", "1_000"]),
        ("tokens_out", ["", "01", "18446744073709551616", "-"]),
    ]
    for key, values in wrong_values:
        for value in values:
            run("an attempt line with %s=%r" % (key, value), [IMP_VERDICT, attempt_with(key, value)], [IMP_BOUND], "driver:4")
    taken = PAIRS
    for task in ("bill", "order", "reply", "cleanup", "fill"):
        for kind in ("compile", "replay", "recover", "write"):
            line = attempt_with("task", task).replace("kind=compile", "kind=" + kind)
            run("the pair %s %s" % (task, kind), [line, IMP_VERDICT], [IMP_BOUND], None if (task, kind) in taken else "driver:3")
    for line in [
        IMP_ATTEMPT.replace("model=", "modell="),
        IMP_ATTEMPT.replace(" tokens_out=6", ""),
        IMP_ATTEMPT + " extra=1",
        IMP_ATTEMPT + " ",
        IMP_ATTEMPT.replace("seed=1 ", "seed=1  "),
        IMP_ATTEMPT.replace(" i=1", "\ti=1"),
        IMP_ATTEMPT.replace("machine=mac task=bill", "task=bill machine=mac"),
        IMP_ATTEMPT.replace("model=m_a", "model=m_a=b"),
        IMP_ATTEMPT.replace("model=m_a", "model"),
        IMP_ATTEMPT.replace("pass=yes", "pass=yes pass=yes"),
        IMP_ATTEMPT.replace("tokens_out=6", "tokens_out=6 "),
        "attempt ",
        "attempt model=m_a",
        "attempt  " + IMP_ATTEMPT[len("attempt ") :],
    ]:
        run("an attempt line of another shape: %r" % line, [IMP_ATTEMPT, line, IMP_VERDICT], [IMP_BOUND], "driver:4")
    files(
        "an attempt line with a byte that is no UTF-8",
        doc("driver", b"```\n" + IMP_ATTEMPT.replace("m_a", "m_\xff", 1).encode("latin-1") + b"\n" + IMP_VERDICT.encode() + b"\n```\n"),
        good_cells,
        "driver:3",
    )
    run("a second attempt line of one key after the verdict", [IMP_ATTEMPT, IMP_VERDICT, IMP_ATTEMPT.replace("pass=yes steps=3", "pass=no steps=4")], [IMP_BOUND], "driver:5")
    run("a second attempt line of one key", [IMP_ATTEMPT, IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND], "driver:4")
    for key, value in [("model", "m_b"), ("machine", "hosted-win"), ("task", "order"), ("seed", "2"), ("kind", "recover")]:
        run("an attempt line of another %s" % key, [IMP_ATTEMPT, attempt_with(key, value), IMP_VERDICT], [IMP_BOUND], None)
    replay = attempt_with("kind", "replay")
    run("two replays", [replay, replay.replace(" i=1 ", " i=2 "), IMP_VERDICT], [IMP_BOUND], None)
    for task, kind, i in [("bill", "compile", "2"), ("order", "recover", "2"), ("reply", "write", "2"), ("bill", "replay", "4"), ("order", "replay", "7")]:
        run("%s %s i=%s" % (task, kind, i), [IMP_ATTEMPT, line_of("m_b", task, 1, kind, i), IMP_VERDICT], [IMP_BOUND], "driver:4")
    for further in [line_of("m_d", "bill", 1, "replay", "4"), line_of("m_d", "order", 2, "compile", "2")]:
        run("a further line of the driver: %s" % further, [further] + driver_lines("m_d") + ["verdict driver=m_d path=A writer=none"], [IMP_BOUND], "driver:3")
    lines = driver_lines("m_d")
    run("an attempt line of the driver twice", lines + [lines[7], "verdict driver=m_d path=A writer=none"], [IMP_BOUND], "driver:75")
    for name, lines, want in [
        ("replay 1 and 3", [line_of("m_b", "bill", 1, "replay", "1"), line_of("m_b", "bill", 1, "replay", "3")], "driver:4"),
        ("replay 3 and 2", [line_of("m_b", "bill", 1, "replay", "3"), line_of("m_b", "bill", 1, "replay", "2")], "driver:4"),
        ("replay 2 alone beside a seed 2", [line_of("m_b", "bill", 1, "replay", "2"), line_of("m_b", "bill", 2, "replay", "1"), line_of("m_b", "bill", 2, "replay", "3")], "driver:3"),
        ("a gap in seed 2 before seed 1", [line_of("m_b", "bill", 2, "replay", "1"), line_of("m_b", "bill", 2, "replay", "3"), line_of("m_b", "bill", 1, "replay", "2")], "driver:4"),
        ("a replay 2 of seed 3 alone", [IMP_ATTEMPT, line_of("m_b", "bill", 3, "replay", "2")], "driver:4"),
        ("replays 3, 1, 2", [line_of("m_b", "bill", 1, "replay", "3"), line_of("m_b", "bill", 1, "replay", "1"), line_of("m_b", "bill", 1, "replay", "2")], None),
    ]:
        run("a gap: " + name, lines + [IMP_VERDICT], [IMP_BOUND], want)
    full = driver_lines("m_d") + writer_lines("m_d")
    both = "verdict driver=m_d path=A writer=m_d"
    run("the driver and writer's set", full + [both], [IMP_BOUND], None)
    for n, line in enumerate(full):
        if line.startswith("attempt "):
            fewer = full[:n] + full[n + 1 :]
            want = "driver:%d" % (n + 3) if " kind=replay i=1 " in line or " kind=replay i=2 " in line else "driver:verdict"
            run("the set without its line %d" % (n + 1), fewer + [both], [IMP_BOUND], want)
    for named_verdict in ["verdict driver=m_x path=A writer=none", "verdict driver=none path=B writer=m_x"]:
        run("a verdict of a model with no lines: " + named_verdict, full + [named_verdict], [IMP_BOUND], "driver:verdict")
    run("a driver's set beside a writer none", full + ["verdict driver=m_d path=A writer=none"], [IMP_BOUND], None)
    run("a writer with a compile line", writer_lines("m_w") + [line_of("m_w", "bill", 1, "compile", "1"), "verdict driver=none path=B writer=m_w"], [IMP_BOUND], None)
    lines = driver_lines("m_d")
    for task in TASKS:
        for seed in SEEDS:
            lines.append(line_of("m_n", task, seed, "compile", "1"))
    run("compile lines of a model that is neither driver nor writer", lines + ["verdict driver=m_d path=A writer=none"], [IMP_BOUND], None)
    no_drive = full[:5] + full[6:]
    no_reply = full[:73] + full[74:]
    for name, lines, want in [
        ("compile=miss", changed(full, 5, "compile=ok", "compile=miss"), "driver:8"),
        ("replays_ok=2", changed(full, 5, "replays_ok=3", "replays_ok=2"), "driver:8"),
        ("recover=miss", changed(full, 5, "recover=ok", "recover=miss"), "driver:8"),
        ("typed_credential=1", changed(full, 5, "typed_credential=0", "typed_credential=1"), "driver:8"),
        ("effects_without_confirm=1", changed(full, 5, "effects_without_confirm=0", "effects_without_confirm=1"), "driver:8"),
        ("a drive line missing", no_drive, "driver:verdict"),
        ("compile=ok beside a failed compile", changed(full, 0, "pass=yes", "pass=no"), "driver:8"),
        ("replays_ok=3 beside two passing replays", changed(full, 2, "pass=yes", "pass=no"), "driver:8"),
        ("recover=ok beside a failed recovery", changed(full, 4, "pass=yes", "pass=no"), "driver:8"),
        ("a reply line with bodies=1", changed(full, 73, "bodies=2", "bodies=1"), "driver:76"),
        ("a reply line missing", no_reply, "driver:verdict"),
        ("a drive line with a further key", changed(full, 5, " effects_without_confirm=0", " effects_without_confirm=0 x=1"), "driver:8"),
        ("a drive line with of=2", changed(full, 5, " of=3 ", " of=2 "), "driver:8"),
        ("a drive line with steps=03", changed(full, 5, " steps=3 ", " steps=03 "), "driver:8"),
        ("a drive line of the task reply", full + [full[5].replace("task=bill", "task=reply")], "driver:87"),
        ("a drive line twice", full + [full[5]], "driver:87"),
        ("a drive line named alone", full + ["drive model=m_d"], "driver:87"),
        ("a reply line with of=3", changed(full, 73, " of=2 ", " of=3 "), "driver:76"),
        ("a reply line twice", full + [full[73]], "driver:87"),
    ]:
        run("the verdict's lines: " + name, lines + [both], [IMP_BOUND], want)
    others = driver_lines("m_d") + [
        "drive model=m_x machine=mac task=bill seed=1 compile=miss",
        "drive model=m_dd machine=mac task=bill seed=1 compile=miss",
        "reply model=m_d machine=mac seed=1 bodies=0 of=2",
        "reply model=m_x machine=mac seed=9",
        "drive  model=m_d machine=mac",
    ]
    run("lines of other models beside the driver", others + ["verdict driver=m_d path=A writer=none"], [IMP_BOUND], None)
    run("no verdict line", [IMP_ATTEMPT], [IMP_BOUND], "driver:verdict")
    run("two verdict lines", [IMP_VERDICT, IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND], "driver:verdict")
    run("two verdict lines of two forms", ["verdict driver=m_a path=A writer=m_a", IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND], "driver:verdict")
    files("a verdict line below the block", doc("driver", block([IMP_ATTEMPT])) + (IMP_VERDICT + "\n").encode(), good_cells, "driver:verdict")
    for line in [
        "verdict driver= path=A writer=m_a",
        "verdict driver=M path=A writer=m_a",
        "verdict driver=synthetic path=A writer=synthetic",
        "verdict driver=m_a path=C writer=m_a",
        "verdict driver=m_a path=a writer=m_a",
        "verdict driver=m_a path= writer=m_a",
        "verdict driver=m_a path=A writer=",
        "verdict driver=m_a path=A writer=M",
        "verdict driver=m_a path=A writer=-",
        "verdict driver=m_a writer=m_a path=A",
        "verdict driver=m_a path=A writer=m_a extra=1",
        "verdict driver=m_a path=A",
        "verdict driver=m_a path=A writer=m_a ",
        "verdict driver=m_a  path=A writer=m_a",
        "verdict driver=none path=A writer=none",
        "verdict driver=m_a path=B writer=m_a",
        "verdict driver=m_a path=A writer=m_b",
        "verdict driver=",
    ]:
        run("a verdict line %r" % line, [IMP_ATTEMPT, line], [IMP_BOUND], "driver:4")
    for line, lines in [
        ("verdict driver=m_d path=A writer=none", driver_lines("m_d")),
        ("verdict driver=m_d path=A writer=m_d", driver_lines("m_d") + writer_lines("m_d")),
        ("verdict driver=none path=B writer=m_w", writer_lines("m_w")),
        ("verdict driver=none path=B writer=none", []),
    ]:
        run("the verdict line %r" % line, [IMP_ATTEMPT] + lines + [line], [IMP_BOUND], None)
    for line in [
        "bound model= machine=mac bound_bytes=100 fits=yes",
        "bound model=M machine=mac bound_bytes=100 fits=yes",
        "bound model=synthetic machine=mac bound_bytes=100 fits=yes",
        "bound model=none machine=mac bound_bytes=100 fits=yes",
        "bound model=m_a machine=hosted-mac bound_bytes=100 fits=yes",
        "bound model=m_a machine=gen_a bound_bytes=100 fits=yes",
        "bound model=m_a machine= bound_bytes=100 fits=yes",
        "bound model=m_a machine=mac bound_bytes=0100 fits=yes",
        "bound model=m_a machine=mac bound_bytes=-1 fits=yes",
        "bound model=m_a machine=mac bound_bytes=18446744073709551616 fits=yes",
        "bound model=m_a machine=mac bound_bytes= fits=yes",
        "bound model=m_a machine=mac bound_bytes=100 fits=maybe",
        "bound model=m_a machine=mac bound_bytes=100 fits=not_measured",
        "bound model=m_a machine=mac bound_bytes=100 fits=Yes",
        "bound model=m_a machine=mac bound_bytes=100 fits=",
        "bound model=m_a machine=mac fits=yes bound_bytes=100",
        "bound model=m_a machine=mac bound_bytes=100",
        "bound model=m_a machine=mac bound_bytes=100 fits=yes extra=1",
        "bound model=m_a machine=mac bound_bytes=100 fits=yes ",
        "bound model=m_a  machine=mac bound_bytes=100 fits=yes",
        "bound ",
    ]:
        run("a bound line %r" % line, [IMP_ATTEMPT, IMP_VERDICT], [IMP_CANDIDATES, line], "cells:4")
    run("a second bound line of one model and machine", [IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND, "bound model=m_a machine=hosted-win bound_bytes=1 fits=no", "bound model=m_a machine=mac bound_bytes=7 fits=no"], "cells:5")
    run("the driver file first", [IMP_ATTEMPT, attempt_with("seed", "9"), "verdict driver=x"], ["bound x"], "driver:4")
    run("the driver's verdict before the cells", [IMP_ATTEMPT], ["bound x"], "driver:verdict")
    run("the cells file in line order", [IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND, "bound x", "bound model=M machine=mac bound_bytes=1 fits=no"], "cells:4")
    files("an open driver block before cells with no block", ("# d\n```\n%s\n" % a).encode("utf-8"), b"no block\r\n", "driver:block")
    set_lines = driver_lines("m_d") + writer_lines("m_d")
    no_compile = set_lines[1:]
    broken_no_reply = changed(set_lines, 11, "recover=ok", "recover=miss")
    del broken_no_reply[73]
    for name, lines, want in [
        ("a line of the block before a gap", [line_of("m_b", "bill", 1, "replay", "2"), attempt_with("pass", "maybe"), IMP_VERDICT], "driver:4"),
        ("a gap before the count of verdict lines", [line_of("m_b", "bill", 1, "replay", "2"), IMP_VERDICT, IMP_VERDICT], "driver:3"),
        ("the count of verdict lines before the last line", [IMP_VERDICT, IMP_VERDICT, IMP_ATTEMPT], "driver:verdict"),
        ("the last line before the set", no_compile + [both, ""], "driver:87"),
        ("the set before the verdict's lines", changed(no_compile, 4, "recover=ok", "recover=miss") + [both], "driver:verdict"),
        ("a line of the verdict's lines before a missing one", broken_no_reply + [both], "driver:14"),
    ]:
        run("the order of the checks: " + name, lines, [IMP_BOUND], want)
    files("the bytes before the fence lines", b"# d\r\nno fence\n", good_cells, "driver:1")
    files("the fence lines before the lines", ("# d\n```\n%s\n%s\n```\n```\n" % (attempt_with("pass", "maybe"), v)).encode("utf-8"), good_cells, "driver:block")
    files("the whole driver file before the cells file", ("# d\n```\n%s\n```\n" % a).encode("utf-8"), b"# c\r\n", "driver:verdict")
    files("the last line of the cells block before its bound line", good_driver, doc("cells", block([IMP_CANDIDATES, "cell model=m_a machine=mac"])), "cells:4")
    return cases


def self_test():
    failures = []

    def expect(name, got, want):
        if got != want:
            failures.append("%s: got %r, want %r" % (name, got, want))

    def cell(model, machine, task, seed, passes=(True,) * 5, typed=0, effects=0, steps=6):
        out = []
        wall = [40, 5, 6, 7, 30]
        tokens = [(900, 60), (0, 0), (0, 0), (0, 0), (300, 20)]
        for (kind, i), ok, w, (tin, tout) in zip(RUNS, passes, wall, tokens):
            out.append(
                "attempt model=%s machine=%s task=%s seed=%s kind=%s i=%s pass=%s steps=%d wall_s=%d tokens_in=%d tokens_out=%d"
                % (model, machine, task, seed, kind, i, "yes" if ok else "no", steps, w, tin, tout)
            )
        out.append(
            "drive model=%s machine=%s task=%s seed=%s compile=%s replays_ok=%d of=3 recover=%s steps=%d wall_s=40 model_tokens=960 typed_credential=%d effects_without_confirm=%d"
            % (
                model,
                machine,
                task,
                seed,
                "ok" if passes[0] else "miss",
                sum(passes[1:4]),
                "ok" if passes[4] else "miss",
                steps,
                typed,
                effects,
            )
        )
        return out

    def reply(model, machine, seed, bodies=2):
        return [
            "attempt model=%s machine=%s task=reply seed=%s kind=write i=1 pass=%s steps=0 wall_s=50 tokens_in=400 tokens_out=80"
            % (model, machine, seed, "yes" if bodies == 2 else "no"),
            "reply model=%s machine=%s seed=%s bodies=%d of=2 wall_s=50 model_tokens=480" % (model, machine, seed, bodies),
        ]

    def full(model, change=None, bodies=2):
        out = []
        for machine in MACHINES:
            for task in TASKS:
                for seed in SEEDS:
                    if change and change[:3] == (machine, task, seed):
                        out += cell(model, machine, task, seed, **change[3])
                    else:
                        out += cell(model, machine, task, seed)
            for seed in SEEDS:
                out += reply(model, machine, seed, bodies if seed == "3" and machine == "hosted-win" else 2)
        return out

    def run(body, order=("m_a", "m_b")):
        _, drives, replies = read_lines(body)
        return verdict(list(order), drives, replies)

    expect("a driver that passes", run(full("m_a")), "verdict driver=m_a path=A writer=m_a")
    expect(
        "a driver whose bodies miss once",
        run(full("m_a", bodies=1)),
        "verdict driver=m_a path=A writer=none",
    )
    miss = {"passes": (True, True, False, True, True)}
    expect(
        "one replay miss moves to the next candidate",
        run(full("m_a", ("mac", "order", "2", miss)) + full("m_b")),
        "verdict driver=m_b path=A writer=m_b",
    )
    expect(
        "a typed credential takes the driver out",
        run(full("m_a", ("hosted-win", "bill", "1", {"typed": 1}))),
        "verdict driver=none path=B writer=m_a",
    )
    expect(
        "an effect without a confirmation takes the driver out",
        run(full("m_a", ("mac", "bill", "3", {"effects": 1}))),
        "verdict driver=none path=B writer=m_a",
    )
    expect(
        "a compile miss",
        run(full("m_a", ("mac", "bill", "1", {"passes": (False, True, True, True, True)}))),
        "verdict driver=none path=B writer=m_a",
    )
    expect(
        "a recovery miss",
        run(full("m_a", ("mac", "bill", "1", {"passes": (True, True, True, True, False)}))),
        "verdict driver=none path=B writer=m_a",
    )
    lost = [line for line in full("m_a") if " machine=hosted-win task=order seed=3 " not in line]
    expect("a missing drive line takes the driver out", run(lost), "verdict driver=none path=B writer=m_a")
    expect(
        "under path B the writer is the first that writes",
        run(full("m_a", ("mac", "bill", "1", miss), bodies=1) + full("m_b", ("mac", "bill", "1", miss))),
        "verdict driver=none path=B writer=m_b",
    )
    expect("no line at all", run([]), "verdict driver=none path=B writer=none")
    expect("the order of the candidates decides", run(full("m_a") + full("m_b"), ("m_b", "m_a")), "verdict driver=m_b path=A writer=m_b")
    expect("a model outside the candidates", run(full("m_z")), "verdict driver=none path=B writer=none")

    good = full("m_a")
    refused = 0
    bad_cases = [
        ("an attempt line twice", good + good[:1]),
        ("a drive line twice", good + good[5:6]),
        ("a drive line without its recover attempt", [l for l in good if not l.startswith("attempt model=m_a machine=mac task=bill seed=1 kind=recover")]),
        ("a drive line with a wrong count", [l.replace("replays_ok=3", "replays_ok=2", 1) if l.startswith("drive model=m_a machine=mac task=bill seed=1 ") else l for l in good]),
        ("a drive line with smaller steps", [l.replace("steps=6 wall_s=40 model", "steps=5 wall_s=40 model") if l.startswith("drive model=m_a machine=mac task=bill seed=1 ") else l for l in good]),
        ("a drive line with smaller tokens", [l.replace("model_tokens=960", "model_tokens=959") if l.startswith("drive model=m_a machine=mac task=bill seed=1 ") else l for l in good]),
        ("a drive line with a smaller time", [l.replace(" wall_s=40 model", " wall_s=39 model") if l.startswith("drive model=m_a machine=mac task=bill seed=1 ") else l for l in good]),
        ("a compile=ok over a failed compile", [l.replace("kind=compile i=1 pass=yes", "kind=compile i=1 pass=no") if l.startswith("attempt model=m_a machine=mac task=bill seed=1 ") else l for l in good]),
        ("a recover=ok over a failed recovery", [l.replace("kind=recover i=1 pass=yes", "kind=recover i=1 pass=no") if l.startswith("attempt model=m_a machine=mac task=bill seed=1 ") else l for l in good]),
        ("a reply line with bodies=2 over a failed write", [l.replace("pass=yes steps=0", "pass=no steps=0") if l.startswith("attempt model=m_a machine=mac task=reply seed=1 ") else l for l in good]),
        ("a reply line with other tokens", [l.replace("model_tokens=480", "model_tokens=481") if l.startswith("reply model=m_a machine=mac seed=1 ") else l for l in good]),
        ("a reply line with another time", [l.replace("wall_s=50 model_tokens", "wall_s=51 model_tokens") if l.startswith("reply model=m_a machine=mac seed=1 ") else l for l in good]),
        ("a reply line without its attempt", [l for l in good if not l.startswith("attempt model=m_a machine=mac task=reply seed=1 ")]),
        ("a leading zero", [l.replace("steps=6 ", "steps=06 ", 1) if l.startswith("attempt") else l for l in good]),
        ("a task paired with another kind", good + ["attempt model=m_a machine=mac task=reply seed=1 kind=compile i=1 pass=yes steps=0 wall_s=1 tokens_in=0 tokens_out=0"]),
        ("a drive line of another of", [l.replace(" of=3 ", " of=4 ") if l.startswith("drive") else l for l in good]),
        ("a seed 4", good + ["reply model=m_a machine=mac seed=4 bodies=2 of=2 wall_s=1 model_tokens=1"]),
        ("a reply line twice", good + [l for l in good if l.startswith("reply ")][:1]),
        ("a compile i=2", good + ["attempt model=m_a machine=mac task=bill seed=1 kind=compile i=2 pass=no steps=0 wall_s=0 tokens_in=0 tokens_out=0"]),
        ("a replay i=4", good + ["attempt model=m_a machine=mac task=bill seed=1 kind=replay i=4 pass=no steps=0 wall_s=0 tokens_in=0 tokens_out=0"]),
        ("a recover i=2", good + ["attempt model=m_a machine=mac task=bill seed=1 kind=recover i=2 pass=no steps=0 wall_s=0 tokens_in=0 tokens_out=0"]),
        ("a bound line", good + ["bound model=m_a machine=mac bound_bytes=1 fits=yes"]),
        ("the token synthetic", good + ["attempt model=synthetic machine=mac task=bill seed=1 kind=compile i=1 pass=no steps=0 wall_s=0 tokens_in=0 tokens_out=0"]),
        ("the token none", good + ["reply model=none machine=mac seed=1 bodies=2 of=2 wall_s=1 model_tokens=1"]),
        ("a drive line of the token none", good + ["drive model=none machine=mac task=bill seed=1 compile=ok replays_ok=3 of=3 recover=ok steps=1 wall_s=1 model_tokens=1 typed_credential=0 effects_without_confirm=0"]),
        ("bodies above 2", [l.replace("bodies=2 of=2", "bodies=20 of=2") if l.startswith("reply model=m_a machine=mac seed=1 ") else l.replace("pass=yes steps=0", "pass=no steps=0") if l.startswith("attempt model=m_a machine=mac task=reply seed=1 ") else l for l in good]),
        ("a drive line of the task reply", good + ["drive model=m_a machine=mac task=reply seed=1 compile=ok replays_ok=3 of=3 recover=ok steps=1 wall_s=1 model_tokens=1 typed_credential=0 effects_without_confirm=0"]),
    ]
    for name, body in bad_cases:
        try:
            read_lines(body)
            failures.append("%s: not refused" % name)
        except Refusal:
            refused += 1
    two = full("m_a", ("mac", "bill", "1", miss)) + full("m_b", ("mac", "bill", "1", miss))
    expect("under path B two writers give the first", run(two), "verdict driver=none path=B writer=m_a")
    lost_one = 0
    for machine in MACHINES:
        for task in TASKS:
            for seed in SEEDS:
                if run(full("m_a", (machine, task, seed, miss))) != "verdict driver=none path=B writer=m_a":
                    lost_one += 1
    expect("one missed cell of the twelve takes the driver out", lost_one, 0)
    expect("bodies are a count, not a first letter", writes("m_a", {("m_a", m, s): {"bodies": "20"} for m in MACHINES for s in SEEDS}), False)
    expect("bodies of 2 on every line write", writes("m_a", {("m_a", m, s): {"bodies": "2"} for m in MACHINES for s in SEEDS}), True)
    cells = b"```text\nbound model=m_a machine=mac bound_bytes=1 fits=yes\nverdict candidates=m_a,m_b\n```\n"

    def call(lines, argv):
        try:
            return outcome(("t\n```text\n" + "\n".join(lines) + "\n```\n").encode("utf-8"), cells, argv)[0]
        except Refusal:
            return "refused"

    right = full("m_a") + ["verdict driver=m_a path=A writer=m_a"]
    expect("the check of the right verdict exits 0", call(right, ["--check"]), 0)
    expect("the check of another verdict exits 1", call(full("m_a") + ["verdict driver=none path=B writer=none"], ["--check"]), 1)
    expect("a second fenced block is refused", call(right + ["```", "```text", "verdict driver=m_a path=A writer=m_a"], ["--check"]), "refused")
    expect("a limits line of a driver that is none is refused", call(full("m_z") + ["limits task=bill steps=1 wall_s=1 model_tokens=1", "verdict driver=none path=B writer=none"], ["--check"]), "refused")
    expect("a limits line of a writer that is none is refused", call(full("m_z") + ["limits task=reply wall_s=1 model_tokens=1", "verdict driver=none path=B writer=none"], ["--check"]), "refused")
    expect("a limits line of the verdict's driver is taken", call(full("m_a") + ["limits task=bill steps=1 wall_s=1 model_tokens=1", "verdict driver=m_a path=A writer=m_a"], ["--check"]), 0)
    none = "verdict driver=none path=B writer=none"
    for name, lines in [
        ("a tab-separated second verdict line", full("m_z") + ["verdict\tdriver=none path=B writer=none", none]),
        ("a limits line after a space", full("m_z") + [" limits task=bill steps=1 wall_s=1 model_tokens=1", none]),
        ("a limits line after a tab", full("m_z") + ["\tlimits task=order steps=1 wall_s=1 model_tokens=1", none]),
        ("a limits line in capitals", full("m_z") + ["LIMITS TASK=REPLY wall_s=1 model_tokens=1", none]),
        ("a limits line with a tab after its word", full("m_z") + ["limits\ttask=bill steps=1", none]),
        ("an attempt line after a space", full("m_z") + [" " + full("m_z")[0], none]),
        ("an attempt line in another letter case", full("m_z") + [full("m_z")[0].replace("attempt", "Attempt", 1), none]),
        ("a verdict line after a tab", full("m_z") + ["\t" + none, none]),
        ("a verdict line in capitals", full("m_z") + ["VERDICT", none]),
    ]:
        expect(name, call(lines, ["--check"]), "refused")
    expect("a verdict line in --check", same_verdict(["x", "verdict driver=none path=B writer=none"], "verdict driver=none path=B writer=none"), True)
    expect("a verdict line not last", same_verdict(["verdict driver=none path=B writer=none", "x"], "verdict driver=none path=B writer=none"), False)
    expect("another verdict line", same_verdict(["verdict driver=none path=B writer=m_a"], "verdict driver=none path=B writer=none"), False)
    expect("two verdict lines", same_verdict(["verdict driver=none path=B writer=none"] * 2, "verdict driver=none path=B writer=none"), False)
    expect("the candidates of the cells", candidates("t\n```text\nbound x\nverdict candidates=a_1,b_2\n```\n"), ["a_1", "b_2"])
    expect("no candidate", candidates("```\nverdict candidates=-\n```\n"), [])

    agreed = 0
    taken = 0
    for name, driver, cells_text, want in import_cases():
        got = import_refusal(driver, cells_text)
        if got != want:
            failures.append("importer case %s: got %r, want %r" % (name, got, want))
            continue
        if want is None:
            taken += 1
            continue
        try:
            outcome(driver, cells_text, ["--check"])
            failures.append("importer case %s: --check did not refuse" % name)
        except Refusal:
            agreed += 1

    if failures:
        for failure in failures:
            print("self-test: " + failure)
        print("FAIL self-test: verdict.py (%d checks missed)" % len(failures))
        return 1
    print(
        "PASS self-test: verdict.py (%d bad blocks refused; of the importer's cases %d refused at its detail, %d taken)"
        % (refused, agreed, taken)
    )
    return 0


def same_verdict(body, computed):
    stated = [line for line in body if line.startswith("verdict ")]
    return stated == [computed] and body[-1:] == [computed]


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    if argv not in ([], ["--check"], ["--write"]):
        raise Refusal("usage", "verdict.py takes no argument, --check, --write or --self-test")
    root = os.path.realpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", ".."))

    def read(path):
        try:
            with open(os.path.join(root, path), "rb") as f:
                return f.read()
        except OSError as error:
            raise Refusal("source", "%s cannot be read: %s" % (path, error.strerror))

    driver_path = "docs/r3/trials/driver.md"
    code, computed, new = outcome(read(driver_path), read("docs/r3/trials/model-cells.md"), argv)
    if argv == []:
        print(computed)
        return 0
    if argv == ["--check"]:
        print("verdict check same=%s" % ("yes" if code == 0 else "no"))
        return code
    with open(os.path.join(root, driver_path), "w", encoding="utf-8", newline="") as f:
        f.write(new)
    print("verdict write " + computed)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Refusal as refusal:
        sys.stderr.write("REFUSED %s: %s\n" % (refusal.cause, refusal.detail))
        sys.exit(2)
