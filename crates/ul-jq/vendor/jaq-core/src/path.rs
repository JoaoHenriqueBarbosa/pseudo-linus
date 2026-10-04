//! Paths and their parts.

use crate::box_iter::box_once;
use crate::val::{ValRs, ValT};
use crate::RcList;
use alloc::{boxed::Box, vec::Vec};

/// Path such as `.[].a?[1:]`.
#[derive(Clone, Debug)]
pub struct Path<F>(pub Vec<(Part<F>, Opt)>);

/// Part of a path, such as `[]`, `a`, and `[1:]` in `.[].a?[1:]`.
#[derive(Clone, Debug)]
pub enum Part<I> {
    /// Access arrays with integer and objects with string indices
    Index(I),
    /// Iterate over arrays with optional range bounds and over objects without bounds
    /// If both are `None`, return iterator over whole array/object
    Range(Option<I>, Option<I>),
}

/// Optionality of a path part, i.e. whether `?` is present.
///
/// For example, `[] | .a` fails with an error, while `[] | .a?` returns nothing.
/// By default, path parts are *essential*, meaning that they fail.
/// Annotating them with `?` makes them *optional*.
#[derive(Copy, Clone, Debug)]
pub enum Opt {
    /// Return nothing if the input cannot be accessed with the path
    Optional,
    /// Fail if the input cannot be accessed with the path
    Essential,
}

impl<I> Default for Part<I> {
    fn default() -> Self {
        Self::Range(None, None)
    }
}

impl Opt {
    /// If `self` is optional, return `x`, else fail with `f(x)`.
    pub fn fail<T, E>(self, x: T, f: impl FnOnce(T) -> E) -> Result<T, E> {
        match self {
            Self::Optional => Ok(x),
            Self::Essential => Err(f(x)),
        }
    }
}

/// Porte pseudo-linus: caminho rastreado de um valor em `path(...)`. `None` quando o valor não veio
/// de operações de caminho (o "result" das mensagens do jq). As combinações de chaves de um caminho
/// (`explode`) saíram: a ordem do jq é montada no avaliador (`path_term`).
pub(crate) type Tracked<V> = Option<RcList<V>>;

impl<'a, V: ValT + 'a> Part<V> {
    fn run(&self, v: V) -> ValRs<'a, V> {
        match self {
            Self::Index(idx) => box_once(v.index(idx)),
            Self::Range(None, None) => Box::new(v.values()),
            Self::Range(from, upto) => box_once(v.range(from.as_ref()..upto.as_ref())),
        }
    }

    /// Porte pseudo-linus: aplica a parte com a opcionalidade (`?` descarta os erros).
    pub(crate) fn run_opt(self, opt: Opt, v: V) -> ValRs<'a, V> {
        let ys = self.run(v);
        match opt {
            Opt::Essential => ys,
            Opt::Optional => Box::new(ys.filter(Result::is_ok)),
        }
    }

    /// Porte pseudo-linus: como [`Self::run_opt`], com caminho. Um valor fora do caminho dá erro
    /// mesmo com `?`.
    pub(crate) fn paths_opt(self, opt: Opt, vp: (V, Tracked<V>)) -> ValRs<'a, (V, Tracked<V>), V> {
        if vp.1.is_none() {
            return box_once(Err(self.invalid(&vp.0)));
        }
        let ys = self.paths(vp);
        match opt {
            Opt::Essential => ys,
            Opt::Optional => Box::new(ys.filter(Result::is_ok)),
        }
    }

    /// Porte pseudo-linus: erro do jq para esta parte aplicada a um valor fora do caminho.
    fn invalid(&self, v: &V) -> crate::Error<V> {
        match self {
            Self::Index(idx) => crate::Error::path_index(v, idx),
            Self::Range(None, None) => crate::Error::path_iter(v),
            Self::Range(from, upto) => crate::Error::path_index(v, &V::from(from.clone()..upto.clone())),
        }
    }

    fn paths(&self, (v, p): (V, Tracked<V>)) -> ValRs<'a, (V, Tracked<V>), V> {
        let Some(p) = p else {
            return box_once(Err(self.invalid(&v)));
        };
        match self {
            Self::Index(idx) => box_once(v.index(idx).map(|v| (v, Some(p.cons(idx.clone()))))),
            Self::Range(None, None) => Box::new(
                v.key_values()
                    .map(move |kv| kv.map(|(k, v)| (v, Some(p.clone().cons(k))))),
            ),
            Self::Range(from, upto) => box_once(
                v.range(from.as_ref()..upto.as_ref())
                    .map(|v| (v, Some(p.cons(V::from(from.clone()..upto.clone()))))),
            ),
        }
    }
}

impl<T> Part<T> {
    /// Apply a function to the contained indices.
    pub(crate) fn map<U, F: FnMut(T) -> U>(self, mut f: F) -> Part<U> {
        use Part::{Index, Range};
        match self {
            Index(i) => Index(f(i)),
            Range(from, upto) => Range(from.map(&mut f), upto.map(&mut f)),
        }
    }
}

impl<F> From<Part<F>> for Path<F> {
    fn from(p: Part<F>) -> Self {
        Self(Vec::from([(p, Opt::Essential)]))
    }
}
