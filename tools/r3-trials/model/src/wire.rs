/// The grammar of a request (contracts.md A12.2, A12.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grammar {
    Action,
    Text,
}

/// The level of a request, which picks the token limits of the settings (A12.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Low,
    Medium,
    High,
}

/// One request line of A12.2:
/// `{"id":<count>,"system":"<text>","prompt":"<text>","snapshot":"<text>"|null,"grammar":"action"|"text","level":"low"|"medium"|"high"}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub id: u64,
    pub system: String,
    pub prompt: String,
    pub snapshot: Option<String>,
    pub grammar: Grammar,
    pub level: Level,
}

/// One reply line of A12.2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub id: u64,
    pub text: String,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub first_token_ms: u64,
    pub ms: u64,
}

struct Cursor<'a> {
    rest: &'a str,
}

impl<'a> Cursor<'a> {
    fn literal(&mut self, text: &str) -> Result<(), String> {
        self.rest = self
            .rest
            .strip_prefix(text)
            .ok_or_else(|| format!("expected {text}"))?;
        Ok(())
    }

    fn count(&mut self) -> Result<u64, String> {
        let end = self
            .rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(self.rest.len());
        let digits = &self.rest[..end];
        if digits.is_empty() || digits.starts_with('0') {
            return Err("the id is no count".to_string());
        }
        let n = digits
            .parse()
            .map_err(|_| "the id is above 2^64 - 1".to_string())?;
        self.rest = &self.rest[end..];
        Ok(n)
    }

