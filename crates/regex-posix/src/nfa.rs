//! Motor próprio: um NFA com a forma da árvore do glibc e três buscas sobre ele.
//!
//! - [`Prog::find`] e [`Prog::longest_end`]: leftmost-longest por simulação de Thompson/Pike
//!   (tempo linear), pra quando o caminho rápido do `regex-automata` não serve (fronteira de
//!   palavra Unicode com texto não ASCII, `not_bol`/`not_eol`).
//! - [`Prog::longest_end_backref`]: busca exaustiva da casada mais longa com referências.
//! - [`Prog::groups`]: os submatches de uma casada já conhecida, reproduzindo o `set_regs` e o
//!   `update_regs` do `regexec.c`: em cada bifurcação vale o destino de menor índice de nó que
//!   ainda leva ao fim da casada (alternação: ramo da esquerda; repetição: mais uma volta; ramo
//!   vazio de `(|a)` perde pro não vazio), a volta vazia de um grupo opcional devolve os registros
//!   anteriores (`(a*)*` em `aa` dá o grupo `aa`), e a repetição `{n,m}` é expandida como o
//!   `parse_dup_op` expande.

use std::sync::Arc;

use crate::ast::{Assertion, Node, Unit};
use crate::charclass::{CaseMode, CharSet, any_set, bracket_set, is_word_char, literal_set, without_separator};
use crate::error::ErrorCode;
use crate::parse::to_upper;

pub type Pc = u32;

/// Limite de instruções depois da expansão das repetições.
const MAX_INSTS: usize = 1 << 22;

/// A cada quantos passos as buscas chamam o gancho de checkpoint.
const CHECKPOINT_EVERY: u32 = 1 << 14;

#[derive(Clone, Debug)]
pub enum Inst {
    Char { set: u32, next: Pc },
    Assert { look: Assertion, next: Pc },
    Open { group: usize, next: Pc },
    Close { group: usize, opt: bool, next: Pc },
    /// Alternação ou repetição: `first` tem prioridade (menor índice de nó no glibc).
    Split { first: Pc, second: Pc },
    Backref { group: usize, next: Pc },
    Match,
}

/// Opções de execução (`eflags` do `regexec`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ExecFlags {
    /// `REG_NOTBOL`: o começo do texto não é começo de linha.
    pub not_bol: bool,
    /// `REG_NOTEOL`: o fim do texto não é fim de linha.
    pub not_eol: bool,
}

pub type Hook = Arc<dyn Fn() + Send + Sync>;

/// Um registro de grupo (`regmatch_t`): -1 é "não participou".
pub type Reg = (isize, isize);

pub struct Prog {
    pub insts: Vec<Inst>,
    sets: Vec<CharSet>,
    pub start: Pc,
    pub nsub: usize,
    pub has_backref: bool,
    /// Caixa da comparação das referências.
    case: CaseMode,
    /// `^`/`$` casam junto de newline (ou do separador).
    anchor_nl: bool,
    nl: u8,
    separator: bool,
    hook: Option<Hook>,
}

/// O que muda a compilação do programa.
#[derive(Clone, Copy, Debug)]
pub struct ProgOptions {
    pub case: CaseMode,
    pub dot_newline: bool,
    pub dot_not_null: bool,
    pub newline_anchor: bool,
    pub separator: Option<u8>,
}

struct Compiler {
    insts: Vec<Inst>,
    sets: Vec<CharSet>,
    o: ProgOptions,
}

impl Compiler {
    fn push(&mut self, i: Inst) -> Result<Pc, ErrorCode> {
        if self.insts.len() >= MAX_INSTS {
            return Err(ErrorCode::Space);
        }
        self.insts.push(i);
        Ok((self.insts.len() - 1) as Pc)
    }

    fn set(&mut self, cs: CharSet) -> u32 {
        let cs = match self.o.separator {
            Some(sep) => without_separator(cs, sep),
            None => cs,
        };
        self.sets.push(cs);
        (self.sets.len() - 1) as u32
    }

