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
    let opening = marks
        .iter()
        .rposition(|mark| is_reading(mark) && rel(mark.at, first_letter) < 0)
        .ok_or_else(|| "no read or event before the first letter".to_string())?;
    let span: Vec<&Seen> = marks[opening..]
        .iter()
        .filter(|mark| is_reading(mark) && rel(mark.at, decided) <= 0)
        .collect();
    for mark in &span {
        let at = rel(mark.at, first_letter);
        if !mark.password {
            return Err(format!(
                "the {} at {at} ms reads another state than Password",
                mark.what
            ));
        }
        if let Some(element) = mark.element.as_deref() {
            if names_a_file(element) && !element.contains("is_password=1") {
                return Err(format!("the {} at {at} ms names {element}", mark.what));
            }
        }
    }
    Ok(span.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(what: &'static str, password: bool, element: Option<&str>, at: u32) -> Seen {
        Seen {
            what,
            password,
            element: element.map(str::to_string),
            at,
        }
    }

    const PAGE: &str = "framework=Chrome window_class=Chrome_WidgetWin_1 file=msedge.exe kind=Edge is_password=0 protected=0";

    fn run_28() -> Vec<Seen> {
        vec![
            seen("event", false, Some(PAGE), 9_500),
            seen("read", true, None, 9_700),
            seen("event", true, None, 9_800),
            seen("read", true, None, 9_900),
        ]
    }

    #[test]
    fn the_marks_of_run_28_are_a_reading_of_the_password_field() {
        assert_eq!(cold_evidence(&run_28(), 10_000, 10_000), Ok(1));
    }

    #[test]
    fn a_mark_that_names_a_plain_element_in_the_span_is_no_evidence() {
        let mut marks = run_28();
        marks.push(seen("event", true, Some(PAGE), 9_950));
        assert!(cold_evidence(&marks, 10_000, 10_000).is_err());
    }

    #[test]
    fn a_mark_that_reads_another_state_in_the_span_is_no_evidence() {
        let mut marks = run_28();
        marks.push(seen("read", false, None, 9_950));
        assert!(cold_evidence(&marks, 10_000, 10_000).is_err());
    }

    #[test]
    fn a_span_with_no_mark_before_the_letter_is_no_evidence() {
        let marks = vec![seen("read", true, None, 10_100)];
        assert!(cold_evidence(&marks, 10_000, 10_200).is_err());
    }

    #[test]
    fn the_marks_up_to_the_deciding_point_are_read_and_no_later_one() {
        let mut marks = run_28();
        marks.push(seen("read", true, None, 10_130));
        marks.push(seen("read", false, None, 10_400));
        assert_eq!(cold_evidence(&marks, 10_000, 10_200), Ok(2));
        assert!(cold_evidence(&marks, 10_000, 10_400).is_err());
    }

    #[test]
    fn the_older_mark_of_the_page_is_what_the_old_pick_read() {
        assert_eq!(newest_mark_of(&run_28(), "msedge.exe"), PAGE);
    }

    #[test]
    fn the_span_is_read_across_the_wrap_of_the_tick_count() {
        let marks = vec![
            seen("read", true, None, u32::MAX - 10),
            seen("read", true, None, 20),
        ];
        assert_eq!(cold_evidence(&marks, 5, 30), Ok(2));
        assert!(!names_a_file("file=none"));
    }
}
