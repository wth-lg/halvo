"""The changed copies of store-3 that the verifier's Mac may not run to their end (they reach destroy_root or a
line that it calls). The list M is the builder's, word for word; the runner is the verifier's."""
import os
import subprocess
import sys
import time

S = "crates/halvo-capture/src/store/"
C = "crates/halvo-capture/src/custody/"
H = "store::hardening_tests::"
D = "store::durable::tests::"
B = "store::db::tests::"
M = [
    ("T1", "file_root.rs:133 destroy_every_root: a link at a root name is unlinked and its target kept", C + "file_root.rs",
     "        Ok(entry) if entry.kind == EntryKind::File => zero_fill(dir, name).is_ok(),",
     "        Ok(entry) if entry.kind != EntryKind::Dir => std::fs::OpenOptions::new().write(true).open(dir.path().join(name)).and_then(|mut f| f.write_all(&[0u8; 7])).is_ok(),",
     [H + "delete_all_writes_nothing_through_a_root_file_or_a_partial_root_file_that_is_a_link"], "apply"),
    ("T2", "file_root.rs:343 destroy_root_at: zeros made durable and a removal that fails answer Ok", C + "file_root.rs",
     "        Err(_) if scrubbed.zeros_durable => Ok(()),\n", "",
     [H + "a_roots_directory_at_0500_destroys_the_root_by_its_zeros_and_seals_the_handle"], "apply"),
    ("T3", "db.rs:133 KeyFile::at: a link at the parent's name is refused", S + "db.rs",
     "        let dir = Dir::open(parent).map_err(store_dir_error)?;\n        Ok(KeyFilePlace::in_dir(dir, name))",
     "        let parent = std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());\n        let dir = Dir::open(&parent).map_err(store_dir_error)?;\n        Ok(KeyFilePlace::in_dir(dir, name))",
     [H + "a_key_file_place_refuses_a_link_at_its_parents_name_and_reads_nothing_behind_it"], "run"),
    ("T4", "db.rs:205 KeyFilePlace::read: one test per answer of the table (the label row taken out)", S + "db.rs",
     "        if key_file.namespace != label.as_str() {", "        if false {",
     [H + "open_refuses_a_key_file_of_another_label_and_changes_no_file"], "apply"),
    ("T5", "db.rs:266 KeyFilePlace::publish: an entry at the name answers AlreadyExists and stays", S + "db.rs",
     "            .link(&self.temp, &self.name)", "            .rename(&self.temp, &self.name)",
     [H + "a_key_file_is_published_create_only_and_an_entry_at_its_name_stays"], "apply"),
    ("T6", "db.rs:301 KeyFilePlace::destroy: a link is counted in not_followed", S + "db.rs",
     "            Ok(entry) if entry.kind != EntryKind::File => 1,", "            Ok(entry) if entry.kind != EntryKind::File => 0,",
     [H + "delete_all_answers_a_fifo_at_store_key_without_a_wait_and_removes_it"], "apply"),
    ("T7", "db.rs:328 zero_and_remove: a link's target keeps its bytes", S + "db.rs",
     "        if entry.kind == EntryKind::File {\n            let _ = zero_fill(dir, name);",
     "        if entry.kind != EntryKind::Dir {\n            let _ = std::fs::OpenOptions::new().write(true).open(dir.path().join(name)).and_then(|mut f| f.write_all(&[0u8; 7]));",
     [H + "delete_all_unlinks_a_store_key_that_is_a_link_and_writes_nothing_through_it"], "apply"),
    ("T8", "db.rs:435 open_connection: the look stands before the open (seam tests)", S + "db.rs",
     "    failpoint::seam(SEAM_BEFORE_THE_LOOK);\n    look_at_the_database_files(store_dir)?;\n",
     "    failpoint::seam(SEAM_BEFORE_THE_LOOK);\n",
     [H + "open_and_create_answer_a_link_a_directory_or_a_fifo_at_each_database_name_within_a_bound"], "apply"),
    ("T9", "db.rs:536 choose_the_name: a table test over every case", S + "db.rs",
     "            VolumeIds::Yes if handle_away => NameChoice::Refused,\n", "",
     [H + "a_no_such_name_answer_chooses_the_kept_name_only_on_a_volume_that_reads_no_both_times"], "run"),
    ("T10", "db.rs:818 after_a_code_of: a look's finding beside a forced code is answered", S + "db.rs",
     "        Err(found) => found,\n        Ok(()) => code_answer(code),", "        Err(_) => code_answer(code),\n        Ok(()) => code_answer(code),",
     [H + "at_the_seam_after_the_last_look_a_moved_directory_a_link_and_a_directory_answer_obstructed"], "apply"),
    ("T11", "db.rs:841 code_answer: a table over 5, 6, 11, 13, 26, another code and None", S + "db.rs",
     "        Some(rusqlite::ffi::SQLITE_BUSY | rusqlite::ffi::SQLITE_LOCKED) => StoreError::Locked,",
     "        Some(rusqlite::ffi::SQLITE_BUSY) => StoreError::Locked,",
     [B + "the_code_table_reads_each_primary_code_and_no_code"], "run"),
    ("T12", "db.rs:868 look_at_the_database_files: a link and a FIFO at each of the four names", S + "db.rs",
     "            Ok(entry) if entry.kind == EntryKind::Link => return Err(obstructed(name, Met::Link)),\n            Ok(_) => return Err(obstructed(name, Met::OtherKind)),",
     "            Ok(entry) if entry.kind == EntryKind::Link => return Err(obstructed(name, Met::Link)),\n            Ok(_) => {}",
     [H + "open_and_create_answer_a_link_a_directory_or_a_fifo_at_each_database_name_within_a_bound"], "apply"),
    ("T13", "db.rs:889 db_error: the error's text holds no text of the SQLite message", S + "db.rs",
     "        rusqlite::Error::SqliteFailure(failure, _) => StoreError::Db(failure.extended_code),",
     "        rusqlite::Error::SqliteFailure(_, Some(text)) => StoreError::Corrupt(text),\n        rusqlite::Error::SqliteFailure(failure, None) => StoreError::Db(failure.extended_code),",
     [B + "the_text_of_a_database_error_holds_no_text_of_the_sqlite_message"], "run"),
    ("T14", "db.rs:965 clear_pending: Absent, Root(same), Root(other), Unknown", S + "db.rs",
     "        if keyed != KeyNames::Root(pending.root_id) {", "        if true {",
     [H + "an_open_beside_a_create_pending_of_its_own_root_keeps_the_root_and_removes_create_pending"], "apply"),
    ("T15", "db.rs:1014 Db::destroy_through: a link at one side-file name, the others still removed", S + "db.rs",
     "                Ok(entry) if entry.kind != EntryKind::File => removal.not_followed += 1,\n                _ => {}\n            }",
     "                Ok(entry) if entry.kind != EntryKind::File => {\n                    removal.not_followed += 1;\n                    break;\n                }\n                _ => {}\n            }",
     [H + "delete_all_unlinks_a_link_at_one_database_side_file_and_still_removes_the_others"], "apply"),
    ("T16", "db.rs:1062 refuse_a_live_holder: a held store answers Locked before its custody is called", S + "db.rs",
     "        Db::refuse_a_live_holder(&store)?;\n        let custody = custody_for(&key_file.locator(roots))?;\n        let started = Instant::now();\n        let store_key = custody.unwrap_store_key(&wrapped, KeyLabel::STORE)?;\n",
     "        let custody = custody_for(&key_file.locator(roots))?;\n        let started = Instant::now();\n        let store_key = custody.unwrap_store_key(&wrapped, KeyLabel::STORE)?;\n        Db::refuse_a_live_holder(&store)?;\n",
     [H + "a_held_store_answers_locked_before_its_custody_is_called"], "apply"),
    ("T17", "db.rs:1204 Db::log_drained: a foreign log with bytes answers DatabaseOutsideTheDirectory", S + "db.rs",
     "            Ok(_) => Err(obstructed(StoreName::Log, Met::DatabaseOutsideTheDirectory)),\n            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),",
     "            Ok(_) => Ok(()),\n            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),",
     [H + "an_old_log_with_bytes_in_the_directory_while_sqlite_holds_another_log_is_refused"], "apply"),
    ("T18", "db.rs:1233 Db::log_look: a log missing after the first write", S + "db.rs",
     "            Err(e) if e.kind() == ErrorKind::NotFound => {\n                Err(obstructed(StoreName::Log, Met::DatabaseOutsideTheDirectory))\n            }",
     "            Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),",
     [H + "an_open_whose_log_is_unlinked_before_its_first_write_answers_obstructed_at_the_second_instant"], "apply"),
    ("T19", "db.rs:1254 Db::page_look: an empty store.db after the checkpoint", S + "db.rs",
     "            Ok(entry) if entry.kind == EntryKind::File && entry.len >= page => Ok(()),",
     "            Ok(entry) if entry.kind == EntryKind::File => Ok(()),",
     [H + "a_create_whose_database_is_cut_to_nothing_before_the_page_look_answers_obstructed"], "apply"),
    ("T20", "db.rs:1623 held_look: one test per failing condition (the moved answer taken out)", S + "db.rs",
     "    if moved != 0 {", "    if moved == i32::MAX {",
     [H + "a_database_file_swapped_at_its_name_after_sqlite_opened_it_is_refused"], "apply"),
    ("T21", "db.rs:1708 EncryptedStore::keyed: writes after the root step answer Sealed", S + "db.rs",
     "        self.keyed = Keyed::Deleting(None);\n        let mut report = remove_the_store_files(&self.dir, None, root_step.0);",
     "        let mut report = remove_the_store_files(&self.dir, None, root_step.0);",
     [H + "a_roots_directory_at_0500_destroys_the_root_by_its_zeros_and_seals_the_handle"], "apply"),
    ("T22", "db.rs:1961 EncryptedStore::answer: 1802, 14 and SegmentMissing with still_named true and false", S + "db.rs",
     "            StoreError::Db(rusqlite::ffi::SQLITE_IOERR_FSTAT | rusqlite::ffi::SQLITE_CANTOPEN)",
     "            StoreError::Db(rusqlite::ffi::SQLITE_IOERR_FSTAT)",
     [H + "the_answer_of_a_handle_takes_a_gone_directory_code_as_obstructed_only_when_the_name_is_gone"], "apply"),
    ("T23", "db.rs:2038 EncryptedStore::root_step: a custody destroy that fails still sweeps the other roots", S + "db.rs",
     "            Some(roots) => file_root::destroy_every_root(roots, Some(named)),\n            None => RootsLeft {",
     "            Some(roots) if destroyed.is_ok() => file_root::destroy_every_root(roots, Some(named)),\n            _ => RootsLeft {",
     [H + "a_root_step_whose_custody_destroy_fails_still_destroys_every_root_file_under_its_roots"], "apply"),
    ("T24", "db.rs:2146 remove_what_stands_at: a link at the store name, its target survives", S + "db.rs",
     "                parent.remove_entry(name)?;\n                parent.sync()",
     "                let _ = std::fs::canonicalize(store_dir).and_then(std::fs::remove_dir_all);\n                parent.remove_entry(name)?;\n                parent.sync()",
     [H + "the_delete_tail_removes_a_link_at_the_store_name_and_every_root_and_follows_nothing"], "apply"),
    ("T25", "db.rs:2221 dir_size: a link to a large file outside", S + "db.rs",
     "                EntryKind::File => *total += entry.len,\n                EntryKind::Dir => pending.push((parent.cloned(), entry.name)),\n                EntryKind::Link | EntryKind::Other => {}",
     "                EntryKind::File | EntryKind::Link => *total += entry.len,\n                EntryKind::Dir => pending.push((parent.cloned(), entry.name)),\n                EntryKind::Other => {}",
     [H + "the_size_of_a_store_counts_nothing_behind_a_link_to_a_large_file"], "apply"),
    ("T26", "durable.rs:114 Dir::remove_entry: a full directory is refused, a link to a directory unlinked", S + "durable.rs",
     "            EntryKind::Dir => self.remove_dir(name),\n            EntryKind::Link => match self.remove_file(name) {",
     "            EntryKind::Dir => std::fs::remove_dir_all(self.path().join(name)),\n            EntryKind::Link => match self.remove_file(name) {",
     [D + "remove_entry_refuses_a_directory_with_entries_and_unlinks_a_link_to_a_directory"], "apply"),
    ("T27", "durable.rs:128 Dir::remove_tree: a link inside the tree is unlinked and its target kept", S + "durable.rs",
     "                Err(e) if refusal(&e) == Some(Refused::Link) => {}\n",
     "                Err(e) if refusal(&e) == Some(Refused::Link) => {\n                    let _ = std::fs::canonicalize(self.path().join(name)).and_then(std::fs::remove_dir_all);\n                }\n",
     [D + "remove_tree_unlinks_a_link_inside_the_tree_and_keeps_its_target"], "run"),
    ("T27b", "durable.rs:128 Dir::remove_tree: a link that stands inside the tree is unlinked and its target kept (a copy that follows it)", S + "durable.rs",
     "        if entry.kind == EntryKind::Dir {\n            match self.open_dir(name) {",
     "        if entry.kind == EntryKind::Link {\n            let _ = std::fs::canonicalize(self.path().join(name)).and_then(std::fs::remove_dir_all);\n        }\n        if entry.kind == EntryKind::Dir {\n            match self.open_dir(name) {",
     [D + "remove_tree_unlinks_a_link_inside_the_tree_and_keeps_its_target"], "run"),
    ("T28", "durable.rs:447 Dir::open_file_for_write: a link and a FIFO are refused without a wait", S + "durable.rs",
     "            let flags = libc::O_WRONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC;",
     "            let flags = libc::O_WRONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC;",
     [H + "delete_all_answers_a_fifo_at_store_key_without_a_wait_and_removes_it"], "apply"),
    ("T29", "durable.rs:475 Dir::link: an existing `to` answers AlreadyExists and stays", S + "durable.rs",
     "                libc::linkat(\n                    fd,\n                    from.as_ptr(),\n                    fd,\n                    to.as_ptr(),\n                    LINK_THE_ENTRY_NOT_ITS_TARGET,\n                )",
     "                libc::renameat(fd, from.as_ptr(), fd, to.as_ptr())",
     [H + "a_key_file_is_published_create_only_and_an_entry_at_its_name_stays"], "apply"),
    ("T31", "lock.rs:14 DirLock: a duplicate of the descriptor keeps no lock after the drop", S + "lock.rs",
     "        let _ = self.file.unlock();", "        let _ = &self.file;",
     [B + "a_dropped_directory_lock_is_released_while_a_duplicate_of_its_descriptor_stays_open"], "run"),
    ("T32", "segment.rs:160 remove_segments_through: a link to a directory in segments is unlinked and counted", S + "segment.rs",
     "        removal.links_or_others_met += 1;\n", "",
     [H + "delete_all_unlinks_a_link_to_a_directory_in_segments_and_counts_it_as_not_followed"], "apply"),
    ("T33", "segment.rs:219 sweep_segment_files: a failure on one file still removes the rest", S + "segment.rs",
     "                    Err(e) => {\n                        sweep.first_failure.get_or_insert(e);\n                    }",
     "                    Err(e) => return Err(e),",
     [H + "a_sweep_whose_first_removal_fails_still_removes_the_other_keyless_file"], "apply"),
    ("T34", "segment.rs:534 remove_written_segment: a link at the shard answers Ok and removes nothing", S + "segment.rs",
     "    let removed = match shard {\n        Ok(shard) => shard.remove_file(&names.file),\n        Err(e) => Err(e),\n    };",
     "    drop(shard);\n    let removed = std::fs::remove_file(store_dir.path().join(SEGMENTS).join(&names.shard).join(&names.file));",
     [H + "removing_a_written_segment_through_a_shard_that_is_a_link_removes_nothing_and_answers_ok"], "run"),
    ("T35", "roots.rs:40 directory_of: a custody made before RootGuard::new gets the same directory", S + "test_support/roots.rs",
     "    roots\n        .entry(root_id)\n        .or_insert_with(|| {\n            let dir = roots_base_outside_the_checked_temp_dir().join(root_id.to_string());",
     "    roots.remove(&root_id);\n    roots\n        .entry(root_id)\n        .or_insert_with(|| {\n            let dir = roots_base_outside_the_checked_temp_dir().join(format!(\"{root_id}-{}\", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));",
     [H + "a_custody_made_before_its_root_guard_shares_the_guards_roots_directory"], "apply"),
    ("P22", "part 2 (2): the probe's busy timeout answers a bare code again", S + "db.rs",
     "        timeout.map_err(|e| after_a_code(store_dir, &e))?;",
     "        timeout.map_err(db_error)?;",
     [H + "the_live_holder_probe_and_the_held_look_answer_a_code_by_its_class_and_never_a_bare_code"], "apply"),
    ("P23", "part 2 (3): a root that a failed create_root published stays", C + "file_root.rs",
     "        if let Err(e) = finished {\n            let published_root = scrub_and_remove(&dir, &root_name(root_id));\n            let _ = scrub_and_remove(&dir, &partial);\n            let _ = dir.sync();\n            return Err(root_io(published_root.removed.err().unwrap_or(e)));\n        }",
     "        finished.map_err(root_io)?;",
     ["custody::file_root::tests::a_create_root_whose_step_after_the_link_fails_leaves_no_root_file_of_its_id"], "run"),
    ("P24", "part 2 (4): RootId prints the hyphenated form again", C + "mod.rs",
     "        write!(f, \"RootId({})\", self.0.simple())",
     "        write!(f, \"RootId({})\", self.0)",
     [H + "a_delete_report_and_its_roots_left_print_each_root_id_in_its_one_text_form"], "run"),
    ("R16", "durable.rs:506 Dir::entry_identity: a link answers EntryKind::Link", S + "durable.rs",
     "            Ok(EntryIdentity {\n                kind: kind(&st),",
     "            Ok(EntryIdentity {\n                kind: if kind(&st) == EntryKind::Link { EntryKind::File } else { kind(&st) },",
     [D + "the_identity_of_an_entry_that_is_a_link_reads_link_and_names_the_link_itself"], "run"),
]


