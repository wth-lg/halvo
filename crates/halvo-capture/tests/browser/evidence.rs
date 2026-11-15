#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Seen {
    pub(crate) what: &'static str,
    pub(crate) password: bool,
    pub(crate) element: Option<String>,
    pub(crate) at: u32,
}

fn rel(tick: u32, from: u32) -> i64 {
    i64::from(tick.wrapping_sub(from) as i32)
}

fn names_a_file(element: &str) -> bool {
    element
        .split(' ')
        .any(|part| part.starts_with("file=") && part != "file=none")
}

pub(crate) fn newest_mark_of(marks: &[Seen], file: &str) -> String {
    let wanted = format!("file={}", file.to_ascii_lowercase());
    marks
        .iter()
        .rev()
        .filter_map(|mark| mark.element.as_deref())
        .find(|element| element.to_ascii_lowercase().contains(&wanted))
        .unwrap_or("element=none")
        .to_string()
}

pub(crate) fn cold_evidence(
    marks: &[Seen],
    first_letter: u32,
    decided: u32,
) -> Result<usize, String> {
    let is_reading = |mark: &Seen| matches!(mark.what, "read" | "event");
    let reads_the_input = |mark: &Seen| {
        mark.password
            && mark
                .element
                .as_deref()
                .is_some_and(|element| element.contains("is_password=1"))
    };
    let mut kept = vec![false; marks.len()];
    let mut holding = false;
    for (index, mark) in marks.iter().enumerate() {
        if mark.what.starts_with("latch") || (mark.what == "event" && !reads_the_input(mark)) {
            holding = false;
        }
        if is_reading(mark) && !mark.password {
            holding = false;
        }
        if reads_the_input(mark) {
            holding = true;
        }
        kept[index] = holding;
    }
    let opening = marks
        .iter()
        .rposition(|mark| is_reading(mark) && rel(mark.at, first_letter) < 0)
        .ok_or_else(|| "no read or event before the first letter".to_string())?;
    let span: Vec<(usize, &Seen)> = marks
        .iter()
        .enumerate()
        .skip(opening)
        .filter(|(_, mark)| is_reading(mark) && rel(mark.at, decided) <= 0)
        .collect();
    for (index, mark) in &span {
        let at = rel(mark.at, first_letter);
        if !mark.password {
            return Err(format!(
                "the {} at {at} ms reads another state than Password",
                mark.what
            ));
        }
        if let Some(element) = mark.element.as_deref() {
            let held_before = *index > 0 && kept[*index - 1];
            if names_a_file(element) && !element.contains("is_password=1") && !held_before {
                return Err(format!("the {} at {at} ms names {element}", mark.what));
            }
        }
    }
    Ok(span.len())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Reading {
    Clear,
    Password,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Letter {
    pub(crate) input: u32,
    pub(crate) recorded: bool,
    pub(crate) not_cleared_in_time: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Mark {
    pub(crate) what: &'static str,
    pub(crate) reading: Option<Reading>,
    pub(crate) another_element: bool,
    pub(crate) began: u32,
    pub(crate) at: Option<u32>,
}

pub(crate) const HOLD_MS: i64 = 120;
pub(crate) const LATE_MS: i64 = 250;
pub(crate) const POLL_STEP_MS: i64 = 30;

pub(crate) fn second_control_allowed(
    letters: &[Letter],
    marks: &[Mark],
    gate_clear_before: bool,
    open_latches_before: usize,
    sink: usize,
) -> Result<(), &'static str> {
    if letters.iter().filter(|letter| letter.recorded).count() != sink {
        return Err("the_sink_differs_from_the_seam");
    }
    let missing: Vec<&Letter> = letters.iter().filter(|letter| !letter.recorded).collect();
    let (Some(first), Some(last)) = (missing.first(), missing.last()) else {
        return Err("no_letter_is_missing");
    };
    if !missing.iter().all(|letter| letter.not_cleared_in_time) {
        return Err("a_missing_letter_was_decided_by_another_rule");
    }
    if !gate_clear_before {
        return Err("the_gate_did_not_read_clear_before_the_letters");
    }
    let span_end = i64::from(last.input) + LATE_MS;
    let in_span = |tick: u32| {
        let at = rel(tick, first.input);
        at >= 0 && at <= rel(span_end as u32, first.input)
    };
    if open_latches_before > 0
        || marks
            .iter()
            .any(|mark| mark.what.starts_with("latch") && in_span(mark.began))
    {
        return Err("a_latch_was_open_at_a_missing_letter");
    }
    if marks.iter().any(|mark| {
        matches!(mark.what, "read" | "event" | "start")
            && in_span(mark.at.unwrap_or(mark.began))
            && (mark.reading != Some(Reading::Clear) || mark.another_element)
    }) {
        return Err("a_mark_through_the_letters_was_not_a_clear_reading_of_the_same_element");
    }
    let mut reads: Vec<(u32, u32, bool)> = marks
        .iter()
        .filter(|mark| mark.what == "read")
        .filter_map(|mark| {
            mark.at.map(|at| {
                let clear = mark.reading == Some(Reading::Clear) && !mark.another_element;
                (mark.began, at, clear)
            })
        })
        .collect();
    reads.sort_by_key(|(began, _, _)| rel(*began, first.input));
    for letter in &missing {
        let hold_end = letter.input.wrapping_add(HOLD_MS as u32);
        let Some(&(began, at, clear)) = reads
            .iter()
            .find(|(began, _, _)| rel(*began, hold_end) >= 0)
        else {
            return Err("no_read_began_after_a_missing_letters_hold");
        };
        let asked_at_the_hold = rel(began, hold_end) <= POLL_STEP_MS;
        let asked_behind_a_read_out_then = reads.iter().any(|(out_began, out_at, _)| {
            rel(*out_began, hold_end) < 0
                && rel(*out_at, hold_end) > 0
                && rel(began, *out_at) >= 0
                && rel(began, *out_at) <= POLL_STEP_MS
        });
        if !asked_at_the_hold && !asked_behind_a_read_out_then {
            return Err("a_missing_letters_read_was_not_asked_on_time");
        }
        if rel(at, letter.input) <= LATE_MS {
            return Err("a_missing_letters_read_was_answered_within_its_250_ms");
        }
        if !clear {
            return Err("a_missing_letters_read_was_not_a_clear_reading_of_the_same_element");
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Typed {
    pub(crate) recorded: bool,
    pub(crate) state_at_the_key: bool,
}

pub(crate) struct AfterStart<'a> {
    pub(crate) letters: &'a [Typed],
    pub(crate) marks: &'a [Mark],
    pub(crate) gate_unknown_before: bool,
    pub(crate) open_latches_before: usize,
    pub(crate) front_tick: u32,
    pub(crate) handler_counts_unchanged: bool,
    pub(crate) page_moved_its_focus: bool,
}

pub(crate) fn after_start_state_met(seen: &AfterStart) -> Result<(), &'static str> {
    if seen.letters.is_empty() {
        return Err("no_letter_was_decided");
    }
    if !seen.gate_unknown_before {
        return Err("the_gate_did_not_read_unknown_before_the_letters");
    }
    if !seen
        .letters
        .iter()
        .all(|letter| !letter.recorded && letter.state_at_the_key)
    {
        return Err("a_letter_was_not_withheld_by_the_state_at_the_key");
    }
    if seen.open_latches_before > 0 || seen.marks.iter().any(|mark| mark.what.starts_with("latch"))
    {
        return Err("a_latch_was_open_or_set_since_the_start");
    }
    if seen.marks.iter().any(|mark| {
        matches!(mark.what, "read" | "event")
            && rel(mark.at.unwrap_or(mark.began), seen.front_tick) >= 0
    }) {
        return Err("an_event_or_own_read_came_since_the_page_came_to_the_front");
    }
    if !seen.handler_counts_unchanged {
        return Err("the_handler_counts_moved_across_the_pages_own_moves");
    }
    if !seen.page_moved_its_focus {
        return Err("the_page_has_no_record_that_it_moved_its_focus");
    }
    Ok(())
}
