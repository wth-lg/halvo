"""The budget arithmetic of the model cells (contracts.md A13.5).

usage: python3 -I tools/r3-trials/model/fits.py [--check | --write | --self-test]

Reads browser_bound_bytes= of the verdict line of docs/r3/trials/browser-profile.md,
the weights lines of tools/r3-trials/model/weights.txt and the cell lines of the
block of docs/r3/trials/model-cells.md, and prints one bound line for each model of
weights.txt on each of the machines mac and hosted-win, then the verdict line:

  bound model=<token> machine=<m> bound_bytes=<n> fits=<yes|no|not-measured>
  verdict candidates=<the tokens of the group agents that fit on both machines, in
                      the order of weights.txt, joined by commas, or ->

The bound of a prompt is the median of its five peaks times 1.25 rounded up to
8 MiB, 8388608 * ceil(5 * median / 33554432); the bound of a model on a machine is
the larger of its two prompts' bounds, and 0 when a prompt has not five cell lines.
The budget is 8589934592 - (browser_bound_bytes + 8388608) - 268435456. fits is
not-measured when a prompt has not five cell lines, a run has capped=yes, or a
prompt's median peak is below the file's byte size; else yes when the bound is at
or under the budget, and no when it is above.

--check   exits 1 when the bound and verdict lines of the block differ from these.
--write   puts these lines in the block in place of its bound and verdict lines,
          at its end.
--self-test runs the rule on fixtures of its own.

Exit 0: done and every check holds; 1: a check missed; 2: a refusal, one line
"REFUSED <cause>: <detail>" on standard error, causes usage and source.
"""

import os
import sys

MIB8 = 8388608
CAP = 8589934592
RUNNER = 268435456
MACHINES = ("mac", "hosted-win")
PROMPTS = ("action", "reply")
CELL_KEYS = (
    "model",
    "file_sha256",
    "machine",
    "prompt",
    "run",
    "peak_bytes",
    "capped",
    "load_ms",
    "first_token_ms",
    "tokens_per_s",
    "tokens_in",
    "tokens_out",
)


class Refusal(Exception):
    def __init__(self, cause, detail):
        Exception.__init__(self, detail)
        self.cause = cause
        self.detail = detail