# The runner of verifier-128 for the hosted macos job. Each copy is applied in the checkout, its named
# tests run by cargo to their end, and the file is restored with git checkout. A copy is CAUGHT only
# when a named test's own line reads FAILED; the lines of the failing assertion are printed.
# usage: python3 hosted_copies.py <checkout> <start epoch file> [--dry]
wt = sys.argv[1]
dry = "--dry" in sys.argv
start = time.time()
if len(sys.argv) > 2 and os.path.exists(sys.argv[2]):
    start = float(open(sys.argv[2]).read().strip())
LAST_START = 25 * 60
ONE_COPY = 300
env = dict(os.environ, CARGO_TARGET_DIR=os.path.join(os.path.abspath(wt), "target", "r1-gate"), RUST_TEST_THREADS="1")
counts = {}
for key, line, path, old, new, tests, mode in M:
    if mode != "apply":
        continue
    full = os.path.join(wt, path)
    text = open(full, encoding="utf-8").read()
    count = text.count(old)
    if count != 1:
        print(f"COPY {key} NOT-APPLIED occurrences={count} :: {line}", flush=True)
        counts["NOT-APPLIED"] = counts.get("NOT-APPLIED", 0) + 1
        continue
    if dry:
        print(f"COPY {key} APPLIES :: {line}", flush=True)
        counts["APPLIES"] = counts.get("APPLIES", 0) + 1
        continue
    if time.time() - start > LAST_START:
        print(f"COPY {key} NOT-RUN the job's time is used up :: {line}", flush=True)
        counts["NOT-RUN"] = counts.get("NOT-RUN", 0) + 1
        continue
    began = time.time()
    open(full, "w", encoding="utf-8").write(text.replace(old, new))
    try:
        done = subprocess.run(
            ["cargo", "test", "--locked", "-p", "halvo-capture", "--lib", "--no-fail-fast", "--", "--exact"] + tests,
            cwd=wt, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=ONE_COPY)
        out = done.stdout.decode("utf-8", "replace").splitlines()
        code = done.returncode
    except subprocess.TimeoutExpired as late:
        out = (late.stdout or b"").decode("utf-8", "replace").splitlines()
        code = "timeout"
    subprocess.run(["git", "checkout", "-q", "--", path], cwd=wt, check=True)
    failed = [l.split(" ")[1] for l in out if l.startswith("test ") and l.endswith(" ... FAILED")]
    passed = [l.split(" ")[1] for l in out if l.startswith("test ") and l.endswith(" ... ok")]
    result = [l for l in out if l.startswith("test result")]
    errors = [l for l in out if l.startswith("error")][:3]
    if code == "timeout":
        verdict = "TIMEOUT"
    elif not result:
        verdict = "NO-BUILD"
    elif failed:
        verdict = "CAUGHT"
    elif passed:
        verdict = "SURVIVED"
    else:
        verdict = "NO-TEST-RAN"
    counts[verdict] = counts.get(verdict, 0) + 1
    print(f"COPY {key} {verdict} seconds={time.time() - began:.0f} exit={code} failed={failed} passed={passed} {result} {errors} :: {line}", flush=True)
    for i, l in enumerate(out):
        if "panicked at" in l:
            print(f"  COPY {key} line: " + l[l.find("panicked at"):][:200], flush=True)
            for more in out[i + 1:i + 4]:
                if more.strip() and not more.startswith("note:"):
                    print(f"  COPY {key} line:   " + more[:400], flush=True)
clean = subprocess.run(["git", "status", "--porcelain"], cwd=wt, stdout=subprocess.PIPE).stdout.decode().strip()
print(f"COPIES {counts} status after the run: {clean or 'clean'}", flush=True)
