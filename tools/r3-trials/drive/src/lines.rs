/// The kinds of run of A13.6.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Compile,
    Replay,
    Recover,
    Write,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Compile => "compile",
            Kind::Replay => "replay",
            Kind::Recover => "recover",
            Kind::Write => "write",
        }
    }
}

/// What one run gave: its own check, its steps, its time in milliseconds with the model's load,
/// its tokens, and the two counts of A13.6.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub kind: Kind,
    pub i: u32,
    pub pass: bool,
    pub steps: u64,
    pub ms: u128,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub typed_credential: u64,
    pub effects_without_confirm: u64,
}

impl Run {
    /// A run that misses at once, as the replays and the recovery do on no playbook (A13.6).
    pub fn missed(kind: Kind, i: u32) -> Run {
        Run {
            kind,
            i,
            pass: false,
            steps: 0,
            ms: 0,
            tokens_in: 0,
            tokens_out: 0,
            typed_credential: 0,
            effects_without_confirm: 0,
        }
    }

    /// `wall_s`: the seconds of the run, rounded up.
    pub fn wall_s(&self) -> u128 {
        self.ms.div_ceil(1000)
    }

    pub fn model_tokens(&self) -> u64 {
        self.tokens_in + self.tokens_out
    }
}

/// One cell: the model, the machine, the task and the seed of its lines.
#[derive(Clone, Debug)]
pub struct Cell<'a> {
    pub model: &'a str,
    pub machine: &'a str,
    pub task: &'a str,
    pub seed: u8,
}

fn yes(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn ok(b: bool) -> &'static str {
    if b {
        "ok"
    } else {
        "miss"
    }
}

/// `attempt model=.. machine=.. task=.. seed=.. kind=.. i=.. pass=.. steps=.. wall_s=.. tokens_in=.. tokens_out=..`
pub fn attempt(cell: &Cell, run: &Run) -> String {
    format!(
        "attempt model={} machine={} task={} seed={} kind={} i={} pass={} steps={} wall_s={} tokens_in={} tokens_out={}",
        cell.model,
        cell.machine,
        cell.task,
        cell.seed,
        run.kind.as_str(),
        run.i,
        yes(run.pass),
        run.steps,
        run.wall_s(),
        run.tokens_in,
        run.tokens_out
    )
}

/// The `drive` line of a cell from its five runs, in the order compile, replay 1 to 3,
/// recover: `steps`, `wall_s` and `model_tokens` are the largest of the five, and the two
/// counts are their sums over the five.
pub fn drive(cell: &Cell, runs: &[Run]) -> String {
    let of = |kind: Kind| runs.iter().filter(move |r| r.kind == kind);
    let compile = of(Kind::Compile).any(|r| r.pass);
    let replays = of(Kind::Replay).filter(|r| r.pass).count();
    let recover = of(Kind::Recover).any(|r| r.pass);
    let steps = runs.iter().map(|r| r.steps).max().unwrap_or(0);
    let wall = runs.iter().map(Run::wall_s).max().unwrap_or(0);
    let tokens = runs.iter().map(Run::model_tokens).max().unwrap_or(0);
    let typed: u64 = runs.iter().map(|r| r.typed_credential).sum();
    let effects: u64 = runs.iter().map(|r| r.effects_without_confirm).sum();
    format!(
        "drive model={} machine={} task={} seed={} compile={} replays_ok={replays} of=3 recover={} steps={steps} wall_s={wall} model_tokens={tokens} typed_credential={typed} effects_without_confirm={effects}",
        cell.model,
        cell.machine,
        cell.task,
        cell.seed,
        ok(compile),
        ok(recover),
    )
}

/// `reply model=.. machine=.. seed=.. bodies=<n> of=2 wall_s=.. model_tokens=..`
pub fn reply(cell: &Cell, bodies: usize, run: &Run) -> String {
    format!(
        "reply model={} machine={} seed={} bodies={bodies} of=2 wall_s={} model_tokens={}",
        cell.model,
        cell.machine,
        cell.seed,
        run.wall_s(),
        run.model_tokens()
    )
}

/// `load model=.. machine=.. task=.. seed=.. kind=.. i=.. load1=<x>`: the 1-minute load of the
/// system when the run started, the line form of this trial for A13.1.
pub fn load(cell: &Cell, run_kind: Kind, i: u32, load1: f64) -> String {
    format!(
        "load model={} machine={} task={} seed={} kind={} i={i} load1={load1:.2}",
        cell.model,
        cell.machine,
        cell.task,
        cell.seed,
        run_kind.as_str()
    )
}

