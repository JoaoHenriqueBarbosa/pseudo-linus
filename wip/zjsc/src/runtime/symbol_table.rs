//! Porte da parte pura de `runtime/SymbolTable.h`: `SymbolTableEntry` (com `Fast`), o enum
//! `SymbolTable::ScopeType` e `PropagateCloneInvalidationToOriginal`.
//!
//! Fica de fora a classe `SymbolTable` (é um `JSCell` com `WriteBarrier<ScopedArgumentsTable>`,
//! `InferredValue<JSScope>` e travas), que entra com o heap.
//!
//! `SymbolTableEntry` só existe na forma fina (`SlimFlag`). A forma gorda (`FatEntry` com
//! `InlineWatchpointSet`) só nasce de `prepareToWatch`, que exige `isWatchable()`, e esta depende de
//! `Options::useJIT()`, que aqui é sempre falso (sem JIT). Logo `isFat()` é sempre falso,
//! `watchpointSet()` é sempre nulo, `inflate`/`freeFatEntry` não têm o que fazer.

use crate::runtime::constant_mode::{mode_for_is_constant, ConstantMode};
use crate::runtime::property_attribute::{DONT_ENUM, READ_ONLY};
use crate::runtime::scope_offset::ScopeOffset;
use crate::runtime::var_offset::{VarKind, VarOffset};

/// `NoLockingNecessaryTag` / `NoLockingNecessary` de `wtf/Locker.h`, o argumento que o gerador de
/// bytecode passa no lugar do `ConcurrentJSLocker` (o parser e o gerador são de uma thread só).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoLockingNecessaryTag;

pub const NO_LOCKING_NECESSARY: NoLockingNecessaryTag = NoLockingNecessaryTag;

/// `missingSymbolMarker()`.
pub const fn missing_symbol_marker() -> i32 {
    i32::MAX
}

/// `SymbolTable::ScopeType`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScopeType {
    VarScope,
    GlobalLexicalScope,
    LexicalScope,
    CatchScope,
    CatchScopeWithSimpleParameter,
    FunctionNameScope,
}

pub use self::ScopeType as SymbolTableScopeType;

/// `SymbolTable::PropagateCloneInvalidationToOriginal : bool`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PropagateCloneInvalidationToOriginal {
    No,
    Yes,
}

const SLIM_FLAG: isize = 0x1;
const READ_ONLY_FLAG: isize = 0x2;
const DONT_ENUM_FLAG: isize = 0x4;
const NOT_NULL_FLAG: isize = 0x8;
const KIND_BITS_MASK: isize = 0x30;
const SCOPE_KIND_BITS: isize = 0x00;
const STACK_KIND_BITS: isize = 0x20;
const DIRECT_ARGUMENT_KIND_BITS: isize = 0x30;
const FLAG_BITS: u32 = 6;

fn var_offset_from_bits(bits: isize) -> VarOffset {
    let kind_bits = bits & KIND_BITS_MASK;
    let kind = if kind_bits == SCOPE_KIND_BITS {
        VarKind::Scope
    } else if kind_bits == STACK_KIND_BITS {
        VarKind::Stack
    } else {
        VarKind::DirectArgument
    };
    VarOffset::assemble(kind, (bits >> FLAG_BITS) as i32 as u32)
}

fn scope_offset_from_bits(bits: isize) -> ScopeOffset {
    debug_assert!((bits & KIND_BITS_MASK) == SCOPE_KIND_BITS);
    ScopeOffset::new((bits >> FLAG_BITS) as i32 as u32)
}

/// `SymbolTableEntry::Fast`: leitura rápida dos bits, sem consultar a forma gorda.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fast {
    bits: isize,
}

pub type SymbolTableEntryFast = Fast;

impl Default for Fast {
    fn default() -> Self {
        Fast { bits: SLIM_FLAG }
    }
}

impl Fast {
    pub fn is_null(&self) -> bool {
        (self.bits & !SLIM_FLAG) == 0
    }

    pub fn var_offset(&self) -> VarOffset {
        var_offset_from_bits(self.bits)
    }

    /// Falha (assert) se o deslocamento não for de escopo.
    pub fn scope_offset(&self) -> ScopeOffset {
        scope_offset_from_bits(self.bits)
    }

    pub fn is_read_only(&self) -> bool {
        (self.bits & READ_ONLY_FLAG) != 0
    }

    pub fn is_dont_enum(&self) -> bool {
        (self.bits & DONT_ENUM_FLAG) != 0
    }

