//! `src/ast/scope.rs` do upstream. Os nomes dos membros são `StoreStr` (faixas do fonte ou strings gravadas no
//! `Store`), então comparar um nome pede o `Store`: as buscas recebem `&Store`. Quem muda os membros de um escopo
//! guardado tira o `Members` do nó, muda com `&Store` e devolve (o padrão de "Escrita durante a visita" do README).

use crate::StrictModeKind;
use crate::base::Ref;
use crate::new_store::NodeStore;
use crate::nodes::{StoreRef, StoreStr};
use crate::store::Store;
use crate::symbol;
use crate::ts::TSNamespaceScope;

/// One binding of a scope in a `Members::Table`: its name, the name's `Members::hash`, and the symbol.
#[derive(Clone, Copy)]
pub struct MemberEntry {
    pub name: StoreStr,
    hash: u32,
    pub member: Member,
}

/// Up to `N` bindings, hashes first so that a lookup that misses (the common
/// case while resolving an identifier up the scope chain) reads one cache
/// line. Scopes start with `N = 4` (most have no more) and move to `N = 8`
/// before becoming a table.
#[derive(Clone)]
pub struct InlineMembers<const N: usize> {
    hashes: [u32; N],
    len: u32,
    members: [Member; N],
    names: [StoreStr; N],
}

impl<const N: usize> InlineMembers<N> {
    fn new() -> Self {
        InlineMembers {
            hashes: [0; N],
            len: 0,
            members: [Member::EMPTY; N],
            names: [StoreStr::EMPTY; N],
        }
    }

    /// Bit `i` set: entry `i` has this hash.
    #[inline]
    fn matches(&self, hash: u32) -> u32 {
        let mut mask = 0u32;
        for (i, h) in self.hashes.iter().enumerate() {
            mask |= u32::from(*h == hash) << i;
        }
        mask & ((1u32 << self.len) - 1)
    }

    #[inline]
    fn find(&self, store: &Store, hash: u32, name: &[u8]) -> Option<usize> {
        let mut mask = self.matches(hash);
        while mask != 0 {
            let i = mask.trailing_zeros() as usize;
            if store.str(self.names[i]) == name {
                return Some(i);
            }
            mask &= mask - 1;
        }
        None
    }

    /// Caller checks `len < N`.
    #[inline]
    fn push(&mut self, hash: u32, name: StoreStr) -> usize {
        let i = self.len as usize;
        self.hashes[i] = hash;
        self.names[i] = name;
        self.members[i] = Member::default();
        self.len += 1;
        i
    }

    fn grow<const M: usize>(&self) -> InlineMembers<M> {
        let mut out = InlineMembers::<M>::new();
        let n = self.len as usize;
        out.hashes[..n].copy_from_slice(&self.hashes[..n]);
        out.members[..n].copy_from_slice(&self.members[..n]);
        out.names[..n].copy_from_slice(&self.names[..n]);
        out.len = self.len;
        out
    }
}

/// A scope's bindings by name. Most scopes bind a handful of names, so they
/// start as a short inline array and only become a hash table past
/// `INLINE_MAX`. The table is the `hashbrown` of the upstream, same version and same hash, so iterating it gives
/// the upstream's order.
#[derive(Default, Clone)]
pub enum Members {
    #[default]
    Empty,
    /// In declaration order.
    Small(Box<InlineMembers<4>>),
    Inline(Box<InlineMembers<8>>),
    Table(hashbrown::HashTable<MemberEntry>),
}

pub struct MembersGetOrPut<'a> {
    pub found_existing: bool,
    pub value_ptr: &'a mut Member,
}

pub enum MembersIter<'a> {
    Inline(&'a [StoreStr], &'a [Member], usize),
    Table(hashbrown::hash_table::Iter<'a, MemberEntry>),
}

impl<'a> Iterator for MembersIter<'a> {
    type Item = (StoreStr, &'a Member);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match self {
            MembersIter::Inline(names, members, i) => {
                let item = (*names.get(*i)?, &members[*i]);
                *i += 1;
                Some(item)
            }
            MembersIter::Table(it) => it.next().map(|e| (e.name, &e.member)),
        }
    }
}

impl Members {
    pub const EMPTY: Members = Members::Empty;
    pub const INLINE_MAX: usize = 8;

