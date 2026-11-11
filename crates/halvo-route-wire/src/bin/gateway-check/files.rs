use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;

use halvo_route_wire::{
    AnswerError, BodyError, Reason, WireTier, MAX_BODY_BYTES, MAX_OUTPUT_TOKENS,
};

use crate::node::{compact, Node};

pub enum Refusal {
    Usage(String),
    File(String, usize),
}

impl fmt::Display for Refusal {
    fn fmt(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Refusal::Usage(detail) => write!(formatter, "usage: {detail}"),
            Refusal::File(name, line) => write!(formatter, "file: {name}:{line}"),
        }
    }
}

pub struct BodyVector {
    pub n: usize,
    pub name: String,
    pub valid: bool,
    pub canon: bool,
    pub error: Option<String>,
    pub body: Vec<u8>,
}

pub struct AnswerVector {
    pub n: usize,
    pub name: String,
    pub status: u16,
    pub body: Vec<u8>,
    pub expect: String,
    pub content: Option<String>,
    pub tokens_in: Option<u64>,
    pub tokens_out: Option<u64>,
    pub cost: Option<u64>,
}

pub struct Gateway {
    pub vectors: Vec<BodyVector>,
    pub answers: Vec<AnswerVector>,
    pub schema: Node,
    pub limits: String,
}

pub fn read(dir: &Path) -> Result<Gateway, Refusal> {
    let vectors = records(dir, "vectors.jsonl", &VECTOR_KEYS, body_vector)?;
    let answers = records(dir, "answers.jsonl", &ANSWER_KEYS, answer_vector)?;
    let schema = schema(dir)?;
    let limits = tsv(dir, "tier-limits.tsv")?;
    Ok(Gateway {
        vectors,
        answers,
        schema,
        limits,
    })
}

pub fn read_names(path: &Path) -> Result<BTreeSet<String>, Refusal> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "names".to_string());
    Ok(text_lines(path, &name)?.into_iter().collect())
}

pub fn limits_text() -> String {
    let mut text = String::from(
        "tier\tmax_body_bytes\tmax_output_tokens\thold_life_ms\tupstream_deadline_ms\tstatus\n",
    );
    for tier in WireTier::ALL {
        text.push_str(&format!(
            "{}\t{MAX_BODY_BYTES}\t{MAX_OUTPUT_TOKENS}\t120000\t90000\tprovisional\n",
            tier.as_str()
        ));
    }
    text
}

fn text_lines(path: &Path, name: &str) -> Result<Vec<String>, Refusal> {
    let refuse = |line| Refusal::File(name.to_string(), line);
    let bytes = fs::read(path).map_err(|_| refuse(0))?;
    let terminated = bytes.ends_with(b"\n");
    let parts: Vec<&[u8]> = bytes.split(|byte| *byte == b'\n').collect();
    let count = if terminated {
        parts.len() - 1
    } else {
        parts.len()
    };
    let mut lines = Vec::with_capacity(count);
    for (at, part) in parts.iter().take(count).enumerate() {
        let number = at + 1;
        if part.contains(&b'\r') || (!terminated && number == count) {
            return Err(refuse(number));
        }
        let text = std::str::from_utf8(part).map_err(|_| refuse(number))?;
        lines.push(text.to_string());
    }
    Ok(lines)
}

const VECTOR_KEYS: [&str; 6] = ["n", "name", "valid", "canon", "error", "body"];
const ANSWER_KEYS: [&str; 9] = [
    "n",
    "name",
    "status",
    "body",
    "expect",
    "content",
    "tokens_in",
    "tokens_out",
    "cost",
];

struct Line {
    values: Vec<Node>,
    hex: bool,
}

impl Line {
    fn bytes(&self, at: usize) -> Option<Vec<u8>> {
        let Node::Text(text) = &self.values[at] else {
            return None;
        };
        if self.hex {
            hex(text)
        } else {
            Some(text.as_bytes().to_vec())
        }
    }
}

fn hex(digits: &str) -> Option<Vec<u8>> {
    let value = |digit: u8| match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        _ => None,
    };
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    digits
        .as_bytes()
        .chunks(2)
        .map(|pair| Some(value(pair[0])? << 4 | value(pair[1])?))
        .collect()
}

type Build<T> = fn(&Line) -> Option<(usize, String, T)>;

fn records<T>(dir: &Path, name: &str, keys: &[&str], build: Build<T>) -> Result<Vec<T>, Refusal> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (at, line) in text_lines(&dir.join(name), name)?.iter().enumerate() {
        let number = at + 1;
        let fields = fields(line, keys).and_then(|fields| build(&fields));
        match fields {
            Some((n, record_name, record)) if n == number && seen.insert(record_name.clone()) => {
                out.push(record)
            }
            _ => return Err(Refusal::File(name.to_string(), number)),
        }
    }
    Ok(out)
}

fn fields(line: &str, keys: &[&str]) -> Option<Line> {
    if !compact(line) {
        return None;
    }
    let Node::Map(entries) = Node::parse(line)? else {
        return None;
    };
    let fits = entries.len() == keys.len()
        && entries
            .iter()
            .zip(keys)
            .all(|((name, _), key)| name == key || (*key == "body" && name == "hex"));
    if !fits {
        return None;
    }
    let hex = entries.iter().any(|(name, _)| name == "hex");
    let values = entries.into_iter().map(|(_, value)| value).collect();
    Some(Line { values, hex })
}

fn name_of(node: &Node) -> Option<String> {
    match node {
        Node::Text(name)
            if (1..=32).contains(&name.len())
                && name
                    .bytes()
                    .all(|byte| matches!(byte, b'a'..=b'z' | b'0'..=b'9' | b'_')) =>
        {
            Some(name.clone())
        }
        _ => None,
    }
}