    /// Compila `node` seguido de `next`; devolve a entrada. `opt` marca o `Close` do grupo de
    /// índice `opt` como opcional (`mark_opt_subexp`).
    fn comp(&mut self, node: &Node, next: Pc, opt: Option<usize>) -> Result<Pc, ErrorCode> {
        Ok(match node {
            Node::Empty => next,
            Node::Lit(u) => {
                let s = self.set(literal_set(*u, self.o.case));
                self.push(Inst::Char { set: s, next })?
            }
            Node::Any => {
                let s = self.set(any_set(self.o.dot_newline, self.o.dot_not_null));
                self.push(Inst::Char { set: s, next })?
            }
            Node::Set(set) => {
                let s = self.set(bracket_set(set, self.o.case));
                self.push(Inst::Char { set: s, next })?
            }
            Node::Assert(a) => self.push(Inst::Assert { look: *a, next })?,
            Node::Group { index, inner } => {
                let close = self.push(Inst::Close { group: *index, opt: opt == Some(*index), next })?;
                let body = self.comp(inner, close, None)?;
                self.push(Inst::Open { group: *index, next: body })?
            }
            Node::Concat(items) => {
                let mut cur = next;
                for item in items.iter().rev() {
                    cur = self.comp(item, cur, None)?;
                }
                cur
            }
            Node::Alt(branches) => {
                // A ordem das bifurcações do glibc: na árvore binária aninhada à esquerda, um
                // primeiro ramo vazio perde pro segundo (o destino vazio é o `next`, de índice maior).
                let mut order: Vec<&Node> = branches.iter().collect();
                if order.len() >= 2 && order[0] == &Node::Empty && order[1] != &Node::Empty {
                    order.swap(0, 1);
                }
                let mut entries = Vec::with_capacity(order.len());
                for b in &order {
                    entries.push(self.comp(b, next, None)?);
                }
                let mut cur = *entries.last().unwrap_or(&next);
                for &e in entries.iter().rev().skip(1) {
                    cur = self.push(Inst::Split { first: e, second: cur })?;
                }
                cur
            }
            Node::Repeat { inner, min, max } => self.repeat(inner, *min, *max, next)?,
            Node::Backref(g) => self.push(Inst::Backref { group: *g, next })?,
        })
    }

    /// Expansão do `parse_dup_op`: `e{n,m}` é `e...e` (n cópias) seguido de `((e?)e)?...` com as
    /// `m-n` cópias opcionais, ou de `e*` se `m` é infinito. As cópias opcionais de um grupo têm o
    /// `Close` marcado como opcional.
    fn repeat(&mut self, inner: &Node, min: u32, max: Option<u32>, next: Pc) -> Result<Pc, ErrorCode> {
        if max == Some(0) {
            return Ok(next);
        }
        let mark = match inner {
            Node::Group { index, .. } => Some(*index),
            _ => None,
        };
        let opt_entry = match max {
            None => {
                let split = self.push(Inst::Split { first: 0, second: next })?;
                let body = self.comp(inner, split, mark)?;
                self.insts[split as usize] = Inst::Split { first: body, second: next };
                split
            }
            Some(m) if m > min => {
                let k = (m - min) as usize;
                // c[0] = next, c[i] = e seguido de c[i-1].
                let mut c = Vec::with_capacity(k + 1);
                c.push(next);
                for i in 1..=k {
                    let e = self.comp(inner, c[i - 1], mark)?;
                    c.push(e);
                }
                let mut s = self.push(Inst::Split { first: c[k], second: c[k - 1] })?;
                for j in 2..=k {
                    s = self.push(Inst::Split { first: s, second: c[k - j] })?;
                }
                s
            }
            Some(_) => next,
        };
        let mut entry = opt_entry;
        for _ in 0..min {
            entry = self.comp(inner, entry, None)?;
        }
        Ok(entry)
    }
}

