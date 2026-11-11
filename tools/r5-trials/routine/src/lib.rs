pub mod files;
pub mod rule;
pub mod sample;
pub mod train;
pub mod trial;

use std::path::Path;

use files::{parse_seeds, parse_targets, read_text, Refusal};
use rule::{Setting, NO_MARK};
use train::{train, Cell, Train, TRAINS_PER_CELL};
use trial::{results, verdict, SettingResult};

pub const SEEDS_PATH: &str = "tools/r5-trials/routine/seeds.txt";
pub const TARGETS_PATH: &str = "docs/r5/targets.toml";

pub struct Trial {
    pub cells: Vec<Cell>,
    pub trains: Vec<Vec<Train>>,
    pub results: Vec<SettingResult>,
}

impl Trial {
    pub fn verdict(&self) -> Option<Setting> {
        verdict(&self.results)
    }

    pub fn block(&self) -> String {
        trial::block(&self.results)
    }

    pub fn sample_setting(&self) -> Setting {
        self.verdict().unwrap_or(NO_MARK)
    }

    pub fn sample(&self) -> String {
        sample::sample(&self.cells, &self.trains, self.sample_setting())
    }
}

pub fn trains_of(cell: &Cell) -> Vec<Train> {
    (1..=TRAINS_PER_CELL).map(|n| train(cell, n)).collect()
}

pub fn run(root: &Path) -> Result<Trial, Refusal> {
    let cells = parse_seeds(read_text(&root.join(SEEDS_PATH)).as_deref())?;
    let lines = parse_targets(read_text(&root.join(TARGETS_PATH)).as_deref())?.lines()?;
    let trains: Vec<Vec<Train>> = cells.iter().map(trains_of).collect();
    let results = results(&cells, &trains, lines);
    Ok(Trial {
        cells,
        trains,
        results,
    })
}

#[cfg(test)]
mod tests;
