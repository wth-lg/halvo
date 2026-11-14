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