impl Prog {
    pub fn compile(root: &Node, nsub: usize, o: ProgOptions, hook: Option<Hook>) -> Result<Prog, ErrorCode> {
        let mut c = Compiler { insts: Vec::new(), sets: Vec::new(), o };
        let m = c.push(Inst::Match)?;
        let start = c.comp(root, m, None)?;
        Ok(Prog {
            insts: c.insts,
            sets: c.sets,
            start,
            nsub,
            has_backref: crate::ast::has_backref(root),
            case: o.case,
            anchor_nl: o.newline_anchor || o.separator.is_some(),
            nl: o.separator.unwrap_or(b'\n'),
            separator: o.separator.is_some(),
            hook,
        })
    }

    fn tick(&self, n: &mut u32) {
        *n += 1;
        if *n >= CHECKPOINT_EVERY {
            *n = 0;
            if let Some(h) = &self.hook {
                h();
            }
        }
    }

    /// Testa o conjunto `set` em `pos`; devolve o tamanho da unidade casada.
    fn test(&self, set: u32, hay: &[u8], pos: usize) -> Option<usize> {
        let (u, len) = decode_at(hay, pos)?;
        self.sets[set as usize].contains_unit(u).then_some(len)
    }

    fn look(&self, a: Assertion, hay: &[u8], pos: usize, f: ExecFlags) -> bool {
        let prev_nl = if pos == 0 { !f.not_bol } else { self.anchor_nl && hay[pos - 1] == self.nl };
        let next_nl = if pos >= hay.len() { !f.not_eol } else { self.anchor_nl && hay[pos] == self.nl };
        let prev_w = || pos > 0 && decode_before(hay, pos).is_some_and(word_unit);
        let next_w = || decode_at(hay, pos).is_some_and(|(u, _)| word_unit(u));
        match a {
            Assertion::LineStart => prev_nl,
            Assertion::LineEnd => next_nl,
            Assertion::BufStart if self.separator => prev_nl,
            Assertion::BufStart => pos == 0,
            Assertion::BufEnd if self.separator => next_nl,
            Assertion::BufEnd => pos >= hay.len(),
            Assertion::WordStart => !prev_w() && next_w(),
            Assertion::WordEnd => prev_w() && !next_w(),
            Assertion::WordBoundary => prev_w() != next_w(),
            Assertion::NotWordBoundary => prev_w() == next_w(),
        }
    }

    /// Fecho-épsilon de `pc` em `pos` dentro de `list`, com o início `start` (vale o primeiro a chegar).
    fn closure(&self, list: &mut Threads, (pc, start): (Pc, usize), hay: &[u8], pos: usize, f: ExecFlags, stack: &mut Vec<Pc>) {
        stack.push(pc);
        while let Some(pc) = stack.pop() {
            if !list.insert(pc, start) {
                continue;
            }
            match self.insts[pc as usize] {
                Inst::Split { first, second } => {
                    stack.push(second);
                    stack.push(first);
                }
                Inst::Assert { look, next } => {
                    if self.look(look, hay, pos, f) {
                        stack.push(next);
                    }
                }
                Inst::Open { next, .. } | Inst::Close { next, .. } => stack.push(next),
                Inst::Backref { .. } | Inst::Char { .. } | Inst::Match => {}
            }
        }
    }

    /// Fim da casada mais longa que começa exatamente em `s` (`re_match`). Sem referências.
    pub fn longest_end(&self, hay: &[u8], s: usize, f: ExecFlags) -> Option<usize> {
        if self.has_backref {
            return self.longest_end_backref(hay, s, hay.len(), f);
        }
        let n = self.insts.len();
        let (mut clist, mut nlist) = (Threads::new(n), Threads::new(n));
        let mut stack = Vec::new();
        let mut pos = s;
        let mut best = None;
        let mut ticks = 0;
        self.closure(&mut clist, (self.start, s), hay, pos, f, &mut stack);
        loop {
            self.tick(&mut ticks);
            if clist.pcs().any(|pc| matches!(self.insts[pc as usize], Inst::Match)) {
                best = Some(pos);
            }
            if pos >= hay.len() || clist.is_empty() {
                break;
            }
            let Some((_, len)) = decode_at(hay, pos) else { break };
            nlist.clear();
            for i in 0..clist.len() {
                let pc = clist.at(i);
                if let Inst::Char { set, next } = self.insts[pc as usize]
                    && self.test(set, hay, pos).is_some()
                {
                    self.closure(&mut nlist, (next, s), hay, pos + len, f, &mut stack);
                }
            }
            pos += len;
            std::mem::swap(&mut clist, &mut nlist);
        }
        best
    }

