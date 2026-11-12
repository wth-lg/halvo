use halvo_learn::{
    admit, build_list, candidate_move, check_sources, counted, new_locators, routine_step,
    use_move, value_hit, AdmitCause, AdmitFacts, CandidateFacts, CandidateMove, EditCause,
    EditMove, EditSources, Locator, LogCause, Mark, RoutineState, RunMode, RunSource, SourceCause,
    SourceFacts, Target, LOCATORS_MAX,
};

fn kept(locators: Vec<Locator>) -> CandidateFacts {
    CandidateFacts {
        verified: true,
        recovered: true,
        pending: false,
        current: true,
        edits: 1,
        locators,
        base_list: Vec::new(),
        barred: Vec::new(),
    }
}

fn first_revision(runs: &[&str], first: u32, last: u32, run_day: u32) -> SourceFacts {
    SourceFacts {
        edit: EditSources {
            workflow: "w1".to_owned(),
            agent: "a1".to_owned(),
            runs: runs.iter().map(|run| (*run).to_owned()).collect(),
            first,
            last,
            clean: 0,
        },
        before: None,
        run: Some(RunSource {
            workflow: "w1".to_owned(),
            agent: "a1".to_owned(),
            run: "r1".to_owned(),
            day: run_day,
        }),
    }
}

#[test]
fn candidate_drops_a_barred_locator_beside_one_left() {
    let facts = CandidateFacts {
        barred: vec![loc("y")],
        ..kept(vec![loc("x"), loc("y")])
    };
    assert_eq!(candidate_move(&facts), CandidateMove::Drop);
    assert_eq!(
        candidate_move(&kept(vec![loc("x"), loc("y")])),
        CandidateMove::Keep(vec![loc("x"), loc("y")])
    );
    let barred_first = CandidateFacts {
        barred: vec![loc("y")],
        ..kept(vec![loc("y"), loc("x")])
    };
    assert_eq!(candidate_move(&barred_first), CandidateMove::Drop);
    let barred_in_the_middle = CandidateFacts {
        barred: vec![loc("y")],
        ..kept(vec![loc("x"), loc("y"), loc("z")])
    };
    assert_eq!(candidate_move(&barred_in_the_middle), CandidateMove::Drop);
    assert_eq!(
        candidate_move(&kept(vec![loc("x"), loc("y"), loc("z")])),
        CandidateMove::Keep(vec![loc("x"), loc("y"), loc("z")])
    );
}

#[test]
fn first_revision_days_are_its_run_day() {
    for (first, last) in [(9, 10), (10, 11), (5, 20)] {
        assert_eq!(
            check_sources(&first_revision(&["r1"], first, last, 10)),
            Err(SourceCause::Source),
            "first {first} last {last} for a run of day 10"
        );
    }
    assert_eq!(check_sources(&first_revision(&["r1"], 10, 10, 10)), Ok(()));
}

#[test]
fn first_revision_holds_one_run() {
    for runs in [&["r0", "r1"][..], &["r0", "r9", "r1"], &["r1", "r1"]] {
        assert_eq!(
            check_sources(&first_revision(runs, 10, 10, 10)),
            Err(SourceCause::Source),
            "runs {runs:?}"
        );
    }
    assert_eq!(check_sources(&first_revision(&["r1"], 10, 10, 10)), Ok(()));
}

#[test]
fn first_revision_days_follow_the_run_not_each_other() {
    assert_eq!(
        check_sources(&first_revision(&["r1"], 10, 10, 30)),
        Err(SourceCause::Source)
    );
    assert_eq!(
        check_sources(&first_revision(&["r1"], 12, 12, 10)),
        Err(SourceCause::Source)
    );
    assert_eq!(check_sources(&first_revision(&["r1"], 30, 30, 30)), Ok(()));
}

#[test]
fn candidate_keeps_the_left_locators_in_their_order() {
    assert_eq!(
        candidate_move(&kept(vec![loc("a"), loc("b")])),
        CandidateMove::Keep(vec![loc("a"), loc("b")])
    );
    let base_in_the_middle = CandidateFacts {
        base_list: vec![loc("m")],
        ..kept(vec![loc("a"), loc("m"), loc("b")])
    };
    assert_eq!(
        candidate_move(&base_in_the_middle),
        CandidateMove::Keep(vec![loc("a"), loc("b")])
    );
    assert_eq!(
        candidate_move(&kept(vec![loc("b"), loc("a")])),
        CandidateMove::Keep(vec![loc("b"), loc("a")])
    );
}

