//! Filter execution.

use crate::box_iter::{self, box_once, flat_map_then, flat_map_then_with, flat_map_with, map_with};
use crate::compile::{AltPattern, Bind, CallType, Fold, Pattern, Term as Ast, TermId as Id};
use crate::data::{DataT, HasLut};
use crate::path::Tracked;
use crate::val::{ValR, ValT, ValX, ValXs};
use crate::{exn, rc_lazy_list, Bind as Arg, Error, Exn, RcList};
use alloc::boxed::Box;
use alloc::vec::Vec;

/// Combination of context and input value.
pub type Cv<'a, D, T = <D as DataT>::V<'a>> = (Ctx<'a, D>, T);
/// Combination of context and input value with a path.
///
/// Porte pseudo-linus: o caminho é `None` quando o valor não veio de operações de caminho.
type Cvp<'a, D> = Cv<'a, D, (<D as DataT>::V<'a>, Tracked<<D as DataT>::V<'a>>)>;
type ValPathXs<'a, V> = ValXs<'a, (V, Tracked<V>), V>;

type Lut<D> = crate::compile::Lut<Native<D>>;

/// Porte pseudo-linus: margem e tamanho dos segmentos de pilha que o avaliador pede ao `stacker`
/// quando a recursão (do programa ou dos dados) aprofunda.
const STACK_RED_ZONE: usize = 256 * 1024;
const STACK_SEGMENT: usize = 4 * 1024 * 1024;

/// Porte pseudo-linus: profundidade máxima de avaliação aninhada. O jq não tem limite: a pilha dele
/// cresce no heap até faltar memória, e aí ele escreve "jq: error: cannot allocate memory" e aborta.
/// Aqui os segmentos de pilha do `stacker` ficam fora da contabilidade do alocador, então o limite
/// faz o papel da memória que acabou: ao estourar, a avaliação desenrola com [`DepthExceeded`] e
/// quem roda o filtro reproduz o fim do jq.
pub const MAX_EVAL_DEPTH: usize = 20_000;

/// Porte pseudo-linus: payload do desenrolar quando o jq morreria por falta de memória (avaliação
/// além de [`MAX_EVAL_DEPTH`], ou uma alocação pedida pelo programa que não cabe). Não é uma exceção
/// do jq: `try` não captura. Quem roda o filtro escreve "jq: error: cannot allocate memory" e aborta,
/// como o `memory_exhausted` do jq.
#[derive(Debug)]
pub struct OutOfMemory;

/// Porte pseudo-linus: termina a avaliação como falta de memória (ver [`OutOfMemory`]).
pub fn out_of_memory() -> ! {
    extern crate std;
    std::panic::resume_unwind(Box::new(OutOfMemory))
}

/// Porte pseudo-linus: roda `f` com pilha garantida (recursão sobre dados fundos).
pub fn with_stack<R>(f: impl FnOnce() -> R) -> R {
    stacker::maybe_grow(STACK_RED_ZONE, STACK_SEGMENT, f)
}

mod depth {
    extern crate std;
    std::thread_local! {
        static DEPTH: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
    }

    /// Nível de avaliação em curso; sai do nível ao ser descartado (inclusive no desenrolar).
    pub struct Level(());

    impl Drop for Level {
        fn drop(&mut self) {
            DEPTH.with(|d| d.set(d.get().saturating_sub(1)));
        }
    }

    /// Entra num nível; desenrola com `DepthExceeded` se passar do limite.
    pub fn enter() -> Level {
        let n = DEPTH.with(|d| {
            let n = d.get() + 1;
            d.set(n);
            n
        });
        let level = Level(());
        if n > super::MAX_EVAL_DEPTH {
            super::out_of_memory();
        }
        level
    }
}

/// Porte pseudo-linus: roda `f` com pilha garantida, contando a profundidade.
fn guarded<T>(f: impl FnOnce() -> T) -> T {
    let _level = depth::enter();
    stacker::maybe_grow(STACK_RED_ZONE, STACK_SEGMENT, f)
}

/// Porte pseudo-linus: iterador cujo `next` também roda com pilha garantida. A recursão do jaq
/// acontece tanto ao montar os iteradores (`run`) quanto ao consumi-los (`next` aninhado).
struct GuardedIter<I>(I);

fn guard_iter<'a, T: 'a>(it: box_iter::BoxIter<'a, T>) -> box_iter::BoxIter<'a, T> {
    Box::new(GuardedIter(it))
}

impl<I: Iterator> Iterator for GuardedIter<I> {
    type Item = I::Item;
    fn next(&mut self) -> Option<I::Item> {
        let inner = &mut self.0;
        guarded(|| inner.next())
    }
}

/// List of bindings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vars<V>(RcList<Bind<V, usize, (Id, Self)>>);

impl<V> Vars<V> {
    /// Initialise new variables from values.
    pub fn new(vars: impl IntoIterator<Item = V>) -> Self {
        Self(RcList::new().extend(vars.into_iter().map(Bind::Var)))
    }

    fn get(&self, i: usize) -> Option<&Bind<V, usize, (Id, Self)>> {
        self.0.get(i)
    }
}

impl<V> Default for Vars<V> {
    fn default() -> Self {
        Self(RcList::default())
    }
}

/// Filter execution context.
pub struct Ctx<'a, D: DataT + ?Sized> {
    data: D::Data<'a>,
    vars: Vars<D::V<'a>>,
    /// Number of bound labels at the current path
    ///
    /// This is used to create fresh break IDs.
    labels: usize,
}

impl<'a, D: DataT> Clone for Ctx<'a, D> {
    fn clone(&self) -> Self {
        self.with_vars(Vars(self.vars.0.clone()))
    }
}

impl<'a, D: DataT> Ctx<'a, D> {
    /// Construct a fresh context.
    ///
    /// If you do not use any filters that need custom data (such as `inputs`)
    /// and your value type has a `'static` lifetime, then you may use
    /// [`crate::data::JustLut`] as [`DataT`].
    pub fn new(data: D::Data<'a>, vars: Vars<D::V<'a>>) -> Self {
        Self {
            data,
            vars,
            labels: 0,
        }
    }

    /// Add a new variable binding.
    fn cons_var(mut self, x: D::V<'a>) -> Self {
        self.vars.0 = self.vars.0.cons(Bind::Var(x));
        self
    }

    /// Add a new filter binding.
    fn cons_fun(mut self, (f, ctx): (Id, Self)) -> Self {
        self.vars.0 = self.vars.0.cons(Bind::Fun((f, ctx.vars)));
        self
    }

    fn cons_label(mut self) -> Self {
        self.labels += 1;
        self.vars.0 = self.vars.0.cons(Bind::Label(self.labels));
        self
    }

    /// Remove the `skip` most recent variable bindings.
    fn skip_vars(mut self, skip: usize) -> Self {
        if skip > 0 {
            self.vars.0 = self.vars.0.skip(skip).clone();
        }
        self
    }

    /// Replace variables in context with given ones.
    fn with_vars(&self, vars: Vars<D::V<'a>>) -> Self {
        Self {
            vars,
            data: self.data.clone(),
            labels: self.labels,
        }
    }

    fn lut(&self) -> &'a Lut<D> {
        self.data.lut()
    }

    /// Return global data.
    pub fn data(&self) -> &D::Data<'a> {
        &self.data
    }
}

