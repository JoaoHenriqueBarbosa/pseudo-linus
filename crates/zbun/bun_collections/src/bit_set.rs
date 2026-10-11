//! O `IntegerBitSet` de `src/collections/bit_set.rs` do upstream: um conjunto de até `usize::BITS` bits guardado
//! numa palavra só. Entra o subconjunto que o lexer usa; o resto do arquivo vem quando alguém pedir.

/// Conjunto de `SIZE` bits numa única palavra.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct IntegerBitSet<const SIZE: usize> {
    /// The bit mask, as a single integer
    pub mask: usize,
}

impl<const SIZE: usize> IntegerBitSet<SIZE> {
    /// The number of items in this bit set
    pub(crate) const BIT_LENGTH: usize = SIZE;

    const FULL_MASK: usize = if SIZE as u32 >= usize::BITS {
        usize::MAX
    } else {
        (1usize << (SIZE as u32)) - 1
    };

    /// Creates a bit set with no elements present.
    pub const fn init_empty() -> Self {
        Self { mask: 0 }
    }

    /// Creates a bit set with all elements present.
    pub const fn init_full() -> Self {
        Self {
            mask: Self::FULL_MASK,
        }
    }

    /// Returns true if the bit at the specified index
    /// is present in the set, false otherwise.
    pub fn is_set(self, index: usize) -> bool {
        debug_assert!(index < Self::BIT_LENGTH);
        (self.mask & Self::mask_bit(index)) != 0
    }

    /// Returns the total number of set bits in this bit set.
    pub const fn count(self) -> usize {
        self.mask.count_ones() as usize
    }

    /// Adds a specific bit to the bit set
    pub fn set(&mut self, index: usize) {
        debug_assert!(index < Self::BIT_LENGTH);
        self.mask |= Self::mask_bit(index);
    }

    /// Removes a specific bit from the bit set
    pub fn unset(&mut self, index: usize) {
        debug_assert!(index < Self::BIT_LENGTH);
        if SIZE == 0 {
            return;
        }
        self.mask &= !Self::mask_bit(index);
    }

    /// Finds the index of the first set bit.
    /// If no bits are set, returns null.
    pub fn find_first_set(self) -> Option<usize> {
        if self.mask == 0 {
            return None;
        }
        Some(self.mask.trailing_zeros() as usize)
    }

    #[inline(always)]
    fn mask_bit(index: usize) -> usize {
        if SIZE == 0 {
            return 0;
        }
        1usize << index
    }
}

#[cfg(test)]
mod tests {
    use super::IntegerBitSet;

    #[test]
    fn set_unset_count() {
        let mut s = IntegerBitSet::<22>::init_empty();
        assert!(!s.is_set(3));
        s.set(3);
        s.set(21);
        assert!(s.is_set(3) && s.is_set(21));
        assert_eq!(s.count(), 2);
        assert_eq!(s.find_first_set(), Some(3));
        s.unset(3);
        assert_eq!(s.find_first_set(), Some(21));
        assert_eq!(IntegerBitSet::<22>::init_full().count(), 22);
    }
}

/// Conjunto de bits de tamanho conhecido só em tempo de execução. O upstream tem dois braços (`Static` até
/// `usize::BITS * 8 - 1` bits, `Dynamic` com `unsafe` de alocação); aqui um `Vec<usize>` serve aos dois, com a mesma
/// semântica: os bits depois de `bit_length` ficam zerados.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AutoBitSet {
    masks: Vec<usize>,
    bit_length: usize,
}

impl AutoBitSet {
    const WORD_BITS: usize = usize::BITS as usize;

    pub fn init_empty(bit_length: usize) -> Result<AutoBitSet, bun_alloc::AllocError> {
        let words = bit_length.div_ceil(Self::WORD_BITS);
        let mut masks = Vec::new();
        masks.try_reserve_exact(words).map_err(|_| bun_alloc::AllocError)?;
        masks.resize(words, 0);
        Ok(AutoBitSet { masks, bit_length })
    }

