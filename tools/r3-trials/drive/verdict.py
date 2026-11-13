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

--check   exits 1 when the last line of the block is not this verdict line.
--write   puts this verdict line in the block in place of its verdict line, at its end.
--self-test runs the rule on fixtures of its own.

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


def limits_fit(body, computed):
    """Whether every limits line names a job that the verdict gives: bill and order its driver,
    reply its writer (A13.6)."""
    driver = re.search(r"driver=(\S+)", computed).group(1)
    writer = re.search(r"writer=(\S+)", computed).group(1)
    for line in body:
        if line.startswith("limits "):
            task = re.search(r" task=(\S+)", line + " ")
            task = task.group(1) if task else ""
            if (task in TASKS and driver == "none") or (task == "reply" and writer == "none") or task not in TASKS + ("reply",):
                return False
    return True


def outcome(driver_text, cells_text, argv):
    """The verdict line that the rule gives, and the exit of the call: --check is 1 when the block's
    last line is not that line; every call refuses a limits line that the verdict gives no job for."""
    order = candidates(cells_text)
    body, fences = last_block(driver_text)
    _, drives, replies = read_lines(body)
    computed = verdict(order, drives, replies)
    if not limits_fit(body, computed):
        raise Refusal("source", "a limits line names a job that the verdict gives to none")
    if argv == ["--check"]:
        return (0 if same_verdict(body, computed) else 1), computed, body, fences
    return 0, computed, body, fences


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
    cells = "```text\nbound x\nverdict candidates=m_a,m_b\n```\n"

    def call(lines, argv):
        try:
            return outcome("t\n```text\n" + "\n".join(lines) + "\n```\n", cells, argv)[0]
        except Refusal:
            return "refused"

    right = full("m_a") + ["verdict driver=m_a path=A writer=m_a"]
    expect("the check of the right verdict exits 0", call(right, ["--check"]), 0)
    expect("the check of another verdict exits 1", call(full("m_a") + ["verdict driver=none path=B writer=none"], ["--check"]), 1)
    expect("a second fenced block is refused", call(right + ["```", "```text", "verdict driver=m_a path=A writer=m_a"], ["--check"]), "refused")
    expect("a limits line of a driver that is none is refused", call(full("m_z") + ["limits task=bill steps=1 wall_s=1 model_tokens=1", "verdict driver=none path=B writer=none"], ["--check"]), "refused")
    expect("a limits line of a writer that is none is refused", call(full("m_z") + ["limits task=reply wall_s=1 model_tokens=1", "verdict driver=none path=B writer=none"], ["--check"]), "refused")
    expect("a limits line of the verdict's driver is taken", call(full("m_a") + ["limits task=bill steps=1 wall_s=1 model_tokens=1", "verdict driver=m_a path=A writer=m_a"], ["--check"]), 0)
    expect("a verdict line in --check", same_verdict(["x", "verdict driver=none path=B writer=none"], "verdict driver=none path=B writer=none"), True)
    expect("a verdict line not last", same_verdict(["verdict driver=none path=B writer=none", "x"], "verdict driver=none path=B writer=none"), False)
    expect("another verdict line", same_verdict(["verdict driver=none path=B writer=m_a"], "verdict driver=none path=B writer=none"), False)
    expect("two verdict lines", same_verdict(["verdict driver=none path=B writer=none"] * 2, "verdict driver=none path=B writer=none"), False)
    expect("the candidates of the cells", candidates("t\n```text\nbound x\nverdict candidates=a_1,b_2\n```\n"), ["a_1", "b_2"])
    expect("no candidate", candidates("```\nverdict candidates=-\n```\n"), [])

    if failures:
        for failure in failures:
            print("self-test: " + failure)
        print("FAIL self-test: verdict.py (%d checks missed)" % len(failures))
        return 1
    print("PASS self-test: verdict.py (%d bad blocks refused)" % refused)
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
            with open(os.path.join(root, path), encoding="utf-8", newline="") as f:
                return f.read()
        except OSError as error:
            raise Refusal("source", "%s cannot be read: %s" % (path, error.strerror))

    driver_path = "docs/r3/trials/driver.md"
    text = read(driver_path)
    code, computed, body, fences = outcome(text, read("docs/r3/trials/model-cells.md"), argv)
    if argv == []:
        print(computed)
        return 0
    if argv == ["--check"]:
        print("verdict check same=%s" % ("yes" if code == 0 else "no"))
        return code
    lines = text.split("\n")
    kept = [line for line in body if not line.startswith("verdict ")]
    new = lines[: fences[0] + 1] + kept + [computed] + lines[fences[1] :]
    with open(os.path.join(root, driver_path), "w", encoding="utf-8", newline="") as f:
        f.write("\n".join(new))
    print("verdict write " + computed)
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Refusal as refusal:
        sys.stderr.write("REFUSED %s: %s\n" % (refusal.cause, refusal.detail))
        sys.exit(2)
