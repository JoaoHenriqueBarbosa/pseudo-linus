//! `match` (PEP 634): `compiler_match` e os `compiler_pattern_*` do 3.13, mais o dobramento de `x is None` em
//! `POP_JUMP_IF_[NOT_]NONE` do `flowgraph.c`.
//!
//! Cada caso copia o sujeito (menos o último), compila o padrão (que consome a cópia ou deixa as capturas na pilha),
//! grava os nomes capturados, avalia a guarda e roda o corpo. Quando o padrão falha, `fail_pop[n]` descarta os `n`
//! itens que ele ainda deixou na pilha antes de cair no caso seguinte.

use super::*;
use crate::ast::{MatchCase, Pattern, PatternKind as P};

const GET_LEN: u16 = 20;
const MATCH_KEYS: u16 = 27;
const MATCH_MAPPING: u16 = 28;
const MATCH_SEQUENCE: u16 = 29;
const MATCH_CLASS: u16 = 96;

/// `pattern_context`: o estado da compilação de um padrão.
struct PatCtx {
    /// Nomes capturados até agora, na ordem em que seus valores estão na pilha (do topo para baixo, invertida).
    stores: Vec<String>,
    /// Itens que o padrão em curso mantém no topo da pilha.
    on_top: i64,
    /// Os alvos de falha: `fail_pop[n]` descarta `n` itens e cai no caso seguinte.
    fail_pop: Vec<usize>,
}

/// `WILDCARD_CHECK`: `_`.
fn is_wildcard(p: &Pattern) -> bool {
    matches!(&p.kind, P::MatchAs { pattern: None, name: None })
}

/// `WILDCARD_STAR_CHECK`: `*_`.
fn is_star_wildcard(p: &Pattern) -> bool {
    matches!(&p.kind, P::MatchStar { name: None })
}

