//! Bytecode do CPython 3.13 emitido ao lado do código interno do compilador.
//!
//! O interpretador executa o `Op` de `compile.rs`; este módulo reconstrói, a partir da mesma árvore, o que o
//! compilador do CPython 3.13 produziria: `co_code` (opcodes, `oparg`, entradas de CACHE e `EXTENDED_ARG`),
//! `co_consts`, `co_names`, a `co_linetable` no formato de localização do 3.13 e `co_stacksize`. O `dis.py`
//! real do Debian roda em cima disso.
//!
//! O pipeline espelha `Python/codegen` + `Python/flowgraph.c`: grafo de blocos básicos, laço de passes
//! (blocos vazios, saídas pequenas inlinadas, peephole, NOPs, alcançabilidade, saltos redundantes, constantes
//! não usadas, `LOAD_FAST_CHECK`, superinstruções, números de linha) e montagem com saltos relativos. O que
//! este emissor ainda não cobre devolve `None` (e `Emitted::synthetic` toma o lugar); a lista do que falta
//! está em `wip/notes/python-dis.md`.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};

use crate::ast::{
    Arguments, BoolOp, CmpOp, Constant, ExceptHandler, Expr, ExprContext, ExprKind as E, Keyword, Operator, Pos, Stmt,
    StmtKind as S, UnaryOp, WithItem,
};
use crate::compile::Code;
use crate::cpyops;
use crate::object::{Set, Value};

mod complex;
mod generic;
mod pattern;
mod suspend;
pub(crate) use complex::restore_image as restore_complex_const;
pub use generic::{generic_scope, layout, Layout};
pub use suspend::genexp_code;

const BEFORE_ASYNC_WITH: u16 = 1;
const BEFORE_WITH: u16 = 2;
const BINARY_SLICE: u16 = 4;
const BINARY_SUBSCR: u16 = 5;
const CHECK_EG_MATCH: u16 = 6;
const CHECK_EXC_MATCH: u16 = 7;
const DELETE_SUBSCR: u16 = 9;
const END_FOR: u16 = 11;
const FORMAT_SIMPLE: u16 = 14;
const FORMAT_WITH_SPEC: u16 = 15;
const GET_ITER: u16 = 19;
const LOAD_ASSERTION_ERROR: u16 = 23;
const LOAD_BUILD_CLASS: u16 = 24;
const MAKE_FUNCTION: u16 = 26;
const NOP: u16 = 30;
const POP_EXCEPT: u16 = 31;
const POP_TOP: u16 = 32;
const PUSH_EXC_INFO: u16 = 33;
const PUSH_NULL: u16 = 34;
const RETURN_VALUE: u16 = 36;
const SETUP_ANNOTATIONS: u16 = 37;
const STORE_SLICE: u16 = 38;
const STORE_SUBSCR: u16 = 39;
const TO_BOOL: u16 = 40;
const UNARY_INVERT: u16 = 41;
const UNARY_NEGATIVE: u16 = 42;
const UNARY_NOT: u16 = 43;
const WITH_EXCEPT_START: u16 = 44;
const BINARY_OP: u16 = 45;
const BUILD_CONST_KEY_MAP: u16 = 46;
const BUILD_LIST: u16 = 47;
const BUILD_MAP: u16 = 48;
const BUILD_SET: u16 = 49;
const BUILD_SLICE: u16 = 50;
const BUILD_STRING: u16 = 51;
const BUILD_TUPLE: u16 = 52;
const CALL: u16 = 53;
const CALL_FUNCTION_EX: u16 = 54;
const CALL_INTRINSIC_1: u16 = 55;
const CALL_INTRINSIC_2: u16 = 56;
const CALL_KW: u16 = 57;
const COMPARE_OP: u16 = 58;
const CONTAINS_OP: u16 = 59;
const CONVERT_VALUE: u16 = 60;
const COPY: u16 = 61;
const COPY_FREE_VARS: u16 = 62;
const DELETE_ATTR: u16 = 63;
const DELETE_DEREF: u16 = 64;
const DELETE_FAST: u16 = 65;
const DELETE_GLOBAL: u16 = 66;
const DELETE_NAME: u16 = 67;
const DICT_MERGE: u16 = 68;
const DICT_UPDATE: u16 = 69;
const FOR_ITER: u16 = 72;
const IMPORT_FROM: u16 = 74;
const IMPORT_NAME: u16 = 75;
const IS_OP: u16 = 76;
const JUMP_BACKWARD: u16 = 77;
const JUMP_FORWARD: u16 = 79;
const LIST_APPEND: u16 = 80;
const LIST_EXTEND: u16 = 81;
const LOAD_ATTR: u16 = 82;
const LOAD_CONST: u16 = 83;
const LOAD_DEREF: u16 = 84;
const LOAD_FAST: u16 = 85;
const LOAD_FAST_CHECK: u16 = 87;
const LOAD_FAST_LOAD_FAST: u16 = 88;
const LOAD_GLOBAL: u16 = 91;
const LOAD_NAME: u16 = 92;
const LOAD_SUPER_ATTR: u16 = 93;
const MAKE_CELL: u16 = 94;
const POP_JUMP_IF_FALSE: u16 = 97;
const POP_JUMP_IF_NONE: u16 = 98;
const POP_JUMP_IF_NOT_NONE: u16 = 99;
const POP_JUMP_IF_TRUE: u16 = 100;
const RAISE_VARARGS: u16 = 101;
const RERAISE: u16 = 102;
const RETURN_CONST: u16 = 103;
const SET_ADD: u16 = 105;
const SET_FUNCTION_ATTRIBUTE: u16 = 106;
const SET_UPDATE: u16 = 107;
const STORE_ATTR: u16 = 108;
const STORE_DEREF: u16 = 109;
const STORE_FAST: u16 = 110;
const STORE_FAST_LOAD_FAST: u16 = 111;
const STORE_FAST_STORE_FAST: u16 = 112;
const STORE_GLOBAL: u16 = 113;
const STORE_NAME: u16 = 114;
const SWAP: u16 = 115;
const UNPACK_EX: u16 = 116;
const UNPACK_SEQUENCE: u16 = 117;
const RESUME: u16 = 149;
/// Pseudo-opcode do compilador: vira `JUMP_FORWARD` ou `JUMP_BACKWARD` na montagem.
const JUMP: u16 = 256;
/// Pseudo-opcode do compilador: vira `LOAD_FAST` na montagem (a célula que a função de dentro fecha).
const LOAD_CLOSURE: u16 = 258;
/// Pseudo-opcode: fecha o intervalo aberto por um `SETUP_*`; `label_exception_targets` o troca por `NOP`.
const POP_BLOCK: u16 = 263;
/// Pseudo-opcodes que abrem um intervalo protegido; o destino é o bloco que trata a exceção.
const SETUP_CLEANUP: u16 = 264;
const SETUP_FINALLY: u16 = 265;
const SETUP_WITH: u16 = 266;
/// `INTRINSIC_LIST_TO_TUPLE` (`CALL_INTRINSIC_1`).
const INTRINSIC_LIST_TO_TUPLE: i64 = 6;
/// `INTRINSIC_PRINT` (`CALL_INTRINSIC_1`): o valor do `compile(..., 'single')`.
const INTRINSIC_PRINT: i64 = 1;
/// `INTRINSIC_IMPORT_STAR` (`CALL_INTRINSIC_1`): `from m import *`.
const INTRINSIC_IMPORT_STAR: i64 = 2;
/// `INTRINSIC_PREP_RERAISE_STAR` (`CALL_INTRINSIC_2`): monta o grupo que o `try/except*` relança.
const INTRINSIC_PREP_RERAISE_STAR: i64 = 1;

/// Marca, em `Emitted::consts`, a posição de um objeto `code` de função aninhada: o `co_consts` do objeto
/// troca a n-ésima marca pelo n-ésimo código de `Code::functions` (a ordem de criação é a mesma da emissão).
pub const CODE_CONST: &str = "<code>";

/// Bits de `SET_FUNCTION_ATTRIBUTE` (`MAKE_FUNCTION_*`).
const FUNC_DEFAULTS: i64 = 1;
const FUNC_KWDEFAULTS: i64 = 2;
const FUNC_ANNOTATIONS: i64 = 4;
const FUNC_CLOSURE: i64 = 8;

/// Instrução sem bloco de destino.
const NO_TARGET: usize = usize::MAX;
/// Quantas instruções no máximo um bloco de saída pode ter para ser copiado no lugar de um salto.
const MAX_COPY_SIZE: usize = 4;
/// `STACK_USE_GUIDELINE`: acima disso a chamada usa `CALL_FUNCTION_EX`, que este emissor não cobre.
const STACK_USE_GUIDELINE: usize = 30;

/// Intervalo de fonte de uma instrução: linhas de 1, colunas em bytes UTF-8, `-1` quando ausente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Loc {
    pub line: i32,
    pub end_line: i32,
    pub col: i32,
    pub end_col: i32,
}

pub const NO_LOC: Loc = Loc { line: -1, end_line: -1, col: -1, end_col: -1 };

impl Loc {
    fn of(pos: &Pos) -> Loc {
        let line = pos.lineno as i32;
        let end_line = pos.end_lineno.map_or(line, |l| l as i32);
        let col = pos.col_offset as i32;
        let end_col = pos.end_col_offset.map_or(col, |c| c as i32);
        Loc { line, end_line, col, end_col }
    }
}

/// O que o emissor produz para um código: tudo o que o `dis` lê do objeto `code`.
#[derive(Debug, Default, Clone)]
pub struct Emitted {
    pub code: Vec<u8>,
    pub consts: Vec<Value>,
    pub names: Vec<Rc<str>>,
    pub linetable: Vec<u8>,
    /// `co_exceptiontable`: entradas varint de 3.11+ (início, tamanho, destino, profundidade com o bit de `lasti`).
    pub exceptiontable: Vec<u8>,
    pub stacksize: usize,
    pub first_line: i32,
    /// Sem o bytecode real (construção que o emissor ainda não cobre): `co_code` é um esqueleto de `RESUME` e
    /// `NOP` que só preserva a tabela de linhas, e `co_consts`/`co_names` seguem o código interno.
    pub synthetic: bool,
}

/// Uma entrada decodificada da `co_linetable`, em unidades de código (2 bytes).
#[derive(Debug, Clone, Copy)]
pub struct Entry {
    pub start: usize,
    pub end: usize,
    pub loc: Loc,
}

impl Emitted {
    /// As entradas da tabela de localização.
    pub fn entries(&self) -> Vec<Entry> {
        decode_locations(&self.linetable, self.first_line)
    }

    /// `co_lnotab` (`decode_linetable`): pares `(avanço em bytes, avanço de linha)` a cada mudança de linha; os
    /// trechos sem linha não contam, e avanços fora de `0..=255` e `-128..=127` viram vários pares.
    pub fn lnotab(&self) -> Vec<u8> {
        let (mut out, mut line, mut addr) = (Vec::new(), self.first_line, 0usize);
        for (start, _, l) in self.lines() {
            let Some(l) = l.filter(|&l| l != line) else { continue };
            let mut bdelta = start - addr;
            let mut ldelta = l - line;
            while bdelta > 255 {
                out.extend([255, 0]);
                bdelta -= 255;
            }
            while !(-128..=127).contains(&ldelta) {
                let step = ldelta.clamp(-128, 127);
                out.extend([bdelta as u8, step as i8 as u8]);
                ldelta -= step;
                bdelta = 0;
            }
            out.extend([bdelta as u8, ldelta as i8 as u8]);
            (line, addr) = (l, start);
        }
        out
    }

    /// `co_lines()`: `(início, fim, linha ou None)` em bytes. Entradas vizinhas com a mesma linha viram uma só
    /// (`lineiter_next` do CPython).
    pub fn lines(&self) -> Vec<(usize, usize, Option<i32>)> {
        let mut out: Vec<(usize, usize, Option<i32>)> = Vec::new();
        for e in self.entries() {
            let line = (e.loc.line >= 0).then_some(e.loc.line);
            match out.last_mut() {
                Some(last) if last.2 == line => last.1 = e.end * 2,
                _ => out.push((e.start * 2, e.end * 2, line)),
            }
        }
        out
    }

    /// `co_positions()`: uma localização por unidade de código, entradas de CACHE inclusive.
    pub fn positions(&self) -> Vec<Loc> {
        let mut out = Vec::new();
        for e in self.entries() {
            for _ in e.start..e.end {
                out.push(e.loc);
            }
        }
        out
    }

    /// Deslocamento (em bytes) da primeira instrução da linha.
    pub fn first_offset_of_line(&self, line: usize) -> Option<usize> {
        self.entries().iter().find(|e| e.loc.line == line as i32).map(|e| e.start * 2)
    }

    /// O esqueleto de um código que o emissor não cobre: `RESUME` e um `NOP` por instrução interna, com a
    /// tabela de linhas e de colunas do código interno.
    pub fn synthetic(code: &Code) -> Emitted {
        let first_line = code.first_line.max(1) as i32;
        let resume_line = code.first_line as i32;
        let mut bytes = vec![RESUME as u8, 0];
        let mut items = vec![(Loc { line: resume_line, end_line: resume_line, col: 0, end_col: 0 }, 1usize)];
        for (i, line) in code.lines.iter().enumerate() {
            bytes.extend_from_slice(&[NOP as u8, 0]);
            let span = code.spans.get(i).copied().unwrap_or_default();
            let loc = if span.lineno > 0 {
                Loc { line: span.lineno as i32, end_line: span.end_lineno as i32, col: span.col as i32, end_col: span.end_col as i32 }
            } else {
                Loc { line: *line as i32, end_line: *line as i32, col: -1, end_col: -1 }
            };
            items.push((loc, 1));
        }
        Emitted {
            code: bytes,
            linetable: encode_locations(first_line, &items),
            stacksize: 1,
            first_line,
            synthetic: true,
            ..Emitted::default()
        }
    }
}

/// O bytecode de `code`: o emitido de verdade ou, na falta dele, o esqueleto.
pub fn of(code: &Code) -> Rc<Emitted> {
    code.cpy.clone().unwrap_or_else(|| Rc::new(Emitted::synthetic(code)))
}

// ---------------------------------------------------------------------------------------------------------
// Tabela de localização (`Objects/locations.md`)

fn write_varint(out: &mut Vec<u8>, mut v: u32) {
    while v >= 64 {
        out.push(0x40 | (v & 63) as u8);
        v >>= 6;
    }
    out.push(v as u8);
}

fn write_svarint(out: &mut Vec<u8>, v: i32) {
    let u = if v < 0 { (v.unsigned_abs() << 1) | 1 } else { (v as u32) << 1 };
    write_varint(out, u);
}

fn first_byte(code: u8, len: usize) -> u8 {
    0x80 | (code << 3) | (len as u8 - 1)
}

/// `write_location_info_entry`: uma entrada de até 8 unidades.
fn write_entry(out: &mut Vec<u8>, a_line: &mut i32, loc: Loc, len: usize) {
    if loc.line < 0 {
        out.push(first_byte(15, len));
        return;
    }
    let line_delta = loc.line - *a_line;
    let (col, end_col) = (loc.col, loc.end_col);
    if col < 0 || end_col < 0 {
        if loc.end_line == loc.line || loc.end_line == -1 {
            out.push(first_byte(13, len));
            write_svarint(out, line_delta);
            *a_line = loc.line;
            return;
        }
    } else if loc.end_line == loc.line {
        if line_delta == 0 && col < 80 && end_col - col < 16 && end_col >= col {
            out.push(first_byte((col / 8) as u8, len));
            out.push((((col % 8) << 4) | (end_col - col)) as u8);
            return;
        }
        if (0..3).contains(&line_delta) && col < 128 && end_col < 128 {
            out.push(first_byte(10 + line_delta as u8, len));
            out.push(col as u8);
            out.push(end_col as u8);
            *a_line = loc.line;
            return;
        }
    }
    out.push(first_byte(14, len));
    write_svarint(out, line_delta);
    write_varint(out, (loc.end_line - loc.line).max(0) as u32);
    write_varint(out, (col + 1) as u32);
    write_varint(out, (end_col + 1) as u32);
    *a_line = loc.line;
}

/// `assemble_emit_location`: parte em entradas de no máximo 8 unidades.
fn emit_location(out: &mut Vec<u8>, a_line: &mut i32, loc: Loc, mut size: usize) {
    while size > 8 {
        write_entry(out, a_line, loc, 8);
        size -= 8;
    }
    if size > 0 {
        write_entry(out, a_line, loc, size);
    }
}

