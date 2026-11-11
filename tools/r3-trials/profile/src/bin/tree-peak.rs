use std::io::Write;
use std::process::{Command, ExitCode};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use r3_profile::footprint::{measure, Stopper};

struct Flag(Arc<AtomicBool>);

impl Stopper for Flag {
    fn stopped(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

fn out(line: &str) {
    let mut s = std::io::stdout().lock();
    let _ = writeln!(s, "{line}");
    let _ = s.flush();
}

fn refuse(detail: &str) -> ExitCode {
    eprintln!("REFUSED usage: {detail}");
    ExitCode::from(2)
}

fn watch(flag: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return;
        };
        rt.block_on(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let (Ok(mut term), Ok(mut int)) = (
                    signal(SignalKind::terminate()),
                    signal(SignalKind::interrupt()),
                ) else {
                    return;
                };
                tokio::select! {
                    _ = term.recv() => {}
                    _ = int.recv() => {}
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
            flag.store(true, Ordering::SeqCst);
        });
    });
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("--help") {
        out("Usage: tree-peak --runs N -- COMMAND [ARG...]");
        out("Starts COMMAND N times, one after the other. Every 250 ms it reads the memory of the");
        out("command and of every descendant: on macOS the sum of phys_footprint, where a process");
        out("that the reparenting to launchd took out of the tree counts when it started during the");
        out("run and its executable lies in an .app bundle of the tree or in the directory of the");
        out("command; on Windows the peak of a Job object that holds the command from its first");
        out("instruction. Prints tree run=<i> exit=<status> peak_bytes=<n> per run, treeinfo and");
        out(
            "treepart lines, and tree median_bytes=<n>. It sets no cap. Exit 0 when every run ran,",
        );
        out("1 when a run could not start or a signal stopped it, 2 for a refusal.");
        return ExitCode::SUCCESS;
    }
    let (runs, command) = match args.as_slice() {
        [flag, n, sep, command @ ..] if flag == "--runs" && sep == "--" && !command.is_empty() => {
            match n.parse::<u32>() {
                Ok(n) if n > 0 => (n, command.to_vec()),
                _ => return refuse("--runs takes a count from 1"),
            }
        }
        _ => return refuse("tree-peak --runs N -- COMMAND [ARG...]"),
    };
    let flag = Arc::new(AtomicBool::new(false));
    watch(flag.clone());
    let stop = Flag(flag);
    let mut peaks: Vec<u128> = Vec::new();
    for i in 1..=runs {
        if stop.stopped() {
            out(&format!("notrun tool=tree-peak run={i} cause=signal"));
            return ExitCode::from(1);
        }
        let mut cmd = Command::new(&command[0]);
        cmd.args(&command[1..]);
        let m = match measure(&mut cmd, &stop) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("tree-peak: {e}");
                out(&format!("notrun tool=tree-peak run={i} cause=spawn"));
                return ExitCode::from(1);
            }
        };
        out(&format!(
            "tree run={i} exit={} peak_bytes={}",
            m.exit, m.peak
        ));
        out(&format!(
            "treeinfo run={i} processes_max={} adopted={} samples={} left={} lifetime_max_sum={}",
            m.processes_max, m.adopted, m.samples, m.left, m.lifetime_max_sum
        ));
        for (exe, bytes) in &m.parts {
            out(&format!("treepart run={i} exe={exe} bytes={bytes}"));
        }
        if stop.stopped() {
            out(&format!("notrun tool=tree-peak run={i} cause=signal"));
            return ExitCode::from(1);
        }
        peaks.push(m.peak as u128);
    }
    out(&format!("tree median_bytes={}", r3_web::median(&mut peaks)));
    ExitCode::SUCCESS
}
