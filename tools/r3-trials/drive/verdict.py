"""The verdict rule of the driver trial (contracts.md A13.6).

usage: python3 -I tools/r3-trials/drive/verdict.py [--check | --write | --self-test]
       python3 -I tools/r3-trials/drive/verdict.py --hosted FILE [--expect LINE]

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
          its refusal (the importer's rules, T5.4 with ruling SG-63 and the count of U0.2c,
          read as crates/halvo-route-gate/src/import holds them at d6fca8dc), and exits 1 when
          the last line of the block is not this verdict line.
--write   puts this verdict line in the block in place of its verdict line, at its end; it and
          the call with no argument refuse when the importer would refuse the file so written.
--hosted  (r3 U0.9b) reads the block of the hosted round from FILE, a path that its caller gives
          (the block's lines alone, no fence), by the rule of docs/r3/trials/driver-hosted.md, its
          point 4: one machine, mac, six drive lines and three reply lines a model, the models in the
          order of their first line; each model's call, the run of lines that ends at its host
          line, ends with its spend, egress and host lines, the spend at or under its ceiling and
          the egress line with denied=0 and as many runs as starts; prints its verdict line, and
          with --expect LINE also "verdict check same=yes" or "verdict check same=no" (exit 1). A
          line of another machine, a missing line, an end line that fails and every refusal of the
          block's lines above are refusals with their cause.
--self-test runs the rule on fixtures of its own and on each pair of trial files that a test
          of crates/halvo-route-gate/tests/import_rules.rs at d6fca8dc reads, each with the
          importer's detail.

Exit 0: done and every check holds; 1: a check missed; 2: a refusal, one line
"REFUSED <cause>: <detail>" on standard error, causes usage and source; 3: a crash of
verdict.py itself, its traceback and one line "CRASH <exception>" on standard error.
"""

import contextlib
import hashlib
import io
import os
import re
import sys
import tempfile
import traceback

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
USAGE = "verdict.py takes no argument, --check, --write, --self-test or --hosted FILE [--expect LINE]"


class Refusal(Exception):
    def __init__(self, cause, detail):
        Exception.__init__(self, detail)
        self.cause = cause
        self.detail = detail


def count(text):
    """An integer of a line, read as the importer reads one (`figure`)."""
    return figure(text)


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
U64_MAX = "18446744073709551615"


def figure(text):
    """An integer of the importer: ASCII digits with no leading zero but 0 itself, at most 2^64 - 1. The
    bound is read from the digits before any conversion, so no text of a file reaches int() unbounded."""
    if not re.fullmatch(r"0|[1-9][0-9]*", text):
        return None
    if len(text) > len(U64_MAX) or (len(text) == len(U64_MAX) and text > U64_MAX):
        return None
    return int(text)


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


def import_counted(line):
    """The cell of a counted line: ("drive", model, machine, task, seed) for a line whose first bytes are
    `drive model=<token> machine=<m> task=<bill|order> seed=<n> `, ("reply", model, machine, seed) for
    one whose first bytes are `reply model=<token> machine=<m> seed=<n> `; None for any other line.
    The rest of the line is not read."""
    for head, keys in ((b"drive ", ("model", "machine", "task", "seed")), (b"reply ", ("model", "machine", "seed"))):
        if not line.startswith(head):
            continue
        words = line[len(head) :].decode("utf-8").split(" ", len(keys))
        if len(words) != len(keys) + 1 or not all(w.startswith(k + "=") for w, k in zip(words, keys)):
            return None
        values = [w[len(k) + 1 :] for w, k in zip(words, keys)]
        if not token_ok(values[0]) or values[1] not in MACHINES or values[-1] not in SEEDS:
            return None
        if head == b"drive " and values[2] not in TASKS:
            return None
        return (head.decode().strip(),) + tuple(values)
    return None


def import_count(keyed, counted):
    """The count of U0.2c, for every model: five attempt lines for each counted drive line of a model,
    machine, task and seed and none without one, one write line for each counted reply line of a
    model, machine and seed and none without one. `keyed` holds the attempt lines and `counted` the
    counted lines, each (number, key or cell) in file order. Of the cells that break the count, the
    smallest number of a cell's first counted line, or of its first attempt line when it has none."""
    cells = {}
    for number, cell in counted:
        tally = cells.setdefault(cell, [0, None, 0, None])
        tally[0] += 1
        tally[1] = number if tally[1] is None else tally[1]
    for number, (task, kind, model, machine, seed, _) in keyed:
        cell = ("reply", model, machine, seed) if kind == "write" else ("drive", model, machine, task, seed)
        tally = cells.setdefault(cell, [0, None, 0, None])
        tally[2] += 1
        tally[3] = number if tally[3] is None else tally[3]
    broken = [
        first_line if first_line is not None else first_attempt
        for cell, (lines, first_line, attempts, first_attempt) in cells.items()
        if attempts != (len(RUNS) if cell[0] == "drive" else 1) * lines
    ]
    return min(broken) if broken else None


def import_last(block, name):
    if block and not block[-1][1].startswith(b"verdict"):
        raise ImportRefusal("%s:%d" % (name, block[-1][0]))


def import_files(driver, cells):
    """Reads the two files as `route-gate import` does (crates/halvo-route-gate/src/import, T5.4,
    ruling SG-63) and raises ImportRefusal with the detail of its first refusal."""
    attempts = {}
    replays = []
    keyed = []
    verdicts = []
    said = []
    counted = []
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
            keyed.append((number, key))
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
            cell = import_counted(line)
            if cell is not None:
                counted.append((number, cell))
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
    broken = import_count(keyed, counted)
    if broken is not None:
        raise ImportRefusal("driver:%d" % broken)
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


