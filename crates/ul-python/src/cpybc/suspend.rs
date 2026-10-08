//! Geradores, corrotinas, compreensões inline e as passagens do grafo que só elas exigem (`SWAP` estático).
//!
//! Espelha `codegen.c` do 3.13: `compiler_comprehension` (com `push_inlined_comprehension_state`),
//! `compiler_add_yield_from`, `wrap_in_stopiteration_handler` e, em `flowgraph.c`, `swaptimize` e
//! `apply_static_swaps`. Vive num módulo filho de `cpybc` para ver os itens privados dele.

use super::*;
use crate::ast::Comprehension;

pub(super) const CLEANUP_THROW: u16 = 8;
pub(super) const END_ASYNC_FOR: u16 = 10;
pub(super) const END_SEND: u16 = 12;
pub(super) const GET_AITER: u16 = 16;
pub(super) const GET_ANEXT: u16 = 18;
pub(super) const RETURN_GENERATOR: u16 = 35;
pub(super) const GET_YIELD_FROM_ITER: u16 = 21;
pub(super) const GET_AWAITABLE: u16 = 73;
pub(super) const JUMP_BACKWARD_NO_INTERRUPT: u16 = 78;
pub(super) const LOAD_FAST_AND_CLEAR: u16 = 86;
pub(super) const MAP_ADD: u16 = 95;
pub(super) const SEND: u16 = 104;
pub(super) const YIELD_VALUE: u16 = 118;
/// Pseudo-opcode: salto sem `eval breaker`; vira `JUMP_FORWARD` ou `JUMP_BACKWARD_NO_INTERRUPT` na montagem.
pub(super) const JUMP_NO_INTERRUPT: u16 = 257;
/// Pseudo-opcode: `STORE_FAST` que aceita a pilha com `NULL` (restaura o local de uma compreensão inline).
pub(super) const STORE_FAST_MAYBE_NULL: u16 = 267;

pub(super) const RESUME_AFTER_YIELD: i64 = 1;
pub(super) const RESUME_AFTER_YIELD_FROM: i64 = 2;
pub(super) const RESUME_AFTER_AWAIT: i64 = 3;
/// `RESUME` depois de um `yield` com só o tratador implícito do gerador ativo (`RESUME_OPARG_DEPTH1_MASK`).
pub(super) const RESUME_DEPTH1: i64 = 4;
const INTRINSIC_STOPITERATION_ERROR: i64 = 3;
const INTRINSIC_ASYNC_GEN_WRAP: i64 = 4;

/// A coleção que uma compreensão monta (ou o gerador, que não monta nada).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Comp {
    List,
    Set,
    Dict,
    Gen,
}

/// Os nomes que as compreensões inline dentro de `e` ligam (o `symtable` os funde no escopo de fora). O corpo de uma
/// função é outro escopo: da expressão geradora só conta o primeiro iterável, que roda aqui.
fn nested_targets(e: &Expr, out: &mut Vec<String>) {
    match &e.kind {
        E::Lambda { .. } => return,
        E::GeneratorExp { generators, .. } => {
            if let Some(g) = generators.first() {
                nested_targets(&g.iter, out);
            }
            return;
        }
        E::ListComp { generators, .. } | E::SetComp { generators, .. } | E::DictComp { generators, .. } => {
            for g in generators {
                crate::compile::ordered_targets(&g.target, out);
            }
        }
        _ => {}
    }
    for c in crate::compile::children(e) {
        nested_targets(c, out);
    }
}

fn swappable(op: u16) -> bool {
    matches!(op, STORE_FAST | STORE_FAST_MAYBE_NULL | POP_TOP)
}

fn stores_to(i: &Instr) -> i64 {
    if matches!(i.op, STORE_FAST | STORE_FAST_MAYBE_NULL) {
        i.arg
    } else {
        -1
    }
}

