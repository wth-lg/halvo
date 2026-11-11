//! The gate statistics of the learning run (contracts.md L7.3): the exact
//! one-sided binomial tail in whole numbers, `critical(n)`, the stub and the
//! verdict of `sh tools/r5-trials/stat/run.sh`.

use std::fmt;
use std::path::Path;

pub const EXPECTED_PATH: &str = "tools/r5-trials/stat/expected.txt";
pub const TARGETS_PATH: &str = "docs/r5/targets.toml";

pub const ALPHA: Ratio = Ratio::new(1, 20);
pub const HALF: Ratio = Ratio::new(1, 2);
pub const NINE_TENTHS: Ratio = Ratio::new(9, 10);

pub const FIRST_N: u32 = 5;
pub const LAST_N: u32 = 24;
pub const STUB_N: u32 = 12;
pub const DEV_N: u32 = 8;
pub const HELDOUT_N: u32 = 12;

pub const SAME_PER_10000_MAX: u128 = 500;
pub const GAIN90_PER_10000_MIN: u128 = 8000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ratio {
    pub num: u128,
    pub den: u128,
}

impl Ratio {
    pub const fn new(num: u128, den: u128) -> Ratio {
        Ratio { num, den }
    }

    pub fn lowest(self) -> Ratio {
        let g = gcd(self.num, self.den);
        Ratio::new(self.num / g, self.den / g)
    }

    pub fn at_most(self, bound: Ratio) -> bool {
        self.num * bound.den <= bound.num * self.den
    }

