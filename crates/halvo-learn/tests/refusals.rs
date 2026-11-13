use halvo_learn::{
    admit, brought, candidate_move, check_sources, counted, edit_step, is_rejected, new_locators,
    use_move, value_hit, verify_run, AdmitCause, AdmitFacts, CandidateFacts, CandidateMove,
    EditAnswer, EditCause, EditMove, EditSources, EditState, Locator, LogCause, Outcome, RunCause,
    RunFacts, RunMode, RunSource, SourceCause, SourceFacts, Target, EDITS_MAX,
};

fn sources(runs: &[&str], first: u32, last: u32, clean: u32) -> EditSources {
    EditSources {
        workflow: "w1".to_owned(),
        agent: "a1".to_owned(),
        runs: runs.iter().map(|run| (*run).to_owned()).collect(),
        first,
        last,
        clean,
    }
}

fn run_source(workflow: &str, run: &str, day: u32) -> Option<RunSource> {
    Some(RunSource {
        workflow: workflow.to_owned(),
        agent: "a1".to_owned(),
        run: run.to_owned(),
        day,
    })
}

fn check(
    edit: EditSources,
    before: Option<EditSources>,
    run: Option<RunSource>,
) -> Result<(), SourceCause> {
    check_sources(&SourceFacts { edit, before, run })
}

#[test]
fn refusal_no_source() {
    let rev1 = || sources(&["r1"], 10, 10, 0);
    let rev2 = || sources(&["r1", "r2"], 10, 12, 1);
    assert_eq!(check(rev1(), None, run_source("w1", "r1", 10)), Ok(()));
    assert_eq!(
        check(rev1(), None, run_source("w1", "r9", 10)),
        Err(SourceCause::Source)
    );
    assert_eq!(
        check(rev1(), None, run_source("w2", "r1", 10)),
        Err(SourceCause::Source)
    );
    assert_eq!(check(rev1(), None, None), Err(SourceCause::Source));
    let other_agent = RunSource {
        agent: "a2".to_owned(),
        ..run_source("w1", "r1", 10).unwrap()
    };
    assert_eq!(
        check(rev1(), None, Some(other_agent)),
        Err(SourceCause::Source)
    );

    assert_eq!(
        check(rev2(), Some(rev1()), run_source("w1", "r2", 12)),
        Ok(())
    );
    for kept_days in [
        sources(&["r1", "r3"], 10, 12, 1),
        sources(&["r2"], 10, 12, 1),
    ] {
        assert_eq!(
            check(kept_days, Some(rev2()), None),
            Err(SourceCause::Source)
        );
    }
    assert_eq!(
        check(
            sources(&["r9", "r2"], 10, 12, 1),
            Some(rev1()),
            run_source("w1", "r2", 12)
        ),
        Err(SourceCause::Source)
    );
    assert_eq!(
        check(
            sources(&["r2", "r3"], 12, 14, 1),
            Some(rev2()),
            run_source("w1", "r3", 14)
        ),
        Err(SourceCause::Source)
    );

    assert_eq!(
        check(
            sources(&["r1", "r2"], 10, 11, 1),
            Some(rev1()),
            run_source("w1", "r2", 12)
        ),
        Err(SourceCause::Source)
    );
    assert_eq!(
        check(
            sources(&["r1", "r2"], 8, 10, 1),
            Some(rev1()),
            run_source("w1", "r2", 8)
        ),
        Ok(())
    );
    assert_eq!(
        check(
            sources(&["r1", "r2"], 10, 10, 1),
            Some(rev1()),
            run_source("w1", "r2", 8)
        ),
        Err(SourceCause::Source)
    );

    assert_eq!(check(rev2(), Some(rev2()), None), Ok(()));
    assert_eq!(
        check(sources(&["r1", "r2"], 10, 12, 2), Some(rev2()), None),
        Err(SourceCause::Source)
    );

    assert_eq!(
        check(rev2(), Some(rev2()), run_source("w1", "r3", 14)),
        Ok(())
    );
    assert_eq!(
        check(
            sources(&["r1", "r2"], 10, 14, 1),
            Some(rev2()),
            run_source("w1", "r3", 14)
        ),
        Err(SourceCause::Source)
    );
    assert_eq!(
        check(
            sources(&["r1", "r2"], 11, 12, 1),
            Some(rev2()),
            run_source("w1", "r3", 14)
        ),
        Err(SourceCause::Source)
    );
}

#[derive(Clone, Copy)]
enum Event {
    StepOk,
    StepFailed(Option<LogCause>),
    Recovered,
}

struct Line {
    step: u32,
    event: Event,
}

