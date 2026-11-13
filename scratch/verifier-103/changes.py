# Verifier 103, claim D: one-rule changes of my own on the head 93de14e1 of unit win-2.
# Written from the code of the head and the two rulings, before I opened the builder's rows.
# Each entry: id, group, file, the rule that the change breaks, the exact text taken out (old),
# the text put in (new), the tests of the head that must fail, and the assertion in each.
# A prediction "none" means: by my reading no test of the head holds the rule.
#
#   python3 -I changes.py list
#   python3 -I changes.py check <repo-root>        every old text is in its file exactly once
#   python3 -I changes.py apply <id> <repo-root>   write the changed copy (CRLF files keep CRLF)
#   python3 -I changes.py tests <id>               the named tests, one per line
import sys

W = "crates/halvo-capture/src/windows/"
T = "windows::tests::"
E1 = T + "packets_that_a_stopped_run_held_do_not_arrive_after_a_barrier_drawn_after_the_stop"
CLOSED = T + "a_closed_hand_off_takes_no_packet_while_its_forwarder_is_parked"
GONE2 = T + "a_gone_writer_closes_the_hand_off_and_counts_what_it_held"
GONE4 = T + "a_gone_writer_counts_the_queued_packets_and_the_pending_motion_sample"
RESTART = T + "packets_that_a_stopped_run_still_held_count_after_a_restart"
READ = T + "the_read_at_a_stop_takes_what_is_queued_and_ends_at_its_bound"
JOINED = T + "a_handle_whose_thread_was_joined_posts_nothing_on_drop"
DROP_ONCE = T + "a_dropped_source_whose_pump_does_not_end_waits_its_deadline_once"
DROP_NONE = T + "a_dropped_source_whose_stop_timed_out_waits_no_more"
FG = "windows::foreground::tests::a_foreground_hook_that_the_system_refuses_reports_the_os_code"
TI = "windows::window_title::tests::a_title_hook_that_the_system_refuses_reports_the_os_code"
KEPT = "windows::screen::tests::a_stop_whose_thread_cannot_be_spawned_keeps_the_session"
SCR_DROP = "windows::screen::tests::a_dropped_screen_source_whose_stop_timed_out_waits_no_more"
FUNC = "functional:keys_queued_before_a_stop_are_delivered_or_counted"

PUSH_CLOSED = r'''        if queue.closed {
            self.shared.counts.lose(1);
            return;
        }
        if queue.packets.len() < HANDOFF_CAPACITY {'''
MOTION_CLOSED = r'''        if queue.closed {
            self.shared.counts.lose(1);
            return;
        }
        if queue.motion.replace(packet).is_some() {'''
CLOSE = r'''        queue.closed = true;
        discard(&mut queue, &self.shared.counts);
'''
LET_GO = r'''        if self.join.take().is_some() {
            self.signal.set();
            (self.post)(self.id);
        }'''
SHUT = r'''        let stopped = pump.stop(INPUT_STOP_DEADLINE);
        if let Some(handoff) = &self.handoff {
            handoff.close();
        }
        if let Err(timeout @ CaptureError::Timeout { .. }) = &stopped {
            self.timed_out = Some(timeout.clone());
            return stopped;
        }
'''