    pub fn bit_length(&self) -> usize {
        self.bit_length
    }

    pub fn is_set(&self, index: usize) -> bool {
        debug_assert!(index < self.bit_length);
        (self.masks[index / Self::WORD_BITS] >> (index % Self::WORD_BITS)) & 1 != 0
    }

    pub fn set(&mut self, index: usize) {
        debug_assert!(index < self.bit_length);
        self.masks[index / Self::WORD_BITS] |= 1usize << (index % Self::WORD_BITS);
    }

    pub fn unset(&mut self, index: usize) {
        debug_assert!(index < self.bit_length);
        self.masks[index / Self::WORD_BITS] &= !(1usize << (index % Self::WORD_BITS));
    }

    pub fn set_all(&mut self, value: bool) {
        self.masks.fill(if value { usize::MAX } else { 0 });
        let tail = self.bit_length % Self::WORD_BITS;
        if value && tail != 0 {
            if let Some(last) = self.masks.last_mut() {
                *last = (1usize << tail) - 1;
            }
        }
    }

    /// Os bits de `self` também ligados em `other`? Tamanhos diferentes nunca se cruzam (como no upstream).
    pub fn has_intersection(&self, other: &AutoBitSet) -> bool {
        self.bit_length == other.bit_length && self.masks.iter().zip(&other.masks).any(|(a, b)| a & b != 0)
    }

    /// `self |= other`; os dois têm o mesmo `bit_length`.
    pub fn set_union(&mut self, other: &AutoBitSet) {
        assert_eq!(self.bit_length, other.bit_length, "AutoBitSet::set_union: mismatched bit lengths");
        for (a, b) in self.masks.iter_mut().zip(&other.masks) {
            *a |= b;
        }
    }

    /// `self &= other`; os dois têm o mesmo `bit_length`.
    pub fn set_intersection(&mut self, other: &AutoBitSet) {
        assert_eq!(self.bit_length, other.bit_length, "AutoBitSet::set_intersection: mismatched bit lengths");
        for (a, b) in self.masks.iter_mut().zip(&other.masks) {
            *a &= b;
        }
    }

    /// Todo bit de `self` também está em `other`?
    pub fn subset_of(&self, other: &AutoBitSet) -> bool {
        assert_eq!(self.bit_length, other.bit_length, "AutoBitSet::subset_of: mismatched bit lengths");
        self.masks.iter().zip(&other.masks).all(|(a, b)| a & !b == 0)
    }

    /// As palavras de apoio: o bit `i` é `words()[i / usize::BITS] >> (i % usize::BITS) & 1`.
    pub fn words(&self) -> &[usize] {
        &self.masks
    }

    pub fn count(&self) -> usize {
        self.masks.iter().map(|m| m.count_ones() as usize).sum()
    }

    pub fn find_first_set(&self) -> Option<usize> {
        self.masks
            .iter()
            .position(|&m| m != 0)
            .map(|word| word * Self::WORD_BITS + self.masks[word].trailing_zeros() as usize)
    }
}

#[cfg(test)]
mod auto_tests {
    use super::AutoBitSet;

    #[test]
    fn crosses_the_word_boundary() {
        let mut s = AutoBitSet::init_empty(130).unwrap();
        assert_eq!(s.find_first_set(), None);
        s.set(1);
        s.set(64);
        s.set(129);
        assert!(s.is_set(64) && !s.is_set(65));
        assert_eq!(s.count(), 3);
        let mut t = AutoBitSet::init_empty(130).unwrap();
        t.set(64);
        assert!(s.has_intersection(&t) && t.subset_of(&s) && !s.subset_of(&t));
        s.unset(1);
        assert_eq!(s.find_first_set(), Some(64));
        s.set_all(true);
        assert_eq!(s.count(), 130);
        s.set_intersection(&t);
        assert_eq!(s.count(), 1);
        s.set_union(&t);
        assert_eq!(s.words().len(), 3);
    }
}
