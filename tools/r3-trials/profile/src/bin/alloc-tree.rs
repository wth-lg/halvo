use std::process::{Command, ExitCode};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MIB: usize = 1_048_576;

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn hold(mib: usize, until: u128) {
    let block = vec![0x5a_u8; mib * MIB];
    std::hint::black_box(&block);
    while now_ms() < until {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::hint::black_box(&block);
}

fn usage() -> ExitCode {
    eprintln!(
        "REFUSED usage: alloc-tree --mib A[,B...] [--orphan MIB] --hold-ms N; each child holds its MiB, written, until the parent's deadline"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let me = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return usage(),
    };
    match args.as_slice() {
        [flag, mib, until] if flag == "--child" => {
            let (Ok(mib), Ok(until)) = (mib.parse::<usize>(), until.parse::<u128>()) else {
                return usage();
            };
            hold(mib, until);
            ExitCode::SUCCESS
        }
        [flag, mib, until] if flag == "--orphan-parent" => {
            let started = Command::new(&me).args(["--child", mib, until]).spawn();
            if started.is_err() {
                return ExitCode::from(1);
            }
            ExitCode::SUCCESS
        }
        _ => {
            let mut mibs: Vec<usize> = Vec::new();
            let mut orphan: Option<usize> = None;
            let mut hold_ms: Option<u128> = None;
            let mut i = 0;
            while i < args.len() {
                let value = args.get(i + 1).cloned().unwrap_or_default();
                match args[i].as_str() {
                    "--mib" => {
                        let parsed: Result<Vec<usize>, _> =
                            value.split(',').map(|v| v.parse::<usize>()).collect();
                        match parsed {
                            Ok(v) => mibs = v,
                            Err(_) => return usage(),
                        }
                    }
                    "--orphan" => match value.parse() {
                        Ok(v) => orphan = Some(v),
                        Err(_) => return usage(),
                    },
                    "--hold-ms" => match value.parse() {
                        Ok(v) => hold_ms = Some(v),
                        Err(_) => return usage(),
                    },
                    _ => return usage(),
                }
                i += 2;
            }
            let Some(hold_ms) = hold_ms else {
                return usage();
            };
            let until = (now_ms() + hold_ms).to_string();
            let mut children = Vec::new();
            for m in &mibs {
                match Command::new(&me)
                    .args(["--child", &m.to_string(), &until])
                    .spawn()
                {
                    Ok(c) => children.push(c),
                    Err(_) => return ExitCode::from(1),
                }
            }
            if let Some(m) = orphan {
                match Command::new(&me)
                    .args(["--orphan-parent", &m.to_string(), &until])
                    .status()
                {
                    Ok(s) if s.success() => {}
                    _ => return ExitCode::from(1),
                }
            }
            let expected: usize = mibs.iter().sum::<usize>() + orphan.unwrap_or(0);
            println!(
                "alloc children={} orphan_mib={} expected_bytes={}",
                mibs.len(),
                orphan.unwrap_or(0),
                expected * MIB
            );
            for mut c in children {
                let _ = c.wait();
            }
            ExitCode::SUCCESS
        }
    }
}
