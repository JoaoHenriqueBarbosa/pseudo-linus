//! `fts5_buffer.c`: buffers de bytes crescentes, listas de posições (poslists), o conjunto de
//! termos da verificação de integridade e utilitários de texto do FTS5.
//!
//! Desvios do C, decorrentes do modelo v2:
//!
//! * `Fts5Buffer` é um `Vec<u8>` (`n` é `p.len()`, `nSpace` é a capacidade). Falta de memória
//!   aborta em Rust, então as funções que só falham por `SQLITE_NOMEM` perdem o parâmetro `pRc`
//!   (`sqlite3Fts5BufferAppendBlob(&rc, ...)` vira `buf.append_blob(...)`). Quem acrescentava o
//!   NUL depois do `n` (`BufferAppendString`) não precisa: o texto é uma fatia, e o
//!   `BufferAppendString` é só `append_blob`.
//! * `sqlite3Fts5BufferSize`, `fts5BufferGrow`, `sqlite3Fts5MallocZero` e `sqlite3Fts5Strndup`
//!   somem (o `Vec` cresce sozinho; `Strndup` é `to_vec`).
//! * `sqlite3Fts5Put32`/`sqlite3Fts5Get32` são `util::put4byte(p, v as u32)` e
//!   `util::get4byte(p) as i32` (não se repetem aqui).
//! * As funções com `pRc` que preservam um erro anterior (`sqlite3Fts5Mprintf`,
//!   `sqlite3Fts5BufferAppendPrintf`) mantêm o `&mut i32`: com `*rc != SQLITE_OK` não fazem nada.

use crate::consts::{SQLITE_NOMEM, SQLITE_OK};
use crate::printf::{mprintf, PrintfArg};

use super::varint::{fts5_append_varint, fts5_fast_get_varint32};

/// `FTS5_POS2COLUMN(iPos)`: a coluna de uma posição `(iCol<<32) + iPos`.
#[inline]
pub fn fts5_pos2column(i_pos: i64) -> i32 {
    (i_pos >> 32) as i32
}

/// `FTS5_POS2OFFSET(iPos)`: o deslocamento (em tokens) dentro da coluna.
#[inline]
pub fn fts5_pos2offset(i_pos: i64) -> i32 {
    (i_pos & 0x7FFF_FFFF) as i32
}

/// `Fts5Buffer`: buffer para a construção incremental de dados (`p` e `n` do C são o `Vec`).
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Fts5Buffer {
    /// Os bytes acumulados; `p.len()` é o `n` do C.
    pub p: Vec<u8>,
}

impl Fts5Buffer {
    /// Buffer vazio (`Fts5Buffer buf = {0, 0, 0}`).
    pub fn new() -> Fts5Buffer {
        Fts5Buffer { p: Vec::new() }
    }

    /// O `pBuf->n` do C.
    #[inline]
    pub fn n(&self) -> i32 {
        self.p.len() as i32
    }

    /// `sqlite3Fts5BufferAppendVarint`: acrescenta `i_val` como varint do SQLite.
    pub fn append_varint(&mut self, i_val: i64) {
        fts5_append_varint(&mut self.p, i_val as u64);
    }

    /// `sqlite3Fts5BufferAppendBlob`: acrescenta `data`.
    pub fn append_blob(&mut self, data: &[u8]) {
        self.p.extend_from_slice(data);
    }

    /// `sqlite3Fts5BufferFree`: libera a memória e zera o buffer.
    pub fn free(&mut self) {
        self.p = Vec::new();
    }

    /// `sqlite3Fts5BufferZero`: esvazia o conteúdo mantendo a memória reservada.
    pub fn zero(&mut self) {
        self.p.clear();
    }

    /// `sqlite3Fts5BufferSet`: o buffer passa a conter só `data`.
    pub fn set(&mut self, data: &[u8]) {
        self.p.clear();
        self.p.extend_from_slice(data);
    }

    /// `sqlite3Fts5BufferAppendPrintf`: formata `fmt` (formato do `sqlite3_mprintf`) e acrescenta
    /// o resultado. Sem efeito se `*rc` já é um erro; falha de formatação vira `SQLITE_NOMEM`.
    pub fn append_printf(&mut self, rc: &mut i32, fmt: &[u8], args: &[PrintfArg]) {
        if *rc == SQLITE_OK {
            match mprintf(fmt, args) {
                None => *rc = SQLITE_NOMEM,
                Some(z) => self.append_blob(&z),
            }
        }
    }
}