fn line(step: u32, event: Event) -> Line {
    Line { step, event }
}

fn use_move_of(lines: &[Line], step: u32, verified: bool) -> Option<EditMove> {
    let own: Vec<&Line> = lines.iter().filter(|l| l.step == step).collect();
    let counted_line = own
        .iter()
        .any(|l| matches!(l.event, Event::StepFailed(cause) if counted(cause).is_some()));
    let step_ok = own.iter().any(|l| matches!(l.event, Event::StepOk));
    use_move(counted_line, verified, step_ok, !own.is_empty())
}

fn recovered_of(lines: &[Line], step: u32) -> bool {
    let own: Vec<&Line> = lines.iter().filter(|l| l.step == step).collect();
    match own
        .iter()
        .rposition(|l| matches!(l.event, Event::Recovered))
    {
        Some(at) => !own[at..]
            .iter()
            .any(|l| matches!(l.event, Event::StepFailed(_))),
        None => false,
    }
}

fn verified_of(outcome: Outcome, unknown: u32) -> bool {
    verify_run(&RunFacts { outcome, unknown }).is_ok()
}

fn kept_facts(locators: Vec<Locator>) -> CandidateFacts {
    CandidateFacts {
        verified: verified_of(Outcome::Completed, 0),
        recovered: recovered_of(&[line(3, Event::Recovered), line(3, Event::StepOk)], 3),
        pending: false,
        current: true,
        edits: 0,
        locators,
        base_list: Vec::new(),
        barred: Vec::new(),
    }
}

fn loc(by: &str, value: &str, name: Option<&str>) -> Locator {
    Locator {
        by: by.to_owned(),
        value: value.to_owned(),
        name: name.map(str::to_owned),
    }
}

