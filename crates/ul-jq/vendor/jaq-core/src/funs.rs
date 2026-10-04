use crate::box_iter::BoxIter;
use crate::native::{bome, v, Filter, RunPathsPtr};
use crate::{Bind, DataT, Error, Exn, RunPtr, ValT};
use alloc::boxed::Box;

pub fn run<D: DataT>() -> Box<[Filter<RunPtr<D>>]>
where
    for<'a> D::V<'a>: ValT,
{
    let f = || [Bind::Fun(())].into();
    // Porte pseudo-linus: só `error_empty` e `path` (com o erro do jq para resultado fora do
    // caminho). `range`, `keys_unsorted` e as nativas que o jq 1.7.1 não tem saíram; `range` com a
    // semântica do jq fica no jaq-json.
    Box::new([
        ("error_empty", v(0), (|cv| bome(Err(Error::new(cv.1))))),
        ("path", f(), |mut cv| {
            let (f, fc) = cv.0.pop_fun();
            let cvp = (fc, (cv.1, Some(Default::default())));
            Box::new(f.paths(cvp).map(|vp| {
                let (v, path) = vp?;
                match path {
                    Some(path) => Ok(crate::filter::path_vec(&path).into_iter().collect()),
                    None => Err(Exn::from(Error::path_result(&v))),
                }
            }))
        }),
    ])
}

fn once_or_empty<'a, T: 'a, E: 'a>(r: Result<Option<T>, E>) -> BoxIter<'a, Result<T, E>> {
    Box::new(r.transpose().into_iter())
}

macro_rules! first {
    ( $run:ident ) => {
        |mut cv| {
            let (f, fc) = cv.0.pop_fun();
            Box::new(f.$run((fc, cv.1)).next().into_iter())
        }
    };
}
/// Porte pseudo-linus: `def last(g): reduce g as $item (null; $item);` do jq 1.7.1 (sem saída, dá
/// `null`).
macro_rules! last {
    ( $run:ident, $null:expr ) => {
        |mut cv| {
            let (f, fc) = cv.0.pop_fun();
            let last = f.$run((fc, cv.1)).try_fold(None, |_, x| x.map(Some));
            once_or_empty(last.map(|x| Some(x.unwrap_or_else($null))))
        }
    };
}

/// Porte pseudo-linus: o `limit` do jq 1.7.1, com a semântica da definição dele (inclusive `$n`
/// negativo, que devolve tudo):
///
/// ~~~ text
/// def limit($n; exp):
///     if $n > 0 then label $out | foreach exp as $item ($n; .-1; $item, if . <= 0 then break $out else empty end)
///     elif $n == 0 then empty
///     else exp end;
/// ~~~
///
/// É nativo para que `path(limit(...))` funcione: no jaq, variável não carrega caminho.
macro_rules! limit {
    ( $run:ident ) => {
        |mut cv| {
            let ((f, fc), n) = (cv.0.pop_fun(), cv.0.pop_var());
            let zero = D::V::from(0isize);
            if n > zero {
                let mut iter = f.$run((fc, cv.1));
                let mut state = Some(n);
                Box::new(core::iter::from_fn(move || {
                    let s = state.take()?;
                    match iter.next()? {
                        Err(e) => Some(Err(e)),
                        Ok(x) => match s - D::V::from(1isize) {
                            Err(e) => Some(Err(Exn::from(e))),
                            Ok(next) => {
                                if next > zero {
                                    state = Some(next);
                                }
                                Some(Ok(x))
                            }
                        },
                    }
                }))
            } else if n == zero {
                Box::new(core::iter::empty())
            } else {
                f.$run((fc, cv.1))
            }
        }
    };
}

pub fn paths<D: DataT>() -> Box<[Filter<RunPathsPtr<D>>]>
where
    for<'a> D::V<'a>: ValT,
{
    let f = || [Bind::Fun(())].into();
    let vf = || [Bind::Var(()), Bind::Fun(())].into();
    // Porte pseudo-linus: sem `skip` (o jq 1.7.1 não tem).
    Box::new([
        ("first", f(), (first!(run), first!(paths))),
        ("last", f(), (last!(run, D::V::null), last!(paths, || (D::V::null(), None)))),
        ("limit", vf(), (limit!(run), limit!(paths))),
    ])
}