impl<'a, D: DataT> Ctx<'a, D> {
    /// Remove the latest bound variable from the context.
    ///
    /// This is useful for writing [`Native`] filters.
    pub fn pop_var(&mut self) -> D::V<'a> {
        let (head, tail) = match core::mem::take(&mut self.vars.0).pop() {
            Some((Bind::Var(head), tail)) => (head, tail),
            _ => panic!(),
        };
        self.vars.0 = tail;
        head
    }

    /// Remove the latest bound function from the context.
    ///
    /// This is useful for writing [`Native`] filters.
    pub fn pop_fun(&mut self) -> (Id, Self) {
        let ((id, vars), tail) = match core::mem::take(&mut self.vars.0).pop() {
            Some((Bind::Fun(head), tail)) => (head, tail),
            _ => panic!(),
        };
        self.vars.0 = tail;
        (id, self.with_vars(vars))
    }
}

/// Enhance the context `ctx` with variables bound to the outputs of `args` executed on `cv`,
/// and return the enhanced contexts together with the original value of `cv`.
///
/// This is used when we call filters with variable arguments.
fn bind_vars<'a, D: DataT, T: 'a + Clone>(
    args: &'a [Arg<Id>],
    ctx: Ctx<'a, D>,
    cv: Cv<'a, D, T>,
    proj: fn(&T) -> D::V<'a>,
) -> ValXs<'a, Cv<'a, D, T>, D::V<'a>> {
    match args.split_first() {
        Some((Arg::Var(arg), [])) => map_with(
            arg.run((cv.0.clone(), proj(&cv.1))),
            (ctx, cv.1),
            |y, (ctx, v)| Ok((ctx.cons_var(y?), v)),
        ),
        Some((Arg::Fun(arg), [])) => box_once(Ok((ctx.cons_fun((*arg, cv.0)), cv.1))),
        Some((Arg::Var(arg), rest)) => flat_map_then_with(
            arg.run((cv.0.clone(), proj(&cv.1))),
            (ctx, cv),
            move |y, (ctx, cv)| bind_vars(rest, ctx.cons_var(y), cv, proj),
        ),
        Some((Arg::Fun(arg), rest)) => {
            bind_vars(rest, ctx.cons_fun((*arg, cv.0.clone())), cv, proj)
        }
        None => box_once(Ok((ctx, cv.1))),
    }
}

fn bind_pat<'a, D: DataT>(
    (idxs, pat): &'a (Id, Pattern<Id>),
    ctx: Ctx<'a, D>,
    cv: Cv<'a, D>,
) -> ValXs<'a, Ctx<'a, D>, D::V<'a>> {
    let (ctx0, v0) = cv.clone();
    let v1 = map_with(idxs.run(cv), v0, move |i, v0| Ok(v0.index(&i?)?));
    match pat {
        Pattern::Var => Box::new(v1.map(move |v| Ok(ctx.clone().cons_var(v?)))),
        Pattern::Idx(pats) => flat_map_then_with(v1, (ctx, ctx0), move |v, (ctx, ctx0)| {
            bind_pats(pats, ctx, (ctx0, v))
        }),
        // Porte pseudo-linus: alternativas só aparecem no topo; aqui ficam por completude.
        Pattern::Alt(alts) => flat_map_then_with(v1, ctx, move |v, ctx| bind_alts(alts, 0, ctx, v)),
    }
}

/// Porte pseudo-linus: liga a primeira alternativa que não der erro (padrões de `reduce`/`foreach`).
fn bind_alts<'a, D: DataT>(
    alts: &'a [AltPattern<Id>],
    i: usize,
    ctx: Ctx<'a, D>,
    y: D::V<'a>,
) -> ValXs<'a, Ctx<'a, D>, D::V<'a>> {
    let (pat, slots) = &alts[i];
    let bound = bind_alt(pat, slots, ctx.clone(), y.clone());
    if i + 1 == alts.len() {
        return bound;
    }
    try_catch_run(bound, move |_e| bind_alts(alts, i + 1, ctx.clone(), y.clone()))
}

fn bind_pats<'a, D: DataT>(
    pats: &'a [(Id, Pattern<Id>)],
    ctx: Ctx<'a, D>,
    cv: Cv<'a, D>,
) -> ValXs<'a, Ctx<'a, D>, D::V<'a>> {
    match pats.split_first() {
        None => box_once(Ok(ctx)),
        Some((pat, [])) => bind_pat(pat, ctx, cv),
        Some((pat, rest)) => flat_map_then_with(bind_pat(pat, ctx, cv.clone()), cv, |ctx, cv| {
            bind_pats(rest, ctx, cv)
        }),
    }
}