    /// Casada leftmost-longest que começa em `start` ou depois.
    pub fn find(&self, hay: &[u8], start: usize, f: ExecFlags) -> Option<(usize, usize)> {
        if self.has_backref {
            return self.find_backref(hay, start, f, None);
        }
        let n = self.insts.len();
        let (mut clist, mut nlist) = (Threads::new(n), Threads::new(n));
        let mut stack = Vec::new();
        let mut best: Option<(usize, usize)> = None;
        let mut pos = align(hay, start);
        let mut ticks = 0;
        loop {
            self.tick(&mut ticks);
            if best.is_none() {
                self.closure(&mut clist, (self.start, pos), hay, pos, f, &mut stack);
            }
            for i in 0..clist.len() {
                let pc = clist.at(i);
                if matches!(self.insts[pc as usize], Inst::Match) {
                    let s = clist.start_of(pc);
                    best = match best {
                        Some((bs, be)) if bs < s || (bs == s && be >= pos) => Some((bs, be)),
                        _ => Some((s, pos)),
                    };
                }
            }
            if pos >= hay.len() {
                break;
            }
            let alive = match best {
                Some((bs, _)) => clist.pcs().any(|pc| clist.start_of(pc) <= bs),
                None => true,
            };
            if !alive {
                break;
            }
            let Some((_, len)) = decode_at(hay, pos) else { break };
            nlist.clear();
            for i in 0..clist.len() {
                let pc = clist.at(i);
                let s = clist.start_of(pc);
                if best.is_some_and(|(bs, _)| s > bs) {
                    continue;
                }
                if let Inst::Char { set, next } = self.insts[pc as usize]
                    && self.test(set, hay, pos).is_some()
                {
                    self.closure(&mut nlist, (next, s), hay, pos + len, f, &mut stack);
                }
            }
            pos += len;
            std::mem::swap(&mut clist, &mut nlist);
        }
        best
    }

    /// Leftmost-longest com referências: em cada início possível (a partir de `start`, filtrado
    /// por `candidates` quando há), a busca exaustiva da casada mais longa.
    pub fn find_backref(
        &self,
        hay: &[u8],
        start: usize,
        f: ExecFlags,
        candidates: Option<&dyn Fn(usize) -> Option<usize>>,
    ) -> Option<(usize, usize)> {
        let mut p = align(hay, start);
        loop {
            if let Some(next_candidate) = candidates {
                p = next_candidate(p)?;
            }
            if let Some(e) = self.longest_end_backref(hay, p, hay.len(), f) {
                return Some((p, e));
            }
            if p >= hay.len() {
                return None;
            }
            p += decode_at(hay, p).map(|(_, l)| l).unwrap_or(1);
        }
    }

    /// Fim mais longo (até `limit`) de uma casada que começa em `s`, explorando todos os caminhos
    /// (necessário com referências).
    pub fn longest_end_backref(&self, hay: &[u8], s: usize, limit: usize, f: ExecFlags) -> Option<usize> {
        let mut best: Option<usize> = None;
        self.dfs(hay, s, limit, f, Mode::Longest, &mut |pos, _| {
            if best.is_none_or(|b| pos > b) {
                best = Some(pos);
            }
            pos >= limit
        });
        best
    }