fn loc(value: &str) -> Locator {
    Locator {
        by: "label".to_owned(),
        value: value.to_owned(),
        name: None,
    }
}

fn admitted() -> AdmitFacts {
    AdmitFacts {
        mode: RunMode::Replay,
        has_base: true,
        same_but_target: true,
        kind_web: true,
        class_same: true,
        playbook_ok: true,
        new_count: 1,
        value_hit: false,
        fits: true,
        rejected: false,
        edits_for_step: 0,
    }
}

#[test]
fn admit_names_the_first_row_that_holds() {
    let mut facts = AdmitFacts {
        mode: RunMode::Compile,
        has_base: false,
        same_but_target: false,
        kind_web: false,
        class_same: false,
        playbook_ok: false,
        new_count: 0,
        value_hit: true,
        fits: false,
        rejected: true,
        edits_for_step: 8,
    };
    assert_eq!(admit(&facts), Err(AdmitCause::Mode));
    facts.mode = RunMode::Replay;
    assert_eq!(admit(&facts), Err(AdmitCause::Base));
    facts.has_base = true;
    assert_eq!(admit(&facts), Err(AdmitCause::Scope));
    facts.same_but_target = true;
    facts.kind_web = true;
    assert_eq!(admit(&facts), Err(AdmitCause::Class));
    facts.class_same = true;
    facts.playbook_ok = true;
    assert_eq!(admit(&facts), Err(AdmitCause::Locator));
    facts.new_count = 1;
    assert_eq!(admit(&facts), Err(AdmitCause::Value));
    facts.value_hit = false;
    assert_eq!(admit(&facts), Err(AdmitCause::Length));
    facts.fits = true;
    assert_eq!(admit(&facts), Err(AdmitCause::Rejected));
    facts.rejected = false;
    assert_eq!(admit(&facts), Err(AdmitCause::Full));
    facts.edits_for_step = 7;
    assert_eq!(admit(&facts), Ok(()));
}

#[test]
fn admit_reads_each_fact_of_a_row() {
    let cases = [
        (
            AdmitFacts {
                same_but_target: false,
                ..admitted()
            },
            AdmitCause::Scope,
        ),
        (
            AdmitFacts {
                kind_web: false,
                ..admitted()
            },
            AdmitCause::Scope,
        ),
        (
            AdmitFacts {
                class_same: false,
                ..admitted()
            },
            AdmitCause::Class,
        ),
        (
            AdmitFacts {
                playbook_ok: false,
                ..admitted()
            },
            AdmitCause::Class,
        ),
        (
            AdmitFacts {
                fits: false,
                ..admitted()
            },
            AdmitCause::Length,
        ),
    ];
    for (facts, cause) in cases {
        assert_eq!(admit(&facts), Err(cause), "{facts:?}");
    }
}

#[test]
fn use_ok_needs_a_verified_run() {
    assert_eq!(
        use_move(false, false, true, true),
        Some(EditMove::UseFailOther)
    );
    assert_eq!(use_move(false, true, true, true), Some(EditMove::UseOk));
}

#[test]
fn counted_takes_the_three_names() {
    assert_eq!(counted(None), None);
    for &cause in LogCause::ALL {
        let wanted = match cause {
            LogCause::StepFailed => Some(EditCause::StepFailed),
            LogCause::ClassMismatch => Some(EditCause::ClassMismatch),
            LogCause::Origin => Some(EditCause::Origin),
            _ => None,
        };
        assert_eq!(counted(Some(cause)), wanted, "{}", cause.token());
    }
}

#[test]
fn new_locators_holds_each_once() {
    let served = [loc("served")];
    let proposed = [loc("x"), loc("x"), loc("served"), loc("y")];
    assert_eq!(new_locators(&proposed, &served), vec![loc("x"), loc("y")]);
}

#[test]
fn build_list_puts_new_first_and_cuts() {
    let new = [loc("n1"), loc("n2")];
    let base = [loc("b1"), loc("n1"), loc("b2"), loc("b3")];
    let list = build_list(&new, &base);
    assert_eq!(list, vec![loc("n1"), loc("n2"), loc("b1"), loc("b2")]);
    assert_eq!(list.len(), LOCATORS_MAX);
    assert_eq!(
        build_list(&[loc("n1"), loc("n1")], &[loc("b1")]),
        vec![loc("n1"), loc("b1")]
    );
}

