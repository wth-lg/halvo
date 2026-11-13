//! The command `probe` of contract T13.3: N picks, N admissions and N steps of the spend machine on
//! the release data, with no allocation per round, so that the peak of its process bounds what the
//! router adds to a process that links it.

#![deny(unsafe_code)]

use std::hint::black_box;
use std::io::Write;

use halvo_route::admit::{admit, estimate, Room};
use halvo_route::cap::DefaultCap;
use halvo_route::pick::{pick, Pick};
use halvo_route::policy::{Policy, CELL_COUNT};
use halvo_route::prices::Prices;
use halvo_route::release;
use halvo_route::ruling::Ruling;
use halvo_route::spend::{step, Account, Hold, Move};
use halvo_route::tier::{CallKind, CategoryId, Tier};

/// What a round reads: a ruling, a policy table, a default cap and prices.
pub struct Data<'a> {
    pub ruling: Ruling,
    pub policy: &'a Policy,
    pub cap: Option<DefaultCap>,
    pub prices: &'a Prices,
}

impl Data<'static> {
    /// The compiled-in data of `halvo_route::release` and `Prices::release()`.
    pub fn release() -> Data<'static> {
        Data {
            ruling: release::ruling(),
            policy: release::policy(),
            cap: release::cap(),
            prices: Prices::release(),
        }
    }
}

const BODY_BYTES: u64 = 4096;
const MAX_OUTPUT_TOKENS: u64 = 1024;

/// What `run` made: the number of rounds and whether every round held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Probe {
    pub rounds: u64,
    pub ok: bool,
}

/// Makes `rounds` rounds on `data`, round `i` from 0: the pick of the cell `i` mod 44 of
/// `halvo_route::policy::cells()` under the ceiling `Tier::ALL[i mod 3]`; the admission of a task
/// of that cell's category, with the estimate of `admit::estimate`, the room that the cap and the
/// spend machine's account give and one running task of the same estimate; and one step of the
/// spend machine on one hold of the low-cost tier's amount for 4,096 body bytes and 1,024 output
/// tokens, which goes `reserve`, `dispatch`, `settle` at that amount and starts a new hold on a new
/// account every third round. It allocates nothing. It returns the number of rounds that it made,
/// and `ok` true when no pick names a tier that the ruling does not allow and every step goes
/// through.
pub fn run(data: &Data<'_>, rounds: u64) -> Probe {
    let fresh = Account {
        cap_minor: data.cap.map(|cap| cap.amount),
        active: true,
        spent: 0,
        held: 0,
    };
    let amount = data
        .prices
        .hold(Tier::LowCostHosted, BODY_BYTES, MAX_OUTPUT_TOKENS);
    let mut account = fresh;
    let mut hold = Hold::new();
    let mut ok = true;
    let mut made = 0;
    for round in 0..rounds {
        let round = black_box(round);
        let cell = (round % CELL_COUNT as u64) as usize;
        let category = CategoryId::ALL[cell / CallKind::ALL.len()];
        let kind = CallKind::ALL[cell % CallKind::ALL.len()];
        let ceiling = Tier::ALL[(round % Tier::ALL.len() as u64) as usize];
        if let Pick::Tier { tier, .. } = black_box(pick(data.policy, category, kind, ceiling)) {
            ok &= data.ruling.allowed().contains(&tier);
        }
        let room = data.cap.map(|cap| Room {
            cap_minor: cap.amount,
            spent: account.spent,
            held: account.held,
            period_end_ms: 0,
        });
        let estimate = estimate(data.policy, category);
        black_box(admit(&data.ruling, room.as_ref(), estimate, &[estimate]));
        let mv = match round % 3 {
            0 => {
                account = fresh;
                hold = Hold::new();
                Move::Reserve { amount }
            }
            1 => Move::Dispatch,
            _ => Move::Settle { cost: amount },
        };
        match black_box(step(&account, &hold, mv)) {
            Ok((next_account, next_hold)) => {
                account = next_account;
                hold = next_hold;
            }
            Err(_) => ok = false,
        }
        made += 1;
    }
    Probe { rounds: made, ok }
}