/// `assemble_location_info`: instruções seguidas com a mesma localização viram uma entrada.
fn encode_locations(first_line: i32, items: &[(Loc, usize)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut a_line = first_line;
    let mut cur = NO_LOC;
    let mut size = 0;
    for &(loc, n) in items {
        if loc != cur {
            emit_location(&mut out, &mut a_line, cur, size);
            cur = loc;
            size = 0;
        }
        size += n;
    }
    emit_location(&mut out, &mut a_line, cur, size);
    out
}

fn read_varint(t: &[u8], pos: &mut usize) -> u32 {
    let mut val = 0u32;
    let mut shift = 0;
    loop {
        let b = t.get(*pos).copied().unwrap_or(0);
        *pos += 1;
        val |= u32::from(b & 63) << shift;
        shift += 6;
        if b & 64 == 0 || shift > 30 {
            return val;
        }
    }
}

fn read_svarint(t: &[u8], pos: &mut usize) -> i32 {
    let v = read_varint(t, pos);
    if v & 1 != 0 {
        -((v >> 1) as i32)
    } else {
        (v >> 1) as i32
    }
}

/// O inverso de [`encode_locations`], como o iterador de `co_positions` do CPython.
fn decode_locations(table: &[u8], first_line: i32) -> Vec<Entry> {
    let mut out = Vec::new();
    let mut pos = 0;
    let mut line = first_line;
    let mut unit = 0;
    while pos < table.len() {
        let b = table[pos];
        pos += 1;
        let code = (b >> 3) & 15;
        let len = usize::from(b & 7) + 1;
        let loc = match code {
            15 => NO_LOC,
            14 => {
                line += read_svarint(table, &mut pos);
                let end_delta = read_varint(table, &mut pos) as i32;
                let col = read_varint(table, &mut pos) as i32 - 1;
                let end_col = read_varint(table, &mut pos) as i32 - 1;
                Loc { line, end_line: line + end_delta, col, end_col }
            }
            13 => {
                line += read_svarint(table, &mut pos);
                Loc { line, end_line: line, col: -1, end_col: -1 }
            }
            10..=12 => {
                line += i32::from(code) - 10;
                let col = i32::from(table.get(pos).copied().unwrap_or(0));
                let end_col = i32::from(table.get(pos + 1).copied().unwrap_or(0));
                pos += 2;
                Loc { line, end_line: line, col, end_col }
            }
            _ => {
                let second = table.get(pos).copied().unwrap_or(0);
                pos += 1;
                let col = i32::from(code) * 8 + i32::from(second >> 4);
                Loc { line, end_line: line, col, end_col: col + i32::from(second & 15) }
            }
        };
        out.push(Entry { start: unit, end: unit + len, loc });
        unit += len;
    }
    out
}

// ---------------------------------------------------------------------------------------------------------
// Grafo de fluxo (`flowgraph.c`)

#[derive(Debug, Clone, Copy)]
struct Instr {
    op: u16,
    arg: i64,
    target: usize,
    loc: Loc,
    /// O bloco que trata a exceção levantada aqui (`i_except`), ou `NO_TARGET` (`label_exception_targets`).
    handler: usize,
    /// Só em salto novo de `jump_thread`: o bloco do salto por onde ele passou. O CPython dá ao salto novo o opcode do
    /// salto do destino, então ele é `JUMP_NO_INTERRUPT` se aquele bloco é frio e `JUMP` se é quente, qualquer que seja
    /// o bloco onde o salto novo está (`push_cold_blocks_to_end`). `NO_TARGET` nos demais.
    via: usize,
}

impl Instr {
    fn new(op: u16, arg: i64, loc: Loc) -> Instr {
        Instr { op, arg, target: NO_TARGET, loc, handler: NO_TARGET, via: NO_TARGET }
    }
}

fn is_uncond(op: u16) -> bool {
    op == JUMP || op == JUMP_FORWARD || op == suspend::JUMP_NO_INTERRUPT
}

fn is_jump(op: u16) -> bool {
    is_uncond(op)
        || matches!(
            op,
            POP_JUMP_IF_FALSE | POP_JUMP_IF_TRUE | POP_JUMP_IF_NONE | POP_JUMP_IF_NOT_NONE | FOR_ITER | suspend::SEND
        )
}

/// `SETUP_*`: abre um intervalo protegido e aponta o bloco que o trata.
fn is_block_push(op: u16) -> bool {
    matches!(op, SETUP_CLEANUP | SETUP_FINALLY | SETUP_WITH)
}

/// `HAS_TARGET`: salto ou abertura de intervalo protegido.
fn has_target(op: u16) -> bool {
    is_jump(op) || is_block_push(op)
}

/// `IS_SCOPE_EXIT_OPCODE`: o bloco não continua no seguinte.
fn is_exit(op: u16) -> bool {
    matches!(op, RETURN_VALUE | RETURN_CONST | RAISE_VARARGS | RERAISE)
}

/// O bloco que termina nesta instrução cai no seguinte?
fn falls_through(op: u16) -> bool {
    !(is_uncond(op) || is_exit(op))
}

#[derive(Debug, Default)]
struct Cfg {
    blocks: Vec<Vec<Instr>>,
    order: Vec<usize>,
    cur: usize,
    /// Blocos que são destino de um `SETUP_*` (`b_except_handler`).
    handlers: HashSet<usize>,
    /// Destinos de `SETUP_CLEANUP` e `SETUP_WITH`: a tabela de exceções guarda o `lasti` (`b_preserve_lasti`).
    lasti: HashSet<usize>,
}

impl Cfg {
    fn new() -> Cfg {
        Cfg { blocks: vec![Vec::new()], order: vec![0], ..Cfg::default() }
    }

    fn new_block(&mut self) -> usize {
        self.blocks.push(Vec::new());
        self.blocks.len() - 1
    }

    /// Passa a emitir em `b`, que vai depois do bloco atual no leiaute.
    fn use_block(&mut self, b: usize) {
        self.order.push(b);
        self.cur = b;
    }

    /// Acrescenta uma instrução; depois de salto ou saída o compilador abre um bloco novo.
    fn push(&mut self, ins: Instr) {
        let ended = self.blocks[self.cur].last().is_some_and(|l| is_jump(l.op) || is_exit(l.op));
        if ended {
            let b = self.new_block();
            self.use_block(b);
        }
        let cur = self.cur;
        self.blocks[cur].push(ins);
    }

    fn first_loc(&self, b: usize) -> Loc {
        self.blocks[b].iter().find(|i| !(i.op == NOP && i.loc.line < 0)).map_or(NO_LOC, |i| i.loc)
    }

    /// `eliminate_empty_basic_blocks`.
    fn eliminate_empty(&mut self) {
        let mut redirect: Vec<Option<usize>> = vec![None; self.blocks.len()];
        for (p, &b) in self.order.iter().enumerate() {
            if self.blocks[b].is_empty() {
                redirect[b] = self.order[p + 1..].iter().copied().find(|&n| !self.blocks[n].is_empty());
            }
        }
        for blk in &mut self.blocks {
            for ins in blk.iter_mut() {
                if ins.target != NO_TARGET {
                    if let Some(r) = redirect[ins.target] {
                        ins.target = r;
                    }
                }
                if ins.handler != NO_TARGET {
                    if let Some(r) = redirect[ins.handler] {
                        ins.handler = r;
                    }
                }
            }
        }
        self.handlers = self.handlers.iter().map(|&h| redirect[h].unwrap_or(h)).collect();
        self.lasti = self.lasti.iter().map(|&h| redirect[h].unwrap_or(h)).collect();
        let blocks = &self.blocks;
        self.order.retain(|&b| !blocks[b].is_empty());
    }

    /// `inline_small_exit_blocks`: o salto incondicional para uma saída pequena vira uma cópia dela.
    fn inline_small_exit_blocks(&mut self) {
        for p in 0..self.order.len() {
            let b = self.order[p];
            let Some(last) = self.blocks[b].last().copied() else { continue };
            if !is_uncond(last.op) || last.target == NO_TARGET {
                continue;
            }
            let t = last.target;
            let small_exit = self.blocks[t].len() <= MAX_COPY_SIZE && self.blocks[t].last().is_some_and(|i| is_exit(i.op));
            if small_exit {
                let copy = self.blocks[t].clone();
                let blk = &mut self.blocks[b];
                let n = blk.len();
                blk[n - 1].op = NOP;
                blk[n - 1].arg = 0;
                blk[n - 1].target = NO_TARGET;
                blk.extend(copy);
            }
        }
    }

    /// `optimize_basic_block`, só os padrões que o emissor gera.
    fn optimize(&mut self) {
        for p in 0..self.order.len() {
            let b = self.order[p];
            let mut i = 0;
            while i < self.blocks[b].len() {
                let cur = self.blocks[b][i];
                let next = self.blocks[b].get(i + 1).map(|n| n.op);
                match (cur.op, next) {
                    (COMPARE_OP, Some(TO_BOOL)) => {
                        self.blocks[b][i].op = NOP;
                        self.blocks[b][i].arg = 0;
                        self.blocks[b][i + 1].op = COMPARE_OP;
                        self.blocks[b][i + 1].arg = cur.arg | 16;
                    }
                    (IS_OP | CONTAINS_OP, Some(TO_BOOL)) => {
                        self.blocks[b][i + 1].op = NOP;
                        self.blocks[b][i + 1].arg = 0;
                    }
                    (LOAD_CONST, Some(RETURN_VALUE)) => {
                        self.blocks[b][i].op = RETURN_CONST;
                        self.blocks[b][i + 1].op = NOP;
                        self.blocks[b][i + 1].arg = 0;
                    }
                    // `BUILD_TUPLE n` e `UNPACK_SEQUENCE n`: um elemento some, dois ou três viram um `SWAP`.
                    (BUILD_TUPLE, Some(UNPACK_SEQUENCE)) if self.blocks[b][i + 1].arg == cur.arg && (1..=3).contains(&cur.arg) => {
                        self.blocks[b][i].op = NOP;
                        self.blocks[b][i].arg = 0;
                        if cur.arg == 1 {
                            self.blocks[b][i + 1].op = NOP;
                            self.blocks[b][i + 1].arg = 0;
                        } else {
                            self.blocks[b][i + 1].op = SWAP;
                        }
                    }
                    (SWAP, _) if cur.arg == 1 => {
                        self.blocks[b][i].op = NOP;
                        self.blocks[b][i].arg = 0;
                    }
                    _ => {}
                }
                if is_jump(self.blocks[b][i].op) && !matches!(self.blocks[b][i].op, FOR_ITER | suspend::SEND) {
                    self.thread_jump(b, i);
                }
                i += 1;
            }
        }
    }

    /// `jump_thread`: o salto para um bloco que só salta vai direto ao destino final. Como no 3.13, o salto antigo vira
    /// `NOP` (que guarda a localização dele até `remove_redundant_nops`) e um salto novo, com o opcode do antigo e a
    /// localização do salto do destino, fecha o bloco.
    fn thread_jump(&mut self, b: usize, i: usize) {
        let mut at = i;
        for _ in 0..16 {
            let cur = self.blocks[b][at];
            let t = cur.target;
            if t == NO_TARGET {
                return;
            }
            match self.blocks[t].first().copied() {
                Some(first) if first.op == JUMP && first.target != NO_TARGET && first.target != t => {
                    if at + 1 == self.blocks[b].len() {
                        self.blocks[b][at] = Instr { op: NOP, arg: 0, target: NO_TARGET, ..cur };
                        self.blocks[b].push(Instr { target: first.target, via: t, ..Instr::new(cur.op, 0, first.loc) });
                        at += 1;
                    } else {
                        self.blocks[b][at].target = first.target;
                    }
                }
                _ => return,
            }
        }
    }

    /// `basicblock_remove_redundant_nops`, em todos os blocos.
    fn remove_redundant_nops(&mut self) {
        for p in 0..self.order.len() {
            let b = self.order[p];
            let next_loc = self.order[p + 1..]
                .iter()
                .copied()
                .find(|&n| !self.blocks[n].is_empty())
                .map_or(NO_LOC, |n| self.first_loc(n));
            let mut blk = std::mem::take(&mut self.blocks[b]);
            let len = blk.len();
            let mut out = Vec::with_capacity(len);
            let mut prev_line = -1;
            for src in 0..len {
                let ins = blk[src];
                let line = ins.loc.line;
                if ins.op == NOP {
                    // O NOP some quando não tem linha, quando a próxima instrução tem a mesma linha (ou nenhuma:
                    // ela herda a localização do NOP) e, por último, quando a anterior tem a mesma linha. A
                    // ordem foi medida no oráculo: `def h(): pass` deixa o `RETURN_CONST` com a posição do `pass`.
                    let redundant = if line < 0 {
                        true
                    } else if src + 1 < len {
                        let next_line = blk[src + 1].loc.line;
                        if next_line == line {
                            true
                        } else if next_line < 0 {
                            blk[src + 1].loc = ins.loc;
                            true
                        } else {
                            prev_line == line
                        }
                    } else {
                        prev_line == line || next_loc.line == line
                    };
                    if redundant {
                        continue;
                    }
                }
                out.push(ins);
                prev_line = line;
            }
            self.blocks[b] = out;
        }
    }

    /// Blocos alcançáveis a partir da entrada e o número de predecessores de cada bloco (`mark_reachable`).
    fn reach_preds(&self) -> (Vec<bool>, Vec<usize>) {
        let n = self.blocks.len();
        let mut seen = vec![false; n];
        let mut preds = vec![0usize; n];
        let Some(&entry) = self.order.first() else { return (seen, preds) };
        let mut next_of = vec![NO_TARGET; n];
        for w in self.order.windows(2) {
            next_of[w[0]] = w[1];
        }
        preds[entry] = 1;
        seen[entry] = true;
        let mut stack = vec![entry];
        while let Some(b) = stack.pop() {
            let falls = self.blocks[b].last().is_none_or(|l| falls_through(l.op));
            let mut succ: Vec<usize> = Vec::new();
            if falls && next_of[b] != NO_TARGET {
                succ.push(next_of[b]);
            }
            succ.extend(self.blocks[b].iter().filter(|i| has_target(i.op) && i.target != NO_TARGET).map(|i| i.target));
            for s in succ {
                preds[s] += 1;
                if !seen[s] {
                    seen[s] = true;
                    stack.push(s);
                }
            }
        }
        (seen, preds)
    }

    fn drop_unreachable(&mut self) {
        let (seen, _) = self.reach_preds();
        for p in 0..self.order.len() {
            let b = self.order[p];
            if !seen[b] {
                self.blocks[b].clear();
            }
        }
    }

    /// `remove_redundant_jumps`: o salto para o bloco que vem logo depois some.
    fn remove_redundant_jumps(&mut self) {
        let mut removed = false;
        for p in 0..self.order.len() {
            let b = self.order[p];
            let Some(last) = self.blocks[b].last().copied() else { continue };
            if !is_uncond(last.op) {
                continue;
            }
            let next = self.order[p + 1..].iter().copied().find(|&n| !self.blocks[n].is_empty());
            if next == Some(last.target) {
                if last.loc.line < 0 {
                    self.blocks[b].pop();
                    // No 3.13.5 a localização é propagada antes de este passe (`resolve_line_numbers` dentro de
                    // `optimize_cfg`): os `POP_BLOCK` (já `NOP`) que precedem o salto herdam a mesma linha dele, e o `NOP`
                    // cuja próxima instrução tem a mesma linha some. Aqui eles ainda são pseudo-opcode sem linha.
                    while self.blocks[b].last().is_some_and(|i| i.op == POP_BLOCK) {
                        self.blocks[b].pop();
                    }
                } else {
                    let n = self.blocks[b].len();
                    self.blocks[b][n - 1].op = NOP;
                    self.blocks[b][n - 1].target = NO_TARGET;
                }
                removed = true;
            }
        }
        if removed {
            self.remove_redundant_nops();
            self.eliminate_empty();
        }
    }

    /// `fast_scan_many_locals`: com mais de 64 locais, os de índice 64 em diante são analisados um bloco por vez (a
    /// inicialização não passa de um bloco a outro): `LOAD_FAST` sem `STORE_FAST`, `DELETE_FAST` ou
    /// `LOAD_FAST_AND_CLEAR` antes no mesmo bloco vira `LOAD_FAST_CHECK`.
    fn scan_many_locals(&mut self) {
        let mut states: HashMap<i64, usize> = HashMap::new();
        for (blocknum, &b) in self.order.iter().enumerate() {
            for ins in self.blocks[b].iter_mut().filter(|i| i.arg >= 64) {
                match ins.op {
                    DELETE_FAST | suspend::LOAD_FAST_AND_CLEAR | STORE_FAST => {
                        states.insert(ins.arg, blocknum);
                    }
                    LOAD_FAST => {
                        if states.get(&ins.arg) != Some(&blocknum) {
                            ins.op = LOAD_FAST_CHECK;
                        }
                        states.insert(ins.arg, blocknum);
                    }
                    _ => {}
                }
            }
        }
    }

    /// `add_checks_for_loads_of_uninitialized_variables`: `LOAD_FAST` de variável possivelmente sem valor
    /// vira `LOAD_FAST_CHECK`.
    fn add_checks(&mut self, nlocals: usize, nparams: usize) {
        if nlocals == 0 {
            return;
        }
        if nlocals > 64 {
            self.scan_many_locals();
        }
        let Some(&entry) = self.order.first() else { return };
        let mut next_of = vec![NO_TARGET; self.blocks.len()];
        for w in self.order.windows(2) {
            next_of[w[0]] = w[1];
        }
        let init = (nparams..nlocals.min(64)).fold(0u64, |m, i| m | (1u64 << i));
        let mut start: Vec<Option<u64>> = vec![None; self.blocks.len()];
        start[entry] = Some(init);
        let mut stack = vec![entry];
        while let Some(b) = stack.pop() {
            let mut mask = start[b].unwrap_or(0);
            let mut pushes: Vec<(usize, u64)> = Vec::new();
            for ins in self.blocks[b].iter_mut() {
                if ins.handler != NO_TARGET {
                    pushes.push((ins.handler, mask));
                }
                let bit = if (0..64).contains(&ins.arg) { 1u64 << ins.arg } else { 0 };
                match ins.op {
                    LOAD_FAST => {
                        if mask & bit != 0 {
                            ins.op = LOAD_FAST_CHECK;
                        }
                        mask &= !bit;
                    }
                    LOAD_FAST_CHECK => mask &= !bit,
                    suspend::LOAD_FAST_AND_CLEAR => mask |= bit,
                    STORE_FAST => mask &= !bit,
                    DELETE_FAST => mask |= bit,
                    _ => {}
                }
                if is_jump(ins.op) && ins.target != NO_TARGET {
                    pushes.push((ins.target, mask));
                }
            }
            let falls = self.blocks[b].last().is_none_or(|l| falls_through(l.op));
            if falls && next_of[b] != NO_TARGET {
                pushes.push((next_of[b], mask));
            }
            for (t, m) in pushes {
                let merged = start[t].map_or(m, |old| old | m);
                if start[t] != Some(merged) {
                    start[t] = Some(merged);
                    stack.push(t);
                }
            }
        }
    }

    /// `insert_superinstructions`.
    fn insert_superinstructions(&mut self) {
        for p in 0..self.order.len() {
            let b = self.order[p];
            let n = self.blocks[b].len();
            for i in 0..n.saturating_sub(1) {
                let (a, c) = (self.blocks[b][i], self.blocks[b][i + 1]);
                let sup = match (a.op, c.op) {
                    (LOAD_FAST, LOAD_FAST) => LOAD_FAST_LOAD_FAST,
                    (STORE_FAST, LOAD_FAST) => STORE_FAST_LOAD_FAST,
                    (STORE_FAST, STORE_FAST) => STORE_FAST_STORE_FAST,
                    _ => continue,
                };
                if a.loc.line >= 0 && c.loc.line >= 0 && a.loc.line != c.loc.line {
                    continue;
                }
                if a.arg >= 16 || c.arg >= 16 {
                    continue;
                }
                self.blocks[b][i].op = sup;
                self.blocks[b][i].arg = (a.arg << 4) | c.arg;
                self.blocks[b][i + 1].op = NOP;
                self.blocks[b][i + 1].arg = 0;
            }
        }
        self.remove_redundant_nops();
    }

    fn is_exit_without_lineno(&self, b: usize) -> bool {
        let blk = &self.blocks[b];
        blk.last().is_some_and(|l| is_exit(l.op)) && blk.iter().all(|i| i.loc.line < 0)
    }

    /// `resolve_line_numbers`: copia saídas sem linha para cada salto que as alcança e propaga a localização
    /// da instrução anterior para as que não têm.
    fn resolve_line_numbers(&mut self) {
        let (_, mut preds) = self.reach_preds();
        let mut p = 0;
        while p < self.order.len() {
            let b = self.order[p];
            if let Some(last) = self.blocks[b].last().copied() {
                let t = last.target;
                if is_jump(last.op) && t != NO_TARGET && self.is_exit_without_lineno(t) && preds[t] > 1 {
                    let mut copy = self.blocks[t].clone();
                    copy[0].loc = last.loc;
                    self.blocks.push(copy);
                    let nb = self.blocks.len() - 1;
                    if let Some(tp) = self.order.iter().position(|&x| x == t) {
                        self.order.insert(tp + 1, nb);
                    }
                    preds.push(1);
                    preds[t] -= 1;
                    let n = self.blocks[b].len();
                    self.blocks[b][n - 1].target = nb;
                }
            }
            p += 1;
        }
        for p in 0..self.order.len().saturating_sub(1) {
            let (b, nb) = (self.order[p], self.order[p + 1]);
            if let Some(last) = self.blocks[b].last().copied() {
                if falls_through(last.op) && self.is_exit_without_lineno(nb) {
                    self.blocks[nb][0].loc = last.loc;
                }
            }
        }
        for p in 0..self.order.len() {
            let b = self.order[p];
            let Some(last) = self.blocks[b].last().copied() else { continue };
            let mut prev = NO_LOC;
            for ins in self.blocks[b].iter_mut() {
                if ins.loc.line < 0 {
                    ins.loc = prev;
                } else {
                    prev = ins.loc;
                }
            }
            if falls_through(last.op) {
                if let Some(&nb) = self.order.get(p + 1) {
                    if preds[nb] == 1 && self.blocks[nb][0].loc.line < 0 {
                        self.blocks[nb][0].loc = prev;
                    }
                }
            }
            if is_jump(last.op) && last.target != NO_TARGET {
                let t = last.target;
                if preds[t] == 1 && self.blocks[t][0].loc.line < 0 {
                    self.blocks[t][0].loc = prev;
                }
            }
        }
    }

    /// Profundidade máxima da pilha e a profundidade de entrada de cada bloco (`calculate_stackdepth`); `i64::MIN` nos
    /// blocos que a varredura não alcança. Os destinos de `SETUP_*` entram com o efeito do salto.
    fn stackdepth(&self) -> (i64, Vec<i64>) {
        let mut start = vec![i64::MIN; self.blocks.len()];
        let Some(&entry) = self.order.first() else { return (0, start) };
        let mut next_of = vec![NO_TARGET; self.blocks.len()];
        for w in self.order.windows(2) {
            next_of[w[0]] = w[1];
        }
        let mut max = 0i64;
        start[entry] = 0;
        let mut stack = vec![entry];
        while let Some(b) = stack.pop() {
            let mut d = start[b];
            let mut dead = false;
            for ins in &self.blocks[b] {
                let name = cpyops::info(i64::from(ins.op)).map_or("NOP", |o| o.name);
                let new_depth = d + cpyops::stack_effect(name, ins.arg, false).unwrap_or(0);
                max = max.max(new_depth);
                if has_target(ins.op) && ins.target != NO_TARGET {
                    let jumped = d + cpyops::stack_effect(name, ins.arg, true).unwrap_or(0);
                    max = max.max(jumped);
                    if start[ins.target] < jumped && start[ins.target] < 100 {
                        start[ins.target] = jumped;
                        stack.push(ins.target);
                    }
                }
                d = new_depth;
                if is_uncond(ins.op) || is_exit(ins.op) {
                    dead = true;
                    break;
                }
            }
            if !dead && next_of[b] != NO_TARGET && start[next_of[b]] < d && start[next_of[b]] < 100 {
                start[next_of[b]] = d;
                stack.push(next_of[b]);
            }
        }
        (max, start)
    }

    /// `normalize_jumps`: um salto condicional para trás vira o salto inverso para o bloco seguinte, seguido de um
    /// bloco novo com o salto incondicional para trás (a localização é a do salto condicional).
    fn normalize_jumps(&mut self) {
        let mut p = 0;
        while p < self.order.len() {
            let b = self.order[p];
            let last = self.blocks[b].last().copied();
            if let Some(last) = last.filter(|l| matches!(l.op, POP_JUMP_IF_FALSE | POP_JUMP_IF_TRUE | POP_JUMP_IF_NONE | POP_JUMP_IF_NOT_NONE)) {
                let backward = self.order.iter().position(|&x| x == last.target).is_some_and(|t| t <= p);
                let following = self.order.get(p + 1).copied();
                if let (true, Some(next_b)) = (backward, following) {
                    let reversed = match last.op {
                        POP_JUMP_IF_FALSE => POP_JUMP_IF_TRUE,
                        POP_JUMP_IF_TRUE => POP_JUMP_IF_FALSE,
                        POP_JUMP_IF_NONE => POP_JUMP_IF_NOT_NONE,
                        _ => POP_JUMP_IF_NONE,
                    };
                    let trampoline = self.new_block();
                    self.blocks[trampoline].push(Instr { target: last.target, ..Instr::new(JUMP, 0, last.loc) });
                    let n = self.blocks[b].len();
                    self.blocks[b][n - 1].op = reversed;
                    self.blocks[b][n - 1].target = next_b;
                    // Medido no oráculo: nem o salto invertido nem o salto para trás entram na tabela de exceções.
                    self.blocks[b][n - 1].handler = NO_TARGET;
                    self.order.insert(p + 1, trampoline);
                    p += 1;
                }
            }
            p += 1;
        }
    }

    /// `mark_except_handlers` e `label_exception_targets`: dá a cada instrução o bloco que trata a exceção dela (o
    /// topo da pilha de `SETUP_*` abertos naquele ponto); o `POP_BLOCK` só desempilha. Um destino de salto ou de
    /// `SETUP_*` herda a pilha do ponto em que foi alcançado pela primeira vez.
    fn label_exception_targets(&mut self) {
        for &b in &self.order {
            for ins in &self.blocks[b] {
                if is_block_push(ins.op) && ins.target != NO_TARGET {
                    self.handlers.insert(ins.target);
                    if ins.op != SETUP_FINALLY {
                        self.lasti.insert(ins.target);
                    }
                }
            }
        }
        let Some(&entry) = self.order.first() else { return };
        let n = self.blocks.len();
        let mut next_of = vec![NO_TARGET; n];
        for w in self.order.windows(2) {
            next_of[w[0]] = w[1];
        }
        let mut stacks: Vec<Vec<usize>> = vec![Vec::new(); n];
        let mut visited = vec![false; n];
        visited[entry] = true;
        let mut todo = vec![entry];
        while let Some(b) = todo.pop() {
            let mut stack = std::mem::take(&mut stacks[b]);
            let mut handler = stack.last().copied().unwrap_or(NO_TARGET);
            for i in 0..self.blocks[b].len() {
                let ins = self.blocks[b][i];
                if is_block_push(ins.op) {
                    if ins.target != NO_TARGET && !visited[ins.target] {
                        visited[ins.target] = true;
                        stacks[ins.target] = stack.clone();
                        todo.push(ins.target);
                    }
                    stack.push(ins.target);
                    handler = ins.target;
                } else if ins.op == POP_BLOCK {
                    stack.pop();
                    handler = stack.last().copied().unwrap_or(NO_TARGET);
                    // O `POP_BLOCK` segue pseudo-opcode até `convert_pseudo_ops` (3.13): sem localização ele não é um
                    // `NOP` que os passes de limpeza removam; vira `NOP` depois de `resolve_line_numbers`, com a linha herdada,
                    // e só some se a anterior do mesmo bloco ou a próxima tem a mesma linha.
                } else {
                    self.blocks[b][i].handler = handler;
                    // O `RESUME` depois de um `yield` fora de qualquer `try` ou `with` (só o tratador implícito do
                    // gerador está aberto) leva o bit que deixa o `close()` pular o `GeneratorExit`.
                    if ins.op == RESUME && ins.arg == suspend::RESUME_AFTER_YIELD && stack.len() == 1 {
                        self.blocks[b][i].arg |= suspend::RESUME_DEPTH1;
                    }
                    if is_jump(ins.op) && ins.target != NO_TARGET && !visited[ins.target] {
                        visited[ins.target] = true;
                        stacks[ins.target] = stack.clone();
                        todo.push(ins.target);
                    }
                }
            }
            let falls = self.blocks[b].last().is_none_or(|l| falls_through(l.op));
            let next = next_of[b];
            if falls && next != NO_TARGET && !visited[next] {
                visited[next] = true;
                stacks[next] = stack;
                todo.push(next);
            }
        }
    }

    /// `push_cold_blocks_to_end`: os blocos que só as exceções alcançam (os tratadores e o que sai deles) vão para o
    /// fim, na ordem em que estavam; o bloco frio que cairia num quente ganha um salto explícito.
    fn push_cold_blocks_to_end(&mut self) {
        if self.order.len() < 2 {
            return;
        }
        let n = self.blocks.len();
        let mut next_of = vec![NO_TARGET; n];
        for w in self.order.windows(2) {
            next_of[w[0]] = w[1];
        }
        let falls = |blocks: &Vec<Vec<Instr>>, b: usize| blocks[b].last().is_none_or(|l| falls_through(l.op));
        let mut warm = vec![false; n];
        let mut visited = vec![false; n];
        let entry = self.order[0];
        visited[entry] = true;
        let mut stack = vec![entry];
        while let Some(b) = stack.pop() {
            warm[b] = true;
            let next = next_of[b];
            if falls(&self.blocks, b) && next != NO_TARGET && !visited[next] {
                visited[next] = true;
                stack.push(next);
            }
            for ins in &self.blocks[b] {
                if is_jump(ins.op) && ins.target != NO_TARGET && !visited[ins.target] {
                    visited[ins.target] = true;
                    stack.push(ins.target);
                }
            }
        }
        let mut cold = vec![false; n];
        for &b in &self.order {
            if self.handlers.contains(&b) && !visited[b] {
                visited[b] = true;
                stack.push(b);
            }
        }
        while let Some(b) = stack.pop() {
            cold[b] = true;
            let next = next_of[b];
            if falls(&self.blocks, b) && next != NO_TARGET && !warm[next] && !visited[next] {
                visited[next] = true;
                stack.push(next);
            }
            for ins in &self.blocks[b] {
                if is_jump(ins.op) && ins.target != NO_TARGET && !warm[ins.target] && !visited[ins.target] {
                    visited[ins.target] = true;
                    stack.push(ins.target);
                }
            }
        }
        if !cold.iter().any(|&c| c) {
            return;
        }
        let mut p = 0;
        while p + 1 < self.order.len() {
            let (b, next) = (self.order[p], self.order[p + 1]);
            if cold[b] && falls(&self.blocks, b) && warm[next] {
                let jump = self.new_block();
                self.blocks[jump].push(Instr { target: next, ..Instr::new(JUMP, 0, NO_LOC) });
                cold.push(true);
                self.order.insert(p + 1, jump);
                p += 1;
            }
            p += 1;
        }
        // O salto que sai de um bloco frio para um quente não confere o `eval breaker`: `JUMP_NO_INTERRUPT`. Dentro da
        // região fria (o resto de uma função depois de um `async for`, que só sai pelo tratador do `END_ASYNC_FOR`) o
        // `JUMP` continua `JUMP`. O salto que o `jump_thread` criou segue o bloco do salto por onde passou
        // (`Instr::via`), não o em que ele caiu.
        for &b in &self.order {
            for ins in self.blocks[b].iter_mut().filter(|i| i.op == JUMP) {
                let origin = if ins.via == NO_TARGET { b } else { ins.via };
                if cold[origin] && ins.target != NO_TARGET && !cold[ins.target] {
                    ins.op = suspend::JUMP_NO_INTERRUPT;
                }
            }
        }
        let (warm_order, cold_order): (Vec<usize>, Vec<usize>) = self.order.iter().copied().partition(|&b| !cold[b]);
        self.order = warm_order;
        self.order.extend(cold_order);
        self.remove_redundant_jumps();
    }

    /// `convert_pseudo_ops`: o que sobrou de `SETUP_*` e `POP_BLOCK` vira `NOP`, e os `NOP` sem utilidade saem.
    fn convert_pseudo_ops(&mut self) {
        let mut converted = false;
        for blk in &mut self.blocks {
            for ins in blk.iter_mut() {
                if ins.op == suspend::STORE_FAST_MAYBE_NULL {
                    ins.op = STORE_FAST;
                }
                if is_block_push(ins.op) || ins.op == POP_BLOCK {
                    ins.op = NOP;
                    ins.arg = 0;
                    ins.target = NO_TARGET;
                    converted = true;
                }
            }
        }
        if converted {
            self.remove_redundant_nops();
        }
    }

    /// Achata o grafo, resolve os saltos e gera o `co_code`, a lista de localizações por instrução e a tabela de
    /// exceções (`startdepth` é a profundidade de entrada de cada bloco). Salto incondicional para trás vira
    /// `JUMP_BACKWARD`; `None` se sobrou salto condicional para trás ou pseudo-opcode.
    fn assemble(&self, startdepth: &[i64]) -> Option<(Vec<u8>, Vec<(Loc, usize)>, Vec<u8>)> {
        struct Flat {
            op: u16,
            arg: i64,
            target: usize,
            loc: Loc,
            handler: usize,
        }
        let mut flat: Vec<Flat> = Vec::new();
        let mut start_of = vec![NO_TARGET; self.blocks.len()];
        for &b in &self.order {
            start_of[b] = flat.len();
            for ins in &self.blocks[b] {
                flat.push(Flat { op: ins.op, arg: ins.arg, target: ins.target, loc: ins.loc, handler: ins.handler });
            }
        }
        let n = flat.len();
        let mut target_idx = vec![NO_TARGET; n];
        for (i, f) in flat.iter_mut().enumerate() {
            if is_jump(f.op) {
                let t = *start_of.get(f.target)?;
                if t == NO_TARGET {
                    return None;
                }
                if f.op == JUMP {
                    f.op = if t > i { JUMP_FORWARD } else { JUMP_BACKWARD };
                } else if f.op == suspend::JUMP_NO_INTERRUPT {
                    f.op = if t > i { JUMP_FORWARD } else { suspend::JUMP_BACKWARD_NO_INTERRUPT };
                } else if t <= i {
                    return None;
                }
                target_idx[i] = t;
            }
            if f.op == LOAD_CLOSURE {
                f.op = LOAD_FAST;
            }
            if f.op >= 256 {
                return None;
            }
        }
        let caches: Vec<usize> = flat.iter().map(|f| cpyops::caches_of(f.op as u8)).collect();
        let ext_count = |arg: i64| -> usize {
            if arg > 0xFF_FFFF {
                3
            } else if arg > 0xFFFF {
                2
            } else if arg > 0xFF {
                1
            } else {
                0
            }
        };
        let mut ext: Vec<usize> = flat.iter().enumerate().map(|(i, f)| if target_idx[i] != NO_TARGET { 0 } else { ext_count(f.arg) }).collect();
        let mut offsets = vec![0usize; n + 1];
        let mut args: Vec<i64> = flat.iter().map(|f| f.arg).collect();
        loop {
            for i in 0..n {
                offsets[i + 1] = offsets[i] + 1 + ext[i] + caches[i];
            }
            let mut changed = false;
            for i in 0..n {
                if target_idx[i] != NO_TARGET {
                    let (end, target) = (offsets[i + 1], offsets[target_idx[i]]);
                    let backward = matches!(flat[i].op, JUMP_BACKWARD | suspend::JUMP_BACKWARD_NO_INTERRUPT);
                    let distance = if backward { end - target } else { target - end };
                    let arg = distance as i64;
                    args[i] = arg;
                    let need = ext_count(arg);
                    if need > ext[i] {
                        ext[i] = need;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let mut code = Vec::with_capacity(offsets[n] * 2);
        let mut locs = Vec::with_capacity(n);
        for i in 0..n {
            for k in (1..=ext[i]).rev() {
                code.push(cpyops::EXTENDED_ARG);
                code.push(((args[i] >> (8 * k)) & 0xFF) as u8);
            }
            code.push(flat[i].op as u8);
            code.push((args[i] & 0xFF) as u8);
            for _ in 0..caches[i] {
                code.extend_from_slice(&[0, 0]);
            }
            locs.push((flat[i].loc, 1 + ext[i] + caches[i]));
        }
        // `assemble_exception_table`: um intervalo por trecho seguido de instruções com o mesmo tratador.
        let mut table = Vec::new();
        let mut open: Option<(usize, usize)> = None;
        for i in 0..=n {
            let h = if i < n { flat[i].handler } else { NO_TARGET };
            if open.is_some_and(|(_, cur)| cur == h) {
                continue;
            }
            if let Some((from, cur)) = open {
                let target = *start_of.get(cur)?;
                let depth_entry = *startdepth.get(cur)?;
                if target == NO_TARGET || depth_entry == i64::MIN {
                    return None;
                }
                let lasti = self.lasti.contains(&cur);
                let depth = depth_entry - 1 - i64::from(lasti);
                if depth < 0 {
                    return None;
                }
                write_except_varint(&mut table, from as u32, true);
                write_except_varint(&mut table, (offsets[i] - from) as u32, false);
                write_except_varint(&mut table, offsets[target] as u32, false);
                write_except_varint(&mut table, ((depth as u32) << 1) | u32::from(lasti), false);
            }
            open = (h != NO_TARGET).then_some((offsets[i], h));
        }
        Some((code, locs, table))
    }
}

/// `assemble_emit_exception_table_item`: varint de blocos de 6 bits, o mais significativo primeiro; o bit 7 marca o
/// primeiro campo de cada entrada.
fn write_except_varint(out: &mut Vec<u8>, value: u32, first: bool) {
    let mut msb = if first { 0x80 } else { 0 };
    for shift in [24u32, 18, 12, 6] {
        if value >= (1 << shift) {
            out.push((((value >> shift) & 0x3F) as u8) | 0x40 | msb);
            msb = 0;
        }
    }
    out.push(((value & 0x3F) as u8) | msb);
}

// ---------------------------------------------------------------------------------------------------------
// Geração a partir da árvore (`codegen.c`)

/// A construção ainda não coberta: o chamador cai no esqueleto.
#[derive(Debug)]
pub struct Unsupported;

type Res<T> = Result<T, Unsupported>;

/// Chave de uma constante em `co_consts`: tipos distintos (`1`, `1.0`, `True`) não se fundem.
#[derive(Debug, Clone, PartialEq)]
enum Key {
    None,
    Bool(bool),
    Int(i64),
    /// Inteiro que não cabe em 64 bits, em dígitos decimais.
    Big(String),
    Float(u64),
    Str(String),
    Bytes(Vec<u8>),
    Ellipsis,
    Tuple(Vec<Key>),
    /// `frozenset` de constantes, na ordem dos elementos como o CPython os dobra.
    FrozenSet(Vec<Key>),
    /// O n-ésimo código de função aninhada (`Code::functions`).
    Code(usize),
    /// `complex`: os bits da parte real e da imaginária (`-0.0` e `0.0` são constantes distintas).
    Complex(u64, u64),
}

/// Uma constante já dobrada: a chave de `co_consts` e o valor.
#[derive(Clone)]
struct Cv {
    key: Key,
    value: Value,
}

impl Cv {
    fn none() -> Cv {
        Cv { key: Key::None, value: Value::None }
    }

    fn int(n: i64) -> Cv {
        Cv { key: Key::Int(n), value: Value::Int(n) }
    }

    /// Um inteiro qualquer: `Int` se cabe em 64 bits, senão `Big` com os dígitos decimais.
    fn integer(n: BigInt) -> Cv {
        match n.to_i64() {
            Some(i) => Cv::int(i),
            None => Cv { key: Key::Big(crate::bigint::to_radix(&n, 10)), value: crate::bigint::norm(n) },
        }
    }

    /// Um literal inteiro, em dígitos decimais.
    fn int_literal(digits: &str) -> Res<Cv> {
        Ok(Cv::integer(crate::bigint::parse(digits, 10).ok_or(Unsupported)?))
    }

    fn float(x: f64) -> Cv {
        Cv { key: Key::Float(x.to_bits()), value: Value::Float(x) }
    }

    fn str(s: String) -> Cv {
        Cv { key: Key::Str(s.clone()), value: Value::str(s) }
    }

    fn bytes(b: Vec<u8>) -> Cv {
        Cv { key: Key::Bytes(b.clone()), value: Value::bytes(b) }
    }

    fn tuple(items: Vec<Cv>) -> Cv {
        let key = Key::Tuple(items.iter().map(|c| c.key.clone()).collect());
        Cv { key, value: Value::tuple(items.into_iter().map(|c| c.value).collect()) }
    }

    /// O `frozenset` em que o otimizador de AST dobra um conjunto de constantes (`fold_iter`, `starunpack_helper`).
    fn frozenset(items: Vec<Cv>) -> Res<Cv> {
        let key = Key::FrozenSet(items.iter().map(|c| c.key.clone()).collect());
        let mut set = Set::new();
        for item in items {
            set.add(item.value).map_err(|_| Unsupported)?;
        }
        Ok(Cv { key, value: Value::frozenset(set) })
    }
}

/// A constante de um literal; o que o emissor não representa (inteiro fora de 64 bits) é `Unsupported`.
fn const_cv(c: &Constant) -> Res<Cv> {
    Ok(match c {
        Constant::None => Cv::none(),
        Constant::Bool(b) => Cv { key: Key::Bool(*b), value: Value::Bool(*b) },
        Constant::Int(s) => Cv::int_literal(s)?,
        Constant::Float(x) => Cv::float(*x),
        Constant::Str(s) => Cv::str(s.clone()),
        Constant::Bytes(b) => Cv::bytes(b.clone()),
        Constant::Ellipsis => Cv { key: Key::Ellipsis, value: Value::Builtin("Ellipsis") },
        Constant::Complex(re, im) => Cv::complex(*re, *im),
    })
}

/// O inteiro (`bool`, `int` ou `int` grande) de uma constante: `bool` conta como inteiro, como no `PyNumber_*`.
fn int_of(k: &Key) -> Option<BigInt> {
    match k {
        Key::Int(n) => Some(BigInt::from(*n)),
        Key::Bool(b) => Some(BigInt::from(i64::from(*b))),
        Key::Big(digits) => crate::bigint::parse(digits, 10),
        _ => None,
    }
}

/// O `float` de uma constante numérica; `Some(None)` se um inteiro grande não cabe em `float` (`OverflowError`).
fn float_of(k: &Key) -> Option<Option<f64>> {
    match k {
        Key::Float(bits) => Some(Some(f64::from_bits(*bits))),
        _ => int_of(k).map(|n| crate::bigint::to_f64(&n).ok()),
    }
}

/// Bits que `safe_multiply`, `safe_power` e `safe_lshift` do `ast_opt.c` aceitam num resultado inteiro (`MAX_INT_SIZE`).
const MAX_INT_BITS: u64 = 128;

/// Dobramento de inteiros (`fold_binop` do `ast_opt.c`), com os limites de tamanho do `safe_*`. `Ok(None)`: a operação
/// falha em tempo de execução ou passa do limite (o CPython deixa a expressão como está).
fn fold_int(op: Operator, x: &BigInt, y: &BigInt) -> Res<Option<Cv>> {
    let nonzero = !x.is_zero() && !y.is_zero();
    let (xb, yb) = (x.bits() as u64, y.bits() as u64);
    let too_big = match op {
        Operator::Mult => nonzero && xb + yb > MAX_INT_BITS,
        Operator::Pow if !x.is_zero() && y.is_positive() => y.to_u64().is_none_or(|e| xb > MAX_INT_BITS / e),
        Operator::LShift if nonzero => y.to_u64().is_none_or(|s| s > MAX_INT_BITS || xb > MAX_INT_BITS - s),
        _ => false,
    };
    if too_big {
        return Ok(None);
    }
    Ok(match crate::bigint::binary(op, x, y) {
        Ok(Value::Int(n)) => Some(Cv::int(n)),
        Ok(Value::Big(b)) => Some(Cv::integer((*b).clone())),
        Ok(Value::Float(f)) => Some(Cv::float(f)),
        _ => None,
    })
}

/// Dobramento de `float` (ao menos um operando é `float`).
fn fold_float(op: Operator, a: f64, b: f64) -> Res<Option<Cv>> {
    Ok(match op {
        Operator::Add => Some(Cv::float(a + b)),
        Operator::Sub => Some(Cv::float(a - b)),
        Operator::Mult => Some(Cv::float(a * b)),
        Operator::Div if b == 0.0 => None,
        Operator::Div => Some(Cv::float(a / b)),
        Operator::FloorDiv | Operator::Mod | Operator::Pow => return Err(Unsupported),
        Operator::LShift | Operator::RShift | Operator::BitAnd | Operator::BitOr | Operator::BitXor | Operator::MatMult => None,
    })
}

fn fold_binop(op: Operator, l: &Cv, r: &Cv) -> Res<Option<Cv>> {
    if matches!(l.key, Key::Complex(..)) || matches!(r.key, Key::Complex(..)) {
        return complex::fold_binop(op, &l.key, &r.key);
    }
    if let (Some(a), Some(b)) = (int_of(&l.key), int_of(&r.key)) {
        return fold_int(op, &a, &b);
    }
    if let (Some(a), Some(b)) = (float_of(&l.key), float_of(&r.key)) {
        return match (a, b) {
            (Some(a), Some(b)) => fold_float(op, a, b),
            _ => Ok(None),
        };
    }
    match (&l.key, &r.key, op) {
        (Key::Str(a), Key::Str(b), Operator::Add) => Ok(Some(Cv::str(format!("{a}{b}")))),
        (Key::Bytes(a), Key::Bytes(b), Operator::Add) => Ok(Some(Cv::bytes([a.as_slice(), b.as_slice()].concat()))),
        (Key::Tuple(_), Key::Tuple(_), Operator::Add) => {
            let (mut items, rest) = (tuple_items(l), tuple_items(r));
            items.extend(rest);
            Ok(Some(Cv::tuple(items)))
        }
        (seq, count, Operator::Mult) if is_seq(seq) => Ok(int_of(count).and_then(|n| fold_seq_mult(l, &n))),
        (count, seq, Operator::Mult) if is_seq(seq) => Ok(int_of(count).and_then(|n| fold_seq_mult(r, &n))),
        // `safe_mod` não dobra formatação de texto; o resto é `TypeError` em tempo de execução.
        _ => Ok(None),
    }
}

fn is_seq(k: &Key) -> bool {
    matches!(k, Key::Str(_) | Key::Bytes(_) | Key::Tuple(_))
}

/// Os elementos de uma tupla constante, cada um com a chave e o valor.
fn tuple_items(t: &Cv) -> Vec<Cv> {
    match (&t.key, &t.value) {
        (Key::Tuple(keys), Value::Tuple(vals)) => {
            keys.iter().zip(vals.iter()).map(|(k, v)| Cv { key: k.clone(), value: v.clone() }).collect()
        }
        _ => Vec::new(),
    }
}

/// Itens que `check_complexity` do `ast_opt.c` aceita numa tupla repetida (`MAX_TOTAL_ITEMS`), contando os aninhados.
const MAX_TOTAL_ITEMS: i64 = 1024;
/// Tamanho máximo de uma tupla repetida (`MAX_COLLECTION_SIZE`) e de um texto ou bytes repetido (`MAX_STR_SIZE`).
const MAX_COLLECTION_SIZE: i64 = 256;
const MAX_STR_SIZE: i64 = 4096;

/// O limite que sobra depois de descontar os itens de `k` (tuplas aninhadas inclusive); `None` se estoura.
fn complexity_left(k: &Key, limit: i64) -> Option<i64> {
    let Key::Tuple(items) = k else { return Some(limit) };
    let mut left = limit - items.len() as i64;
    for item in items {
        if left < 0 {
            break;
        }
        left = complexity_left(item, left)?;
    }
    (left >= 0).then_some(left)
}

/// `safe_multiply` do `ast_opt.c` para sequência vezes inteiro: `None` quando falha ou passa dos limites.
fn fold_seq_mult(seq: &Cv, n: &BigInt) -> Option<Cv> {
    let size = match &seq.key {
        Key::Str(s) => s.chars().count(),
        Key::Bytes(b) => b.len(),
        Key::Tuple(t) => t.len(),
        _ => return None,
    };
    let count = n.to_i64()?;
    let is_tuple = matches!(seq.key, Key::Tuple(_));
    if size != 0 {
        let max = if is_tuple { MAX_COLLECTION_SIZE } else { MAX_STR_SIZE };
        if count < 0 || count > max / size as i64 {
            return None;
        }
        if is_tuple && count != 0 {
            complexity_left(&seq.key, MAX_TOTAL_ITEMS / count)?;
        }
    }
    let times = count.max(0) as usize;
    Some(match &seq.key {
        Key::Str(s) => Cv::str(s.repeat(times)),
        Key::Bytes(b) => Cv::bytes(b.repeat(times)),
        _ => Cv::tuple(tuple_items(seq).iter().cloned().cycle().take(size * times).collect()),
    })
}

/// `PyObject_GetItem` de uma constante por um inteiro (`fold_subscr`); `None` se levanta (índice fora, tipo errado).
fn fold_subscript(v: &Cv, index: &Cv) -> Option<Cv> {
    let i = int_of(&index.key)?.to_i64()?;
    let len = match &v.key {
        Key::Str(s) => s.chars().count(),
        Key::Bytes(b) => b.len(),
        Key::Tuple(t) => t.len(),
        _ => return None,
    };
    let at = usize::try_from(if i < 0 { i + len as i64 } else { i }).ok().filter(|&p| p < len)?;
    Some(match &v.key {
        Key::Str(s) => Cv::str(s.chars().nth(at)?.to_string()),
        Key::Bytes(b) => Cv::int(i64::from(b[at])),
        _ => tuple_items(v).swap_remove(at),
    })
}

fn fold_unary(op: UnaryOp, v: &Cv) -> Res<Option<Cv>> {
    if matches!(v.key, Key::Complex(..)) {
        return if op == UnaryOp::Not { Err(Unsupported) } else { Ok(complex::fold_unary(op, &v.key)) };
    }
    if let Key::Float(bits) = v.key {
        let x = f64::from_bits(bits);
        return match op {
            UnaryOp::USub => Ok(Some(Cv::float(-x))),
            UnaryOp::UAdd => Ok(Some(Cv::float(x))),
            UnaryOp::Not => Err(Unsupported),
            _ => Ok(None),
        };
    }
    let Some(n) = int_of(&v.key) else { return if op == UnaryOp::Not { Err(Unsupported) } else { Ok(None) } };
    match op {
        UnaryOp::USub => Ok(Some(Cv::integer(-n))),
        UnaryOp::UAdd => Ok(Some(Cv::integer(n))),
        UnaryOp::Invert if matches!(v.key, Key::Bool(_)) => Err(Unsupported),
        UnaryOp::Invert => Ok(Some(Cv::integer(-n - 1))),
        _ => Err(Unsupported),
    }
}

/// O otimizador de AST do CPython (`ast_opt.c`): a constante em que `e` se dobra, se for o caso. `Ok(None)`: o
/// CPython não dobra; `Err`: dobraria, mas o resultado não cabe no que o emissor representa.
/// A verdade de uma constante (`PyObject_IsTrue`).
fn key_truth(k: &Key) -> bool {
    match k {
        Key::None => false,
        Key::Bool(b) => *b,
        Key::Int(n) => *n != 0,
        Key::Big(_) => true,
        Key::Float(bits) => f64::from_bits(*bits) != 0.0,
        Key::Str(s) => !s.is_empty(),
        Key::Bytes(b) => !b.is_empty(),
        Key::Tuple(items) | Key::FrozenSet(items) => !items.is_empty(),
        Key::Ellipsis | Key::Code(_) => true,
        Key::Complex(re, im) => complex::truth(*re, *im),
    }
}

fn fold(e: &Expr) -> Res<Option<Cv>> {
    Ok(match &e.kind {
        E::Constant { value, .. } => Some(const_cv(value)?),
        // `fold_name` do `ast_opt.c`: sem `-O`, `__debug__` lido é a constante `True`.
        E::Name { id, ctx: ExprContext::Load } if id == "__debug__" => Some(Cv { key: Key::Bool(true), value: Value::Bool(true) }),
        E::UnaryOp { op, operand } => match fold(operand)? {
            Some(v) => fold_unary(*op, &v)?,
            None => None,
        },
        E::BinOp { left, op, right } => match (fold(left)?, fold(right)?) {
            (Some(l), Some(r)) => fold_binop(*op, &l, &r)?,
            _ => None,
        },
        E::Tuple { elts, ctx: ExprContext::Load } => fold_all(elts)?.map(Cv::tuple),
        E::Subscript { value, slice, ctx: ExprContext::Load } => match (fold(value)?, fold(slice)?) {
            (Some(v), Some(i)) => fold_subscript(&v, &i),
            _ => None,
        },
        _ => None,
    })
}

/// A dobra de cada expressão, ou `None` se alguma não é constante.
fn fold_all<'e>(items: impl IntoIterator<Item = &'e Expr>) -> Res<Option<Vec<Cv>>> {
    let mut out = Vec::new();
    for x in items {
        match fold(x)? {
            Some(c) => out.push(c),
            None => return Ok(None),
        }
    }
    Ok(Some(out))
}

enum NameKind {
    Fast(usize),
    Deref(usize),
    Global,
    Name,
}

/// Um laço aberto, para `break`, `continue` e `return` (`fblockinfo`).
#[derive(Clone, Copy)]
struct Loop {
    /// Destino do `continue`.
    start: usize,
    /// Destino do `break`.
    exit: usize,
    /// `for` guarda o iterador na pilha; `while` não.
    is_for: bool,
}

/// Um bloco de quadro aberto (`fblockinfo`): o que `return`, `break` e `continue` desfazem ao sair dele
/// (`compiler_unwind_fblock`).
#[derive(Clone)]
enum Fb {
    Loop(Loop),
    /// O corpo de um `try` com `except`: sair dele é um `POP_BLOCK`.
    TryExcept,
    /// A região dos tratadores de um `try`: nada a desfazer por si só.
    ExcHandler,
    /// O corpo de um tratador; leva o nome ligado por `except E as nome`.
    HandlerCleanup(Option<String>),
    /// `with`: sair dele é `POP_BLOCK` e a chamada de `__exit__(None, None, None)`; leva a localização do comando.
    With(Loc),
    /// `async with`: como `With`, mas a chamada de `__exit__` é esperada (`await`).
    AsyncWith(Loc),
    /// O corpo de um `try`/`finally`: sair dele é `POP_BLOCK` e o corpo do `finally` repetido (`FINALLY_TRY`).
    FinallyTry(Rc<Vec<Stmt>>),
    /// O `finally` do caminho de exceção: sair dele solta a exceção e o estado (`FINALLY_END`).
    FinallyEnd,
    /// O valor que um `return` guarda enquanto o `finally` roda: sair dele o descarta (`POP_VALUE`).
    PopValue,
}

/// A coleção que `starunpack_helper` monta.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Seq {
    List,
    Tuple,
    Set,
}

fn is_starred(e: &Expr) -> bool {
    matches!(e.kind, E::Starred { .. })
}

/// `x[a:b]` (sem passo): `BINARY_SLICE` e `STORE_SLICE` em vez de `BUILD_SLICE` e `BINARY_SUBSCR`.
fn is_two_element_slice(e: &Expr) -> bool {
    matches!(&e.kind, E::Slice { step: None, .. })
}

/// `fold_unaryop` do `ast_opt.c`: `not (a is b)` vira `a is not b` (e o mesmo com `in`), na posição da comparação.
fn flip_not_compare(operand: &Expr) -> Option<Expr> {
    let E::Compare { ops, .. } = &operand.kind else { return None };
    if ops.len() != 1 {
        return None;
    }
    let flipped = match ops[0] {
        CmpOp::Is => CmpOp::IsNot,
        CmpOp::IsNot => CmpOp::Is,
        CmpOp::In => CmpOp::NotIn,
        CmpOp::NotIn => CmpOp::In,
        _ => return None,
    };
    let mut copy = operand.clone();
    if let E::Compare { ops, .. } = &mut copy.kind {
        ops[0] = flipped;
    }
    Some(copy)
}

/// `co_varnames`, depois as células que não são parâmetros, depois as variáveis livres: os índices de
/// `LOAD_FAST`, `LOAD_DEREF`, `MAKE_CELL` e `_varname_from_oparg` (o `localsplus` do CPython).
pub fn localsplus(code: &Code) -> Vec<Rc<str>> {
    let l = layout(code);
    let mut all = l.varnames.clone();
    all.extend(l.cellvars.iter().filter(|c| !l.varnames.contains(c)).cloned());
    all.extend(l.freevars.iter().cloned());
    all
}

struct Gen<'a> {
    cfg: Cfg,
    function: bool,
    code: &'a Code,
    localsplus: Vec<Rc<str>>,
    globals_decl: &'a HashSet<String>,
    imports: &'a HashSet<String>,
    consts: Vec<Value>,
    keys: Vec<Key>,
    names: Vec<Rc<str>>,
    fblocks: Vec<Fb>,
    /// Quantas funções aninhadas já foram criadas (índice em `Code::functions`).
    next_fn: usize,
    /// Os alvos das compreensões inline abertas (a mais interna por último): nome, índice no `localsplus` e se uma função
    /// de dentro o fecha (célula). Dentro da compreensão o nome é local, seja qual for o escopo de fora.
    comp_locals: Vec<(String, usize, bool)>,
    /// Nomes de `co_varnames` que só existem por serem alvo de compreensão inline: fora dela o nome é global (ou livre),
    /// não o local rápido escondido.
    comp_only: HashSet<String>,
    /// `from __future__ import annotations`: as anotações viram o texto delas (`annexpr`) em vez de serem avaliadas.
    future: bool,
    /// `compile(..., 'single')` no módulo: a expressão solta passa por `INTRINSIC_PRINT` (`c_interactive`).
    interactive: bool,
}

/// `update_start_location_to_match_attr`: a localização de um acesso a atributo que atravessa linhas passa a
/// apontar o nome do atributo.
fn match_attr(mut loc: Loc, attr_loc: Loc, attr: &str) -> Loc {
    if loc.line != attr_loc.end_line {
        loc.line = attr_loc.end_line;
        let len = attr.chars().count() as i32;
        loc.col = if len <= attr_loc.end_col { attr_loc.end_col - len } else { attr_loc.end_col };
        loc.end_line = loc.end_line.max(loc.line);
    }
    loc
}

/// Índice de `NB_*` do operador (a forma `inplace` soma 13).
fn nb_op(op: Operator) -> i64 {
    match op {
        Operator::Add => 0,
        Operator::BitAnd => 1,
        Operator::FloorDiv => 2,
        Operator::LShift => 3,
        Operator::MatMult => 4,
        Operator::Mult => 5,
        Operator::Mod => 6,
        Operator::BitOr => 7,
        Operator::Pow => 8,
        Operator::RShift => 9,
        Operator::Sub => 10,
        Operator::Div => 11,
        Operator::BitXor => 12,
    }
}

/// `(cmp << 5) | máscara` do `COMPARE_OP` (`compiler_addcompare`).
fn compare_arg(op: CmpOp) -> Option<i64> {
    Some(match op {
        CmpOp::Lt => 2,
        CmpOp::LtE => (1 << 5) | 10,
        CmpOp::Eq => (2 << 5) | 8,
        CmpOp::NotEq => (3 << 5) | 7,
        CmpOp::Gt => (4 << 5) | 4,
        CmpOp::GtE => (5 << 5) | 12,
        _ => return None,
    })
}

impl<'a> Gen<'a> {
    fn new(
        function: bool,
        code: &'a Code,
        globals_decl: &'a HashSet<String>,
        imports: &'a HashSet<String>,
        resume: Loc,
    ) -> Gen<'a> {
        let mut g = Gen {
            cfg: Cfg::new(),
            function,
            code,
            localsplus: localsplus(code),
            globals_decl,
            imports,
            consts: Vec::new(),
            keys: Vec::new(),
            names: Vec::new(),
            fblocks: Vec::new(),
            next_fn: 0,
            comp_locals: Vec::new(),
            comp_only: HashSet::new(),
            future: false,
            interactive: false,
        };
        g.cfg.push(Instr::new(RESUME, 0, resume));
        g
    }

    fn loc(&self, pos: &Pos) -> Loc {
        Loc::of(pos)
    }

    fn add(&mut self, op: u16, arg: i64, loc: Loc) {
        self.cfg.push(Instr::new(op, arg, loc));
    }

    fn add_jump(&mut self, op: u16, target: usize, loc: Loc) {
        self.cfg.push(Instr { target, ..Instr::new(op, 0, loc) });
    }

    fn add_const(&mut self, key: Key, value: Value) -> usize {
        if let Some(i) = self.keys.iter().position(|k| *k == key) {
            return i;
        }
        self.keys.push(key);
        self.consts.push(value);
        self.consts.len() - 1
    }

    fn add_cv(&mut self, cv: Cv) -> usize {
        self.add_const(cv.key, cv.value)
    }

    /// `LOAD_CONST` da constante `cv`.
    fn load_cv(&mut self, cv: Cv, loc: Loc) {
        let i = self.add_cv(cv);
        self.add(LOAD_CONST, i as i64, loc);
    }

    /// `LOAD_CONST` da constante em que `e` se dobra; `false` se `e` não é constante.
    fn load_folded(&mut self, e: &Expr) -> Res<bool> {
        let Some(cv) = fold(e)? else { return Ok(false) };
        let loc = self.loc(&e.pos);
        self.load_cv(cv, loc);
        Ok(true)
    }

    fn name_index(&mut self, n: &str) -> usize {
        if let Some(i) = self.names.iter().position(|x| &**x == n) {
            return i;
        }
        self.names.push(Rc::from(n));
        self.names.len() - 1
    }

    /// Nome que vira texto de constante (nome de parâmetro de tipo, de alias): o `__x` chega mutilado do AST (`mangle.rs`),
    /// mas a constante leva o original, que o emissor não recupera.
    fn check_plain(&self, n: &str) -> Res<()> {
        if n.starts_with("__") && !n.ends_with("__") {
            return Err(Unsupported);
        }
        self.check_name(n)
    }

    /// Nomes que o compilador trata à parte (`__debug__`, `__class__` fora de uma função que a fecha). O mangling de `__x`
    /// já veio feito no AST do corpo da classe (`mangle::class_body`), então atributos e nomes passam como estão.
    fn check_name(&self, n: &str) -> Res<()> {
        let class_cell = n == "__class__" && self.code.freevars.iter().any(|f| &**f == n);
        if (n == "__class__" && !class_cell) || n == "__debug__" {
            return Err(Unsupported);
        }
        Ok(())
    }

    /// `can_optimize_super_call` e `load_args_for_super`: `super().a`, `super(C, x).a` e a chamada de método sobre eles
    /// carregam o atributo com um `LOAD_SUPER_ATTR` só (`global super`, a classe e o objeto na pilha), sem criar o objeto
    /// `super`. `false` (sem emitir nada) se a expressão não é isso: `super` ligado a outra coisa, argumentos nomeados ou
    /// com `*`, ou `super()` sem argumentos numa função sem parâmetros ou sem a célula `__class__`.
    fn super_attr(&mut self, value: &Expr, attr: &str, method: bool, attr_loc: Loc) -> Res<bool> {
        let E::Call { func, args, keywords } = &value.kind else { return Ok(false) };
        let E::Name { id, .. } = &func.kind else { return Ok(false) };
        if id != "super"
            || attr == "__class__"
            || !keywords.is_empty()
            || self.imports.contains("super")
            || self.imports.contains(SUPER_BOUND)
        {
            return Ok(false);
        }
        if !matches!(self.name_kind("super"), NameKind::Global | NameKind::Name) || self.globals_decl.contains("super") {
            return Ok(false);
        }
        let first_param = match args.len() {
            2 if !args.iter().any(is_starred) => None,
            0 => {
                let has_cell = self.code.freevars.iter().any(|f| &**f == "__class__");
                match self.code.params.first() {
                    Some(p) if self.function && has_cell => Some(p.clone()),
                    _ => return Ok(false),
                }
            }
            _ => return Ok(false),
        };
        // `VISIT(c, expr, e->v.Call.func)`: o nome `super` leva a posição dele; `__class__` e o primeiro parâmetro, a da chamada.
        let sloc = self.loc(&value.pos);
        let super_loc = self.loc(&func.pos);
        self.load_name("super", super_loc, false)?;
        match first_param {
            None => {
                for a in args {
                    self.expr(a)?;
                }
            }
            Some(p) => {
                self.load_name("__class__", sloc, false)?;
                self.load_name(&p, sloc, false)?;
            }
        }
        let i = self.name_index(attr);
        let two_args = i64::from(!args.is_empty()) << 1;
        let oparg = ((i as i64) << 2) | two_args | i64::from(method);
        self.add(LOAD_SUPER_ATTR, oparg, attr_loc);
        // O `NOP` com a localização que aponta o nome do atributo some se a linha da instrução seguinte é a mesma.
        self.add(NOP, 0, match_attr(attr_loc, attr_loc, attr));
        Ok(true)
    }

    fn name_kind(&self, id: &str) -> NameKind {
        if let Some((_, slot, cell)) = self.comp_locals.iter().rev().find(|(n, ..)| n == id) {
            return if *cell { NameKind::Deref(*slot) } else { NameKind::Fast(*slot) };
        }
        if !self.function {
            return NameKind::Name;
        }
        if self.globals_decl.contains(id) {
            return NameKind::Global;
        }
        if self.comp_only.contains(id) {
            // O nome em `co_varnames` é só o alvo escondido da compreensão: fora dela vale o global, ou a variável livre.
            let free = self.code.freevars.iter().any(|n| &**n == id);
            return match self.localsplus.iter().rposition(|v| &**v == id) {
                Some(i) if free => NameKind::Deref(i),
                _ => NameKind::Global,
            };
        }
        let in_cell = self.code.cellvars.iter().chain(&self.code.freevars).any(|n| &**n == id);
        match self.localsplus.iter().position(|v| &**v == id) {
            Some(i) if in_cell => NameKind::Deref(i),
            Some(i) => NameKind::Fast(i),
            None => NameKind::Global,
        }
    }

    /// Índice no `localsplus` da variável livre `id` lida no corpo de uma classe (o `ClassBlock` do `compiler_nameop`);
    /// `None` fora do corpo da classe e para o que não é variável livre dele.
    fn class_free(&self, id: &str) -> Option<usize> {
        if self.function || !self.code.is_class || self.comp_locals.iter().any(|(n, ..)| n == id) {
            return None;
        }
        if !self.code.freevars.iter().any(|n| &**n == id) {
            return None;
        }
        self.localsplus.iter().rposition(|v| &**v == id)
    }

    /// Carrega `id`. Devolve se quem chama ainda precisa de `PUSH_NULL` (global em posição de chamada já
    /// carrega o `NULL` junto, bit 0 do `oparg`).
    fn load_name(&mut self, id: &str, loc: Loc, call: bool) -> Res<bool> {
        self.check_name(id)?;
        // No corpo de uma classe a variável livre (de função de fora) vem do dicionário local ou, faltando, da célula.
        if let Some(i) = self.class_free(id) {
            self.add(generic::LOAD_LOCALS, 0, loc);
            self.add(generic::LOAD_FROM_DICT_OR_DEREF, i as i64, loc);
            return Ok(true);
        }
        Ok(match self.name_kind(id) {
            NameKind::Fast(i) => {
                self.add(LOAD_FAST, i as i64, loc);
                true
            }
            NameKind::Deref(i) => {
                self.add(LOAD_DEREF, i as i64, loc);
                true
            }
            NameKind::Global => {
                let i = self.name_index(id);
                self.add(LOAD_GLOBAL, ((i as i64) << 1) | i64::from(call), loc);
                !call
            }
            NameKind::Name => {
                let i = self.name_index(id);
                self.add(LOAD_NAME, i as i64, loc);
                true
            }
        })
    }

    fn store_name(&mut self, id: &str, loc: Loc) -> Res<()> {
        self.check_name(id)?;
        match self.name_kind(id) {
            NameKind::Fast(i) => self.add(STORE_FAST, i as i64, loc),
            NameKind::Deref(i) => self.add(STORE_DEREF, i as i64, loc),
            NameKind::Global if self.globals_decl.contains(id) => {
                let i = self.name_index(id);
                self.add(STORE_GLOBAL, i as i64, loc);
            }
            NameKind::Global => return Err(Unsupported),
            NameKind::Name => {
                let i = self.name_index(id);
                self.add(STORE_NAME, i as i64, loc);
            }
        }
        Ok(())
    }

    /// `DELETE_*` do nome, na mesma classificação de `store_name`.
    fn del_name(&mut self, id: &str, loc: Loc) -> Res<()> {
        self.check_name(id)?;
        match self.name_kind(id) {
            NameKind::Fast(i) => self.add(DELETE_FAST, i as i64, loc),
            NameKind::Deref(i) => self.add(DELETE_DEREF, i as i64, loc),
            NameKind::Global if self.globals_decl.contains(id) => {
                let i = self.name_index(id);
                self.add(DELETE_GLOBAL, i as i64, loc);
            }
            NameKind::Global => return Err(Unsupported),
            NameKind::Name => {
                let i = self.name_index(id);
                self.add(DELETE_NAME, i as i64, loc);
            }
        }
        Ok(())
    }

    fn stmts(&mut self, body: &[Stmt]) -> Res<()> {
        for s in body {
            self.stmt(s)?;
        }
        Ok(())
    }

    fn stmt(&mut self, s: &Stmt) -> Res<()> {
        let loc = self.loc(&s.pos);
        if let Some(done) = self.generic_stmt(s, loc) {
            return done;
        }
        match &s.kind {
            S::Pass => self.add(NOP, 0, loc),
            S::Global { .. } | S::Nonlocal { .. } => {}
            S::Expr { value } => {
                if self.interactive {
                    // `compiler_visit_stmt_expr` com `c_interactive`: o valor passa por `INTRINSIC_PRINT`.
                    self.expr(value)?;
                    self.add(CALL_INTRINSIC_1, INTRINSIC_PRINT, loc);
                    self.add(POP_TOP, 0, NO_LOC);
                } else if fold(value)?.is_some() {
                    let l = self.loc(&value.pos);
                    self.add(NOP, 0, l);
                } else {
                    self.expr(value)?;
                    self.add(POP_TOP, 0, NO_LOC);
                }
            }
            S::Assign { targets, value, .. } => {
                self.expr(value)?;
                let n = targets.len();
                for (i, t) in targets.iter().enumerate() {
                    if i + 1 < n {
                        // O `COPY` leva a localização do comando inteiro (medido: `i = j = k = t` dá `(4, 17)`).
                        self.add(COPY, 1, loc);
                    }
                    self.store_target(t)?;
                }
            }
            S::AugAssign { target, op, value } => self.aug_assign(s, target, *op, value)?,
            S::Delete { targets } => {
                for t in targets {
                    self.delete_target(t)?;
                }
            }
            S::Assert { test, msg } => {
                let end = self.cfg.new_block();
                self.jump_if(test, end, true)?;
                self.add(LOAD_ASSERTION_ERROR, 0, loc);
                if let Some(m) = msg {
                    self.expr(m)?;
                    self.add(CALL, 0, loc);
                }
                // O `RAISE_VARARGS` leva a localização do teste, não a do comando.
                let test_loc = self.loc(&test.pos);
                self.add(RAISE_VARARGS, 1, test_loc);
                self.cfg.use_block(end);
            }
            S::Raise { exc, cause } => {
                let mut n = 0;
                if let Some(e) = exc {
                    self.expr(e)?;
                    n += 1;
                    if let Some(c) = cause {
                        self.expr(c)?;
                        n += 1;
                    }
                }
                self.add(RAISE_VARARGS, n, loc);
            }
            S::Try { body, handlers, orelse, finalbody } | S::TryStar { body, handlers, orelse, finalbody } => {
                let star = matches!(s.kind, S::TryStar { .. });
                if finalbody.is_empty() {
                    self.try_except(loc, star, body, handlers, orelse)?;
                } else {
                    self.try_finally(s, star, body, handlers, orelse, finalbody)?;
                }
            }
            S::With { items, body, .. } => self.with_stmt(items, 0, body, false)?,
            S::AsyncWith { items, body, .. } => self.with_stmt(items, 0, body, true)?,
            S::ClassDef { name, bases, keywords, decorator_list, type_params, .. } => {
                if !type_params.is_empty() {
                    return Err(Unsupported);
                }
                self.class_stmt(name, bases, keywords, decorator_list, loc)?;
            }
            S::Return { value } => self.return_stmt(s, value.as_deref())?,
            S::AnnAssign { target, annotation, value, simple } => {
                self.ann_assign(loc, target, annotation, value.as_deref(), *simple != 0)?
            }
            S::If { test, body, orelse } => {
                let end = self.cfg.new_block();
                let next = if orelse.is_empty() { end } else { self.cfg.new_block() };
                self.jump_if(test, next, false)?;
                self.stmts(body)?;
                if !orelse.is_empty() {
                    self.add_jump(JUMP, end, NO_LOC);
                    self.cfg.use_block(next);
                    self.stmts(orelse)?;
                }
                self.cfg.use_block(end);
            }
            S::For { target, iter, body, orelse, .. } => self.for_stmt(target, iter, body, orelse)?,
            S::AsyncFor { target, iter, body, orelse, .. } => self.async_for_stmt(target, iter, body, orelse, loc)?,
            S::Match { subject, cases } => self.match_stmt(subject, cases)?,
            S::While { test, body, orelse } => self.while_stmt(test, body, orelse)?,
            S::Break | S::Continue => {
                // `compiler_break` e `compiler_continue`: um NOP na linha do comando, o desfazer dos quadros abertos
                // dentro do laço (`try`, tratadores) e o salto. Só o `break` descarta o iterador do `for`, pois o
                // `continue` volta ao `FOR_ITER`.
                self.add(NOP, 0, loc);
                let mut at = loc;
                let lp = self.unwind(&mut at, false, true)?.ok_or(Unsupported)?;
                if matches!(s.kind, S::Break) {
                    if lp.is_for {
                        self.add(POP_TOP, 0, at);
                    }
                    self.add_jump(JUMP, lp.exit, at);
                } else {
                    self.add_jump(JUMP, lp.start, at);
                }
            }
            S::FunctionDef { name, args, decorator_list, returns, type_params, .. } => {
                if !type_params.is_empty() {
                    return Err(Unsupported);
                }
                self.check_name(name)?;
                for d in decorator_list {
                    self.expr(d)?;
                }
                self.make_function(args, returns.as_deref(), loc)?;
                for d in decorator_list.iter().rev() {
                    let l = self.loc(&d.pos);
                    self.add(CALL, 0, l);
                }
                self.store_name(name, loc)?;
            }
            S::Import { names } => {
                for a in names {
                    self.load_cv(Cv::int(0), loc);
                    self.load_cv(Cv::none(), loc);
                    let n = self.name_index(&a.name);
                    self.add(IMPORT_NAME, n as i64, loc);
                    match &a.asname {
                        Some(asname) => self.import_as(&a.name, asname, loc)?,
                        None => self.store_name(a.name.split('.').next().unwrap_or(""), loc)?,
                    }
                }
            }
            S::ImportFrom { module, names, level } => {
                // `from __future__ import x` sai como qualquer outro: o que ele muda é a compilação das anotações.
                self.load_cv(Cv::int(level.unwrap_or(0)), loc);
                self.load_cv(Cv::tuple(names.iter().map(|a| Cv::str(a.name.clone())).collect()), loc);
                let m = self.name_index(module.as_deref().unwrap_or(""));
                self.add(IMPORT_NAME, m as i64, loc);
                if names.first().is_some_and(|a| a.name == "*") {
                    self.add(CALL_INTRINSIC_1, INTRINSIC_IMPORT_STAR, loc);
                    self.add(POP_TOP, 0, NO_LOC);
                    return Ok(());
                }
                for a in names {
                    let n = self.name_index(&a.name);
                    self.add(IMPORT_FROM, n as i64, loc);
                    self.store_name(a.asname.as_deref().unwrap_or(&a.name), loc)?;
                }
                self.add(POP_TOP, 0, loc);
            }
            _ => return Err(Unsupported),
        }
        Ok(())
    }

    /// `compiler_import_as`: `import a.b.c as d` busca cada componente depois do primeiro com `IMPORT_FROM` (o módulo
    /// anterior sai com `SWAP 2` e `POP_TOP`), liga o último a `d` e descarta o módulo de cima que sobrou; sem ponto é só
    /// o `STORE_*`.
    fn import_as(&mut self, name: &str, asname: &str, loc: Loc) -> Res<()> {
        let mut attrs = name.split('.').skip(1).peekable();
        if attrs.peek().is_none() {
            return self.store_name(asname, loc);
        }
        while let Some(attr) = attrs.next() {
            let i = self.name_index(attr);
            self.add(IMPORT_FROM, i as i64, loc);
            if attrs.peek().is_some() {
                self.add(SWAP, 2, loc);
                self.add(POP_TOP, 0, loc);
            }
        }
        self.store_name(asname, loc)?;
        self.add(POP_TOP, 0, loc);
        Ok(())
    }

    /// `compiler_for`: o iterador, `FOR_ITER` com o `NOP` do alvo, o corpo, o salto para trás sem localização e o
    /// `END_FOR` com o `POP_TOP` do iterador esgotado.
    fn for_stmt(&mut self, target: &Expr, iter: &Expr, body: &[Stmt], orelse: &[Stmt]) -> Res<()> {
        let start = self.cfg.new_block();
        let cleanup = self.cfg.new_block();
        let end = self.cfg.new_block();
        self.iter_value(iter)?;
        let iter_loc = self.loc(&iter.pos);
        self.add(GET_ITER, 0, iter_loc);
        self.cfg.use_block(start);
        self.add_jump(FOR_ITER, cleanup, iter_loc);
        let target_loc = self.loc(&target.pos);
        self.add(NOP, 0, target_loc);
        self.store_target(target)?;
        self.fblocks.push(Fb::Loop(Loop { start, exit: end, is_for: true }));
        self.stmts(body)?;
        self.fblocks.pop();
        self.add_jump(JUMP, start, NO_LOC);
        self.cfg.use_block(cleanup);
        self.add(END_FOR, 0, NO_LOC);
        self.add(POP_TOP, 0, NO_LOC);
        self.stmts(orelse)?;
        self.cfg.use_block(end);
        Ok(())
    }

    /// `compiler_while`: o teste duas vezes (inversão de laço): no topo, saltando para `anchor` se falso, e no fim
    /// do corpo, saltando para o corpo se verdadeiro. Teste constante fica de fora.
    fn while_stmt(&mut self, test: &Expr, body: &[Stmt], orelse: &[Stmt]) -> Res<()> {
        let top = self.cfg.new_block();
        let body_block = self.cfg.new_block();
        let anchor = self.cfg.new_block();
        let end = self.cfg.new_block();
        self.cfg.use_block(top);
        self.fblocks.push(Fb::Loop(Loop { start: top, exit: end, is_for: false }));
        self.jump_if(test, anchor, false)?;
        self.cfg.use_block(body_block);
        self.stmts(body)?;
        self.jump_if(test, body_block, true)?;
        self.fblocks.pop();
        self.cfg.use_block(anchor);
        self.stmts(orelse)?;
        self.cfg.use_block(end);
        Ok(())
    }

    /// `annexpr`: o valor de uma anotação, que com `from __future__ import annotations` é o texto dela.
    fn annotation_value(&mut self, ann: &Expr) -> Res<()> {
        if self.future {
            let loc = self.loc(&ann.pos);
            self.load_cv(Cv::str(crate::compile::ann_text(ann)), loc);
            return Ok(());
        }
        self.expr(ann)
    }

    /// `check_ann_expr`: avalia `e` só para que o erro apareça, e descarta o valor.
    fn eval_and_drop(&mut self, e: &Expr) -> Res<()> {
        self.expr(e)?;
        let loc = self.loc(&e.pos);
        self.add(POP_TOP, 0, loc);
        Ok(())
    }

    /// `check_ann_subscr`: o que um subscrito sem valor avalia: cada limite de fatia e cada elemento de tupla.
    fn eval_subscript_and_drop(&mut self, slice: &Expr) -> Res<()> {
        match &slice.kind {
            E::Slice { lower, upper, step } => {
                for bound in [lower, upper, step].into_iter().flatten() {
                    self.eval_and_drop(bound)?;
                }
                Ok(())
            }
            E::Tuple { elts, .. } => elts.iter().try_for_each(|x| self.eval_subscript_and_drop(x)),
            _ => self.eval_and_drop(slice),
        }
    }

    /// `compiler_annassign`: o valor é gravado primeiro. Nome simples num módulo ou numa classe guarda a anotação em
    /// `__annotations__`; atributo e subscrito sem valor avaliam o objeto (e o subscrito); a anotação de um alvo
    /// complexo é avaliada por último, só em módulo e classe, e some com `from __future__ import annotations`.
    fn ann_assign(&mut self, loc: Loc, target: &Expr, ann: &Expr, value: Option<&Expr>, simple: bool) -> Res<()> {
        if let Some(v) = value {
            self.expr(v)?;
            self.store_target(target)?;
        }
        match &target.kind {
            E::Name { id, .. } if simple && !self.function => {
                self.check_name(id)?;
                self.annotation_value(ann)?;
                let i = self.name_index("__annotations__");
                self.add(LOAD_NAME, i as i64, loc);
                self.load_cv(Cv::str(id.clone()), loc);
                self.add(STORE_SUBSCR, 0, loc);
            }
            E::Name { .. } => {}
            E::Attribute { value: obj, .. } if value.is_none() => self.eval_and_drop(obj)?,
            E::Subscript { value: obj, slice, .. } if value.is_none() => {
                self.eval_and_drop(obj)?;
                self.eval_subscript_and_drop(slice)?;
            }
            E::Attribute { .. } | E::Subscript { .. } => {}
            _ => return Err(Unsupported),
        }
        if !simple && !self.future && !self.function {
            self.eval_and_drop(ann)?;
        }
        Ok(())
    }

    /// `fold_iter`: `for x in [1, 2]` e `x in [1, 2]` percorrem uma tupla constante.
    fn iter_value(&mut self, e: &Expr) -> Res<()> {
        if let E::List { elts, ctx: ExprContext::Load } = &e.kind {
            if let Some(items) = fold_all(elts)? {
                let loc = self.loc(&e.pos);
                self.load_cv(Cv::tuple(items), loc);
                return Ok(());
            }
        }
        // `fold_iter` troca o conjunto de constantes por um `frozenset` constante.
        if let E::Set { elts } = &e.kind {
            if let Some(items) = fold_all(elts)? {
                let loc = self.loc(&e.pos);
                self.load_cv(Cv::frozenset(items)?, loc);
                return Ok(());
            }
        }
        self.expr(e)
    }

    /// `compiler_default_arguments` e `compiler_make_closure`: deixa a função na pilha, com os padrões, os
    /// padrões só-nomeados e a tupla de células da closure aplicados por `SET_FUNCTION_ATTRIBUTE`.
    fn make_function(&mut self, args: &Arguments, returns: Option<&Expr>, loc: Loc) -> Res<()> {
        let mut flags = 0;
        if !args.defaults.is_empty() {
            for d in &args.defaults {
                self.expr(d)?;
            }
            self.add(BUILD_TUPLE, args.defaults.len() as i64, loc);
            flags |= FUNC_DEFAULTS;
        }
        let mut kw_names = Vec::new();
        for (a, d) in args.kwonlyargs.iter().zip(&args.kw_defaults) {
            if let Some(d) = d {
                self.check_name(&a.arg)?;
                self.expr(d)?;
                kw_names.push(a.arg.clone());
            }
        }
        if !kw_names.is_empty() {
            self.load_cv(Cv::tuple(kw_names.iter().map(|n| Cv::str(n.clone())).collect()), loc);
            self.add(BUILD_CONST_KEY_MAP, kw_names.len() as i64, loc);
            flags |= FUNC_KWDEFAULTS;
        }
        // `compiler_visit_annotations`: nome e valor de cada anotação na ordem posicionais, `*args`, só-nomeados,
        // `**kwargs` e `return`, numa tupla; o nome é uma constante na localização do `def`.
        let mut annotations = 0;
        let annotated = args
            .posonlyargs
            .iter()
            .chain(&args.args)
            .chain(args.vararg.as_deref())
            .chain(&args.kwonlyargs)
            .chain(args.kwarg.as_deref())
            .filter_map(|a| a.annotation.as_deref().map(|ann| (a.arg.as_str(), ann)))
            .chain(returns.map(|r| ("return", r)));
        for (name, ann) in annotated {
            self.check_name(name)?;
            self.load_cv(Cv::str(name.to_string()), loc);
            self.annotation_value(ann)?;
            annotations += 2;
        }
        if annotations > 0 {
            self.add(BUILD_TUPLE, annotations, loc);
            flags |= FUNC_ANNOTATIONS;
        }
        let k = self.next_fn;
        let inner = self.code.functions.get(k).ok_or(Unsupported)?.clone();
        self.next_fn += 1;
        self.closure_code(k, &inner, loc, flags)
    }

    /// `compiler_make_closure`: a tupla de células (se a função fecha variáveis), o objeto `code` (o `k`-ésimo de
    /// `Code::functions`), `MAKE_FUNCTION` e um `SET_FUNCTION_ATTRIBUTE` por atributo em `flags`.
    fn closure_code(&mut self, k: usize, inner: &Code, loc: Loc, mut flags: i64) -> Res<()> {
        let freevars = layout(inner).freevars;
        if !freevars.is_empty() {
            for n in &freevars {
                let i = self.localsplus.iter().position(|v| v == n).ok_or(Unsupported)?;
                self.add(LOAD_CLOSURE, i as i64, loc);
            }
            self.add(BUILD_TUPLE, freevars.len() as i64, loc);
            flags |= FUNC_CLOSURE;
        }
        let code_const = self.add_const(Key::Code(k), Value::Builtin(CODE_CONST));
        self.add(LOAD_CONST, code_const as i64, loc);
        self.add(MAKE_FUNCTION, 0, loc);
        for bit in [FUNC_CLOSURE, FUNC_ANNOTATIONS, FUNC_KWDEFAULTS, FUNC_DEFAULTS] {
            if flags & bit != 0 {
                self.add(SET_FUNCTION_ATTRIBUTE, bit, loc);
            }
        }
        Ok(())
    }

    fn return_stmt(&mut self, s: &Stmt, value: Option<&Expr>) -> Res<()> {
        if !self.function {
            return Err(Unsupported);
        }
        let stmt_loc = self.loc(&s.pos);
        let mut loc = stmt_loc;
        let folded = match value {
            Some(v) => fold(v)?,
            None => None,
        };
        let preserve_tos = value.is_some() && folded.is_none();
        if preserve_tos {
            if let Some(v) = value {
                self.expr(v)?;
            }
        } else if let Some(v) = value {
            loc = self.loc(&v.pos);
            self.add(NOP, 0, loc);
        }
        if value.is_none_or(|v| v.pos.lineno != s.pos.lineno) {
            loc = stmt_loc;
            self.add(NOP, 0, loc);
        }
        // `compiler_unwind_fblock_stack`: o `return` dentro de `for`, `try` e tratadores desfaz cada quadro (guardando o
        // valor).
        self.unwind(&mut loc, preserve_tos, false)?;
        match (value, folded) {
            (None, _) => self.load_cv(Cv::none(), loc),
            (Some(_), Some(cv)) => self.load_cv(cv, loc),
            (Some(_), None) => {}
        }
        self.add(RETURN_VALUE, 0, loc);
        Ok(())
    }

    /// `compiler_unwind_fblock_stack`: desfaz os quadros abertos, do mais interno ao mais externo. Com `to_loop`, para
    /// no primeiro laço (que fica de pé) e o devolve. `loc` é a localização corrente (`*ploc`): sair de um `with` a
    /// troca pela do comando `with`. `Err` em `finally`, que o emissor não cobre.
    fn unwind(&mut self, loc: &mut Loc, preserve_tos: bool, to_loop: bool) -> Res<Option<Loop>> {
        for k in (0..self.fblocks.len()).rev() {
            let cur = *loc;
            match self.fblocks[k].clone() {
                Fb::Loop(l) if to_loop => return Ok(Some(l)),
                Fb::Loop(l) => {
                    if l.is_for {
                        if preserve_tos {
                            self.add(SWAP, 2, cur);
                        }
                        self.add(POP_TOP, 0, cur);
                    }
                }
                Fb::TryExcept => self.add(POP_BLOCK, 0, cur),
                Fb::ExcHandler => {}
                Fb::HandlerCleanup(name) => {
                    if name.is_some() {
                        self.add(POP_BLOCK, 0, cur);
                    }
                    if preserve_tos {
                        self.add(SWAP, 2, cur);
                    }
                    self.add(POP_BLOCK, 0, cur);
                    self.add(POP_EXCEPT, 0, cur);
                    if let Some(n) = name {
                        self.clear_handler_name(&n, cur)?;
                    }
                }
                Fb::With(with_loc) | Fb::AsyncWith(with_loc) => {
                    *loc = with_loc;
                    self.add(POP_BLOCK, 0, with_loc);
                    if preserve_tos {
                        self.add(SWAP, 2, with_loc);
                    }
                    self.call_exit_with_nones(with_loc);
                    if matches!(self.fblocks[k], Fb::AsyncWith(_)) {
                        self.await_top(with_loc, 2);
                    }
                    self.add(POP_TOP, 0, with_loc);
                }
                Fb::FinallyTry(body) => {
                    // O `finally` roda com este quadro e os de dentro já retirados; com o valor do `return` guardado, um
                    // `POP_VALUE` ocupa o lugar deles.
                    self.add(POP_BLOCK, 0, cur);
                    let saved: Vec<Fb> = self.fblocks.drain(k..).collect();
                    if preserve_tos {
                        self.fblocks.push(Fb::PopValue);
                    }
                    let ran = self.stmts(&body);
                    if preserve_tos {
                        self.fblocks.pop();
                    }
                    self.fblocks.extend(saved);
                    ran?;
                    // O `finally` parece rodar depois do comando que desfaz: o desfazer fica sem localização.
                    *loc = NO_LOC;
                }
                Fb::FinallyEnd => {
                    if preserve_tos {
                        self.add(SWAP, 2, cur);
                    }
                    self.add(POP_TOP, 0, cur);
                    if preserve_tos {
                        self.add(SWAP, 2, cur);
                    }
                    self.add(POP_BLOCK, 0, cur);
                    self.add(POP_EXCEPT, 0, cur);
                }
                Fb::PopValue => {
                    if preserve_tos {
                        self.add(SWAP, 2, cur);
                    }
                    self.add(POP_TOP, 0, cur);
                }
            }
        }
        Ok(None)
    }

    /// `compiler_call_exit_with_nones`: `__exit__(None, None, None)`, com o método e o `self` já na pilha.
    fn call_exit_with_nones(&mut self, loc: Loc) {
        for _ in 0..3 {
            self.load_cv(Cv::none(), loc);
        }
        self.add(CALL, 2, loc);
    }

    /// `POP_EXCEPT_AND_RERAISE`: o fim dos blocos de limpeza.
    fn pop_except_and_reraise(&mut self, loc: Loc) {
        self.add(COPY, 3, loc);
        self.add(POP_EXCEPT, 0, loc);
        self.add(RERAISE, 1, loc);
    }

    /// `compiler_try_except`: `SETUP_FINALLY` em volta do corpo; o tratador começa com `PUSH_EXC_INFO` e cada cláusula
    /// testa `CHECK_EXC_MATCH`. O que ninguém casa recai no `RERAISE 0`; o `SETUP_CLEANUP` externo restaura o estado de
    /// exceção se o próprio tratador falha.
    fn try_except(&mut self, loc: Loc, star: bool, body: &[Stmt], handlers: &[ExceptHandler], orelse: &[Stmt]) -> Res<()> {
        if star {
            return self.try_star_except(loc, body, handlers, orelse);
        }
        let except = self.cfg.new_block();
        let end = self.cfg.new_block();
        let cleanup = self.cfg.new_block();
        // O `SETUP_FINALLY` leva a localização do `try`: ao virar `NOP` ele aparece na linha do `try`.
        self.add_jump(SETUP_FINALLY, except, loc);
        let body_block = self.cfg.new_block();
        self.cfg.use_block(body_block);
        self.fblocks.push(Fb::TryExcept);
        self.stmts(body)?;
        self.fblocks.pop();
        self.add(POP_BLOCK, 0, NO_LOC);
        self.stmts(orelse)?;
        self.add_jump(JUMP, end, NO_LOC);
        self.cfg.use_block(except);
        self.add_jump(SETUP_CLEANUP, cleanup, NO_LOC);
        self.add(PUSH_EXC_INFO, 0, NO_LOC);
        self.fblocks.push(Fb::ExcHandler);
        let n = handlers.len();
        for (i, h) in handlers.iter().enumerate() {
            let hloc = self.loc(&h.pos);
            if h.r#type.is_none() && i + 1 < n {
                return Err(Unsupported);
            }
            let next_except = self.cfg.new_block();
            if let Some(t) = &h.r#type {
                self.expr(t)?;
                self.add(CHECK_EXC_MATCH, 0, hloc);
                self.add_jump(POP_JUMP_IF_FALSE, next_except, hloc);
            }
            let cleanup_body = self.cfg.new_block();
            match &h.name {
                Some(name) => {
                    let cleanup_end = self.cfg.new_block();
                    self.store_name(name, hloc)?;
                    self.add_jump(SETUP_CLEANUP, cleanup_end, hloc);
                    self.cfg.use_block(cleanup_body);
                    self.fblocks.push(Fb::HandlerCleanup(Some(name.clone())));
                    self.stmts(&h.body)?;
                    self.fblocks.pop();
                    self.add(POP_BLOCK, 0, NO_LOC);
                    self.add(POP_BLOCK, 0, NO_LOC);
                    self.add(POP_EXCEPT, 0, NO_LOC);
                    self.clear_handler_name(name, NO_LOC)?;
                    self.add_jump(JUMP, end, NO_LOC);
                    self.cfg.use_block(cleanup_end);
                    self.clear_handler_name(name, NO_LOC)?;
                    self.add(RERAISE, 1, NO_LOC);
                }
                None => {
                    self.add(POP_TOP, 0, hloc);
                    self.cfg.use_block(cleanup_body);
                    self.fblocks.push(Fb::HandlerCleanup(None));
                    self.stmts(&h.body)?;
                    self.fblocks.pop();
                    self.add(POP_BLOCK, 0, NO_LOC);
                    self.add(POP_EXCEPT, 0, NO_LOC);
                    self.add_jump(JUMP, end, NO_LOC);
                }
            }
            self.cfg.use_block(next_except);
        }
        self.fblocks.pop();
        self.add(RERAISE, 0, NO_LOC);
        self.cfg.use_block(cleanup);
        self.pop_except_and_reraise(NO_LOC);
        self.cfg.use_block(end);
        Ok(())
    }

    /// `name = None; del name`: o fim artificial de `except E as name`, que desfaz a ligação do nome.
    fn clear_handler_name(&mut self, name: &str, loc: Loc) -> Res<()> {
        self.load_cv(Cv::none(), loc);
        self.store_name(name, loc)?;
        self.del_name(name, loc)
    }

    /// `compiler_try_star_except`: como `try_except`, mas cada `except*` recebe o grupo que sobrou do anterior
    /// (`CHECK_EG_MATCH`). Pilha nos tratadores: `[anterior, original, lista, resto]`; a lista junta o que os tratadores
    /// levantam e o resto que ninguém casou, e `INTRINSIC_PREP_RERAISE_STAR` monta o grupo a relançar (ou `None`).
    fn try_star_except(&mut self, loc: Loc, body: &[Stmt], handlers: &[ExceptHandler], orelse: &[Stmt]) -> Res<()> {
        if handlers.is_empty() {
            return Err(Unsupported);
        }
        let except = self.cfg.new_block();
        let orelse_block = self.cfg.new_block();
        let end = self.cfg.new_block();
        let cleanup = self.cfg.new_block();
        let reraise_star = self.cfg.new_block();
        self.add_jump(SETUP_FINALLY, except, loc);
        let body_block = self.cfg.new_block();
        self.cfg.use_block(body_block);
        self.fblocks.push(Fb::TryExcept);
        self.stmts(body)?;
        self.fblocks.pop();
        self.add(POP_BLOCK, 0, NO_LOC);
        self.add_jump(JUMP, orelse_block, NO_LOC);
        self.cfg.use_block(except);
        self.add_jump(SETUP_CLEANUP, cleanup, NO_LOC);
        self.add(PUSH_EXC_INFO, 0, NO_LOC);
        self.fblocks.push(Fb::ExcHandler);
        let n = handlers.len();
        for (i, h) in handlers.iter().enumerate() {
            let hloc = self.loc(&h.pos);
            let last = i + 1 == n;
            let ty = h.r#type.as_ref().ok_or(Unsupported)?;
            let no_match = self.cfg.new_block();
            // Depois deste tratador: o seguinte, ou o relançamento. Os três caminhos (normal, com erro e sem
            // casamento) levam o resto até lá; o último ainda o acrescenta à lista.
            let next = if last { reraise_star } else { self.cfg.new_block() };
            if i == 0 {
                self.add(BUILD_LIST, 0, hloc);
                self.add(COPY, 2, hloc);
            }
            self.expr(ty)?;
            self.add(CHECK_EG_MATCH, 0, hloc);
            self.add(COPY, 1, hloc);
            self.add_jump(POP_JUMP_IF_NONE, no_match, hloc);
            match &h.name {
                Some(name) => self.store_name(name, hloc)?,
                None => self.add(POP_TOP, 0, hloc),
            }
            let cleanup_end = self.cfg.new_block();
            self.add_jump(SETUP_CLEANUP, cleanup_end, hloc);
            let cleanup_body = self.cfg.new_block();
            self.cfg.use_block(cleanup_body);
            self.fblocks.push(Fb::HandlerCleanup(h.name.clone()));
            self.stmts(&h.body)?;
            self.fblocks.pop();
            self.add(POP_BLOCK, 0, NO_LOC);
            if let Some(name) = &h.name {
                self.clear_handler_name(name, NO_LOC)?;
            }
            if last {
                self.add(LIST_APPEND, 1, NO_LOC);
            }
            self.add_jump(JUMP, next, NO_LOC);
            self.cfg.use_block(cleanup_end);
            if let Some(name) = &h.name {
                self.clear_handler_name(name, NO_LOC)?;
            }
            self.add(LIST_APPEND, 3, NO_LOC);
            self.add(POP_TOP, 0, NO_LOC);
            if last {
                self.add(LIST_APPEND, 1, NO_LOC);
            }
            self.add_jump(JUMP, next, NO_LOC);
            self.cfg.use_block(no_match);
            self.add(POP_TOP, 0, hloc);
            if last {
                self.add(LIST_APPEND, 1, NO_LOC);
            }
            self.cfg.use_block(next);
        }
        self.fblocks.pop();
        let reraise = self.cfg.new_block();
        self.add(CALL_INTRINSIC_2, INTRINSIC_PREP_RERAISE_STAR, NO_LOC);
        self.add(COPY, 1, NO_LOC);
        self.add_jump(POP_JUMP_IF_NOT_NONE, reraise, NO_LOC);
        self.add(POP_TOP, 0, NO_LOC);
        self.add(POP_BLOCK, 0, NO_LOC);
        self.add(POP_EXCEPT, 0, NO_LOC);
        self.add_jump(JUMP, end, NO_LOC);
        self.cfg.use_block(reraise);
        self.add(POP_BLOCK, 0, NO_LOC);
        self.add(SWAP, 2, NO_LOC);
        self.add(POP_EXCEPT, 0, NO_LOC);
        self.add(RERAISE, 0, NO_LOC);
        self.cfg.use_block(cleanup);
        self.pop_except_and_reraise(NO_LOC);
        self.cfg.use_block(orelse_block);
        self.stmts(orelse)?;
        self.cfg.use_block(end);
        Ok(())
    }

    /// `compiler_try_finally`: o corpo (com os `except`, se há) sob `SETUP_FINALLY`; o `finally` é emitido duas vezes,
    /// uma no caminho normal e outra no tratador, que termina em `RERAISE 0`.
    fn try_finally(&mut self, s: &Stmt, star: bool, body: &[Stmt], handlers: &[ExceptHandler], orelse: &[Stmt], finalbody: &[Stmt]) -> Res<()> {
        let loc = self.loc(&s.pos);
        let end = self.cfg.new_block();
        let exit = self.cfg.new_block();
        let cleanup = self.cfg.new_block();
        self.add_jump(SETUP_FINALLY, end, loc);
        let body_block = self.cfg.new_block();
        self.cfg.use_block(body_block);
        self.fblocks.push(Fb::FinallyTry(Rc::new(finalbody.to_vec())));
        if handlers.is_empty() {
            self.stmts(body)?;
        } else {
            self.try_except(loc, star, body, handlers, orelse)?;
        }
        self.add(POP_BLOCK, 0, NO_LOC);
        self.fblocks.pop();
        self.stmts(finalbody)?;
        self.add_jump(JUMP, exit, NO_LOC);
        self.cfg.use_block(end);
        self.add_jump(SETUP_CLEANUP, cleanup, NO_LOC);
        self.add(PUSH_EXC_INFO, 0, NO_LOC);
        self.fblocks.push(Fb::FinallyEnd);
        self.stmts(finalbody)?;
        self.fblocks.pop();
        let last = finalbody.last().map_or(NO_LOC, |l| self.loc(&l.pos));
        self.add(RERAISE, 0, last);
        self.cfg.use_block(cleanup);
        self.pop_except_and_reraise(NO_LOC);
        self.cfg.use_block(exit);
        Ok(())
    }

    /// `compiler_with` e `compiler_async_with`: `BEFORE_WITH` deixa o `__exit__` sob o resultado do `__enter__`; o
    /// tratador (`PUSH_EXC_INFO`, `WITH_EXCEPT_START`) o chama com a exceção e decide se a suprime. No `async with`
    /// o `__aenter__` e o `__aexit__` são esperados (`GET_AWAITABLE` e o laço `SEND`), e como no `with` síncrono todas
    /// as instruções levam a localização da expressão do item (`LOC(item->context_expr)`).
    fn with_stmt(&mut self, items: &[WithItem], pos: usize, body: &[Stmt], is_async: bool) -> Res<()> {
        if is_async && !(self.function && self.code.is_async) {
            return Err(Unsupported);
        }
        let item = items.get(pos).ok_or(Unsupported)?;
        let final_block = self.cfg.new_block();
        let exit = self.cfg.new_block();
        let cleanup = self.cfg.new_block();
        self.expr(&item.context_expr)?;
        let loc = self.loc(&item.context_expr.pos);
        if is_async {
            self.add(BEFORE_ASYNC_WITH, 0, loc);
            self.await_top(loc, 1);
        } else {
            self.add(BEFORE_WITH, 0, loc);
        }
        self.add_jump(SETUP_WITH, final_block, loc);
        let block = self.cfg.new_block();
        self.cfg.use_block(block);
        // O quadro guarda a localização do item (`compiler_push_fblock(c, loc, WITH, ...)`): `return`, `break` e
        // `continue` dentro do corpo chamam o `__exit__` na posição de `cm`, não na do comando inteiro.
        self.fblocks.push(if is_async { Fb::AsyncWith(loc) } else { Fb::With(loc) });
        match &item.optional_vars {
            Some(v) => self.store_target(v)?,
            None => self.add(POP_TOP, 0, loc),
        }
        if pos + 1 == items.len() {
            self.stmts(body)?;
        } else {
            self.with_stmt(items, pos + 1, body, is_async)?;
        }
        self.fblocks.pop();
        // `compiler_with` emite o `POP_BLOCK` sem localização: ele herda a da instrução anterior (`resolve_line_numbers`) e
        // o `NOP` em que ele vira só some se a anterior do bloco ou a próxima tem a mesma linha; fica fora da faixa do
        // tratador. `compiler_async_with` o emite com `loc` (a do item), então o `NOP` some junto do `LOAD_CONST`
        // da chamada de `__aexit__` e sobra o `NOP` do `pass` do corpo.
        self.add(POP_BLOCK, 0, if is_async { loc } else { NO_LOC });
        self.call_exit_with_nones(loc);
        if is_async {
            self.await_top(loc, 2);
        }
        self.add(POP_TOP, 0, loc);
        self.add_jump(JUMP, exit, loc);
        self.cfg.use_block(final_block);
        self.add_jump(SETUP_CLEANUP, cleanup, loc);
        self.add(PUSH_EXC_INFO, 0, loc);
        self.add(WITH_EXCEPT_START, 0, loc);
        if is_async {
            self.await_top(loc, 2);
        }
        let suppress = self.cfg.new_block();
        self.add(TO_BOOL, 0, NO_LOC);
        self.add_jump(POP_JUMP_IF_TRUE, suppress, NO_LOC);
        self.add(RERAISE, 2, NO_LOC);
        self.cfg.use_block(suppress);
        self.add(POP_TOP, 0, NO_LOC);
        self.add(POP_BLOCK, 0, NO_LOC);
        self.add(POP_EXCEPT, 0, NO_LOC);
        self.add(POP_TOP, 0, NO_LOC);
        self.add(POP_TOP, 0, NO_LOC);
        self.add_jump(JUMP, exit, NO_LOC);
        self.cfg.use_block(cleanup);
        self.pop_except_and_reraise(NO_LOC);
        self.cfg.use_block(exit);
        Ok(())
    }

    /// `compiler_class`: `LOAD_BUILD_CLASS`, a função do corpo (com a closure, se há), o nome e as bases; o objeto
    /// `code` do corpo é o `k`-ésimo de `Code::functions`, e as bases não podem ter funções (o interpretador as cria
    /// antes do corpo, o CPython depois).
    fn class_stmt(&mut self, name: &str, bases: &[Expr], keywords: &[Keyword], decorators: &[Expr], loc: Loc) -> Res<()> {
        self.check_name(name)?;
        if bases.iter().any(is_starred) || keywords.iter().any(|k| k.arg.is_none()) {
            return Err(Unsupported);
        }
        if bases.len() + keywords.len() * 2 > STACK_USE_GUIDELINE {
            return Err(Unsupported);
        }
        for d in decorators {
            self.expr(d)?;
        }
        let k = self.next_fn;
        let inner = self.code.functions.get(k).ok_or(Unsupported)?.clone();
        // O corpo que fecha variáveis da função de fora lista as livres em `freevars` (`compile.rs`); se ele ficou sem
        // listá-las (nome que o corpo também liga, `global`, `nonlocal`) a tupla de células sairia errada.
        if self.function && !(self.code.cellvars.is_empty() && self.code.freevars.is_empty()) && inner.cpy.is_none() && inner.freevars.is_empty() {
            return Err(Unsupported);
        }
        self.next_fn += 1;
        self.add(LOAD_BUILD_CLASS, 0, loc);
        self.add(PUSH_NULL, 0, loc);
        self.closure_code(k, &inner, loc, 0)?;
        // O nome ligado pode vir mutilado (`class __Inner` em `A` liga `_A__Inner`); a constante leva o original, que o
        // `Code` do corpo guarda.
        self.load_cv(Cv::str(inner.name.clone()), loc);
        for b in bases {
            self.expr(b)?;
        }
        if keywords.is_empty() {
            self.add(CALL, 2 + bases.len() as i64, loc);
        } else {
            let mut names = Vec::new();
            for kw in keywords {
                let n = kw.arg.clone().ok_or(Unsupported)?;
                self.check_name(&n)?;
                self.expr(&kw.value)?;
                names.push(Cv::str(n));
            }
            self.load_cv(Cv::tuple(names), loc);
            self.add(CALL_KW, (2 + bases.len() + keywords.len()) as i64, loc);
        }
        if self.next_fn != k + 1 {
            return Err(Unsupported);
        }
        for d in decorators.iter().rev() {
            let l = self.loc(&d.pos);
            self.add(CALL, 0, l);
        }
        self.store_name(name, loc)
    }

    /// `compiler_augassign`: o alvo é lido, combinado com o valor e gravado de volta; atributo e subscrito mantêm o
    /// objeto na pilha com `COPY` e o reordenam com `SWAP` antes de gravar.
    fn aug_assign(&mut self, s: &Stmt, target: &Expr, op: Operator, value: &Expr) -> Res<()> {
        let loc = self.loc(&s.pos);
        let tloc = self.loc(&target.pos);
        match &target.kind {
            E::Name { id, .. } => {
                self.load_name(id, tloc, false)?;
            }
            E::Attribute { value: obj, attr, .. } => {
                self.expr(obj)?;
                self.add(COPY, 1, tloc);
                let i = self.name_index(attr);
                self.add(LOAD_ATTR, (i as i64) << 1, match_attr(tloc, tloc, attr));
            }
            E::Subscript { value: obj, slice, .. } => {
                self.expr(obj)?;
                if is_two_element_slice(slice) {
                    self.slice_bounds(slice)?;
                    for _ in 0..3 {
                        self.add(COPY, 3, tloc);
                    }
                    self.add(BINARY_SLICE, 0, tloc);
                } else {
                    self.expr(slice)?;
                    self.add(COPY, 2, tloc);
                    self.add(COPY, 2, tloc);
                    self.add(BINARY_SUBSCR, 0, tloc);
                }
            }
            _ => return Err(Unsupported),
        }
        self.expr(value)?;
        self.add(BINARY_OP, nb_op(op) + 13, loc);
        // A gravação volta à localização do alvo: `loc = LOC(e)` depois do `ADDOP_INPLACE`.
        match &target.kind {
            E::Name { id, .. } => self.store_name(id, tloc)?,
            E::Attribute { attr, .. } => {
                let l = match_attr(tloc, tloc, attr);
                self.add(SWAP, 2, l);
                let i = self.name_index(attr);
                self.add(STORE_ATTR, i as i64, l);
            }
            E::Subscript { slice, .. } if is_two_element_slice(slice) => {
                self.add(SWAP, 4, tloc);
                self.add(SWAP, 3, tloc);
                self.add(SWAP, 2, tloc);
                self.add(STORE_SLICE, 0, tloc);
            }
            _ => {
                self.add(SWAP, 3, tloc);
                self.add(SWAP, 2, tloc);
                self.add(STORE_SUBSCR, 0, tloc);
            }
        }
        Ok(())
    }

    /// `del` de um alvo (`compiler_visit_expr` com contexto `Del`).
    fn delete_target(&mut self, t: &Expr) -> Res<()> {
        let loc = self.loc(&t.pos);
        match &t.kind {
            E::Name { id, .. } => self.del_name(id, loc)?,
            E::Attribute { value, attr, .. } => {
                self.expr(value)?;
                let i = self.name_index(attr);
                self.add(DELETE_ATTR, i as i64, match_attr(loc, loc, attr));
            }
            E::Subscript { value, slice, .. } => {
                self.expr(value)?;
                self.expr(slice)?;
                self.add(DELETE_SUBSCR, 0, loc);
            }
            E::Tuple { elts, .. } | E::List { elts, .. } => {
                for x in elts {
                    self.delete_target(x)?;
                }
            }
            _ => return Err(Unsupported),
        }
        Ok(())
    }

    /// `RETURN_VALUE` do fim do corpo, sem localização (herda a da instrução anterior).
    fn implicit_return(&mut self) {
        self.load_cv(Cv::none(), NO_LOC);
        self.add(RETURN_VALUE, 0, NO_LOC);
    }

    fn store_target(&mut self, t: &Expr) -> Res<()> {
        let loc = self.loc(&t.pos);
        match &t.kind {
            E::Name { id, .. } => self.store_name(id, loc)?,
            E::Attribute { value, attr, .. } => {
                self.expr(value)?;
                let l = match_attr(loc, loc, attr);
                let i = self.name_index(attr);
                self.add(STORE_ATTR, i as i64, l);
            }
            E::Subscript { value, slice, .. } => {
                self.expr(value)?;
                if is_two_element_slice(slice) {
                    self.slice_bounds(slice)?;
                    self.add(STORE_SLICE, 0, loc);
                } else {
                    self.expr(slice)?;
                    self.add(STORE_SUBSCR, 0, loc);
                }
            }
            E::Tuple { elts, .. } | E::List { elts, .. } => {
                // `unpack_helper`: `UNPACK_EX` com a posição do `*x` e a contagem do que vem depois, ou
                // `UNPACK_SEQUENCE`; depois um store por elemento.
                let n = elts.len();
                let mut star = None;
                for (i, x) in elts.iter().enumerate() {
                    if is_starred(x) {
                        if star.is_some() {
                            return Err(Unsupported);
                        }
                        star = Some(i);
                    }
                }
                match star {
                    Some(i) => self.add(UNPACK_EX, (i + ((n - i - 1) << 8)) as i64, loc),
                    None => self.add(UNPACK_SEQUENCE, n as i64, loc),
                }
                for x in elts {
                    match &x.kind {
                        E::Starred { value, .. } => self.store_target(value)?,
                        _ => self.store_target(x)?,
                    }
                }
            }
            _ => return Err(Unsupported),
        }
        Ok(())
    }

    /// `compiler_jump_if`: salta para `target` se a verdade de `e` for `cond`.
    fn jump_if(&mut self, e: &Expr, target: usize, cond: bool) -> Res<()> {
        // Teste constante (inclusive dobrado): `LOAD_CONST`, `TO_BOOL` e o salto condicional viram `NOP` e, se a
        // verdade da constante é a que faz saltar, o salto vira incondicional (`optimize_basic_block`).
        if let Some(cv) = fold(e)? {
            let loc = self.loc(&e.pos);
            self.add(NOP, 0, loc);
            if key_truth(&cv.key) == cond {
                self.add_jump(JUMP, target, loc);
            }
            return Ok(());
        }
        match &e.kind {
            E::UnaryOp { op: UnaryOp::Not, operand } => {
                // O otimizador de AST troca `not (a is b)` e `not (a in b)` pelo operador oposto.
                if let Some(flipped) = flip_not_compare(operand) {
                    return self.jump_if(&flipped, target, cond);
                }
                return self.jump_if(operand, target, !cond);
            }
            E::BoolOp { op, values } => {
                let cond2 = *op == BoolOp::Or;
                let next2 = if cond2 != cond { self.cfg.new_block() } else { target };
                let n = values.len() - 1;
                for v in &values[..n] {
                    self.jump_if(v, next2, cond2)?;
                }
                self.jump_if(&values[n], target, cond)?;
                if cond2 != cond {
                    self.cfg.use_block(next2);
                }
                return Ok(());
            }
            E::Compare { left, ops, comparators } if ops.len() > 1 => {
                // Comparação encadeada em condição: cada elo falho cai em `cleanup`, que descarta o operando que
                // sobrou na pilha.
                let loc = self.loc(&e.pos);
                let n = ops.len() - 1;
                let cleanup = self.cfg.new_block();
                self.expr(left)?;
                for (c, op) in comparators.iter().zip(ops.iter()).take(n) {
                    self.expr(c)?;
                    self.add(SWAP, 2, loc);
                    self.add(COPY, 2, loc);
                    self.cmp_op(*op, loc)?;
                    self.add(TO_BOOL, 0, loc);
                    self.add_jump(POP_JUMP_IF_FALSE, cleanup, loc);
                }
                let (Some(last), Some(&last_op)) = (comparators.get(n), ops.get(n)) else { return Err(Unsupported) };
                self.cmp_right(last_op, last)?;
                self.cmp_op(last_op, loc)?;
                self.add(TO_BOOL, 0, loc);
                self.add_jump(if cond { POP_JUMP_IF_TRUE } else { POP_JUMP_IF_FALSE }, target, loc);
                let end = self.cfg.new_block();
                // Em condição o salto por cima do `cleanup` leva a localização da comparação: o `NOP` em que a cópia
                // da saída o transforma sobrevive, pois abre o bloco (o oráculo mostra o `NOP` na linha do `if`).
                self.add_jump(JUMP, end, loc);
                self.cfg.use_block(cleanup);
                self.add(POP_TOP, 0, loc);
                if !cond {
                    self.add_jump(JUMP, target, NO_LOC);
                }
                self.cfg.use_block(end);
                return Ok(());
            }
            E::Compare { left, ops, comparators } if ops.len() == 1 => {
                let against_none = matches!(&comparators[0].kind, E::Constant { value: Constant::None, .. });
                if against_none && matches!(ops[0], CmpOp::Is | CmpOp::IsNot) {
                    self.expr(left)?;
                    let is_none_jump = (ops[0] == CmpOp::Is) == cond;
                    let op = if is_none_jump { POP_JUMP_IF_NONE } else { POP_JUMP_IF_NOT_NONE };
                    let loc = self.loc(&e.pos);
                    self.add_jump(op, target, loc);
                    return Ok(());
                }
            }
            _ => {}
        }
        self.expr(e)?;
        let loc = self.loc(&e.pos);
        self.add(TO_BOOL, 0, loc);
        self.add_jump(if cond { POP_JUMP_IF_TRUE } else { POP_JUMP_IF_FALSE }, target, loc);
        Ok(())
    }

    fn expr(&mut self, e: &Expr) -> Res<()> {
        let loc = self.loc(&e.pos);
        if self.load_folded(e)? {
            return Ok(());
        }
        match &e.kind {
            E::Name { id, ctx: ExprContext::Load } => {
                self.load_name(id, loc, false)?;
            }
            E::Attribute { value, attr, ctx: ExprContext::Load } => {
                if !self.super_attr(value, attr, false, loc)? {
                    self.expr(value)?;
                    let l = match_attr(loc, loc, attr);
                    let i = self.name_index(attr);
                    self.add(LOAD_ATTR, (i as i64) << 1, l);
                }
            }
            E::Call { func, args, keywords } => self.call(e, func, args, keywords)?,
            E::BinOp { left, op, right } => {
                self.expr(left)?;
                self.expr(right)?;
                self.add(BINARY_OP, nb_op(*op), loc);
            }
            E::UnaryOp { op, operand } => self.unary(e, *op, operand)?,
            E::Compare { left, ops, comparators } if ops.len() == 1 => {
                self.expr(left)?;
                self.cmp_right(ops[0], &comparators[0])?;
                self.cmp_op(ops[0], loc)?;
            }
            E::Compare { left, ops, comparators } => {
                // `compiler_compare` encadeado: o resultado de um elo falho fica na pilha (`COPY 1`) e `cleanup` troca
                // o operando que sobrou por ele.
                let n = ops.len().checked_sub(1).ok_or(Unsupported)?;
                let cleanup = self.cfg.new_block();
                let end = self.cfg.new_block();
                self.expr(left)?;
                for (c, op) in comparators.iter().zip(ops.iter()).take(n) {
                    self.expr(c)?;
                    self.add(SWAP, 2, loc);
                    self.add(COPY, 2, loc);
                    self.cmp_op(*op, loc)?;
                    self.add(COPY, 1, loc);
                    self.add(TO_BOOL, 0, loc);
                    self.add_jump(POP_JUMP_IF_FALSE, cleanup, loc);
                    self.add(POP_TOP, 0, loc);
                }
                let (Some(last), Some(&last_op)) = (comparators.get(n), ops.get(n)) else { return Err(Unsupported) };
                self.cmp_right(last_op, last)?;
                self.cmp_op(last_op, loc)?;
                self.add_jump(JUMP, end, NO_LOC);
                self.cfg.use_block(cleanup);
                self.add(SWAP, 2, loc);
                self.add(POP_TOP, 0, loc);
                self.cfg.use_block(end);
            }
            E::BoolOp { op, values } => {
                // `compiler_boolop`: cada operando menos o último é copiado, testado e descartado se não decide.
                let jump = if *op == BoolOp::And { POP_JUMP_IF_FALSE } else { POP_JUMP_IF_TRUE };
                let end = self.cfg.new_block();
                let n = values.len().checked_sub(1).ok_or(Unsupported)?;
                for v in &values[..n] {
                    self.expr(v)?;
                    self.add(COPY, 1, loc);
                    self.add(TO_BOOL, 0, loc);
                    self.add_jump(jump, end, loc);
                    self.add(POP_TOP, 0, loc);
                }
                self.expr(&values[n])?;
                self.cfg.use_block(end);
            }
            E::IfExp { test, body, orelse } => {
                let end = self.cfg.new_block();
                let next = self.cfg.new_block();
                self.jump_if(test, next, false)?;
                self.expr(body)?;
                self.add_jump(JUMP, end, NO_LOC);
                self.cfg.use_block(next);
                self.expr(orelse)?;
                self.cfg.use_block(end);
            }
            E::NamedExpr { target, value } => {
                self.expr(value)?;
                self.add(COPY, 1, loc);
                self.store_target(target)?;
            }
            E::Subscript { value, slice, ctx: ExprContext::Load } => {
                self.expr(value)?;
                if is_two_element_slice(slice) {
                    self.slice_bounds(slice)?;
                    self.add(BINARY_SLICE, 0, loc);
                } else {
                    self.expr(slice)?;
                    self.add(BINARY_SUBSCR, 0, loc);
                }
            }
            E::Slice { lower, upper, step } => {
                for bound in [lower, upper] {
                    match bound {
                        Some(b) => self.expr(b)?,
                        None => self.load_cv(Cv::none(), loc),
                    }
                }
                let mut n = 2;
                if let Some(st) = step {
                    n += 1;
                    self.expr(st)?;
                }
                self.add(BUILD_SLICE, n, loc);
            }
            E::JoinedStr { values } => {
                if values.len() > STACK_USE_GUIDELINE {
                    return Err(Unsupported);
                }
                for v in values {
                    self.expr(v)?;
                }
                match values.len() {
                    0 => self.load_cv(Cv::str(String::new()), loc),
                    1 => {}
                    n => self.add(BUILD_STRING, n as i64, loc),
                }
            }
            E::FormattedValue { value, conversion, format_spec } => {
                self.expr(value)?;
                match conversion {
                    -1 => {}
                    115 => self.add(CONVERT_VALUE, 1, loc),
                    114 => self.add(CONVERT_VALUE, 2, loc),
                    97 => self.add(CONVERT_VALUE, 3, loc),
                    _ => return Err(Unsupported),
                }
                match format_spec {
                    Some(spec) => {
                        self.expr(spec)?;
                        self.add(FORMAT_WITH_SPEC, 0, loc);
                    }
                    None => self.add(FORMAT_SIMPLE, 0, loc),
                }
            }
            E::Tuple { elts, ctx: ExprContext::Load } => self.sequence(elts, Seq::Tuple, loc)?,
            E::List { elts, ctx: ExprContext::Load } => self.sequence(elts, Seq::List, loc)?,
            E::Set { elts } => self.sequence(elts, Seq::Set, loc)?,
            E::Dict { keys, values } => self.dict(keys, values, loc)?,
            E::Lambda { args, .. } => self.make_function(args, None, loc)?,
            E::Yield { value } => self.yield_expr(value.as_deref(), loc)?,
            E::YieldFrom { value } => self.yield_from_expr(value, loc)?,
            E::Await { value } => self.await_expr(value, loc)?,
            E::ListComp { elt, generators } => self.comprehension(e, suspend::Comp::List, elt, None, generators)?,
            E::SetComp { elt, generators } => self.comprehension(e, suspend::Comp::Set, elt, None, generators)?,
            E::DictComp { key, value, generators } => {
                self.comprehension(e, suspend::Comp::Dict, key, Some(&**value), generators)?
            }
            E::GeneratorExp { generators, .. } => self.genexp(e, generators)?,
            _ => return Err(Unsupported),
        }
        Ok(())
    }

    /// `ADDOP_COMPARE`: o operador de comparação (`IS_OP`, `CONTAINS_OP` ou `COMPARE_OP`).
    fn cmp_op(&mut self, op: CmpOp, loc: Loc) -> Res<()> {
        match op {
            CmpOp::Is => self.add(IS_OP, 0, loc),
            CmpOp::IsNot => self.add(IS_OP, 1, loc),
            CmpOp::In => self.add(CONTAINS_OP, 0, loc),
            CmpOp::NotIn => self.add(CONTAINS_OP, 1, loc),
            other => {
                let arg = compare_arg(other).ok_or(Unsupported)?;
                self.add(COMPARE_OP, arg, loc);
            }
        }
        Ok(())
    }

    /// O operando da direita do último operador: com `in` e `not in` o otimizador de AST troca a lista ou o conjunto
    /// de constantes por uma tupla ou um `frozenset` (`fold_compare`).
    fn cmp_right(&mut self, op: CmpOp, right: &Expr) -> Res<()> {
        if matches!(op, CmpOp::In | CmpOp::NotIn) {
            self.iter_value(right)
        } else {
            self.expr(right)
        }
    }

    /// `compiler_slice` quando só `BINARY_SLICE` e `STORE_SLICE` o usam: os dois limites, com `None` no que falta.
    fn slice_bounds(&mut self, slice: &Expr) -> Res<()> {
        let E::Slice { lower, upper, .. } = &slice.kind else { return Err(Unsupported) };
        let loc = self.loc(&slice.pos);
        for bound in [lower, upper] {
            match bound {
                Some(b) => self.expr(b)?,
                None => self.load_cv(Cv::none(), loc),
            }
        }
        Ok(())
    }

    /// `starunpack_helper`: os elementos e o `BUILD_*`. Com `*x`, a coleção é criada no primeiro `*` e cada elemento
    /// vira `*_APPEND` ou `*_EXTEND`; tupla com `*` passa por lista e `INTRINSIC_LIST_TO_TUPLE`. Mais de dois
    /// elementos constantes: a tupla constante é estendida na lista vazia (ou é a própria tupla).
    fn sequence(&mut self, elts: &[Expr], kind: Seq, loc: Loc) -> Res<()> {
        let n = elts.len();
        if n > STACK_USE_GUIDELINE {
            return Err(Unsupported);
        }
        let (build, add, extend) = match kind {
            Seq::List | Seq::Tuple => (BUILD_LIST, LIST_APPEND, LIST_EXTEND),
            Seq::Set => (BUILD_SET, SET_ADD, SET_UPDATE),
        };
        if n > 2 {
            if let Some(items) = fold_all(elts)? {
                match kind {
                    Seq::Tuple => self.load_cv(Cv::tuple(items), loc),
                    Seq::List => {
                        self.add(BUILD_LIST, 0, loc);
                        self.load_cv(Cv::tuple(items), loc);
                        self.add(LIST_EXTEND, 1, loc);
                    }
                    Seq::Set => {
                        self.add(BUILD_SET, 0, loc);
                        self.load_cv(Cv::frozenset(items)?, loc);
                        self.add(SET_UPDATE, 1, loc);
                    }
                }
                return Ok(());
            }
        }
        if !elts.iter().any(is_starred) {
            for x in elts {
                self.expr(x)?;
            }
            let plain = match kind {
                Seq::Tuple => BUILD_TUPLE,
                Seq::List => BUILD_LIST,
                Seq::Set => BUILD_SET,
            };
            self.add(plain, n as i64, loc);
            return Ok(());
        }
        let mut built = false;
        for (i, x) in elts.iter().enumerate() {
            if let E::Starred { value, .. } = &x.kind {
                if !built {
                    self.add(build, i as i64, loc);
                    built = true;
                }
                self.expr(value)?;
                self.add(extend, 1, loc);
            } else {
                self.expr(x)?;
                if built {
                    self.add(add, 1, loc);
                }
            }
        }
        if kind == Seq::Tuple {
            self.add(CALL_INTRINSIC_1, INTRINSIC_LIST_TO_TUPLE, loc);
        }
        Ok(())
    }

    /// `compiler_dict`: `{k: v}` sai como `BUILD_MAP` ou `BUILD_CONST_KEY_MAP` (`compiler_subdict`); cada `**x` funde o
    /// que veio antes com `DICT_UPDATE 1`. No máximo 15 pares.
    fn dict(&mut self, keys: &[Option<Expr>], values: &[Expr], loc: Loc) -> Res<()> {
        let n = keys.len();
        if n * 2 > STACK_USE_GUIDELINE || values.len() != n {
            return Err(Unsupported);
        }
        let mut have_dict = false;
        let mut elements = 0usize;
        for (i, key) in keys.iter().enumerate() {
            if key.is_none() {
                if elements > 0 {
                    self.subdict(keys, values, i - elements, i, loc)?;
                    if have_dict {
                        self.add(DICT_UPDATE, 1, loc);
                    }
                    have_dict = true;
                    elements = 0;
                }
                if !have_dict {
                    self.add(BUILD_MAP, 0, loc);
                    have_dict = true;
                }
                self.expr(&values[i])?;
                self.add(DICT_UPDATE, 1, loc);
            } else {
                elements += 1;
            }
        }
        if elements > 0 {
            self.subdict(keys, values, n - elements, n, loc)?;
            if have_dict {
                self.add(DICT_UPDATE, 1, loc);
            }
            have_dict = true;
        }
        if !have_dict {
            self.add(BUILD_MAP, 0, loc);
        }
        Ok(())
    }

    /// `compiler_subdict`: os pares `begin..end`; chaves todas constantes (e mais de uma) saem como
    /// `BUILD_CONST_KEY_MAP`, as outras como `BUILD_MAP`.
    fn subdict(&mut self, keys: &[Option<Expr>], values: &[Expr], begin: usize, end: usize, loc: Loc) -> Res<()> {
        let n = end - begin;
        let ks = keys.get(begin..end).ok_or(Unsupported)?;
        let vs = values.get(begin..end).ok_or(Unsupported)?;
        let const_keys = if n > 1 { fold_all(ks.iter().flatten())? } else { None };
        if let Some(const_keys) = const_keys {
            for v in vs {
                self.expr(v)?;
            }
            self.load_cv(Cv::tuple(const_keys), loc);
            self.add(BUILD_CONST_KEY_MAP, n as i64, loc);
        } else {
            for (k, v) in ks.iter().flatten().zip(vs) {
                self.expr(k)?;
                self.expr(v)?;
            }
            self.add(BUILD_MAP, n as i64, loc);
        }
        Ok(())
    }

    fn unary(&mut self, e: &Expr, op: UnaryOp, operand: &Expr) -> Res<()> {
        let loc = self.loc(&e.pos);
        // `not (a is b)` e `not (a in b)`: o otimizador de AST os reescreve como o operador oposto, na posição da
        // comparação.
        if op == UnaryOp::Not {
            if let Some(flipped) = flip_not_compare(operand) {
                return self.expr(&flipped);
            }
        }
        self.expr(operand)?;
        match op {
            UnaryOp::USub => self.add(UNARY_NEGATIVE, 0, loc),
            UnaryOp::Invert => self.add(UNARY_INVERT, 0, loc),
            UnaryOp::UAdd => self.add(CALL_INTRINSIC_1, 5, loc),
            UnaryOp::Not => {
                self.add(TO_BOOL, 0, loc);
                self.add(UNARY_NOT, 0, loc);
            }
        }
        Ok(())
    }

    /// `compiler_call`: chamada comum, de método (`LOAD_ATTR` com o bit de método), com nomeados ou, com `*x`, `**x`
    /// ou argumentos demais, por `CALL_FUNCTION_EX`.
    fn call(&mut self, e: &Expr, func: &Expr, args: &[Expr], keywords: &[Keyword]) -> Res<()> {
        let plain_args = !args.iter().any(is_starred) && !keywords.iter().any(|k| k.arg.is_none());
        let mut call_loc = self.loc(&e.pos);
        // Os nomes dos argumentos nomeados saem com a localização do atributo na chamada de método (a do
        // `LOAD_ATTR`), e com a da chamada inteira nas outras.
        let mut names_loc = call_loc;
        let method = match &func.kind {
            E::Attribute { value, attr, ctx: ExprContext::Load } => {
                let imported = matches!(&value.kind, E::Name { id, .. } if self.imports.contains(id));
                let small = args.len() + keywords.len() + usize::from(!keywords.is_empty()) < STACK_USE_GUIDELINE;
                (!imported && small && plain_args).then_some((value, attr))
            }
            _ => None,
        };
        let by_ex = !plain_args || args.len() + keywords.len() * 2 > STACK_USE_GUIDELINE;
        if by_ex && method.is_some() {
            return Err(Unsupported);
        }
        if let Some((value, attr)) = method {
            let attr_loc = self.loc(&func.pos);
            let meth_loc = match_attr(attr_loc, attr_loc, attr);
            if !self.super_attr(value, attr, true, attr_loc)? {
                self.expr(value)?;
                let i = self.name_index(attr);
                self.add(LOAD_ATTR, ((i as i64) << 1) | 1, meth_loc);
            }
            call_loc = match_attr(call_loc, attr_loc, attr);
            names_loc = meth_loc;
        } else if let E::Name { id, .. } = &func.kind {
            let floc = self.loc(&func.pos);
            if self.load_name(id, floc, true)? {
                self.add(PUSH_NULL, 0, floc);
            }
        } else {
            self.expr(func)?;
            let floc = self.loc(&func.pos);
            self.add(PUSH_NULL, 0, floc);
        }
        if by_ex {
            return self.call_ex(args, keywords, call_loc);
        }
        for a in args {
            self.expr(a)?;
        }
        if keywords.is_empty() {
            self.add(CALL, args.len() as i64, call_loc);
            return Ok(());
        }
        let mut names = Vec::new();
        for k in keywords {
            let name = k.arg.clone().ok_or(Unsupported)?;
            self.check_name(&name)?;
            self.expr(&k.value)?;
            names.push(Cv::str(name));
        }
        self.load_cv(Cv::tuple(names), names_loc);
        self.add(CALL_KW, (args.len() + keywords.len()) as i64, call_loc);
        Ok(())
    }

    /// O `ex_call` de `compiler_call_helper`: os posicionais viram uma tupla (ou o único `*x`), os nomeados um
    /// dicionário (`**x` entra por `DICT_MERGE 1`), e `CALL_FUNCTION_EX` chama.
    fn call_ex(&mut self, args: &[Expr], keywords: &[Keyword], loc: Loc) -> Res<()> {
        match args {
            [only] if is_starred(only) => {
                if let E::Starred { value, .. } = &only.kind {
                    self.expr(value)?;
                }
            }
            _ => self.sequence(args, Seq::Tuple, loc)?,
        }
        if !keywords.is_empty() {
            let mut have_dict = false;
            let mut seen = 0usize;
            for (i, kw) in keywords.iter().enumerate() {
                if kw.arg.is_some() {
                    seen += 1;
                    continue;
                }
                if seen > 0 {
                    self.subkwargs(&keywords[i - seen..i], loc)?;
                    if have_dict {
                        self.add(DICT_MERGE, 1, loc);
                    }
                    have_dict = true;
                    seen = 0;
                }
                if !have_dict {
                    self.add(BUILD_MAP, 0, loc);
                    have_dict = true;
                }
                self.expr(&kw.value)?;
                self.add(DICT_MERGE, 1, loc);
            }
            if seen > 0 {
                self.subkwargs(&keywords[keywords.len() - seen..], loc)?;
                if have_dict {
                    self.add(DICT_MERGE, 1, loc);
                }
            }
        }
        self.add(CALL_FUNCTION_EX, i64::from(!keywords.is_empty()), loc);
        Ok(())
    }

    /// `compiler_subkwargs`: um trecho de argumentos nomeados vira `BUILD_CONST_KEY_MAP` (mais de um) ou `BUILD_MAP 1`.
    fn subkwargs(&mut self, kws: &[Keyword], loc: Loc) -> Res<()> {
        let n = kws.len();
        if n * 2 > STACK_USE_GUIDELINE {
            return Err(Unsupported);
        }
        if n > 1 {
            let mut names = Vec::new();
            for kw in kws {
                let name = kw.arg.clone().ok_or(Unsupported)?;
                self.check_name(&name)?;
                self.expr(&kw.value)?;
                names.push(Cv::str(name));
            }
            self.load_cv(Cv::tuple(names), loc);
            self.add(BUILD_CONST_KEY_MAP, n as i64, loc);
        } else {
            for kw in kws {
                let name = kw.arg.clone().ok_or(Unsupported)?;
                self.check_name(&name)?;
                self.load_cv(Cv::str(name), loc);
                self.expr(&kw.value)?;
            }
            self.add(BUILD_MAP, n as i64, loc);
        }
        Ok(())
    }

    /// `remove_unused_consts`: descarta as constantes que nenhuma instrução usa (a primeira fica sempre, pois
    /// pode ser a docstring) e renumera as outras.
    fn remove_unused_consts(&mut self) {
        let mut used = vec![false; self.consts.len()];
        if let Some(first) = used.first_mut() {
            *first = true;
        }
        for &b in &self.cfg.order {
            for ins in &self.cfg.blocks[b] {
                if matches!(ins.op, LOAD_CONST | RETURN_CONST) {
                    used[ins.arg as usize] = true;
                }
            }
        }
        let mut remap = vec![0i64; used.len()];
        let mut next = 0;
        for (i, u) in used.iter().enumerate() {
            remap[i] = next;
            if *u {
                next += 1;
            }
        }
        for b in self.cfg.order.clone() {
            for ins in self.cfg.blocks[b].iter_mut() {
                if matches!(ins.op, LOAD_CONST | RETURN_CONST) {
                    ins.arg = remap[ins.arg as usize];
                }
            }
        }
        let mut keep = used.iter();
        self.consts.retain(|_| *keep.next().unwrap_or(&true));
    }

    /// `fold_tuple_on_constants`: `LOAD_CONST` n vezes seguido de `BUILD_TUPLE n` vira o `LOAD_CONST` da tupla.
    fn fold_tuples(&mut self) {
        let mut blocks = std::mem::take(&mut self.cfg.blocks);
        for blk in blocks.iter_mut() {
            for i in 0..blk.len() {
                let n = blk[i].arg as usize;
                if blk[i].op != BUILD_TUPLE || n > i || !blk[i - n..i].iter().all(|x| x.op == LOAD_CONST) {
                    continue;
                }
                if blk[i - n..i].iter().any(|x| matches!(self.keys[x.arg as usize], Key::Code(_))) {
                    continue;
                }
                let items: Vec<Cv> = blk[i - n..i]
                    .iter()
                    .map(|x| Cv { key: self.keys[x.arg as usize].clone(), value: self.consts[x.arg as usize].clone() })
                    .collect();
                let idx = self.add_cv(Cv::tuple(items));
                for x in &mut blk[i - n..i] {
                    x.op = NOP;
                    x.arg = 0;
                }
                blk[i].op = LOAD_CONST;
                blk[i].arg = idx as i64;
            }
        }
        self.cfg.blocks = blocks;
    }

    /// `insert_prefix_instructions`: `COPY_FREE_VARS` e depois um `MAKE_CELL` por célula, na ordem dos índices,
    /// sem localização (o `dis` mostra `--`).
    fn insert_prefix(&mut self) {
        let mut prefix = Vec::new();
        if !self.code.freevars.is_empty() {
            prefix.push(Instr::new(COPY_FREE_VARS, self.code.freevars.len() as i64, NO_LOC));
        }
        let mut cells: Vec<usize> =
            self.code.cellvars.iter().filter_map(|c| self.localsplus.iter().position(|v| v == c)).collect();
        cells.sort_unstable();
        prefix.extend(cells.into_iter().map(|i| Instr::new(MAKE_CELL, i as i64, NO_LOC)));
        if self.suspendable() {
            prefix.extend(self.generator_prefix());
        }
        if let Some(&entry) = self.cfg.order.first() {
            let rest = std::mem::take(&mut self.cfg.blocks[entry]);
            prefix.extend(rest);
            self.cfg.blocks[entry] = prefix;
        }
    }

    /// Os passes de `_PyCfg_OptimizeCodeUnit` e a montagem. `None` se o grafo ainda tem o que o emissor não monta
    /// ou se o número de funções aninhadas criadas não bate com o de `Code::functions`.
    fn finish(mut self, first_line: i32, nparams: usize) -> Option<Emitted> {
        if self.next_fn != self.code.functions.len() {
            return None;
        }
        let nlocals = self.code.varnames.len();
        self.cfg.label_exception_targets();
        self.cfg.eliminate_empty();
        self.cfg.inline_small_exit_blocks();
        self.fold_tuples();
        self.cfg.optimize();
        self.fold_none_jumps();
        self.cfg.optimize_swaps();
        self.cfg.remove_redundant_nops();
        // O 3.13.5 só inlina antes do `optimize_basic_block` (`inline_small_or_no_lineno_blocks`): uma saída que só fica
        // pequena depois de os `NOP` sumirem (o fim de um `match` com dois `POP_TOP` e um `return` constante) não é copiada.
        self.cfg.drop_unreachable();
        self.cfg.remove_redundant_nops();
        self.cfg.eliminate_empty();
        self.cfg.remove_redundant_jumps();
        self.remove_unused_consts();
        self.cfg.add_checks(nlocals, nparams);
        self.cfg.insert_superinstructions();
        self.cfg.eliminate_empty();
        self.cfg.push_cold_blocks_to_end();
        self.cfg.resolve_line_numbers();
        let (maxdepth, startdepth) = self.cfg.stackdepth();
        let stacksize = maxdepth.max(0) as usize;
        self.insert_prefix();
        self.cfg.convert_pseudo_ops();
        self.cfg.normalize_jumps();
        let (code, locs, exceptiontable) = self.cfg.assemble(&startdepth)?;
        Some(Emitted {
            code,
            consts: self.consts,
            names: self.names,
            linetable: encode_locations(first_line, &locs),
            exceptiontable,
            stacksize,
            first_line,
            synthetic: false,
        })
    }
}

