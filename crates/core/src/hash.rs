//! FxHash (rustc's hasher): much cheaper than the default SipHash for the short string and
//! integer keys used here. Not DoS-resistant, which a single-request renderer doesn't need.

use std::hash::{BuildHasherDefault, Hasher};

pub type HashMap<K, V> = std::collections::HashMap<K, V, BuildHasherDefault<FxHasher>>;
pub type HashSet<K> = std::collections::HashSet<K, BuildHasherDefault<FxHasher>>;
pub type IndexMap<K, V> = indexmap::IndexMap<K, V, BuildHasherDefault<FxHasher>>;

#[derive(Default, Clone, Copy)]
pub struct FxHasher(u64);

impl FxHasher {
    #[inline]
    fn add(&mut self, word: u64) {
        self.0 = (self.0.rotate_left(5) ^ word).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, mut bytes: &[u8]) {
        while let Some((chunk, rest)) = bytes.split_first_chunk::<8>() {
            self.add(u64::from_le_bytes(*chunk));
            bytes = rest;
        }
        if let Some((chunk, rest)) = bytes.split_first_chunk::<4>() {
            self.add(u32::from_le_bytes(*chunk) as u64);
            bytes = rest;
        }
        for &b in bytes {
            self.add(b as u64);
        }
    }
    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(i as u64);
    }
    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }
    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }
}