    /// Submatches de uma casada de `s` a `e`, com as escolhas do glibc. `None` se o programa não
    /// consegue terminar exatamente em `e` (não deve acontecer).
    pub fn groups(&self, hay: &[u8], s: usize, e: usize, f: ExecFlags) -> Option<Vec<Reg>> {
        // O glibc clona os nós alcançados por épsilon depois de uma âncora
        // (`duplicate_node_closure`), e o `set_regs` só aceita terminar no nó final de menor
        // índice que vale ali: o original, se algum caminho chega ao fim sem âncora na última
        // sequência de épsilons. É por isso que `(^)*` deixa o grupo de fora. Primeiro tenta só
        // esses caminhos; se não há, qualquer um.
        for mode in [Mode::GroupsPlainHalt, Mode::Groups] {
            let mut out = None;
            self.dfs(hay, s, e, f, mode, &mut |_, regs| {
                out = Some(regs.to_vec());
                true
            });
            if out.is_some() {
                return out;
            }
        }
        None
    }

    /// Busca em profundidade com pilha de falhas (`re_fail_stack` do glibc). Em `Mode::Groups` só
    /// aceita terminar em `limit` e para na primeira; em `Mode::Longest` visita todas as casadas.
    /// `on_match` devolve `true` pra parar.
    fn dfs(&self, hay: &[u8], s: usize, limit: usize, f: ExecFlags, mode: Mode, on_match: &mut dyn FnMut(usize, &[Reg]) -> bool) {
        let n = self.nsub + 1;
        let mut regs: Vec<Reg> = vec![(-1, -1); n];
        regs[0] = (s as isize, limit as isize);
        let mut prev = regs.clone();
        let mut eps: Vec<Pc> = Vec::new();
        let mut stack: Vec<Frame> = Vec::new();
        let span = limit.saturating_sub(s) + 1;
        let mut memo = (!self.has_backref).then(|| Memo::new(self.insts.len(), span));
        let groups_mode = mode != Mode::Longest;
        let plain_halt = mode == Mode::GroupsPlainHalt;
        let mut pc = self.start;
        let mut pos = s;
        let mut ticks = 0;
        loop {
            self.tick(&mut ticks);
            // Peculiaridade do set_regs com referências: voltar a um nó épsilon já visitado na
            // mesma posição encerra o percurso (com sucesso se nenhum grupo ficou aberto).
            if groups_mode && self.has_backref && eps.contains(&pc) {
                // O glibc atualiza os registros do nó antes de testar a parada.
                match self.insts[pc as usize] {
                    Inst::Open { group, .. } | Inst::Close { group, .. } if group < n => {
                        update_regs(&self.insts[pc as usize], pos, &mut regs, &mut prev);
                    }
                    _ => {}
                }
                if regs.iter().any(|&(a, b)| a > -1 && b == -1) {
                    match pop(&mut stack, &mut pc, &mut pos, &mut regs, &mut prev, &mut eps) {
                        true => continue,
                        false => return,
                    }
                }
                if on_match(pos, &regs) {
                    return;
                }
                if !pop(&mut stack, &mut pc, &mut pos, &mut regs, &mut prev, &mut eps) {
                    return;
                }
                continue;
            }
            let advanced: bool = match self.insts[pc as usize] {
                Inst::Match => {
                    let halt_ok = !plain_halt
                        || !eps.iter().any(|&p| matches!(self.insts[p as usize], Inst::Assert { .. }));
                    if !groups_mode || (pos == limit && halt_ok) {
                        regs[0].1 = pos as isize;
                        if on_match(pos, &regs) {
                            return;
                        }
                    }
                    false
                }
                Inst::Char { set, next } => match self.test(set, hay, pos) {
                    Some(len) if pos + len <= limit => {
                        pos += len;
                        eps.clear();
                        pc = next;
                        match &mut memo {
                            Some(m) => m.first_visit(pc, pos - s),
                            None => true,
                        }
                    }
                    _ => false,
                },
                Inst::Assert { look, next } => {
                    mark(&mut eps, pc);
                    if self.look(look, hay, pos, f) {
                        pc = next;
                        true
                    } else {
                        false
                    }
                }
                Inst::Open { group, next } | Inst::Close { group, next, .. } => {
                    if group < n {
                        update_regs(&self.insts[pc as usize], pos, &mut regs, &mut prev);
                    }
                    mark(&mut eps, pc);
                    pc = next;
                    true
                }
                Inst::Split { first, second } => {
                    mark(&mut eps, pc);
                    if eps.contains(&first) {
                        pc = second;
                    } else {
                        stack.push(Frame { pc: second, pos, regs: regs.clone(), prev: prev.clone(), eps: eps.clone() });
                        pc = first;
                    }
                    true
                }
                Inst::Backref { group, next } => {
                    let (gs, ge) = regs.get(group).copied().unwrap_or((-1, -1));
                    if gs < 0 || ge < 0 {
                        false
                    } else {
                        let (gs, ge) = (gs as usize, ge as usize);
                        let len = ge - gs;
                        if len == 0 {
                            mark(&mut eps, pc);
                            pc = next;
                            true
                        } else if pos + len <= limit && self.same_text(hay, gs, ge, pos, len) {
                            pos += len;
                            eps.clear();
                            pc = next;
                            true
                        } else {
                            false
                        }
                    }
                }
            };
            if advanced {
                continue;
            }
            if !pop(&mut stack, &mut pc, &mut pos, &mut regs, &mut prev, &mut eps) {
                return;
            }
        }
    }

