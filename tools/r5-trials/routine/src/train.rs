use rand_chacha::rand_core::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;

pub const TRAIN_DAYS: u32 = 730;
pub const TRAINS_PER_CELL: u32 = 200;
pub const LAST_SAME_TRAIN: u32 = 100;
pub const LAST_STOP_TRAIN: u32 = 150;
pub const CHANGE_FIRST_DAY: u32 = 200;
pub const CHANGE_LAST_DAY: u32 = 500;
pub const PAUSE_FIRST_DAY: u32 = 60;
pub const PAUSE_LAST_DAY: u32 = 180;
pub const PAUSED_GRID_DAYS: usize = 2;
pub const FIRST_MISSABLE_GRID_INDEX: usize = 3;
pub const SEED_PER_CELL: u64 = 1000;
pub const PERCENT: u32 = 100;

pub const JITTERS: [u32; 3] = [0, 1, 3];
pub const MISS_PCTS: [u32; 3] = [0, 10, 20];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cadence {
    Seven,
    Fourteen,
    Thirty,
}

impl Cadence {
    pub const ALL: [Cadence; 3] = [Cadence::Seven, Cadence::Fourteen, Cadence::Thirty];

    pub fn days(self) -> u32 {
        match self {
            Cadence::Seven => 7,
            Cadence::Fourteen => 14,
            Cadence::Thirty => 30,
        }
    }

    pub fn after_shift(self) -> Cadence {
        match self {
            Cadence::Seven => Cadence::Fourteen,
            Cadence::Fourteen => Cadence::Thirty,
            Cadence::Thirty => Cadence::Fourteen,
        }
    }

    pub fn from_days(days: u32) -> Option<Cadence> {
        Cadence::ALL.into_iter().find(|c| c.days() == days)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub cadence: Cadence,
    pub jitter: u32,
    pub miss_pct: u32,
    pub seed: u64,
}

impl Cell {
    pub fn name(&self) -> String {
        format!("{}-{}-{}", self.cadence.days(), self.jitter, self.miss_pct)
    }
}

pub fn grid() -> Vec<Cell> {
    let mut cells = Vec::new();
    for cadence in Cadence::ALL {
        for jitter in JITTERS {
            for miss_pct in MISS_PCTS {
                let seed = cells.len() as u64 + 1;
                cells.push(Cell {
                    cadence,
                    jitter,
                    miss_pct,
                    seed,
                });
            }
        }
    }
    cells
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Same,
    Stop,
    Shift,
}

impl Kind {
    pub fn of(number: u32) -> Kind {
        if number <= LAST_SAME_TRAIN {
            Kind::Same
        } else if number <= LAST_STOP_TRAIN {
            Kind::Stop
        } else {
            Kind::Shift
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Kind::Same => "same",
            Kind::Stop => "stop",
            Kind::Shift => "shift",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Train {
    pub number: u32,
    pub kind: Kind,
    pub days: Vec<u32>,
}

pub struct Draws(ChaCha20Rng);

impl Draws {
    pub fn new(cell: &Cell, number: u32) -> Draws {
        Draws(ChaCha20Rng::seed_from_u64(
            cell.seed * SEED_PER_CELL + u64::from(number),
        ))
    }

    pub fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }

    pub fn from_to(&mut self, a: u32, b: u32) -> u32 {
        a + self.next_u32() % (b - a + 1)
    }

    pub fn left_out(&mut self, miss_pct: u32) -> bool {
        self.next_u32() % PERCENT < miss_pct
    }
}

pub fn grid_days(cadence: Cadence, shift_from: Option<u32>) -> Vec<u32> {
    let mut days = Vec::new();
    let mut step = cadence.days();
    let mut shift_from = shift_from;
    let mut day = 1;
    while day <= TRAIN_DAYS {
        days.push(day);
        if shift_from.is_some_and(|from| day >= from) {
            step = cadence.after_shift().days();
            shift_from = None;
        }
        day += step;
    }
    days
}

pub fn train(cell: &Cell, number: u32) -> Train {
    let kind = Kind::of(number);
    let mut draws = Draws::new(cell, number);
    let change = (kind != Kind::Same).then(|| draws.from_to(CHANGE_FIRST_DAY, CHANGE_LAST_DAY));
    let pause =
        (cell.cadence == Cadence::Seven).then(|| draws.from_to(PAUSE_FIRST_DAY, PAUSE_LAST_DAY));
    let grid = grid_days(cell.cadence, change.filter(|_| kind == Kind::Shift));
    let paused: Vec<u32> = pause.map_or_else(Vec::new, |pause| {
        grid.iter()
            .copied()
            .filter(|&g| g > pause)
            .take(PAUSED_GRID_DAYS)
            .collect()
    });
    let stop = change.filter(|_| kind == Kind::Stop);
    let mut days = Vec::new();
    for (index, &grid_day) in grid.iter().enumerate() {
        let day = grid_day + draws.from_to(0, cell.jitter);
        let left_out = index >= FIRST_MISSABLE_GRID_INDEX && draws.left_out(cell.miss_pct);
        let stands = !left_out
            && !paused.contains(&grid_day)
            && day <= TRAIN_DAYS
            && stop.is_none_or(|stop| day <= stop);
        if stands {
            days.push(day);
        }
    }
    Train { number, kind, days }
}
