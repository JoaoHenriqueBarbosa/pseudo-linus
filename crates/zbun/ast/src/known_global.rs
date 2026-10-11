//! `src/ast/known_global.rs` do upstream. No porte, o `E::New` é um índice no `Store`, que entra como parâmetro no
//! lugar do `&mut E::New` e da arena; o resto (a tabela, as regras de cada construtor) é o do upstream.

use crate as js_ast;
use crate::E;
use crate::Symbol;
use crate::nodes::StoreRef;
use crate::{NodeStore, Store};

#[derive(Copy, Clone, PartialEq, Eq, Hash, strum::IntoStaticStr)]
pub enum KnownGlobal {
    WeakSet,
    WeakMap,
    Date,
    Set,
    Map,
    Headers,
    Response,
    TextEncoder,
    TextDecoder,
    Error,
    TypeError,
    SyntaxError,
    RangeError,
    ReferenceError,
    EvalError,
    URIError,
    AggregateError,
    Array,
    Object,
    Function,
    RegExp,
}

bun_core::comptime_string_map! {
    /// PERF: `minify_global_constructor` probes this for every `new Ident` expression and the
    /// overwhelming majority of probes are misses (any user-defined class); the macro's length
    /// dispatch rejects those on a single `usize` compare, with no hashing.
    static KNOWN_GLOBALS: KnownGlobal = {
        b"WeakSet" => KnownGlobal::WeakSet,
        b"WeakMap" => KnownGlobal::WeakMap,
        b"Date" => KnownGlobal::Date,
        b"Set" => KnownGlobal::Set,
        b"Map" => KnownGlobal::Map,
        b"Headers" => KnownGlobal::Headers,
        b"Response" => KnownGlobal::Response,
        b"TextEncoder" => KnownGlobal::TextEncoder,
        b"TextDecoder" => KnownGlobal::TextDecoder,
        b"Error" => KnownGlobal::Error,
        b"TypeError" => KnownGlobal::TypeError,
        b"SyntaxError" => KnownGlobal::SyntaxError,
        b"RangeError" => KnownGlobal::RangeError,
        b"ReferenceError" => KnownGlobal::ReferenceError,
        b"EvalError" => KnownGlobal::EvalError,
        b"URIError" => KnownGlobal::URIError,
        b"AggregateError" => KnownGlobal::AggregateError,
        b"Array" => KnownGlobal::Array,
        b"Object" => KnownGlobal::Object,
        b"Function" => KnownGlobal::Function,
        b"RegExp" => KnownGlobal::RegExp,
    };
}

#[inline]
fn lookup(name: &[u8]) -> Option<KnownGlobal> {
    KNOWN_GLOBALS.get(name).copied()
}

impl KnownGlobal {
    #[inline(always)]
    fn call_from_new(store: &mut Store, e: StoreRef<E::New>, loc: crate::Loc) -> js_ast::Expr {
        let new = &mut store[e];
        let call = E::Call {
            target: new.target,
            args: core::mem::take(&mut new.args),
            close_paren_loc: new.close_parens_loc,
            can_be_unwrapped_if_unused: new.can_be_unwrapped_if_unused,
            ..Default::default()
        };
        js_ast::Expr::init(store, call, loc)
    }

    /// `new X(...)` sem o `new`: os itens saem do `E::New` (o `AstAlloc::take` do upstream).
    fn array_from_args(store: &mut Store, e: StoreRef<E::New>, loc: crate::Loc) -> js_ast::Expr {
        let items = core::mem::take(&mut store[e].args);
        js_ast::Expr::init(store, E::Array { items, ..Default::default() }, loc)
    }