/// Chave de [`module_imports`] para "o módulo liga `super` sem ser por `import`" (não é um identificador válido).
const SUPER_BOUND: &str = "<super>";

/// Os nomes ligados por `import` no nível do módulo (`DEF_IMPORT` na tabela de símbolos global), onde o
/// compilador não otimiza a chamada de método. Leva também a chave [`SUPER_BOUND`] quando o módulo liga `super` de
/// qualquer outra forma (atribuição, `def`, `class`, `for`, `with`, `except`, `del`, `global`): a tabela global então
/// define o nome e `can_optimize_super_call` não emite `LOAD_SUPER_ATTR`.
pub fn module_imports(body: &[Stmt]) -> HashSet<String> {
    fn bind_targets(targets: &[&Expr], out: &mut HashSet<String>) {
        let mut names = Vec::new();
        targets.iter().for_each(|t| crate::compile::ordered_targets(t, &mut names));
        if names.iter().any(|n| n == "super") {
            out.insert(SUPER_BOUND.to_string());
        }
    }
    fn walk(body: &[Stmt], out: &mut HashSet<String>) {
        for s in body {
            match &s.kind {
                S::Assign { targets, .. } => bind_targets(&targets.iter().collect::<Vec<_>>(), out),
                S::AugAssign { target, .. } | S::AnnAssign { target, .. } => bind_targets(&[&**target], out),
                S::Delete { targets } => bind_targets(&targets.iter().collect::<Vec<_>>(), out),
                S::For { target, .. } | S::AsyncFor { target, .. } => bind_targets(&[&**target], out),
                S::With { items, .. } | S::AsyncWith { items, .. } => {
                    bind_targets(&items.iter().filter_map(|i| i.optional_vars.as_deref()).collect::<Vec<_>>(), out)
                }
                S::Try { handlers, .. } | S::TryStar { handlers, .. } if handlers.iter().any(|h| h.name.as_deref() == Some("super")) => {
                    out.insert(SUPER_BOUND.to_string());
                }
                S::FunctionDef { name, .. } | S::AsyncFunctionDef { name, .. } | S::ClassDef { name, .. } if name == "super" => {
                    out.insert(SUPER_BOUND.to_string());
                }
                S::Global { names } if names.iter().any(|n| n == "super") => {
                    out.insert(SUPER_BOUND.to_string());
                }
                _ => {}
            }
            match &s.kind {
                S::Import { names } => {
                    for a in names {
                        let bound = a.asname.clone().unwrap_or_else(|| a.name.split('.').next().unwrap_or("").to_string());
                        out.insert(bound);
                    }
                }
                S::ImportFrom { names, .. } => {
                    for a in names {
                        out.insert(a.asname.clone().unwrap_or_else(|| a.name.clone()));
                    }
                }
                S::If { body, orelse, .. } | S::For { body, orelse, .. } | S::AsyncFor { body, orelse, .. } | S::While { body, orelse, .. } => {
                    walk(body, out);
                    walk(orelse, out);
                }
                S::With { body, .. } | S::AsyncWith { body, .. } => walk(body, out),
                S::Try { body, handlers, orelse, finalbody } | S::TryStar { body, handlers, orelse, finalbody } => {
                    walk(body, out);
                    for h in handlers {
                        walk(&h.body, out);
                    }
                    walk(orelse, out);
                    walk(finalbody, out);
                }
                _ => {}
            }
        }
    }
    let mut out = HashSet::new();
    walk(body, &mut out);
    out
}