    fn hex4(&mut self) -> Result<u32, String> {
        let digits = self
            .rest
            .get(..4)
            .filter(|d| d.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or_else(|| "a \\u escape needs four hex digits".to_string())?;
        self.rest = &self.rest[4..];
        Ok(u32::from_str_radix(digits, 16).unwrap_or(0))
    }

    fn string(&mut self) -> Result<String, String> {
        self.literal("\"")?;
        let mut out = String::new();
        loop {
            let c = self
                .rest
                .chars()
                .next()
                .ok_or_else(|| "a string has no end".to_string())?;
            self.rest = &self.rest[c.len_utf8()..];
            match c {
                '"' => return Ok(out),
                '\\' => {
                    let e = self
                        .rest
                        .chars()
                        .next()
                        .ok_or_else(|| "a string has no end".to_string())?;
                    self.rest = &self.rest[e.len_utf8()..];
                    match e {
                        '"' => out.push('"'),
                        '\\' => out.push('\\'),
                        '/' => out.push('/'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'n' => out.push('\n'),
                        'r' => out.push('\r'),
                        't' => out.push('\t'),
                        'u' => out.push(self.unicode()?),
                        _ => return Err(format!("\\{e} is no escape of JSON")),
                    }
                }
                c if (c as u32) < 0x20 => {
                    return Err("a string holds a control character".to_string())
                }
                c => out.push(c),
            }
        }
    }

    fn unicode(&mut self) -> Result<char, String> {
        let high = self.hex4()?;
        let code = match high {
            0xD800..=0xDBFF => {
                self.literal("\\u")
                    .map_err(|_| "a high surrogate has no low one".to_string())?;
                let low = self.hex4()?;
                if !(0xDC00..=0xDFFF).contains(&low) {
                    return Err("a high surrogate has no low one".to_string());
                }
                0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00)
            }
            0xDC00..=0xDFFF => return Err("a low surrogate stands alone".to_string()),
            code => code,
        };
        char::from_u32(code).ok_or_else(|| "a \\u escape names no character".to_string())
    }

    fn word<T: Copy>(&mut self, words: &[(&str, T)]) -> Result<T, String> {
        for (text, value) in words {
            if let Some(rest) = self.rest.strip_prefix(text) {
                self.rest = rest;
                return Ok(*value);
            }
        }
        Err(format!(
            "expected one of {}",
            words.iter().map(|w| w.0).collect::<Vec<_>>().join(" ")
        ))
    }
}

pub fn parse_request(line: &str) -> Result<Request, String> {
    let mut c = Cursor { rest: line };
    c.literal("{\"id\":")?;
    let id = c.count()?;
    c.literal(",\"system\":")?;
    let system = c.string()?;
    c.literal(",\"prompt\":")?;
    let prompt = c.string()?;
    c.literal(",\"snapshot\":")?;
    let snapshot = if c.rest.starts_with("null") {
        c.literal("null")?;
        None
    } else {
        Some(c.string()?)
    };
    c.literal(",\"grammar\":")?;
    let grammar = c.word(&[("\"action\"", Grammar::Action), ("\"text\"", Grammar::Text)])?;
    c.literal(",\"level\":")?;
    let level = c.word(&[
        ("\"low\"", Level::Low),
        ("\"medium\"", Level::Medium),
        ("\"high\"", Level::High),
    ])?;
    c.literal("}")?;
    if !c.rest.is_empty() {
        return Err("the line goes on after its object".to_string());
    }
    Ok(Request {
        id,
        system,
        prompt,
        snapshot,
        grammar,
        level,
    })
}

fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn reply_line(reply: &Reply) -> String {
    format!(
        "{{\"id\":{},\"text\":{},\"tokens_in\":{},\"tokens_out\":{},\"first_token_ms\":{},\"ms\":{}}}",
        reply.id,
        json_string(&reply.text),
        reply.tokens_in,
        reply.tokens_out,
        reply.first_token_ms,
        reply.ms
    )
}

pub fn ready_line(model: &str, ctx: u32, threads: u32, batch: u32, load_ms: u64) -> String {
    format!("ready model={model} ctx={ctx} threads={threads} batch={batch} load_ms={load_ms}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"{"id":1,"system":"Be brief.","prompt":"Pay the bill.","snapshot":"e1 heading \"Pay\"\ne2 button \"Pay now\"","grammar":"action","level":"medium"}"#;

    #[test]
    fn a_request_of_the_contract_parses() {
        assert_eq!(
            parse_request(LINE),
            Ok(Request {
                id: 1,
                system: "Be brief.".to_string(),
                prompt: "Pay the bill.".to_string(),
                snapshot: Some("e1 heading \"Pay\"\ne2 button \"Pay now\"".to_string()),
                grammar: Grammar::Action,
                level: Level::Medium,
            })
        );
        let null =
            r#"{"id":12,"system":"","prompt":"x","snapshot":null,"grammar":"text","level":"high"}"#;
        let request = parse_request(null).unwrap();
        assert_eq!(request.id, 12);
        assert_eq!(request.snapshot, None);
        assert_eq!(request.grammar, Grammar::Text);
        assert_eq!(request.level, Level::High);
        assert_eq!(request.system, "");
    }

    #[test]
    fn the_escapes_of_json_are_read() {
        let line = r#"{"id":2,"system":"a\\b\/c\td\re\bf\fg","prompt":"é€😀","snapshot":null,"grammar":"text","level":"low"}"#;
        let request = parse_request(line).unwrap();
        assert_eq!(request.system, "a\\b/c\td\re\u{8}f\u{c}g");
        assert_eq!(request.prompt, "\u{e9}\u{20ac}\u{1f600}");
        let raw = "{\"id\":3,\"system\":\"\u{e9}\",\"prompt\":\"x\",\"snapshot\":null,\"grammar\":\"text\",\"level\":\"low\"}";
        assert_eq!(parse_request(raw).unwrap().system, "\u{e9}");
    }

    #[test]
    fn a_line_of_another_form_is_refused() {
        let swap = |from: &str, to: &str| LINE.replacen(from, to, 1);
        let cases = [
            swap("\"id\":1", "\"id\":0"),
            swap("\"id\":1", "\"id\":01"),
            swap("\"id\":1", "\"id\":-1"),
            swap("\"id\":1", "\"id\":\"1\""),
            swap("\"id\":1,", "\"id\":1 ,"),
            swap("{\"id\":1,", "{\"id\":1,\"extra\":1,"),
            swap("\"level\":\"medium\"", "\"level\":\"max\""),
            swap("\"grammar\":\"action\"", "\"grammar\":\"json\""),
            swap(",\"level\":\"medium\"", ""),
            swap(
                "\"system\":\"Be brief.\",\"prompt\":\"Pay the bill.\"",
                "\"prompt\":\"Pay the bill.\",\"system\":\"Be brief.\"",
            ),
            swap("\"snapshot\":\"", "\"snapshot\":nul\""),
            swap("Be brief.", "Be\nbrief."),
            swap("Be brief.", "Be \\x brief."),
            swap("Be brief.", "\\ud83d"),
            swap("Be brief.", "\\ude00"),
            swap("Be brief.", "\\u12g4"),
            format!("{LINE} "),
            format!("{LINE}x"),
            LINE[..LINE.len() - 1].to_string(),
            String::new(),
        ];
        for case in cases {
            assert!(parse_request(&case).is_err(), "{case}");
        }
        let id = format!("{{\"id\":{},", u64::MAX as u128 + 1);
        assert!(parse_request(&LINE.replacen("{\"id\":1,", &id, 1)).is_err());
    }

    #[test]
    fn a_reply_is_one_compact_json_line() {
        let reply = Reply {
            id: 7,
            text: "{\"action\":\"done\"}\n\u{1}\u{e9}\\".to_string(),
            tokens_in: 120,
            tokens_out: 9,
            first_token_ms: 40,
            ms: 95,
        };
        assert_eq!(
            reply_line(&reply),
            r#"{"id":7,"text":"{\"action\":\"done\"}\n\u0001é\\","tokens_in":120,"tokens_out":9,"first_token_ms":40,"ms":95}"#
        );
    }

    #[test]
    fn the_ready_line_has_the_form_of_the_contract() {
        assert_eq!(
            ready_line("m-Q4_K_M.gguf", 8192, 4, 2048, 731),
            "ready model=m-Q4_K_M.gguf ctx=8192 threads=4 batch=2048 load_ms=731"
        );
    }
}