fn run_and_bind<'a, D: DataT>(
    xs: &'a Id,
    cv: Cv<'a, D>,
    pat: &'a Pattern<Id>,
) -> ValXs<'a, Ctx<'a, D>, D::V<'a>> {
    let xs = xs.run((cv.0.clone(), cv.1));
    match pat {
        Pattern::Var => map_with(xs, cv.0, move |y, ctx| Ok(ctx.cons_var(y?))),
        Pattern::Idx(pats) => {
            flat_map_then_with(xs, cv.0, |y, ctx| bind_pats(pats, ctx.clone(), (ctx, y)))
        }
        Pattern::Alt(alts) => flat_map_then_with(xs, cv.0, move |y, ctx| bind_alts(alts, 0, ctx, y)),
    }
}

fn bind_run<'a, D: DataT, T: Clone + 'a>(
    pat: &'a Pattern<Id>,
    r: &'a Id,
    cv: Cv<'a, D, T>,
    y: D::V<'a>,
    run: IdRunFn<'a, D, T>,
) -> ValXs<'a, T, D::V<'a>> {
    match pat {
        Pattern::Var => run(r, (cv.0.cons_var(y), cv.1)),
        Pattern::Idx(pats) => {
            let r = move |ctx, vp| run(r, (ctx, vp));
            flat_map_then_with(bind_pats(pats, cv.0.clone(), (cv.0, y)), cv.1, r)
        }
        Pattern::Alt(alts) => {
            let r = move |ctx, vp| run(r, (ctx, vp));
            flat_map_then_with(bind_alts(alts, 0, cv.0, y), cv.1, r)
        }
    }
}

fn label_run<'a, D: DataT, T: 'a>(
    cv: Cv<'a, D, T>,
    run: impl Fn(Cv<'a, D, T>) -> ValXs<'a, T, D::V<'a>>,
) -> ValXs<'a, T, D::V<'a>> {
    let ctx = cv.0.cons_label();
    let labels = ctx.labels;
    Box::new(run((ctx, cv.1)).map_while(move |y| match y {
        Err(Exn(exn::Inner::Break(b))) if b == labels => None,
        y => Some(y),
    }))
}

fn try_catch_run<'a, T: 'a, V: 'a, I: Iterator<Item = ValX<'a, T, V>> + 'a>(
    mut ys: ValXs<'a, T, V>,
    f: impl Fn(Error<V>) -> I + 'a,
) -> ValXs<'a, T, V> {
    let mut end: Option<I> = None;
    Box::new(core::iter::from_fn(move || match &mut end {
        Some(end) => end.next(),
        None => match ys.next()? {
            Err(Exn(exn::Inner::Err(e))) => {
                end = Some(f(*e));
                end.as_mut().and_then(|end| end.next())
            }
            y => Some(y),
        },
    }))
}

/// Porte pseudo-linus: `reduce` e `foreach` com a semântica do jq 1.7.1 (`gen_reduce` e
/// `gen_foreach`): o estado é um só e vira a última saída da atualização; se a atualização não
/// tiver saída, o estado vira `null` (o `LOADVN` do jq). O `foreach` emite cada saída da atualização
/// (ou do `extract`) assim que ela sai. O fork original explorava cada saída como um ramo separado.
fn fold_jq<'a, D: DataT, T: Clone + 'a>(
    xs: impl Iterator<Item = ValX<'a, Ctx<'a, D>, D::V<'a>>> + Clone + 'a,
    cv: Cv<'a, D, T>,
    init: &'a Id,
    update: &'a Id,
    fold_type: &'a Fold<Id>,
    run: IdRunFn<'a, D, T>,
    null: fn() -> T,
) -> ValXs<'a, T, D::V<'a>> {
    let inits = run(init, cv);
    flat_map_then_with(inits, xs, move |state, xs| -> ValXs<'a, T, D::V<'a>> {
        match fold_type {
            Fold::Reduce => Box::new(core::iter::once_with(move || {
                let mut state = state;
                for ctx in xs {
                    let mut last = None;
                    for out in run(update, (ctx?, state)) {
                        last = Some(out?);
                    }
                    state = last.unwrap_or_else(null);
                }
                Ok(state)
            })),
            Fold::Foreach(proj) => Box::new(Foreach {
                xs,
                state: Some(state),
                ctx: None,
                outs: None,
                extract: None,
                proj: proj.as_ref(),
                run,
                update,
                null,
                done: false,
            }),
        }
    })
}

/// Iterador do `foreach` do jq (ver [`fold_jq`]).
struct Foreach<'a, D: DataT, T, X> {
    xs: X,
    state: Option<T>,
    ctx: Option<Ctx<'a, D>>,
    outs: Option<ValXs<'a, T, D::V<'a>>>,
    extract: Option<ValXs<'a, T, D::V<'a>>>,
    proj: Option<&'a Id>,
    run: IdRunFn<'a, D, T>,
    update: &'a Id,
    null: fn() -> T,
    done: bool,
}