/// `next_swappable_instruction`: a próxima instrução (pulando `NOP`) que `apply_static_swaps` sabe reordenar; com
/// `lineno >= 0`, só dentro da mesma linha.
fn next_swappable(blk: &[Instr], mut i: usize, lineno: i32) -> Option<usize> {
    loop {
        i += 1;
        let ins = blk.get(i)?;
        if lineno >= 0 && ins.loc.line != lineno {
            return None;
        }
        if ins.op == NOP {
            continue;
        }
        return swappable(ins.op).then_some(i);
    }
}

impl Cfg {
    /// `swaptimize`: a corrida de `SWAP` e `NOP` que começa em `ix` vira o menor número de `SWAP`; devolve o índice da
    /// última instrução da corrida.
    fn swaptimize(&mut self, b: usize, ix: usize) -> usize {
        const VISITED: i64 = -1;
        let blk = &mut self.blocks[b];
        let limit = blk.len() - ix;
        let mut depth = blk[ix].arg;
        let mut len = 0;
        let mut more = false;
        loop {
            len += 1;
            if len >= limit {
                break;
            }
            let ins = blk[ix + len];
            if ins.op == SWAP {
                depth = depth.max(ins.arg);
                more = true;
            } else if ins.op != NOP {
                break;
            }
        }
        if !more {
            return ix;
        }
        let mut stack: Vec<i64> = (0..depth).collect();
        for ins in &blk[ix..ix + len] {
            if ins.op == SWAP {
                let top = stack[0];
                stack[0] = stack[ins.arg as usize - 1];
                stack[ins.arg as usize - 1] = top;
            }
        }
        let mut current = len as i64 - 1;
        for i in 0..stack.len() {
            if stack[i] == VISITED || stack[i] == i as i64 {
                continue;
            }
            let mut j = i;
            loop {
                if j != 0 {
                    let slot = &mut blk[ix + current as usize];
                    slot.op = SWAP;
                    slot.arg = j as i64 + 1;
                    current -= 1;
                }
                if stack[j] == VISITED {
                    break;
                }
                let next = stack[j] as usize;
                stack[j] = VISITED;
                j = next;
            }
        }
        while current >= 0 {
            let slot = &mut blk[ix + current as usize];
            slot.op = NOP;
            slot.arg = 0;
            current -= 1;
        }
        ix + len - 1
    }

    /// `apply_static_swaps`: troca as instruções (`STORE_FAST`, `POP_TOP`) em vez dos itens da pilha.
    fn apply_static_swaps(&mut self, b: usize, i: usize) {
        let blk = &mut self.blocks[b];
        let mut i = i as i64;
        while i >= 0 {
            let at = i as usize;
            let op = blk[at].op;
            if op != SWAP {
                if op == NOP || swappable(op) {
                    i -= 1;
                    continue;
                }
                return;
            }
            let Some(j) = next_swappable(blk, at, -1) else { return };
            let mut k = j;
            let lineno = blk[j].loc.line;
            for _ in 1..blk[at].arg {
                match next_swappable(blk, k, lineno) {
                    Some(n) => k = n,
                    None => return,
                }
            }
            let (store_j, store_k) = (stores_to(&blk[j]), stores_to(&blk[k]));
            if store_j >= 0 || store_k >= 0 {
                if store_j == store_k {
                    return;
                }
                for idx in j + 1..k {
                    let s = stores_to(&blk[idx]);
                    if s >= 0 && (s == store_j || s == store_k) {
                        return;
                    }
                }
            }
            blk[at].op = NOP;
            blk[at].arg = 0;
            blk.swap(j, k);
            i -= 1;
        }
    }

    /// O caso `SWAP` de `optimize_basic_block`: `SWAP 1` some, uma corrida de `SWAP` encurta e os `SWAP` que sobram
    /// se aplicam às instruções vizinhas.
    pub(super) fn optimize_swaps(&mut self) {
        for p in 0..self.order.len() {
            let b = self.order[p];
            let mut i = 0;
            while i < self.blocks[b].len() {
                if self.blocks[b][i].op == SWAP {
                    if self.blocks[b][i].arg == 1 {
                        self.blocks[b][i].op = NOP;
                        self.blocks[b][i].arg = 0;
                    } else {
                        i = self.swaptimize(b, i);
                        self.apply_static_swaps(b, i);
                    }
                }
                i += 1;
            }
        }
    }
}

