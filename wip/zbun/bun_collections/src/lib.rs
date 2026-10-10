//! Porte para Rust seguro do subconjunto de `bun_collections` (`src/collections` do bun 1.4.2) que o transpilador
//! usa. Os módulos mantêm nome e estrutura do upstream; o README de `wip/zbun` diz o que já entrou.
#![forbid(unsafe_code)]

pub mod array_hash_map;
pub mod bit_set;
pub mod identity_context;
pub mod index_sort;
pub mod pool;
pub mod vec_ext;
pub mod zig_hash_map;

#[doc(hidden)]
pub use paste as __paste;

pub use array_hash_map::{
    AllocError, ArrayHashMap, OwnKey, StringArrayHashMap, StringHashMap, StringHashMapGetOrPut, StringSet,
};
pub use bit_set::IntegerBitSet;
pub use identity_context::{IdentityContext, IdentityHash};
pub use pool::{ObjectPool, ObjectPoolType, PoolGuard};
pub use vec_ext::{VecExt, prepend_from};
pub use zig_hash_map::{AutoHashContext, HashContext, HashMap};

/// std-compat path so call sites that wrote `bun_collections::hash_map::Entry`
/// against the old std-alias keep compiling.
pub mod hash_map {
    /// Result of `HashMap::get_or_put`: the unordered map has no stable index
    /// or key slot to hand out, so unlike `array_hash_map::GetOrPutResult` this
    /// only exposes `found_existing` + `value_ptr`.
    pub struct GetOrPutResult<'a, V> {
        pub found_existing: bool,
        pub value_ptr: &'a mut V,
    }

    /// Owned `{key, value}` pair returned from `fetchRemove` / `fetchPut`;
    /// re-exported from `array_hash_map` rather than duplicated.
    pub use crate::array_hash_map::KV;
}