/// Parâmetros da assinatura (`co_argcount` + só-nomeados + `*args` + `**kwargs`): os primeiros locais, sempre
/// inicializados.
fn param_count(code: &Code) -> usize {
    code.params.len() + code.kwonly.len() + usize::from(code.vararg.is_some()) + usize::from(code.kwarg.is_some())
}

/// A função comum (nem gerador, nem corrotina, nem corpo de classe) tem bytecode emitido.
fn is_plain_function(code: &Code) -> bool {
    code.is_function && !code.is_class && !code.is_generator && !code.is_async
}

/// O bytecode de uma função `def` (o corpo, depois da docstring, e os parâmetros já resolvidos em `code`).
pub fn function(
    code: &Code,
    body: &[Stmt],
    globals_decl: &HashSet<String>,
    imports: &HashSet<String>,
    comp_only: HashSet<String>,
    future: bool,
) -> Option<Emitted> {
    if !code.is_function || code.is_class {
        return None;
    }
    let first = code.first_line.max(1) as i32;
    let resume = Loc { line: first, end_line: first, col: 0, end_col: 0 };
    let mut g = Gen::new(true, code, globals_decl, imports, resume);
    g.comp_only = comp_only;
    g.future = future;
    let rest = match &code.doc {
        Some(doc) => {
            g.add_const(Key::Str(doc.clone()), Value::str(doc.clone()));
            body.get(1..).unwrap_or(&[])
        }
        None => {
            g.add_const(Key::None, Value::None);
            body
        }
    };
    g.stmts(rest).ok()?;
    g.implicit_return();
    if g.suspendable() {
        g.stop_iteration_handler();
    }
    g.finish(first, param_count(code))
}

