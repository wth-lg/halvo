use crate::forms;
use crate::json::{Cursor, NotShape};
use crate::model;

pub const NAME_MAX: usize = 32;
pub const ID_MAX: usize = 16;
pub const LABEL_MAX: usize = 64;
pub const STEPS_MAX: usize = 8;
pub const PAGE_MAX: usize = 16;
pub const OCCURRENCES_MIN: usize = 8;
pub const OCCURRENCES_MAX: usize = 16;
pub const RESTART_AFTER_DAYS: u64 = 93;
pub const BYTES_MAX: u64 = 1_048_576;

pub const FILE_NAME: &str = "the file name is not <name>.jsonl";
pub const SIZE: &str = "the file holds more than 1048576 bytes";
pub const TEXT: &str = "the file is not UTF-8 text with LF line ends and one LF at its end";
pub const HEADER_NAME: &str = "the header case is not the file name without .jsonl";
pub const NAME: &str = "the case name is not 1 to 32 bytes of a to z, 0 to 9 and -";
pub const RESTART: &str = "no occurrence more than 93 days after the first holds restart yes";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Benign,
    Fault,
}

impl Kind {
    pub const ALL: [Kind; 2] = [Kind::Benign, Kind::Fault];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Benign => "benign",
            Kind::Fault => "fault",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    Renamed,
    RenamedTwice,
    Alternates,
    Flaky,
    Decoy,
    Reused,
}