#[test]
fn value_hit_reads_desc_value_and_name() {
    let target = |desc: &str, value: &str, name: Option<&str>| Target {
        desc: desc.to_owned(),
        locators: vec![Locator {
            by: "text".to_owned(),
            value: value.to_owned(),
            name: name.map(str::to_owned),
        }],
    };
    let four = vec!["Ab12".to_owned()];
    let three = vec!["Ab1".to_owned()];
    assert!(value_hit(&target("Pay Ab12", "Pay", None), &four));
    assert!(value_hit(&target("Pay", "Pay Ab12", None), &four));
    assert!(value_hit(&target("Pay", "Pay", Some("Ab12 now")), &four));
    assert!(!value_hit(&target("Pay", "Pay", Some("Ab1 now")), &four));
    assert!(!value_hit(&target("Pay Ab1", "Ab1", Some("Ab1")), &three));
}

fn sources(workflow: &str, runs: &[&str], first: u32, last: u32, clean: u32) -> EditSources {
    EditSources {
        workflow: workflow.to_owned(),
        agent: "a1".to_owned(),
        runs: runs.iter().map(|run| (*run).to_owned()).collect(),
        first,
        last,
        clean,
    }
}

#[test]
fn check_sources_reads_runs_and_agent() {
    let before = sources("w1", &["r1", "r2"], 10, 12, 1);
    let same = SourceFacts {
        edit: sources("w1", &["r1", "r2"], 10, 12, 1),
        before: Some(before.clone()),
        run: None,
    };
    assert_eq!(check_sources(&same), Ok(()));
    let other_runs = SourceFacts {
        edit: sources("w1", &["r1", "r9"], 10, 12, 1),
        ..same.clone()
    };
    assert_eq!(check_sources(&other_runs), Err(SourceCause::Source));

    let added = |agent: &str| SourceFacts {
        edit: sources("w1", &["r1", "r2", "r3"], 10, 14, 2),
        before: Some(before.clone()),
        run: Some(RunSource {
            workflow: "w1".to_owned(),
            agent: agent.to_owned(),
            run: "r3".to_owned(),
            day: 14,
        }),
    };
    assert_eq!(check_sources(&added("a1")), Ok(()));
    assert_eq!(check_sources(&added("a2")), Err(SourceCause::Source));
}

#[test]
fn routine_reads_the_window_in_any_order() {
    let state = routine_step(None, &[30, 10, 20, 20, 30, 200], 30);
    assert_eq!(state.seen, 3);
    assert_eq!(state.first, Some(10));
    assert_eq!(state.last, Some(30));
    assert_eq!(state.period, Some(10));
    assert_eq!(state.next, Some(40));

    let window = routine_step(None, &[1, 200, 210, 220], 220);
    assert_eq!(window.seen, 3);
    assert_eq!(window.first, Some(200));
}

#[test]
fn routine_misses_nothing_without_a_period() {
    let state = routine_step(None, &[10], 100);
    assert_eq!(state.period, None);
    assert_eq!(state.next, None);
    assert_eq!(state.missed, 0);
    assert_eq!(state.mark, Mark::None);
    let two = routine_step(Some(&state), &[10, 40], 130);
    assert_eq!(two.last, Some(40));
    assert_eq!(two.period, None);
    assert_eq!(two.missed, 0);
    assert_eq!(two.mark, Mark::None);
}

#[test]
fn routine_keeps_prev_where_the_days_give_none() {
    let prev = routine_step(None, &[10, 20, 30], 30);
    let later = routine_step(Some(&prev), &[], 200);
    assert_eq!(later.first, Some(10));
    assert_eq!(later.last, Some(30));
    assert_eq!(later.period, Some(10));
    assert_eq!(later.next, Some(40));
    assert_eq!(later.seen, 0);
}

#[test]
fn routine_since_is_the_day_the_mark_changed() {
    let days = [10, 20, 30];
    let mut prev: Option<RoutineState> = None;
    let mut by_day = Vec::new();
    for today in 30..=91 {
        let state = routine_step(prev.as_ref(), &days, today);
        by_day.push((today, state));
        prev = Some(state);
    }
    let at = |day: u32| by_day.iter().find(|(d, _)| *d == day).unwrap().1;
    assert_eq!(at(89).mark, Mark::None);
    assert_eq!(at(89).since, None);
    assert_eq!(at(90).mark, Mark::Stale);
    assert_eq!(at(90).since, Some(90));
    assert_eq!(at(91).mark, Mark::Stale);
    assert_eq!(at(91).since, Some(90));
    let back = routine_step(Some(&at(91)), &[10, 20, 30, 92], 92);
    assert_eq!(back.mark, Mark::None);
    assert_eq!(back.since, Some(92));
    let still = routine_step(Some(&back), &[10, 20, 30, 92], 93);
    assert_eq!(still.since, Some(92));
}
