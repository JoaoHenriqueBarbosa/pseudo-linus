//! Gerador pseudoaleatório determinístico (SplitMix64) e hash FNV-1a, sem dependência.

#[derive(Clone, Debug)]
pub struct SplitMix64(u64);

impl SplitMix64 {
    pub fn new(seed: u64) -> SplitMix64 {
        SplitMix64(seed)
    }

    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Inteiro em `0..n` (viés desprezível pra n pequeno).
    #[inline]
    pub fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n
    }

    /// Permutação de `0..n` (Fisher-Yates).
    pub fn permutation(&mut self, n: usize) -> Vec<usize> {
        let mut v: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            let j = self.below(i as u64 + 1) as usize;
            v.swap(i, j);
        }
        v
    }
}

/// FNV-1a de 64 bits.
pub fn fnv1a(data: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in data {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_permutation_is_complete() {
        let a: Vec<u64> = (0..5).scan(SplitMix64::new(1), |r, _| Some(r.next_u64())).collect();
        let b: Vec<u64> = (0..5).scan(SplitMix64::new(1), |r, _| Some(r.next_u64())).collect();
        assert_eq!(a, b);
        let mut p = SplitMix64::new(9).permutation(100);
        p.sort_unstable();
        assert_eq!(p, (0..100).collect::<Vec<_>>());
    }

    #[test]
    fn fnv_known_value() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
    }
}