    /// The low 32 bits of wyhash. A match is always confirmed by comparing
    /// the names. `Table` feeds `hashbrown` the value widened, since it takes
    /// its tag from the top bits.
    #[inline]
    pub fn hash(name: &[u8]) -> u32 {
        bun_wyhash::hash(name) as u32
    }

    #[inline]
    fn table_hash(hash: u32) -> u64 {
        (u64::from(hash) << 32) | u64::from(hash)
    }

    #[inline]
    pub fn len(&self) -> usize {
        match self {
            Members::Empty => 0,
            Members::Small(inline) => inline.len as usize,
            Members::Inline(inline) => inline.len as usize,
            Members::Table(table) => table.len(),
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn iter(&self) -> MembersIter<'_> {
        match self {
            Members::Empty => MembersIter::Inline(&[], &[], 0),
            Members::Small(m) => MembersIter::Inline(&m.names[..m.len as usize], &m.members[..m.len as usize], 0),
            Members::Inline(m) => MembersIter::Inline(&m.names[..m.len as usize], &m.members[..m.len as usize], 0),
            Members::Table(table) => MembersIter::Table(table.iter()),
        }
    }

    pub fn values(&self) -> impl Iterator<Item = &Member> + '_ {
        self.iter().map(|(_, member)| member)
    }

    #[inline]
    pub fn get(&self, store: &Store, name: &[u8]) -> Option<&Member> {
        self.get_hashed(store, Self::hash(name), name)
    }

    #[inline]
    pub fn contains_key(&self, store: &Store, name: &[u8]) -> bool {
        self.get(store, name).is_some()
    }

    #[inline]
    pub fn get_hashed(&self, store: &Store, hash: u32, name: &[u8]) -> Option<&Member> {
        match self {
            Members::Empty => None,
            Members::Small(inline) => inline.find(store, hash, name).map(|i| &inline.members[i]),
            Members::Inline(inline) => inline.find(store, hash, name).map(|i| &inline.members[i]),
            Members::Table(table) => table
                .find(Self::table_hash(hash), |e| e.hash == hash && store.str(e.name) == name)
                .map(|e| &e.member),
        }
    }

    /// Room for about `n` members (the module scope, sized from the source).
    pub fn reserve(&mut self, n: usize) {
        if n > Self::INLINE_MAX {
            self.to_table(n);
        }
    }

    fn to_table(&mut self, capacity: usize) {
        fn fill<const N: usize>(inline: &InlineMembers<N>, capacity: usize) -> hashbrown::HashTable<MemberEntry> {
            let mut table = hashbrown::HashTable::with_capacity(capacity.max(Members::INLINE_MAX * 2));
            for i in 0..inline.len as usize {
                let entry = MemberEntry {
                    name: inline.names[i],
                    hash: inline.hashes[i],
                    member: inline.members[i],
                };
                table.insert_unique(Members::table_hash(entry.hash), entry, |e| Members::table_hash(e.hash));
            }
            table
        }
        let table = match core::mem::take(self) {
            Members::Table(mut table) => {
                table.reserve(capacity, |e| Self::table_hash(e.hash));
                table
            }
            Members::Empty => hashbrown::HashTable::with_capacity(capacity),
            Members::Small(inline) => fill(&inline, capacity),
            Members::Inline(inline) => fill(&inline, capacity),
        };
        *self = Members::Table(table);
    }