/// The command `route-gate probe --rounds N` on the release data. It prints `probe rounds=<N>
/// ok=<yes|no>` and returns 0 for `ok=yes` and 1 for `ok=no` or a failed write of that line, with
/// nothing on `err`; or it prints one line `REFUSED usage: <detail>` on `err`, nothing on `out`,
/// and returns 2.
pub fn command(args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    command_on(&Data::release(), args, out, err)
}

fn command_on(data: &Data<'_>, args: &[String], out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    let rounds = match rounds(args) {
        Ok(rounds) => rounds,
        Err(detail) => {
            let _ = writeln!(err, "REFUSED usage: {detail}");
            return 2;
        }
    };
    let probe = run(data, rounds);
    let word = if probe.ok { "yes" } else { "no" };
    match writeln!(out, "probe rounds={} ok={word}", probe.rounds).and_then(|()| out.flush()) {
        Ok(()) if probe.ok => 0,
        _ => 1,
    }
}

fn rounds(args: &[String]) -> Result<u64, String> {
    let mut found = None;
    let mut words = args.iter();
    while let Some(name) = words.next() {
        if name != "--rounds" {
            return Err(format!("unknown argument {}", printable(name)));
        }
        let Some(value) = words.next() else {
            return Err("--rounds has no value".to_owned());
        };
        if found.replace(value).is_some() {
            return Err("--rounds twice".to_owned());
        }
    }
    let Some(text) = found else {
        return Err("--rounds is required".to_owned());
    };
    let canonical =
        !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit()) && !text.starts_with('0');
    match text.parse::<u64>() {
        Ok(value) if canonical => Ok(value),
        _ => Err("--rounds takes a whole number from 1".to_owned()),
    }
}

fn printable(word: &str) -> &str {
    if word.starts_with("--") && word.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') {
        word
    } else {
        "of another form"
    }
}

#[cfg(test)]
#[forbid(unsafe_code)]
mod tests {
    use std::process::Command;
    use std::sync::atomic::Ordering;

    use halvo_route::policy::{Basis, PickCell, Row};
    use halvo_route::tier::Level;

    use super::*;

    const HOSTED_CELLS: usize = 30;

    const CHECK_ROUNDS: u64 = 100_000;

    const SCRIPT: &str = include_str!("../../../scripts/r6/ci.d/70-r6-memory.sh");
    const PROBE: &str = include_str!("probe.rs");
    const MAIN: &str = include_str!("main.rs");

    const CHILD: &str = "HALVO_ROUTE_GATE_PROBE_ALLOCATION_CHILD";

    use super::counting::{
        this_thread, ALLOCATIONS, BYTES, FIRST_ALLOCATIONS, FIRST_BYTES, FIRST_THREAD,
    };

    fn first_thread() -> (u64, u64) {
        (
            FIRST_ALLOCATIONS.load(Ordering::SeqCst),
            FIRST_BYTES.load(Ordering::SeqCst),
        )
    }

    fn other_threads() -> (u64, u64) {
        (
            ALLOCATIONS.load(Ordering::SeqCst),
            BYTES.load(Ordering::SeqCst),
        )
    }

    fn since(before: (u64, u64), after: (u64, u64)) -> (u64, u64) {
        (after.0 - before.0, after.1 - before.1)
    }

    mod token {
        use std::sync::atomic::Ordering;

        use super::{this_thread, FIRST_THREAD};

        pub(super) struct Apart(());

        pub(super) const ON_THE_FIRST_THREAD: &str =
            "the test runs on the process's first thread, whose calls the windows leave out, so \
             nothing was measured; the test harness runs a test there when it cannot start a \
             thread for it";
        pub(super) const NO_FIRST_THREAD: &str =
            "no thread made a call of the allocator before the test began";

        pub(super) fn refusal(first: usize, me: usize, first_calls: u64) -> Option<&'static str> {
            if first_calls == 0 {
                return Some(NO_FIRST_THREAD);
            }
            if me == first {
                return Some(ON_THE_FIRST_THREAD);
            }
            None
        }