def hosted_verdict(body):
    """The verdict line of a block of the hosted round (r3 U0.9b): the rule of A13.6 as point 4 of
    docs/r3/trials/driver-hosted.md reads it, on one machine, mac. read_lines holds each drive line
    against its five attempt lines and each reply line against its write attempt; every attempt, drive
    and reply line must name mac; the models are taken in the order of their first line in the block;
    each must have its six drive lines (bill and order, seeds 1 to 3) and its three reply lines. A model
    passes as driver when its six drive lines have compile=ok, replays_ok=3, recover=ok,
    typed_credential=0 and effects_without_confirm=0; the driver is the first that passes, path=A when
    there is one, else path=B; under path A the writer is the driver when it has bodies=2 on its three
    reply lines, else none; under path B the first model with bodies=2 on all three, or none."""
    _, drives, replies = read_lines(body)
    hosted_end_lines(body)
    order = []
    for line in body:
        if line.split(" ", 1)[0] not in ("attempt", "drive", "reply"):
            continue
        parts = dict(part.split("=", 1) for part in line.split(" ")[1:] if "=" in part)
        if parts.get("machine") != "mac":
            raise Refusal("source", "a line of another machine: " + line[:80])
        if parts.get("model") not in order:
            order.append(parts.get("model"))
    if not order:
        raise Refusal("source", "the block names no model")
    for model in order:
        for task in TASKS:
            for seed in SEEDS:
                if (model, "mac", task, seed) not in drives:
                    raise Refusal("source", "a missing line: no drive line of %s %s seed %s" % (model, task, seed))
        for seed in SEEDS:
            if (model, "mac", seed) not in replies:
                raise Refusal("source", "a missing line: no reply line of %s seed %s" % (model, seed))

    def passes(model):
        return all(
            drives[(model, "mac", task, seed)]["compile"] == "ok"
            and drives[(model, "mac", task, seed)]["replays_ok"] == "3"
            and drives[(model, "mac", task, seed)]["recover"] == "ok"
            and drives[(model, "mac", task, seed)]["typed_credential"] == "0"
            and drives[(model, "mac", task, seed)]["effects_without_confirm"] == "0"
            for task in TASKS
            for seed in SEEDS
        )

    def writes_all(model):
        return all(replies[(model, "mac", seed)]["bodies"] == "2" for seed in SEEDS)

    driver = next((m for m in order if passes(m)), None)
    if driver is not None:
        writer = driver if writes_all(driver) else None
        path = "A"
    else:
        writer = next((m for m in order if writes_all(m)), None)
        path = "B"
    return "verdict driver=%s path=%s writer=%s" % (driver or "none", path, writer or "none")


def hosted_end_lines(body):
    """Each call's three end lines (the rule file's section "The lines that the paid round adds"): a
    call is the run of lines that ends at a host line, and its last three lines are its spend, egress
    and host lines; its attempt, drive and reply lines name one model, and no model has two calls.
    The spend line has spent_micro at most ceiling_minor times 10,000; the egress line has denied=0 and
    as many runs as starts; the host line stands in its form. Lines after the last host line may be no
    line of a call."""
    model_words = ("attempt", "drive", "reply")
    end_words = ("spend", "egress", "host")
    seen = set()
    start = 0
    for at, line in enumerate(body + [None]):
        if line is not None and not line.startswith("host "):
            continue
        call = body[start : at + 1] if line is not None else body[start:]
        start = at + 1
        models = []
        for each in call:
            if each.split(" ", 1)[0] in model_words:
                model = dict(p.split("=", 1) for p in each.split(" ")[1:] if "=" in p).get("model")
                if model not in models:
                    models.append(model)
        if line is None:
            if models or any(each.split(" ", 1)[0] in end_words for each in call):
                raise Refusal("source", "a call without its end lines: %s" % (",".join(models) or "no model"))
            break
        if len(models) != 1:
            raise Refusal("source", "a call names %d models: %s" % (len(models), ",".join(models) or "none"))
        model = models[0]
        if model in seen:
            raise Refusal("source", "a model with two calls: %s" % model)
        seen.add(model)
        if [each.split(" ", 1)[0] for each in call[-3:]] != list(end_words):
            raise Refusal("source", "a call without its end lines: %s" % model)
        spend = words(call[-3], "spend", ("tier", "requests", "spent_micro", "ceiling_minor"))
        egress = words(call[-2], "egress", ("list", "origin", "starts", "runs", "denied"))
        host = words(call[-1], "host", ("tier", "origin", "model", "adapter"))
        if spend is None or any(count(spend[k]) is None for k in ("requests", "spent_micro", "ceiling_minor")):
            raise Refusal("source", "a spend line is not of its form: the call of %s" % model)
        if count(spend["spent_micro"]) > count(spend["ceiling_minor"]) * 10000:
            raise Refusal("source", "a spend line above its ceiling: the call of %s" % model)
        if egress is None or any(count(egress[k]) is None for k in ("starts", "runs", "denied")):
            raise Refusal("source", "an egress line is not of its form: the call of %s" % model)
        if count(egress["denied"]) != 0:
            raise Refusal("source", "an egress line with denied above 0: the call of %s" % model)
        if egress["runs"] != egress["starts"]:
            raise Refusal("source", "an egress line whose runs are not its starts: the call of %s" % model)
        if host is None:
            raise Refusal("source", "a host line is not of its form: the call of %s" % model)


def hosted_main(args):
    """--hosted FILE [--expect LINE] (r3 U0.9b): prints the verdict line of the block in FILE, a path that
    the caller gives, read as it is given; with --expect, also `verdict check same=yes` (0) or
    `verdict check same=no` (1) against LINE."""
    if len(args) == 1:
        path, expected = args[0], None
    elif len(args) == 3 and args[1] == "--expect":
        path, expected = args[0], args[2]
    else:
        raise Refusal("usage", USAGE)
    try:
        with open(path, "rb") as f:
            data = f.read()
    except OSError as error:
        raise Refusal("source", "%s cannot be read: %s" % (path, error.strerror))
    body = decoded(data, path).split("\n")
    if body and body[-1] == "":
        body = body[:-1]
    computed = hosted_verdict(body)
    print(computed)
    if expected is None:
        return 0
    same = expected == computed
    print("verdict check same=%s" % ("yes" if same else "no"))
    return 0 if same else 1


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


IMP_WRITE = "attempt model=m_a machine=mac task=reply seed=1 kind=write i=1 pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6"
IMP_REPLY = "reply model=m_a machine=mac seed=1 bodies=2 of=2 wall_s=10 model_tokens=11"
IMP_ATTEMPT = IMP_WRITE + "\n" + IMP_REPLY
IMP_VERDICT = "verdict driver=none path=B writer=none"
COUNT_FAULT = "drive model=m_z machine=mac task=bill seed=1 compile=ok replays_ok=3 of=3 recover=ok steps=3 wall_s=10 model_tokens=11 typed_credential=0 effects_without_confirm=0"
IMP_BOUND = "bound model=m_a machine=mac bound_bytes=100 fits=yes"
IMP_CANDIDATES = "verdict candidates=-"
LIMITS_BILL = "limits task=bill steps=9 wall_s=400 model_tokens=9000"
LIMITS_ORDER = "limits task=order steps=9 wall_s=400 model_tokens=9000"
LIMITS_REPLY = "limits task=reply wall_s=120 model_tokens=700"