    /// Compara o texto do grupo com o texto em `pos` (com `RE_ICASE`, em maiúsculas, como o glibc
    /// compara o buffer traduzido).
    fn same_text(&self, hay: &[u8], gs: usize, ge: usize, pos: usize, len: usize) -> bool {
        let a = &hay[gs..ge];
        let b = &hay[pos..pos + len];
        if self.case == CaseMode::Sensitive {
            return a == b;
        }
        let (mut i, mut j) = (0, 0);
        while i < a.len() && j < b.len() {
            let (Some((ua, la)), Some((ub, lb))) = (decode_at(a, i), decode_at(b, j)) else { return false };
            let up = |u: Unit| match u {
                Unit::Char(c) => Unit::Char(to_upper(c)),
                x => x,
            };
            if up(ua) != up(ub) {
                return false;
            }
            i += la;
            j += lb;
        }
        i == a.len() && j == b.len()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Submatches, só caminhos sem âncora entre o último caractere e o fim.
    GroupsPlainHalt,
    /// Submatches, qualquer caminho.
    Groups,
    /// Todas as casadas (fim mais longo com referências).
    Longest,
}

struct Frame {
    pc: Pc,
    pos: usize,
    regs: Vec<Reg>,
    prev: Vec<Reg>,
    eps: Vec<Pc>,
}

fn pop(stack: &mut Vec<Frame>, pc: &mut Pc, pos: &mut usize, regs: &mut Vec<Reg>, prev: &mut Vec<Reg>, eps: &mut Vec<Pc>) -> bool {
    match stack.pop() {
        Some(fr) => {
            *pc = fr.pc;
            *pos = fr.pos;
            *regs = fr.regs;
            *prev = fr.prev;
            *eps = fr.eps;
            true
        }
        None => false,
    }
}

/// `update_regs` do `regexec.c` pra um nó `Open` ou `Close`.
fn update_regs(inst: &Inst, pos: usize, regs: &mut [Reg], prev: &mut Vec<Reg>) {
    let cur = pos as isize;
    match *inst {
        Inst::Open { group, .. } => regs[group] = (cur, -1),
        Inst::Close { group, opt, .. } => {
            if regs[group].0 < cur {
                regs[group].1 = cur;
                prev.clear();
                prev.extend_from_slice(regs);
            } else if opt && prev[group].0 != -1 {
                // Volta vazia de grupo opcional que já casou antes: desfaz, inclusive os grupos de
                // dentro (`((a?))*`).
                regs.copy_from_slice(prev);
            } else {
                regs[group].1 = cur;
            }
        }
        _ => {}
    }
}

fn mark(eps: &mut Vec<Pc>, pc: Pc) {
    if !eps.contains(&pc) {
        eps.push(pc);
    }
}

fn word_unit(u: Unit) -> bool {
    match u {
        Unit::Char(c) => is_word_char(c),
        Unit::Byte(_) => false,
    }
}

/// Unidade em `pos`: caractere UTF-8 válido ou byte solto.
pub fn decode_at(hay: &[u8], pos: usize) -> Option<(Unit, usize)> {
    let b0 = *hay.get(pos)?;
    if b0 < 0x80 {
        return Some((Unit::Char(b0 as char), 1));
    }
    let width = match b0 {
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return Some((Unit::Byte(b0), 1)),
    };
    let end = pos + width;
    if end <= hay.len()
        && let Ok(s) = std::str::from_utf8(&hay[pos..end])
        && let Some(c) = s.chars().next()
    {
        return Some((Unit::Char(c), width));
    }
    Some((Unit::Byte(b0), 1))
}

/// Unidade que termina em `pos`.
pub fn decode_before(hay: &[u8], pos: usize) -> Option<Unit> {
    if pos == 0 {
        return None;
    }
    let last = hay[pos - 1];
    if last < 0x80 {
        return Some(Unit::Char(last as char));
    }
    for back in 2..=4.min(pos) {
        let start = pos - back;
        if let Some((u @ Unit::Char(_), len)) = decode_at(hay, start)
            && start + len == pos
        {
            return Some(u);
        }
    }
    Some(Unit::Byte(last))
}

/// Se `pos` cai no meio de um caractere UTF-8 válido, avança pro fim dele (o glibc só começa
/// casadas no primeiro byte de um caractere).
pub fn align(hay: &[u8], pos: usize) -> usize {
    if pos == 0 || pos >= hay.len() || hay[pos] & 0xc0 != 0x80 {
        return pos;
    }
    for back in 1..=3.min(pos) {
        let start = pos - back;
        if let Some((Unit::Char(_), len)) = decode_at(hay, start)
            && start + len > pos
        {
            return start + len;
        }
    }
    pos
}

/// Conjunto de threads com o início de cada uma (o primeiro a chegar fica).
struct Threads {
    dense: Vec<Pc>,
    sparse: Vec<u32>,
    starts: Vec<usize>,
}

impl Threads {
    fn new(n: usize) -> Threads {
        Threads { dense: Vec::with_capacity(n), sparse: vec![u32::MAX; n], starts: vec![0; n] }
    }