/// O bytecode de um `lambda`: `None` é a primeira constante (sem docstring) e o corpo é um `RETURN_VALUE` na
/// localização da expressão.
pub fn lambda(
    code: &Code,
    body: &Expr,
    globals_decl: &HashSet<String>,
    imports: &HashSet<String>,
    comp_only: HashSet<String>,
    return_pos: Option<Pos>,
) -> Option<Emitted> {
    if !is_plain_function(code) {
        return None;
    }
    let first = code.first_line.max(1) as i32;
    let resume = Loc { line: first, end_line: first, col: 0, end_col: 0 };
    let mut g = Gen::new(true, code, globals_decl, imports, resume);
    g.comp_only = comp_only;
    g.add_const(Key::None, Value::None);
    g.expr(body).ok()?;
    // O valor de um alias (`type X = v`) devolve com a localização da instrução; o resto, com a do corpo.
    let loc = g.loc(return_pos.as_ref().unwrap_or(&body.pos));
    g.add(RETURN_VALUE, 0, loc);
    g.finish(first, param_count(code))
}

/// O bytecode do corpo de uma classe (`compiler_class_body`): `__module__`, `__qualname__`, `__firstlineno__`, o
/// `SETUP_ANNOTATIONS` se há anotação e a docstring no começo, o corpo, e no fim `__static_attributes__` (sem
/// localização) e o retorno de `None`, ou o de `__classcell__` quando um método usa `__class__` ou `super()` (a única
/// célula do corpo). As variáveis de função de fora que o corpo e os métodos fecham são as `freevars` da classe
/// (`COPY_FREE_VARS` no prefixo, `LOAD_LOCALS` e `LOAD_FROM_DICT_OR_DEREF` na leitura direta).
pub fn class_body(code: &Code, body: &[Stmt], first_line: i32, imports: &HashSet<String>, future: bool) -> Option<Emitted> {
    // Corpo de classe criado dentro de `<generic parameters of C>` (PEP 695): a única variável livre é `.type_params`.
    let in_type_params = code.type_params_role & crate::pep695::CHILD != 0;
    let view = in_type_params.then(|| generic::class_view(code));
    let code = view.as_ref().unwrap_or(code);
    let class_cell = code.cellvars.iter().any(|c| &**c == "__class__");
    if !code.is_class
        || (in_type_params && code.freevars.len() != 1)
        || code.cellvars.iter().any(|c| &**c != "__class__")
        || code.uses_class_cell != class_cell
    {
        return None;
    }
    let no_globals = HashSet::new();
    let loc = Loc { line: first_line, end_line: first_line, col: 0, end_col: 0 };
    let mut g = Gen::new(false, code, &no_globals, imports, loc);
    g.future = future;
    let name = g.name_index("__name__");
    g.add(LOAD_NAME, name as i64, loc);
    g.store_name("__module__", loc).ok()?;
    let qualname = g.add_const(Key::Str(code.qualname.clone()), Value::str(code.qualname.clone()));
    g.add(LOAD_CONST, qualname as i64, loc);
    g.store_name("__qualname__", loc).ok()?;
    let line = g.add_const(Key::Int(i64::from(first_line)), Value::Int(i64::from(first_line)));
    g.add(LOAD_CONST, line as i64, loc);
    g.store_name("__firstlineno__", loc).ok()?;
    if in_type_params {
        g.type_params_store(loc)?;
    }
    if has_annotation(body) {
        g.add(SETUP_ANNOTATIONS, 0, loc);
    }
    let rest = match crate::compile::docstring(body) {
        Some(doc) => {
            let doc_loc = g.loc(&body.first()?.pos);
            let i = g.add_const(Key::Str(doc.clone()), Value::str(doc));
            g.add(LOAD_CONST, i as i64, doc_loc);
            g.store_name("__doc__", doc_loc).ok()?;
            body.get(1..).unwrap_or(&[])
        }
        None => body,
    };
    g.stmts(rest).ok()?;
    let mut attrs = std::collections::BTreeSet::new();
    crate::compile::static_attributes(body, &mut attrs);
    g.load_cv(Cv::tuple(attrs.into_iter().map(Cv::str).collect()), NO_LOC);
    g.store_name("__static_attributes__", NO_LOC).ok()?;
    if class_cell {
        // O corpo devolve a célula `__class__`, que `__build_class__` entrega ao `type`.
        let cell = g.localsplus.iter().position(|v| &**v == "__class__")?;
        g.add(LOAD_CLOSURE, cell as i64, NO_LOC);
        g.add(COPY, 1, NO_LOC);
        g.store_name("__classcell__", NO_LOC).ok()?;
        g.add(RETURN_VALUE, 0, NO_LOC);
    } else {
        g.implicit_return();
    }
    g.finish(first_line, 0)
}

