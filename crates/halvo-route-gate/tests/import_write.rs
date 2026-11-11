use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use halvo_route_gate::import::{command, write_files, WriteFault};

fn scratch() -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = Path::new(env!("CARGO_TARGET_TMPDIR"));
    loop {
        let dir = base.join(format!(
            "route-gate-import-write-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        if std::fs::create_dir(&dir).is_ok() {
            return dir;
        }
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("the directory")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .into_string()
                .expect("utf-8")
        })
        .collect();
    names.sort();
    names
}

fn text(dir: &Path, name: &str) -> String {
    dir.join(name).to_str().expect("utf-8 path").to_owned()
}

struct Broken;

impl Write for Broken {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "reader ended"))
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(io::ErrorKind::BrokenPipe, "reader ended"))
    }
}

fn inputs(dir: &Path) -> Vec<String> {
    std::fs::write(
        dir.join("driver.md"),
        "```\nattempt model=m_a machine=mac task=bill seed=1 kind=compile i=1 pass=yes steps=3 wall_s=10 tokens_in=5 tokens_out=6\nverdict driver=none path=B writer=none\n```\n",
    )
    .expect("driver");
    std::fs::write(
        dir.join("model-cells.md"),
        "```\nbound model=m_a machine=mac bound_bytes=100 fits=yes\n```\n",
    )
    .expect("cells");
    let out = dir.join("out");
    std::fs::create_dir(&out).expect("out");
    [
        "--driver",
        &text(dir, "driver.md"),
        "--cells",
        &text(dir, "model-cells.md"),
        "--date",
        "s1=2026-11-10",
        "--date",
        "s2=2026-11-10",
        "--out",
        &text(dir, "out"),
    ]
    .map(str::to_owned)
    .to_vec()
}

#[test]
fn a_failed_write_of_the_line_removes_the_four_files_and_exits_1() {
    let dir = scratch();
    let args = inputs(&dir);
    let mut stderr = Vec::new();
    let code = command(&args, &mut Broken, &mut stderr);
    assert_eq!(code, 1);
    assert_eq!(stderr, b"");
    assert_eq!(names(&dir.join("out")), Vec::<String>::new());
}

#[test]
fn the_command_writes_its_line_to_the_writer_it_is_given() {
    let dir = scratch();
    let args = inputs(&dir);
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(command(&args, &mut stdout, &mut stderr), 0);
    assert_eq!(stdout, b"import rows=1 models=1 sources=2\n");
    assert_eq!(stderr, b"");
    assert_eq!(
        names(&dir.join("out")),
        ["freeze.txt", "models.tsv", "recorded.tsv", "sources.tsv"]
    );
}

#[test]
fn a_refusal_goes_to_the_error_writer_alone() {
    let dir = scratch();
    let mut args = inputs(&dir);
    args[9] = text(&dir, "out-2");
    std::fs::create_dir(dir.join("out-2")).expect("out-2");
    assert_eq!(command(&args, &mut Vec::new(), &mut Vec::new()), 0);
    args[5] = "s1=2026-11-31".to_owned();
    args[9] = text(&dir, "out");
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    assert_eq!(command(&args, &mut stdout, &mut stderr), 2);
    assert_eq!(stdout, b"");
    let line = String::from_utf8(stderr).expect("utf-8");
    assert!(line.starts_with("REFUSED usage: "), "{line}");
    assert!(line.ends_with('\n') && line.matches('\n').count() == 1);
}

#[test]
fn a_file_that_appears_during_the_writes_removes_the_files_of_this_call_and_keeps_it() {
    let dir = scratch();
    let out = text(&dir, "");
    let files = [
        ("recorded.tsv", "r\n".to_owned()),
        ("models.tsv", "m\n".to_owned()),
        ("sources.tsv", "s\n".to_owned()),
        ("freeze.txt", "f\n".to_owned()),
    ];
    let other = dir.join("sources.tsv");
    let mut seen = Vec::new();
    let made = write_files(&out, &files, &mut |name: &str| {
        seen.push(name.to_owned());
        if name == "sources.tsv" {
            std::fs::write(&other, "another call\n").expect("another call writes");
        }
    });
    assert_eq!(made, Err(WriteFault::Exists("sources.tsv".to_owned())));
    assert_eq!(seen, ["recorded.tsv", "models.tsv", "sources.tsv"]);
    assert_eq!(names(&dir), ["sources.tsv"]);
    assert_eq!(
        std::fs::read_to_string(&other).expect("kept"),
        "another call\n"
    );
}

#[test]
fn a_directory_that_cannot_be_written_leaves_no_file() {
    let dir = scratch();
    let files = [
        ("recorded.tsv", "r\n".to_owned()),
        ("models.tsv", "m\n".to_owned()),
    ];
    let missing = text(&dir, "missing");
    let made = write_files(&missing, &files, &mut |_: &str| {});
    assert_eq!(made, Err(WriteFault::Unwritable));
    assert_eq!(names(&dir), Vec::<String>::new());
    let made = write_files(&text(&dir, ""), &files, &mut |_: &str| {});
    assert_eq!(made, Ok(()));
    assert_eq!(names(&dir), ["models.tsv", "recorded.tsv"]);
}