CHANGES = [
    # ---- the rules of a closed hand-off
    dict(id="h01-close-sets-nothing", group="closed hand-off", file=W + "mod.rs",
         rule="close() refuses later pushes (it sets `closed`)",
         old=CLOSE, new=r'''        discard(&mut queue, &self.shared.counts);
''',
         tests=[CLOSED], assertion='assert_eq!(lost, 4, "each push into the closed hand-off is counted lost"): reads 0'),
    dict(id="h02-closed-push-not-counted", group="closed hand-off", file=W + "mod.rs",
         rule="a push into a closed queue adds one to lost",
         old=PUSH_CLOSED, new=r'''        if queue.closed {
            return;
        }
        if queue.packets.len() < HANDOFF_CAPACITY {''',
         tests=[CLOSED, GONE2], assertion="CLOSED: assert_eq!(lost, 4): reads 1; GONE2: assert_eq!(handoff.counts().lost, 2): reads 1"),
    dict(id="h03-closed-motion-push-not-counted", group="closed hand-off", file=W + "mod.rs",
         rule="a motion push into a closed queue adds one to lost",
         old=MOTION_CLOSED, new=r'''        if queue.closed {
            return;
        }
        if queue.motion.replace(packet).is_some() {''',
         tests=[CLOSED, GONE2], assertion="CLOSED: assert_eq!(lost, 4): reads 3; GONE2: assert_eq!(handoff.counts().lost, 2): reads 1"),
    dict(id="h04-closed-motion-push-pends", group="closed hand-off", file=W + "mod.rs",
         rule="a motion push into a closed queue leaves no pending sample",
         old=MOTION_CLOSED, new=r'''        if queue.motion.replace(packet).is_some() {''',
         tests=[CLOSED, GONE2], assertion='CLOSED: assert_eq!(lost, 4): reads 3 (and assert!(!pending)); GONE2: reads 1'),
    dict(id="h05-closed-push-counted-and-queued", group="closed hand-off", file=W + "mod.rs",
         rule="a push into a closed queue is not queued",
         old=PUSH_CLOSED, new=r'''        if queue.closed {
            self.shared.counts.lose(1);
        }
        if queue.packets.len() < HANDOFF_CAPACITY {''',
         tests=[CLOSED], assertion='assert_eq!(queued, 0, "a push into the closed hand-off was queued"): reads 3'),
    dict(id="h06-close-keeps-what-it-held", group="closed hand-off", file=W + "mod.rs",
         rule="a close takes out of the queue what the forwarder has not taken (E1)",
         old=CLOSE, new=r'''        queue.closed = true;
''',
         tests=[E1], assertion='assert_eq!(old_after.len(), 1, "... packets of the stopped run arrive after the barrier")'),
    dict(id="h07-close-drops-without-a-count", group="closed hand-off", file=W + "mod.rs",
         rule="a close counts what the queue still held as lost (E1)",
         old=CLOSE, new=r'''        queue.closed = true;
        queue.packets.clear();
        queue.motion = None;
''',
         tests=[E1, RESTART], assertion='E1: assert_eq!(lost_at_the_stop, 5): reads 0; RESTART: wait_until "the old forwarder did not count what it held"'),
    dict(id="h08-held-count-forgets-the-pending-sample", group="closed hand-off", file=W + "mod.rs",
         rule="the pending motion sample is among what a close or a gone writer counts",
         old=r'''    let held = queue.packets.len() + usize::from(queue.motion.is_some());''',
         new=r'''    let held = queue.packets.len();''',
         tests=[E1, GONE4], assertion="E1: assert_eq!(lost_at_the_stop, 5): reads 4; GONE4: assert_eq!(handoff.counts().lost, 4): reads 3"),
    dict(id="h09-gone-writer-counts-nothing", group="closed hand-off", file=W + "mod.rs",
         rule="a gone writer counts what the queue held",
         old=r'''            let mut queue = shared.lock();
            discard(&mut queue, &shared.counts);
            queue.closed = true;''',
         new=r'''            let mut queue = shared.lock();
            queue.closed = true;''',
         tests=[GONE4], assertion="assert_eq!(handoff.counts().lost, 4): reads 0"),
    dict(id="h10-gone-writer-leaves-the-queue-open", group="closed hand-off", file=W + "mod.rs",
         rule="a gone writer closes the queue, so that a later push is counted",
         old=r'''            discard(&mut queue, &shared.counts);
            queue.closed = true;
            queue.writer_gone = true;''',
         new=r'''            discard(&mut queue, &shared.counts);
            queue.writer_gone = true;''',
         tests=[GONE2], assertion="assert_eq!(handoff.counts().lost, 2): reads 0"),
    # ---- the OS code of a failed start
    dict(id="o01-os-code-always-zero", group="os code", file=W + "mod.rs",
         rule="an OS error carries the code that the system gave",
         old=r'''        code: i64::from(error.code().0),
''', new=r'''        code: 0,
''',
         tests=[FG, TI], assertion="assert_eq!(started, Err(refused_hook_error()))"),
    dict(id="o02-last-error-not-read", group="os code", file=W + "mod.rs",
         rule="last_os_error reads the thread's last error",
         old=r'''    os_error(stage, &windows::core::Error::from_thread())''',
         new=r'''    os_error(stage, &windows::core::Error::from(windows::core::HRESULT(0)))''',
         tests=[FG, TI], assertion="assert_eq!(started, Err(refused_hook_error()))"),
    dict(id="o03-title-code-read-before-the-call", group="os code", file=W + "window_title.rs",
         rule="the code is read after the call that failed (when it is read)",
         old=r'''    let hook = installer(EVENT_OBJECT_NAMECHANGE, Some(title_proc));
    if hook.is_invalid() {
        let error = super::last_os_error("SetWinEventHook");''',
         new=r'''    let error = super::last_os_error("SetWinEventHook");
    let hook = installer(EVENT_OBJECT_NAMECHANGE, Some(title_proc));
    if hook.is_invalid() {''',
         tests=[TI], assertion="assert_eq!(started, Err(refused_hook_error()))"),
    dict(id="o04-os-code-unsigned", group="os code", file=W + "mod.rs",
         rule="the code is the signed HRESULT",
         old=r'''        code: i64::from(error.code().0),
''', new=r'''        code: i64::from(error.code().0 as u32),
''',
         tests=[FG, TI], assertion="assert_eq!(started, Err(refused_hook_error()))"),
    dict(id="o05-raw-input-registration-code-zero", group="os code", file=W + "input.rs",
         rule="a refused RegisterRawInputDevices reports the system's code",
         old=r'''        ready.report(Err(super::os_error("RegisterRawInputDevices", &error)));''',
         new=r'''        let _ = &error;
        ready.report(Err(CaptureError::Os {
            stage: "RegisterRawInputDevices",
            domain: "HRESULT",
            code: 0,
        }));''',
         tests=[], assertion="none: by my reading no test of the head makes this step fail"),
    dict(id="o06-screen-stop-spawn-code-zero", group="os code", file=W + "screen.rs",
         rule="a stop thread that cannot start reports the io code",
         old=r'''                stage: "spawn the screen stop thread",
                domain: "io",
                code: i64::from(e.raw_os_error().unwrap_or(0)),
''',
         new=r'''                stage: "spawn the screen stop thread",
                domain: "io",
                code: 0,
''',
         tests=[KEPT], assertion="assert_eq!(first, Err(refused.clone())): code 8 expected"),
    dict(id="o07-foreground-code-fixed-nonzero", group="os code", file=W + "foreground.rs",
         rule="the foreground hook reports the code of its own refusal (where it is read)",
         old=r'''        let error = super::last_os_error("SetWinEventHook");
        FOREGROUND_CTX''',
         new=r'''        let error = super::os_error(
            "SetWinEventHook",
            &windows::core::Error::from(windows::core::HRESULT(-2147467259)),
        );
        FOREGROUND_CTX''',
         tests=[FG], assertion="assert_eq!(started, Err(refused_hook_error()))"),
    # ---- the read at a stop and its bound
    dict(id="s01-no-read-at-the-stop", group="read at a stop", file=W + "mod.rs",
         rule="the pump reads its queue before it returns (E2)",
         old=r'''    read_at_stop(STOP_READ_LIMIT);
}''', new=r'''}''',
         tests=[READ, FUNC], assertion="READ: assert_eq!(left, BEYOND): reads 4196; FUNC: delivered plus lost equals the presses"),
    dict(id="s02-read-at-the-stop-has-no-bound", group="read at a stop", file=W + "mod.rs",
         rule="the read at a stop ends at STOP_READ_LIMIT",
         old=r'''    for _ in 0..limit {
''', new=r'''    let _ = limit;
    loop {
''',
         tests=[READ], assertion="assert_eq!(left, BEYOND): reads 0"),
    dict(id="s03-read-at-the-stop-takes-and-drops", group="read at a stop", file=W + "mod.rs",
         rule="the read at a stop dispatches what it takes, so that it is delivered or counted",
         old=r'''            if msg.message != WM_QUIT {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
''', new=r'''            let _ = msg.message != WM_QUIT;
''',
         tests=[FUNC], assertion="FUNC: delivered plus lost equals the presses (the in-crate test posts thread messages, which a dispatch drops anyway: it must pass)"),
    dict(id="s04-no-read-at-the-stop-event", group="read at a stop", file=W + "mod.rs",
         rule="the read also happens when the pump ends at its stop event",
         old=r'''        if woke.0 != WAIT_OBJECT_0.0 + 1 {
            break;
        }''', new=r'''        if woke.0 != WAIT_OBJECT_0.0 + 1 {
            return;
        }''',
         tests=[], assertion="none in the crate: the in-crate test ends its pump at the quit message; FUNC may or may not take this path; my stop probe must show events in neither count"),
    dict(id="s05-no-read-at-the-quit-message", group="read at a stop", file=W + "mod.rs",
         rule="the read also happens when the pump ends at its quit message",
         old=r'''                if msg.message == WM_QUIT {
                    break 'pump;
                }''', new=r'''                if msg.message == WM_QUIT {
                    return;
                }''',
         tests=[READ], assertion="assert_eq!(left, BEYOND): reads 4196"),
    # ---- the drop that waits once; the drop after a join
    dict(id="d01-handle-drop-stops-again", group="drop", file=W + "mod.rs",
         rule="the drop of a pump handle whose stop was asked waits no more (W2-1 point 1)",
         old=r'''        if self.stop_requested {
            self.let_go();
        } else {
            let _ = self.stop(INPUT_STOP_DEADLINE);
        }''', new=r'''        let _ = self.stop(INPUT_STOP_DEADLINE);''',
         tests=[DROP_ONCE, DROP_NONE, JOINED], assertion="DROP_ONCE: waited < 1.5 deadlines; DROP_NONE: waited < 250 ms; JOINED: assert_eq!(after_timeout, 1)"),
    dict(id="d02-source-drop-stops-after-a-timeout", group="drop", file=W + "mod.rs",
         rule="a source whose shutdown answered Timeout is dropped without a wait (W2-1 point 1 b)",
         old=r'''        if self.timed_out.is_none() {
            let _ = self.shutdown();
        }''', new=r'''        let _ = self.shutdown();''',
         tests=[DROP_NONE], assertion='assert!(waited < Duration::from_millis(250), "the drop waited ...")'),
    dict(id="d03-drop-after-a-join-posts", group="drop", file=W + "mod.rs",
         rule="a handle whose thread was joined posts nothing (E4)",
         old=LET_GO, new=r'''        let _ = self.join.take();
        self.signal.set();
        (self.post)(self.id);''',
         tests=[JOINED], assertion='assert_eq!(after_ok, 0, "the drop after a stop that answered Ok posted ...")'),
    dict(id="d04-drop-after-a-timeout-posts-nothing", group="drop", file=W + "mod.rs",
         rule="after a Timeout the drop posts its quit message once (W2-1 point 1 b, kept by E4)",
         old=LET_GO, new=r'''        if self.join.take().is_some() {
            self.signal.set();
        }''',
         tests=[JOINED], assertion='assert_eq!(after_timeout, 1, "the drop after a stop that timed out posts its quit message once")'),
    # ---- the kept session; the hand-off of a stop
    dict(id="k01-refused-stop-drops-the-session", group="kept session", file=W + "screen.rs",
         rule="a stop that cannot start its thread keeps the session (E3)",
         old=r'''                Err((session, error)) => {
                    self.session = Some(session);
''', new=r'''                Err((session, error)) => {
                    drop(session);
''',
         tests=[KEPT], assertion='assert!(busy, "the source whose stop thread could not start would take a new begin")'),
    dict(id="k02-refusal-stays-after-a-later-stop", group="kept session", file=W + "screen.rs",
         rule="a stop that started clears the refusal, so that the next run reads Running",
         old=r'''                    self.stopping = Some(stopping);
                    self.stop_failed = None;
''', new=r'''                    self.stopping = Some(stopping);
''',
         tests=[], assertion="none: by my reading no test begins a run after a refused stop that later went through"),
    dict(id="k03-drop-does-not-try-the-stop-again", group="kept session", file=W + "screen.rs",
         rule="the drop tries a refused stop again (E3)",
         old=r'''        let _ = self.shutdown(deadline);
    }

    fn state(''', new=r'''        if self.stop_failed.is_none() {
            let _ = self.shutdown(deadline);
        }
    }

    fn state(''',
         tests=[KEPT], assertion='assert_eq!(STOP_SPAWNS.load(Ordering::SeqCst), 3, "the second shutdown and the drop each try the stop again")'),
    dict(id="k04-screen-shutdown-leaves-the-hand-off-open", group="kept session", file=W + "screen.rs",
         rule="the screen source closes its hand-off at shutdown (E1 for the screen source)",
         old=r'''        let stopped = self.life.shutdown(super::SCREEN_STOP_DEADLINE);
        if let Some(handoff) = &self.handoff {
            handoff.close();
        }
        stopped''', new=r'''        self.life.shutdown(super::SCREEN_STOP_DEADLINE)''',
         tests=[], assertion="none in the crate: no in-crate test drives ScreenSource::shutdown; a functional test may"),
    dict(id="k05-timed-out-stop-leaves-the-hand-off-open", group="kept session", file=W + "mod.rs",
         rule="a shutdown that answers Timeout closes the hand-off, so that the stuck pump's packets are counted and stay out of the sink",
         old=SHUT, new=r'''        let stopped = pump.stop(INPUT_STOP_DEADLINE);
        if let Err(timeout @ CaptureError::Timeout { .. }) = &stopped {
            self.timed_out = Some(timeout.clone());
            return stopped;
        }
        if let Some(handoff) = &self.handoff {
            handoff.close();
        }
''',
         tests=[], assertion="none: by my reading no test pushes a packet after a shutdown that timed out"),
    dict(id="k06-shutdown-leaves-the-hand-off-open", group="kept session", file=W + "mod.rs",
         rule="shutdown closes the hand-off before it returns (E1)",
         old=SHUT, new=r'''        let stopped = pump.stop(INPUT_STOP_DEADLINE);
        if let Err(timeout @ CaptureError::Timeout { .. }) = &stopped {
            self.timed_out = Some(timeout.clone());
            return stopped;
        }
''',
         tests=[E1], assertion='one of the assertions of E1 (old_after == ["old-1"], or lost_at_the_stop == 5: reads 0)'),
    dict(id="k07-screen-drop-waits-after-a-timeout", group="drop", file=W + "screen.rs",
         rule="a screen source whose stop timed out is dropped without a wait (W2-1 point 1 b)",
         old=r'''        if self.timed_out.is_some() && self.stopping.is_some() {
            self.stopping = None;
            return;
        }
''', new="",
         tests=[SCR_DROP], assertion='assert!(waited < Duration::from_millis(100), "the drop waited ...")'),
]