impl<'a, D: DataT, T: Clone + 'a, X> Iterator for Foreach<'a, D, T, X>
where
    X: Iterator<Item = ValX<'a, Ctx<'a, D>, D::V<'a>>>,
{
    type Item = ValX<'a, T, D::V<'a>>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if self.done {
                return None;
            }
            if let Some(ex) = &mut self.extract {
                match ex.next() {
                    Some(Err(e)) => {
                        self.done = true;
                        return Some(Err(e));
                    }
                    Some(y) => return Some(y),
                    None => self.extract = None,
                }
            }
            if let Some(outs) = &mut self.outs {
                match outs.next() {
                    Some(Ok(out)) => {
                        self.state = Some(out.clone());
                        match (self.proj, &self.ctx) {
                            (Some(proj), Some(ctx)) => {
                                self.extract = Some((self.run)(proj, (ctx.clone(), out)));
                                continue;
                            }
                            _ => return Some(Ok(out)),
                        }
                    }
                    Some(Err(e)) => {
                        self.done = true;
                        return Some(Err(e));
                    }
                    None => {
                        self.outs = None;
                        if self.state.is_none() {
                            self.state = Some((self.null)());
                        }
                    }
                }
            }
            match self.xs.next()? {
                Ok(ctx) => {
                    let state = self.state.take().unwrap_or_else(self.null);
                    self.outs = Some((self.run)(self.update, (ctx.clone(), state)));
                    self.ctx = Some(ctx);
                }
                Err(e) => {
                    self.done = true;
                    return Some(Err(e));
                }
            }
        }
    }
}

/// Porte pseudo-linus: `l as p1 ?// p2 ?// ... | r`. Liga a alternativa `i` (variáveis que ela não
/// tem ficam `null`) e roda `r`; um erro ao ligar ou no corpo passa para a próxima alternativa, e o
/// erro da última sobe. Saídas já emitidas ficam.
fn alt_run<'a, D: DataT, T: Clone + 'a>(
    alts: &'a [AltPattern<Id>],
    i: usize,
    r: &'a Id,
    cv: Cv<'a, D, T>,
    y: D::V<'a>,
    run: IdRunFn<'a, D, T>,
) -> ValXs<'a, T, D::V<'a>> {
    let (pat, slots) = &alts[i];
    let outs = bind_alt(pat, slots, cv.0.clone(), y.clone());
    let body = flat_map_then_with(outs, cv.1.clone(), move |ctx, t| run(r, (ctx, t)));
    if i + 1 == alts.len() {
        return body;
    }
    try_catch_run(body, move |_e| alt_run(alts, i + 1, r, cv.clone(), y.clone(), run))
}

/// Liga uma alternativa: as variáveis do padrão são ligadas num contexto vazio e depois copiadas, na
/// ordem da união de todas as alternativas, para o contexto de fora.
fn bind_alt<'a, D: DataT>(
    pat: &'a Pattern<Id>,
    slots: &'a [Option<usize>],
    ctx: Ctx<'a, D>,
    y: D::V<'a>,
) -> ValXs<'a, Ctx<'a, D>, D::V<'a>> {
    let scratch = ctx.with_vars(Vars::new([]));
    let bound: ValXs<'a, Ctx<'a, D>, D::V<'a>> = match pat {
        Pattern::Var => box_once(Ok(scratch.cons_var(y))),
        Pattern::Idx(pats) => bind_pats(pats, scratch, (ctx.clone(), y)),
        Pattern::Alt(alts) => bind_alts(alts, 0, scratch, y),
    };
    let n = pattern_vars(pat);
    Box::new(bound.map(move |b| {
        let mut b = b?;
        // Mais recente primeiro: inverte para a ordem de ligação.
        let mut vals: Vec<D::V<'a>> = (0..n).map(|_| b.pop_var()).collect();
        vals.reverse();
        let mut out = ctx.clone();
        for slot in slots.iter() {
            out = out.cons_var(slot.map_or_else(D::V::null, |j| vals[j].clone()));
        }
        Ok(out)
    }))
}

/// Número de variáveis que um padrão liga.
fn pattern_vars(pat: &Pattern<Id>) -> usize {
    match pat {
        Pattern::Var => 1,
        Pattern::Idx(pats) => pats.iter().map(|(_, p)| pattern_vars(p)).sum(),
        Pattern::Alt(alts) => alts.first().map_or(0, |(_, slots)| slots.len()),
    }
}

/// Porte pseudo-linus: função que aplica uma parte de caminho já avaliada.
type ApplyPartFn<'a, D, T> =
    fn(crate::path::Part<<D as DataT>::V<'a>>, crate::path::Opt, T) -> crate::val::ValRs<'a, T, <D as DataT>::V<'a>>;

/// Porte pseudo-linus: `t[k1]...[kn]` na ordem do jq. O jq avalia a subexpressão da chave antes do
/// termo indexado, então a chave da última parte varia mais devagar (`[[1,2],[3,4]] | .[0,1][0,1]`
/// dá `1,3,2,4`), e o termo roda de novo para cada combinação de chaves. As chaves são avaliadas
/// contra a entrada original.
fn path_term<'a, D: DataT, T: Clone + 'a>(
    f: &'a Id,
    parts: &'a [(crate::path::Part<Id>, crate::path::Opt)],
    cv: Cv<'a, D, T>,
    keyin: D::V<'a>,
    run: IdRunFn<'a, D, T>,
    apply: ApplyPartFn<'a, D, T>,
) -> ValXs<'a, T, D::V<'a>> {
    use crate::path::Part;
    let Some(((part, opt), init)) = parts.split_last() else {
        return run(f, cv);
    };
    let opt = *opt;
    let inner = move |part: Part<D::V<'a>>, cv: Cv<'a, D, T>, keyin: D::V<'a>| -> ValXs<'a, T, D::V<'a>> {
        flat_map_then(path_term(f, init, cv, keyin, run, apply), move |t| {
            Box::new(apply(part.clone(), opt, t).map(|r| r.map_err(Exn::from)))
        })
    };
    let key = |k: &'a Id, cv: &Cv<'a, D, T>, keyin: &D::V<'a>| k.run((cv.0.clone(), keyin.clone()));
    match part {
        Part::Index(k) => flat_map_then_with(key(k, &cv, &keyin), (cv, keyin), move |kv, (cv, keyin)| {
            inner(Part::Index(kv), cv, keyin)
        }),
        Part::Range(None, None) => inner(Part::Range(None, None), cv, keyin),
        Part::Range(Some(from), None) => {
            flat_map_then_with(key(from, &cv, &keyin), (cv, keyin), move |fv, (cv, keyin)| {
                inner(Part::Range(Some(fv), None), cv, keyin)
            })
        }
        Part::Range(None, Some(upto)) => {
            flat_map_then_with(key(upto, &cv, &keyin), (cv, keyin), move |uv, (cv, keyin)| {
                inner(Part::Range(None, Some(uv)), cv, keyin)
            })
        }
        Part::Range(Some(from), Some(upto)) => {
            flat_map_then_with(key(from, &cv, &keyin), (cv, keyin), move |fv, (cv, keyin)| {
                let k2 = key(upto, &cv, &keyin);
                flat_map_then_with(k2, (cv, keyin, fv), move |uv, (cv, keyin, fv)| {
                    inner(Part::Range(Some(fv), Some(uv)), cv, keyin)
                })
            })
        }
    }
}