fn yes_no(node: &Node) -> Option<bool> {
    match node {
        Node::Text(word) if word == "yes" => Some(true),
        Node::Text(word) if word == "no" => Some(false),
        _ => None,
    }
}

fn text(node: &Node) -> Option<String> {
    match node {
        Node::Text(text) => Some(text.clone()),
        _ => None,
    }
}

fn maybe<T>(node: &Node, read: impl Fn(&Node) -> Option<T>) -> Option<Option<T>> {
    match node {
        Node::Null => Some(None),
        _ => read(node).map(Some),
    }
}

fn body_vector(line: &Line) -> Option<(usize, String, BodyVector)> {
    let fields = &line.values;
    let n = usize::try_from(fields[0].count()?).ok()?;
    let name = name_of(&fields[1])?;
    let error = maybe(&fields[4], |node| {
        text(node).filter(|word| BodyError::ALL.iter().any(|error| error.as_str() == word))
    })?;
    let vector = BodyVector {
        n,
        name: name.clone(),
        valid: yes_no(&fields[2])?,
        canon: yes_no(&fields[3])?,
        error,
        body: line.bytes(5)?,
    };
    Some((n, name, vector))
}

fn answer_vector(line: &Line) -> Option<(usize, String, AnswerVector)> {
    let fields = &line.values;
    let n = usize::try_from(fields[0].count()?).ok()?;
    let name = name_of(&fields[1])?;
    let expect = text(&fields[4]).filter(|word| {
        word == "ok"
            || Reason::ALL.iter().any(|reason| reason.as_str() == word)
            || AnswerError::ALL.iter().any(|error| error.as_str() == word)
    })?;
    let vector = AnswerVector {
        n,
        name: name.clone(),
        status: u16::try_from(fields[2].count()?).ok()?,
        body: line.bytes(3)?,
        content: maybe(&fields[5], text)?,
        tokens_in: maybe(&fields[6], Node::count)?,
        tokens_out: maybe(&fields[7], Node::count)?,
        cost: maybe(&fields[8], Node::count)?,
        expect,
    };
    let values = [
        vector.content.is_some(),
        vector.tokens_in.is_some(),
        vector.tokens_out.is_some(),
        vector.cost.is_some(),
    ];
    let set = vector.expect == "ok";
    if values.iter().any(|value| *value != set) {
        return None;
    }
    Some((n, name, vector))
}

fn schema(dir: &Path) -> Result<Node, Refusal> {
    let name = "body.schema.json";
    let lines = text_lines(&dir.join(name), name)?;
    if lines.len() > 1 {
        return Err(Refusal::File(name.to_string(), 2));
    }
    match Node::parse(&lines[0]) {
        Some(node @ Node::Map(_)) if compact(&lines[0]) => Ok(node),
        _ => Err(Refusal::File(name.to_string(), 1)),
    }
}

fn tsv(dir: &Path, name: &str) -> Result<String, Refusal> {
    let lines = text_lines(&dir.join(name), name)?;
    let width = lines[0].split('\t').count();
    for (at, line) in lines.iter().enumerate() {
        let cells: Vec<&str> = line.split('\t').collect();
        if cells.len() != width
            || cells
                .iter()
                .any(|cell| cell.is_empty() || cell.contains('"'))
        {
            return Err(Refusal::File(name.to_string(), at + 1));
        }
    }
    Ok(lines.iter().map(|line| format!("{line}\n")).collect())
}

pub struct Measure {
    pub closed: bool,
    pub names: BTreeSet<String>,
    pub maximum: Option<u64>,
    pub max_length: Option<u64>,
}

pub fn measure(root: &Node) -> Measure {
    let mut measure = Measure {
        closed: true,
        names: BTreeSet::new(),
        maximum: root
            .get("properties")
            .and_then(|node| node.get("max_output_tokens"))
            .and_then(|node| node.get("maximum"))
            .and_then(Node::count),
        max_length: root
            .get("properties")
            .and_then(|node| node.get("messages"))
            .and_then(|node| node.get("items"))
            .and_then(|node| node.get("properties"))
            .and_then(|node| node.get("content"))
            .and_then(|node| node.get("maxLength"))
            .and_then(Node::count),
    };
    walk(root, &mut measure);
    measure
}

const SCHEMA_MAPS: [&str; 5] = [
    "properties",
    "$defs",
    "definitions",
    "patternProperties",
    "dependentSchemas",
];
const DATA: [&str; 5] = ["enum", "const", "default", "examples", "required"];

fn walk(node: &Node, measure: &mut Measure) {
    match node {
        Node::List(items) => items.iter().for_each(|item| walk(item, measure)),
        Node::Map(entries) => {
            let object = match node.get("type") {
                Some(Node::Text(word)) => word == "object",
                Some(Node::List(words)) => words.contains(&Node::Text("object".to_string())),
                _ => false,
            } || node.get("properties").is_some();
            if object && node.get("additionalProperties") != Some(&Node::Bool(false)) {
                measure.closed = false;
            }
            for (key, value) in entries {
                if DATA.contains(&key.as_str()) {
                    continue;
                }
                match value {
                    Node::Map(members) if SCHEMA_MAPS.contains(&key.as_str()) => {
                        for (name, schema) in members {
                            if key == "properties" {
                                measure.names.insert(name.clone());
                            }
                            walk(schema, measure);
                        }
                    }
                    _ => walk(value, measure),
                }
            }
        }
        _ => {}
    }
}
