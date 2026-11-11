"""usage: python3 -I stamp-cells.py WORKTREE EVIDENCE OUT

Writes the records of every cell that was measured on this Mac into the new
directory OUT, one file per cell in file order, and OUT/sources.tsv, which
names the helper record line of each cell. The deciding cell of Apple Vision
at the accurate level follows the coordinator's ruling of 2026-11-10 20:43Z:
the three sets of the one quiet helper call when that call ran; the runs of
the first set and of the repeat sets 2 to 4 when it exited 75; and the same
runs, marked pending, while it still waits.
"""
import json
import pathlib
import statistics
import subprocess
import sys

if len(sys.argv) != 4:
    sys.exit(__doc__)
worktree, evidence, out = (pathlib.Path(p) for p in sys.argv[1:4])
if out.exists():
    sys.exit("stamp-cells: %s exists. Fix: name a new directory." % out)
(out / "raw").mkdir(parents=True)
local = evidence / "local"
binary = worktree / "tools" / "r2-probes" / "target" / "release" / "r2-probes"
system = "%s-%s" % tuple(
    subprocess.run(["sw_vers", flag], capture_output=True, text=True, check=True).stdout.strip()
    for flag in ("-productVersion", "-buildVersion")
)
sources = []


def helper_line(log, number):
    fields = log.read_text().splitlines()[number - 1].split("\t")
    if fields[4] != "0":
        sys.exit("stamp-cells: line %d of %s has exit code %s" % (number, log, fields[4]))
    return {"start": fields[0], "end": fields[1], "before": fields[5], "after": fields[7], "log": log, "number": number}


def calls(raw):
    values = []
    for line in raw.read_text().splitlines():
        answer = json.loads(line)
        if "ok" not in answer:
            sys.exit("stamp-cells: %s holds a run without a record" % raw)
        values.append(next(f["value"] for f in answer["ok"]["fields"] if f["key"] == "call_ms"))
    return values


def peak(directories):
    best = 0
    for directory in directories:
        for path in sorted(directory.glob("time-*.txt")):
            for line in path.read_text().splitlines():
                if "maximum resident set size" in line:
                    best = max(best, int(line.split()[0]))
    if best > 8_000_000_000:
        sys.exit("stamp-cells: a peak of %d bytes passes 8,000,000,000" % best)
    return best


def stamp(order, cell, directories, lines, version, extra):
    raw = out / "raw" / ("%s.jsonl" % order)
    raw.write_text("".join((d / "raw.jsonl").read_text() for d in directories))
    command = [
        str(binary), "stamp", "--in", str(raw), "--out", str(out / ("%s.jsonl" % order)), "--cell", cell,
        "--text", "version=%s" % version,
        "--text", "start_utc=%s" % lines[0]["start"], "--text", "end_utc=%s" % lines[-1]["end"],
        "--text", "load_before_1min=%s" % lines[0]["before"], "--text", "load_after_1min=%s" % lines[-1]["after"],
        "--number", "peak_rss_bytes=%d" % peak(directories), "--text", "peak_tool=/usr/bin/time_-l",
    ]
    for key, value in extra:
        command += ["--text", "%s=%s" % (key, value)]
    subprocess.run(command, check=True, capture_output=True)
    where = "; ".join("line %d of %s (start %s, end %s, load %s then %s)" % (l["number"], l["log"], l["start"], l["end"], l["before"], l["after"]) for l in lines)
    sources.append((cell, "this Mac, helper record %s" % where, str(out / ("%s.jsonl" % order))))
    return calls(raw)


def spread(directories):
    medians = [statistics.median(calls(d / "raw.jsonl")) for d in directories]
    everything = [value for d in directories for value in calls(d / "raw.jsonl")]
    extra = [("set_%d_ms" % (index + 1), "%.1f" % median) for index, median in enumerate(medians)]
    return extra + [("run_min_ms", "%.1f" % min(everything)), ("run_max_ms", "%.1f" % max(everything))]


accurate = "engine.macos-15.apple-vision-accurate.x1.0.this-mac"
first = local / "final" / "apple-vision-accurate"
first_line = helper_line(local / "final" / "helper-apple-vision-accurate.tsv", 1)
repeats = [local / "repeat-42889376963" / ("accurate-%d" % k) for k in (2, 3, 4)]
repeat_lines = [helper_line(local / "repeat-42889376963" / "helper-accurate.tsv", k - 1) for k in (2, 3, 4)]
quiet = local / "quiet"
quiet_out = (quiet / "call.out").read_text() if (quiet / "call.out").exists() else ""
if "helper exit=0" in quiet_out:
    sets = [quiet / "sets" / ("set-%d" % k) for k in (1, 2, 3)]
    state = stamp("10-accurate", accurate, sets, [helper_line(quiet / "helper.tsv", 1)], system, spread(sets) + [("quiet_run", "yes")])
elif "helper exit=75" in quiet_out:
    sets = [first] + repeats
    state = stamp("10-accurate", accurate, sets, [first_line] + repeat_lines, system, spread(sets) + [("quiet_run", "none")])
elif "helper exit" in quiet_out:
    sys.exit("stamp-cells: the quiet call ended with another status: %s" % quiet_out.splitlines()[-1])
else:
    sets = [first] + repeats
    state = stamp("10-accurate", accurate, sets, [first_line] + repeat_lines, system, spread(sets) + [("quiet_run", "pending")])
stamp("20-accurate-first-set", accurate + ".first-set", [first], [first_line], system, [])
for number, directory, line in zip((2, 3, 4), repeats, repeat_lines):
    stamp("2%d-accurate-repeat-%d" % (number - 1, number), "%s.repeat-%d" % (accurate, number), [directory], [line], system, [])
stamp("30-fast", "engine.macos-15.apple-vision-fast.x1.0.this-mac", [local / "final" / "apple-vision-fast"],
      [helper_line(local / "final" / "helper-apple-vision-fast.tsv", 1)], system, [])
stamp("40-pp", "engine.macos-15.pp-ocrv6-tiny.x1.0.this-mac", [local / "final" / "pp-ocrv6-tiny"],
      [helper_line(local / "final" / "helper-pp-ocrv6-tiny.tsv", 1)], "onnxruntime-1.28.0", [])
(out / "sources.tsv").write_text("".join("\t".join(row) + "\n" for row in sources))
print("cells=%d deciding runs=%d median=%.1f" % (len(sources), len(state), statistics.median(state)))