impl Gen<'_> {
    fn pat_loc(&self, p: &Pattern) -> Loc {
        let pos = Pos {
            lineno: p.pos.lineno,
            col_offset: p.pos.col_offset,
            end_lineno: Some(p.pos.end_lineno),
            end_col_offset: Some(p.pos.end_col_offset),
        };
        self.loc(&pos)
    }

    /// `ensure_fail_pop`: garante os alvos de falha de 0 a `n`.
    fn ensure_fail_pop(&mut self, pc: &mut PatCtx, n: usize) {
        while pc.fail_pop.len() < n + 1 {
            let b = self.cfg.new_block();
            pc.fail_pop.push(b);
        }
    }

    /// `jump_to_fail_pop`: salta descartando o que o padrão deixou no topo e o que ele ia capturar.
    fn jump_to_fail_pop(&mut self, pc: &mut PatCtx, loc: Loc, op: u16) {
        let pops = (pc.on_top.max(0) as usize) + pc.stores.len();
        self.ensure_fail_pop(pc, pops);
        let target = pc.fail_pop[pops];
        self.add_jump(op, target, loc);
    }

    /// `emit_and_reset_fail_pop`: um `POP_TOP` por alvo de falha, do mais fundo ao mais raso.
    fn emit_and_reset_fail_pop(&mut self, pc: &mut PatCtx, loc: Loc) {
        let size = pc.fail_pop.len();
        if size == 0 {
            return;
        }
        for k in (1..size).rev() {
            self.cfg.use_block(pc.fail_pop[k]);
            self.add(POP_TOP, 0, loc);
        }
        self.cfg.use_block(pc.fail_pop[0]);
        pc.fail_pop.clear();
    }

    /// `pattern_helper_rotate`: os `SWAP` que levam o topo `count - 1` posições para baixo.
    fn rotate(&mut self, loc: Loc, mut count: usize) {
        while count > 1 {
            self.add(SWAP, count as i64, loc);
            count -= 1;
        }
    }

    /// `pattern_helper_store_name`: guarda o topo sob os itens que ficam e anota o nome para o fim do caso.
    fn store_pattern_name(&mut self, loc: Loc, name: Option<&str>, pc: &mut PatCtx) -> Res<()> {
        let Some(n) = name else {
            self.add(POP_TOP, 0, loc);
            return Ok(());
        };
        if pc.stores.iter().any(|s| s == n) {
            return Err(Unsupported);
        }
        let rotations = (pc.on_top.max(0) as usize) + pc.stores.len() + 1;
        self.rotate(loc, rotations);
        pc.stores.push(n.to_string());
        Ok(())
    }

    /// `compiler_match_inner`.
    pub(super) fn match_stmt(&mut self, subject: &Expr, cases: &[MatchCase]) -> Res<()> {
        self.expr(subject)?;
        let end = self.cfg.new_block();
        let n = cases.len();
        let last = cases.last().ok_or(Unsupported)?;
        let has_default = usize::from(is_wildcard(&last.pattern) && n > 1);
        for (i, m) in cases.iter().enumerate().take(n - has_default) {
            let ploc = self.pat_loc(&m.pattern);
            let not_last = i != n - has_default - 1;
            if not_last {
                self.add(COPY, 1, ploc);
            }
            let mut pc = PatCtx { stores: Vec::new(), on_top: 0, fail_pop: Vec::new() };
            self.pattern(&m.pattern, &mut pc)?;
            for name in std::mem::take(&mut pc.stores) {
                self.store_name(&name, ploc)?;
            }
            if let Some(g) = &m.guard {
                self.ensure_fail_pop(&mut pc, 0);
                let target = pc.fail_pop[0];
                self.jump_if(g, target, false)?;
            }
            if not_last {
                self.add(POP_TOP, 0, ploc);
            }
            self.stmts(&m.body)?;
            self.add_jump(JUMP, end, NO_LOC);
            self.emit_and_reset_fail_pop(&mut pc, ploc);
        }
        if has_default == 1 {
            let ploc = self.pat_loc(&last.pattern);
            self.add(NOP, 0, ploc);
            if let Some(g) = &last.guard {
                self.jump_if(g, end, false)?;
            }
            self.stmts(&last.body)?;
        }
        self.cfg.use_block(end);
        Ok(())
    }

    /// `compiler_pattern`.
    fn pattern(&mut self, p: &Pattern, pc: &mut PatCtx) -> Res<()> {
        let loc = self.pat_loc(p);
        match &p.kind {
            P::MatchValue { value } => {
                self.expr(value)?;
                self.cmp_op(CmpOp::Eq, loc)?;
                self.add(TO_BOOL, 0, loc);
                self.jump_to_fail_pop(pc, loc, POP_JUMP_IF_FALSE);
            }
            P::MatchSingleton { value } => {
                self.load_cv(const_cv(value)?, loc);
                self.add(IS_OP, 0, loc);
                self.jump_to_fail_pop(pc, loc, POP_JUMP_IF_FALSE);
            }
            P::MatchSequence { patterns } => self.pattern_sequence(loc, patterns, pc)?,
            P::MatchMapping { keys, patterns, rest } => self.pattern_mapping(loc, keys, patterns, rest.as_deref(), pc)?,
            P::MatchClass { cls, patterns, kwd_attrs, kwd_patterns } => {
                self.pattern_class(loc, cls, patterns, kwd_attrs, kwd_patterns, pc)?
            }
            P::MatchStar { name } => self.store_pattern_name(loc, name.as_deref(), pc)?,
            P::MatchAs { pattern: None, name } => self.store_pattern_name(loc, name.as_deref(), pc)?,
            P::MatchAs { pattern: Some(sub), name } => {
                pc.on_top += 1;
                self.add(COPY, 1, loc);
                self.pattern(sub, pc)?;
                pc.on_top -= 1;
                self.store_pattern_name(loc, name.as_deref(), pc)?;
            }
            P::MatchOr { patterns } => self.pattern_or(p, patterns, pc)?,
        }
        Ok(())
    }

    /// `GET_LEN`, a constante e a comparação, saltando para a falha se ela não vale.
    fn length_check(&mut self, loc: Loc, n: usize, op: CmpOp, pc: &mut PatCtx) -> Res<()> {
        self.add(GET_LEN, 0, loc);
        self.load_cv(Cv::int(n as i64), loc);
        self.cmp_op(op, loc)?;
        self.jump_to_fail_pop(pc, loc, POP_JUMP_IF_FALSE);
        Ok(())
    }

    /// `compiler_pattern_sequence`.
    fn pattern_sequence(&mut self, loc: Loc, patterns: &[Pattern], pc: &mut PatCtx) -> Res<()> {
        let size = patterns.len();
        let mut star = None;
        let mut only_wildcard = true;
        let mut star_wildcard = false;
        for (i, pat) in patterns.iter().enumerate() {
            if matches!(pat.kind, P::MatchStar { .. }) {
                if star.is_some() {
                    return Err(Unsupported);
                }
                star_wildcard = is_star_wildcard(pat);
                only_wildcard &= star_wildcard;
                star = Some(i);
                continue;
            }
            only_wildcard &= is_wildcard(pat);
        }
        pc.on_top += 1;
        self.add(MATCH_SEQUENCE, 0, loc);
        self.jump_to_fail_pop(pc, loc, POP_JUMP_IF_FALSE);
        match star {
            None => self.length_check(loc, size, CmpOp::Eq, pc)?,
            Some(_) if size > 1 => self.length_check(loc, size - 1, CmpOp::GtE, pc)?,
            Some(_) => {}
        }
        pc.on_top -= 1;
        if only_wildcard {
            self.add(POP_TOP, 0, loc);
        } else if star_wildcard {
            self.sequence_subscr(loc, patterns, star, pc)?;
        } else {
            self.sequence_unpack(loc, patterns, pc)?;
        }
        Ok(())
    }

    /// `pattern_helper_sequence_unpack`: `UNPACK_SEQUENCE` ou `UNPACK_EX` e um subpadrão por elemento.
    fn sequence_unpack(&mut self, loc: Loc, patterns: &[Pattern], pc: &mut PatCtx) -> Res<()> {
        let n = patterns.len();
        match patterns.iter().position(|x| matches!(x.kind, P::MatchStar { .. })) {
            Some(i) => self.add(UNPACK_EX, (i + ((n - i - 1) << 8)) as i64, loc),
            None => self.add(UNPACK_SEQUENCE, n as i64, loc),
        }
        pc.on_top += n as i64;
        for pat in patterns {
            pc.on_top -= 1;
            self.subpattern(pat, pc)?;
        }
        Ok(())
    }

    /// `pattern_helper_sequence_subscr`: `[a, *_, z]` lê os extremos por `BINARY_SUBSCR` em vez de desempacotar.
    fn sequence_subscr(&mut self, loc: Loc, patterns: &[Pattern], star: Option<usize>, pc: &mut PatCtx) -> Res<()> {
        let size = patterns.len();
        let star = star.ok_or(Unsupported)?;
        pc.on_top += 1;
        for (i, pat) in patterns.iter().enumerate() {
            if is_wildcard(pat) || i == star {
                continue;
            }
            self.add(COPY, 1, loc);
            if i < star {
                self.load_cv(Cv::int(i as i64), loc);
            } else {
                self.add(GET_LEN, 0, loc);
                self.load_cv(Cv::int((size - i) as i64), loc);
                self.add(BINARY_OP, nb_op(Operator::Sub), loc);
            }
            self.add(BINARY_SUBSCR, 0, loc);
            self.subpattern(pat, pc)?;
        }
        pc.on_top -= 1;
        self.add(POP_TOP, 0, loc);
        Ok(())
    }

    /// `compiler_pattern_subpattern`.
    fn subpattern(&mut self, p: &Pattern, pc: &mut PatCtx) -> Res<()> {
        self.pattern(p, pc)
    }

    /// `compiler_pattern_mapping`.
    fn pattern_mapping(&mut self, loc: Loc, keys: &[Expr], patterns: &[Pattern], rest: Option<&str>, pc: &mut PatCtx) -> Res<()> {
        let size = keys.len();
        if patterns.len() != size {
            return Err(Unsupported);
        }
        pc.on_top += 1;
        self.add(MATCH_MAPPING, 0, loc);
        self.jump_to_fail_pop(pc, loc, POP_JUMP_IF_FALSE);
        if size == 0 && rest.is_none() {
            pc.on_top -= 1;
            self.add(POP_TOP, 0, loc);
            return Ok(());
        }
        if size > 0 {
            self.length_check(loc, size, CmpOp::GtE, pc)?;
        }
        for key in keys {
            self.expr(key)?;
        }
        self.add(BUILD_TUPLE, size as i64, loc);
        self.add(MATCH_KEYS, 0, loc);
        pc.on_top += 2;
        self.add(COPY, 1, loc);
        self.load_cv(Cv::none(), loc);
        self.add(IS_OP, 1, loc);
        self.jump_to_fail_pop(pc, loc, POP_JUMP_IF_FALSE);
        self.add(UNPACK_SEQUENCE, size as i64, loc);
        pc.on_top += size as i64 - 1;
        for pat in patterns {
            pc.on_top -= 1;
            self.subpattern(pat, pc)?;
        }
        pc.on_top -= 2;
        match rest {
            Some(name) => {
                self.add(BUILD_MAP, 0, loc);
                self.add(SWAP, 3, loc);
                self.add(DICT_UPDATE, 2, loc);
                self.add(UNPACK_SEQUENCE, size as i64, loc);
                for left in (1..=size).rev() {
                    self.add(COPY, 1 + left as i64, loc);
                    self.add(SWAP, 2, loc);
                    self.add(DELETE_SUBSCR, 0, loc);
                }
                self.store_pattern_name(loc, Some(name), pc)?;
            }
            None => {
                self.add(POP_TOP, 0, loc);
                self.add(POP_TOP, 0, loc);
            }
        }
        Ok(())
    }

    /// `compiler_pattern_class`.
    fn pattern_class(
        &mut self,
        loc: Loc,
        cls: &Expr,
        patterns: &[Pattern],
        kwd_attrs: &[String],
        kwd_patterns: &[Pattern],
        pc: &mut PatCtx,
    ) -> Res<()> {
        let (nargs, nattrs) = (patterns.len(), kwd_attrs.len());
        if kwd_patterns.len() != nattrs {
            return Err(Unsupported);
        }
        self.expr(cls)?;
        self.load_cv(Cv::tuple(kwd_attrs.iter().map(|a| Cv::str(a.clone())).collect()), loc);
        self.add(MATCH_CLASS, nargs as i64, loc);
        self.add(COPY, 1, loc);
        self.load_cv(Cv::none(), loc);
        self.add(IS_OP, 1, loc);
        pc.on_top += 1;
        self.jump_to_fail_pop(pc, loc, POP_JUMP_IF_FALSE);
        self.add(UNPACK_SEQUENCE, (nargs + nattrs) as i64, loc);
        pc.on_top += (nargs + nattrs) as i64 - 1;
        for pat in patterns.iter().chain(kwd_patterns) {
            pc.on_top -= 1;
            if is_wildcard(pat) {
                self.add(POP_TOP, 0, loc);
                continue;
            }
            self.subpattern(pat, pc)?;
        }
        Ok(())
    }

    /// `compiler_pattern_or`: cada alternativa roda sobre uma cópia do sujeito; quem casa salta para `end`, e as capturas
    /// de todas ficam na ordem da primeira.
    fn pattern_or(&mut self, p: &Pattern, patterns: &[Pattern], pc: &mut PatCtx) -> Res<()> {
        let end = self.cfg.new_block();
        let old_stores = std::mem::take(&mut pc.stores);
        let old_fail_pop = std::mem::take(&mut pc.fail_pop);
        let old_on_top = pc.on_top;
        let mut control: Vec<String> = Vec::new();
        for (i, alt) in patterns.iter().enumerate() {
            let aloc = self.pat_loc(alt);
            pc.stores = Vec::new();
            pc.fail_pop = Vec::new();
            pc.on_top = 0;
            self.add(COPY, 1, aloc);
            self.pattern(alt, pc)?;
            if i == 0 {
                control = pc.stores.clone();
            } else {
                if pc.stores.len() != control.len() {
                    return Err(Unsupported);
                }
                let mut icontrol = control.len();
                while icontrol > 0 {
                    icontrol -= 1;
                    let istores = pc.stores.iter().position(|s| *s == control[icontrol]).ok_or(Unsupported)?;
                    if icontrol != istores {
                        let rotations = istores + 1;
                        let rotated: Vec<String> = pc.stores.drain(..rotations).collect();
                        let at = icontrol - istores;
                        for (k, name) in rotated.into_iter().enumerate() {
                            pc.stores.insert(at + k, name);
                        }
                        for _ in 0..rotations {
                            self.rotate(aloc, icontrol + 1);
                        }
                    }
                }
            }
            self.add_jump(JUMP, end, aloc);
            self.emit_and_reset_fail_pop(pc, aloc);
        }
        pc.stores = old_stores;
        pc.fail_pop = old_fail_pop;
        pc.on_top = old_on_top;
        let ploc = self.pat_loc(p);
        self.add(POP_TOP, 0, ploc);
        self.jump_to_fail_pop(pc, ploc, JUMP);
        self.cfg.use_block(end);
        let nrots = control.len() + 1 + (pc.on_top.max(0) as usize) + pc.stores.len();
        for name in control {
            self.rotate(ploc, nrots);
            if pc.stores.contains(&name) {
                return Err(Unsupported);
            }
            pc.stores.push(name);
        }
        self.add(POP_TOP, 0, ploc);
        Ok(())
    }

    /// `LOAD_CONST None`, `IS_OP` e salto condicional viram `POP_JUMP_IF_NONE` ou `POP_JUMP_IF_NOT_NONE` (o `TO_BOOL`
    /// entre eles some).
    pub(super) fn fold_none_jumps(&mut self) {
        let mut blocks = std::mem::take(&mut self.cfg.blocks);
        for blk in blocks.iter_mut() {
            for i in 0..blk.len() {
                if blk[i].op != LOAD_CONST || !matches!(self.keys[blk[i].arg as usize], Key::None) {
                    continue;
                }
                let skip = |blk: &[Instr], mut at: usize| {
                    while blk.get(at).is_some_and(|x| x.op == NOP) {
                        at += 1;
                    }
                    at
                };
                let is = skip(blk, i + 1);
                if blk.get(is).is_none_or(|x| x.op != IS_OP) {
                    continue;
                }
                let mut jump = skip(blk, is + 1);
                if blk.get(jump).is_some_and(|x| x.op == TO_BOOL) {
                    blk[jump].op = NOP;
                    jump = skip(blk, jump + 1);
                }
                let Some(j) = blk.get(jump).copied() else { continue };
                if !matches!(j.op, POP_JUMP_IF_FALSE | POP_JUMP_IF_TRUE) {
                    continue;
                }
                let invert = (blk[is].arg != 0) ^ (j.op == POP_JUMP_IF_FALSE);
                blk[i].op = NOP;
                blk[i].arg = 0;
                blk[is].op = NOP;
                blk[is].arg = 0;
                blk[jump].op = if invert { POP_JUMP_IF_NOT_NONE } else { POP_JUMP_IF_NONE };
            }
        }
        self.cfg.blocks = blocks;
    }
}
