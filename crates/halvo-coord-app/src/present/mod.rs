mod card;
mod outcome;
#[cfg(test)]
mod tests;

pub use card::Card;
pub use outcome::{decide, Outcome};
