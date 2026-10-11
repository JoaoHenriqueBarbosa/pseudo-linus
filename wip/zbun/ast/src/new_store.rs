//! O dono dos nós da AST. No upstream, `new_store.rs` é a arena de blocos do mimalloc que o `StoreRef` aponta; no
//! porte, o papel é o mesmo e o meio muda: um `Vec<T>` por tipo de nó e um `Vec<u8>` para as strings, e os
//! `StoreRef`/`StoreSlice`/`StoreStr` de `nodes.rs` são índices nesses vetores (ver a seção "Arena tipada" do
//! README de `wip/zbun`).
//!
//! O `Store` concreto do transpilador é declarado com `define_store!`, uma linha por tipo de nó portado.

use crate::nodes::{StoreRef, StoreSlice, StoreStr};

/// Liga o tipo de nó `Self` ao seu vetor dentro do store `S`. Implementado por `define_store!`.
pub trait Stored<S>: Sized {
    fn nodes(store: &S) -> &Vec<Self>;
    fn nodes_mut(store: &mut S) -> &mut Vec<Self>;
}

/// As operações de um store declarado com `define_store!`.
pub trait NodeStore: Sized {
    fn bytes(&self) -> &Vec<u8>;
    fn bytes_mut(&mut self) -> &mut Vec<u8>;
    fn utf16(&self) -> &Vec<u16>;
    fn utf16_mut(&mut self) -> &mut Vec<u16>;

    /// Grava `node` e devolve o índice dele.
    fn alloc<T: Stored<Self>>(&mut self, node: T) -> StoreRef<T> {
        let nodes = T::nodes_mut(self);
        let position = to_u32(nodes.len());
        nodes.push(node);
        StoreRef::from_position(position)
    }

    /// Grava os nós em sequência e devolve a faixa contígua que eles ocupam.
    fn alloc_slice<T: Stored<Self>>(&mut self, items: impl IntoIterator<Item = T>) -> StoreSlice<T> {
        let nodes = T::nodes_mut(self);
        let start = nodes.len();
        nodes.extend(items);
        StoreSlice::from_range(to_u32(start), to_u32(nodes.len() - start))
    }

    /// O `VecExt::append` do upstream sobre uma lista da arena: a faixa com `item` no fim. Se a faixa termina no fim
    /// do vetor, cresce no lugar; senão os elementos são copiados para o fim antes, e quem guardava a faixa antiga
    /// continua vendo os mesmos elementos.
    fn append<T: Stored<Self> + Clone>(&mut self, items: StoreSlice<T>, item: T) -> StoreSlice<T> {
        let nodes = T::nodes_mut(self);
        let range = items.range();
        let start = if range.end == nodes.len() {
            range.start
        } else {
            let start = nodes.len();
            nodes.extend_from_within(range);
            start
        };
        nodes.push(item);
        StoreSlice::from_range(to_u32(start), to_u32(items.len() + 1))
    }

    /// O `orderedRemove` do upstream sobre uma lista da arena: tira o elemento `index`, desloca os seguintes uma
    /// posição para trás (a ordem se mantém) e encurta a faixa em `items`.
    fn ordered_remove<T: Stored<Self> + Clone>(&mut self, items: &mut StoreSlice<T>, index: usize) -> T {
        let range = items.range();
        let slots = &mut T::nodes_mut(self)[range];
        let removed = slots[index].clone();
        slots[index..].rotate_left(1);
        items.truncate(slots.len() - 1);
        removed
    }

    /// O `clone_in` do upstream: um nó novo, com outro índice, igual ao original. As faixas internas são copiadas
    /// como números, então as duas cópias veem os mesmos elementos, como no upstream.
    fn clone_in<T: Stored<Self> + Clone>(&mut self, node: StoreRef<T>) -> StoreRef<T> {
        let copy = T::nodes(self)[node.position()].clone();
        self.alloc(copy)
    }

    /// Grava os bytes no vetor de strings e devolve a faixa.
    fn alloc_str(&mut self, text: &[u8]) -> StoreStr {
        let bytes = self.bytes_mut();
        let start = bytes.len();
        bytes.extend_from_slice(text);
        StoreStr::from_range(to_u32(start), to_u32(text.len()))
    }

