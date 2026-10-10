//! `src/ast/g.rs` do upstream, portado item a item na ordem em que os outros módulos pedem.
use crate::{Ref, StoreStr};

pub struct NamespaceAlias {
    pub namespace_ref: Ref,
    pub alias: StoreStr,

    pub was_originally_property_access: bool,

    pub import_record_index: u32,
}

impl Default for NamespaceAlias {
    fn default() -> Self {
        Self {
            namespace_ref: Ref::default(),
            alias: StoreStr::EMPTY,
            was_originally_property_access: false,
            import_record_index: u32::MAX,
        }
    }
}
