use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::record::{names, CaptureRecord, Payload, Pointer, PointerDetail, Title};

names! {
    /// A layer whose records `input-copy` copies into an input copy (C6.2). The fixture transport does not
    /// use it.
    Layer {
        AccessibilityText = "accessibility_text",
        ExtractedText = "extracted_text",
    }
}

/// The payload keys of a title record that a set lists beside `text`, which every set lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TitleKeys {
    pub cut: bool,
    pub why: bool,
    pub site: bool,
}

/// The payload keys of a keystroke record that a set lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeystrokeKeys {
    pub class: bool,
}

/// The payload keys of a pointer record that a set lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PointerKeys {
    pub class: bool,
    pub button: bool,
    pub count: bool,
}

/// An input set (C6): the layers that `input-copy` copies, and for each record kind of the set the payload
/// keys that a server serves. Every set holds the app and the title; `keystroke` and `pointer` are in the set
/// only where its file has their line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputSet {
    layers: Vec<Layer>,
    title: TitleKeys,
    keystroke: Option<KeystrokeKeys>,
    pointer: Option<PointerKeys>,
}

/// Why the set reader refused a file (C6.4): the file is missing or unreadable, or its bytes are not a file of
/// C6.2, with the number of the first line that does not fit, counted from 1.
#[derive(Debug)]
pub enum SetError {
    Unreadable { path: PathBuf, kind: io::ErrorKind },
    Form { path: PathBuf, line: usize },
}

/// The most bytes of a set file (C6.4). A file of C6.2 has at most 7 lines and about 200 bytes.
pub const SET_MAX: usize = 1_024;

const LAYERS: [&str; 2] = ["accessibility_text", "extracted_text"];
const TITLE: [&str; 4] = ["text", "cut", "why", "site"];
const KEYSTROKE: [&str; 1] = ["class"];
const POINTER: [&str; 3] = ["class", "button", "count"];

fn list(value: &[u8], legal: &[&str]) -> Option<Vec<bool>> {
    let value = std::str::from_utf8(value).ok()?;
    let inner = value.strip_prefix('[')?.strip_suffix(']')?;
    let mut flags = vec![false; legal.len()];
    if inner.is_empty() {
        return Some(flags);
    }
    let mut last: Option<usize> = None;
    for item in inner.split(", ") {
        let name = item.strip_prefix('"')?.strip_suffix('"')?;
        let i = legal.iter().position(|l| *l == name)?;
        if last.is_some_and(|l| i <= l) {
            return None;
        }
        last = Some(i);
        flags[i] = true;
    }
    Some(flags)
}

fn value<'a>(line: &'a [u8], key: &str) -> Option<&'a [u8]> {
    line.strip_prefix(key.as_bytes())?.strip_prefix(b" = ")
}

struct SetLines<'a> {
    rest: &'a [u8],
    n: usize,
}

impl<'a> SetLines<'a> {
    fn next(&mut self) -> Result<Option<&'a [u8]>, usize> {
        if self.rest.is_empty() {
            return Ok(None);
        }
        self.n += 1;
        let Some(i) = self.rest.iter().position(|b| *b == b'\n') else {
            return Err(self.n);
        };
        let line = &self.rest[..i];
        self.rest = &self.rest[i + 1..];
        Ok(Some(line))
    }

    fn required(&mut self) -> Result<&'a [u8], usize> {
        self.next()?.ok_or(self.n + 1)
    }
}

impl InputSet {
    /// Reads a set file by `parse`, reporting the path with the defect (C6.4). A path that is no regular
    /// file is unreadable, and it is not opened when the system says so before the open. The reader takes at
    /// most `SET_MAX` bytes and one more, so that a longer file is refused, as no file of C6.2, with the
    /// number of the line that holds its byte `SET_MAX + 1`, and is not read further.
    pub fn read(path: &Path) -> Result<InputSet, SetError> {
        let unreadable = |kind: io::ErrorKind| SetError::Unreadable {
            path: path.to_owned(),
            kind,
        };
        let regular = |meta: io::Result<fs::Metadata>| match meta {
            Ok(m) if m.is_file() => Ok(()),
            Ok(_) => Err(unreadable(io::ErrorKind::InvalidInput)),
            Err(e) => Err(unreadable(e.kind())),
        };
        regular(fs::metadata(path))?;
        let file = File::open(path).map_err(|e| unreadable(e.kind()))?;
        regular(file.metadata())?;
        let mut bytes = Vec::with_capacity(SET_MAX + 1);
        file.take(SET_MAX as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| unreadable(e.kind()))?;
        if bytes.len() > SET_MAX {
            let line = bytes[..SET_MAX].iter().filter(|b| **b == b'\n').count() + 1;
            return Err(SetError::Form {
                path: path.to_owned(),
                line,
            });
        }
        InputSet::parse(&bytes).map_err(|line| SetError::Form {
            path: path.to_owned(),
            line,
        })
    }

