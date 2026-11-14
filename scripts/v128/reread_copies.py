"""Changed copies of verifier-128 for its reread of store-3 at eeb6a4c7 (ruling ST-46 K8), run on the hosted
macos job alone. Each copy is applied in the checkout, judged, and restored with git checkout. A copy is CAUGHT
only when a test's own line reads FAILED in the run's failures list.
usage: python3 reread_copies.py <checkout> <start epoch file> [--dry]"""
import os
import re
import subprocess
import sys
import time

wt = sys.argv[1]
dry = "--dry" in sys.argv
start = time.time()
if len(sys.argv) > 2 and os.path.exists(sys.argv[2]):
    start = float(open(sys.argv[2]).read().strip())
S = "crates/halvo-capture/src/store/"
C = "crates/halvo-capture/src/custody/"
H = "store::hardening_tests::"
KILL = H + "a_kill_before_each_file_system_call_of_each_operation_leaves_no_key_or_root_behind"
WHOLE = ["store::", "custody::"]
LOCK = (S + "lock.rs",
        "            Err(TryLockError::WouldBlock) => Err(StoreError::Locked),",
        "            Err(TryLockError::WouldBlock) => Ok(Self { file }),")
M = [
    ("HEAD-my-probes", [], [H + "v128r::", "--nocapture", "--test-threads=1"]),
    ("V1-a-link-in-the-roots-path-is-not-refused", [(S + "db.rs",
      "            Ok(meta) if meta.file_type().is_symlink() => return true,",
      "            Ok(meta) if meta.file_type().is_symlink() => {}")],
     [H + "v128r::v128r_issue_1", H + "pass_two::", "--nocapture"]),
    ("V1b-a-parent-part-in-the-roots-path-is-not-refused", [(S + "db.rs",
      "    if roots.components().any(|part| part == Component::ParentDir) {\n        return true;\n    }\n", "")],
     [H + "v128r::v128r_issue_1", H + "pass_two::"]),
    ("L44-the-tail-goes-on-after-locked", [(S + "db.rs",
      "            Err(StoreError::Locked) => return Err(StoreError::Locked),\n            Err(_) => None,",
      "            Err(_) => None,")], WHOLE),
    ("E3-lock-refusal-taken-as-the-lock", [LOCK], WHOLE),
    ("L47-unknown-key-file-does-not-stop-the-cleanup", [(S + "db.rs",
      "    if !present || keyed == KeyNames::Unknown {", "    if !present {")], WHOLE),
    ("E6-live-root-id-not-refused-at-the-look", [(S + "db.rs",
      "            if file_root::root_file_may_be_present(roots, custody.root_id()) {",
      "            if false && file_root::root_file_may_be_present(roots, custody.root_id()) {")], WHOLE),
    ("L61-unnamed-prints-yes", [(S + "mod.rs",
      "            RootReading::Unnamed => \"unknown\",", "            RootReading::Unnamed => \"yes\",")], WHOLE),
    ("G3-shred-clears-no-key", [(S + "expiry.rs",
      "                \"UPDATE segment_keys SET wrapped_key = NULL WHERE segment_id = ?1\",",
      "                \"UPDATE segment_keys SET wrapped_key = wrapped_key WHERE segment_id = ?1\",")],
     ["--exact", KILL, "--nocapture"]),
    ("G2b-mark-stays-owed-after-the-root", [(S + "db.rs",
      "        if let Err(e) = write_pending(dir, custody.as_ref(), PendingReading::Made) {",
      "        if let Err(e) = write_pending(dir, custody.as_ref(), PendingReading::Owed) {")],
     ["--exact", KILL]),
    ("L65-partial-name-of-the-right-length-is-a-witness", [(C + "file_root.rs",
      "            matches!(file, RootFile::Root(_)) && of_named(file) && can_be_the_root_file(entry)",
      "            of_named(file) && can_be_the_root_file(entry)")],
     [H + "pass_two::", H + "closed_list::"]),
    ("L48-pending-that-stays-not-counted", [(S + "db.rs",
      "        files_left: segments.entries_left + pending_left,", "        files_left: segments.entries_left,")],
     [H + "pass_two::", H + "closed_list::", "an_open_whose_cleanup", "create_pending"]),
]
LAST_START = 44 * 60
env = dict(os.environ, CARGO_TARGET_DIR=os.path.join(os.path.abspath(wt), "target", "r1-gate"))
counts = {}
for key, edits, args in M:
    texts, bad = {}, None
    for path, old, new in edits:
        text = texts.get(path) or open(os.path.join(wt, path), encoding="utf-8").read()
        if text.count(old) != 1:
            bad = f"{path}: occurrences={text.count(old)}"
            break
        texts[path] = text.replace(old, new)
    if bad:
        print(f"COPY {key} NOT-APPLIED {bad}", flush=True)
        counts["NOT-APPLIED"] = counts.get("NOT-APPLIED", 0) + 1
        continue
    if dry:
        print(f"COPY {key} APPLIES", flush=True)
        continue
    if time.time() - start > LAST_START:
        print(f"COPY {key} NOT-RUN the job's time is used up", flush=True)
        counts["NOT-RUN"] = counts.get("NOT-RUN", 0) + 1
        continue
    for path, text in texts.items():
        open(os.path.join(wt, path), "w", encoding="utf-8").write(text)
    began = time.time()
    try:
        done = subprocess.run(["cargo", "test", "--locked", "-p", "halvo-capture", "--lib", "--no-fail-fast", "--"] + args,
                              cwd=wt, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=900)
        out, code = done.stdout.decode("utf-8", "replace").splitlines(), done.returncode
    except subprocess.TimeoutExpired as late:
        out, code = (late.stdout or b"").decode("utf-8", "replace").splitlines(), "timeout"
    if texts:
        subprocess.run(["git", "checkout", "-q", "--"] + list(texts), cwd=wt, check=True)
    result = [l for l in out if l.startswith("test result")]
    counted = re.match(r"^test result: \w+\. (\d+) passed; (\d+) failed", result[0]) if result else None
    n_passed, n_failed = (int(counted.group(1)), int(counted.group(2))) if counted else (0, 0)
    failed = []
    if "failures:" in out:
        last = len(out) - 1 - out[::-1].index("failures:")
        failed = [l.strip() for l in out[last + 1:] if re.match(r"^    [a-z_0-9:]+$", l)]
    verdict = "TIMEOUT" if code == "timeout" else "NO-BUILD" if not result else "CAUGHT" if n_failed else "SURVIVED" if n_passed else "NO-TEST-RAN"
    if not edits:
        verdict = "HEAD-" + ("PASSES" if n_passed and not n_failed else "FAILS")
    counts[verdict] = counts.get(verdict, 0) + 1
    print(f"COPY {key} {verdict} seconds={time.time() - began:.0f} exit={code} passed={n_passed} failed={n_failed} {result[:1]}", flush=True)
    for name in failed[:40]:
        print(f"  COPY {key} failed: {name}", flush=True)
    if verdict == "NO-BUILD":
        for l in [l for l in out if l.startswith("error")][:4]:
            print(f"  COPY {key} build: {l[:300]}", flush=True)
    shown = 0
    for i, l in enumerate(out):
        if "panicked at" in l and shown < 8:
            shown += 1
            print(f"  COPY {key} line: " + l[l.find("panicked at"):][:170], flush=True)
            for more in out[i + 1:i + 3]:
                if more.strip() and not more.startswith("note:"):
                    print(f"  COPY {key} line:   " + more[:520], flush=True)
    for l in out:
        if re.search(r"V128R |KILL (rows=|op=shred unkilled|op=\w+ calls)|KILL op=\w+ before_call=\d+ exit=\d+ states=EndStates \{.*: false.*named-limit", l) and (not edits or key.startswith(("V1", "G"))):
            print(f"  COPY {key} | " + l[l.find("V128R ") if "V128R " in l else l.find("KILL "):][:900], flush=True)
clean = subprocess.run(["git", "status", "--porcelain"], cwd=wt, stdout=subprocess.PIPE).stdout.decode().strip()
print(f"COPIES {counts} status after the run: {clean or 'clean'}", flush=True)