/// `notrun model=.. machine=.. task=.. seed=.. cause=<token>`
pub fn notrun(cell: &Cell, cause: &str) -> String {
    format!(
        "notrun model={} machine={} task={} seed={} cause={cause}",
        cell.model, cell.machine, cell.task, cell.seed
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(kind: Kind, i: u32, pass: bool, steps: u64, ms: u128, tin: u64, tout: u64) -> Run {
        Run {
            kind,
            i,
            pass,
            steps,
            ms,
            tokens_in: tin,
            tokens_out: tout,
            typed_credential: 0,
            effects_without_confirm: 0,
        }
    }

    const CELL: Cell = Cell {
        model: "minicpm5_2b",
        machine: "mac",
        task: "bill",
        seed: 2,
    };

    #[test]
    fn an_attempt_line_has_the_keys_of_a13_6_in_order() {
        let r = run(Kind::Replay, 3, true, 7, 12_001, 0, 0);
        assert_eq!(
            attempt(&CELL, &r),
            "attempt model=minicpm5_2b machine=mac task=bill seed=2 kind=replay i=3 pass=yes steps=7 wall_s=13 tokens_in=0 tokens_out=0"
        );
        let w = run(Kind::Write, 1, false, 0, 1000, 10, 5);
        let cell = Cell {
            task: "reply",
            ..CELL
        };
        assert_eq!(
            attempt(&cell, &w),
            "attempt model=minicpm5_2b machine=mac task=reply seed=2 kind=write i=1 pass=no steps=0 wall_s=1 tokens_in=10 tokens_out=5"
        );
    }

    #[test]
    fn wall_s_is_rounded_up() {
        assert_eq!(run(Kind::Compile, 1, true, 1, 0, 0, 0).wall_s(), 0);
        assert_eq!(run(Kind::Compile, 1, true, 1, 1, 0, 0).wall_s(), 1);
        assert_eq!(run(Kind::Compile, 1, true, 1, 1000, 0, 0).wall_s(), 1);
        assert_eq!(run(Kind::Compile, 1, true, 1, 1001, 0, 0).wall_s(), 2);
    }

    #[test]
    fn the_drive_line_takes_the_largest_of_the_five_runs_and_sums_the_counts() {
        let mut runs = vec![
            run(Kind::Compile, 1, true, 6, 90_500, 3000, 200),
            run(Kind::Replay, 1, true, 6, 9_000, 0, 0),
            run(Kind::Replay, 2, false, 3, 8_000, 0, 0),
            run(Kind::Replay, 3, true, 6, 95_000, 0, 0),
            run(Kind::Recover, 1, true, 7, 40_000, 3100, 150),
        ];
        runs[0].typed_credential = 1;
        runs[4].typed_credential = 2;
        runs[1].effects_without_confirm = 1;
        runs[4].effects_without_confirm = 1;
        assert_eq!(
            drive(&CELL, &runs),
            "drive model=minicpm5_2b machine=mac task=bill seed=2 compile=ok replays_ok=2 of=3 recover=ok steps=7 wall_s=95 model_tokens=3250 typed_credential=3 effects_without_confirm=2"
        );
        runs[0].pass = false;
        runs[4].pass = false;
        assert!(drive(&CELL, &runs).contains(" compile=miss replays_ok=2 of=3 recover=miss "));
    }

    #[test]
    fn a_missed_run_has_no_steps() {
        let m = Run::missed(Kind::Recover, 1);
        assert_eq!(
            attempt(&CELL, &m),
            "attempt model=minicpm5_2b machine=mac task=bill seed=2 kind=recover i=1 pass=no steps=0 wall_s=0 tokens_in=0 tokens_out=0"
        );
    }

    #[test]
    fn the_reply_load_and_notrun_lines() {
        let cell = Cell {
            task: "reply",
            ..CELL
        };
        let w = run(Kind::Write, 1, true, 0, 61_200, 400, 90);
        assert_eq!(
            reply(&cell, 2, &w),
            "reply model=minicpm5_2b machine=mac seed=2 bodies=2 of=2 wall_s=62 model_tokens=490"
        );
        assert_eq!(
            load(&CELL, Kind::Compile, 1, 141.987),
            "load model=minicpm5_2b machine=mac task=bill seed=2 kind=compile i=1 load1=141.99"
        );
        assert_eq!(
            notrun(&CELL, "browser_start"),
            "notrun model=minicpm5_2b machine=mac task=bill seed=2 cause=browser_start"
        );
    }
}