    /// The entry for `name`, inserted with `Member::default()` if absent. No upstream o nome é guardado por
    /// referência e a função é `unsafe` (o nome tem de viver mais que o escopo); aqui ele já é uma faixa do `Store`.
    pub fn get_or_put_hashed(&mut self, store: &Store, hash: u32, name: StoreStr) -> MembersGetOrPut<'_> {
        let text = store.str(name);
        loop {
            match self {
                Members::Empty => {
                    *self = Members::Small(Box::new(InlineMembers::new()));
                }
                Members::Small(inline) => {
                    let found = inline.find(store, hash, text);
                    if found.is_some() || (inline.len as usize) < 4 {
                        let Members::Small(inline) = self else { unreachable!() };
                        let i = match found {
                            Some(i) => i,
                            None => inline.push(hash, name),
                        };
                        return MembersGetOrPut { found_existing: found.is_some(), value_ptr: &mut inline.members[i] };
                    }
                    let grown = inline.grow::<8>();
                    *self = Members::Inline(Box::new(grown));
                }
                Members::Inline(inline) => {
                    let found = inline.find(store, hash, text);
                    if found.is_some() || (inline.len as usize) < Self::INLINE_MAX {
                        let Members::Inline(inline) = self else { unreachable!() };
                        let i = match found {
                            Some(i) => i,
                            None => inline.push(hash, name),
                        };
                        return MembersGetOrPut { found_existing: found.is_some(), value_ptr: &mut inline.members[i] };
                    }
                    self.to_table(Self::INLINE_MAX * 2);
                }
                Members::Table(table) => {
                    return match table.entry(
                        Self::table_hash(hash),
                        |e| e.hash == hash && store.str(e.name) == text,
                        |e| Self::table_hash(e.hash),
                    ) {
                        hashbrown::hash_table::Entry::Occupied(o) => {
                            MembersGetOrPut { found_existing: true, value_ptr: &mut o.into_mut().member }
                        }
                        hashbrown::hash_table::Entry::Vacant(v) => MembersGetOrPut {
                            found_existing: false,
                            value_ptr: &mut v.insert(MemberEntry { name, hash, member: Member::default() }).into_mut().member,
                        },
                    };
                }
            }
        }
    }

    /// Insert or overwrite.
    pub fn put(&mut self, store: &Store, name: StoreStr, member: Member) {
        let hash = Self::hash(store.str(name));
        *self.get_or_put_hashed(store, hash, name).value_ptr = member;
    }
}

#[derive(Clone)]
pub struct Scope {
    pub kind: Kind,
    /// O escopo pai, dono deste pela lista `children`.
    pub parent: Option<StoreRef<Scope>>,
    pub children: Vec<StoreRef<Scope>>,
    pub members: Members,
    pub generated: Vec<Ref>,
    /// This scope's index in the visit pass's pre-order walk and the index of
    /// the last scope inside it, which `Ast::scope_uses` refers to.
    /// `u32::MAX` for a scope the visit pass never entered.
    pub visit_span: [u32; 2],

    // This is used to store the ref of the label symbol for ScopeLabel scopes.
    pub label_ref: Ref,
    pub label_stmt_is_loop: bool,

    // If a scope contains a direct eval() expression, then none of the symbols
    // inside that scope can be renamed. We conservatively assume that the
    // evaluated code might reference anything that it has access to.
    pub contains_direct_eval: bool,

    // This is to help forbid "arguments" inside class body scopes
    pub forbid_arguments: bool,

    pub strict_mode: StrictModeKind,

    pub is_after_const_local_prefix: bool,

    // This will be non-null if this is a TypeScript "namespace" or "enum"
    pub ts_namespace: Option<StoreRef<TSNamespaceScope>>,
}

impl Scope {
    /// All-empty `Scope` as a `const`, for struct-update syntax in the parser's per-scope allocation.
    pub const EMPTY: Self = Self {
        kind: Kind::Block,
        parent: None,
        children: Vec::new(),
        members: Members::EMPTY,
        generated: Vec::new(),
        visit_span: [u32::MAX, u32::MAX],
        label_ref: Ref::NONE,
        label_stmt_is_loop: false,
        contains_direct_eval: false,
        forbid_arguments: false,
        strict_mode: StrictModeKind::SloppyMode,
        is_after_const_local_prefix: false,
        ts_namespace: None,
    };

    /// `(first, last)`: this scope and everything inside it, as visit-pass
    /// pre-order indices. `None` if the visit pass did not enter and leave it.
    pub fn visit_span(&self) -> Option<(u32, u32)> {
        let [first, last] = self.visit_span;
        (first != u32::MAX && first <= last).then_some((first, last))
    }

    #[inline]
    pub fn get_member_with_hash(&self, store: &Store, name: &[u8], hash_value: u32) -> Option<Member> {
        self.members.get_hashed(store, hash_value, name).copied()
    }