    #[inline(never)]
    pub fn minify_global_constructor(
        store: &mut Store,
        e: StoreRef<E::New>,
        symbols: &[Symbol],
        loc: crate::Loc,
        minify_whitespace: bool,
    ) -> Option<js_ast::Expr> {
        let id = if let js_ast::ExprData::EIdentifier(ident) = store[e].target.data {
            ident.ref_
        } else {
            return None;
        };
        let symbol = &symbols[id.inner_index() as usize];
        if symbol.kind != js_ast::symbol::Kind::Unbound {
            return None;
        }

        let constructor = lookup(store.str(symbol.original_name))?;
        let args = store[e].args;
        let n = args.len();
        let first = (n >= 1).then(|| store[args][0]);

        match constructor {
            // Error constructors can be called without 'new' with identical behavior
            KnownGlobal::Error
            | KnownGlobal::TypeError
            | KnownGlobal::SyntaxError
            | KnownGlobal::RangeError
            | KnownGlobal::ReferenceError
            | KnownGlobal::EvalError
            | KnownGlobal::URIError
            | KnownGlobal::AggregateError => {
                // Convert `new Error(...)` to `Error(...)` to save bytes
                Some(Self::call_from_new(store, e, loc))
            }

            KnownGlobal::Object => {
                if n == 0 {
                    // new Object() -> {}
                    return Some(js_ast::Expr::init(store, E::Object::default(), loc));
                }

                if let (1, Some(arg)) = (n, first) {
                    match arg.data {
                        js_ast::ExprData::EObject(_) | js_ast::ExprData::EArray(_) => {
                            // new Object({a: 1}) -> {a: 1}
                            // new Object([1, 2]) -> [1, 2]
                            return Some(arg);
                        }
                        js_ast::ExprData::ENull(_) | js_ast::ExprData::EUndefined(_) => {
                            // new Object(null) -> {}
                            // new Object(undefined) -> {}
                            return Some(js_ast::Expr::init(store, E::Object::default(), loc));
                        }
                        _ => {}
                    }
                }

                // For other cases, just remove 'new'
                Some(Self::call_from_new(store, e, loc))
            }

            KnownGlobal::Array => match (n, first) {
                // new Array() -> []
                (0, _) => Some(js_ast::Expr::init(store, E::Array::default(), loc)),
                (1, Some(arg)) => {
                    // For single argument, only convert to literal if we're SURE it's not a number
                    if let js_ast::ExprData::EObject(_) | js_ast::ExprData::EArray(_) = arg.data {
                        // new Array({}) -> [{}], new Array([1]) -> [[1]]
                        // These are definitely not numbers, safe to convert
                        return Some(Self::array_from_args(store, e, loc));
                    }

                    // For other types, check via knownPrimitive
                    // Only convert if we know for certain it's not a number
                    // unknown could be a number at runtime, so we must preserve Array() call
                    match arg.known_primitive(store) {
                        js_ast::expr::PrimitiveType::Null
                        | js_ast::expr::PrimitiveType::Undefined
                        | js_ast::expr::PrimitiveType::Boolean
                        | js_ast::expr::PrimitiveType::String
                        | js_ast::expr::PrimitiveType::Bigint => Some(Self::array_from_args(store, e, loc)),
                        js_ast::expr::PrimitiveType::Number => {
                            let val = match arg.data {
                                js_ast::ExprData::ENumber(num) => num.value(),
                                _ => return Some(Self::call_from_new(store, e, loc)),
                            };
                            // only want this with whitespace minification
                            if minify_whitespace && (0.0..=10.0).contains(&val) && val.fract() == 0.0 {
                                let hole = js_ast::Expr { data: js_ast::ExprData::EMissing(E::Missing {}), loc: arg.loc };
                                let items = store.alloc_slice((0..val as usize).map(|_| hole));
                                return Some(js_ast::Expr::init(store, E::Array { items, ..Default::default() }, loc));
                            }
                            Some(Self::call_from_new(store, e, loc))
                        }
                        js_ast::expr::PrimitiveType::Unknown | js_ast::expr::PrimitiveType::Mixed => {
                            // Could be a number, preserve Array() call
                            Some(Self::call_from_new(store, e, loc))
                        }
                    }
                }
                // new Array(1, 2, 3) -> [1, 2, 3]
                // But NOT new Array(3) which creates an array with 3 empty slots
                _ => Some(Self::array_from_args(store, e, loc)),
            },

            KnownGlobal::Function => {
                // Just remove 'new' for Function
                Some(Self::call_from_new(store, e, loc))
            }
            KnownGlobal::RegExp => {
                // Don't optimize RegExp - the semantics are too complex:
                // - new RegExp(re) creates a copy, but RegExp(re) returns the same instance
                // - This affects object identity and lastIndex behavior
                // - The difference only applies when flags are undefined
                // Keep the original new RegExp() call to preserve correct semantics
                None
            }
            KnownGlobal::WeakSet | KnownGlobal::WeakMap => {
                let pure = match (n, first.map(|arg| arg.data)) {
                    // "new WeakSet()" is pure
                    (0, _) => true,
                    // "new WeakSet(null)" is pure
                    // "new WeakSet(void 0)" is pure
                    (1, Some(js_ast::ExprData::ENull(_) | js_ast::ExprData::EUndefined(_))) => true,
                    // "new WeakSet([])" is pure; "new WeakSet([x])" is impure because an exception is thrown if
                    // "x" is not an object
                    (1, Some(js_ast::ExprData::EArray(array))) => store[array].items.is_empty(),
                    // "new WeakSet(x)" is impure because the iterator for "x" could have side effects
                    _ => false,
                };
                Self::mark_pure(store, e, pure)
            }
            KnownGlobal::Date | KnownGlobal::Response => {
                // "new Date()" / "new Response()" is pure; with one argument of a known primitive type
                // ('', 0, null, true, false, undefined) too. "new Date(x)" is impure because the argument could be a
                // string with side effects.
                let pure = match (n, first) {
                    (0, _) => true,
                    (1, Some(arg)) => match arg.known_primitive(store) {
                        js_ast::expr::PrimitiveType::Null
                        | js_ast::expr::PrimitiveType::Undefined
                        | js_ast::expr::PrimitiveType::Boolean
                        | js_ast::expr::PrimitiveType::Number
                        | js_ast::expr::PrimitiveType::String => true,
                        _ => false,
                    },
                    _ => false,
                };
                Self::mark_pure(store, e, pure)
            }

            KnownGlobal::Set => {
                let pure = match (n, first.map(|arg| arg.data)) {
                    // "new Set()" is pure
                    (0, _) => true,
                    // "new Set([a, b, c])" is pure
                    // "new Set(null)" is pure
                    // "new Set(void 0)" is pure
                    (
                        1,
                        Some(js_ast::ExprData::EArray(_) | js_ast::ExprData::ENull(_) | js_ast::ExprData::EUndefined(_)),
                    ) => true,
                    // "new Set(x)" is impure because the iterator for "x" could have side effects
                    _ => false,
                };
                Self::mark_pure(store, e, pure)
            }

            // "new Headers()", "new TextEncoder()" and "new TextDecoder()" are pure. We _could_ validate the
            // encoding argument, but let's not bother.
            KnownGlobal::Headers | KnownGlobal::TextDecoder | KnownGlobal::TextEncoder => {
                Self::mark_pure(store, e, n == 0)
            }

            KnownGlobal::Map => {
                let pure = match (n, first.map(|arg| arg.data)) {
                    // "new Map()" is pure
                    (0, _) => true,
                    // "new Map(null)" is pure
                    // "new Map(void 0)" is pure
                    (1, Some(js_ast::ExprData::ENull(_) | js_ast::ExprData::EUndefined(_))) => true,
                    // "new Map([[a, b], [c, d]])" is pure
                    (1, Some(js_ast::ExprData::EArray(array))) => {
                        let items = store[array].items;
                        store[items].iter().all(|item| matches!(item.data, js_ast::ExprData::EArray(_)))
                    }
                    // "new Map(x)" is impure because the iterator for "x" could have side effects
                    _ => false,
                };
                Self::mark_pure(store, e, pure)
            }
        }
    }