impl Form {
    pub const ALL: [Form; 6] = [
        Form::Renamed,
        Form::RenamedTwice,
        Form::Alternates,
        Form::Flaky,
        Form::Decoy,
        Form::Reused,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Form::Renamed => "renamed",
            Form::RenamedTwice => "renamed-twice",
            Form::Alternates => "alternates",
            Form::Flaky => "flaky",
            Form::Decoy => "decoy",
            Form::Reused => "reused",
        }
    }

    pub fn kind(self) -> Kind {
        match self {
            Form::Decoy | Form::Reused => Kind::Fault,
            Form::Renamed | Form::RenamedTwice | Form::Alternates | Form::Flaky => Kind::Benign,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Click,
    Fill,
    Extract,
}

impl Verb {
    pub const ALL: [Verb; 3] = [Verb::Click, Verb::Fill, Verb::Extract];

    pub fn as_str(self) -> &'static str {
        match self {
            Verb::Click => "click",
            Verb::Fill => "fill",
            Verb::Extract => "extract",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Button,
    Link,
    Textbox,
    Checkbox,
    Combobox,
    Text,
}

impl Role {
    pub const ALL: [Role; 6] = [
        Role::Button,
        Role::Link,
        Role::Textbox,
        Role::Checkbox,
        Role::Combobox,
        Role::Text,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Role::Button => "button",
            Role::Link => "link",
            Role::Textbox => "textbox",
            Role::Checkbox => "checkbox",
            Role::Combobox => "combobox",
            Role::Text => "text",
        }
    }
}

fn named<T: Copy>(all: &[T], text: &str, name: fn(T) -> &'static str) -> Option<T> {
    all.iter().copied().find(|value| name(*value) == text)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub n: u32,
    pub verb: Verb,
    pub control: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Control {
    pub id: String,
    pub role: Role,
    pub name: String,
    pub test_id: String,
    pub passes: Vec<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occurrence {
    pub day: u32,
    pub page: Vec<Control>,
    pub repair: Vec<Vec<String>>,
    pub flaky: Vec<u32>,
    pub restart: bool,
}

impl Occurrence {
    pub fn control(&self, id: &str) -> Option<&Control> {
        self.page.iter().find(|control| control.id == id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Case {
    pub name: String,
    pub kind: Kind,
    pub form: Form,
    pub shows: u32,
    pub steps: Vec<Step>,
    pub occurrences: Vec<Occurrence>,
}

struct RawStep {
    n: u64,
    verb: String,
    control: String,
}

struct RawHeader {
    case: String,
    kind: String,
    form: String,
    shows: u64,
    steps: Vec<RawStep>,
}

struct RawControl {
    id: String,
    role: String,
    name: String,
    test_id: String,
    passes: Vec<u64>,
}

struct RawOccurrence {
    day: u64,
    page: Vec<RawControl>,
    repair: Vec<(String, Vec<String>)>,
    flaky: Vec<u64>,
    restart: String,
}

fn raw_header(line: &str) -> Result<RawHeader, NotShape> {
    let mut c = Cursor::new(line);
    c.literal("{\"case\":")?;
    let case = c.string()?;
    c.literal(",\"kind\":")?;
    let kind = c.string()?;
    c.literal(",\"form\":")?;
    let form = c.string()?;
    c.literal(",\"shows\":")?;
    let shows = c.number()?;
    c.literal(",\"steps\":")?;
    let steps = c.list(|c| {
        c.literal("{\"n\":")?;
        let n = c.number()?;
        c.literal(",\"verb\":")?;
        let verb = c.string()?;
        c.literal(",\"control\":")?;
        let control = c.string()?;
        c.literal("}")?;
        Ok(RawStep { n, verb, control })
    })?;
    c.literal("}")?;
    c.end()?;
    Ok(RawHeader {
        case,
        kind,
        form,
        shows,
        steps,
    })
}

fn raw_occurrence(line: &str) -> Result<RawOccurrence, NotShape> {
    let mut c = Cursor::new(line);
    c.literal("{\"day\":")?;
    let day = c.number()?;
    c.literal(",\"page\":")?;
    let page = c.list(|c| {
        c.literal("{\"id\":")?;
        let id = c.string()?;
        c.literal(",\"role\":")?;
        let role = c.string()?;
        c.literal(",\"name\":")?;
        let name = c.string()?;
        c.literal(",\"test_id\":")?;
        let test_id = c.string()?;
        c.literal(",\"passes\":")?;
        let passes = c.list(|c| c.number())?;
        c.literal("}")?;
        Ok(RawControl {
            id,
            role,
            name,
            test_id,
            passes,
        })
    })?;
    c.literal(",\"repair\":")?;
    let repair = c.object(|c| c.list(|c| c.string()))?;
    c.literal(",\"flaky\":")?;
    let flaky = c.list(|c| c.number())?;
    c.literal(",\"restart\":")?;
    let restart = c.string()?;
    c.literal("}")?;
    c.end()?;
    Ok(RawOccurrence {
        day,
        page,
        repair,
        flaky,
        restart,
    })
}

fn is_id(text: &str, max: usize) -> bool {
    (1..=max).contains(&text.len())
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn is_label(text: &str) -> bool {
    (1..=LABEL_MAX).contains(&text.len()) && text.bytes().all(|b| (0x20..=0x7e).contains(&b))
}

fn count(value: u64) -> Option<u32> {
    u32::try_from(value).ok().filter(|value| *value >= 1)
}

fn not_shape(line: usize) -> String {
    format!("line {line} is not the shape of L9.1")
}

fn header_limits(header: &RawHeader) -> Result<(Kind, Form, u32, Vec<Step>), String> {
    let kind = named(&Kind::ALL, &header.kind, Kind::as_str)
        .ok_or_else(|| "line 1: kind is not benign or fault".to_owned())?;
    let form = named(&Form::ALL, &header.form, Form::as_str)
        .ok_or_else(|| "line 1: form is not one of the six forms of L9.3".to_owned())?;
    let shows = u32::try_from(header.shows)
        .ok()
        .filter(|shows| *shows <= 2)
        .ok_or_else(|| "line 1: shows is not 0, 1 or 2".to_owned())?;
    if !(1..=STEPS_MAX).contains(&header.steps.len()) {
        return Err(format!(
            "line 1: the case has {} steps, not 1 to 8",
            header.steps.len()
        ));
    }
    let mut steps = Vec::new();
    for (number, raw) in (1u32..).zip(&header.steps) {
        if raw.n != u64::from(number) {
            return Err(format!("line 1: step {number} is not numbered {number}"));
        }
        let verb = named(&Verb::ALL, &raw.verb, Verb::as_str)
            .ok_or_else(|| format!("line 1: step {number}: verb is not click, fill or extract"))?;
        if !is_id(&raw.control, ID_MAX) {
            return Err(format!(
                "line 1: step {number}: control is not a control id"
            ));
        }
        steps.push(Step {
            n: number,
            verb,
            control: raw.control.clone(),
        });
    }
    Ok((kind, form, shows, steps))
}

fn occurrence_limits(
    raw: &RawOccurrence,
    line: usize,
    previous_day: u64,
    steps: &[Step],
) -> Result<Occurrence, String> {
    let day =
        count(raw.day).ok_or_else(|| format!("line {line}: day is outside 1 to 4294967295"))?;
    if u64::from(day) <= previous_day {
        return Err(format!("line {line}: day does not rise"));
    }
    if raw.page.len() > PAGE_MAX {
        return Err(format!(
            "line {line}: the page holds {} controls, more than 16",
            raw.page.len()
        ));
    }
    let mut page: Vec<Control> = Vec::new();
    for (number, control) in (1usize..).zip(&raw.page) {
        let at = format!("line {line}: control {number}");
        if !is_id(&control.id, ID_MAX) {
            return Err(format!("{at}: id is not a control id"));
        }
        if page.iter().any(|kept| kept.id == control.id) {
            return Err(format!("{at}: id stands twice on the page"));
        }
        let role = named(&Role::ALL, &control.role, Role::as_str)
            .ok_or_else(|| format!("{at}: role is not one of the six roles"))?;
        if !is_label(&control.name) {
            return Err(format!(
                "{at}: name is not 1 to 64 bytes of printable ASCII"
            ));
        }
        if !is_label(&control.test_id) {
            return Err(format!(
                "{at}: test_id is not 1 to 64 bytes of printable ASCII"
            ));
        }
        let passes = control
            .passes
            .iter()
            .map(|pass| count(*pass))
            .collect::<Option<Vec<u32>>>()
            .ok_or_else(|| format!("{at}: passes holds a number outside 1 to 4294967295"))?;
        page.push(Control {
            id: control.id.clone(),
            role,
            name: control.name.clone(),
            test_id: control.test_id.clone(),
            passes,
        });
    }
    for step in steps {
        if page.iter().all(|control| control.id != step.control) {
            return Err(format!(
                "line {line}: the page lacks the intended control of step {}",
                step.n
            ));
        }
    }
    let flaky = raw
        .flaky
        .iter()
        .map(|step| count(*step))
        .collect::<Option<Vec<u32>>>()
        .ok_or_else(|| format!("line {line}: flaky holds a number outside 1 to 4294967295"))?;
    let restart = match raw.restart.as_str() {
        "yes" => true,
        "no" => false,
        _ => return Err(format!("line {line}: restart is not yes or no")),
    };
    Ok(Occurrence {
        day,
        page,
        repair: Vec::new(),
        flaky,
        restart,
    })
}

fn repair(
    raw: &RawOccurrence,
    line: usize,
    page: &[Control],
    steps: &[Step],
) -> Result<Vec<Vec<String>>, String> {
    for step in steps {
        if raw.repair.iter().all(|(key, _)| *key != step.n.to_string()) {
            return Err(format!("line {line}: repair lacks step {}", step.n));
        }
    }
    let in_order = raw.repair.len() == steps.len()
        && raw
            .repair
            .iter()
            .zip(steps)
            .all(|((key, _), step)| *key == step.n.to_string());
    if !in_order {
        return Err(format!(
            "line {line}: repair holds a key other than the step numbers in order"
        ));
    }
    for ((_, named), step) in raw.repair.iter().zip(steps) {
        let n = step.n;
        if !(1..=2).contains(&named.len()) {
            return Err(format!(
                "line {line}: repair of step {n} names {} controls, not 1 or 2",
                named.len()
            ));
        }
        if named
            .iter()
            .any(|id| page.iter().all(|control| control.id != *id))
        {
            return Err(format!(
                "line {line}: repair of step {n} names a control that the page lacks"
            ));
        }
        if named.last() != Some(&step.control) {
            return Err(format!(
                "line {line}: repair of step {n} does not name the intended control last"
            ));
        }
    }
    Ok(raw.repair.iter().map(|(_, named)| named.clone()).collect())
}

fn compiled_resolve(first: &Occurrence, steps: &[Step]) -> Result<(), String> {
    for step in steps {
        let compiled = model::compiled(&first.page, &step.control);
        for (label, locator) in ["test_id", "role"].into_iter().zip(&compiled) {
            let found = model::resolve(&first.page, locator).map(|control| control.id.as_str());
            if found != Some(step.control.as_str()) {
                return Err(format!(
                    "line 2: the compiled {label} locator of step {} does not resolve to its intended control",
                    step.n
                ));
            }
        }
    }
    Ok(())
}

pub fn named_and_sized(file_name: &str, bytes: u64) -> Result<&str, String> {
    let stem = file_name
        .strip_suffix(".jsonl")
        .ok_or_else(|| FILE_NAME.to_owned())?;
    if !is_id(stem, NAME_MAX) {
        return Err(NAME.to_owned());
    }
    if bytes > BYTES_MAX {
        return Err(SIZE.to_owned());
    }
    Ok(stem)
}

pub fn read_case(file_name: &str, bytes: &[u8], calendar_days_min: u64) -> Result<Case, String> {
    let stem = named_and_sized(file_name, u64::try_from(bytes.len()).unwrap_or(u64::MAX))?;
    let text = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| text.strip_suffix('\n'))
        .filter(|text| {
            !text.starts_with('\u{feff}') && !text.contains('\r') && !text.ends_with('\n')
        })
        .ok_or_else(|| TEXT.to_owned())?;
    let lines: Vec<&str> = text.split('\n').collect();
    let header = raw_header(lines[0]).map_err(|NotShape| not_shape(1))?;
    let raws = (2usize..)
        .zip(&lines[1..])
        .map(|(line, text)| raw_occurrence(text).map_err(|NotShape| not_shape(line)))
        .collect::<Result<Vec<_>, _>>()?;
    if header.case != stem {
        return Err(HEADER_NAME.to_owned());
    }
    let (kind, form, shows, steps) = header_limits(&header)?;
    if !(OCCURRENCES_MIN..=OCCURRENCES_MAX).contains(&raws.len()) {
        return Err(format!(
            "the case has {} occurrences, not 8 to 16",
            raws.len()
        ));
    }
    let mut occurrences: Vec<Occurrence> = Vec::new();
    for (line, raw) in (2usize..).zip(&raws) {
        let previous_day = occurrences
            .last()
            .map_or(0, |occurrence| u64::from(occurrence.day));
        occurrences.push(occurrence_limits(raw, line, previous_day, &steps)?);
    }
    let first_day = u64::from(occurrences[0].day);
    let span = u64::from(occurrences[occurrences.len() - 1].day) - first_day;
    if span < calendar_days_min {
        return Err(format!(
            "the case spans {span} days, fewer than {calendar_days_min}"
        ));
    }
    for (index, raw) in raws.iter().enumerate() {
        let named = repair(raw, index + 2, &occurrences[index].page, &steps)?;
        occurrences[index].repair = named;
    }
    compiled_resolve(&occurrences[0], &steps)?;
    let restarted = occurrences.iter().any(|occurrence| {
        occurrence.restart && u64::from(occurrence.day) - first_day > RESTART_AFTER_DAYS
    });
    if !restarted {
        return Err(RESTART.to_owned());
    }
    let case = Case {
        name: stem.to_owned(),
        kind,
        form,
        shows,
        steps,
        occurrences,
    };
    forms::check(&case)?;
    Ok(case)
}
