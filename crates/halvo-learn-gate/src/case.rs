use std::path::Path;

use crate::forms;
use crate::page::{compiled_locators, control, Control, Role};

/// The most bytes of a case file (contracts.md L9.1).
pub const FILE_BYTES_MAX: u64 = 1_048_576;

/// The days after the first occurrence beyond which an occurrence with `restart` `yes` must stand (L9.2).
pub const RESTART_AFTER: u32 = 93;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Benign,
    Fault,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Benign => "benign",
            Kind::Fault => "fault",
        }
    }
}

/// The forms of L9.3, in the order of that item.
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
    fn read(text: &str) -> Option<Verb> {
        match text {
            "click" => Some(Verb::Click),
            "fill" => Some(Verb::Fill),
            "extract" => Some(Verb::Extract),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Step {
    pub n: u32,
    pub verb: Verb,
    pub control: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Occurrence {
    pub day: u32,
    pub page: Vec<Control>,
    /// The controls that the repairer names for each step, in the order of `steps`.
    pub repair: Vec<Vec<String>>,
    pub flaky: Vec<u32>,
    pub restart: bool,
}

/// A case file that the reader accepted (L9).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Case {
    pub name: String,
    pub kind: Kind,
    pub form: Form,
    pub shows: u32,
    pub steps: Vec<Step>,
    pub occurrences: Vec<Occurrence>,
}

/// A value of the compact JSON of a case line: the reader knows no other kind of value. A number past 64 bits
/// is held as none, which every limit of L9 refuses.
enum Value {
    Text(String),
    Number(Option<u64>),
    List(Vec<Value>),
    Object(Vec<(String, Value)>),
}

struct Bytes<'a> {
    text: &'a [u8],
    at: usize,
}

impl Bytes<'_> {
    fn next(&mut self) -> Option<u8> {
        let byte = self.text.get(self.at).copied();
        self.at += 1;
        byte
    }

    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn value(&mut self) -> Option<Value> {
        match self.peek()? {
            b'"' => self.text_value().map(Value::Text),
            b'[' => {
                self.at += 1;
                let mut items = Vec::new();
                if self.peek()? == b']' {
                    self.at += 1;
                    return Some(Value::List(items));
                }
                loop {
                    items.push(self.value()?);
                    match self.next()? {
                        b',' => continue,
                        b']' => return Some(Value::List(items)),
                        _ => return None,
                    }
                }
            }
            b'{' => {
                self.at += 1;
                let mut pairs = Vec::new();
                if self.peek()? == b'}' {
                    self.at += 1;
                    return Some(Value::Object(pairs));
                }
                loop {
                    let key = self.text_value()?;
                    if self.next()? != b':' {
                        return None;
                    }
                    pairs.push((key, self.value()?));
                    match self.next()? {
                        b',' => continue,
                        b'}' => return Some(Value::Object(pairs)),
                        _ => return None,
                    }
                }
            }
            b'0'..=b'9' => {
                let start = self.at;
                while self.peek().is_some_and(|b| b.is_ascii_digit()) {
                    self.at += 1;
                }
                let digits = &self.text[start..self.at];
                if digits.len() > 1 && digits[0] == b'0' {
                    return None;
                }
                let number = digits.iter().try_fold(0u64, |sum, d| {
                    sum.checked_mul(10)?.checked_add(u64::from(d - b'0'))
                });
                Some(Value::Number(number))
            }
            _ => None,
        }
    }

    fn text_value(&mut self) -> Option<String> {
        if self.next()? != b'"' {
            return None;
        }
        let mut out = Vec::new();
        loop {
            match self.next()? {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => match self.next()? {
                    escaped @ (b'"' | b'\\') => out.push(escaped),
                    _ => return None,
                },
                byte if byte < 0x20 => return None,
                byte => out.push(byte),
            }
        }
    }
}

fn parse_line(line: &str) -> Option<Value> {
    let mut bytes = Bytes {
        text: line.as_bytes(),
        at: 0,
    };
    let value = bytes.value()?;
    (bytes.at == line.len()).then_some(value)
}