    /// The `e.can_be_unwrapped_if_unused = .IfUnused` of each pure case; the expression itself stays.
    fn mark_pure(store: &mut Store, e: StoreRef<E::New>, pure: bool) -> Option<js_ast::Expr> {
        if pure {
            store[e].can_be_unwrapped_if_unused = js_ast::CanBeUnwrapped::IfUnused;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lookup_exhaustive() {
        // Round-trip every variant through its tag name.
        for v in [
            KnownGlobal::WeakSet,
            KnownGlobal::WeakMap,
            KnownGlobal::Date,
            KnownGlobal::Set,
            KnownGlobal::Map,
            KnownGlobal::Headers,
            KnownGlobal::Response,
            KnownGlobal::TextEncoder,
            KnownGlobal::TextDecoder,
            KnownGlobal::Error,
            KnownGlobal::TypeError,
            KnownGlobal::SyntaxError,
            KnownGlobal::RangeError,
            KnownGlobal::ReferenceError,
            KnownGlobal::EvalError,
            KnownGlobal::URIError,
            KnownGlobal::AggregateError,
            KnownGlobal::Array,
            KnownGlobal::Object,
            KnownGlobal::Function,
            KnownGlobal::RegExp,
        ] {
            let name: &'static str = v.into();
            assert!(lookup(name.as_bytes()) == Some(v), "{}", name);
        }
        // Misses at every interesting length, including a length with no bucket.
        for miss in [
            b"".as_slice(),
            b"Se",
            b"Sat",
            b"Math",
            b"Arrow",
            b"String",
            b"Promise",
            b"Function_",
            b"TypeErrorr",
            b"SyntaxErrro",
            b"SyntaxErrorX",
            b"ReferenceErrro",
            b"AggregateErrorX",
        ] {
            assert!(lookup(miss).is_none(), "{:?}", bstr::BStr::new(miss));
        }
    }
}