    pub fn per_10000_rounded_down(self) -> u128 {
        self.num * 10_000 / self.den
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

pub fn choose(n: u32, k: u32) -> u128 {
    (0..k).fold(1u128, |acc, i| acc * u128::from(n - i) / u128::from(i + 1))
}

pub fn upper_tail(n: u32, c: u32, win: Ratio) -> Ratio {
    let no_win = win.den - win.num;
    let num = (c..=n)
        .map(|k| choose(n, k) * win.num.pow(k) * no_win.pow(n - k))
        .sum();
    Ratio::new(num, win.den.pow(n)).lowest()
}

pub fn critical(n: u32) -> Option<(u32, Ratio)> {
    (0..=n).find_map(|c| {
        let tail = upper_tail(n, c, HALF);
        tail.at_most(ALPHA).then_some((c, tail))
    })
}

pub fn critical_lines() -> Vec<String> {
    (FIRST_N..=LAST_N)
        .map(|n| match critical(n) {
            Some((c, tail)) => format!(
                "critical n={n} c={c} tail_num={} tail_den={}",
                tail.num, tail.den
            ),
            None => format!("critical n={n} c=- tail_num=- tail_den=-"),
        })
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stub {
    pub same_per_10000: u128,
    pub gain90_per_10000: u128,
}

pub fn stub() -> Stub {
    let c = critical(STUB_N).map_or(STUB_N + 1, |(c, _)| c);
    Stub {
        same_per_10000: upper_tail(STUB_N, c, HALF).per_10000_rounded_down(),
        gain90_per_10000: upper_tail(STUB_N, c, NINE_TENTHS).per_10000_rounded_down(),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Match {
    Same,
    Differ,
}

impl Match {
    fn of(equal: bool) -> Match {
        if equal {
            Match::Same
        } else {
            Match::Differ
        }
    }
}

impl fmt::Display for Match {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Match::Same => "same",
            Match::Differ => "differ",
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Targets {
    pub dev_critical: u64,
    pub heldout_critical: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Verdict {
    pub lines_equal: bool,
    pub stub: Stub,
    pub dev_critical: Match,
    pub heldout_critical: Match,
}

impl Verdict {
    pub fn ok(&self) -> bool {
        self.lines_equal
            && self.stub.same_per_10000 <= SAME_PER_10000_MAX
            && self.stub.gain90_per_10000 >= GAIN90_PER_10000_MIN
            && self.dev_critical == Match::Same
            && self.heldout_critical == Match::Same
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Usage(String),
    Input(String),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Usage(detail) => write!(f, "REFUSED usage: {detail}"),
            Refusal::Input(detail) => write!(f, "REFUSED input: {detail}"),
        }
    }
}

pub struct Block {
    pub text: String,
    pub verdict: Verdict,
}

fn refuse_input(detail: String) -> Refusal {
    Refusal::Input(detail)
}

fn text_lines<'a>(name: &str, text: &'a str) -> Result<Vec<&'a str>, Refusal> {
    let body = text
        .strip_suffix('\n')
        .filter(|body| !body.starts_with('\u{feff}') && !body.contains('\r'))
        .ok_or_else(|| refuse_input(format!("{name} is not LF text with one LF at its end")))?;
    Ok(body.split('\n').collect())
}

fn is_count(word: &str) -> bool {
    !word.is_empty()
        && word.bytes().all(|b| b.is_ascii_digit())
        && (word == "0" || !word.starts_with('0'))
}

fn is_critical_line(line: &str) -> bool {
    let words: Vec<&str> = line.split(' ').collect();
    let keys = ["n=", "c=", "tail_num=", "tail_den="];
    words.len() == 5
        && words[0] == "critical"
        && words[1..]
            .iter()
            .zip(keys)
            .all(|(word, key)| word.strip_prefix(key).is_some_and(is_count))
}

pub fn parse_expected(text: &str) -> Result<Vec<String>, Refusal> {
    let lines = text_lines("expected", text)?;
    let want = (LAST_N - FIRST_N + 1) as usize;
    if lines.len() != want {
        return Err(refuse_input(format!(
            "expected holds {} lines, not {want}",
            lines.len()
        )));
    }
    for (i, line) in lines.iter().enumerate() {
        if !is_critical_line(line) {
            return Err(refuse_input(format!(
                "expected line {} is not of its form",
                i + 1
            )));
        }
    }
    Ok(lines.into_iter().map(str::to_owned).collect())
}

fn is_name(word: &str) -> bool {
    !word.is_empty()
        && word
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

pub fn parse_targets(text: &str) -> Result<Targets, Refusal> {
    let mut table: Option<&str> = None;
    let mut seen: Vec<(&str, &str)> = Vec::new();
    let mut dev_critical = None;
    let mut heldout_critical = None;
    for (i, line) in text_lines("targets", text)?.into_iter().enumerate() {
        let bad = || refuse_input(format!("targets line {} is not of its form", i + 1));
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            if !is_name(name) || seen.iter().any(|(t, k)| *t == name && k.is_empty()) {
                return Err(bad());
            }
            seen.push((name, ""));
            table = Some(name);
            continue;
        }
        let (key, value) = line.split_once(" = ").ok_or_else(bad)?;
        let current = table.ok_or_else(bad)?;
        if !is_name(key) || !is_count(value) || seen.contains(&(current, key)) {
            return Err(bad());
        }
        seen.push((current, key));
        let value: u64 = value.parse().map_err(|_| bad())?;
        match (current, key) {
            ("gate", "dev_critical") => dev_critical = Some(value),
            ("gate", "heldout_critical") => heldout_critical = Some(value),
            _ => {}
        }
    }
    let absent = |key: &str| refuse_input(format!("targets key {key} is absent"));
    Ok(Targets {
        dev_critical: dev_critical.ok_or_else(|| absent("dev_critical"))?,
        heldout_critical: heldout_critical.ok_or_else(|| absent("heldout_critical"))?,
    })
}

fn critical_count(n: u32) -> Option<u64> {
    critical(n).map(|(c, _)| u64::from(c))
}

pub fn block(expected: &[String], targets: Targets) -> Block {
    let lines = critical_lines();
    let stub = stub();
    let verdict = Verdict {
        lines_equal: lines == expected,
        stub,
        dev_critical: Match::of(critical_count(DEV_N) == Some(targets.dev_critical)),
        heldout_critical: Match::of(critical_count(HELDOUT_N) == Some(targets.heldout_critical)),
    };
    let mut text = String::new();
    for line in &lines {
        text.push_str(line);
        text.push('\n');
    }
    text.push_str(&format!(
        "stub n={STUB_N} same_per_10000={} gain90_per_10000={}\n",
        stub.same_per_10000, stub.gain90_per_10000
    ));
    text.push_str(&format!(
        "targets dev_critical={} heldout_critical={}\n",
        verdict.dev_critical, verdict.heldout_critical
    ));
    text.push_str(&format!(
        "verdict stat={}\n",
        if verdict.ok() { "ok" } else { "miss" }
    ));
    Block { text, verdict }
}

fn read_text(name: &str, path: &Path) -> Result<String, Refusal> {
    std::fs::read(path)
        .map_err(|_| refuse_input(format!("{name} cannot be read")))
        .and_then(|bytes| {
            String::from_utf8(bytes).map_err(|_| refuse_input(format!("{name} is not UTF-8")))
        })
}

pub fn run(root: &Path) -> Result<Block, Refusal> {
    let expected = parse_expected(&read_text("expected", &root.join(EXPECTED_PATH))?)?;
    let targets = parse_targets(&read_text("targets", &root.join(TARGETS_PATH))?)?;
    Ok(block(&expected, targets))
}

#[cfg(test)]
mod tests;
