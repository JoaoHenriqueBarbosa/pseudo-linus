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