        pub(super) fn apart_from_the_harness(start: (u64, u64)) -> Apart {
            if let Some(why) = refusal(FIRST_THREAD.load(Ordering::SeqCst), this_thread(), start.0)
            {
                panic!("{why}");
            }
            Apart(())
        }
    }

    use token::{apart_from_the_harness, refusal, Apart, NO_FIRST_THREAD, ON_THE_FIRST_THREAD};

    struct Window {
        first: (u64, u64),
        others: (u64, u64),
    }

    fn allocations<T>(_: &Apart, work: impl FnOnce() -> T) -> (Window, T) {
        let before = (first_thread(), other_threads());
        let held = work();
        let after = (first_thread(), other_threads());
        let window = Window {
            first: since(before.0, after.0),
            others: since(before.1, after.1),
        };
        (window, held)
    }

    fn one_round_and_many<T>(
        apart: &Apart,
        rounds: impl Fn(u64) -> T,
    ) -> ((Window, T), (Window, T)) {
        black_box(rounds(1));
        let one = allocations(apart, || rounds(1));
        let many = allocations(apart, || rounds(CHECK_ROUNDS));
        (one, many)
    }

    fn the_same_count(one: &Window, many: &Window) -> bool {
        many.others == one.others
    }

    fn window_line(name: &str, window: &Window) -> String {
        format!(
            "window {name} first_calls={} first_bytes={} other_calls={} other_bytes={}",
            window.first.0, window.first.1, window.others.0, window.others.1
        )
    }

    fn print_windows(start: (u64, u64), windows: [(&str, &Window); 2]) {
        let end = first_thread();
        let on_first = this_thread() == FIRST_THREAD.load(Ordering::SeqCst);
        for (name, window) in windows {
            println!("{}", window_line(name, window));
        }
        println!(
            "first_thread start_calls={} start_bytes={} end_calls={} end_bytes={} \
             test_on_first_thread={on_first}",
            start.0, start.1, end.0, end.1
        );
    }

    fn in_a_child_alone(name: &str) -> bool {
        if std::env::var_os(CHILD).is_some() {
            return true;
        }
        let output = Command::new(std::env::current_exe().expect("the test binary"))
            .args(["--exact", name, "--test-threads", "1", "--nocapture"])
            .env(CHILD, "1")
            .output()
            .expect("the test binary runs");
        let out = String::from_utf8_lossy(&output.stdout);
        let err = String::from_utf8_lossy(&output.stderr);
        let text =
            format!("the child's standard output:\n{out}\nthe child's standard error:\n{err}");
        assert!(output.status.success(), "{text}");
        assert!(out.contains("test result: ok. 1 passed"), "{text}");
        false
    }

    fn hosted_policy() -> Policy {
        let base = Policy::hand_back_all().to_text();
        let cell = "utility_bill_payment\tcompile\thand_back\t-\tuntested\t0\t0\n";
        let hosted = base.replacen(
            cell,
            "utility_bill_payment\tcompile\tlow_cost_hosted\tlow\tline_met\t4\t4\n",
            1,
        );
        assert_ne!(hosted, base);
        Policy::parse_for(&hosted, &Tier::ALL).expect("a hosted table")
    }

    fn words(args: &[&str]) -> Vec<String> {
        args.iter().map(|arg| (*arg).to_owned()).collect()
    }

    fn call(args: &[&str]) -> (u8, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = command(&words(args), &mut out, &mut err);
        (
            code,
            String::from_utf8(out).expect("utf-8"),
            String::from_utf8(err).expect("utf-8"),
        )
    }

    struct Broken;

    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("closed"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::other("closed"))
        }
    }

    #[test]
    fn probe_allocates_nothing_per_round() {
        let start = first_thread();
        if !in_a_child_alone("probe::tests::probe_allocates_nothing_per_round") {
            return;
        }
        let apart = apart_from_the_harness(start);
        let data = Data::release();
        let (one, many) = one_round_and_many(&apart, |rounds| run(&data, rounds));
        print_windows(start, [("rounds=1", &one.0), ("rounds=many", &many.0)]);
        assert_eq!(
            (one.1, many.1),
            (
                Probe {
                    rounds: 1,
                    ok: true
                },
                Probe {
                    rounds: CHECK_ROUNDS,
                    ok: true
                }
            )
        );
        assert!(
            the_same_count(&one.0, &many.0),
            "1 round {:?}, many rounds {:?}",
            one.0.others,
            many.0.others
        );
    }

    #[test]
    fn the_measure_sees_a_round_that_allocates() {
        let start = first_thread();
        if !in_a_child_alone("probe::tests::the_measure_sees_a_round_that_allocates") {
            return;
        }
        let apart = apart_from_the_harness(start);
        let data = Data::release();
        let late = |rounds: u64| rounds == CHECK_ROUNDS;
        let measuring = one_round_and_many(&apart, |rounds| {
            let probe = run(&data, rounds);
            if late(rounds) {
                black_box(Box::new(rounds));
            }
            probe
        });
        let started = one_round_and_many(&apart, |rounds| {
            let probe = run(&data, rounds);
            std::thread::scope(|scope| {
                scope
                    .spawn(|| {
                        if late(rounds) {
                            black_box(Box::new(rounds));
                        }
                    })
                    .join()
            })
            .expect("the started thread ends");
            probe
        });
        let sized = one_round_and_many(&apart, |rounds| {
            black_box(Vec::<u8>::with_capacity(
                usize::try_from(rounds).expect("a size"),
            ));
            run(&data, rounds)
        });
        for (name, (one, many)) in [
            ("measuring", measuring),
            ("started", started),
            ("sized", sized),
        ] {
            println!("{}", window_line(&format!("{name}:rounds=1"), &one.0));
            println!("{}", window_line(&format!("{name}:rounds=many"), &many.0));
            assert!(
                !the_same_count(&one.0, &many.0),
                "{name}: 1 round {:?}, many rounds {:?}",
                one.0.others,
                many.0.others
            );
        }
    }

    #[test]
    fn the_allocation_test_makes_the_rounds_of_the_check() {
        let lines: Vec<String> = SCRIPT
            .lines()
            .filter(|line| line.starts_with("rounds="))
            .map(str::to_owned)
            .collect();
        assert_eq!(lines, vec![format!("rounds={CHECK_ROUNDS}")]);
    }

    fn code_of(text: &str) -> String {
        text.lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn the_probe_names_no_process() {
        let outside = &PROBE[..PROBE.find("#[cfg(test)]").expect("the test module")];
        let from = MAIN.find("Some(\"probe\")").expect("the arm of probe");
        let to = from + MAIN[from..].find("Some(_)").expect("the arm after it");
        for (part, text) in [("probe.rs", outside), ("the arm of probe", &MAIN[from..to])] {
            let code = code_of(text);
            for word in ["process", "Command"] {
                assert!(!code.contains(word), "{part} names {word}");
            }
        }
    }

    #[test]
    fn the_probe_runs_on_the_release_data() {
        let data = Data::release();
        assert!(std::ptr::eq(data.policy, release::policy()));
        assert_eq!(*data.policy, *release::policy());
        assert_eq!(data.ruling, release::ruling());
        assert_eq!(data.cap, release::cap());
        assert!(std::ptr::eq(data.prices, Prices::release()));
    }

    #[test]
    fn probe_visits_every_cell_that_can_hold_a_hosted_pick() {
        let mut hosted = 0;
        for index in 0..CELL_COUNT {
            let mut rows = *Policy::hand_back_all().rows();
            rows[index] = Row {
                pick: PickCell::Tier(Tier::Frontier),
                level: Some(Level::Low),
                basis: Basis::LineMet,
                k: 4,
                n: 4,
            };
            let Ok(policy) = Policy::from_rows(rows, &Tier::ALL) else {
                continue;
            };
            hosted += 1;
            let data = Data {
                policy: &policy,
                ..Data::release()
            };
            assert!(!run(&data, CELL_COUNT as u64 * 3).ok, "cell {index}");
        }
        assert_eq!(hosted, HOSTED_CELLS);
    }

    #[test]
    fn the_count_sees_allocations_of_every_thread_but_the_first() {
        let start = first_thread();
        if !in_a_child_alone(
            "probe::tests::the_count_sees_allocations_of_every_thread_but_the_first",
        ) {
            return;
        }
        let apart = apart_from_the_harness(start);
        let (boxes, _) = allocations(&apart, || {
            std::hint::black_box(Box::new(1_u64)) == Box::new(1)
        });
        let (started, _) = allocations(&apart, || {
            std::thread::scope(|scope| {
                scope
                    .spawn(|| std::hint::black_box(vec![0_u8; 4096]))
                    .join()
            })
        });
        print_windows(start, [("boxes", &boxes), ("started", &started)]);
        assert_eq!(boxes.others, (2, 16));
        let count = started.others;
        assert!(count.0 >= 1 && count.1 >= 4096, "{count:?}");
    }

    #[test]
    fn a_child_that_fails_on_purpose() {
        if std::env::var_os(CHILD).is_some() {
            assert_eq!(std::hint::black_box((1, 2)), (1, 3));
        }
    }

    #[test]
    fn a_failed_child_says_what_it_saw() {
        let failed = std::panic::catch_unwind(|| {
            in_a_child_alone("probe::tests::a_child_that_fails_on_purpose")
        })
        .expect_err("the child fails");
        let text = failed.downcast_ref::<String>().expect("a panic text");
        assert!(text.contains("left: (1, 2)"), "{text}");
        assert!(text.contains("right: (1, 3)"), "{text}");
        assert!(
            text.contains("test result: FAILED. 0 passed; 1 failed"),
            "{text}"
        );
    }

    #[test]
    fn a_child_that_prints_a_pass_and_fails() {
        if std::env::var_os(CHILD).is_some() {
            println!("test result: ok. 1 passed");
            assert_eq!(std::hint::black_box((1, 2)), (1, 3));
        }
    }

    #[test]
    fn a_child_that_prints_a_pass_and_fails_is_refused() {
        let failed = std::panic::catch_unwind(|| {
            in_a_child_alone("probe::tests::a_child_that_prints_a_pass_and_fails")
        })
        .expect_err("the child fails");
        let text = failed.downcast_ref::<String>().expect("a panic text");
        assert!(text.contains("test result: ok. 1 passed\n"), "{text}");
        assert!(
            text.contains("test result: FAILED. 0 passed; 1 failed"),
            "{text}"
        );
    }

    #[test]
    fn a_child_that_runs_no_test_fails_the_parent() {
        let failed = std::panic::catch_unwind(|| in_a_child_alone("probe::tests::no_such_test"))
            .expect_err("a child that runs no test fails");
        let text = failed.downcast_ref::<String>().expect("a panic text");
        assert!(text.contains("test result: ok. 0 passed"), "{text}");
    }

    #[test]
    fn a_window_leaves_out_the_calls_of_the_first_thread() {
        let start = first_thread();
        if !in_a_child_alone("probe::tests::a_window_leaves_out_the_calls_of_the_first_thread") {
            return;
        }
        let apart = apart_from_the_harness(start);
        let size = 1 << 20;
        FIRST_THREAD.store(this_thread(), Ordering::SeqCst);
        let (window, _) = allocations(&apart, || black_box(vec![0_u8; size]));
        println!("{}", window_line("mark-moved", &window));
        let size = size as u64;
        assert!(
            window.first.0 >= 1 && window.first.1 >= size && window.first.1 < 2 * size,
            "first {:?}, others {:?}",
            window.first,
            window.others
        );
        assert!(window.others.1 < size, "{:?}", window.others);
    }

    #[test]
    fn a_window_is_refused_when_no_thread_called_before_the_test() {
        let refused = std::panic::catch_unwind(|| apart_from_the_harness((0, 0)))
            .err()
            .expect("the guard refuses");
        assert_eq!(
            refused.downcast_ref::<String>().map(String::as_str),
            Some(NO_FIRST_THREAD)
        );
    }

    #[test]
    fn a_window_leaves_out_only_a_first_thread_that_is_not_the_test() {
        assert_eq!(refusal(7, 8, 1), None);
        assert_eq!(refusal(7, 7, 1), Some(ON_THE_FIRST_THREAD));
        assert_eq!(refusal(7, 8, 0), Some(NO_FIRST_THREAD));
        assert_eq!(refusal(0, 8, 0), Some(NO_FIRST_THREAD));
    }

    #[test]
    fn the_refusal_on_the_first_thread_says_that_nothing_was_measured_and_why() {
        assert!(
            ON_THE_FIRST_THREAD.contains("nothing was measured"),
            "{ON_THE_FIRST_THREAD}"
        );
        assert!(
            ON_THE_FIRST_THREAD.contains(
                "the test harness runs a test there when it cannot start a thread for it"
            ),
            "{ON_THE_FIRST_THREAD}"
        );
    }

    #[test]
    fn a_test_on_the_first_thread_is_refused_at_the_call() {
        let start = first_thread();
        if !in_a_child_alone("probe::tests::a_test_on_the_first_thread_is_refused_at_the_call") {
            return;
        }
        FIRST_THREAD.store(this_thread(), Ordering::SeqCst);
        let refused = std::panic::catch_unwind(|| apart_from_the_harness(start))
            .err()
            .expect("the guard refuses");
        assert_eq!(
            refused.downcast_ref::<String>().map(String::as_str),
            Some(ON_THE_FIRST_THREAD)
        );
    }

    #[test]
    fn a_window_line_names_each_count() {
        let window = Window {
            first: (1, 2),
            others: (3, 4),
        };
        assert_eq!(
            window_line("w", &window),
            "window w first_calls=1 first_bytes=2 other_calls=3 other_bytes=4"
        );
    }

    #[test]
    fn the_same_count_reads_the_other_threads_alone() {
        let window = |first, others| Window { first, others };
        assert!(the_same_count(
            &window((0, 0), (5, 40)),
            &window((3, 208), (5, 40))
        ));
        assert!(!the_same_count(
            &window((0, 0), (5, 40)),
            &window((0, 0), (6, 40))
        ));
        assert!(!the_same_count(
            &window((0, 0), (5, 40)),
            &window((0, 0), (5, 48))
        ));
        assert!(!the_same_count(
            &window((1, 8), (5, 40)),
            &window((0, 0), (6, 48))
        ));
    }

    #[test]
    fn probe_holds_on_the_release_data() {
        assert!(run(&Data::release(), 44 * 3 * 2).ok);
        assert!(run(&Data::release(), 1).ok);
        assert!(run(&Data::release(), 0).ok);
    }

    #[test]
    fn probe_misses_a_hosted_pick_that_the_ruling_does_not_allow() {
        let policy = hosted_policy();
        let data = Data {
            policy: &policy,
            ..Data::release()
        };
        assert!(!run(&data, 44 * 3).ok);
        assert!(run(&Data::release(), 44 * 3).ok);
    }

    #[test]
    fn probe_misses_a_refused_step() {
        let no_cap = Data {
            cap: None,
            ..Data::release()
        };
        assert!(!run(&no_cap, 1).ok);
        let dear = Prices::parse(
            "tier\tinput\toutput\tsource\tdate\n\
             on_device\t0\t0\tdevice\t2026-11-10\n\
             low_cost_hosted\t5000000000000\t0\tfixture\t2026-11-10\n\
             frontier\t0\t0\tfixture\t2026-11-10\n",
        )
        .expect("a price file");
        let over = Data {
            prices: &dear,
            ..Data::release()
        };
        assert!(!run(&over, 1).ok);
        let tiny = DefaultCap {
            amount: 1,
            ..release::cap().expect("the release cap")
        };
        let small = Data {
            cap: Some(tiny),
            ..Data::release()
        };
        assert!(!run(&small, 1).ok);
    }

    #[test]
    fn probe_prints_its_line() {
        assert_eq!(
            call(&["--rounds", "3"]),
            (0, "probe rounds=3 ok=yes\n".to_owned(), String::new())
        );
        assert_eq!(
            call(&["--rounds", "100000"]),
            (0, "probe rounds=100000 ok=yes\n".to_owned(), String::new())
        );
    }

    #[test]
    fn probe_prints_a_miss() {
        let policy = hosted_policy();
        let data = Data {
            policy: &policy,
            ..Data::release()
        };
        let mut out = Vec::new();
        let mut err = Vec::new();
        let code = command_on(&data, &words(&["--rounds", "132"]), &mut out, &mut err);
        assert_eq!(
            (code, out.as_slice(), err.as_slice()),
            (1, b"probe rounds=132 ok=no\n".as_slice(), b"".as_slice())
        );
    }

    #[test]
    fn probe_fails_a_write_with_nothing_on_standard_error() {
        let mut err = Vec::new();
        let code = command(&words(&["--rounds", "3"]), &mut Broken, &mut err);
        assert_eq!((code, err.as_slice()), (1, b"".as_slice()));
    }

    #[test]
    fn probe_refusals_of_usage() {
        for (args, detail) in [
            (&[][..], "--rounds is required"),
            (&["--rounds"][..], "--rounds has no value"),
            (&["--rounds", "1", "--rounds", "2"][..], "--rounds twice"),
            (&["--seed", "1"][..], "unknown argument --seed"),
            (&["--Rounds", "1"][..], "unknown argument of another form"),
            (&["rounds", "1"][..], "unknown argument of another form"),
            (
                &["--rounds", "0"][..],
                "--rounds takes a whole number from 1",
            ),
            (
                &["--rounds", "01"][..],
                "--rounds takes a whole number from 1",
            ),
            (
                &["--rounds", "-1"][..],
                "--rounds takes a whole number from 1",
            ),
            (
                &["--rounds", "+1"][..],
                "--rounds takes a whole number from 1",
            ),
            (
                &["--rounds", ""][..],
                "--rounds takes a whole number from 1",
            ),
            (
                &["--rounds", "1x"][..],
                "--rounds takes a whole number from 1",
            ),
            (
                &["--rounds", "18446744073709551616"][..],
                "--rounds takes a whole number from 1",
            ),
        ] {
            assert_eq!(
                call(args),
                (2, String::new(), format!("REFUSED usage: {detail}\n")),
                "{args:?}"
            );
        }
    }
}

