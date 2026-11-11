use std::fmt;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

/// The longest control line, in bytes, before its line feed.
pub const MAX_LINE_BYTES: usize = 4096;

/// A control line of I11.3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    Show,
    Hide,
    Destroy,
    Load(String),
    Send { count: u64, per_second: u64 },
    Sample,
    Quit,
}

/// The cause of an `err` answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrCause {
    UnknownLine,
    NoWindow,
    Busy,
}

/// The one answer line to a control line: `ok`, with pairs for `show` and `sample` alone, or
/// `err <cause>`. `ticks` and `frames` are `None` where no page exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    Ok,
    Shown {
        show_ms: u64,
    },
    Sampled {
        pid: u32,
        ticks: Option<u64>,
        frames: Option<u64>,
    },
    Err(ErrCause),
}

/// What answers the control lines. `quit` is answered `ok` by the server, which then calls
/// `quit`.
pub trait Core: Send + Sync + 'static {
    fn answer(&self, line: Line) -> Answer;
    fn quit(&self);
}

impl ErrCause {
    pub fn name(self) -> &'static str {
        match self {
            ErrCause::UnknownLine => "unknown_line",
            ErrCause::NoWindow => "no_window",
            ErrCause::Busy => "busy",
        }
    }
}

impl fmt::Display for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let count = |n: Option<u64>| n.map_or_else(|| "-".to_string(), |n| n.to_string());
        match self {
            Answer::Ok => write!(f, "ok"),
            Answer::Shown { show_ms } => write!(f, "ok show_ms={show_ms}"),
            Answer::Sampled { pid, ticks, frames } => write!(
                f,
                "ok pid={pid} ticks={} frames={}",
                count(*ticks),
                count(*frames)
            ),
            Answer::Err(cause) => write!(f, "err {}", cause.name()),
        }
    }
}

fn page_path(path: &str) -> bool {
    path.split('/').all(|part| {
        !part.is_empty()
            && part != "."
            && part != ".."
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
    })
}

fn positive(word: &str) -> Option<u64> {
    if !word.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    word.parse::<u64>().ok().filter(|n| *n > 0)
}

/// Parses one line without its line feed; a carriage return at its end is taken off. `load`
/// takes a relative path of one or more parts separated by `/`, each of ASCII letters, digits,
/// `.`, `-` and `_`, and none empty, `.` or `..`.
pub fn parse_line(text: &str) -> Result<Line, ErrCause> {
    let text = text.strip_suffix('\r').unwrap_or(text);
    let words: Vec<&str> = text.split(' ').collect();
    match words.as_slice() {
        ["show"] => Ok(Line::Show),
        ["hide"] => Ok(Line::Hide),
        ["destroy"] => Ok(Line::Destroy),
        ["sample"] => Ok(Line::Sample),
        ["quit"] => Ok(Line::Quit),
        ["load", path] if page_path(path) => Ok(Line::Load((*path).to_string())),
        ["send", count, per_second] => match (positive(count), positive(per_second)) {
            (Some(count), Some(per_second)) => Ok(Line::Send { count, per_second }),
            _ => Err(ErrCause::UnknownLine),
        },
        _ => Err(ErrCause::UnknownLine),
    }
}

/// Binds 127.0.0.1 on `port`, or on a free port for `None`.
pub fn listen(port: Option<u16>) -> io::Result<TcpListener> {
    TcpListener::bind((Ipv4Addr::LOCALHOST, port.unwrap_or(0)))
}

/// Accepts connections on `listener` on a thread of its own and answers each line of each
/// connection with one line, until the connection closes or a line is longer than
/// `MAX_LINE_BYTES`.
pub fn serve(listener: TcpListener, core: Arc<dyn Core>) -> io::Result<()> {
    thread::Builder::new()
        .name("r4-control".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let core = core.clone();
                let _ = thread::Builder::new()
                    .name("r4-control-conn".into())
                    .spawn(move || {
                        let _ = connection(stream, core.as_ref());
                    });
            }
        })?;
    Ok(())
}