    /// Compares the bytes with the form of C6.2 and returns the set, or the number of the first line that does
    /// not fit: the number of the last line plus 1 where the bytes end before a required line, so 1 for no
    /// bytes, and its own number for a last line with no LF.
    pub fn parse(bytes: &[u8]) -> Result<InputSet, usize> {
        let mut lines = SetLines { rest: bytes, n: 0 };
        let fits = |ok: bool, lines: &SetLines| if ok { Ok(()) } else { Err(lines.n) };
        let line = lines.required()?;
        fits(line == b"envelope = [\"app\"]", &lines)?;
        let line = lines.required()?;
        let layer_flags = value(line, "layers").and_then(|v| list(v, &LAYERS));
        let Some(layer_flags) = layer_flags else {
            return Err(lines.n);
        };
        let line = lines.required()?;
        fits(line == b"[kinds]", &lines)?;
        let line = lines.required()?;
        fits(line == b"foreground_app = []", &lines)?;
        let line = lines.required()?;
        let title = value(line, "window_title").and_then(|v| list(v, &TITLE));
        let Some(title) = title.filter(|t| t[0]) else {
            return Err(lines.n);
        };
        let mut keystroke = None;
        let mut pointer = None;
        let mut line = lines.next()?;
        if let Some(l) = line {
            if let Some(v) = value(l, "keystroke") {
                let Some(flags) = list(v, &KEYSTROKE) else {
                    return Err(lines.n);
                };
                keystroke = Some(KeystrokeKeys { class: flags[0] });
                line = lines.next()?;
            }
        }
        if let Some(l) = line {
            let Some(flags) = value(l, "pointer").and_then(|v| list(v, &POINTER)) else {
                return Err(lines.n);
            };
            pointer = Some(PointerKeys {
                class: flags[0],
                button: flags[1],
                count: flags[2],
            });
            if lines.next()?.is_some() {
                return Err(lines.n);
            }
        }
        let layers = Layer::ALL
            .iter()
            .zip(layer_flags)
            .filter_map(|(layer, on)| on.then_some(*layer))
            .collect();
        Ok(InputSet {
            layers,
            title: TitleKeys {
                cut: title[1],
                why: title[2],
                site: title[3],
            },
            keystroke,
            pointer,
        })
    }

    /// The `layers` list, which `input-copy` copies by.
    pub fn layers(&self) -> &[Layer] {
        &self.layers
    }

    /// The keys of a title record beside `text`.
    pub fn title(&self) -> TitleKeys {
        self.title
    }

    /// The keys of a keystroke record, or none when the kind is outside the set.
    pub fn keystroke(&self) -> Option<KeystrokeKeys> {
        self.keystroke
    }

    /// The keys of a pointer record, or none when the kind is outside the set.
    pub fn pointer(&self) -> Option<PointerKeys> {
        self.pointer
    }

    /// The reduction of C6.5: a run mark whole; none for a record of a kind outside the set, `screen_frame`
    /// among them; else the whole envelope with the payload keys that the set lists, a listed key that the
    /// record does not hold staying absent and `site` served or taken out whole. A second reduction by the
    /// same set changes nothing.
    pub fn reduce(&self, record: &CaptureRecord) -> Option<CaptureRecord> {
        let payload = match record.payload() {
            Payload::RunMark(_) => return Some(record.clone()),
            Payload::ScreenFrame => return None,
            Payload::ForegroundApp => Payload::ForegroundApp,
            Payload::WindowTitle(t) => Payload::WindowTitle(Title {
                text: t.text.clone(),
                cut: t.cut.filter(|_| self.title.cut),
                why: t.why.filter(|_| self.title.why),
                site: t.site.clone().filter(|_| self.title.site),
            }),
            Payload::Keystroke(class) => {
                let keys = self.keystroke?;
                Payload::Keystroke(class.filter(|_| keys.class))
            }
            Payload::Pointer(p) => Payload::Pointer(reduce_pointer(*p, self.pointer?)),
        };
        Some(record.with_payload(payload))
    }
}

fn reduce_pointer(p: Pointer, keys: PointerKeys) -> Pointer {
    let (click, motion, button, count) = match p {
        Pointer::Click(b) => (true, None, b, None),
        Pointer::Motion(m, c) => (false, Some(m), None, c),
        Pointer::Unclassed(Some(PointerDetail::Button(b))) => (false, None, Some(b), None),
        Pointer::Unclassed(Some(PointerDetail::Count(c))) => (false, None, None, Some(c)),
        Pointer::Unclassed(None) => (false, None, None, None),
    };
    let button = button.filter(|_| keys.button);
    let count = count.filter(|_| keys.count);
    match (keys.class, click, motion) {
        (true, true, _) => Pointer::Click(button),
        (true, false, Some(m)) => Pointer::Motion(m, count),
        _ => Pointer::Unclassed(
            button
                .map(PointerDetail::Button)
                .or(count.map(PointerDetail::Count)),
        ),
    }
}