fn object(value: Value, keys: &[&str]) -> Option<Vec<Value>> {
    let Value::Object(pairs) = value else {
        return None;
    };
    if pairs.len() != keys.len() || pairs.iter().zip(keys).any(|((k, _), want)| k != want) {
        return None;
    }
    Some(pairs.into_iter().map(|(_, v)| v).collect())
}

fn text(value: Value) -> Option<String> {
    match value {
        Value::Text(t) => Some(t),
        _ => None,
    }
}

fn number(value: Value) -> Option<Option<u64>> {
    match value {
        Value::Number(n) => Some(n),
        _ => None,
    }
}

fn list<T>(value: Value, item: impl Fn(Value) -> Option<T>) -> Option<Vec<T>> {
    match value {
        Value::List(items) => items.into_iter().map(item).collect(),
        _ => None,
    }
}

struct HeaderLine {
    case: String,
    kind: String,
    form: String,
    shows: Option<u64>,
    steps: Vec<(Option<u64>, String, String)>,
}

fn header_line(line: &str) -> Option<HeaderLine> {
    let mut v = object(
        parse_line(line)?,
        &["case", "kind", "form", "shows", "steps"],
    )?
    .into_iter();
    let case = text(v.next()?)?;
    let kind = text(v.next()?)?;
    let form = text(v.next()?)?;
    let shows = number(v.next()?)?;
    let steps = list(v.next()?, |step| {
        let mut s = object(step, &["n", "verb", "control"])?.into_iter();
        Some((number(s.next()?)?, text(s.next()?)?, text(s.next()?)?))
    })?;
    Some(HeaderLine {
        case,
        kind,
        form,
        shows,
        steps,
    })
}

struct ControlLine {
    id: String,
    role: String,
    name: String,
    test_id: String,
    passes: Vec<Option<u64>>,
}

struct OccurrenceLine {
    day: Option<u64>,
    page: Vec<ControlLine>,
    repair: Vec<(String, Vec<String>)>,
    flaky: Vec<Option<u64>>,
    restart: String,
}

fn occurrence_line(line: &str) -> Option<OccurrenceLine> {
    let keys = ["day", "page", "repair", "flaky", "restart"];
    let mut v = object(parse_line(line)?, &keys)?.into_iter();
    let day = number(v.next()?)?;
    let page = list(v.next()?, |c| {
        let keys = ["id", "role", "name", "test_id", "passes"];
        let mut c = object(c, &keys)?.into_iter();
        Some(ControlLine {
            id: text(c.next()?)?,
            role: text(c.next()?)?,
            name: text(c.next()?)?,
            test_id: text(c.next()?)?,
            passes: list(c.next()?, number)?,
        })
    })?;
    let Value::Object(pairs) = v.next()? else {
        return None;
    };
    let repair = pairs
        .into_iter()
        .map(|(key, named)| Some((key, list(named, text)?)))
        .collect::<Option<Vec<_>>>()?;
    let flaky = list(v.next()?, number)?;
    let restart = text(v.next()?)?;
    Some(OccurrenceLine {
        day,
        page,
        repair,
        flaky,
        restart,
    })
}

fn is_word(text: &str, max: usize) -> bool {
    (1..=max).contains(&text.len())
        && text
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'-'))
}

fn is_printable(text: &str) -> bool {
    (1..=64).contains(&text.len()) && text.bytes().all(|b| (0x20..=0x7e).contains(&b))
}

fn whole(number: Option<u64>) -> Option<u32> {
    number
        .and_then(|n| u32::try_from(n).ok())
        .filter(|n| *n >= 1)
}

/// The case name of a file name `<name>.jsonl`, or the rule that the name breaks (L9.1).
pub fn case_name(file_name: &str) -> Result<&str, String> {
    let name = file_name
        .strip_suffix(".jsonl")
        .ok_or("the file name does not end in .jsonl")?;
    if !is_word(name, 32) {
        return Err("the case name is not 1 to 32 bytes of a to z, 0 to 9 and -".into());
    }
    Ok(name)
}