/// `find_ann`: algum `x: T` no corpo (dentro de `if`, laços, `with` e `try`, mas não de funções e classes), que pede
/// `SETUP_ANNOTATIONS` no começo do módulo ou da classe.
fn has_annotation(body: &[Stmt]) -> bool {
    body.iter().any(|s| match &s.kind {
        S::AnnAssign { .. } => true,
        S::For { body, orelse, .. } | S::AsyncFor { body, orelse, .. } | S::While { body, orelse, .. } | S::If { body, orelse, .. } => {
            has_annotation(body) || has_annotation(orelse)
        }
        S::With { body, .. } | S::AsyncWith { body, .. } => has_annotation(body),
        S::Try { body, handlers, orelse, finalbody } | S::TryStar { body, handlers, orelse, finalbody } => {
            handlers.iter().any(|h| has_annotation(&h.body))
                || has_annotation(body)
                || has_annotation(finalbody)
                || has_annotation(orelse)
        }
        _ => false,
    })
}

/// O bytecode de um módulo (`compile(..., 'exec')`); com `interactive`, o de `compile(..., 'single')`.
pub fn module(code: &Code, body: &[Stmt], imports: &HashSet<String>, future: bool, interactive: bool) -> Option<Emitted> {
    let no_globals = HashSet::new();
    let resume = Loc { line: 0, end_line: 1, col: 0, end_col: 0 };
    let mut g = Gen::new(false, code, &no_globals, imports, resume);
    g.future = future;
    g.interactive = interactive;
    if has_annotation(body) {
        // Fica na linha do primeiro comando, que é onde o CPython a põe.
        let loc = g.loc(&body.first()?.pos);
        g.add(SETUP_ANNOTATIONS, 0, loc);
    }
    // O modo interativo não passa por `compiler_body`: não há docstring, a primeira expressão também é impressa.
    let rest = match crate::compile::docstring(body).filter(|_| !interactive) {
        Some(doc) => {
            let loc = g.loc(&body.first()?.pos);
            let i = g.add_const(Key::Str(doc.clone()), Value::str(doc));
            g.add(LOAD_CONST, i as i64, loc);
            g.store_name("__doc__", loc).ok()?;
            body.get(1..).unwrap_or(&[])
        }
        None => body,
    };
    g.stmts(rest).ok()?;
    g.implicit_return();
    g.finish(1, 0)
}

/// O bytecode de `compile(expr, ..., 'eval')`; `e` leva as posições do texto da expressão.
pub fn expression(code: &Code, e: &Expr) -> Option<Emitted> {
    let no_globals = HashSet::new();
    let no_imports = HashSet::new();
    let resume = Loc { line: 0, end_line: 1, col: 0, end_col: 0 };
    let mut g = Gen::new(false, code, &no_globals, &no_imports, resume);
    g.expr(e).ok()?;
    g.add(RETURN_VALUE, 0, NO_LOC);
    g.finish(1, 0)
}