/// `sqlite3Fts5Mprintf`: `sqlite3_mprintf` que respeita um erro anterior. Com `*rc != SQLITE_OK`
/// devolve `None` sem formatar; se a formatação falha, grava `SQLITE_NOMEM` em `*rc`.
pub fn fts5_mprintf(rc: &mut i32, fmt: &[u8], args: &[PrintfArg]) -> Option<Vec<u8>> {
    if *rc != SQLITE_OK {
        return None;
    }
    let ret = mprintf(fmt, args);
    if ret.is_none() {
        *rc = SQLITE_NOMEM;
    }
    ret
}

/// `sqlite3Fts5PoslistNext64`: avança por uma lista de posições `a` (todo o `a` é a lista, `n`
/// do C é `a.len()`). `*pi` é o deslocamento em `a` e `*pi_off` a posição corrente
/// `(iCol<<32)+iPos`. Devolve 1 no fim da lista (com `*pi_off = -1`) e 0 se leu uma posição.
pub fn fts5_poslist_next64(a: &[u8], pi: &mut i32, pi_off: &mut i64) -> i32 {
    let mut i = *pi as usize;
    if i >= a.len() {
        /* EOF */
        *pi_off = -1;
        return 1;
    }
    let i_off = *pi_off;
    let mut i_val = fts5_fast_get_varint32(a, &mut i);
    if i_val <= 1 {
        if i_val == 0 {
            *pi = i as i32;
            return 0;
        }
        i_val = fts5_fast_get_varint32(a, &mut i);
        let i_off = (i_val as i64) << 32;
        debug_assert!(i_off >= 0);
        i_val = fts5_fast_get_varint32(a, &mut i);
        if i_val < 2 {
            /* Registro corrompido: para de ler aqui. */
            *pi_off = -1;
            return 1;
        }
        *pi_off = i_off + (i_val.wrapping_sub(2) & 0x7FFF_FFFF) as i64;
    } else {
        *pi_off = (i_off & (0x7FFF_FFFFi64 << 32))
            + ((i_off + i_val.wrapping_sub(2) as i64) & 0x7FFF_FFFF);
    }
    *pi = i as i32;
    0
}

/// `Fts5PoslistReader`: iterador sobre uma lista de posições.
#[derive(Debug, Clone)]
pub struct Fts5PoslistReader<'a> {
    /// A lista de posições (`a`/`n` do C).
    pub a: &'a [u8],
    /// Deslocamento corrente em `a`.
    pub i: i32,
    /// Uso livre do cliente.
    pub b_flag: u8,
    /// Verdadeiro no fim.
    pub b_eof: u8,
    /// `(iCol<<32) + iPos`.
    pub i_pos: i64,
}

