//! The named generators: ChaCha20 keyed with the SHA-256 of a text, drawn one 64-bit word at a time.

use rand_chacha::rand_core::{Rng, SeedableRng};
use rand_chacha::ChaCha20Rng;
use sha2::{Digest, Sha256};

/// A ChaCha20 generator keyed with the SHA-256 of the UTF-8 bytes of its name.
pub struct Draw(ChaCha20Rng);

impl Draw {
    /// The generator of this name, such as `halvo-sample build 1 golden`.
    pub fn named(name: &str) -> Draw {
        let key: [u8; 32] = Sha256::digest(name.as_bytes()).into();
        Draw(ChaCha20Rng::from_seed(key))
    }

    /// The next 64-bit word.
    pub fn word(&mut self) -> u64 {
        self.0.next_u64()
    }

    /// A choice among `n` values: the next word mod `n`. `n` must be above 0.
    pub fn below(&mut self, n: u64) -> u64 {
        self.word() % n
    }

    /// A value from `low` to `high`, both included: `low` plus the next word mod the number of values.
    pub fn between(&mut self, low: u64, high: u64) -> u64 {
        low + self.below(high - low + 1)
    }

    /// `count` values of `from`, drawn without repetition: each draw takes the next word mod the number of
    /// values left and removes the value at that index of the values left, in their order. The values are
    /// returned in the order of the draws.
    pub fn pick<T: Clone>(&mut self, from: &[T], count: usize) -> Vec<T> {
        let mut left: Vec<T> = from.to_vec();
        let mut out = Vec::with_capacity(count);
        for _ in 0..count.min(left.len()) {
            let i = self.below(left.len() as u64) as usize;
            out.push(left.remove(i));
        }
        out
    }

    /// Shuffles `list` in place (Fisher-Yates, from the last index down): for each index i from the last down
    /// to 1 the next word w swaps the entries i and w mod (i + 1).
    pub fn shuffle<T>(&mut self, list: &mut [T]) {
        for i in (1..list.len()).rev() {
            let j = self.below(i as u64 + 1) as usize;
            list.swap(i, j);
        }
    }
}