    /// Associated-fn form of `can_merge_symbols` taking the scope's [`Kind`] by value.
    pub fn can_merge_symbol_kinds<const IS_TYPESCRIPT_ENABLED: bool>(
        scope_kind: Kind,
        existing: symbol::Kind,
        new: symbol::Kind,
    ) -> SymbolMergeResult {
        use symbol::Kind as Sk;

        if existing == Sk::Unbound {
            return SymbolMergeResult::ReplaceWithNew;
        }

        if IS_TYPESCRIPT_ENABLED {
            // In TypeScript, imports are allowed to silently collide with symbols within
            // the module. Presumably this is because the imports may be type-only:
            //
            //   import {Foo} from 'bar'
            //   class Foo {}
            //
            if existing == Sk::Import {
                return SymbolMergeResult::ReplaceWithNew;
            }

            // "enum Foo {} enum Foo {}"
            // "namespace Foo { ... } enum Foo {}"
            if new == Sk::TsEnum && (existing == Sk::TsEnum || existing == Sk::TsNamespace) {
                return SymbolMergeResult::ReplaceWithNew;
            }

            // "namespace Foo { ... } namespace Foo { ... }"
            // "function Foo() {} namespace Foo { ... }"
            // "enum Foo {} namespace Foo { ... }"
            if new == Sk::TsNamespace {
                match existing {
                    Sk::TsNamespace | Sk::TsEnum | Sk::HoistedFunction | Sk::GeneratorOrAsyncFunction | Sk::Class => {
                        return SymbolMergeResult::KeepExisting;
                    }
                    _ => {}
                }
            }
        }

        // "var foo; var foo;"
        // "var foo; function foo() {}"
        // "function foo() {} var foo;"
        // "function *foo() {} function *foo() {}" but not "{ function *foo() {} function *foo() {} }"
        if new.is_hoisted_or_function()
            && existing.is_hoisted_or_function()
            && (scope_kind == Kind::Entry
                || scope_kind == Kind::FunctionBody
                || scope_kind == Kind::FunctionArgs
                || (new == existing && existing.is_hoisted()))
        {
            return SymbolMergeResult::ReplaceWithNew;
        }

        // "get #foo() {} set #foo() {}"
        // "set #foo() {} get #foo() {}"
        if (existing == Sk::PrivateGet && new == Sk::PrivateSet) || (existing == Sk::PrivateSet && new == Sk::PrivateGet) {
            return SymbolMergeResult::BecomePrivateGetSetPair;
        }
        if (existing == Sk::PrivateStaticGet && new == Sk::PrivateStaticSet)
            || (existing == Sk::PrivateStaticSet && new == Sk::PrivateStaticGet)
        {
            return SymbolMergeResult::BecomePrivateStaticGetSetPair;
        }

        // "try {} catch (e) { var e }"
        if existing == Sk::CatchIdentifier && new == Sk::Hoisted {
            return SymbolMergeResult::ReplaceWithNew;
        }

        // "function() { var arguments }"
        if existing == Sk::Arguments && new == Sk::Hoisted {
            return SymbolMergeResult::KeepExisting;
        }

        // "function() { let arguments }"
        if existing == Sk::Arguments && new != Sk::Hoisted {
            return SymbolMergeResult::OverwriteWithNew;
        }

        SymbolMergeResult::Forbidden
    }

    /// No upstream é método recursivo sobre `&mut self`; com os filhos no `Store`, recebe o `Store` e o índice.
    pub fn recursive_set_strict_mode(store: &mut Store, scope: StoreRef<Scope>, kind: StrictModeKind) {
        if store[scope].strict_mode == StrictModeKind::SloppyMode {
            store[scope].strict_mode = kind;
            let children = core::mem::take(&mut store[scope].children);
            for &child in &children {
                Scope::recursive_set_strict_mode(store, child, kind);
            }
            store[scope].children = children;
        }
    }

    #[inline]
    pub fn kind_stops_hoisting(&self) -> bool {
        self.kind as u8 >= Kind::Entry as u8
    }
}

impl Default for Scope {
    #[inline]
    fn default() -> Self {
        Self::EMPTY
    }
}

// Do not make this a packed struct
// Two hours of debugging time lost to that.
// It causes a crash due to undefined memory
#[derive(Clone, Copy, Default)]
pub struct Member {
    pub ref_: Ref,
    pub loc: crate::Loc,
}

impl Member {
    pub const EMPTY: Member = Member { ref_: Ref::NONE, loc: crate::Loc::EMPTY };
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SymbolMergeResult {
    Forbidden,
    ReplaceWithNew,
    OverwriteWithNew,
    KeepExisting,
    BecomePrivateGetSetPair,
    BecomePrivateStaticGetSetPair,
}

#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, strum::IntoStaticStr)]
#[strum(serialize_all = "snake_case")]
pub enum Kind {
    Block,
    With,
    Label,
    ClassName,
    ClassBody,
    CatchBinding,

    // The scopes below stop hoisted variables from extending into parent scopes
    Entry, // This is a module, TypeScript enum, or TypeScript namespace
    FunctionArgs,
    FunctionBody,
    ClassStaticInit,
}