impl<'a> Fts5PoslistReader<'a> {
    /// `sqlite3Fts5PoslistReaderInit`: inicia o iterador sobre `a` e lê a primeira posição. O
    /// `b_eof` do resultado é o retorno do C.
    pub fn init(a: &'a [u8]) -> Fts5PoslistReader<'a> {
        let mut it = Fts5PoslistReader { a, i: 0, b_flag: 0, b_eof: 0, i_pos: 0 };
        it.next();
        it
    }

    /// `sqlite3Fts5PoslistReaderNext`: avança; devolve verdadeiro se chegou ao fim.
    pub fn next(&mut self) -> bool {
        if fts5_poslist_next64(self.a, &mut self.i, &mut self.i_pos) != 0 {
            self.b_eof = 1;
        }
        self.b_eof != 0
    }
}

/// `sqlite3Fts5PoslistSafeAppend`: acrescenta a posição `i_pos` à lista em `buf`. `*pi_prev` é a
/// posição anterior escrita e vira `i_pos`. Posição menor que a anterior é ignorada.
pub fn fts5_poslist_safe_append(buf: &mut Fts5Buffer, pi_prev: &mut i64, i_pos: i64) {
    if i_pos >= *pi_prev {
        const COLMASK: i64 = 0x7FFF_FFFFi64 << 32;
        if (i_pos & COLMASK) != (*pi_prev & COLMASK) {
            buf.p.push(1);
            fts5_append_varint(&mut buf.p, (i_pos >> 32) as u64);
            *pi_prev = i_pos & COLMASK;
        }
        fts5_append_varint(&mut buf.p, (i_pos - *pi_prev + 2) as u64);
        *pi_prev = i_pos;
    }
}

/// `Fts5PoslistWriter`: o estado do escritor de listas de posições.
#[derive(Debug, Clone, Copy, Default)]
pub struct Fts5PoslistWriter {
    /// A posição anterior.
    pub i_prev: i64,
}

impl Fts5PoslistWriter {
    /// `sqlite3Fts5PoslistWriterAppend`: acrescenta `i_pos` a `buf`. Sempre `SQLITE_OK`.
    pub fn append(&mut self, buf: &mut Fts5Buffer, i_pos: i64) -> i32 {
        fts5_poslist_safe_append(buf, &mut self.i_prev, i_pos);
        SQLITE_OK
    }
}

/// `sqlite3Fts5IsBareword`: verdadeiro se `t` pode fazer parte de uma palavra nua do FTS5:
/// todo caractere não ASCII, as 52 letras, os 10 dígitos, o sublinhado e o caractere de
/// substituição unicode (0x1A).
pub fn fts5_is_bareword(t: u8) -> bool {
    #[rustfmt::skip]
    static A_BAREWORD: [u8; 128] = [
        0, 0, 0, 0, 0, 0, 0, 0,    0, 0, 0, 0, 0, 0, 0, 0,   /* 0x00 .. 0x0F */
        0, 0, 0, 0, 0, 0, 0, 0,    0, 0, 1, 0, 0, 0, 0, 0,   /* 0x10 .. 0x1F */
        0, 0, 0, 0, 0, 0, 0, 0,    0, 0, 0, 0, 0, 0, 0, 0,   /* 0x20 .. 0x2F */
        1, 1, 1, 1, 1, 1, 1, 1,    1, 1, 0, 0, 0, 0, 0, 0,   /* 0x30 .. 0x3F */
        0, 1, 1, 1, 1, 1, 1, 1,    1, 1, 1, 1, 1, 1, 1, 1,   /* 0x40 .. 0x4F */
        1, 1, 1, 1, 1, 1, 1, 1,    1, 1, 1, 0, 0, 0, 0, 1,   /* 0x50 .. 0x5F */
        0, 1, 1, 1, 1, 1, 1, 1,    1, 1, 1, 1, 1, 1, 1, 1,   /* 0x60 .. 0x6F */
        1, 1, 1, 1, 1, 1, 1, 1,    1, 1, 1, 0, 0, 0, 0, 0,   /* 0x70 .. 0x7F */
    ];
    (t & 0x80) != 0 || A_BAREWORD[t as usize] != 0
}

/// `Fts5Termset`: o balde de termos da verificação de integridade no modo `offsets=0`.
/// Cada balde guarda `(iIdx, termo)`; só a pertinência importa, não a ordem dentro do balde.
#[derive(Debug)]
pub struct Fts5Termset {
    a_hash: Vec<Vec<(i32, Vec<u8>)>>,
}

impl Fts5Termset {
    /// `sqlite3Fts5TermsetNew`.
    pub fn new() -> Fts5Termset {
        Fts5Termset { a_hash: vec![Vec::new(); 512] }
    }

    /// `sqlite3Fts5TermsetAdd`: acrescenta `(i_idx, term)` e devolve se já estava presente.
    pub fn add(&mut self, i_idx: i32, term: &[u8]) -> bool {
        /* O mesmo hash do fts5_hash.c: não importa para a correção, mas faz os testes de colisão
        ** de hash realmente colidirem. O `char` do C tem sinal, então o byte estende o sinal. */
        let mut hash: u32 = 13;
        for &b in term.iter().rev() {
            hash = (hash << 3) ^ hash ^ (b as i8 as i32 as u32);
        }
        hash = (hash << 3) ^ hash ^ (i_idx as u32);
        let slot = (hash % self.a_hash.len() as u32) as usize;
        let bucket = &mut self.a_hash[slot];
        if bucket.iter().any(|(i, t)| *i == i_idx && t.as_slice() == term) {
            return true;
        }
        bucket.insert(0, (i_idx, term.to_vec()));
        false
    }
}

impl Default for Fts5Termset {
    fn default() -> Self {
        Fts5Termset::new()
    }
}