    fn insert(&mut self, pc: Pc, start: usize) -> bool {
        let i = self.sparse[pc as usize];
        if (i as usize) < self.dense.len() && self.dense[i as usize] == pc {
            return false;
        }
        self.sparse[pc as usize] = self.dense.len() as u32;
        self.dense.push(pc);
        self.starts[pc as usize] = start;
        true
    }

    fn clear(&mut self) {
        self.dense.clear();
    }

    fn len(&self) -> usize {
        self.dense.len()
    }

    fn is_empty(&self) -> bool {
        self.dense.is_empty()
    }

    fn at(&self, i: usize) -> Pc {
        self.dense[i]
    }

    fn start_of(&self, pc: Pc) -> usize {
        self.starts[pc as usize]
    }

    fn pcs(&self) -> impl Iterator<Item = Pc> + '_ {
        self.dense.iter().copied()
    }
}

/// Estados (instrução, posição) já explorados sem sucesso logo depois de consumir um caractere.
/// Sem referências, o sucesso a partir daí não depende do caminho, então basta visitar uma vez.
struct Memo {
    width: usize,
    bits: Option<Vec<u64>>,
    set: std::collections::HashSet<(Pc, usize)>,
}

impl Memo {
    fn new(insts: usize, span: usize) -> Memo {
        let total = insts.saturating_mul(span);
        let bits = (total <= 1 << 26).then(|| vec![0u64; total.div_ceil(64)]);
        Memo { width: insts, bits, set: Default::default() }
    }