def find(cid):
    for c in CHANGES:
        if c["id"] == cid:
            return c
    sys.exit("no such change: " + cid)


def load(root, c):
    path = root.rstrip("/") + "/" + c["file"]
    with open(path, "rb") as f:
        data = f.read().decode("utf-8")
    old, new = c["old"], c["new"]
    if "\r\n" in data:
        old = old.replace("\n", "\r\n")
        new = new.replace("\n", "\r\n")
    return path, data, old, new


def main():
    if len(sys.argv) < 2:
        sys.exit("usage: changes.py list | check <root> | apply <id> <root> | tests <id>")
    cmd = sys.argv[1]
    if cmd == "list":
        for c in CHANGES:
            print("\t".join([c["id"], c["group"], c["file"], c["rule"], ",".join(c["tests"]) or "none", c["assertion"]]))
        print("changes=%d" % len(CHANGES))
    elif cmd == "ids":
        for c in CHANGES:
            print(c["id"])
    elif cmd == "check":
        bad = 0
        for c in CHANGES:
            _, data, old, _new = load(sys.argv[2], c)
            n = data.count(old)
            if n != 1:
                bad += 1
            print("%s\tfound=%d" % (c["id"], n))
        print("check changes=%d not_exactly_once=%d" % (len(CHANGES), bad))
        sys.exit(1 if bad else 0)
    elif cmd == "apply":
        c = find(sys.argv[2])
        path, data, old, new = load(sys.argv[3], c)
        if data.count(old) != 1:
            print("NOT APPLIED %s found=%d" % (c["id"], data.count(old)))
            sys.exit(1)
        with open(path, "wb") as f:
            f.write(data.replace(old, new).encode("utf-8"))
        print("applied %s to %s" % (c["id"], c["file"]))
    elif cmd == "tests":
        for t in find(sys.argv[2])["tests"]:
            print(t)
    elif cmd == "file":
        print(find(sys.argv[2])["file"])
    else:
        sys.exit("unknown command")


main()
