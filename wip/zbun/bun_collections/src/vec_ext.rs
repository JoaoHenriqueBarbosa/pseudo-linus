//! `src/collections/vec_ext.rs` do upstream, parcial: a parte do `VecExt` que o parser usa e que faz alguma coisa
//! além de chamar o método do `Vec` com outro nome.
//!
//! Ficam fora, pela regra de repasse do repositório, os sinônimos de uma linha (`init_capacity`, `slice`,
//! `slice_mut`, `slice_const`, `append`, `append_slice`, `ensure_unused_capacity`, `shrink_retaining_capacity`,
//! `clear_retaining_capacity`, `ordered_remove`, `move_from_list`): quem os chamava usa o nome do `Vec`
//! (`with_capacity`, `as_slice`, `push`, `extend_from_slice`, `reserve`, `truncate`, `clear`, `remove`, o próprio
//! `Vec`). Os que dependem de ponteiro cru (`from_bump_slice`, `shallow_copy`, `reserve_spare`,
//! `unused_capacity_slice`) também ficam fora; o porte não tem o buffer de arena que eles atravessam.

use bun_alloc::ArenaVec;

pub trait VecExt<T>: Sized {
    fn init_one(value: T) -> Self;
    fn from_slice(items: &[T]) -> Self
    where
        T: Clone;
    fn from_owned_slice(items: Box<[T]>) -> Self;
    fn from_arena_slice(items: &[T]) -> Self
    where
        T: Copy;
    fn from_bump_vec(v: ArenaVec<'_, T>) -> Self;
    fn at(&self, index: usize) -> &T;
    fn mut_(&mut self, index: usize) -> &mut T;
    fn len_u32(&self) -> u32;
    fn append_assume_capacity(&mut self, value: T);
    fn ensure_total_capacity(&mut self, n: usize);
    fn shrink_and_free(&mut self, new_len: usize);
    fn clear_and_free(&mut self);
    fn move_to_list_managed(&mut self) -> Vec<T>;
    fn to_owned_slice(&mut self) -> Box<[T]>;
}

impl<T> VecExt<T> for Vec<T> {
    fn init_one(value: T) -> Self {
        let mut v = Vec::with_capacity(1);
        v.push(value);
        v
    }
    fn from_slice(items: &[T]) -> Self
    where
        T: Clone,
    {
        items.to_vec()
    }
    fn from_owned_slice(items: Box<[T]>) -> Self {
        items.into_vec()
    }
    fn from_arena_slice(items: &[T]) -> Self
    where
        T: Copy,
    {
        items.to_vec()
    }
    fn from_bump_vec(v: ArenaVec<'_, T>) -> Self {
        v.into_vec()
    }
    fn at(&self, index: usize) -> &T {
        &self[index]
    }
    fn mut_(&mut self, index: usize) -> &mut T {
        &mut self[index]
    }
    fn len_u32(&self) -> u32 {
        u32::try_from(self.len()).expect("int cast")
    }
    fn append_assume_capacity(&mut self, value: T) {
        debug_assert!(self.len() < self.capacity());
        self.push(value);
    }
    fn ensure_total_capacity(&mut self, n: usize) {
        let need = n.saturating_sub(self.len());
        self.reserve(need);
    }
    fn shrink_and_free(&mut self, new_len: usize) {
        self.truncate(new_len);
        self.shrink_to_fit();
    }
    fn clear_and_free(&mut self) {
        *self = Vec::new();
    }
    fn move_to_list_managed(&mut self) -> Vec<T> {
        core::mem::take(self)
    }
    fn to_owned_slice(&mut self) -> Box<[T]> {
        core::mem::take(self).into_boxed_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::VecExt;

    #[test]
    fn vec_ext_basics() {
        let mut v = Vec::init_one(1u32);
        v.ensure_total_capacity(4);
        assert!(v.capacity() >= 4);
        v.append_assume_capacity(2);
        *v.mut_(0) = 7;
        assert_eq!((*v.at(0), v.len_u32()), (7, 2));
        assert_eq!(&*v.to_owned_slice(), [7, 2]);
        assert!(v.is_empty());
        let mut w = Vec::from_slice(&[1u8, 2, 3]);
        w.shrink_and_free(1);
        assert_eq!(w, [1]);
    }
}