/// Reads one case file from its bytes, with `calendar_days_min` of the targets file: the case, or the rule of
/// L9 that the file breaks.
pub fn read_case(file_name: &str, bytes: &[u8], calendar_days_min: u64) -> Result<Case, String> {
    let name = case_name(file_name)?;
    if u64::try_from(bytes.len()).map_or(true, |n| n > FILE_BYTES_MAX) {
        return Err("the file holds more than 1,048,576 bytes".into());
    }
    let body = std::str::from_utf8(bytes)
        .ok()
        .and_then(|t| t.strip_suffix('\n'))
        .filter(|t| !t.is_empty() && !t.starts_with('\u{feff}') && !t.contains('\r'))
        .ok_or("the file is not a text file of L0.4")?;
    let mut lines = body.split('\n');
    let header = lines
        .next()
        .and_then(header_line)
        .ok_or("line 1 is not the header of L9.1")?;
    let occurrence_lines = lines
        .enumerate()
        .map(|(i, line)| {
            occurrence_line(line).ok_or(format!("line {} is not an occurrence of L9.1", i + 2))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if header.case != name {
        return Err("the header's case is not the file name without .jsonl".into());
    }
    let form = Form::ALL
        .into_iter()
        .find(|f| f.as_str() == header.form)
        .ok_or("the form is not one of L9.3")?;
    let kind = match header.kind.as_str() {
        "benign" => Kind::Benign,
        "fault" => Kind::Fault,
        _ => return Err("the kind is not benign or fault".into()),
    };
    let shows = match header.shows {
        Some(s @ 0..=2) => s as u32,
        _ => return Err("shows is not 0, 1 or 2".into()),
    };
    if !(1..=8).contains(&header.steps.len()) {
        return Err("the case does not have 1 to 8 steps".into());
    }
    let mut steps = Vec::new();
    for (n, (number, verb, intended)) in (1u32..).zip(header.steps) {
        if number != Some(u64::from(n)) {
            return Err(format!("step {n} is not numbered {n}"));
        }
        let verb = Verb::read(&verb).ok_or(format!("step {n} has no verb of L9.1"))?;
        if !is_word(&intended, 16) {
            return Err(format!("step {n} names no control id"));
        }
        steps.push(Step {
            n,
            verb,
            control: intended,
        });
    }
    if !(8..=16).contains(&occurrence_lines.len()) {
        return Err("the case does not have 8 to 16 occurrences".into());
    }
    let mut occurrences = Vec::new();
    let mut last_day = 0u32;
    for (i, line) in occurrence_lines.into_iter().enumerate() {
        let at = i + 2;
        let occurrence = read_occurrence(line, &steps, at)?;
        if occurrence.day <= last_day {
            return Err(format!("line {at}: the day does not rise"));
        }
        last_day = occurrence.day;
        occurrences.push(occurrence);
    }
    let first_day = occurrences[0].day;
    if u64::from(last_day - first_day) < calendar_days_min {
        return Err("the case spans fewer days than calendar_days_min".into());
    }
    for step in &steps {
        let intended = control(&occurrences[0].page, &step.control)
            .ok_or("the first page lacks an intended control")?;
        for locator in compiled_locators(intended) {
            let found = locator.resolve(&occurrences[0].page);
            if found.map(|c| c.id.as_str()) != Some(step.control.as_str()) {
                return Err(format!(
                    "a compiled locator of step {} does not resolve to its intended control on the first page",
                    step.n
                ));
            }
        }
    }
    let restarted = occurrences
        .iter()
        .any(|o| o.restart && o.day - first_day > RESTART_AFTER);
    if !restarted {
        return Err("no occurrence more than 93 days after the first holds restart yes".into());
    }
    let case = Case {
        name: name.to_owned(),
        kind,
        form,
        shows,
        steps,
        occurrences,
    };
    forms::check(&case).map_err(|rule| format!("{}: {rule}", case.form.as_str()))?;
    Ok(case)
}

fn read_occurrence(line: OccurrenceLine, steps: &[Step], at: usize) -> Result<Occurrence, String> {
    let day = whole(line.day).ok_or(format!("line {at}: the day is outside 1 to 4294967295"))?;
    if line.page.len() > 16 {
        return Err(format!("line {at}: the page holds more than 16 controls"));
    }
    let mut page: Vec<Control> = Vec::new();
    for c in line.page {
        if !is_word(&c.id, 16) || page.iter().any(|p| p.id == c.id) {
            return Err(format!(
                "line {at}: a control id is not of L9.2 or stands twice"
            ));
        }
        let role = Role::read(&c.role).ok_or(format!("line {at}: a role is not of L9.2"))?;
        if !is_printable(&c.name) || !is_printable(&c.test_id) {
            return Err(format!(
                "line {at}: a name or a test id is not 1 to 64 bytes of printable ASCII"
            ));
        }
        let passes = c
            .passes
            .into_iter()
            .map(whole)
            .collect::<Option<Vec<u32>>>()
            .ok_or(format!(
                "line {at}: passes holds a number outside 1 to 4294967295"
            ))?;
        page.push(Control {
            id: c.id,
            role,
            name: c.name,
            test_id: c.test_id,
            passes,
        });
    }
    let keys: Vec<String> = steps.iter().map(|s| s.n.to_string()).collect();
    if line.repair.len() != keys.len()
        || line
            .repair
            .iter()
            .zip(&keys)
            .any(|((k, _), want)| k != want)
    {
        return Err(format!(
            "line {at}: the keys of repair are not the step numbers in order"
        ));
    }
    let mut repair = Vec::new();
    for ((_, named), step) in line.repair.into_iter().zip(steps) {
        if control(&page, &step.control).is_none() {
            return Err(format!(
                "line {at}: the page lacks the intended control of step {}",
                step.n
            ));
        }
        if !(1..=2).contains(&named.len()) || named.iter().any(|id| control(&page, id).is_none()) {
            return Err(format!(
                "line {at}: repair of step {} names no control of the page, or more than two",
                step.n
            ));
        }
        if named.last() != Some(&step.control) {
            return Err(format!(
                "line {at}: repair of step {} does not end with its intended control",
                step.n
            ));
        }
        repair.push(named);
    }
    let flaky = line
        .flaky
        .into_iter()
        .map(whole)
        .collect::<Option<Vec<u32>>>()
        .ok_or(format!(
            "line {at}: flaky holds a number outside 1 to 4294967295"
        ))?;
    let restart = match line.restart.as_str() {
        "yes" => true,
        "no" => false,
        _ => return Err(format!("line {at}: restart is not yes or no")),
    };
    Ok(Occurrence {
        day,
        page,
        repair,
        flaky,
        restart,
    })
}

/// Reads every case of a directory in byte order of the file names, or the refusal line's detail
/// `<file name>: <rule>` of the first file that the reader refuses (L9.4). A directory that holds an entry other
/// than a file `<name>.jsonl` is refused.
pub fn read_dir(dir: &Path, calendar_days_min: u64) -> Result<Vec<Case>, String> {
    let entries = std::fs::read_dir(dir).map_err(|_| "the directory cannot be read".to_owned())?;
    let mut names = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|_| "the directory cannot be read".to_owned())?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "an entry whose name is not UTF-8".to_owned())?;
        let is_file = entry.file_type().is_ok_and(|t| t.is_file());
        if !is_file || case_name(&name).is_err() {
            return Err(format!(
                "{name}: the directory holds an entry that is not a file <name>.jsonl"
            ));
        }
        names.push(name);
    }
    names.sort();
    let mut cases = Vec::new();
    for name in names {
        cases.push(read_file(&dir.join(&name), &name, calendar_days_min)?);
    }
    Ok(cases)
}

/// Reads one case file at a path, testing its size before it reads it.
pub fn read_file(path: &Path, name: &str, calendar_days_min: u64) -> Result<Case, String> {
    case_name(name).map_err(|rule| format!("{name}: {rule}"))?;
    let size = std::fs::metadata(path)
        .map_err(|_| format!("{name}: the file cannot be read"))?
        .len();
    if size > FILE_BYTES_MAX {
        return Err(format!("{name}: the file holds more than 1,048,576 bytes"));
    }
    let bytes = std::fs::read(path).map_err(|_| format!("{name}: the file cannot be read"))?;
    read_case(name, &bytes, calendar_days_min).map_err(|rule| format!("{name}: {rule}"))
}