/// For every value `v` returned by `self.run(cv)`, call `f(cv, v)` and return all results.
///
/// This has a special optimisation for the case where only a single `v` is returned.
/// In that case, we can consume `cv` instead of cloning it.
fn pipe<'a, D: DataT, T: 'a, F>(l: &'a Id, cv: Cv<'a, D>, r: F) -> ValXs<'a, T, D::V<'a>>
where
    F: Fn(Cv<'a, D>, D::V<'a>) -> ValXs<'a, T, D::V<'a>> + 'a,
{
    flat_map_then_with(l.run(cv.clone()), cv, move |y, cv| r(cv, y))
}

type Pairs<'a, T> = box_iter::BoxIter<'a, (T, T)>;

/// Run `self` and `r` and return the cartesian product of their outputs.
fn cartesian<'a, D: DataT>(l: &'a Id, r: &'a Id, cv: Cv<'a, D>) -> Pairs<'a, ValX<'a, D::V<'a>>> {
    flat_map_with(l.run(cv.clone()), cv, move |l, cv| {
        map_with(r.run(cv), l, |r, l| (l, r))
    })
}

/// Porte pseudo-linus: produto cartesiano com o lado direito por fora, a ordem do jq para
/// operadores binários (ele avalia as duas subexpressões e o backtracking varia primeiro a da
/// esquerda).
fn cartesian_rl<'a, D: DataT>(l: &'a Id, r: &'a Id, cv: Cv<'a, D>) -> Pairs<'a, ValX<'a, D::V<'a>>> {
    flat_map_with(r.run(cv.clone()), cv, move |r, cv| {
        map_with(l.run(cv), r, |l, r| (l, r))
    })
}

/// Porte pseudo-linus: `_modify(paths; update)` do jq 1.7.1, feito em Rust com o documento possuído
/// (sem as cópias que o `reduce` em jq faria):
///
/// ~~~ text
/// def _modify(paths; update): reduce path(paths) as $p ([., []];
///     . as $dot | null | label $out | ($dot[0] | getpath($p)) as $v
///     | ((($v | update | (., break $out) as $v | $dot | setpath([0] + $p; $v)),
///        ($dot | setpath([1, (.[1] | length)]; $p))))
///   | . as $dot | $dot[0] | delpaths($dot[1]);
/// ~~~
///
/// Os caminhos vêm da entrada original; cada um recebe a primeira saída de `update` aplicada ao valor
/// atual; os que não têm saída são apagados todos juntos no fim.
fn modify<'a, D: DataT>(
    path: &'a Id,
    cv: Cv<'a, D>,
    update: impl Fn(Ctx<'a, D>, D::V<'a>) -> ValXs<'a, D::V<'a>> + 'a,
) -> ValXs<'a, D::V<'a>> {
    let (ctx, input) = cv;
    let mut paths = path.paths((ctx.clone(), (input.clone(), Some(RcList::new()))));
    let mut doc = Some(input);
    let mut dels: Vec<D::V<'a>> = Vec::new();
    Box::new(core::iter::from_fn(move || {
        let mut cur_doc = doc.take()?;
        for p in paths.by_ref() {
            let (v, p) = match p {
                Ok(vp) => vp,
                Err(e) => return Some(Err(e)),
            };
            let Some(p) = p else {
                return Some(Err(Exn::from(Error::path_result(&v))));
            };
            let p: Vec<D::V<'a>> = path_vec(&p);
            let old = match cur_doc.getpath(&p) {
                Ok(old) => old,
                Err(e) => return Some(Err(Exn::from(e))),
            };
            match update(ctx.clone(), old).next() {
                Some(Ok(new)) => match cur_doc.setpath(&p, new) {
                    Ok(d) => cur_doc = d,
                    Err(e) => return Some(Err(Exn::from(e))),
                },
                Some(Err(e)) => return Some(Err(e)),
                None => dels.push(p.into_iter().collect()),
            }
        }
        let dels = core::mem::take(&mut dels);
        Some(if dels.is_empty() { Ok(cur_doc) } else { cur_doc.delpaths(dels).map_err(Exn::from) })
    }))
}

/// Porte pseudo-linus: `_assign(paths; $value)`: `reduce path(paths) as $p (.; setpath($p; $value))`.
fn assign<'a, D: DataT>(path: &'a Id, cv: Cv<'a, D>, value: D::V<'a>) -> ValXs<'a, D::V<'a>> {
    let (ctx, input) = cv;
    let mut paths = path.paths((ctx, (input.clone(), Some(RcList::new()))));
    let mut doc = Some(input);
    Box::new(core::iter::from_fn(move || {
        let mut cur_doc = doc.take()?;
        for p in paths.by_ref() {
            let (v, p) = match p {
                Ok(vp) => vp,
                Err(e) => return Some(Err(e)),
            };
            let Some(p) = p else {
                return Some(Err(Exn::from(Error::path_result(&v))));
            };
            match cur_doc.setpath(&path_vec(&p), value.clone()) {
                Ok(d) => cur_doc = d,
                Err(e) => return Some(Err(Exn::from(e))),
            }
        }
        Some(Ok(cur_doc))
    }))
}