    pub fn get_attributes(&self) -> u32 {
        let mut attributes = 0;
        if self.is_read_only() {
            attributes |= READ_ONLY;
        }
        if self.is_dont_enum() {
            attributes |= DONT_ENUM;
        }
        attributes
    }

    pub fn is_fat(&self) -> bool {
        (self.bits & SLIM_FLAG) == 0
    }
}

impl From<&SymbolTableEntry> for Fast {
    fn from(entry: &SymbolTableEntry) -> Fast {
        Fast { bits: entry.bits }
    }
}

/// `SymbolTableEntry`. Só movível no C++ (cópia apagada); aqui não implementa `Clone`.
#[derive(Debug, PartialEq, Eq)]
pub struct SymbolTableEntry {
    bits: isize,
}

impl Default for SymbolTableEntry {
    fn default() -> Self {
        SymbolTableEntry { bits: SLIM_FLAG }
    }
}

impl SymbolTableEntry {
    /// `SymbolTableEntry(VarOffset offset, unsigned attributes)`.
    pub fn new(offset: VarOffset, attributes: u32) -> Self {
        let mut entry = SymbolTableEntry { bits: SLIM_FLAG };
        debug_assert!(Self::is_valid_var_offset(offset));
        entry.pack(offset, (attributes & READ_ONLY) != 0, (attributes & DONT_ENUM) != 0);
        entry
    }

    /// `SymbolTableEntry(VarOffset offset)`.
    pub fn from_var_offset(offset: VarOffset) -> Self {
        let mut entry = SymbolTableEntry { bits: SLIM_FLAG };
        debug_assert!(Self::is_valid_var_offset(offset));
        entry.pack(offset, false, false);
        entry
    }

    pub fn swap(&mut self, other: &mut SymbolTableEntry) {
        std::mem::swap(&mut self.bits, &mut other.bits);
    }

    pub fn is_null(&self) -> bool {
        (self.bits & !SLIM_FLAG) == 0
    }

    pub fn var_offset(&self) -> VarOffset {
        var_offset_from_bits(self.bits)
    }

    /// `isWatchable()`: `Options::useJIT()` é falso, então nunca é observável.
    pub fn is_watchable(&self) -> bool {
        false
    }

    /// Falha (assert) se o deslocamento não for de escopo.
    pub fn scope_offset(&self) -> ScopeOffset {
        scope_offset_from_bits(self.bits)
    }

    pub fn get_fast(&self) -> Fast {
        Fast::from(self)
    }

    pub fn get_attributes(&self) -> u32 {
        self.get_fast().get_attributes()
    }

    pub fn set_read_only(&mut self) {
        self.bits |= READ_ONLY_FLAG;
    }

    pub fn is_read_only(&self) -> bool {
        (self.bits & READ_ONLY_FLAG) != 0
    }

    pub fn constant_mode(&self) -> ConstantMode {
        mode_for_is_constant(self.is_read_only())
    }

    pub fn is_dont_enum(&self) -> bool {
        (self.bits & DONT_ENUM_FLAG) != 0
    }

    /// `prepareToWatch()`: só infla se `isWatchable()`, que aqui é falso.
    pub fn prepare_to_watch(&mut self) {
        debug_assert!(!self.is_fat());
    }

    fn is_fat(&self) -> bool {
        (self.bits & SLIM_FLAG) == 0
    }

    fn pack(&mut self, offset: VarOffset, read_only: bool, dont_enum: bool) {
        debug_assert!(!self.is_fat());
        let mut bits: isize = ((offset.raw_offset() as isize) << FLAG_BITS) | NOT_NULL_FLAG | SLIM_FLAG;
        if read_only {
            bits |= READ_ONLY_FLAG;
        }
        if dont_enum {
            bits |= DONT_ENUM_FLAG;
        }
        match offset.kind() {
            VarKind::Scope => bits |= SCOPE_KIND_BITS,
            VarKind::Stack => bits |= STACK_KIND_BITS,
            VarKind::DirectArgument => bits |= DIRECT_ARGUMENT_KIND_BITS,
            VarKind::Invalid => panic!("SymbolTableEntry::pack: VarOffset inválido"),
        }
        self.bits = bits;
    }

    fn is_valid_var_offset(offset: VarOffset) -> bool {
        (((offset.raw_offset() as isize) << FLAG_BITS) >> FLAG_BITS) == offset.raw_offset() as isize
    }
}