def median(values):
    """The middle value in ascending order; for an even count the higher of the two (C23.7)."""
    ordered = sorted(values)
    return ordered[len(ordered) // 2]


def prompt_bound(peaks):
    """The median of the peaks times 1.25, rounded up to 8 MiB, in integers."""
    return MIB8 * -(-5 * median(peaks) // (4 * MIB8))


def budget(browser_bound):
    """The model budget: the cap less the browser bound with one step added, less the runner."""
    return CAP - (browser_bound + MIB8) - RUNNER


def words(line, head, keys):
    """The values of a line `head k1=v1 k2=v2 ...` with exactly the keys `keys`, in order."""
    parts = line.split(" ")
    if parts[0] != head or len(parts) != len(keys) + 1:
        return None
    values = []
    for part, key in zip(parts[1:], keys):
        if not part.startswith(key + "="):
            return None
        values.append(part[len(key) + 1 :])
    return values


def count(text):
    if text.isdigit() and (text == "0" or not text.startswith("0")):
        return int(text)
    return None


def parse_weights(text):
    """The weights lines: a list of (token, group, bytes) in the order of the file, and the
    SHA-256 of each token's file."""
    out = []
    shas = {}
    for n, line in enumerate(text.splitlines(), 1):
        v = words(line, "weights", ("model", "group", "file", "sha256", "bytes", "url", "spdx"))
        if v is None or v[1] not in ("agents", "routing") or count(v[4]) is None:
            raise Refusal("source", "weights.txt line %d is no weights line" % n)
        out.append((v[0], v[1], count(v[4])))
        shas[v[0]] = v[3]
    return out, shas


def browser_bound(text):
    for line in text.splitlines():
        if line.startswith("verdict "):
            for part in line.split(" "):
                if part.startswith("browser_bound_bytes="):
                    value = count(part[len("browser_bound_bytes=") :])
                    if value is not None:
                        return value
    raise Refusal("source", "browser-profile.md has no verdict line with browser_bound_bytes=")


def block(text):
    """The lines of the one fenced block, and the line numbers of its fences."""
    lines = text.split("\n")
    fences = [i for i, line in enumerate(lines) if line.lstrip(" \t").startswith("```")]
    if len(fences) != 2:
        raise Refusal("source", "model-cells.md has %d fence lines, not 2" % len(fences))
    return lines[fences[0] + 1 : fences[1]], fences


def cells(lines, shas):
    """{(token, machine, prompt): [(peak_bytes, capped), ...]} from the cell lines. A line
    whose file_sha256 is not the SHA-256 of its model's weights line, and a run number that
    stands twice for one model, machine and prompt, are refused."""
    out = {}
    runs = set()
    for line in lines:
        if not line.startswith("cell "):
            continue
        v = words(line, "cell", CELL_KEYS)
        if v is None or count(v[5]) is None or v[6] not in ("yes", "no") or not count(v[4]):
            raise Refusal("source", "a cell line is not of the form of A13.5: " + line[:80])
        if shas.get(v[0]) != v[1]:
            raise Refusal("source", "a cell line names another file than its model's weights line: " + line[:80])
        if (v[0], v[2], v[3], v[4]) in runs:
            raise Refusal("source", "a run number stands twice: " + line[:80])
        runs.add((v[0], v[2], v[3], v[4]))
        out.setdefault((v[0], v[2], v[3]), []).append((count(v[5]), v[6] == "yes"))
    return out


def same_lines(body, computed):
    """Whether the bound and verdict lines of the block are the computed ones, in order, and
    the block ends with the computed verdict line."""
    stated = [line for line in body if line.startswith("bound ") or line.startswith("verdict ")]
    return stated == computed and body[-1:] == computed[-1:]


def lines_for(weights, found, browser):
    limit = budget(browser)
    out = []
    fit = {}
    for token, group, file_bytes in weights:
        for machine in MACHINES:
            runs = [found.get((token, machine, prompt), []) for prompt in PROMPTS]
            if any(len(r) != 5 for r in runs):
                bound, fits = 0, "not-measured"
            else:
                peaks = [[p for p, _ in r] for r in runs]
                bound = max(prompt_bound(p) for p in peaks)
                if any(c for r in runs for _, c in r) or any(median(p) < file_bytes for p in peaks):
                    fits = "not-measured"
                else:
                    fits = "yes" if bound <= limit else "no"
            fit[(token, machine)] = fits
            out.append("bound model=%s machine=%s bound_bytes=%d fits=%s" % (token, machine, bound, fits))
    chosen = [
        token
        for token, group, _ in weights
        if group == "agents" and all(fit[(token, m)] == "yes" for m in MACHINES)
    ]
    out.append("verdict candidates=" + (",".join(chosen) if chosen else "-"))
    return out


def self_test():
    failures = []

    def expect(name, got, want):
        if got != want:
            failures.append("%s: got %r, want %r" % (name, got, want))

    expect("median of five", median([5, 1, 4, 2, 3]), 3)
    expect("median of an even count", median([4, 1, 3, 2]), 3)
    expect("a bound at 256 MiB", prompt_bound([214748364] * 5), 256 * 1048576)
    expect("a bound one step up", prompt_bound([214748365] * 5), 264 * 1048576)
    expect("a bound from the median", prompt_bound([1, 2, 973078528, 10 ** 10, 10 ** 10]), 1216348160)
    expect("the budget of the bound on main", budget(1216348160), 7096762368)
    peak = 2000000000
    file_bytes = 1500000000
    weights = [("m_a", "agents", file_bytes), ("m_b", "agents", file_bytes), ("r_c", "routing", file_bytes)]

    def full(peaks_of):
        found = {}
        for token, _, _ in weights:
            for machine in MACHINES:
                for prompt in PROMPTS:
                    found[(token, machine, prompt)] = [(p, c) for p, c in peaks_of(token, machine, prompt)]
        return found

    def plain(token, machine, prompt):
        return [(peak, False)] * 5

    out = lines_for(weights, full(plain), 1216348160)
    expect(
        "every model fits",
        out[-1],
        "verdict candidates=m_a,m_b",
    )
    expect(
        "the bound line of a model",
        out[0],
        "bound model=m_a machine=mac bound_bytes=2508193792 fits=yes",
    )
    expect("one bound line per model and machine", len(out), 7)

    def big_on_win(token, machine, prompt):
        if token == "m_a" and machine == "hosted-win" and prompt == "reply":
            return [(7096762368, False)] * 5
        return plain(token, machine, prompt)

    out = lines_for(weights, full(big_on_win), 1216348160)
    expect("a model above the budget on one machine", out[1], "bound model=m_a machine=hosted-win bound_bytes=8875147264 fits=no")
    expect("a model above the budget leaves the verdict", out[-1], "verdict candidates=m_b")

    def at_budget(token, machine, prompt):
        if token == "m_a":
            return [(5677409894, False)] * 5
        return plain(token, machine, prompt)

    out = lines_for(weights, full(at_budget), 1216348160)
    expect("a bound at the budget fits", out[0], "bound model=m_a machine=mac bound_bytes=7096762368 fits=yes")

    def capped(token, machine, prompt):
        if token == "m_b" and machine == "mac" and prompt == "action":
            return [(peak, False)] * 4 + [(8600000000, True)]
        return plain(token, machine, prompt)

    out = lines_for(weights, full(capped), 1216348160)
    expect("a capped run", out[2], "bound model=m_b machine=mac bound_bytes=2508193792 fits=not-measured")
    expect("a capped run leaves the verdict", out[-1], "verdict candidates=m_a")

    def below_file(token, machine, prompt):
        if token == "m_a" and machine == "mac" and prompt == "reply":
            return [(file_bytes - 1, False)] * 5
        return plain(token, machine, prompt)

    out = lines_for(weights, full(below_file), 1216348160)
    expect("a median below the file", out[0], "bound model=m_a machine=mac bound_bytes=2508193792 fits=not-measured")

    def at_file(token, machine, prompt):
        if token == "m_a" and machine == "mac" and prompt == "reply":
            return [(file_bytes, False)] * 5
        return plain(token, machine, prompt)

    out = lines_for(weights, full(at_file), 1216348160)
    expect("a median equal to the file is measured", out[0], "bound model=m_a machine=mac bound_bytes=2508193792 fits=yes")
    found = full(plain)
    found[("m_b", "mac", "reply")] = [(peak, False)] * 6
    out = lines_for(weights, found, 1216348160)
    expect("a prompt with six cells", out[2], "bound model=m_b machine=mac bound_bytes=0 fits=not-measured")

    def cell_line(sha, run):
        return (
            "cell model=m_a file_sha256=%s machine=mac prompt=action run=%s peak_bytes=1 capped=no"
            " load_ms=1 first_token_ms=1 tokens_per_s=1 tokens_in=1 tokens_out=1" % (sha, run)
        )

    shas = {"m_a": "a" * 64}

    def refused(lines):
        try:
            cells(lines, shas)
        except Refusal:
            return True
        except Exception:
            return False
        return False

    expect("the lines of one model's file are taken", refused([cell_line("a" * 64, n) for n in "12345"]), False)
    expect("a cell of another file is refused", refused([cell_line("b" * 64, 1)]), True)
    expect("a cell of a model with no weights line is refused", refused([cell_line("a" * 64, 1).replace("m_a", "m_z")]), True)
    expect("a run number twice is refused", refused([cell_line("a" * 64, 1), cell_line("a" * 64, 1)]), True)
    expect("a run number that is no count is refused", refused([cell_line("a" * 64, 0)]), True)
    computed = ["bound model=m_a machine=mac bound_bytes=8388608 fits=yes", "verdict candidates=m_a"]
    expect("the check takes the computed lines", same_lines(["x"] + computed, computed), True)
    expect(
        "the check reads the verdict line",
        same_lines([computed[0], "verdict candidates=m_a,gemma_4_e4b"], computed),
        False,
    )
    expect("the check reads the last line", same_lines(computed + ["x"], computed), False)

    found = full(plain)
    del found[("m_a", "hosted-win", "action")]
    out = lines_for(weights, found, 1216348160)
    expect("a prompt with no cell", out[1], "bound model=m_a machine=hosted-win bound_bytes=0 fits=not-measured")
    expect("a prompt with no cell leaves the verdict", out[-1], "verdict candidates=m_b")
    found = full(plain)
    found[("m_b", "mac", "reply")] = [(peak, False)] * 4
    out = lines_for(weights, found, 1216348160)
    expect("a prompt with four cells", out[2], "bound model=m_b machine=mac bound_bytes=0 fits=not-measured")

    out = lines_for(weights, {}, 1216348160)
    expect("no cell at all", out[-1], "verdict candidates=-")
    expect("a routing model never enters the verdict", "r_c" in lines_for(weights, full(plain), 1216348160)[-1], False)
    out = lines_for([("m_b", "agents", file_bytes), ("m_a", "agents", file_bytes)], full(plain), 1216348160)
    expect("the verdict keeps the order of weights.txt", out[-1], "verdict candidates=m_b,m_a")
    out = lines_for(weights, full(plain), 1216348160 + MIB8 * 600)
    expect("a larger browser bound lowers the budget", out[-1], "verdict candidates=-")

    rejected = 0
    for bad in (
        "cell model=m file_sha256=x machine=mac prompt=action run=1 peak_bytes=01 capped=no load_ms=1 first_token_ms=1 tokens_per_s=1 tokens_in=1 tokens_out=1",
        "cell model=m file_sha256=x machine=mac prompt=action run=1 peak_bytes=1 capped=maybe load_ms=1 first_token_ms=1 tokens_per_s=1 tokens_in=1 tokens_out=1",
        "cell model=m machine=mac prompt=action run=1 peak_bytes=1 capped=no load_ms=1 first_token_ms=1 tokens_per_s=1 tokens_in=1 tokens_out=1",
    ):
        try:
            cells([bad], {"m": "x"})
        except Refusal:
            rejected += 1
    expect("bad cell lines rejected", rejected, 3)

    if failures:
        for failure in failures:
            print("self-test: " + failure)
        print("FAIL self-test: fits.py (%d checks missed)" % len(failures))
        return 1
    print("PASS self-test: fits.py (%d bad cell lines rejected)" % rejected)
    return 0


def main(argv):
    if argv == ["--self-test"]:
        return self_test()
    if argv not in ([], ["--check"], ["--write"]):
        raise Refusal("usage", "fits.py takes no argument, --check, --write or --self-test")
    root = os.path.realpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", ".."))

    def read(path):
        try:
            with open(os.path.join(root, path), encoding="utf-8", newline="") as f:
                return f.read()
        except OSError as error:
            raise Refusal("source", "%s cannot be read: %s" % (path, error.strerror))

    cells_path = "docs/r3/trials/model-cells.md"
    weights, shas = parse_weights(read("tools/r3-trials/model/weights.txt"))
    browser = browser_bound(read("docs/r3/trials/browser-profile.md"))
    text = read(cells_path)
    body, fences = block(text)
    computed = lines_for(weights, cells(body, shas), browser)
    if argv == []:
        for line in computed:
            print(line)
        return 0
    kept = [line for line in body if not line.startswith("bound ") and not line.startswith("verdict ")]
    if argv == ["--check"]:
        same = same_lines(body, computed)
        print("fits check lines=%d same=%s" % (len(computed), "yes" if same else "no"))
        return 0 if same else 1
    lines = text.split("\n")
    new = lines[: fences[0] + 1] + kept + computed + lines[fences[1] :]
    with open(os.path.join(root, cells_path), "w", encoding="utf-8", newline="") as f:
        f.write("\n".join(new))
    print("fits write lines=%d" % len(computed))
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv[1:]))
    except Refusal as refusal:
        sys.stderr.write("REFUSED %s: %s\n" % (refusal.cause, refusal.detail))
        sys.exit(2)