#[cfg(test)]
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    struct Counting;

    pub(super) static FIRST_THREAD: AtomicUsize = AtomicUsize::new(0);
    pub(super) static FIRST_ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
    pub(super) static FIRST_BYTES: AtomicU64 = AtomicU64::new(0);
    pub(super) static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
    pub(super) static BYTES: AtomicU64 = AtomicU64::new(0);

    thread_local! {
        static MARK: u8 = const { 0 };
    }

    pub(super) fn this_thread() -> usize {
        MARK.with(|mark| std::ptr::from_ref(mark).addr())
    }

    #[allow(unsafe_code)]
    unsafe impl GlobalAlloc for Counting {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let me = this_thread();
            let first =
                match FIRST_THREAD.compare_exchange(0, me, Ordering::SeqCst, Ordering::SeqCst) {
                    Ok(_) => me,
                    Err(first) => first,
                };
            let (calls, bytes) = if me == first {
                (&FIRST_ALLOCATIONS, &FIRST_BYTES)
            } else {
                (&ALLOCATIONS, &BYTES)
            };
            calls.fetch_add(1, Ordering::SeqCst);
            bytes.fetch_add(layout.size() as u64, Ordering::SeqCst);
            System.alloc(layout)
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            System.dealloc(ptr, layout)
        }
    }

    #[global_allocator]
    static COUNTING: Counting = Counting;
}
