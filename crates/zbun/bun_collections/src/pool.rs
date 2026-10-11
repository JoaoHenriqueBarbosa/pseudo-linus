//! `src/collections/pool.rs` do upstream, na parte que o parser usa: o `object_pool!` com armazenamento por thread,
//! o `ObjectPoolType` e o `PoolGuard`.
//!
//! No upstream a lista livre é uma lista ligada de nós em ponteiro cru, com `MaybeUninit` para os tipos sem
//! `INIT`. Aqui é um `Vec<T>` por thread usado como pilha (o último devolvido é o próximo entregue, como o
//! `prepend`/`pop_first` de lá), e o guarda é dono do valor enquanto vive. `INIT` é obrigatório, porque não existe
//! valor "não inicializado" sem `unsafe`, e devolve `Self` direto: o único `INIT` do parser (`StringVoidMap`) não
//! falha, e o `Result<Self, bun_core::Error>` do upstream só existia para os pools do servidor. O limite de
//! `MAX_COUNT` é o do upstream: com a pilha cheia, o valor devolvido é solto.

use core::cell::RefCell;
use core::marker::PhantomData;
use core::ops::{Deref, DerefMut};
use std::thread::LocalKey;

pub trait ObjectPoolType: Sized + 'static {
    /// Initializer for freshly allocated values.
    const INIT: fn() -> Self;

    /// Called when a value is reused from the free list. Default is a no-op.
    #[inline]
    fn reset(&mut self) {}
}

/// O armazenamento por thread que o `object_pool!` declara.
pub trait PoolStorage<T: 'static> {
    const DATA: &'static LocalKey<RefCell<Vec<T>>>;
}

pub struct ObjectPool<T, const MAX_COUNT: usize, S>(PhantomData<(T, S)>);

impl<T: ObjectPoolType, const MAX_COUNT: usize, S: PoolStorage<T>> ObjectPool<T, MAX_COUNT, S> {
    /// RAII front-door: pop or allocate a value and wrap it in a `PoolGuard`
    /// that returns it to this pool on `Drop`.
    pub fn get() -> PoolGuard<'static, T> {
        let reused = S::DATA.with_borrow_mut(Vec::pop);
        let value = match reused {
            Some(mut value) => {
                value.reset();
                value
            }
            None => (T::INIT)(),
        };
        PoolGuard { value: Some(value), release: Self::release, _marker: PhantomData }
    }

    /// Return a value to the pool's free list (or drop it if the pool is full).
    fn release(value: T) {
        S::DATA.with_borrow_mut(|list| {
            if MAX_COUNT == 0 || list.len() < MAX_COUNT {
                list.push(value);
            }
        });
    }
}

pub struct PoolGuard<'a, T: ObjectPoolType> {
    value: Option<T>,
    release: fn(T),
    _marker: PhantomData<&'a mut T>,
}

impl<T: ObjectPoolType> Deref for PoolGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.value.as_ref().expect("PoolGuard vivo sempre tem valor")
    }
}

impl<T: ObjectPoolType> DerefMut for PoolGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value.as_mut().expect("PoolGuard vivo sempre tem valor")
    }
}

impl<T: ObjectPoolType> Drop for PoolGuard<'_, T> {
    fn drop(&mut self) {
        if let Some(value) = self.value.take() {
            (self.release)(value);
        }
    }
}

/// `object_pool!(pub Name: T, threadsafe, MAX)`: declara o armazenamento por thread e o tipo do pool.
#[macro_export]
macro_rules! object_pool {
    ($vis:vis $name:ident : $ty:ty, threadsafe, $max:expr) => {
        $crate::__paste::paste! {
            ::std::thread_local! {
                static [<__ $name:snake:upper _DATA>]: ::core::cell::RefCell<::std::vec::Vec<$ty>> =
                    const { ::core::cell::RefCell::new(::std::vec::Vec::new()) };
            }
            #[doc(hidden)]
            $vis struct [<__ $name Storage>];
            impl $crate::pool::PoolStorage<$ty> for [<__ $name Storage>] {
                const DATA: &'static ::std::thread::LocalKey<::core::cell::RefCell<::std::vec::Vec<$ty>>> =
                    &[<__ $name:snake:upper _DATA>];
            }
            $vis type $name = $crate::pool::ObjectPool<$ty, { $max }, [<__ $name Storage>]>;
        }
    };
}

#[cfg(test)]
mod tests {
    use super::ObjectPoolType;

    struct Scratch(Vec<u8>);

    impl ObjectPoolType for Scratch {
        const INIT: fn() -> Self = || Scratch(Vec::new());
        fn reset(&mut self) {
            self.0.clear();
        }
    }

    crate::object_pool!(ScratchPool: Scratch, threadsafe, 1);

    #[test]
    fn reuses_and_resets() {
        {
            let mut a = ScratchPool::get();
            a.0.extend_from_slice(b"abc");
            a.0.reserve(64);
        }
        let b = ScratchPool::get();
        assert!(b.0.is_empty());
        assert!(b.0.capacity() >= 64);
        let c = ScratchPool::get();
        assert_eq!(c.0.capacity(), 0);
        drop(b);
        drop(c);
        // MAX_COUNT = 1: o segundo devolvido é solto, e só um volta.
        let d = ScratchPool::get();
        let e = ScratchPool::get();
        assert!(d.0.capacity() >= 64);
        assert_eq!(e.0.capacity(), 0);
    }
}