impl Gen<'_> {
    /// Gerador, corrotina ou gerador assíncrono: tem `RETURN_GENERATOR` no começo e o tratador de `StopIteration`.
    pub(super) fn suspendable(&self) -> bool {
        self.function && (self.code.is_generator || self.code.is_async)
    }

    /// `wrap_in_stopiteration_handler`: um `SETUP_CLEANUP` antes do `RESUME` cobre o corpo todo e o tratador, no fim,
    /// converte a `StopIteration` que escapa em `RuntimeError`.
    pub(super) fn stop_iteration_handler(&mut self) {
        let handler = self.cfg.new_block();
        self.cfg.use_block(handler);
        self.add(CALL_INTRINSIC_1, INTRINSIC_STOPITERATION_ERROR, NO_LOC);
        self.add(RERAISE, 1, NO_LOC);
        let entry = self.cfg.order[0];
        let setup = Instr { target: handler, ..Instr::new(SETUP_CLEANUP, 0, NO_LOC) };
        self.cfg.blocks[entry].insert(0, setup);
    }

    /// `RETURN_GENERATOR` e `POP_TOP` que `insert_prefix_instructions` põe antes do `RESUME`.
    pub(super) fn generator_prefix(&self) -> Vec<Instr> {
        let line = self.code.first_line.max(1) as i32;
        let loc = Loc { line, end_line: line, col: -1, end_col: -1 };
        vec![Instr::new(RETURN_GENERATOR, 0, loc), Instr::new(POP_TOP, 0, loc)]
    }

    /// `ADDOP_YIELD`: `YIELD_VALUE` e o `RESUME` que o segue (num gerador assíncrono o valor passa antes por
    /// `INTRINSIC_ASYNC_GEN_WRAP`).
    fn add_yield(&mut self, loc: Loc) {
        if self.code.is_async && self.code.is_generator {
            self.add(CALL_INTRINSIC_1, INTRINSIC_ASYNC_GEN_WRAP, loc);
        }
        self.add(YIELD_VALUE, 0, loc);
        self.add(RESUME, RESUME_AFTER_YIELD, loc);
    }

    /// `yield` e `yield v`.
    pub(super) fn yield_expr(&mut self, value: Option<&Expr>, loc: Loc) -> Res<()> {
        if !self.function {
            return Err(Unsupported);
        }
        match value {
            Some(v) => self.expr(v)?,
            None => self.load_cv(Cv::none(), loc),
        }
        self.add_yield(loc);
        Ok(())
    }

    /// `compiler_add_yield_from`: o laço `SEND` com o `SETUP_FINALLY` virtual que repassa `throw` e `close`.
    fn add_yield_from(&mut self, loc: Loc, resume: i64) {
        let send = self.cfg.new_block();
        let fail = self.cfg.new_block();
        let exit = self.cfg.new_block();
        self.cfg.use_block(send);
        self.add_jump(SEND, exit, loc);
        self.add_jump(SETUP_FINALLY, fail, loc);
        self.add(YIELD_VALUE, 1, loc);
        self.add(POP_BLOCK, 0, NO_LOC);
        self.add(RESUME, resume, loc);
        self.add_jump(JUMP_NO_INTERRUPT, send, loc);
        self.cfg.use_block(fail);
        self.add(CLEANUP_THROW, 0, loc);
        self.cfg.use_block(exit);
        self.add(END_SEND, 0, loc);
    }

    /// `yield from v`.
    pub(super) fn yield_from_expr(&mut self, value: &Expr, loc: Loc) -> Res<()> {
        if !self.function || self.code.is_async {
            return Err(Unsupported);
        }
        self.expr(value)?;
        self.add(GET_YIELD_FROM_ITER, 0, loc);
        self.load_cv(Cv::none(), loc);
        self.add_yield_from(loc, RESUME_AFTER_YIELD_FROM);
        Ok(())
    }

    /// `await v`.
    pub(super) fn await_expr(&mut self, value: &Expr, loc: Loc) -> Res<()> {
        if !self.function || !self.code.is_async {
            return Err(Unsupported);
        }
        self.expr(value)?;
        self.await_top(loc, 0);
        Ok(())
    }

    /// `GET_AWAITABLE oparg` e o laço `SEND` que espera o valor do topo da pilha (`await`, `async with`).
    pub(super) fn await_top(&mut self, loc: Loc, oparg: i64) {
        self.add(GET_AWAITABLE, oparg, loc);
        self.send_await(loc);
    }

    /// `LOAD_CONST None` e o laço `SEND` que espera o valor do topo (`ADD_YIELD_FROM` com `await`): o que segue o
    /// `GET_AWAITABLE` e o `GET_ANEXT`.
    fn send_await(&mut self, loc: Loc) {
        self.load_cv(Cv::none(), loc);
        self.add_yield_from(loc, RESUME_AFTER_AWAIT);
    }

    /// `[... for ...]`, `{... for ...}` e `{k: v for ...}` inline (PEP 709): os locais do laço são guardados na pilha
    /// (`LOAD_FAST_AND_CLEAR`) e restaurados no fim, ou no `SETUP_FINALLY` virtual se o laço levanta.
    pub(super) fn comprehension(
        &mut self,
        e: &Expr,
        kind: Comp,
        elt: &Expr,
        value: Option<&Expr>,
        generators: &[Comprehension],
    ) -> Res<()> {
        if generators.is_empty() {
            return Err(Unsupported);
        }
        // `async for` só numa corrotina; fora dela a compilação do interpretador já recusou.
        if generators.iter().any(|g| g.is_async != 0) && !(self.function && self.code.is_async) {
            return Err(Unsupported);
        }
        let loc = self.loc(&e.pos);
        let mut names: Vec<String> = Vec::new();
        for g in generators {
            crate::compile::ordered_targets(&g.target, &mut names);
        }
        // As compreensões de dentro vêm na ordem em que o `symtable` entra nelas: as condições do primeiro `for`, o
        // iterável e as condições de cada um dos outros, o valor e o elemento. Um nome repetido vale uma vez.
        for (i, g) in generators.iter().enumerate() {
            if i > 0 {
                nested_targets(&g.iter, &mut names);
            }
            g.ifs.iter().for_each(|c| nested_targets(c, &mut names));
        }
        if let Some(v) = value {
            nested_targets(v, &mut names);
        }
        nested_targets(elt, &mut names);
        let mut seen = HashSet::new();
        names.retain(|n| seen.insert(n.clone()));
        // Uma função de dentro (`lambda`, expressão geradora) que fecha um alvo o torna célula; fora de função não há
        // `co_cellvars` para o corpo do módulo ou da classe.
        let elts: Vec<&Expr> = std::iter::once(elt).chain(value).collect();
        let captured = crate::compile::captured_names(&names, &crate::compile::comp_body(&elts, generators));
        if !self.function && !captured.is_empty() {
            return Err(Unsupported);
        }
        let mut locals: Vec<(String, usize, bool)> = Vec::new();
        for n in &names {
            self.check_name(n)?;
            if self.globals_decl.contains(n) {
                return Err(Unsupported);
            }
            let slot = self.localsplus.iter().position(|v| &**v == n.as_str()).ok_or(Unsupported)?;
            locals.push((n.clone(), slot, captured.contains(n)));
        }
        if locals.is_empty() {
            return Err(Unsupported);
        }
        let slots: Vec<usize> = locals.iter().map(|l| l.1).collect();
        let first = &generators[0];
        let iter_loc = self.loc(&first.iter.pos);
        self.iter_value(&first.iter)?;
        self.add(if first.is_async != 0 { GET_AITER } else { GET_ITER }, 0, iter_loc);
        for &(_, slot, cell) in &locals {
            self.add(LOAD_FAST_AND_CLEAR, slot as i64, loc);
            if cell {
                self.add(MAKE_CELL, slot as i64, loc);
            }
        }
        self.add(SWAP, slots.len() as i64 + 1, loc);
        let cleanup = self.cfg.new_block();
        let end = self.cfg.new_block();
        self.add_jump(SETUP_FINALLY, cleanup, loc);
        let body_block = self.cfg.new_block();
        self.cfg.use_block(body_block);
        let build = match kind {
            Comp::List => BUILD_LIST,
            Comp::Set => BUILD_SET,
            Comp::Dict | Comp::Gen => BUILD_MAP,
        };
        self.add(build, 0, loc);
        self.add(SWAP, 2, loc);
        let mark = self.comp_locals.len();
        self.comp_locals.extend(locals);
        self.comp_generator(generators, 0, elt, value, kind, true, loc)?;
        self.comp_locals.truncate(mark);
        self.add(POP_BLOCK, 0, NO_LOC);
        self.add_jump(JUMP, end, NO_LOC);
        self.cfg.use_block(cleanup);
        self.add(SWAP, 2, NO_LOC);
        self.add(POP_TOP, 0, NO_LOC);
        self.restore_locals(&slots, loc);
        self.add(RERAISE, 0, NO_LOC);
        self.cfg.use_block(end);
        self.restore_locals(&slots, loc);
        Ok(())
    }

    /// `restore_inlined_comprehension_locals`: o resultado volta ao topo e os locais guardados são regravados na
    /// ordem inversa à da pilha.
    fn restore_locals(&mut self, slots: &[usize], loc: Loc) {
        self.add(SWAP, slots.len() as i64 + 1, loc);
        for &i in slots.iter().rev() {
            self.add(STORE_FAST_MAYBE_NULL, i as i64, loc);
        }
    }

    /// `compiler_sync_comprehension_generator` e `compiler_async_comprehension_generator`: um `for` da compreensão
    /// (com os `if` e os laços de dentro). Só o primeiro, quando o iterador já está na pilha (`iter_on_stack`), não o
    /// calcula. O `async for` chama `__anext__` sob um `SETUP_FINALLY` cujo tratador é o `END_ASYNC_FOR`, e todas as
    /// instruções dele levam a localização da compreensão.
    pub(super) fn comp_generator(
        &mut self,
        generators: &[Comprehension],
        idx: usize,
        elt: &Expr,
        value: Option<&Expr>,
        kind: Comp,
        iter_on_stack: bool,
        comp_loc: Loc,
    ) -> Res<()> {
        let g = &generators[idx];
        let is_async = g.is_async != 0;
        let iter_loc = self.loc(&g.iter.pos);
        let start = self.cfg.new_block();
        let if_cleanup = self.cfg.new_block();
        let anchor = self.cfg.new_block();
        if !iter_on_stack {
            if idx == 0 {
                self.add(LOAD_FAST, 0, comp_loc);
            } else {
                self.iter_value(&g.iter)?;
                if is_async {
                    self.add(GET_AITER, 0, iter_loc);
                } else {
                    self.add(GET_ITER, 0, iter_loc);
                }
            }
        }
        if !is_async {
            self.add(GET_ITER, 0, iter_loc);
        }
        self.cfg.use_block(start);
        if is_async {
            self.add_jump(SETUP_FINALLY, anchor, comp_loc);
            self.add(GET_ANEXT, 0, comp_loc);
            self.send_await(comp_loc);
            self.add(POP_BLOCK, 0, comp_loc);
        } else {
            self.add_jump(FOR_ITER, anchor, iter_loc);
        }
        self.store_target(&g.target)?;
        for c in &g.ifs {
            self.jump_if(c, if_cleanup, false)?;
        }
        let mut elt_loc = self.loc(&elt.pos);
        if idx + 1 < generators.len() {
            self.comp_generator(generators, idx + 1, elt, value, kind, false, comp_loc)?;
        } else {
            let depth = generators.len() as i64 + 1;
            match kind {
                Comp::List | Comp::Set => {
                    self.expr(elt)?;
                    self.add(if kind == Comp::List { LIST_APPEND } else { SET_ADD }, depth, elt_loc);
                }
                Comp::Dict => {
                    let v = value.ok_or(Unsupported)?;
                    self.expr(elt)?;
                    self.expr(v)?;
                    let (k, vl) = (self.loc(&elt.pos), self.loc(&v.pos));
                    elt_loc = Loc { line: k.line, end_line: vl.end_line, col: k.col, end_col: vl.end_col };
                    self.add(MAP_ADD, depth, elt_loc);
                }
                Comp::Gen => {
                    self.expr(elt)?;
                    self.add_yield(elt_loc);
                    self.add(POP_TOP, 0, elt_loc);
                }
            }
        }
        self.cfg.use_block(if_cleanup);
        self.add_jump(JUMP, start, elt_loc);
        self.cfg.use_block(anchor);
        if is_async {
            self.add(END_ASYNC_FOR, 0, comp_loc);
        } else {
            self.add(END_FOR, 0, NO_LOC);
            self.add(POP_TOP, 0, NO_LOC);
        }
        Ok(())
    }

    /// `compiler_async_for`: cada passo chama `__anext__` sob um `SETUP_FINALLY` cujo tratador é o `END_ASYNC_FOR`
    /// (que engole o `StopAsyncIteration`); o `else` roda depois dele.
    pub(super) fn async_for_stmt(
        &mut self,
        target: &Expr,
        iter: &Expr,
        body: &[Stmt],
        orelse: &[Stmt],
        loc: Loc,
    ) -> Res<()> {
        if !self.function || !self.code.is_async {
            return Err(Unsupported);
        }
        let start = self.cfg.new_block();
        let except = self.cfg.new_block();
        let end = self.cfg.new_block();
        self.expr(iter)?;
        let iter_loc = self.loc(&iter.pos);
        self.add(GET_AITER, 0, iter_loc);
        self.cfg.use_block(start);
        self.fblocks.push(Fb::Loop(Loop { start, exit: end, is_for: true }));
        self.add_jump(SETUP_FINALLY, except, loc);
        self.add(GET_ANEXT, 0, loc);
        self.send_await(loc);
        self.add(POP_BLOCK, 0, loc);
        self.store_target(target)?;
        self.stmts(body)?;
        self.add_jump(JUMP, start, NO_LOC);
        self.fblocks.pop();
        self.cfg.use_block(except);
        // O `END_ASYNC_FOR` usa a linha e as colunas do iterável (`loc = LOC(s->v.AsyncFor.iter)`).
        self.add(END_ASYNC_FOR, 0, iter_loc);
        self.stmts(orelse)?;
        self.cfg.use_block(end);
        Ok(())
    }

    /// A expressão geradora no código de fora: a função `<genexpr>` (o `k`-ésimo de `Code::functions`), o iterável do
    /// primeiro `for` e a chamada que cria o gerador.
    pub(super) fn genexp(&mut self, e: &Expr, generators: &[Comprehension]) -> Res<()> {
        let first = generators.first().ok_or(Unsupported)?;
        let loc = self.loc(&e.pos);
        let k = self.next_fn;
        let inner = self.code.functions.get(k).ok_or(Unsupported)?.clone();
        self.next_fn += 1;
        self.closure_code(k, &inner, loc, 0)?;
        let iter_loc = self.loc(&first.iter.pos);
        self.iter_value(&first.iter)?;
        self.add(if first.is_async != 0 { GET_AITER } else { GET_ITER }, 0, iter_loc);
        self.add(CALL, 0, loc);
        Ok(())
    }
}

/// O bytecode do código `<genexpr>` da expressão em `pos`: `.0` é o iterador do primeiro `for`, que o código de fora
/// já criou.
pub fn genexp_code(
    code: &Code,
    pos: &Pos,
    elt: &Expr,
    generators: &[Comprehension],
    globals_decl: &HashSet<String>,
    imports: &HashSet<String>,
) -> Option<Emitted> {
    if !code.is_function {
        return None;
    }
    let first = pos.lineno.max(1) as i32;
    let resume = Loc { line: first, end_line: first, col: 0, end_col: 0 };
    let mut g = Gen::new(true, code, globals_decl, imports, resume);
    let comp_loc = g.loc(pos);
    g.comp_generator(generators, 0, elt, None, Comp::Gen, false, comp_loc).ok()?;
    g.implicit_return();
    g.stop_iteration_handler();
    g.finish(first, 1)
}