/// Caminho rastreado (lista invertida) como vetor na ordem natural.
pub(crate) fn path_vec<V: Clone>(p: &RcList<V>) -> Vec<V> {
    let mut v: Vec<V> = p.iter().cloned().collect();
    v.reverse();
    v
}

fn def_run<'a, D: DataT, T: 'a>(
    id: &'a Id,
    call_typ: &CallType,
    cvs: ValXs<'a, Cv<'a, D, T>, D::V<'a>>,
    run: IdRunFn<'a, D, T>,
    with_vars: impl Fn(Vars<D::V<'a>>) -> Ctx<'a, D> + 'a,
    into: fn(T) -> exn::CallInput<D::V<'a>>,
    from: fn(exn::CallInput<D::V<'a>>) -> T,
) -> ValXs<'a, T, D::V<'a>> {
    use core::ops::ControlFlow;
    let outs = |cvs| flat_map_then(cvs, move |cv| run(id, cv));
    let catch = |all: bool| {
        move |r| match r {
            Err(Exn(exn::Inner::TailCall(tc))) if all || tc.0 == id => {
                ControlFlow::Continue(run(tc.0, (with_vars(tc.1), from(tc.2))))
            }
            Ok(_) | Err(_) => ControlFlow::Break(r),
        }
    };

    match call_typ {
        CallType::Inline => outs(cvs),
        CallType::CatchOne => Box::new(crate::Stack::new([outs(cvs)].into(), catch(false))),
        CallType::CatchAll => Box::new(crate::Stack::new([outs(cvs)].into(), catch(true))),
        CallType::Throw => Box::new(cvs.map(move |cv| {
            cv.and_then(|cv| {
                let tc = (id, cv.0.vars, into(cv.1));
                Err(Exn(exn::Inner::TailCall(Box::new(tc))))
            })
        })),
    }
}

fn lazy<I: Iterator, F: FnOnce() -> I>(f: F) -> impl Iterator<Item = I::Item> {
    core::iter::once_with(f).flatten()
}

#[test]
fn lazy_is_lazy() {
    let f = || panic!();
    let mut iter = core::iter::once(0).chain(lazy(|| box_once(f())));
    assert_eq!(iter.size_hint(), (1, None));
    assert_eq!(iter.next(), Some(0));
}

/// Runs `def recurse(f): ., (f? | recurse(f)); v | recurse(f)`.
fn recurse_run<'a, T: Clone + 'a, V: 'a, I: Iterator<Item = ValR<T, V>> + 'a>(
    x: T,
    f: &'a impl Fn(T) -> I,
) -> ValXs<'a, T, V> {
    let id = core::iter::once(Ok(x.clone()));
    Box::new(id.chain(f(x).flatten().flat_map(|y| recurse_run(y, f))))
}

/// A filter which is implemented using function pointers.
///
/// Porte pseudo-linus: sem função de atualização (as atualizações usam só os caminhos), e a função
/// de caminhos é opcional: nativa sem ela roda normalmente em `path(...)` e o resultado só fica no
/// caminho se for idêntico à entrada, como no jq.
pub struct Native<D: DataT + ?Sized> {
    run: RunPtr<D>,
    paths: Option<PathsPtr<D>>,
}

type IdRunFn<'a, D, T> = fn(&Id, Cv<'a, D, T>) -> ValXs<'a, T, <D as DataT>::V<'a>>;

/// Run function pointer (see [`Id::run`]).
pub type RunPtr<D> = for<'a> fn(Cv<'a, D>) -> ValXs<'a, <D as DataT>::V<'a>>;
/// Paths function pointer (see [`Id::paths`]).
pub type PathsPtr<D> = for<'a> fn(Cvp<'a, D>) -> ValPathXs<'a, <D as DataT>::V<'a>>;

impl<D: DataT> Native<D> {
    /// Create a native filter from a run function.
    ///
    /// A filter created this way initially does not support paths.
    /// For that, use [`Self::with_paths`].
    pub const fn new(run: RunPtr<D>) -> Self {
        Self { run, paths: None }
    }

    /// Specify a paths function (used for `path(...)`).
    pub const fn with_paths(self, paths: PathsPtr<D>) -> Self {
        Self { paths: Some(paths), ..self }
    }
}

