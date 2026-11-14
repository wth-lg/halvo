use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

#[derive(Default)]
pub(crate) struct Ids {
    state: RandomState,
    count: u64,
}

impl Ids {
    pub(crate) fn bytes(&mut self) -> [u8; 16] {
        self.count += 1;
        let mut out = [0u8; 16];
        for (half, part) in out.chunks_mut(8).zip(0u8..) {
            let mut hasher = self.state.build_hasher();
            hasher.write_u64(self.count);
            hasher.write_u8(part);
            half.copy_from_slice(&hasher.finish().to_le_bytes());
        }
        out
    }
}