fn admitted_facts() -> AdmitFacts {
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
fn refusal_value_in_label() {
    let slot_values = vec!["Acme Utility".to_owned()];
    assert_eq!(slot_values[0].len(), 12);
    let with_value = Target {
        desc: "button \"Pay\"".to_owned(),
        locators: vec![loc("role", "button", Some("Pay Acme Utility now"))],
    };
    let without_value = Target {
        desc: "button \"Pay\"".to_owned(),
        locators: vec![loc("role", "button", Some("Pay now"))],
    };
    assert!(value_hit(&with_value, &slot_values));
    assert!(!value_hit(&without_value, &slot_values));
    let in_second = Target {
        desc: "button \"Pay\"".to_owned(),
        locators: vec![
            loc("role", "button", Some("Pay now")),
            loc("label", "Pay", Some("Pay Acme Utility now")),
        ],
    };
    assert!(value_hit(&in_second, &slot_values));

    let facts = AdmitFacts {
        value_hit: value_hit(&with_value, &slot_values),
        ..admitted_facts()
    };
    assert_eq!(admit(&facts), Err(AdmitCause::Value));
    let control = AdmitFacts {
        value_hit: value_hit(&without_value, &slot_values),
        ..admitted_facts()
    };
    assert_eq!(admit(&control), Ok(()));
}

#[test]
fn refusal_compile_run_edit() {
    assert_eq!(admit(&admitted_facts()), Ok(()));
    for mode in [RunMode::Compile, RunMode::None] {
        let facts = AdmitFacts {
            mode,
            ..admitted_facts()
        };
        assert_eq!(
            admit(&facts),
            Err(AdmitCause::Mode),
            "mode {}",
            mode.token()
        );
    }

    let missing_step = AdmitFacts {
        has_base: false,
        ..admitted_facts()
    };
    assert_eq!(admit(&missing_step), Err(AdmitCause::Base));

    let compiled = [loc("role", "button", Some("Pay")), loc("css", "#pay", None)];
    let learned = loc("label", "Pay bill", None);
    let mut served = vec![learned.clone()];
    served.extend(compiled.iter().cloned());
    let held = vec![learned, compiled[1].clone()];
    let facts = AdmitFacts {
        new_count: new_locators(&held, &served).len(),
        ..admitted_facts()
    };
    assert_eq!(admit(&facts), Err(AdmitCause::Locator));

    let one_new = vec![loc("text", "Pay now", None), compiled[1].clone()];
    let control = AdmitFacts {
        new_count: new_locators(&one_new, &served).len(),
        ..admitted_facts()
    };
    assert_eq!(control.new_count, 1);
    assert_eq!(admit(&control), Ok(()));

    let other_name = vec![loc("role", "button", Some("Pay all"))];
    assert_eq!(new_locators(&other_name, &served), other_name);
}

#[test]
fn refusal_edits_full() {
    assert_eq!(EDITS_MAX, 8);
    for edits_for_step in [8, 9] {
        let full = AdmitFacts {
            edits_for_step,
            ..admitted_facts()
        };
        assert_eq!(admit(&full), Err(AdmitCause::Full));
    }
    let below = AdmitFacts {
        edits_for_step: 7,
        ..admitted_facts()
    };
    assert_eq!(admit(&below), Ok(()));
}

#[test]
fn refusal_unverified_run() {
    let x = loc("text", "Pay now", None);
    assert_eq!(
        candidate_move(&kept_facts(vec![x.clone()])),
        CandidateMove::Keep(vec![x.clone()])
    );

    assert_eq!(
        verify_run(&RunFacts {
            outcome: Outcome::Failed,
            unknown: 0
        }),
        Err(RunCause::Failed)
    );
    let failed = CandidateFacts {
        verified: verified_of(Outcome::Failed, 0),
        ..kept_facts(vec![x.clone()])
    };
    assert_eq!(candidate_move(&failed), CandidateMove::Drop);
    for &outcome in Outcome::ALL {
        if outcome == Outcome::Completed {
            continue;
        }
        let cause = verify_run(&RunFacts {
            outcome,
            unknown: 0,
        });
        assert_eq!(
            cause.map_err(RunCause::token),
            Err(outcome.token()),
            "outcome {}",
            outcome.token()
        );
        let facts = CandidateFacts {
            verified: verified_of(outcome, 0),
            ..kept_facts(vec![x.clone()])
        };
        assert_eq!(candidate_move(&facts), CandidateMove::Drop);
    }

    assert_eq!(
        verify_run(&RunFacts {
            outcome: Outcome::Completed,
            unknown: 1
        }),
        Err(RunCause::Unknown)
    );
    let unknown = CandidateFacts {
        verified: verified_of(Outcome::Completed, 1),
        ..kept_facts(vec![x.clone()])
    };
    assert_eq!(candidate_move(&unknown), CandidateMove::Drop);

    let no_recovered = CandidateFacts {
        recovered: recovered_of(&[line(3, Event::StepOk)], 3),
        ..kept_facts(vec![x.clone()])
    };
    assert_eq!(candidate_move(&no_recovered), CandidateMove::Drop);

    let recompiled = CandidateFacts {
        current: false,
        ..kept_facts(vec![x])
    };
    assert_eq!(candidate_move(&recompiled), CandidateMove::Drop);
}

#[test]
fn refusal_transient_keeps() {
    assert_eq!(counted(Some(LogCause::Timeout)), None);
    assert_eq!(counted(Some(LogCause::Declined)), None);
    assert_eq!(counted(None), None);
    assert_eq!(
        counted(Some(LogCause::StepFailed)),
        Some(EditCause::StepFailed)
    );
    for &cause in LogCause::ALL {
        let named = EditCause::from_token(cause.token());
        assert_eq!(counted(Some(cause)), named, "cause {}", cause.token());
    }

    let timeout_then_ok_unverified = [
        line(2, Event::StepFailed(Some(LogCause::Timeout))),
        line(2, Event::StepOk),
    ];
    assert_eq!(
        use_move_of(&timeout_then_ok_unverified, 2, false),
        Some(EditMove::UseFailOther)
    );

    let timeout_then_ok = [
        line(2, Event::StepFailed(Some(LogCause::Timeout))),
        line(2, Event::StepOk),
    ];
    assert_eq!(
        use_move_of(&timeout_then_ok, 2, true),
        Some(EditMove::UseOk)
    );

    let timeout_alone = [line(2, Event::StepFailed(Some(LogCause::Timeout)))];
    assert_eq!(
        use_move_of(&timeout_alone, 2, false),
        Some(EditMove::UseFailOther)
    );
    assert_eq!(
        use_move_of(&timeout_alone, 2, true),
        Some(EditMove::UseFailOther)
    );

    let counted_then_ok = [
        line(2, Event::StepFailed(Some(LogCause::StepFailed))),
        line(2, Event::StepOk),
    ];
    assert_eq!(
        use_move_of(&counted_then_ok, 2, true),
        Some(EditMove::UseFailCounted)
    );

    assert_eq!(use_move_of(&[], 2, true), None);

    assert_eq!(
        edit_step(EditState::Provisional, EditMove::UseFailOther, 1),
        EditAnswer::Next {
            state: EditState::Provisional,
            clean: 1
        }
    );
}

#[test]
fn refusal_second_pending() {
    let x = loc("text", "Pay now", None);
    for edits in [1, 3] {
        let pending = CandidateFacts {
            pending: true,
            edits,
            ..kept_facts(vec![x.clone()])
        };
        assert_eq!(candidate_move(&pending), CandidateMove::Drop);
        let control = CandidateFacts {
            pending: false,
            edits,
            ..kept_facts(vec![x.clone()])
        };
        assert_eq!(
            candidate_move(&control),
            CandidateMove::Keep(vec![x.clone()])
        );
    }
}

#[test]
fn refusal_failed_after_recovered() {
    let x = loc("text", "Pay now", None);
    for cause in [LogCause::StepFailed, LogCause::Timeout] {
        let lines = [
            line(3, Event::Recovered),
            line(3, Event::StepFailed(Some(cause))),
        ];
        let facts = CandidateFacts {
            recovered: recovered_of(&lines, 3),
            ..kept_facts(vec![x.clone()])
        };
        assert_eq!(
            candidate_move(&facts),
            CandidateMove::Drop,
            "cause {}",
            cause.token()
        );
    }

    let failed_then_recovered = [
        line(3, Event::StepFailed(Some(LogCause::StepFailed))),
        line(3, Event::Recovered),
        line(3, Event::StepOk),
    ];
    let control = CandidateFacts {
        recovered: recovered_of(&failed_then_recovered, 3),
        ..kept_facts(vec![x.clone()])
    };
    assert_eq!(candidate_move(&control), CandidateMove::Keep(vec![x]));
}

#[test]
fn refusal_failure_of_other_step() {
    for cause in [LogCause::StepFailed, LogCause::Timeout] {
        let lines = [
            line(1, Event::StepOk),
            line(2, Event::StepFailed(Some(cause))),
        ];
        assert_eq!(
            use_move_of(&lines, 1, true),
            Some(EditMove::UseOk),
            "cause {}",
            cause.token()
        );
    }

    let own_failure = [
        line(1, Event::StepOk),
        line(1, Event::StepFailed(Some(LogCause::StepFailed))),
    ];
    assert_eq!(
        use_move_of(&own_failure, 1, true),
        Some(EditMove::UseFailCounted)
    );
}

#[test]
fn refusal_rejected_again() {
    let compiled = [loc("role", "button", Some("Pay"))];
    let active = loc("label", "Pay bill", None);
    let reverted_new = loc("text", "Pay this bill", None);
    let reverted_list = [reverted_new.clone(), active.clone()];
    let barred = brought(&reverted_list, std::slice::from_ref(&active));
    assert_eq!(barred, vec![reverted_new.clone()]);

    let mut served = vec![active.clone()];
    served.extend(compiled.iter().cloned());
    let again = new_locators(std::slice::from_ref(&reverted_new), &served);
    assert!(is_rejected(&again, &barred));
    let facts = AdmitFacts {
        new_count: again.len(),
        rejected: is_rejected(&again, &barred),
        ..admitted_facts()
    };
    assert_eq!(admit(&facts), Err(AdmitCause::Rejected));
    let other = new_locators(&[loc("text", "Settle", None)], &served);
    let control = AdmitFacts {
        new_count: other.len(),
        rejected: is_rejected(&other, &barred),
        ..admitted_facts()
    };
    assert_eq!(admit(&control), Ok(()));

    let first_p = loc("text", "Pay", Some("Pay"));
    let first_q = loc("css", "#pay-now", None);
    let on_compiled = brought(&[first_p.clone(), first_q.clone()], &[]);
    assert_eq!(on_compiled, vec![first_p.clone(), first_q.clone()]);
    let mixed = new_locators(&[loc("text", "Settle", None), first_q.clone()], &compiled);
    assert_eq!(mixed.len(), 2);
    assert!(is_rejected(&mixed, &on_compiled));
    let mixed_facts = AdmitFacts {
        new_count: mixed.len(),
        rejected: is_rejected(&mixed, &on_compiled),
        ..admitted_facts()
    };
    assert_eq!(admit(&mixed_facts), Err(AdmitCause::Rejected));

    let listed = CandidateFacts {
        base_list: vec![active.clone()],
        ..kept_facts(vec![active.clone()])
    };
    assert_eq!(candidate_move(&listed), CandidateMove::Drop);
    assert_eq!(
        candidate_move(&kept_facts(vec![active.clone()])),
        CandidateMove::Keep(vec![active.clone()])
    );
    let y = loc("text", "Pay the bill", None);
    let one_left = CandidateFacts {
        base_list: vec![active.clone()],
        ..kept_facts(vec![active.clone(), y.clone()])
    };
    assert_eq!(candidate_move(&one_left), CandidateMove::Keep(vec![y]));

    let late = loc("text", "Pay", None);
    let barred_at_finish = brought(&[late.clone(), active.clone()], &[active]);
    let barred_after = CandidateFacts {
        barred: barred_at_finish,
        ..kept_facts(vec![late.clone()])
    };
    assert_eq!(candidate_move(&barred_after), CandidateMove::Drop);

    let at_limit = CandidateFacts {
        edits: EDITS_MAX,
        ..kept_facts(vec![late.clone()])
    };
    assert_eq!(candidate_move(&at_limit), CandidateMove::Drop);
    let below_limit = CandidateFacts {
        edits: EDITS_MAX - 1,
        ..kept_facts(vec![late.clone()])
    };
    assert_eq!(
        candidate_move(&below_limit),
        CandidateMove::Keep(vec![late])
    );
}

struct Row {
    state: EditState,
    list: Vec<Locator>,
    on_list: Vec<Locator>,
}

struct StepRows {
    compiled: Vec<Locator>,
    rows: Vec<Row>,
}

impl StepRows {
    fn barred(&self) -> Vec<Locator> {
        self.rows
            .iter()
            .filter(|row| row.state == EditState::Reverted)
            .flat_map(|row| brought(&row.list, &row.on_list))
            .collect()
    }

    fn base_list(&self) -> Vec<Locator> {
        self.rows
            .iter()
            .rev()
            .find(|row| row.state == EditState::Active)
            .map(|row| row.list.clone())
            .unwrap_or_default()
    }

    fn served(&self) -> Vec<Locator> {
        let mut served = self.base_list();
        served.extend(self.compiled.iter().cloned());
        served
    }

    fn admit(&self, proposed: &[Locator]) -> Result<(), AdmitCause> {
        let new = new_locators(proposed, &self.served());
        admit(&AdmitFacts {
            new_count: new.len(),
            rejected: is_rejected(&new, &self.barred()),
            edits_for_step: self.rows.len() as u32,
            ..admitted_facts()
        })
    }

    fn candidate_move(&self, proposed: &[Locator]) -> CandidateMove {
        candidate_move(&CandidateFacts {
            edits: self.rows.len() as u32,
            base_list: self.base_list(),
            barred: self.barred(),
            ..kept_facts(new_locators(proposed, &self.served()))
        })
    }
}

#[test]
fn refusal_steps_apart() {
    let a1 = loc("label", "Pay bill", None);
    let r1 = loc("text", "Pay this bill", None);
    let x = loc("css", "#settle", None);
    let mut first = StepRows {
        compiled: vec![loc("role", "button", Some("Pay"))],
        rows: vec![
            Row {
                state: EditState::Active,
                list: vec![a1.clone()],
                on_list: Vec::new(),
            },
            Row {
                state: EditState::Reverted,
                list: vec![r1.clone(), a1.clone()],
                on_list: vec![a1.clone()],
            },
        ],
    };
    while first.rows.len() < EDITS_MAX as usize {
        let n = first.rows.len();
        first.rows.push(Row {
            state: EditState::Active,
            list: vec![loc("label", &format!("Pay bill {n}"), None), a1.clone()],
            on_list: vec![a1.clone()],
        });
    }
    let second = StepRows {
        compiled: vec![loc("role", "link", Some("Account"))],
        rows: vec![Row {
            state: EditState::Active,
            list: vec![loc("label", "Account", None)],
            on_list: Vec::new(),
        }],
    };

    let proposals = [vec![r1.clone()], vec![x.clone()]];
    assert!(is_rejected(&proposals[0], &first.barred()));
    assert!(!is_rejected(&proposals[1], &first.barred()));
    assert!(!is_rejected(&proposals[0], &second.barred()));
    assert!(!is_rejected(&proposals[1], &second.barred()));

    assert_eq!(first.admit(&proposals[0]), Err(AdmitCause::Rejected));
    assert_eq!(first.admit(&proposals[1]), Err(AdmitCause::Full));
    assert_eq!(first.candidate_move(&proposals[0]), CandidateMove::Drop);
    assert_eq!(first.candidate_move(&proposals[1]), CandidateMove::Drop);

    assert_eq!(second.admit(&proposals[0]), Ok(()));
    assert_eq!(second.admit(&proposals[1]), Ok(()));
    assert_eq!(
        second.candidate_move(&proposals[0]),
        CandidateMove::Keep(vec![r1])
    );
    assert_eq!(
        second.candidate_move(&proposals[1]),
        CandidateMove::Keep(vec![x])
    );
}
