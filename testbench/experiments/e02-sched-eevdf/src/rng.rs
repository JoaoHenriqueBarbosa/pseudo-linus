//! Gerador pseudoaleatório determinístico (SplitMix64) pros benchmarks, sem dependência.

/// SplitMix64.
#[derive(Clone, Debug)]
pub struct SplitMix(u64);

impl SplitMix {
    /// Gerador com semente.
    pub fn new(seed: u64) -> SplitMix {
        SplitMix(seed)
    }

    /// Próximo número de 64 bits.
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Inteiro em [0, n).
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// Inteiro em [lo, hi].
    pub fn range_i64(&mut self, lo: i64, hi: i64) -> i64 {
        lo + self.below((hi - lo + 1) as u64) as i64
    }
}