    /// `true` na primeira visita.
    fn first_visit(&mut self, pc: Pc, rel: usize) -> bool {
        match &mut self.bits {
            Some(bits) => {
                let i = rel * self.width + pc as usize;
                let (w, b) = (i / 64, i % 64);
                if bits[w] & (1 << b) != 0 {
                    return false;
                }
                bits[w] |= 1 << b;
                true
            }
            None => self.set.insert((pc, rel)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::decode;
    use crate::parse::{View, parse};
    use crate::syntax::Syntax;

    fn prog(p: &str, syn: Syntax) -> Prog {
        let parsed = parse(&decode(p.as_bytes()), syn, View::Glibc).unwrap();
        let o = ProgOptions {
            case: CaseMode::Sensitive,
            dot_newline: true,
            dot_not_null: false,
            newline_anchor: false,
            separator: None,
        };
        Prog::compile(&parsed.root, parsed.nsub, o, None).unwrap()
    }

    fn groups(p: &str, syn: Syntax, hay: &str) -> Vec<String> {
        let pr = prog(p, syn);
        let (s, e) = pr.find(hay.as_bytes(), 0, ExecFlags::default()).expect("casa");
        let regs = pr.groups(hay.as_bytes(), s, e, ExecFlags::default()).expect("grupos");
        regs.iter()
            .map(|&(a, b)| if a < 0 || b < 0 { "-".to_string() } else { hay[a as usize..b as usize].to_string() })
            .collect()
    }

    #[test]
    fn leftmost_longest() {
        let pr = prog("a|ab", Syntax::EGREP);
        assert_eq!(pr.find(b"xab", 0, ExecFlags::default()), Some((1, 3)));
        let pr = prog("x*", Syntax::EGREP);
        assert_eq!(pr.find(b"abc", 0, ExecFlags::default()), Some((0, 0)));
        let pr = prog("b+", Syntax::EGREP);
        assert_eq!(pr.find(b"abbbc", 0, ExecFlags::default()), Some((1, 4)));
        assert_eq!(pr.find(b"abbbc", 4, ExecFlags::default()), None);
    }

    #[test]
    fn glibc_submatch_rules() {
        // Respostas do glibc 2.41 (sondas sed-sub e gawk do F01).
        assert_eq!(groups("(a|ab)(c|bcd)(d*)", Syntax::EGREP, "abcd"), ["abcd", "a", "bcd", ""]);
        assert_eq!(groups("(a|ab)(bc|c)?", Syntax::EGREP, "ab"), ["ab", "ab", "-"]);
        assert_eq!(groups("\\(a*\\)\\(ab\\)*\\(b*\\)", Syntax::GREP, "abab"), ["abab", "", "ab", ""]);
        assert_eq!(groups("(a*)*", Syntax::EGREP, "aa"), ["aa", "aa"]);
        assert_eq!(groups("(a*)*", Syntax::EGREP, "b"), ["", ""]);
        assert_eq!(groups("(wee|week)(knights|night)", Syntax::EGREP, "weeknights"), ["weeknights", "wee", "knights"]);
        assert_eq!(groups("(.*)(.*)", Syntax::EGREP, "abc"), ["abc", "abc", ""]);
        assert_eq!(groups("(a)|(b)", Syntax::EGREP, "b"), ["b", "-", "b"]);
    }

    #[test]
    fn backrefs() {
        let pr = prog("\\(.\\)\\1", Syntax::GREP);
        assert_eq!(pr.find(b"hello", 0, ExecFlags::default()), Some((2, 4)));
        let pr = prog("^\\(.*\\)\\1$", Syntax::GREP);
        assert_eq!(pr.find(b"abcabc", 0, ExecFlags::default()), Some((0, 6)));
        assert_eq!(pr.find(b"abca", 0, ExecFlags::default()), None);
    }

    #[test]
    fn utf8_helpers() {
        let s = "aé".as_bytes();
        assert_eq!(decode_at(s, 1), Some((Unit::Char('é'), 2)));
        assert_eq!(decode_before(s, 3), Some(Unit::Char('é')));
        assert_eq!(align(s, 2), 3);
        assert_eq!(decode_at(b"\xff", 0), Some((Unit::Byte(0xff), 1)));
    }
}