fn connection(stream: TcpStream, core: &dyn Core) -> io::Result<()> {
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    loop {
        let mut bytes = Vec::new();
        let read = (&mut reader)
            .take(MAX_LINE_BYTES as u64 + 1)
            .read_until(b'\n', &mut bytes)?;
        if read == 0 {
            return Ok(());
        }
        let ended = bytes.last() == Some(&b'\n');
        if ended {
            bytes.pop();
        }
        if bytes.len() > MAX_LINE_BYTES || (!ended && read > MAX_LINE_BYTES) {
            writeln!(writer, "{}", Answer::Err(ErrCause::UnknownLine))?;
            return Ok(());
        }
        let line = std::str::from_utf8(&bytes)
            .map_err(|_| ErrCause::UnknownLine)
            .and_then(parse_line);
        match line {
            Ok(Line::Quit) => {
                writeln!(writer, "{}", Answer::Ok)?;
                writer.flush()?;
                core.quit();
            }
            Ok(line) => writeln!(writer, "{}", core.answer(line))?,
            Err(cause) => writeln!(writer, "{}", Answer::Err(cause))?,
        }
        writer.flush()?;
        if !ended {
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::BOUND;
    use std::sync::Mutex;

    #[test]
    fn every_line_parses() {
        assert_eq!(parse_line("show"), Ok(Line::Show));
        assert_eq!(parse_line("hide"), Ok(Line::Hide));
        assert_eq!(parse_line("destroy"), Ok(Line::Destroy));
        assert_eq!(parse_line("sample"), Ok(Line::Sample));
        assert_eq!(parse_line("quit"), Ok(Line::Quit));
        assert_eq!(parse_line("quit\r"), Ok(Line::Quit));
        assert_eq!(
            parse_line("load probe.html"),
            Ok(Line::Load("probe.html".into()))
        );
        assert_eq!(
            parse_line("load sub/dir/page.html"),
            Ok(Line::Load("sub/dir/page.html".into()))
        );
        assert_eq!(
            parse_line("send 200000 1000"),
            Ok(Line::Send {
                count: 200_000,
                per_second: 1000
            })
        );
    }

    #[test]
    fn other_lines_are_unknown() {
        for text in [
            "",
            "Show",
            " show",
            "show ",
            "show now",
            "load",
            "load ",
            "load a b",
            "load /etc/hosts",
            "load ../x.html",
            "load a/../b.html",
            "load ./a.html",
            "load a//b.html",
            "load a\\b.html",
            "load C:x.html",
            "send",
            "send 10",
            "send 10 x",
            "send 0 1000",
            "send 10 0",
            "send -1 10",
            "send 10 10 10",
            "quit\r\r",
            "sample\0",
        ] {
            assert_eq!(parse_line(text), Err(ErrCause::UnknownLine), "{text:?}");
        }
    }

    #[test]
    fn answers_print_as_the_contract_says() {
        assert_eq!(Answer::Ok.to_string(), "ok");
        assert_eq!(Answer::Shown { show_ms: 412 }.to_string(), "ok show_ms=412");
        assert_eq!(
            Answer::Sampled {
                pid: 77,
                ticks: Some(3),
                frames: Some(9)
            }
            .to_string(),
            "ok pid=77 ticks=3 frames=9"
        );
        assert_eq!(
            Answer::Sampled {
                pid: 77,
                ticks: None,
                frames: None
            }
            .to_string(),
            "ok pid=77 ticks=- frames=-"
        );
        assert_eq!(
            Answer::Err(ErrCause::UnknownLine).to_string(),
            "err unknown_line"
        );
        assert_eq!(Answer::Err(ErrCause::NoWindow).to_string(), "err no_window");
        assert_eq!(Answer::Err(ErrCause::Busy).to_string(), "err busy");
    }

    #[derive(Default)]
    struct Fake {
        lines: Mutex<Vec<Line>>,
        quits: Mutex<u32>,
    }

    impl Core for Fake {
        fn answer(&self, line: Line) -> Answer {
            let answer = match &line {
                Line::Show => Answer::Shown { show_ms: 5 },
                Line::Hide => Answer::Err(ErrCause::NoWindow),
                Line::Sample => Answer::Sampled {
                    pid: 1,
                    ticks: None,
                    frames: Some(2),
                },
                _ => Answer::Ok,
            };
            self.lines.lock().unwrap().push(line);
            answer
        }

        fn quit(&self) {
            *self.quits.lock().unwrap() += 1;
        }
    }

    fn connect(addr: std::net::SocketAddr) -> TcpStream {
        let stream = TcpStream::connect(addr).unwrap();
        stream.set_read_timeout(Some(BOUND)).unwrap();
        stream
    }

    fn exchange(stream: &mut TcpStream, reader: &mut BufReader<TcpStream>, line: &str) -> String {
        stream.write_all(line.as_bytes()).unwrap();
        let mut answer = String::new();
        reader.read_line(&mut answer).unwrap();
        answer
    }

    #[test]
    fn the_server_answers_each_line_with_one_line_and_quits_after_ok() {
        let listener = listen(None).unwrap();
        let addr = listener.local_addr().unwrap();
        assert!(addr.ip().is_loopback());
        assert_ne!(addr.port(), 0);
        let fake = Arc::new(Fake::default());
        serve(listener, fake.clone()).unwrap();

        let mut stream = connect(addr);
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        assert_eq!(
            exchange(&mut stream, &mut reader, "show\n"),
            "ok show_ms=5\n"
        );
        assert_eq!(
            exchange(&mut stream, &mut reader, "hide\r\n"),
            "err no_window\n"
        );
        assert_eq!(
            exchange(&mut stream, &mut reader, "sample\n"),
            "ok pid=1 ticks=- frames=2\n"
        );
        assert_eq!(
            exchange(&mut stream, &mut reader, "dance\n"),
            "err unknown_line\n"
        );
        assert_eq!(*fake.quits.lock().unwrap(), 0);
        assert_eq!(exchange(&mut stream, &mut reader, "quit\n"), "ok\n");
        for _ in 0..200 {
            if *fake.quits.lock().unwrap() == 1 {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(*fake.quits.lock().unwrap(), 1);
        assert_eq!(
            *fake.lines.lock().unwrap(),
            vec![Line::Show, Line::Hide, Line::Sample]
        );
    }

    #[test]
    fn a_second_connection_is_served_and_a_long_line_closes_its_own() {
        let listener = listen(None).unwrap();
        let addr = listener.local_addr().unwrap();
        serve(listener, Arc::new(Fake::default())).unwrap();

        let mut long = connect(addr);
        let mut text = "x".repeat(MAX_LINE_BYTES + 1);
        text.push('\n');
        long.write_all(text.as_bytes()).unwrap();
        let mut reader = BufReader::new(long.try_clone().unwrap());
        let mut answer = String::new();
        reader.read_line(&mut answer).unwrap();
        assert_eq!(answer, "err unknown_line\n");
        answer.clear();
        assert_eq!(reader.read_line(&mut answer).unwrap(), 0);

        let mut stream = connect(addr);
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        assert_eq!(exchange(&mut stream, &mut reader, "load a.html\n"), "ok\n");
    }

    #[test]
    fn a_line_that_is_not_utf8_is_unknown() {
        let listener = listen(None).unwrap();
        let addr = listener.local_addr().unwrap();
        serve(listener, Arc::new(Fake::default())).unwrap();
        let mut stream = connect(addr);
        stream.write_all(b"sh\xffow\n").unwrap();
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut answer = String::new();
        reader.read_line(&mut answer).unwrap();
        assert_eq!(answer, "err unknown_line\n");
    }

    #[test]
    fn a_given_port_is_bound_on_loopback() {
        let free = listen(None).unwrap().local_addr().unwrap().port();
        let listener = listen(Some(free)).unwrap();
        assert_eq!(
            listener.local_addr().unwrap(),
            (Ipv4Addr::LOCALHOST, free).into()
        );
    }
}