/// Porte pseudo-linus: valores calculados dentro de `path(...)` continuam no caminho só se forem
/// idênticos (`jv_identical`) ao valor do caminho atual.
fn mark<'a, V: ValT + 'a>(ys: ValXs<'a, V>, (v, p): (V, Tracked<V>)) -> ValPathXs<'a, V> {
    Box::new(ys.map(move |y| {
        y.map(|y| {
            let keep = p.is_some() && y.identical(&v);
            let path = if keep { p.clone() } else { None };
            (y, path)
        })
    }))
}

impl Id {
    /// `f.run((c, v))` returns the output of `v | f` in the context `c`.
    pub fn run<'a, D: DataT>(&self, cv: Cv<'a, D>) -> ValXs<'a, D::V<'a>> {
        use core::iter::once;
        match &cv.0.lut().terms[self.0] {
            Ast::Id => box_once(Ok(cv.1)),
            // Porte pseudo-linus: a recursão em dados fundos também passa pela guarda de pilha.
            Ast::Recurse => guard_iter(recurse_run(cv.1, &|v| v.values())),
            Ast::ToString => box_once(Ok(cv.1.into_string())),
            Ast::Int(n) => box_once(Ok(D::V::from(*n))),
            Ast::Num(x) => box_once(D::V::from_num(x).map_err(Exn::from)),
            Ast::Str(s) => box_once(Ok(D::V::from(s.clone()))),
            Ast::Arr(f) => box_once(f.run(cv).collect()),
            Ast::ObjEmpty => box_once(D::V::from_map([]).map_err(Exn::from)),
            Ast::ObjSingle(k, v) => {
                Box::new(cartesian(k, v, cv).map(|(k, v)| Ok(D::V::from_map([(k?, v?)])?)))
            }
            // Porte pseudo-linus: `{a: f, b: g}` varia primeiro a última entrada (como no jq), ao
            // contrário dos operadores binários.
            Ast::ObjMerge(l, r) => Box::new(cartesian(l, r, cv).map(|(x, y)| Ok((x? + y?)?))),
            Ast::TryCatch(f, c) => try_catch_run(f.run((cv.0.clone(), cv.1)), move |e| {
                c.run((cv.0.clone(), e.into_val()))
            }),
            Ast::Neg(f) => Box::new(f.run(cv).map(|v| Ok((-v?)?))),

            // `l | r`
            Ast::Pipe(l, None, r) => {
                flat_map_then_with(l.run((cv.0.clone(), cv.1)), cv.0, move |y, ctx| {
                    r.run((ctx, y))
                })
            }
            // `l as $x | r`, `l as [...] | r`, or `l as {...} | r`
            Ast::Pipe(l, Some(pat), r) => pipe(l, cv, move |cv, y| {
                bind_run(pat, r, cv, y, |f, cv| f.run(cv))
            }),
            // Porte pseudo-linus: `l as p1 ?// p2 ?// ... | r`.
            Ast::PipeAlt(l, alts, r) => pipe(l, cv, move |cv, y| alt_run(alts, 0, r, cv, y, |f, cv| f.run(cv))),
            Ast::Comma(l, r) => Box::new(l.run(cv.clone()).chain(lazy(|| r.run(cv)))),
            Ast::Alt(l, r) => {
                let mut l = l
                    .run(cv.clone())
                    .filter(|v| v.as_ref().map_or(true, ValT::as_bool));
                match l.next() {
                    Some(head) => Box::new(once(head).chain(l)),
                    None => r.run(cv),
                }
            }
            Ast::Ite(if_, then_, else_) => pipe(if_, cv, move |cv, v| {
                if v.as_bool() { then_ } else { else_ }.run(cv)
            }),
            Ast::Path(f, path) => {
                let keyin = cv.1.clone();
                path_term(f, &path.0, cv, keyin, |f, cv| f.run(cv), |part, opt, v| part.run_opt(opt, v))
            }

            // Porte pseudo-linus: atualizações pelo `_modify`/`_assign` do jq (ver [`modify`]).
            Ast::Update(path, f) => modify(path, cv, move |ctx, v| f.run((ctx, v))),
            Ast::UpdateMath(path, op, f) => pipe(f, cv, move |cv, y| {
                modify(path, cv, move |_, x| box_once(op.run(x, y.clone()).map_err(Exn::from)))
            }),
            Ast::UpdateAlt(path, f) => pipe(f, cv, move |cv, y| {
                modify(path, cv, move |_, x| box_once(Ok(if x.as_bool() { x } else { y.clone() })))
            }),
            Ast::Assign(path, f) => pipe(f, cv, move |cv, y| assign(path, cv, y)),
            Ast::Logic(l, stop, r) => pipe(l, cv, move |cv, l| {
                if l.as_bool() == *stop {
                    box_once(Ok(D::V::from(*stop)))
                } else {
                    Box::new(r.run(cv).map(|r| Ok(D::V::from(r?.as_bool()))))
                }
            }),
            // Porte pseudo-linus: o jq avalia o lado direito por fora (`[(1,2) + (10,20)]` dá
            // `[11,12,21,22]`).
            Ast::Math(l, op, r) => Box::new(cartesian_rl(l, r, cv).map(|(x, y)| Ok(op.run(x?, y?)?))),
            Ast::Cmp(l, op, r) => {
                Box::new(cartesian_rl(l, r, cv).map(|(x, y)| Ok(D::V::from(op.run(&x?, &y?)))))
            }

            Ast::Fold(xs, pat, init, update, fold_type) => {
                let xs = rc_lazy_list::List::from_iter(run_and_bind(xs, cv.clone(), pat));
                fold_jq(xs, cv, init, update, fold_type, |f, cv| f.run(cv), D::V::null)
            }
            Ast::Var(v) => match cv.0.vars.get(*v).unwrap() {
                Bind::Var(v) => box_once(Ok(v.clone())),
                Bind::Fun((id, vars)) => id.run((cv.0.with_vars(vars.clone()), cv.1)),
                Bind::Label(l) => box_once(Err(Exn(exn::Inner::Break(*l)))),
            },
            Ast::CallDef(id, args, skip, call_typ) => {
                let data = cv.0.data.clone();
                let with_vars = move |vars| Ctx {
                    vars,
                    data: data.clone(),
                    labels: cv.0.labels,
                };
                let cvs = bind_vars(args, cv.0.clone().skip_vars(*skip), cv, Clone::clone);
                let (into, from) = (exn::CallInput::Run, exn::CallInput::unwrap_run);
                // Porte pseudo-linus: chamada de definição é onde a recursão sem fim acontece.
                guarded(|| guard_iter(def_run(id, call_typ, cvs, Id::run, with_vars, into, from)))
            }
            Ast::Native(id, args) => {
                let cvs = bind_vars(args, cv.0.with_vars(Vars::new([])), cv, Clone::clone);
                flat_map_then(cvs, |cv| (cv.0.lut().funs[*id].run)(cv))
            }
            Ast::Label(id) => label_run(cv, |cv| id.run(cv)),
        }
    }

    /// `f.paths((c, (v, p)))` returns the outputs and paths of `v | f` in the context `c`,
    /// where `v` is assumed to be at path `p`.
    ///
    /// In particular, `v | path(f)` in context `c` yields the same paths as
    /// `f.paths((c, (v, Default::default())))`.
    pub fn paths<'a, D: DataT>(&self, cv: Cvp<'a, D>) -> ValPathXs<'a, D::V<'a>> {
        let proj_cv = |cv: &Cvp<'a, D>| (cv.0.clone(), cv.1 .0.clone());
        let proj_val = |(val, _path): &(D::V<'a>, _)| val.clone();
        match &cv.0.lut().terms[self.0] {
            // Porte pseudo-linus: termos que não são de caminho rodam normalmente, e o resultado só
            // continua no caminho se for idêntico ao valor atual (o `path_intact` do jq); se não for,
            // a próxima operação de caminho (ou o fim do `path(...)`) dá o erro do jq.
            Ast::ToString | Ast::Int(_) | Ast::Num(_) | Ast::Str(_) => mark(self.run(proj_cv(&cv)), cv.1),
            Ast::Arr(_) | Ast::ObjEmpty | Ast::ObjSingle(..) | Ast::ObjMerge(..) => {
                mark(self.run(proj_cv(&cv)), cv.1)
            }
            Ast::Neg(_) | Ast::Logic(..) | Ast::Math(..) | Ast::Cmp(..) => mark(self.run(proj_cv(&cv)), cv.1),
            Ast::Update(..) | Ast::Assign(..) | Ast::UpdateMath(..) | Ast::UpdateAlt(..) => {
                mark(self.run(proj_cv(&cv)), cv.1)
            }
            Ast::Id => box_once(Ok(cv.1)),
            Ast::Recurse => match cv.1 {
                (v, Some(p)) => {
                    let all = recurse_run((v, p), &|(v, p): (D::V<'a>, RcList<D::V<'a>>)| {
                        v.key_values().map(move |r| r.map(|(k, v_)| (v_, p.clone().cons(k))))
                    });
                    guard_iter(Box::new(all.map(|r| r.map(|(v, p)| (v, Some(p))))))
                }
                (v, None) => {
                    // `..` é `recurse(.[]?)`: sai o próprio valor e o `.[]?` seguinte falha.
                    let err = Error::path_iter(&v);
                    Box::new(core::iter::once(Ok((v, None))).chain(core::iter::once(Err(Exn::from(err)))))
                }
            },
            Ast::Pipe(l, None, r) => {
                flat_map_then_with(l.paths((cv.0.clone(), cv.1)), cv.0, move |y, ctx| {
                    r.paths((ctx, y))
                })
            }
            Ast::Pipe(l, Some(pat), r) => {
                flat_map_then_with(l.run(proj_cv(&cv)), cv, move |y, cv| {
                    bind_run(pat, r, cv, y, |f, cv| f.paths(cv))
                })
            }
            Ast::PipeAlt(l, alts, r) => {
                flat_map_then_with(l.run(proj_cv(&cv)), cv, move |y, cv| {
                    alt_run(alts, 0, r, cv, y, |f, cv| f.paths(cv))
                })
            }
            Ast::Comma(l, r) => Box::new(l.paths(cv.clone()).chain(lazy(|| r.paths(cv)))),
            Ast::Alt(l, r) => {
                let any_true = l
                    .run(proj_cv(&cv))
                    .any(|v| v.as_ref().map_or(true, ValT::as_bool));
                if any_true { l } else { r }.paths(cv)
            }
            Ast::Ite(if_, then_, else_) => {
                flat_map_then_with(if_.run(proj_cv(&cv)), cv, move |v, cv| {
                    if v.as_bool() { then_ } else { else_ }.paths(cv)
                })
            }
            Ast::TryCatch(f, c) => {
                let input = cv.1.clone();
                try_catch_run(f.paths((cv.0.clone(), cv.1)), move |e| {
                    mark(c.run((cv.0.clone(), e.into_val())), input.clone())
                })
            }
            Ast::Path(f, path) => {
                let keyin = cv.1 .0.clone();
                path_term(f, &path.0, cv, keyin, |f, cv| f.paths(cv), |part, opt, vp| part.paths_opt(opt, vp))
            }
            Ast::Var(v) => match cv.0.vars.get(*v).unwrap() {
                Bind::Var(x) => {
                    let (v, p) = cv.1;
                    let keep = p.is_some() && x.identical(&v);
                    box_once(Ok((x.clone(), if keep { p } else { None })))
                }
                Bind::Fun(l) => l.0.paths((cv.0.with_vars(l.1.clone()), cv.1)),
                Bind::Label(l) => box_once(Err(Exn(exn::Inner::Break(*l)))),
            },
            Ast::Fold(xs, pat, init, update, fold_type) => {
                let xs = rc_lazy_list::List::from_iter(run_and_bind(xs, proj_cv(&cv), pat));
                fold_jq(xs, cv, init, update, fold_type, |f, cv| f.paths(cv), || (D::V::null(), None))
            }
            Ast::CallDef(id, args, skip, call_typ) => {
                let data = cv.0.data.clone();
                let with_vars = move |vars| Ctx {
                    vars,
                    data: data.clone(),
                    labels: cv.0.labels,
                };
                let cvs = bind_vars(args, cv.0.clone().skip_vars(*skip), cv, proj_val);
                let (into, from) = (exn::CallInput::Paths, exn::CallInput::unwrap_paths);
                guarded(|| guard_iter(def_run(id, call_typ, cvs, Id::paths, with_vars, into, from)))
            }
            Ast::Label(id) => label_run(cv, |cv| id.paths(cv)),
            Ast::Native(id, args) => {
                let cvs = bind_vars(args, cv.0.with_vars(Vars::new([])), cv, proj_val);
                flat_map_then(cvs, |cv| {
                    let native = &cv.0.lut().funs[*id];
                    match native.paths {
                        Some(paths) => paths(cv),
                        None => {
                            let vp = cv.1.clone();
                            mark((native.run)((cv.0, cv.1 .0)), vp)
                        }
                    }
                })
            }
        }
    }
}