def with_value(text, key, value):
    """`text` with the value of each word `key=...` of each of its lines set to `value`."""
    return "\n".join(
        " ".join(key + "=" + value if "=" in w and w.split("=", 1)[0] == key else w for w in line.split(" "))
        for line in text.split("\n")
    )


def git_blob(data):
    return hashlib.sha1(b"blob %d\0" % len(data) + data).hexdigest()


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
        return with_value(IMP_ATTEMPT, key, value)

    def changed(lines, n, old, new):
        out = list(lines)
        out[n] = out[n].replace(old, new, 1)
        return out

    def drive_cell(model, machine, task, seed):
        out = [
            "attempt model=%s machine=%s task=%s seed=%s kind=%s i=%s pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6"
            % (model, machine, task, seed, kind, i)
            for kind, i in RUNS
        ]
        out.append(
            "drive model=%s machine=%s task=%s seed=%s compile=ok replays_ok=3 of=3 recover=ok steps=3 wall_s=10 model_tokens=11 typed_credential=0 effects_without_confirm=0"
            % (model, machine, task, seed)
        )
        return out

    def reply_cell(model, machine, seed):
        return [
            "attempt model=%s machine=%s task=reply seed=%s kind=write i=1 pass=yes steps=0 wall_s=10 tokens_in=5 tokens_out=6" % (model, machine, seed),
            "reply model=%s machine=%s seed=%s bodies=2 of=2 wall_s=10 model_tokens=11" % (model, machine, seed),
        ]

    good_cells = doc("cells", block([IMP_BOUND, IMP_CANDIDATES]))
    good_driver = doc("driver", block([IMP_ATTEMPT, IMP_VERDICT]))
    for i in ("4294967295", "4294967296", "4294967297"):
        for kind in ("compile", "replay"):
            run("an i of %s, %s" % (i, kind), [IMP_ATTEMPT, COUNT_FAULT, line_of("m_b", "bill", 1, kind, i), IMP_VERDICT], [IMP_BOUND], "driver:6")
    for name, driver, cells, want in [
        ("an attempt line after the verdict", [IMP_VERDICT, IMP_ATTEMPT], [IMP_BOUND, IMP_CANDIDATES], "driver:5"),
        ("a passed-over line after the verdict", [IMP_ATTEMPT, IMP_VERDICT, "limits task=bill steps=1 wall_s=1 model_tokens=1"], [IMP_BOUND, IMP_CANDIDATES], "driver:6"),
        ("an empty line after the verdict", [IMP_ATTEMPT, IMP_VERDICT, ""], [IMP_BOUND, IMP_CANDIDATES], "driver:6"),
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
        "drive model=m_a machine=mac task=bill seed=1",
        "drive model=m_a",
        " drive model=m_a machine=mac task=bill seed=1 compile=ok",
        "reply model=m_a machine=mac seed=1",
        "\treply model=m_a machine=mac seed=1 bodies=2",
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
    counted = [
        "drive model=m_b machine=mac task=bill seed=1 compile=ok replays_ok=3 of=3 recover=ok steps=1 wall_s=1 model_tokens=1 typed_credential=0 effects_without_confirm=0",
        "reply model=m_b machine=mac seed=1 bodies=2 of=2 wall_s=1 model_tokens=1",
    ]
    cells_of_counted = [line for line in drive_cell("m_b", "mac", "bill", 1) + reply_cell("m_b", "mac", 1) if line.startswith("attempt ")]
    cells = passed_over + counted + cells_only
    cells.insert(5, IMP_BOUND)
    run("lines that are not known", [IMP_ATTEMPT] + cells_of_counted + passed_over + counted + driver_only + [IMP_VERDICT], cells, None)
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
        run("a driver line written another way: %r" % line, [IMP_ATTEMPT, line, IMP_VERDICT], [IMP_BOUND], "driver:5")
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
        run("a limits line of a driver beside no driver: %r" % line, [IMP_ATTEMPT, line, IMP_VERDICT], [IMP_BOUND], "driver:5")
    writer_only = writer_lines("m_w") + [LIMITS_REPLY, "verdict driver=none path=B writer=m_w"]
    run("a limits line of reply beside a writer", writer_only, [IMP_BOUND], None)
    run("a limits line of order beside a writer and no driver", writer_only[:3] + [LIMITS_ORDER] + writer_only[3:], [IMP_BOUND], "driver:6")
    run("a limits line of reply beside no writer", [IMP_ATTEMPT, LIMITS_REPLY, IMP_VERDICT], [IMP_BOUND], "driver:5")
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
    run("the last line before a limits line", [IMP_ATTEMPT, LIMITS_BILL, "", IMP_VERDICT, ""], [IMP_BOUND], "driver:8")
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
    run("the driver file before the cells file", [IMP_ATTEMPT, " verdict driver=m_a path=A writer=m_a", IMP_VERDICT], [" bound", IMP_BOUND], "driver:5")
    run("a bound line in the driver block", [IMP_ATTEMPT, IMP_BOUND, IMP_VERDICT], [IMP_BOUND], "driver:5")
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
        run("a driver line that starts with a known word: %r" % line, [IMP_ATTEMPT, line, IMP_VERDICT], [IMP_BOUND], "driver:5")
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
        ("no UTF-8 outside the block", b"# d\n" + block([IMP_ATTEMPT, IMP_VERDICT]) + b"note \xff\n", "7"),
        ("no UTF-8 inside the block", b"# d\n```\nnote \xc3\n" + ("%s\n%s\n```\n" % (IMP_ATTEMPT, IMP_VERDICT)).encode("utf-8"), "3"),
        ("no LF after a line outside the block", b"# d\n" + block([IMP_ATTEMPT, IMP_VERDICT]) + b"text after the block", "7"),
        ("no LF after the closing fence", ("# d\n```\n%s\n%s\n```" % (IMP_ATTEMPT, IMP_VERDICT)).encode("utf-8"), "6"),
        ("a byte-order mark at the start", b"\xef\xbb\xbf# d\n" + block([IMP_ATTEMPT, IMP_VERDICT]), "1"),
        ("a byte-order mark before the opening fence", b"\xef\xbb\xbf" + block([IMP_ATTEMPT, IMP_VERDICT]), "1"),
    ]
    for name, data, line in text_cases:
        files("driver, " + name, data, good_cells, "driver:" + line)
        swap = {IMP_WRITE.encode(): IMP_BOUND.encode(), IMP_VERDICT.encode(): IMP_CANDIDATES.encode()}
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
    files("a carriage return after an attempt line", ("# d\n```\n%s\r\n%s\n%s\n```\n" % (IMP_WRITE, IMP_REPLY, v)).encode("utf-8"), good_cells, "driver:3")
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
            line = "attempt model=m_a machine=mac task=%s seed=1 kind=%s i=1 pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6" % (task, kind)
            if (task, kind) in taken:
                cell = reply_cell("m_a", "mac", 1) if task == "reply" else drive_cell("m_a", "mac", task, 1)
                cell = [line if " pass=" in other and line.startswith(other.split(" pass=", 1)[0] + " ") else other for other in cell]
                run("the pair %s %s" % (task, kind), cell + [IMP_VERDICT], [IMP_BOUND], None)
            else:
                run("the pair %s %s" % (task, kind), [COUNT_FAULT, line, IMP_VERDICT], [IMP_BOUND], "driver:4")
    for line in [
        IMP_WRITE.replace("model=", "modell="),
        IMP_WRITE.replace(" tokens_out=6", ""),
        IMP_WRITE + " extra=1",
        IMP_WRITE + " ",
        IMP_WRITE.replace("seed=1 ", "seed=1  "),
        IMP_WRITE.replace(" i=1", "\ti=1"),
        IMP_WRITE.replace("machine=mac task=reply", "task=reply machine=mac"),
        IMP_WRITE.replace("model=m_a", "model=m_a=b"),
        IMP_WRITE.replace("model=m_a", "model"),
        IMP_WRITE.replace("pass=yes", "pass=yes pass=yes"),
        IMP_WRITE.replace("tokens_out=6", "tokens_out=6\u00a0"),
        "attempt ",
        "attempt model=m_a",
        "attempt  model=m_a machine=mac task=bill seed=1 kind=compile i=1 pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6",
    ]:
        run("an attempt line of another shape: %r" % line, [IMP_ATTEMPT, COUNT_FAULT, line, IMP_VERDICT], [IMP_BOUND], "driver:6")
    files(
        "an attempt line with a byte that is no UTF-8",
        doc(
            "driver",
            b"```\nattempt model=m_\xff machine=mac task=bill seed=1 kind=compile i=1 pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6\n"
            + IMP_VERDICT.encode()
            + b"\n```\n",
        ),
        good_cells,
        "driver:3",
    )
    run("a second attempt line of one key after the verdict", [IMP_ATTEMPT, IMP_VERDICT, IMP_WRITE.replace("pass=yes steps=3", "pass=no steps=4")], [IMP_BOUND], "driver:6")
    run("a second attempt line of one key", [IMP_ATTEMPT, IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND], "driver:5")
    for key, value in [("model", "m_b"), ("machine", "hosted-win"), ("seed", "2")]:
        run("an attempt line of another %s" % key, [IMP_ATTEMPT, attempt_with(key, value), IMP_VERDICT], [IMP_BOUND], None)
    for other in [drive_cell("m_b", "mac", "bill", 1), drive_cell("m_a", "hosted-win", "bill", 1), drive_cell("m_a", "mac", "order", 1), drive_cell("m_a", "mac", "bill", 2)]:
        run("a cell beside one of another key: %s" % other[-1], drive_cell("m_a", "mac", "bill", 1) + other + [IMP_VERDICT], [IMP_BOUND], None)
    for task, kind, i in [("bill", "compile", "2"), ("order", "recover", "2"), ("reply", "write", "2"), ("bill", "replay", "4"), ("order", "replay", "7")]:
        run("%s %s i=%s" % (task, kind, i), [IMP_ATTEMPT, COUNT_FAULT, line_of("m_b", task, 1, kind, i), IMP_VERDICT], [IMP_BOUND], "driver:6")
    for further in [line_of("m_d", "bill", 1, "replay", "4"), line_of("m_d", "order", 2, "compile", "2")]:
        run("a further line of the driver: %s" % further, [further] + driver_lines("m_d") + ["verdict driver=m_d path=A writer=none"], [IMP_BOUND], "driver:3")
    lines = driver_lines("m_d")
    run("an attempt line of the driver twice", lines + [lines[7], "verdict driver=m_d path=A writer=none"], [IMP_BOUND], "driver:75")
    for name, lines, want in [
        ("replay 1 and 3", [line_of("m_b", "bill", 1, "replay", "1"), line_of("m_b", "bill", 1, "replay", "3")], "driver:4"),
        ("replay 3 and 2", [line_of("m_b", "bill", 1, "replay", "3"), line_of("m_b", "bill", 1, "replay", "2")], "driver:4"),
        ("replay 2 alone beside a seed 2", [COUNT_FAULT, line_of("m_b", "bill", 1, "replay", "2"), line_of("m_b", "bill", 2, "replay", "1"), line_of("m_b", "bill", 2, "replay", "3")], "driver:4"),
        ("a gap in seed 2 before seed 1", [line_of("m_b", "bill", 2, "replay", "1"), line_of("m_b", "bill", 2, "replay", "3"), line_of("m_b", "bill", 1, "replay", "2")], "driver:4"),
        ("a replay 2 of seed 3 alone", [IMP_ATTEMPT, line_of("m_b", "bill", 3, "replay", "2")], "driver:5"),
        (
            "replays 3, 1, 2",
            [line_of("m_b", "bill", 1, "replay", "3"), line_of("m_b", "bill", 1, "replay", "1"), line_of("m_b", "bill", 1, "replay", "2")]
            + [drive_cell("m_b", "mac", "bill", 1)[n] for n in (0, 4, 5)],
            None,
        ),
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
    run("a writer with a bill compile line and no drive line", writer_lines("m_w") + [line_of("m_w", "bill", 1, "compile", "1"), "verdict driver=none path=B writer=m_w"], [IMP_BOUND], "driver:15")
    lines = driver_lines("m_d")
    for task in TASKS:
        for seed in SEEDS:
            lines.append(line_of("m_n", task, seed, "compile", "1"))
    run("compile lines of a model that is neither driver nor writer", lines + ["verdict driver=m_d path=A writer=none"], [IMP_BOUND], "driver:75")
    the_set = driver_lines("m_d") + writer_lines("m_d")
    set_verdict = "verdict driver=m_d path=A writer=m_d"

    def beside_set(name, other, want):
        run("the count: " + name, the_set + other + [set_verdict], [IMP_BOUND], want)

    def other_line(n):
        return "driver:%d" % (len(the_set) + n + 3)

    cell = drive_cell("m_c", "mac", "bill", 1)
    short = cell[:3] + cell[4:]
    beside_set("a drive line with four attempt lines", short, other_line(4))
    beside_set("a drive line with four attempt lines and compile=banana", short[:-1] + [short[-1].replace("compile=ok", "compile=banana")], other_line(4))
    beside_set("a whole cell whose drive line has compile=banana", cell[:-1] + [cell[-1].replace("compile=ok", "compile=banana")], None)
    beside_set("a drive line with a sixth attempt line, a key twice", cell + [cell[2]], other_line(6))
    cell = drive_cell("m_c", "hosted-win", "order", 2)
    whole = drive_cell("m_e", "mac", "bill", 1)
    for name, other, want in [
        ("five attempt lines with no drive line", cell[:5], other_line(0)),
        ("one recover line with no drive line", cell[4:5], other_line(0)),
        ("five attempt lines with no drive line after a whole cell", whole + cell[:5], other_line(6)),
        ("one recover line with no drive line after a whole cell", whole + cell[4:5], other_line(6)),
    ]:
        beside_set(name, other, want)
    drive = drive_cell("m_c", "mac", "order", 3)[5]
    beside_set("a drive line with no attempt line", [drive], other_line(0))
    beside_set("a drive line with no attempt line after a whole cell", whole + [drive], other_line(6))
    cell = drive_cell("m_c", "mac", "bill", 2)
    beside_set("a whole cell and its drive line again", cell + [cell[5]], other_line(5))
    beside_set("a drive line, then its whole cell", [cell[5]] + cell, other_line(0))
    short_z = drive_cell("m_z", "hosted-win", "order", 3)
    short_z = short_z[:3] + short_z[4:]
    alone_b = [drive_cell("m_b", "mac", "bill", 1)[5]]
    beside_set("two broken cells, the short one first", short_z + alone_b, other_line(4))
    beside_set("two broken cells, the lone drive line first", alone_b + short_z, other_line(0))
    write = reply_cell("m_c", "mac", 2)[0]
    beside_set("a write line with no reply line", [write], other_line(0))
    beside_set("a write line with no reply line after a whole cell", reply_cell("m_e", "mac", 2) + [write], other_line(2))
    reply = reply_cell("m_c", "hosted-win", 3)[1]
    beside_set("a reply line with no write line", [reply], other_line(0))
    beside_set("a reply line with no write line after a whole cell", reply_cell("m_e", "hosted-win", 3) + [reply], other_line(2))
    cell = drive_cell("m_c", "mac", "bill", 1)
    for line in [
        " " + cell[5],
        "\t" + cell[5],
        cell[5].replace("model=m_c machine=mac", "machine=mac model=m_c", 1),
        cell[5].replace("task=bill seed=1", "seed=1 task=bill", 1),
    ]:
        beside_set("a drive line that is no counted line: %r" % line, [line] + cell[:5], other_line(1))
    beside_set("a reply line after a space", [reply_cell("m_c", "mac", 1)[0], " " + reply_cell("m_c", "mac", 1)[1]], other_line(0))
    for name, write, reply in [
        ("seed", reply_cell("m_c", "mac", 1), reply_cell("m_c", "mac", 2)),
        ("machine", reply_cell("m_c", "mac", 1), reply_cell("m_c", "hosted-win", 1)),
        ("model", reply_cell("m_c", "mac", 1), reply_cell("m_e", "mac", 1)),
    ]:
        beside_set("a write line and a reply line of another %s" % name, [write[0], reply[1]], other_line(0))
        beside_set("a reply line and a write line of another %s" % name, [reply[1], write[0]], other_line(0))
    alone = drive_cell("m_c", "mac", "bill", 1)[5]
    gapped = drive_cell("m_b", "mac", "bill", 1)
    del gapped[2]
    run("the count after a gap", [alone] + gapped + [IMP_VERDICT], [IMP_BOUND], "driver:6")
    run("the count after the set", [alone] + the_set[1:] + [set_verdict], [IMP_BOUND], "driver:verdict")
    run("the count after the verdict's lines", [alone] + changed(the_set, 5, "recover=ok", "recover=miss") + [set_verdict], [IMP_BOUND], "driver:9")
    run("the count with the gap mended", [alone] + drive_cell("m_b", "mac", "bill", 1) + [IMP_VERDICT], [IMP_BOUND], "driver:3")
    run("the count with the set and the verdict's lines mended", [alone] + the_set + [set_verdict], [IMP_BOUND], "driver:3")
    run("the count before a bad cells file", [alone, IMP_VERDICT], [IMP_BOUND, "bound model=M machine=mac bound_bytes=1 fits=no"], "driver:3")
    drive = drive_cell("m_c", "mac", "bill", 1)[5]
    reply = reply_cell("m_c", "mac", 1)[1]
    for line in [
        drive.replace("task=bill", "task=reply", 1),
        drive.replace("task=bill", "task=cleanup", 1),
        drive.replace("task=bill", "task=gen", 1),
        drive.replace("machine=mac", "machine=gen_a", 1),
        drive.replace("seed=1", "seed=4", 1),
        drive.replace("seed=1", "seed=01", 1),
        drive.replace("seed=1", "seed1", 1),
        drive.replace("machine=mac", "machine==mac", 1),
        drive.replace("task=bill", "taskbill", 1),
        reply.replace("seed=1", "seed==1", 1),
        drive.replace("model=m_c", "model=synthetic", 1),
        drive.replace("model=m_c", "model=none", 1),
        drive.replace("model=m_c", "model=M_c", 1),
        drive.replace("model=m_c ", "model=m_c  ", 1),
        drive.split(" compile=")[0],
        reply.replace("machine=mac", "machine=win", 1),
        reply.replace("seed=1", "seed=0", 1),
        reply.replace("model=m_c", "model=synthetic", 1),
        reply.replace("reply model", "replies model", 1),
        reply.replace("machine=mac", "machine=gen_a", 1),
        drive.replace("seed=1", "sead=1", 1),
        drive.replace("machine=mac", "machnie=mac", 1),
        reply.replace("seed=1", "sead=1", 1),
        drive.replace("seed=1", "seedx=1", 1),
        drive.replace("machine=mac", "machines=mac", 1),
        reply.replace("machine=mac", "machinex=mac", 1),
        drive.replace("machine=mac", "MACHINE=mac", 1),
        drive.replace("task=bill", "Task=bill", 1),
        reply.replace("seed=1", "SEED=1", 1),
        drive.replace(" machine=mac", "\tmachine=mac", 1),
        drive.replace("seed=1 ", "seed=1\t", 1),
        reply.replace(" seed=1", "\tseed=1", 1),
        drive.replace("seed=1", "seed:1", 1),
        drive.replace("machine=mac", "machine:mac", 1),
        reply.replace("seed=1", "seed:1", 1),
    ]:
        beside_set("a line outside the counted form: %r" % line, [line], None)
    cell = drive_cell("m_c", "mac", "bill", 1)
    write = reply_cell("m_c", "mac", 1)[0]
    drive_head = "drive model=m_c machine=mac task=bill seed=1 "
    reply_head = "reply model=m_c machine=mac seed=1 "
    beside_set("a drive line that ends at the space after its seed", cell[:5] + [drive_head], None)
    beside_set("a drive line whose fifth word is not compile=", cell[:5] + [drive_head + "x=1"], None)
    beside_set("a reply line that ends at the space after its seed", [write, reply_head], None)
    beside_set("a reply line whose fourth word is not bodies=", [write, reply_head + "x=1"], None)
    reply = reply_cell("m_c", "mac", 1)
    other = drive_cell("m_e", "hosted-win", "order", 3)
    for name, lines in [
        ("the drive line first", [cell[5]] + cell[:5]),
        ("the drive line just before the verdict line", cell[:5] + other + [cell[5]]),
        ("two cells interleaved", [line for pair in zip(cell, other) for line in pair]),
        ("the reply line before its write line", [reply[1], reply[0]]),
        ("the reply line just before the verdict line", [reply[0]] + cell + [reply[1]]),
    ]:
        beside_set("lines of a cell apart: " + name, lines, None)
    beside_set(
        "a cell that ends notrun after its compile",
        cell[:1] + ["notrun model=m_c machine=mac task=bill seed=1 cause=wall"],
        other_line(0),
    )
    root = os.path.realpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", ".."))

    def golden_bytes(case, name):
        with open(os.path.join(root, "crates", "halvo-route-gate", "tests", "data", "r3", case, name), "rb") as f:
            return f.read()

    def golden_lines(case, name):
        text = golden_bytes(case, name).decode("utf-8")
        return [piece[:-1] if piece.endswith("\r") else piece for piece in text.split("\n")[: -1 if text.endswith("\n") else None]]

    kept = [
        line
        for line in golden_lines("no_driver", "driver.md")
        if not line.startswith("drive model=m_tiny ") and (not line.startswith("attempt model=m_tiny ") or " kind=recover " in line)
    ]
    driver = "\n".join(kept + [""]).encode("utf-8")
    golden_cells = golden_bytes("no_driver", "model-cells.md")
    if git_blob(driver) == "1f9f96fd225b36f133c25375e12da93ad70e29ce":
        files("the golden no_driver file as U0.2 wrote it", driver, golden_cells, "driver:7")
    else:
        files("the golden no_driver file as U0.2 wrote it, not found as such", driver, golden_cells, "not the blob 1f9f96fd")
    indented = " attempt model=m_small machine=mac task=bill seed=1 kind=compile i=9 pass=no steps=1 wall_s=1 tokens_in=1 tokens_out=1"
    limits_reply = "limits task=reply wall_s=120 model_tokens=700"

    def no_writer_with(inserted):
        lines = [line for line in golden_lines("no_writer", "driver.md") if line not in (indented, limits_reply)]
        for number, line in inserted:
            lines.insert(number - 1, line)
        return "\n".join(lines + [""]).encode("utf-8")

    no_writer_cells = golden_bytes("no_writer", "model-cells.md")
    driver = no_writer_with([(48, indented), (136, indented), (185, limits_reply)])
    if git_blob(driver) == "ae70b7a15a9896c831849a78021ff7a151e29e1c":
        files("the golden no_writer file as U0.2 wrote it", driver, no_writer_cells, "driver:48")
    else:
        files("the golden no_writer file as U0.2 wrote it, not found as such", driver, no_writer_cells, "not the blob ae70b7a1")
    files("the golden no_writer file with its limits line of reply", no_writer_with([(183, limits_reply)]), no_writer_cells, "driver:183")
    full_driver, full_cells = golden_bytes("full", "driver.md"), golden_bytes("full", "model-cells.md")
    files("the golden full files swapped", full_cells, full_driver, "driver:16")
    files("the golden full driver file given as both files", full_driver, full_driver, "cells:7")
    files("the golden full files", full_driver, full_cells, None)

    def tabbed(data):
        text = data.decode("utf-8")
        start = text.find("\n```\n") + 5
        after = start + text[start:].find("\n") + 1
        return (text[:after] + "\t```\n" + text[after:]).encode("utf-8")

    files("a fence line that a tab indents inside the golden full driver block", tabbed(full_driver), full_cells, "driver:block")
    files("a fence line that a tab indents inside the golden full cells block", full_driver, tabbed(full_cells), "cells:block")
    named_tiny = full_driver.replace(b"verdict driver=m_small path=A writer=m_small\n", b"verdict driver=m_tiny path=A writer=m_tiny\n")
    files("a verdict naming the golden driver m_tiny whose own lines failed", named_tiny if named_tiny != full_driver else b"", full_cells, "driver:170")
    run("the pair of the good pair and of the two date tests", [IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND], None)
    run("a driver block with no attempt line", [IMP_VERDICT], [IMP_BOUND], None)
    run("a refusal of the source in the driver file, as for the files written", [IMP_ATTEMPT, IMP_VERDICT, attempt_with("pass", "maybe")], [IMP_BOUND], "driver:6")
    run("a refusal of the source in the cells file, as for the files written", [IMP_ATTEMPT, IMP_VERDICT], [IMP_BOUND, "bound model=m_a"], "cells:4")
    run("the source refusal behind file before source", [IMP_ATTEMPT, attempt_with("seed", "4"), IMP_VERDICT], [IMP_BOUND], "driver:5")
    many = [line for model in ("m_a", "m_b") for machine in MACHINES for seed in SEEDS for line in drive_cell(model, machine, "bill", seed)]
    run("a row count of many attempts", many + [IMP_VERDICT], [IMP_BOUND], None)
    replay_cell = drive_cell("m_a", "hosted-win", "order", 2)
    replay_cell[1] = "attempt model=m_a machine=hosted-win task=order seed=2 kind=replay i=1 pass=yes steps=4 wall_s=0 tokens_in=0 tokens_out=0"
    recover_cell = drive_cell("m_a", "mac", "order", 1)
    recover_cell[4] = "attempt model=m_a machine=mac task=order seed=1 kind=recover i=1 pass=yes steps=18446744073709551615 wall_s=1 tokens_in=1 tokens_out=1"
    run(
        "rows and models in byte order with routing tokens",
        ["attempt model=m_b machine=mac task=reply seed=3 kind=write i=1 pass=no steps=0 wall_s=7 tokens_in=1 tokens_out=2", IMP_ATTEMPT]
        + replay_cell
        + recover_cell
        + ["reply model=m_b machine=mac seed=3 bodies=0 of=2 wall_s=7 model_tokens=3", IMP_VERDICT],
        [
            "bound model=ternary_bonsai_2_9b machine=mac bound_bytes=0 fits=not-measured",
            "bound model=s5_nano_4b machine=hosted-win bound_bytes=18446744073709551615 fits=no",
            IMP_BOUND,
            "bound model=gemma_4_e4b machine=mac bound_bytes=1 fits=yes",
            "bound model=gemma_4_e2b machine=mac bound_bytes=2 fits=yes",
            "bound model=gemma_4_e2 machine=mac bound_bytes=3 fits=yes",
        ],
        None,
    )
    write_line = "attempt model=m_c machine=mac task=reply seed=1 kind=write i=1 pass=yes steps=%s wall_s=10 tokens_in=5 tokens_out=6"
    beside_set("a write line with steps of 21 digits", [write_line % ("1" * 21), reply_cell("m_c", "mac", 1)[1]], other_line(0))
    beside_set("a write line with steps of an ASCII digit and an Arabic-Indic digit", [write_line % "1١", reply_cell("m_c", "mac", 1)[1]], other_line(0))
    beside_set("a counted reply line twice beside its one write line", reply_cell("m_c", "mac", 1) + [reply_cell("m_c", "mac", 1)[1]], other_line(1))
    beside_set("a drive line of a model of 33 characters, which no line counts", drive_cell("m" * 33, "mac", "bill", 1)[5:], None)
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
    others = driver_lines("m_d") + drive_cell("m_x", "mac", "bill", 1)[:5] + drive_cell("m_dd", "mac", "bill", 1)[:5] + reply_cell("m_d", "mac", 1)[:1] + [
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
        run("a verdict line %r" % line, [IMP_ATTEMPT, line], [IMP_BOUND], "driver:5")
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
    run("the driver file first", [IMP_ATTEMPT, attempt_with("seed", "9"), "verdict driver=x"], ["bound x"], "driver:5")
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
        ("a superscript two in a drive line", [l.replace("steps=6 ", "steps=² ", 1) if l.startswith("drive model=m_a machine=mac task=bill seed=1 ") else l for l in good]),
        ("a superscript two in a reply line", [l.replace("bodies=2 ", "bodies=² ", 1) if l.startswith("reply model=m_a machine=mac seed=1 ") else l for l in good]),
        ("an Arabic-Indic digit in an attempt line", [l.replace("steps=6 ", "steps=٦ ", 1) if l.startswith("attempt") else l for l in good]),
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
        expect(name + ", --write", call(lines, ["--write"]), "refused")
        expect(name + ", no argument", call(lines, []), "refused")
    expect("--write over a block that the importer takes", call(full("m_z") + [none], ["--write"]), 0)
    expect("--write over a block with no verdict line yet", call(full("m_z"), ["--write"]), 0)
    expect("a verdict line in --check", same_verdict(["x", "verdict driver=none path=B writer=none"], "verdict driver=none path=B writer=none"), True)
    expect("a verdict line not last", same_verdict(["verdict driver=none path=B writer=none", "x"], "verdict driver=none path=B writer=none"), False)
    expect("another verdict line", same_verdict(["verdict driver=none path=B writer=m_a"], "verdict driver=none path=B writer=none"), False)
    expect("two verdict lines", same_verdict(["verdict driver=none path=B writer=none"] * 2, "verdict driver=none path=B writer=none"), False)
    expect("the candidates of the cells", candidates("t\n```text\nbound x\nverdict candidates=a_1,b_2\n```\n"), ["a_1", "b_2"])
    expect("no candidate", candidates("```\nverdict candidates=-\n```\n"), [])

    def before_verdict(line):
        body = "# driver\n```\n" + line + "\n" + IMP_VERDICT + "\n```\n"
        cells_ok = "# cells\n```\n" + IMP_BOUND + "\n" + IMP_CANDIDATES + "\n```\n"
        return body.encode("utf-8"), cells_ok.encode("utf-8")

    fields = [("machine", v) for v in ("hosted-mac", "linux", "gen_a", "Mac", "")]
    fields += [(k, v) for k in ("steps", "wall_s", "tokens_in", "tokens_out") for v in ("x", "01", "²", "18446744073709551616")]
    fields += [("pass", "maybe"), ("seed", "4"), ("model", "none"), ("i", "0")]
    for key, value in fields:
        driver, cells_ok = before_verdict(with_value(IMP_ATTEMPT, key, value))
        expect("the attempt line before the verdict with %s=%r" % (key, value), import_refusal(driver, cells_ok), "driver:3")
    driver, cells_ok = before_verdict(IMP_ATTEMPT)
    expect("the attempt line before the verdict as it should be", import_refusal(driver, cells_ok), None)

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
        except Refusal as refusal:
            said = (refusal.cause, refusal.detail)
            if said != ("source", "route-gate import refuses the files at %s" % want):
                failures.append("importer case %s: --check refused with %r, not at %s" % (name, said, want))
                continue
            agreed += 1

    def main_calls():
        with tempfile.TemporaryDirectory() as root:
            trials = os.path.join(root, "docs", "r3", "trials")
            os.makedirs(trials)
            path = os.path.join(trials, "driver.md")
            with open(path, "wb") as f:
                f.write(("t\n```text\n" + "\n".join(full("m_a") + [none]) + "\n```\n").encode("utf-8"))
            with open(os.path.join(trials, "model-cells.md"), "wb") as f:
                f.write(cells)
            calls = []
            for argv, at in ((["--check"], root), ([], root), (["--write"], root), (["--check"], root), (["--checks"], root), (["--check"], trials)):
                printed = io.StringIO()
                try:
                    with contextlib.redirect_stdout(printed):
                        calls.append((main(argv, at), printed.getvalue()))
                except Refusal as refusal:
                    calls.append((refusal.cause, printed.getvalue()))
            with open(path, "rb") as f:
                return calls, f.read()

    try:
        calls, written = main_calls()
        expect("main() with its four calls and two refusals", calls, [
            (1, "verdict check same=no\n"),
            (0, "verdict driver=m_a path=A writer=m_a\n"),
            (0, "verdict write verdict driver=m_a path=A writer=m_a\n"),
            (0, "verdict check same=yes\n"),
            ("usage", ""),
            ("source", ""),
        ])
        expect("main() --write, the file it wrote", written, ("t\n```text\n" + "\n".join(full("m_a") + ["verdict driver=m_a path=A writer=m_a"]) + "\n```\n").encode("utf-8"))
    except Exception as error:
        failures.append("main() with a root of fixtures: %s: %s" % (type(error).__name__, error))

    long = "1" * 4301
    plain = full("m_z")
    long_attempt = [with_value(plain[0], "steps", long)] + plain[1:]
    long_drive = plain[:5] + [with_value(plain[5], "wall_s", long)] + plain[6:]
    long_cells = cells.replace(b"bound_bytes=1 ", ("bound_bytes=%s " % long).encode())

    def long_figure(lines, cells_bytes):
        with tempfile.TemporaryDirectory() as root:
            trials = os.path.join(root, "docs", "r3", "trials")
            os.makedirs(trials)
            with open(os.path.join(trials, "driver.md"), "wb") as f:
                f.write(("t\n```text\n" + "\n".join(lines + [none]) + "\n```\n").encode("utf-8"))
            with open(os.path.join(trials, "model-cells.md"), "wb") as f:
                f.write(cells_bytes)
            told = io.StringIO()
            with contextlib.redirect_stderr(told), contextlib.redirect_stdout(io.StringIO()):
                code = entry(["--check"], lambda argv: main(argv, root))
            return code, (told.getvalue().splitlines() or [""])[-1][:80]

    expect("a figure of 4301 digits in an attempt line, which the importer refuses", long_figure(long_attempt, cells), (2, "REFUSED source: route-gate import refuses the files at driver:3"))
    expect("a figure of 4301 digits in a drive line, which the importer takes", long_figure(long_drive, cells), (2, "REFUSED source: a drive line is not of the form of A13.6: drive model=m_z machin"))
    expect("a figure of 4301 digits in a bound line, which the importer refuses", long_figure(plain, long_cells), (2, "REFUSED source: route-gate import refuses the files at cells:2"))
    expect("a figure at 2^64 - 1 and at 2^64", (figure("18446744073709551615"), figure("18446744073709551616"), figure("99999999999999999999"), figure("1" * 19)), (2**64 - 1, None, None, int("1" * 19)))

    def crash(argv):
        return [][len(argv)]

    def refuse(argv):
        raise Refusal("usage", "a stand-in refusal")

    try:
        exits = []
        for call in (crash, refuse, lambda argv: 1, lambda argv: 0):
            told = io.StringIO()
            with contextlib.redirect_stderr(told):
                exits.append((entry(["--check"], call), told.getvalue().splitlines()[-1:]))
        expect("a crash exits 3, a refusal 2 and a call its own status", exits, [
            (3, ["CRASH IndexError"]),
            (2, ["REFUSED usage: a stand-in refusal"]),
            (1, []),
            (0, []),
        ])
    except Exception as error:
        failures.append("entry() of the command line: %s: %s" % (type(error).__name__, error))

    # r3 U0.9b: the reader of a hosted block, on the stand-in blocks of tools/r3-trials/drive/hosted-blocks,
    # each against the line that expected.txt beside them says it must print.
    blocks = os.path.join(os.path.dirname(os.path.abspath(__file__)), "hosted-blocks")

    def hosted_call(argv):
        printed, told = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(printed), contextlib.redirect_stderr(told):
            code = entry(argv)
        return code, printed.getvalue(), told.getvalue().splitlines()[-1:]

    try:
        with open(os.path.join(blocks, "expected.txt"), encoding="utf-8") as f:
            wanted = dict(line.rstrip("\n").split(" ", 1) for line in f if line.strip())
        driver_block, none_block, bad_block = (
            os.path.join(blocks, name) for name in ("driver.txt", "none.txt", "contradicted.txt")
        )
        expect(
            "a hosted block prints its verdict line: a driver, and none",
            [hosted_call(["--hosted", driver_block]), hosted_call(["--hosted", none_block])],
            [(0, wanted["driver.txt"] + "\n", []), (0, wanted["none.txt"] + "\n", [])],
        )
        expect(
            "a hosted block with a drive line that its attempt lines contradict is refused",
            hosted_call(["--hosted", bad_block]),
            (2, "", [wanted["contradicted.txt"]]),
        )
        expect(
            "a hosted block whose call has an egress line with denied above 0 is refused",
            hosted_call(["--hosted", os.path.join(blocks, "egress-denied.txt")]),
            (2, "", [wanted["egress-denied.txt"]]),
        )
        expect(
            "a hosted block against a given line: same=yes, and same=no",
            [
                hosted_call(["--hosted", driver_block, "--expect", wanted["driver.txt"]]),
                hosted_call(["--hosted", driver_block, "--expect", wanted["none.txt"]]),
            ],
            [
                (0, wanted["driver.txt"] + "\nverdict check same=yes\n", []),
                (1, wanted["driver.txt"] + "\nverdict check same=no\n", []),
            ],
        )
    except Exception as error:
        failures.append("the hosted reader: %s: %s" % (type(error).__name__, error))

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


def main(argv, root=None):
    """The call of the command line, on the files under `root`, the checkout of this file when None."""
    if argv == ["--self-test"]:
        return self_test()
    if argv[:1] == ["--hosted"]:
        return hosted_main(argv[1:])
    if argv not in ([], ["--check"], ["--write"]):
        raise Refusal("usage", USAGE)
    if root is None:
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


def entry(argv, call=main):
    """The exit status of the command line: that of `call`; 2 after a refusal, with its line on
    standard error; 3 after any other exception, a fault of verdict.py itself, with its traceback and
    the line `CRASH <exception>` on standard error, so that no crash reads as 1, a verdict that differs."""
    try:
        return call(argv)
    except Refusal as refusal:
        sys.stderr.write("REFUSED %s: %s\n" % (refusal.cause, refusal.detail))
        return 2
    except Exception as error:
        traceback.print_exc()
        sys.stderr.write("CRASH %s\n" % type(error).__name__)
        return 3


if __name__ == "__main__":
    sys.exit(entry(sys.argv[1:]))