    /// Copia uma string que já está no Store para uma faixa nova (o `alloc_str(str(x))` sem a fatia emprestada, que
    /// prenderia o Store enquanto ele cresce).
    fn dup_str(&mut self, text: StoreStr) -> StoreStr {
        let bytes = self.bytes_mut();
        let start = bytes.len();
        bytes.extend_from_within(text.range());
        StoreStr::from_range(to_u32(start), to_u32(text.range().len()))
    }

    /// Os bytes de uma `StoreStr`.
    fn str(&self, text: StoreStr) -> &[u8] {
        &self.bytes()[text.range()]
    }

    /// Grava unidades UTF-16 no vetor de strings UTF-16 e devolve a faixa. Como no upstream, o tamanho da faixa é a
    /// contagem de unidades, não de bytes; quem lê sabe pela flag `is_utf16` do `EString` qual vetor a faixa indexa.
    fn alloc_str16(&mut self, text: &[u16]) -> StoreStr {
        let units = self.utf16_mut();
        let start = units.len();
        units.extend_from_slice(text);
        StoreStr::from_range(to_u32(start), to_u32(text.len()))
    }

    /// As unidades de uma `StoreStr` gravada por `alloc_str16`.
    fn str16(&self, text: StoreStr) -> &[u16] {
        &self.utf16()[text.range()]
    }
}

/// O store indexa com `u32`, como o upstream limita `StoreSlice` a 4 G elementos.
fn to_u32(n: usize) -> u32 {
    u32::try_from(n).expect("Store: mais de 4 G elementos de um tipo")
}

/// Declara um store com um vetor por tipo de nó: `define_store! { pub struct Store { e_binary: E::Binary, ... } }`.
/// Gera os `Stored`, o `NodeStore` e os `Index`/`IndexMut` por `StoreRef<T>`, `StoreSlice<T>` e `StoreStr`.
#[macro_export]
macro_rules! define_store {
    ($(#[$meta:meta])* $vis:vis struct $store:ident { $($field:ident: $node:ty),* $(,)? }) => {
        $(#[$meta])*
        // `Clone`: o `Store` de cada arquivo nasce como cópia do `define_store` (README, "`Define` com um `Store` por
        // arquivo").
        #[derive(Default, Clone)]
        $vis struct $store {
            bytes: Vec<u8>,
            utf16: Vec<u16>,
            $($field: Vec<$node>,)*
        }

        $(
            impl $crate::new_store::Stored<$store> for $node {
                #[inline]
                fn nodes(store: &$store) -> &Vec<$node> {
                    &store.$field
                }
                #[inline]
                fn nodes_mut(store: &mut $store) -> &mut Vec<$node> {
                    &mut store.$field
                }
            }
        )*

        impl $crate::new_store::NodeStore for $store {
            #[inline]
            fn bytes(&self) -> &Vec<u8> {
                &self.bytes
            }
            #[inline]
            fn bytes_mut(&mut self) -> &mut Vec<u8> {
                &mut self.bytes
            }
            #[inline]
            fn utf16(&self) -> &Vec<u16> {
                &self.utf16
            }
            #[inline]
            fn utf16_mut(&mut self) -> &mut Vec<u16> {
                &mut self.utf16
            }
        }

        impl<T: $crate::new_store::Stored<$store>> core::ops::Index<$crate::nodes::StoreRef<T>> for $store {
            type Output = T;
            #[inline]
            fn index(&self, node: $crate::nodes::StoreRef<T>) -> &T {
                &T::nodes(self)[node.position()]
            }
        }
        impl<T: $crate::new_store::Stored<$store>> core::ops::IndexMut<$crate::nodes::StoreRef<T>> for $store {
            #[inline]
            fn index_mut(&mut self, node: $crate::nodes::StoreRef<T>) -> &mut T {
                &mut T::nodes_mut(self)[node.position()]
            }
        }
        impl<T: $crate::new_store::Stored<$store>> core::ops::Index<$crate::nodes::StoreSlice<T>> for $store {
            type Output = [T];
            #[inline]
            fn index(&self, items: $crate::nodes::StoreSlice<T>) -> &[T] {
                &T::nodes(self)[items.range()]
            }
        }
        impl<T: $crate::new_store::Stored<$store>> core::ops::IndexMut<$crate::nodes::StoreSlice<T>> for $store {
            #[inline]
            fn index_mut(&mut self, items: $crate::nodes::StoreSlice<T>) -> &mut [T] {
                &mut T::nodes_mut(self)[items.range()]
            }
        }
    };
}
