#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct PageStep {
    pub(crate) phase: String,
    pub(crate) hold: u64,
    pub(crate) step: u32,
}

pub(crate) fn steps_of(test_steps: &[(&str, u64)]) -> Vec<PageStep> {
    test_steps
        .iter()
        .zip(0..)
        .map(|((phase, hold), step)| PageStep {
            phase: (*phase).to_string(),
            hold: *hold,
            step,
        })
        .collect()
}

pub(crate) fn address_of(steps: &[PageStep]) -> String {
    steps
        .iter()
        .map(|at| format!("{}:{}", at.phase, at.hold))
        .collect::<Vec<_>>()
        .join(",")
}

pub(crate) fn on_its_step(read: Option<&PageStep>, want: &PageStep) -> Result<(), &'static str> {
    let Some(read) = read else {
        return Err("the_page_named_no_step");
    };
    if read.step != want.step {
        return Err("the_page_is_on_another_step");
    }
    if read.phase != want.phase || read.hold != want.hold {
        return Err("the_page_is_in_another_phase");
    }
    Ok(())
}

fn at(phase: &str, hold: u64, step: u32) -> PageStep {
    PageStep {
        phase: phase.to_string(),
        hold,
        step,
    }
}

#[test]
#[ignore = "a made-up page read: 52-windows-browser runs it by its exact name"]
fn a_round_on_its_own_step_of_the_page_counts() {
    let want = at("tab", 300, 2);
    assert_eq!(on_its_step(Some(&want.clone()), &want), Ok(()));
}

#[test]
#[ignore = "a made-up page read: 52-windows-browser runs it by its exact name"]
fn a_round_on_another_step_of_the_page_does_not_count() {
    let want = at("tab", 300, 2);
    for read in [at("tab", 150, 1), at("tab", 300, 1), at("tab", 300, 3)] {
        assert_eq!(
            on_its_step(Some(&read), &want),
            Err("the_page_is_on_another_step"),
            "a round read on step {} counted for step {}",
            read.step,
            want.step
        );
    }
}

#[test]
#[ignore = "a made-up page read: 52-windows-browser runs it by its exact name"]
fn a_round_on_the_page_in_another_phase_or_hold_at_its_step_does_not_count() {
    let want = at("click", 150, 3);
    for read in [at("tab", 150, 3), at("click", 300, 3), at("control", 0, 3)] {
        assert_eq!(
            on_its_step(Some(&read), &want),
            Err("the_page_is_in_another_phase"),
            "a round read in phase {} at the hold {} counted",
            read.phase,
            read.hold
        );
    }
}

#[test]
#[ignore = "a made-up page read: 52-windows-browser runs it by its exact name"]
fn a_page_that_names_no_step_does_not_count() {
    assert_eq!(
        on_its_step(None, &at("control", 0, 0)),
        Err("the_page_named_no_step")
    );
}

#[test]
#[ignore = "a made-up page read: 52-windows-browser runs it by its exact name"]
fn the_steps_of_a_page_go_into_its_address_in_order() {
    let steps = steps_of(&[("control", 0), ("tab", 150), ("click", 300), ("control", 0)]);
    assert_eq!(
        steps.iter().map(|at| at.step).collect::<Vec<_>>(),
        vec![0, 1, 2, 3]
    );
    let read: Vec<(String, u64)> = address_of(&steps)
        .split(',')
        .filter_map(|at| {
            let (phase, hold) = at.split_once(':')?;
            Some((phase.to_string(), hold.parse().ok()?))
        })
        .collect();
    assert_eq!(
        read,
        vec![
            ("control".to_string(), 0),
            ("tab".to_string(), 150),
            ("click".to_string(), 300),
            ("control".to_string(), 0),
        ]
    );
}
