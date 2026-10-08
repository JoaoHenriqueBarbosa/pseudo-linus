//! A célula de valor do VDBE (`Mem`, o `sqlite3_value` do C) e tudo do `vdbemem.c` que não
//! depende da conexão, do pager nem do `Vdbe` (modelo v2, ver CONVENTIONS.md, item 8).
//!
//! Convenções deste módulo:
//!
//! - `Mem.z` é sempre um `Vec<u8>` POSSUÍDO. As flags `MEM_DYN`, `MEM_STATIC` e `MEM_EPHEM`
//!   continuam existindo em `flags` porque o C as testa para decidir (por exemplo
//!   `sqlite3VdbeMemMakeWriteable` só acrescenta o terminador se `z` não é o `zMalloc`), mas
//!   deixam de ser ponteiros para memória alheia: o texto de uma célula `EPHEM` ou `STATIC` é uma
//!   CÓPIA. Os destrutores (`xDel`) viram `Drop`.
//! - `sz_malloc > 0` e nenhuma das três flags acima significa "`z` é o `zMalloc` desta célula"
//!   (buffer gerenciado). Nesse caso `z.len() == sz_malloc as usize`. Fora disso `sz_malloc == 0`
//!   e `z.len()` é o tamanho exato do conteúdo (mais o terminador, se houver). O tamanho de
//!   alocação imita o `sqlite3DbMallocSize` do Debian (`printf::malloc_size`).
//! - A `union MemValue` do C vira campos separados (`u_i`, `u_r`, `n_zero`). Quem lê os bits de um
//!   real (o `OP_MakeRecord`, por exemplo) usa `u_r.to_bits()` explícito, nunca `u_i`.
//! - O `db` da célula some: onde o C consulta `db->aLimit[SQLITE_LIMIT_LENGTH]` entra um parâmetro
//!   `limit: i32`. Onde o C devolve `sqlite3ErrorToParser(db, SQLITE_TOOBIG)` a função devolve só
//!   `SQLITE_TOOBIG` e quem tem a conexão grava o erro no `Parse`.
//! - Texto e blob são bytes. Leituras além do fim de `z` não existem: use [`Mem::bytes`].
//!
//! ADIADAS (precisam de `Connection`, `Context`, `Vdbe`, `FuncDef`, `Expr` ou `Parse`):
//! `sqlite3VdbeMemFinalize`, `sqlite3VdbeMemAggValue`, `sqlite3VdbeMemSetPointer` (valores
//! ponteiro), `sqlite3ValueFromExpr` (precisa de `Expr`), `sqlite3ValueIsOfClass` (destrutores),
//! `sqlite3Stat4*` (`SQLITE_ENABLE_STAT4` está desligado no Debian). Funções só de `SQLITE_DEBUG`
//! (`sqlite3VdbeCheckMemInvariants`, `sqlite3VdbeMemValidStrRep`, `sqlite3VdbeMemAboutToChange`,
//! `sqlite3VdbeMemPrettyPrint`, `sqlite3VdbeMemIsRowSet`) não existem; `Mem::is_row_set` cobre o
//! único uso fora de `assert`. `sqlite3ValueSetStr`, `sqlite3ValueFree` e `sqlite3MemSetArrayInt64`
//! são repasses de uma linha e o chamador usa o alvo direto.
//!
//! Do `vdbe.c` vêm `applyAffinity`, `applyNumericAffinity`, `alsoAnInt`, `computeNumericType` e
//! `numericType` (o `sqlite3VdbeMemCast` precisa de `applyAffinity`), do `utf.c` vêm
//! `sqlite3VdbeMemTranslate` e `sqlite3VdbeMemHandleBom` (são operações sobre `Mem`) e do `main.c`
//! vêm as três colações embutidas (`binCollFunc`, `nocaseCollatingFunc`, `rtrimCollFunc`), de que
//! o `CollSeq` abaixo precisa para comparar.

use std::any::Any;
use std::borrow::Cow;
use std::rc::Rc;

use crate::consts::{
    LARGEST_INT64, MEM_AFFMASK, MEM_AGG, MEM_BLOB, MEM_DYN, MEM_EPHEM, MEM_INT, MEM_INTREAL,
    MEM_NULL, MEM_REAL, MEM_STATIC, MEM_STR, MEM_SUBTYPE, MEM_TERM, MEM_TYPEMASK, MEM_ZERO,
    SMALLEST_INT64, SQLITE_AFF_BLOB, SQLITE_AFF_INTEGER, SQLITE_AFF_NUMERIC, SQLITE_AFF_REAL,
    SQLITE_AFF_TEXT, SQLITE_BLOB, SQLITE_FLOAT, SQLITE_INTEGER, SQLITE_NOMEM, SQLITE_NOMEM_BKPT,
    SQLITE_NULL, SQLITE_OK, SQLITE_TEXT, SQLITE_TOOBIG, SQLITE_UTF16BE, SQLITE_UTF16LE,
    SQLITE_UTF16_ALIGNED, SQLITE_UTF8,
};
use crate::printf::{malloc_size, mprintf, PrintfArg};
use crate::rowset::{row_set_delete, row_set_init, RowSet};
use crate::utf::translate_bytes;
use crate::util::{at, atof, atoi64, int64_to_text, is_nan, strnicmp};

/// `SQLITE_UTF8` como `u8` (o campo `Mem.enc` do C é `u8`).
pub(crate) const ENC_UTF8: u8 = SQLITE_UTF8 as u8;
/// `SQLITE_UTF16LE` como `u8`.
pub(crate) const ENC_UTF16LE: u8 = SQLITE_UTF16LE as u8;
/// `SQLITE_UTF16BE` como `u8`.
pub(crate) const ENC_UTF16BE: u8 = SQLITE_UTF16BE as u8;
/// `SQLITE_UTF16_ALIGNED` como `u8`.
const ENC_ALIGNED: u8 = SQLITE_UTF16_ALIGNED as u8;

/// `sqlite3Config.bUseLongDouble`: verdadeiro no x86-64 do Debian (o `long double` de 80 bits).
pub(crate) const USE_LONG_DOUBLE: bool = true;

// ---------------------------------------------------------------------------------------------
// Colações, KeyInfo e UnpackedRecord (sqliteInt.h)
// ---------------------------------------------------------------------------------------------

/// A função de comparação de uma colação (`CollSeq.xCmp`).
///
/// As três embutidas são as do `main.c`. `User` guarda a comparação de uma colação criada pelo
/// usuário (`sqlite3_create_collation`): o `pUser` do C vira a captura do closure. O closure só
/// vê os bytes das duas strings (na codificação `CollSeq.enc`) e nunca toca a conexão, como o
/// `xCmp` do C. O `Rc` existe porque o `CollSeq` é compartilhado e imutável entre a tabela de
/// colações da conexão e todos os `KeyInfo` que o apontam.
#[derive(Clone)]
pub enum CollFn {
    /// `binCollFunc`: `memcmp` e depois o tamanho.
    Binary,
    /// `nocaseCollatingFunc`: `sqlite3StrNICmp` e depois o tamanho.
    NoCase,
    /// `rtrimCollFunc`: ignora espaços finais e usa a binária.
    RTrim,
    /// Colação do usuário, ligada pela conexão (`sqlite3_create_collation`).
    User(Rc<dyn Fn(&[u8], &[u8]) -> i32>),
}

/// `memcmp` do glibc em x86-64: a diferença do primeiro par de bytes distintos entre os `n`
/// primeiros bytes (`n = min(a.len(), b.len())` dos dois argumentos), ou zero.
pub(crate) fn memcmp(a: &[u8], b: &[u8]) -> i32 {
    let n = a.len().min(b.len());
    for i in 0..n {
        if a[i] != b[i] {
            return a[i] as i32 - b[i] as i32;
        }
    }
    0
}

/// `binCollFunc` do `main.c`.
fn bin_coll_func(z1: &[u8], z2: &[u8]) -> i32 {
    let n1 = z1.len() as i32;
    let n2 = z2.len() as i32;
    let n = z1.len().min(z2.len());
    let rc = memcmp(&z1[..n], &z2[..n]);
    if rc == 0 {
        n1 - n2
    } else {
        rc
    }
}

/// `rtrimCollFunc` do `main.c`.
fn rtrim_coll_func(z1: &[u8], z2: &[u8]) -> i32 {
    let mut n1 = z1.len();
    let mut n2 = z2.len();
    while n1 != 0 && z1[n1 - 1] == b' ' {
        n1 -= 1;
    }
    while n2 != 0 && z2[n2 - 1] == b' ' {
        n2 -= 1;
    }
    bin_coll_func(&z1[..n1], &z2[..n2])
}

/// `nocaseCollatingFunc` do `main.c`.
fn nocase_collating_func(z1: &[u8], z2: &[u8]) -> i32 {
    let n1 = z1.len() as i32;
    let n2 = z2.len() as i32;
    let r = strnicmp(Some(z1), Some(z2), n1.min(n2));
    if r == 0 {
        n1 - n2
    } else {
        r
    }
}

impl CollFn {
    /// Chama o `xCmp` do C: compara os `n1` bytes de `z1` com os `n2` bytes de `z2`.
    pub fn call(&self, z1: &[u8], z2: &[u8]) -> i32 {
        match self {
            CollFn::Binary => bin_coll_func(z1, z2),
            CollFn::NoCase => nocase_collating_func(z1, z2),
            CollFn::RTrim => rtrim_coll_func(z1, z2),
            CollFn::User(f) => f(z1, z2),
        }
    }
}

/// `CollSeq` do sqliteInt.h: uma colação numa codificação. Compartilhada (`Rc<CollSeq>`) entre a
/// tabela de colações da conexão e os `KeyInfo`.
#[derive(Clone)]
pub struct CollSeq {
    /// Nome da colação, em UTF-8 (`zName`).
    pub name: Vec<u8>,
    /// Codificação de texto que `x_cmp` entende.
    pub enc: u8,
    /// A comparação (`xCmp` mais `pUser`).
    pub x_cmp: CollFn,
}

/// `KeyInfo` do sqliteInt.h: como comparar as colunas de um índice ou de uma chave de ordenação.
///
/// Fica imutável depois de montado e é compartilhado (cursor, `P4_KEYINFO`, `UnpackedRecord`,
/// ordenador), por isso o `Rc<KeyInfo>` em quem o referencia. `nRef` e `db` do C não existem:
/// a contagem é a do `Rc` e o `db` só servia ao `mallocFailed` e ao `ENC(db)` (que é `enc`).
#[derive(Clone)]
pub struct KeyInfo {
    /// Codificação de texto: um dos `SQLITE_UTF*`.
    pub enc: u8,
    /// Número de colunas-chave do índice.
    pub n_key_field: u16,
    /// Total de colunas, as chaves mais as outras.
    pub n_all_field: u16,
    /// Ordem de cada coluna (`KEYINFO_ORDER_*`).
    pub a_sort_flags: Vec<u8>,
    /// Colação de cada termo da chave; `None` é o ponteiro nulo do C (BINARY implícita).
    pub a_coll: Vec<Option<Rc<CollSeq>>>,
}

/// `UnpackedRecord` do sqliteInt.h: um registro já separado em campos, para comparação.
///
/// `u.z` (o cache de `aMem[0].z` do `vdbeRecordCompareString`) não existe: o comparador lê
/// `a_mem[0].z` direto. `u.i` vira `u_i`.
pub struct UnpackedRecord {
    /// Colação e ordem de cada campo.
    pub p_key_info: Rc<KeyInfo>,
    /// Os valores.
    pub a_mem: Vec<Mem>,
    /// Cache de `a_mem[0].u_i` para `vdbeRecordCompareInt` (`u.i`).
    pub u_i: i64,
    /// Cache de `a_mem[0].n` para `vdbeRecordCompareString`.
    pub n: i32,
    /// Número de entradas de `a_mem` em uso.
    pub n_field: u16,
    /// Resultado da comparação se as chaves forem iguais.
    pub default_rc: i8,
    /// Erro detectado pelo comparador (`SQLITE_CORRUPT` ou `SQLITE_NOMEM`).
    pub err_code: u8,
    /// Valor a devolver se (lhs < rhs).
    pub r1: i8,
    /// Valor a devolver se (lhs > rhs).
    pub r2: i8,
    /// Verdadeiro se uma comparação de igualdade já foi vista.
    pub eq_seen: u8,
}

// ---------------------------------------------------------------------------------------------
// A célula
// ---------------------------------------------------------------------------------------------

/// `Mem` do vdbeInt.h (o `sqlite3_value`). Uma célula guarda um valor SQL com várias
/// representações possíveis, indicadas por `flags` (`MEM_*`).
///
/// O acumulador de agregado (`MEM_Agg`) e o `RowSet` (`MEM_Blob|MEM_Dyn`) não são clonáveis:
/// o C afirma que nunca se copia uma célula assim (`assert(!sqlite3VdbeMemIsRowSet(pFrom))`) e o
/// `Clone` os deixa em `None`.
#[derive(Default)]
pub struct Mem {
    /// Combinação de `MEM_*`.
    pub flags: u16,
    /// `SQLITE_UTF8`, `SQLITE_UTF16BE` ou `SQLITE_UTF16LE`.
    pub enc: u8,
    /// Subtipo do valor (válido com `MEM_SUBTYPE`).
    pub e_subtype: u8,
    /// Número de bytes da string ou do blob, sem o terminador.
    pub n: i32,
    /// Inteiro (`u.i`), com `MEM_INT` ou `MEM_INTREAL`.
    pub u_i: i64,
    /// Real (`u.r`), com `MEM_REAL`.
    pub u_r: f64,
    /// Zeros acrescentados ao blob (`u.nZero`), com `MEM_ZERO`.
    pub n_zero: i32,
    /// Conteúdo de string ou blob (`z`).
    pub z: Vec<u8>,
    /// Tamanho do buffer gerenciado (`szMalloc`); 0 se `z` não é gerenciado.
    pub sz_malloc: i32,
    /// Armazenamento transitório do `serial_type` no `OP_MakeRecord` (`uTemp`).
    pub u_temp: u32,
    /// Contexto do agregado, com `MEM_AGG` (o que `sqlite3_aggregate_context` entrega).
    pub agg: Option<Box<dyn Any>>,
    /// O conjunto de rowids de um `OP_RowSetAdd`, com `MEM_BLOB|MEM_DYN`.
    pub row_set: Option<Box<RowSet>>,
}

impl Clone for Mem {
    /// Cópia profunda do valor. O acumulador de agregado e o `RowSet` não são copiados.
    fn clone(&self) -> Mem {
        debug_assert!(self.agg.is_none() && self.row_set.is_none());
        Mem {
            flags: self.flags,
            enc: self.enc,
            e_subtype: self.e_subtype,
            n: self.n,
            u_i: self.u_i,
            u_r: self.u_r,
            n_zero: self.n_zero,
            z: self.z.clone(),
            sz_malloc: self.sz_malloc,
            u_temp: self.u_temp,
            agg: None,
            row_set: None,
        }
    }
}

impl Mem {
    /// `sqlite3VdbeMemInit`: uma célula nova com as flags dadas e o resto zerado.
    pub fn init(flags: u16) -> Mem {
        debug_assert!(flags & !MEM_TYPEMASK == 0);
        Mem { flags, ..Mem::default() }
    }

    /// `sqlite3ValueNew`: um valor novo, `NULL`.
    pub fn value_new() -> Mem {
        Mem { flags: MEM_NULL, ..Mem::default() }
    }

    /// `VdbeMemDynamic(X)`: verdadeiro se a célula guarda conteúdo que precisa ser liberado
    /// por um destrutor (`MEM_Agg` ou `MEM_Dyn`).
    #[inline]
    pub fn is_dynamic(&self) -> bool {
        self.flags & (MEM_AGG | MEM_DYN) != 0
    }

    /// `MemSetTypeFlag(p, f)`: troca as flags de tipo por `f`.
    #[inline]
    pub fn set_type_flag(&mut self, f: u16) {
        self.flags = (self.flags & !(MEM_TYPEMASK | MEM_ZERO)) | f;
    }

    /// `MemNullNochng(X)`: o `NULL` de "sem mudança" de `xUpdate`.
    #[inline]
    pub fn is_null_nochng(&self) -> bool {
        (self.flags & MEM_TYPEMASK) == (MEM_NULL | MEM_ZERO) && self.n == 0 && self.n_zero == 0
    }

    /// Os `n` bytes de conteúdo de `z` (sem o terminador). Nunca passa do fim do buffer.
    #[inline]
    pub fn bytes(&self) -> &[u8] {
        let n = (self.n.max(0) as usize).min(self.z.len());
        &self.z[..n]
    }

    /// `sqlite3VdbeMemIsRowSet` (só existe no C sob `SQLITE_DEBUG`, mas o `vdbe.c` testa em
    /// código de produção via asserts de `OP_RowSet*`).
    #[inline]
    pub fn is_row_set(&self) -> bool {
        (self.flags & (MEM_BLOB | MEM_DYN)) == (MEM_BLOB | MEM_DYN) && self.row_set.is_some()
    }
}

/// Faz de `v` o buffer gerenciado da célula (`z = zMalloc = v; szMalloc = DbMallocSize(z)`).
fn adopt_z(p: &mut Mem, mut v: Vec<u8>) {
    let sz = malloc_size(v.len() as u64) as usize;
    if v.len() < sz {
        v.resize(sz, 0);
    }
    p.sz_malloc = sz as i32;
    p.z = v;
}

/// `vdbeMemRenderNum`: escreve em `z_buf` (`sz` bytes, seguido de NUL) a forma textual de uma
/// célula `MEM_Int`, `MEM_Real` ou `MEM_IntReal` e grava o tamanho em `p.n`.
fn mem_render_num(p: &mut Mem, sz: usize, z_buf: &mut [u8]) {
    debug_assert!(p.flags & (MEM_INT | MEM_REAL | MEM_INTREAL) != 0);
    debug_assert!(sz > 22);
    if p.flags & MEM_INT != 0 {
        p.n = int64_to_text(p.u_i, z_buf);
    } else {
        let r = if p.flags & MEM_INTREAL != 0 { p.u_i as f64 } else { p.u_r };
        let t = mprintf(b"%!.15g", &[PrintfArg::Double(r)]).unwrap_or_default();
        let n = t.len().min(sz - 1);
        z_buf[..n].copy_from_slice(&t[..n]);
        z_buf[n] = 0;
        p.n = n as i32;
    }
}

/// `sqlite3VdbeChangeEncoding`: garante que a representação em string tenha a codificação
/// `desired_enc`. Sem string só troca `enc`. Devolve `SQLITE_OK` ou `SQLITE_NOMEM`.
pub fn vdbe_change_encoding(p: &mut Mem, desired_enc: i32) -> i32 {
    debug_assert!(!p.is_row_set());
    debug_assert!(
        desired_enc == SQLITE_UTF8 || desired_enc == SQLITE_UTF16LE || desired_enc == SQLITE_UTF16BE
    );
    if p.flags & MEM_STR == 0 {
        p.enc = desired_enc as u8;
        return SQLITE_OK;
    }
    if p.enc as i32 == desired_enc {
        return SQLITE_OK;
    }
    mem_translate(p, desired_enc as u8)
}

/// `sqlite3VdbeMemTranslate` (utf.c): converte a string da célula para `desired_enc`.
/// O trabalho de conversão é de [`translate_bytes`]; aqui ficam as flags, o terminador e a
/// alocação. Devolve `SQLITE_OK` ou `SQLITE_NOMEM`.
pub fn mem_translate(p: &mut Mem, desired_enc: u8) -> i32 {
    debug_assert!(p.flags & MEM_STR != 0);
    debug_assert!(p.enc != desired_enc);
    debug_assert!(p.enc != 0);
    debug_assert!(p.n >= 0);

    // Entre UTF-16 LE e BE só se trocam os bytes dos pares, no próprio lugar.
    if p.enc != ENC_UTF8 && desired_enc != ENC_UTF8 {
        let rc = mem_make_writeable(p);
        if rc != SQLITE_OK {
            debug_assert!(rc == SQLITE_NOMEM);
            return SQLITE_NOMEM_BKPT;
        }
        let out = translate_bytes(p.bytes(), p.enc, desired_enc);
        p.z[..out.len()].copy_from_slice(&out);
        p.enc = desired_enc;
        return SQLITE_OK;
    }

    // Maior tamanho possível da saída, mais o terminador (1 byte em UTF-8, 2 em UTF-16).
    let len: usize;
    if desired_enc == ENC_UTF8 {
        p.n &= !1;
        len = 2 * p.n as usize + 1;
    } else {
        len = 2 * p.n as usize + 2;
    }
    let term = if desired_enc == ENC_UTF8 { 1 } else { 2 };
    let mut out = translate_bytes(p.bytes(), p.enc, desired_enc);
    let new_n = out.len();
    out.resize(len.max(new_n + term), 0);

    let c = MEM_STR | MEM_TERM | (p.flags & (MEM_AFFMASK | MEM_SUBTYPE));
    p.n = new_n as i32;
    mem_release(p);
    p.flags = c;
    p.enc = desired_enc;
    adopt_z(p, out);
    SQLITE_OK
}

/// `sqlite3VdbeMemHandleBom` (utf.c): se a string UTF-16 começa por uma marca de ordem de
/// bytes, remove-a e ajusta `enc`. Não troca bytes de lugar.
pub fn mem_handle_bom(p: &mut Mem) -> i32 {
    let mut rc = SQLITE_OK;
    let mut bom: u8 = 0;
    debug_assert!(p.n >= 0);
    if p.n > 1 {
        let b1 = at(&p.z, 0);
        let b2 = at(&p.z, 1);
        if b1 == 0xFE && b2 == 0xFF {
            bom = ENC_UTF16BE;
        }
        if b1 == 0xFF && b2 == 0xFE {
            bom = ENC_UTF16LE;
        }
    }
    if bom != 0 {
        rc = mem_make_writeable(p);
        if rc == SQLITE_OK {
            p.n -= 2;
            let n = p.n as usize;
            p.z.copy_within(2..2 + n, 0);
            p.z[n] = 0;
            p.z[n + 1] = 0;
            p.flags |= MEM_TERM;
            p.enc = bom;
        }
    }
    rc
}

/// `sqlite3VdbeMemGrow`: garante um buffer gerenciado de ao menos `n` bytes. Com `preserve`,
/// o conteúdo atual (`p.n` bytes, ou o buffer inteiro se já for gerenciado) é mantido; a
/// célula precisa ser string ou blob. Devolve `SQLITE_OK` ou `SQLITE_NOMEM`.
pub fn mem_grow(p: &mut Mem, n: i32, preserve: bool) -> i32 {
    debug_assert!(!p.is_row_set());
    debug_assert!(!preserve || p.flags & (MEM_BLOB | MEM_STR) != 0);
    let n = n.max(0) as usize;
    let sz = malloc_size(n as u64) as usize;
    // `szMalloc>0 && z==zMalloc`: o buffer atual é o gerenciado.
    let managed = p.sz_malloc > 0 && p.flags & (MEM_DYN | MEM_EPHEM | MEM_STATIC) == 0;

    let mut v: Vec<u8> = if preserve {
        // Realloc (gerenciado, guarda tudo) ou novo buffer com cópia de `p.n` bytes.
        let mut old = std::mem::take(&mut p.z);
        if !managed {
            old.truncate(p.n.max(0) as usize);
        }
        old
    } else {
        p.z = Vec::new();
        Vec::new()
    };
    if sz > v.len() && v.try_reserve_exact(sz - v.len()).is_err() {
        mem_set_null(p);
        p.z = Vec::new();
        p.sz_malloc = 0;
        return SQLITE_NOMEM_BKPT;
    }
    v.resize(sz, 0);

    p.z = v;
    p.sz_malloc = sz as i32;
    p.flags &= !(MEM_DYN | MEM_EPHEM | MEM_STATIC);
    SQLITE_OK
}

/// `sqlite3VdbeMemClearAndResize`: garante um buffer gerenciado de ao menos `sz_new` bytes,
/// podendo descartar string e blob (inteiro, real e `NULL` são preservados).
pub fn mem_clear_and_resize(p: &mut Mem, sz_new: i32) -> i32 {
    debug_assert!(sz_new > 0);
    debug_assert!(p.flags & MEM_DYN == 0 || p.sz_malloc == 0);
    if p.sz_malloc < sz_new || p.z.len() < sz_new as usize {
        return mem_grow(p, sz_new, false);
    }
    debug_assert!(p.flags & MEM_DYN == 0);
    p.flags &= MEM_NULL | MEM_INT | MEM_REAL | MEM_INTREAL;
    SQLITE_OK
}

/// `sqlite3VdbeMemZeroTerminateIfAble`: otimização que termina a string com zero se a
/// alocação tem espaço. Células `MEM_Dyn` ficam como estão (o C só as termina conhecendo o
/// `xDel`, que aqui não existe).
pub fn mem_zero_terminate_if_able(p: &mut Mem) {
    if (p.flags & (MEM_STR | MEM_TERM | MEM_EPHEM | MEM_STATIC)) != MEM_STR {
        // Só vale para string que não seja efêmera nem estática.
        return;
    }
    if p.enc != ENC_UTF8 {
        return;
    }
    if p.flags & MEM_DYN != 0 {
        return;
    }
    let n = p.n.max(0) as usize;
    if p.sz_malloc as usize > n && p.z.len() > n {
        p.z[n] = 0;
        p.flags |= MEM_TERM;
    }
}

/// `vdbeMemAddTerminator`: a célula tem uma string sem terminador; acrescenta três zeros (assim
/// há um duplo zero em fronteira par para terminar UTF-16, mesmo com `n` ímpar).
fn mem_add_terminator(p: &mut Mem) -> i32 {
    if mem_grow(p, p.n + 3, true) != SQLITE_OK {
        return SQLITE_NOMEM_BKPT;
    }
    let n = p.n as usize;
    p.z[n] = 0;
    p.z[n + 1] = 0;
    p.z[n + 2] = 0;
    p.flags |= MEM_TERM;
    SQLITE_OK
}

/// `sqlite3VdbeMemMakeWriteable`: move o conteúdo para o buffer gerenciado, onde pode ser
/// escrito. Devolve `SQLITE_OK` ou `SQLITE_NOMEM`.
pub fn mem_make_writeable(p: &mut Mem) -> i32 {
    debug_assert!(!p.is_row_set());
    if p.flags & (MEM_STR | MEM_BLOB) != 0 {
        if p.flags & MEM_ZERO != 0 && mem_expand_blob(p) != SQLITE_OK {
            return SQLITE_NOMEM;
        }
        if p.sz_malloc == 0 || p.flags & (MEM_DYN | MEM_EPHEM | MEM_STATIC) != 0 {
            let rc = mem_add_terminator(p);
            if rc != SQLITE_OK {
                return rc;
            }
        }
    }
    p.flags &= !MEM_EPHEM;
    SQLITE_OK
}

/// `sqlite3VdbeMemExpandBlob`: se o blob tem cauda de zeros (`MEM_Zero`), vira um blob comum.
pub fn mem_expand_blob(p: &mut Mem) -> i32 {
    debug_assert!(p.flags & MEM_ZERO != 0);
    debug_assert!(p.flags & MEM_BLOB != 0 || p.is_null_nochng());
    debug_assert!(!p.is_row_set());

    // Número de bytes do blob expandido.
    let mut n_byte = p.n.wrapping_add(p.n_zero);
    if n_byte <= 0 {
        if p.flags & MEM_BLOB == 0 {
            return SQLITE_OK;
        }
        n_byte = 1;
    }
    if mem_grow(p, n_byte, true) != SQLITE_OK {
        return SQLITE_NOMEM_BKPT;
    }
    let n = p.n as usize;
    let n_zero = p.n_zero.max(0) as usize;
    p.z[n..n + n_zero].fill(0);
    p.n = p.n.wrapping_add(p.n_zero);
    p.flags &= !(MEM_ZERO | MEM_TERM);
    SQLITE_OK
}

/// `sqlite3VdbeMemNulTerminate`: garante que a string termine em `\0`.
pub fn mem_nul_terminate(p: &mut Mem) -> i32 {
    if (p.flags & (MEM_TERM | MEM_STR)) != MEM_STR {
        SQLITE_OK
    } else {
        mem_add_terminator(p)
    }
}

/// `sqlite3VdbeMemStringify`: acrescenta `MEM_Str` a uma célula numérica (nunca `NULL` nem
/// blob), na codificação `enc`. Com `b_force` as representações numéricas são invalidadas.
pub fn mem_stringify(p: &mut Mem, enc: u8, b_force: bool) -> i32 {
    const N_BYTE: usize = 32;
    debug_assert!(p.flags & MEM_ZERO == 0);
    debug_assert!(p.flags & (MEM_STR | MEM_BLOB) == 0);
    debug_assert!(p.flags & (MEM_INT | MEM_REAL | MEM_INTREAL) != 0);
    debug_assert!(!p.is_row_set());

    if mem_clear_and_resize(p, N_BYTE as i32) != SQLITE_OK {
        p.enc = 0;
        return SQLITE_NOMEM_BKPT;
    }

    let mut buf = [0u8; N_BYTE];
    mem_render_num(p, N_BYTE, &mut buf);
    let n = p.n as usize;
    p.z[..=n].copy_from_slice(&buf[..=n]);
    p.enc = ENC_UTF8;
    p.flags |= MEM_STR | MEM_TERM;
    if b_force {
        p.flags &= !(MEM_INT | MEM_REAL | MEM_INTREAL);
    }
    vdbe_change_encoding(p, enc as i32);
    SQLITE_OK
}

/// `vdbeMemClearExternAndSetNull`: libera o que um destrutor liberaria e deixa a célula `NULL`.
///
/// Para `MEM_Agg` o C chama antes `sqlite3VdbeMemFinalize` (o `xFinalize` do usuário, ADIADO:
/// precisa de `FuncDef` e `Context`); a camada do VDBE tem que finalizar o agregado antes de
/// liberar a célula. Aqui o acumulador só é descartado.
fn mem_clear_extern_and_set_null(p: &mut Mem) {
    debug_assert!(p.is_dynamic());
    if p.flags & MEM_AGG != 0 {
        p.agg = None;
    }
    if p.flags & MEM_DYN != 0 {
        if let Some(rs) = p.row_set.take() {
            row_set_delete(*rs);
        }
        p.z = Vec::new();
        p.sz_malloc = 0;
    }
    p.flags = MEM_NULL;
}

/// `vdbeMemClear`.
fn mem_clear(p: &mut Mem) {
    if p.is_dynamic() {
        mem_clear_extern_and_set_null(p);
    }
    p.sz_malloc = 0;
    p.z = Vec::new();
}

/// `sqlite3VdbeMemRelease`: libera tudo o que a célula guarda, o conteúdo externo e o buffer
/// gerenciado.
pub fn mem_release(p: &mut Mem) {
    if p.is_dynamic() || p.sz_malloc != 0 {
        mem_clear(p);
    }
}

/// `sqlite3VdbeMemReleaseMalloc`: como [`mem_release`] quando se sabe que a célula não é
/// `MEM_Dyn` nem `MEM_Agg`.
pub fn mem_release_malloc(p: &mut Mem) {
    debug_assert!(!p.is_dynamic());
    if p.sz_malloc != 0 {
        mem_clear(p);
    }
}

/// `memIntValue`: `sqlite3Atoi64` sobre o texto da célula.
fn mem_int_value(p: &Mem) -> i64 {
    let mut value: i64 = 0;
    atoi64(p.bytes(), &mut value, p.n, p.enc);
    value
}

/// `sqlite3VdbeIntValue`: o melhor inteiro que representa o valor da célula (0 para `NULL`).
pub fn vdbe_int_value(p: &Mem) -> i64 {
    let flags = p.flags;
    if flags & (MEM_INT | MEM_INTREAL) != 0 {
        p.u_i
    } else if flags & MEM_REAL != 0 {
        real_to_i64(p.u_r)
    } else if flags & (MEM_STR | MEM_BLOB) != 0 {
        mem_int_value(p)
    } else {
        0
    }
}

/// `memRealValue`: `sqlite3AtoF` sobre o texto da célula.
fn mem_real_value(p: &Mem) -> f64 {
    atof(p.bytes(), p.n, p.enc, USE_LONG_DOUBLE).1
}

/// `sqlite3VdbeRealValue`: o melhor `double` que representa o valor da célula (0.0 para `NULL`).
pub fn vdbe_real_value(p: &Mem) -> f64 {
    if p.flags & MEM_REAL != 0 {
        p.u_r
    } else if p.flags & (MEM_INT | MEM_INTREAL) != 0 {
        p.u_i as f64
    } else if p.flags & (MEM_STR | MEM_BLOB) != 0 {
        mem_real_value(p)
    } else {
        0.0
    }
}

/// `sqlite3VdbeBooleanValue`: 1 se verdadeiro, 0 se falso, `if_null` se `NULL`.
pub fn vdbe_boolean_value(p: &Mem, if_null: i32) -> i32 {
    if p.flags & (MEM_INT | MEM_INTREAL) != 0 {
        return (p.u_i != 0) as i32;
    }
    if p.flags & MEM_NULL != 0 {
        return if_null;
    }
    (vdbe_real_value(p) != 0.0) as i32
}

/// `sqlite3VdbeIntegerAffinity`: a célula já é `MEM_Real` ou `MEM_IntReal`; vira `MEM_Int`
/// se isso não perder informação.
pub fn vdbe_integer_affinity(p: &mut Mem) {
    debug_assert!(p.flags & (MEM_REAL | MEM_INTREAL) != 0);
    debug_assert!(!p.is_row_set());

    if p.flags & MEM_INTREAL != 0 {
        p.set_type_flag(MEM_INT);
    } else {
        let ix = real_to_i64(p.u_r);

        // Só marca como inteiro se (1) a ida e volta real->int->real não muda nada e (2) o
        // inteiro não é o maior nem o menor possível (ticket #3922).
        if p.u_r == ix as f64 && ix > SMALLEST_INT64 && ix < LARGEST_INT64 {
            p.u_i = ix;
            p.set_type_flag(MEM_INT);
        }
    }
}

/// `sqlite3VdbeMemIntegerify`: converte a célula em inteiro, invalidando o resto.
pub fn mem_integerify(p: &mut Mem) -> i32 {
    debug_assert!(!p.is_row_set());
    p.u_i = vdbe_int_value(p);
    p.set_type_flag(MEM_INT);
    SQLITE_OK
}

/// `sqlite3VdbeMemRealify`: converte a célula em real, invalidando o resto.
pub fn mem_realify(p: &mut Mem) -> i32 {
    p.u_r = vdbe_real_value(p);
    p.set_type_flag(MEM_REAL);
    SQLITE_OK
}

/// `sqlite3RealSameAsInt`: verdadeiro se `r1` e `i` são o mesmo valor dentro da precisão do
/// ponto flutuante. Supõe que `i` veio de uma atribuição a partir de `r1`.
pub fn real_same_as_int(r1: f64, i: i64) -> bool {
    let r2 = i as f64;
    r1 == 0.0
        || (r1.to_bits() == r2.to_bits() && i >= -2251799813685248 && i < 2251799813685248)
}

/// `sqlite3RealToI64`: o inteiro mais próximo de `r`, saturando nos extremos. NaN dá o mesmo
/// que a instrução `cvttsd2si` do x86 (o menor `i64`), que é o que o C produz.
pub fn real_to_i64(r: f64) -> i64 {
    if r < -9223372036854774784.0 {
        return SMALLEST_INT64;
    }
    if r > 9223372036854774784.0 {
        return LARGEST_INT64;
    }
    if r.is_nan() {
        return SMALLEST_INT64;
    }
    r as i64
}

/// `sqlite3VdbeMemNumerify`: converte a célula em `MEM_Real` ou `MEM_Int`, invalidando as
/// outras representações. Converte o quanto der do texto e ignora o resto.
pub fn mem_numerify(p: &mut Mem) -> i32 {
    if p.flags & (MEM_INT | MEM_REAL | MEM_INTREAL | MEM_NULL) == 0 {
        debug_assert!(p.flags & (MEM_BLOB | MEM_STR) != 0);
        let (rc, r) = atof(p.bytes(), p.n, p.enc, USE_LONG_DOUBLE);
        p.u_r = r;
        let mut ix: i64 = 0;
        let as_int = ((rc == 0 || rc == 1) && atoi64(p.bytes(), &mut ix, p.n, p.enc) <= 1) || {
            ix = real_to_i64(p.u_r);
            real_same_as_int(p.u_r, ix)
        };
        if as_int {
            p.u_i = ix;
            p.set_type_flag(MEM_INT);
        } else {
            p.set_type_flag(MEM_REAL);
        }
    }
    debug_assert!(p.flags & (MEM_INT | MEM_REAL | MEM_INTREAL | MEM_NULL) != 0);
    p.flags &= !(MEM_STR | MEM_BLOB | MEM_ZERO);
    SQLITE_OK
}

/// `sqlite3VdbeMemCast`: converte à força o valor para a afinidade `aff` (o `CAST` do SQL).
pub fn mem_cast(p: &mut Mem, aff: u8, encoding: u8) -> i32 {
    if p.flags & MEM_NULL != 0 {
        return SQLITE_OK;
    }
    match aff {
        SQLITE_AFF_BLOB => {
            // Na verdade um cast para BLOB.
            if p.flags & MEM_BLOB == 0 {
                apply_affinity(p, SQLITE_AFF_TEXT, encoding);
                debug_assert!(p.flags & MEM_STR != 0);
                if p.flags & MEM_STR != 0 {
                    p.set_type_flag(MEM_BLOB);
                }
            } else {
                p.flags &= !(MEM_TYPEMASK & !MEM_BLOB);
            }
        }
        SQLITE_AFF_NUMERIC => {
            mem_numerify(p);
        }
        SQLITE_AFF_INTEGER => {
            mem_integerify(p);
        }
        SQLITE_AFF_REAL => {
            mem_realify(p);
        }
        _ => {
            debug_assert!(aff == SQLITE_AFF_TEXT);
            debug_assert!(MEM_STR == (MEM_BLOB >> 3));
            p.flags |= (p.flags & MEM_BLOB) >> 3;
            apply_affinity(p, SQLITE_AFF_TEXT, encoding);
            debug_assert!(p.flags & MEM_STR != 0);
            p.flags &= !(MEM_INT | MEM_REAL | MEM_INTREAL | MEM_BLOB | MEM_ZERO);
            if encoding != ENC_UTF8 {
                p.n &= !1;
            }
            let rc = vdbe_change_encoding(p, encoding as i32);
            if rc != 0 {
                return rc;
            }
            mem_zero_terminate_if_able(p);
        }
    }
    SQLITE_OK
}

/// `sqlite3VdbeMemSetNull`: apaga o valor e deixa a célula `NULL`, preservando o buffer
/// gerenciado (para soltá-lo também, [`mem_release`]).
pub fn mem_set_null(p: &mut Mem) {
    if p.is_dynamic() {
        mem_clear_extern_and_set_null(p);
    } else {
        p.flags = MEM_NULL;
    }
}

/// `sqlite3VdbeMemSetZeroBlob`: o valor passa a ser um blob de `n` bytes zero.
pub fn mem_set_zero_blob(p: &mut Mem, n: i32) {
    mem_release(p);
    p.flags = MEM_BLOB | MEM_ZERO;
    p.n = 0;
    p.n_zero = n.max(0);
    p.enc = ENC_UTF8;
    p.z = Vec::new();
}

/// `sqlite3VdbeMemSetInt64`: apaga o valor e grava `val` como INTEGER.
pub fn mem_set_int64(p: &mut Mem, val: i64) {
    if p.is_dynamic() {
        // vdbeReleaseAndSetInt64: o destrutor roda antes de gravar o inteiro.
        mem_set_null(p);
    }
    p.u_i = val;
    p.flags = MEM_INT;
}

/// `sqlite3VdbeMemSetDouble`: apaga o valor e grava `val` como REAL; NaN vira `NULL`.
pub fn mem_set_double(p: &mut Mem, val: f64) {
    mem_set_null(p);
    if !is_nan(val) {
        p.u_r = val;
        p.flags = MEM_REAL;
    }
}

/// `sqlite3VdbeMemSetRowSet`: a célula passa a guardar um `RowSet` vazio.
pub fn mem_set_row_set(p: &mut Mem) -> i32 {
    debug_assert!(!p.is_row_set());
    mem_release(p);
    p.row_set = Some(Box::new(row_set_init()));
    p.flags = MEM_BLOB | MEM_DYN;
    SQLITE_OK
}

/// `sqlite3VdbeMemTooBig`: verdadeiro se o texto ou blob passa de `limit`
/// (`db->aLimit[SQLITE_LIMIT_LENGTH]`).
pub fn mem_too_big(p: &Mem, limit: i32) -> bool {
    if p.flags & (MEM_STR | MEM_BLOB) != 0 {
        let mut n = p.n;
        if p.flags & MEM_ZERO != 0 {
            n = n.wrapping_add(p.n_zero);
        }
        return n > limit;
    }
    false
}

/// `memcpy(pTo, pFrom, MEMCELLSIZE)`: copia os campos de valor. O texto de uma string ou blob é
/// clonado (a célula destino deixa de ter buffer gerenciado); nas outras células o buffer do
/// destino é mantido para reuso, como o `zMalloc` do C.
fn mem_copy_cell(to: &mut Mem, from: &Mem) {
    to.flags = from.flags;
    to.enc = from.enc;
    to.e_subtype = from.e_subtype;
    to.n = from.n;
    to.u_i = from.u_i;
    to.u_r = from.u_r;
    to.n_zero = from.n_zero;
    if from.flags & (MEM_STR | MEM_BLOB) != 0 {
        to.z = from.z.clone();
        to.sz_malloc = 0;
    }
}

/// `vdbeClrCopy`.
fn vdbe_clr_copy(to: &mut Mem, from: &Mem, e_type: u16) {
    mem_clear_extern_and_set_null(to);
    debug_assert!(!to.is_dynamic());
    mem_shallow_copy(to, from, e_type);
}

/// `sqlite3VdbeMemShallowCopy`: cópia "rasa" de `from` em `to`: o texto fica marcado como
/// `src_type` (`MEM_EPHEM` ou `MEM_STATIC`) e, em Rust, é uma cópia dos bytes.
pub fn mem_shallow_copy(to: &mut Mem, from: &Mem, src_type: u16) {
    debug_assert!(!from.is_row_set());
    if to.is_dynamic() {
        vdbe_clr_copy(to, from, src_type);
        return;
    }
    mem_copy_cell(to, from);
    if from.flags & MEM_STATIC == 0 {
        to.flags &= !(MEM_DYN | MEM_STATIC | MEM_EPHEM);
        debug_assert!(src_type == MEM_EPHEM || src_type == MEM_STATIC);
        to.flags |= src_type;
    }
}

/// `sqlite3VdbeMemCopy`: cópia completa de `from` em `to` (o valor anterior de `to` é
/// descartado). Devolve `SQLITE_OK` ou `SQLITE_NOMEM`.
pub fn mem_copy(to: &mut Mem, from: &Mem) -> i32 {
    let mut rc = SQLITE_OK;
    debug_assert!(!from.is_row_set());
    if to.is_dynamic() {
        mem_clear_extern_and_set_null(to);
    }
    mem_copy_cell(to, from);
    to.flags &= !MEM_DYN;
    if to.flags & (MEM_STR | MEM_BLOB) != 0 && from.flags & MEM_STATIC == 0 {
        to.flags |= MEM_EPHEM;
        rc = mem_make_writeable(to);
    }
    rc
}

/// `sqlite3VdbeMemMove`: transfere o conteúdo de `from` para `to` (o de `to` é liberado) e
/// deixa `from` como `NULL`.
pub fn mem_move(to: &mut Mem, from: &mut Mem) {
    mem_release(to);
    *to = std::mem::take(from);
    from.flags = MEM_NULL;
    from.sz_malloc = 0;
}

/// Como o `SQLITE_STATIC`, `SQLITE_TRANSIENT`, `SQLITE_DYNAMIC` e um destrutor do `xDel` de
/// `sqlite3VdbeMemSetStr`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StrDtor {
    /// `SQLITE_TRANSIENT`: copia para o buffer gerenciado da célula.
    Transient,
    /// `SQLITE_STATIC`: marca `MEM_Static`.
    Static,
    /// `SQLITE_DYNAMIC`: a célula passa a ser dona do buffer (`zMalloc = z`).
    Dynamic,
    /// Um destrutor qualquer: marca `MEM_Dyn` (o destrutor é o `Drop`).
    Func,
}

/// Corpo comum de [`mem_set_str`] e [`mem_set_str_dynamic`].
fn mem_set_str_inner(
    p: &mut Mem,
    z: Option<Cow<'_, [u8]>>,
    n: i64,
    enc: u8,
    x_del: StrDtor,
    limit: i32,
) -> i32 {
    let mut enc = enc;
    let mut n_byte = n;
    debug_assert!(!p.is_row_set());
    debug_assert!(enc != 0 || n >= 0);

    // Sem ponteiro a célula vira NULL.
    let Some(z) = z else {
        mem_set_null(p);
        return SQLITE_OK;
    };

    let i_limit = limit as i64;
    let mut flags: u16;
    if n_byte < 0 {
        debug_assert!(enc != 0);
        if enc == ENC_UTF8 {
            n_byte = z.iter().position(|&c| c == 0).unwrap_or(z.len()) as i64;
        } else {
            n_byte = 0;
            while n_byte <= i_limit && (at(&z, n_byte as usize) | at(&z, n_byte as usize + 1)) != 0 {
                n_byte += 2;
            }
        }
        flags = MEM_STR | MEM_TERM;
    } else if enc == 0 {
        flags = MEM_BLOB;
        enc = ENC_UTF8;
    } else {
        flags = MEM_STR;
    }
    if n_byte > i_limit {
        // O destrutor roda ao descartar `z`. O C ainda faz `sqlite3ErrorToParser`: de quem
        // tem a conexão.
        mem_set_null(p);
        return SQLITE_TOOBIG;
    }

    if x_del == StrDtor::Transient {
        let mut n_alloc = n_byte;
        if flags & MEM_TERM != 0 {
            n_alloc += if enc == ENC_UTF8 { 1 } else { 2 };
        }
        if mem_clear_and_resize(p, n_alloc.max(32) as i32) != SQLITE_OK {
            return SQLITE_NOMEM_BKPT;
        }
        let n_alloc = n_alloc as usize;
        let take = n_alloc.min(z.len());
        p.z[..take].copy_from_slice(&z[..take]);
        p.z[take..n_alloc].fill(0);
    } else {
        mem_release(p);
        let mut v = z.into_owned();
        if v.len() < n_byte as usize {
            v.resize(n_byte as usize, 0);
        }
        match x_del {
            StrDtor::Dynamic => adopt_z(p, v),
            StrDtor::Static => {
                p.z = v;
                flags |= MEM_STATIC;
            }
            _ => {
                p.z = v;
                flags |= MEM_DYN;
            }
        }
    }

    p.n = (n_byte & 0x7fffffff) as i32;
    p.flags = flags;
    p.enc = enc;

    if enc > ENC_UTF8 && mem_handle_bom(p) != SQLITE_OK {
        return SQLITE_NOMEM_BKPT;
    }
    SQLITE_OK
}

/// `sqlite3VdbeMemSetStr`: faz da célula uma string (ou blob, com `enc == 0`). `z == None` é o
/// ponteiro nulo (a célula vira `NULL`); `n < 0` mede a string até o primeiro zero (par de zeros
/// em UTF-16). `limit` é `SQLITE_LIMIT_LENGTH` (`SQLITE_MAX_LENGTH` sem conexão). Devolve
/// `SQLITE_OK`, `SQLITE_NOMEM` ou `SQLITE_TOOBIG` (neste caso a célula vira `NULL`).
///
/// `z` é sempre copiado; `x_del` diz que marca e que gestão a célula assume. Para entregar um
/// buffer já alocado sem cópia (`SQLITE_DYNAMIC`) use [`mem_set_str_dynamic`].
pub fn mem_set_str(
    p: &mut Mem,
    z: Option<&[u8]>,
    n: i64,
    enc: u8,
    x_del: StrDtor,
    limit: i32,
) -> i32 {
    mem_set_str_inner(p, z.map(Cow::Borrowed), n, enc, x_del, limit)
}

/// `sqlite3VdbeMemSetStr` com `SQLITE_DYNAMIC` e um buffer que a célula passa a possuir.
pub fn mem_set_str_dynamic(p: &mut Mem, z: Option<Vec<u8>>, n: i64, enc: u8, limit: i32) -> i32 {
    mem_set_str_inner(p, z.map(Cow::Owned), n, enc, StrDtor::Dynamic, limit)
}

/// `sqlite3VdbeMemFromBtree`: leva para a célula `amt` bytes do payload da entrada do cursor,
/// a partir de `offset`. `max_record_size` é `sqlite3BtreeMaxRecordSize(pCur)` e `read_payload`
/// é `sqlite3BtreePayload(pCur, offset, amt, z)` (recebe o destino de `amt` bytes e devolve o
/// código de erro). Se falhar, a célula é liberada.
pub fn mem_from_btree(
    p: &mut Mem,
    max_record_size: u32,
    offset: u32,
    amt: u32,
    read_payload: impl FnOnce(&mut [u8]) -> i32,
) -> i32 {
    p.flags = MEM_NULL;
    if max_record_size < offset.wrapping_add(amt) {
        return crate::consts::SQLITE_CORRUPT;
    }
    let mut rc = mem_clear_and_resize(p, amt.wrapping_add(1) as i32);
    if rc == SQLITE_OK {
        rc = read_payload(&mut p.z[..amt as usize]);
        if rc == SQLITE_OK {
            // A área extra serve às leituras de registros malformados.
            p.z[amt as usize] = 0;
            p.flags = MEM_BLOB;
            p.n = amt as i32;
        } else {
            mem_release(p);
        }
    }
    rc
}

/// `sqlite3VdbeMemFromBtreeZeroOffset`: como [`mem_from_btree`] com `offset == 0`. `available` é
/// o trecho do payload na página local (`sqlite3BtreePayloadFetch`): se `amt` cabe nele a célula
/// recebe uma cópia efêmera (`MEM_EPHEM`); senão lê o payload todo. Quem só precisa dos bytes
/// de uma chave deve usar `available` direto e evitar a cópia.
pub fn mem_from_btree_zero_offset(
    p: &mut Mem,
    max_record_size: u32,
    amt: u32,
    available: &[u8],
    read_payload: impl FnOnce(&mut [u8]) -> i32,
) -> i32 {
    debug_assert!(!p.is_dynamic());
    debug_assert!(!p.is_row_set());
    if amt as usize <= available.len() {
        p.z = available[..amt as usize].to_vec();
        p.sz_malloc = 0;
        p.flags = MEM_BLOB | MEM_EPHEM;
        p.n = amt as i32;
        SQLITE_OK
    } else {
        mem_from_btree(p, max_record_size, 0, amt, read_payload)
    }
}

/// `valueToText`: converte a célula (que não é `NULL`) em string na codificação `enc` e devolve
/// os `n` bytes (já terminados em zero no buffer), ou `None` se faltar memória.
fn value_to_text(p: &mut Mem, enc: u8) -> Option<&[u8]> {
    debug_assert!((enc & 3) == (enc & !ENC_ALIGNED));
    debug_assert!(!p.is_row_set());
    debug_assert!(p.flags & MEM_NULL == 0);
    let want = enc & !ENC_ALIGNED;
    if p.flags & (MEM_BLOB | MEM_STR) != 0 {
        if p.flags & MEM_ZERO != 0 && mem_expand_blob(p) != SQLITE_OK {
            return None;
        }
        p.flags |= MEM_STR;
        if p.enc != want {
            vdbe_change_encoding(p, want as i32);
        }
        // O C testa se o ponteiro é ímpar; sem ponteiros, uma string efêmera ou estática é
        // tratada como se pudesse ser (a conversão só leva o texto ao buffer gerenciado).
        if enc & ENC_ALIGNED != 0
            && p.flags & (MEM_EPHEM | MEM_STATIC) != 0
            && mem_make_writeable(p) != SQLITE_OK
        {
            return None;
        }
        mem_nul_terminate(p);
    } else {
        mem_stringify(p, want, false);
    }
    if p.enc == want {
        Some(p.bytes())
    } else {
        None
    }
}

/// `sqlite3ValueText`: a string da célula na codificação `enc` (que pode vir com
/// `SQLITE_UTF16_ALIGNED`), convertendo se preciso. `None` para `NULL` ou falta de memória.
/// O buffer da célula fica terminado em zero; o slice devolvido tem os `n` bytes de texto.
pub fn value_text(p: &mut Mem, enc: u8) -> Option<&[u8]> {
    debug_assert!((enc & 3) == (enc & !ENC_ALIGNED));
    debug_assert!(!p.is_row_set());
    if (p.flags & (MEM_STR | MEM_TERM)) == (MEM_STR | MEM_TERM) && p.enc == enc {
        return Some(p.bytes());
    }
    if p.flags & MEM_NULL != 0 {
        return None;
    }
    value_to_text(p, enc)
}

/// `sqlite3ValueBytes`: o número de bytes da célula se usasse a codificação `enc`.
pub fn value_bytes(p: &mut Mem, enc: u8) -> i32 {
    debug_assert!(p.flags & MEM_NULL == 0 || p.flags & (MEM_STR | MEM_BLOB) == 0);
    if p.flags & MEM_STR != 0 && p.enc == enc {
        return p.n;
    }
    if p.flags & MEM_STR != 0 && enc != ENC_UTF8 && p.enc != ENC_UTF8 {
        return p.n;
    }
    if p.flags & MEM_BLOB != 0 {
        if p.flags & MEM_ZERO != 0 {
            return p.n.wrapping_add(p.n_zero);
        } else {
            return p.n;
        }
    }
    if p.flags & MEM_NULL != 0 {
        return 0;
    }
    // valueBytes
    if value_to_text(p, enc).is_some() {
        p.n
    } else {
        0
    }
}

/// `sqlite3_value_type`: a tabela `aType[]` indexada pelos bits de `MEM_AffMask`.
pub fn value_type(p: &Mem) -> i32 {
    const A_TYPE: [u8; 64] = [
        SQLITE_BLOB as u8,    /* 0x00 (not possible) */
        SQLITE_NULL as u8,    /* 0x01 NULL */
        SQLITE_TEXT as u8,    /* 0x02 TEXT */
        SQLITE_NULL as u8,    /* 0x03 (not possible) */
        SQLITE_INTEGER as u8, /* 0x04 INTEGER */
        SQLITE_NULL as u8,    /* 0x05 (not possible) */
        SQLITE_INTEGER as u8, /* 0x06 INTEGER + TEXT */
        SQLITE_NULL as u8,    /* 0x07 (not possible) */
        SQLITE_FLOAT as u8,   /* 0x08 FLOAT */
        SQLITE_NULL as u8,    /* 0x09 (not possible) */
        SQLITE_FLOAT as u8,   /* 0x0a FLOAT + TEXT */
        SQLITE_NULL as u8,    /* 0x0b (not possible) */
        SQLITE_INTEGER as u8, /* 0x0c (not possible) */
        SQLITE_NULL as u8,    /* 0x0d (not possible) */
        SQLITE_INTEGER as u8, /* 0x0e (not possible) */
        SQLITE_NULL as u8,    /* 0x0f (not possible) */
        SQLITE_BLOB as u8,    /* 0x10 BLOB */
        SQLITE_NULL as u8,    /* 0x11 (not possible) */
        SQLITE_TEXT as u8,    /* 0x12 (not possible) */
        SQLITE_NULL as u8,    /* 0x13 (not possible) */
        SQLITE_INTEGER as u8, /* 0x14 INTEGER + BLOB */
        SQLITE_NULL as u8,    /* 0x15 (not possible) */
        SQLITE_INTEGER as u8, /* 0x16 (not possible) */
        SQLITE_NULL as u8,    /* 0x17 (not possible) */
        SQLITE_FLOAT as u8,   /* 0x18 FLOAT + BLOB */
        SQLITE_NULL as u8,    /* 0x19 (not possible) */
        SQLITE_FLOAT as u8,   /* 0x1a (not possible) */
        SQLITE_NULL as u8,    /* 0x1b (not possible) */
        SQLITE_INTEGER as u8, /* 0x1c (not possible) */
        SQLITE_NULL as u8,    /* 0x1d (not possible) */
        SQLITE_INTEGER as u8, /* 0x1e (not possible) */
        SQLITE_NULL as u8,    /* 0x1f (not possible) */
        SQLITE_FLOAT as u8,   /* 0x20 INTREAL */
        SQLITE_NULL as u8,    /* 0x21 (not possible) */
        SQLITE_FLOAT as u8,   /* 0x22 INTREAL + TEXT */
        SQLITE_NULL as u8,    /* 0x23 (not possible) */
        SQLITE_FLOAT as u8,   /* 0x24 (not possible) */
        SQLITE_NULL as u8,    /* 0x25 (not possible) */
        SQLITE_FLOAT as u8,   /* 0x26 (not possible) */
        SQLITE_NULL as u8,    /* 0x27 (not possible) */
        SQLITE_FLOAT as u8,   /* 0x28 (not possible) */
        SQLITE_NULL as u8,    /* 0x29 (not possible) */
        SQLITE_FLOAT as u8,   /* 0x2a (not possible) */
        SQLITE_NULL as u8,    /* 0x2b (not possible) */
        SQLITE_FLOAT as u8,   /* 0x2c (not possible) */
        SQLITE_NULL as u8,    /* 0x2d (not possible) */
        SQLITE_FLOAT as u8,   /* 0x2e (not possible) */
        SQLITE_NULL as u8,    /* 0x2f (not possible) */
        SQLITE_BLOB as u8,    /* 0x30 (not possible) */
        SQLITE_NULL as u8,    /* 0x31 (not possible) */
        SQLITE_TEXT as u8,    /* 0x32 (not possible) */
        SQLITE_NULL as u8,    /* 0x33 (not possible) */
        SQLITE_FLOAT as u8,   /* 0x34 (not possible) */
        SQLITE_NULL as u8,    /* 0x35 (not possible) */
        SQLITE_FLOAT as u8,   /* 0x36 (not possible) */
        SQLITE_NULL as u8,    /* 0x37 (not possible) */
        SQLITE_FLOAT as u8,   /* 0x38 (not possible) */
        SQLITE_NULL as u8,    /* 0x39 (not possible) */
        SQLITE_FLOAT as u8,   /* 0x3a (not possible) */
        SQLITE_NULL as u8,    /* 0x3b (not possible) */
        SQLITE_FLOAT as u8,   /* 0x3c (not possible) */
        SQLITE_NULL as u8,    /* 0x3d (not possible) */
        SQLITE_FLOAT as u8,   /* 0x3e (not possible) */
        SQLITE_NULL as u8,    /* 0x3f (not possible) */
    ];
    A_TYPE[(p.flags & MEM_AFFMASK) as usize] as i32
}

// ---------------------------------------------------------------------------------------------
// Afinidade (vdbe.c)
// ---------------------------------------------------------------------------------------------

/// `alsoAnInt`: a string da célula parece um inteiro e tem valor real `r_value`. Devolve
/// verdadeiro e grava o inteiro em `pi_value` se ele está no intervalo.
fn also_an_int(p: &Mem, r_value: f64, pi_value: &mut i64) -> bool {
    let i_value = real_to_i64(r_value);
    if real_same_as_int(r_value, i_value) {
        *pi_value = i_value;
        return true;
    }
    0 == atoi64(p.bytes(), pi_value, p.n, p.enc)
}

/// `applyNumericAffinity`: converte a string da célula em número, se der sem perder informação.
/// Com `b_try_for_int`, textos como `48.00` ficam também `MEM_Int`.
pub fn apply_numeric_affinity(p: &mut Mem, b_try_for_int: bool) {
    debug_assert!(p.flags & (MEM_STR | MEM_INT | MEM_REAL | MEM_INTREAL) == MEM_STR);
    let (rc, r_value) = atof(p.bytes(), p.n, p.enc, USE_LONG_DOUBLE);
    if rc <= 0 {
        return;
    }
    let mut i_value: i64 = 0;
    if rc == 1 && also_an_int(p, r_value, &mut i_value) {
        p.u_i = i_value;
        p.flags |= MEM_INT;
    } else {
        p.u_r = r_value;
        p.flags |= MEM_REAL;
        if b_try_for_int {
            vdbe_integer_affinity(p);
        }
    }
    // TEXT->NUMERIC é muitos->um: a representação textual pode não ser a canônica do número
    // e por isso é invalidada (ticket 343634942dd54ab57b7024).
    p.flags &= !MEM_STR;
}

/// `applyAffinity` (e `sqlite3ValueApplyAffinity`): aplica a afinidade `affinity` à célula,
/// usando a codificação `enc` ao converter para texto.
pub fn apply_affinity(p: &mut Mem, affinity: u8, enc: u8) {
    if affinity >= SQLITE_AFF_NUMERIC {
        debug_assert!(
            affinity == SQLITE_AFF_INTEGER
                || affinity == SQLITE_AFF_REAL
                || affinity == SQLITE_AFF_NUMERIC
                || affinity == crate::consts::SQLITE_AFF_FLEXNUM
        );
        if p.flags & MEM_INT == 0 {
            if p.flags & (MEM_REAL | MEM_INTREAL) == 0 {
                if p.flags & MEM_STR != 0 {
                    apply_numeric_affinity(p, true);
                }
            } else if affinity <= SQLITE_AFF_REAL {
                vdbe_integer_affinity(p);
            }
        }
    } else if affinity == SQLITE_AFF_TEXT {
        // Só converte se há representação inteira ou real mas não de string (blob e NULL
        // não são convertidos).
        if p.flags & MEM_STR == 0 && p.flags & (MEM_REAL | MEM_INT | MEM_INTREAL) != 0 {
            mem_stringify(p, enc, true);
        }
        p.flags &= !(MEM_REAL | MEM_INT | MEM_INTREAL);
    }
}

/// `sqlite3_value_numeric_type`: tenta converter texto em número sem perda e devolve o tipo
/// resultante (`SQLITE_INTEGER`, `SQLITE_FLOAT`, ...).
pub fn value_numeric_type(p: &mut Mem) -> i32 {
    let mut e_type = value_type(p);
    if e_type == SQLITE_TEXT {
        apply_numeric_affinity(p, false);
        e_type = value_type(p);
    }
    e_type
}

/// `computeNumericType`: o tipo numérico (`MEM_Int` ou `MEM_Real`) de uma célula de texto ou
/// blob, gravando `u_i`/`u_r`; não mexe nas flags.
fn compute_numeric_type(p: &mut Mem) -> u16 {
    debug_assert!(p.flags & (MEM_INT | MEM_REAL | MEM_INTREAL) == 0);
    debug_assert!(p.flags & (MEM_STR | MEM_BLOB) != 0);
    if p.flags & MEM_ZERO != 0 && mem_expand_blob(p) != SQLITE_OK {
        p.u_i = 0;
        return MEM_INT;
    }
    let (rc, r) = atof(p.bytes(), p.n, p.enc, USE_LONG_DOUBLE);
    p.u_r = r;
    let mut ix: i64 = 0;
    if rc <= 0 {
        if rc == 0 && atoi64(p.bytes(), &mut ix, p.n, p.enc) <= 1 {
            p.u_i = ix;
            return MEM_INT;
        } else {
            return MEM_REAL;
        }
    } else if rc == 1 && atoi64(p.bytes(), &mut ix, p.n, p.enc) == 0 {
        p.u_i = ix;
        return MEM_INT;
    }
    MEM_REAL
}

/// `numericType`: o tipo numérico da célula (`MEM_Int`, `MEM_Real`, `MEM_IntReal`, `MEM_Null`
/// ou uma combinação), sem alterar as flags (mas gravando `u_i`/`u_r`).
pub fn numeric_type(p: &mut Mem) -> u16 {
    if p.flags & (MEM_INT | MEM_REAL | MEM_INTREAL | MEM_NULL) != 0 {
        return p.flags & (MEM_INT | MEM_REAL | MEM_INTREAL | MEM_NULL);
    }
    debug_assert!(p.flags & (MEM_STR | MEM_BLOB) != 0);
    compute_numeric_type(p)
}

// ---------------------------------------------------------------------------------------------
// Comparação (vdbeaux.c)
// ---------------------------------------------------------------------------------------------

/// `vdbeCompareMemString` sobre as partes: `z1` (codificação `enc1`) contra o texto de `mem2`,
/// pela colação `coll`. Em erro de memória grava `SQLITE_NOMEM` em `prc_err` e devolve 0.
pub(crate) fn vdbe_compare_mem_string_parts(
    enc1: u8,
    z1: &[u8],
    mem2: &Mem,
    coll: &CollSeq,
    prc_err: Option<&mut u8>,
) -> i32 {
    if enc1 == coll.enc {
        // As strings já estão na codificação certa.
        return coll.x_cmp.call(z1, mem2.bytes());
    }
    let mut c1 = Mem {
        flags: MEM_STR | MEM_EPHEM,
        enc: enc1,
        n: z1.len() as i32,
        z: z1.to_vec(),
        ..Mem::default()
    };
    let mut c2 = Mem {
        flags: MEM_STR | MEM_EPHEM,
        enc: mem2.enc,
        n: mem2.bytes().len() as i32,
        z: mem2.bytes().to_vec(),
        ..Mem::default()
    };
    let v1 = value_text(&mut c1, coll.enc);
    let v2 = value_text(&mut c2, coll.enc);
    match (v1, v2) {
        (Some(v1), Some(v2)) => coll.x_cmp.call(v1, v2),
        _ => {
            if let Some(e) = prc_err {
                *e = SQLITE_NOMEM as u8;
            }
            0
        }
    }
}

/// `vdbeCompareMemString`: as duas células são strings; compara pela colação `coll`.
fn vdbe_compare_mem_string(m1: &Mem, m2: &Mem, coll: &CollSeq, prc_err: Option<&mut u8>) -> i32 {
    vdbe_compare_mem_string_parts(m1.enc, m1.bytes(), m2, coll, prc_err)
}

/// `isAllZero`.
pub(crate) fn is_all_zero(z: &[u8]) -> bool {
    z.iter().all(|&c| c == 0)
}

/// `sqlite3BlobCompare`: compara dois blobs (o mais curto é o menor se for prefixo do outro).
pub fn blob_compare(b1: &Mem, b2: &Mem) -> i32 {
    let n1 = b1.n;
    let n2 = b2.n;

    // Um blob com conteúdo diferente de zero seguido de zeros só nasce no OP_MakeRecord e
    // nunca chega aqui.
    debug_assert!(b1.flags & MEM_ZERO == 0 || n1 == 0);
    debug_assert!(b2.flags & MEM_ZERO == 0 || n2 == 0);

    if (b1.flags | b2.flags) & MEM_ZERO != 0 {
        if b1.flags & b2.flags & MEM_ZERO != 0 {
            return b1.n_zero.wrapping_sub(b2.n_zero);
        } else if b1.flags & MEM_ZERO != 0 {
            if !is_all_zero(b2.bytes()) {
                return -1;
            }
            return b1.n_zero.wrapping_sub(n2);
        } else {
            if !is_all_zero(b1.bytes()) {
                return 1;
            }
            return n1.wrapping_sub(b2.n_zero);
        }
    }
    let c = memcmp(b1.bytes(), b2.bytes());
    if c != 0 {
        return c;
    }
    n1.wrapping_sub(n2)
}

/// `sqlite3IntFloatCompare`: compara um inteiro de 64 bits com um `double`; negativo, zero ou
/// positivo se `i` é menor, igual ou maior que `r`. NaN conta como `NULL`, e todo inteiro é
/// maior que `NULL`.
///
/// Com `bUseLongDouble` o C converte `i` para `long double` (exato) e compara; sem ele,
/// compara por `(i64)r` e só depois por `double`. Os dois caminhos dão a ordem matemática
/// exata, que é o que se calcula aqui.
pub fn int_float_compare(i: i64, r: f64) -> i32 {
    if is_nan(r) {
        return 1;
    }
    if r < -9223372036854775808.0 {
        return 1;
    }
    if r >= 9223372036854775808.0 {
        return -1;
    }
    let y = r as i64;
    if i < y {
        return -1;
    }
    if i > y {
        return 1;
    }
    let yd = y as f64;
    if yd < r {
        -1
    } else if yd > r {
        1
    } else {
        0
    }
}

/// `sqlite3MemCompare`: compara os valores de duas células. Ordem: `NULL`s primeiro, depois os
/// números, depois o texto (pela colação `coll`) e por fim os blobs (por `memcmp`). Dois `NULL`
/// são iguais.
pub fn mem_compare(m1: &Mem, m2: &Mem, coll: Option<&CollSeq>) -> i32 {
    let f1 = m1.flags;
    let f2 = m2.flags;
    let combined_flags = f1 | f2;
    debug_assert!(!m1.is_row_set() && !m2.is_row_set());

    // Se um valor é NULL ele é menor que o outro; dois NULL dão 0.
    if combined_flags & MEM_NULL != 0 {
        return (f2 & MEM_NULL) as i32 - (f1 & MEM_NULL) as i32;
    }

    // Ao menos um dos dois é número.
    if combined_flags & (MEM_INT | MEM_REAL | MEM_INTREAL) != 0 {
        if f1 & f2 & (MEM_INT | MEM_INTREAL) != 0 {
            if m1.u_i < m2.u_i {
                return -1;
            }
            if m1.u_i > m2.u_i {
                return 1;
            }
            return 0;
        }
        if f1 & f2 & MEM_REAL != 0 {
            if m1.u_r < m2.u_r {
                return -1;
            }
            if m1.u_r > m2.u_r {
                return 1;
            }
            return 0;
        }
        if f1 & (MEM_INT | MEM_INTREAL) != 0 {
            if f2 & MEM_REAL != 0 {
                return int_float_compare(m1.u_i, m2.u_r);
            } else if f2 & (MEM_INT | MEM_INTREAL) != 0 {
                if m1.u_i < m2.u_i {
                    return -1;
                }
                if m1.u_i > m2.u_i {
                    return 1;
                }
                return 0;
            } else {
                return -1;
            }
        }
        if f1 & MEM_REAL != 0 {
            if f2 & (MEM_INT | MEM_INTREAL) != 0 {
                return -int_float_compare(m2.u_i, m1.u_r);
            } else {
                return -1;
            }
        }
        return 1;
    }

    // Se um é string e o outro blob, a string é menor; duas strings usam a colação.
    if combined_flags & MEM_STR != 0 {
        if f1 & MEM_STR == 0 {
            return 1;
        }
        if f2 & MEM_STR == 0 {
            return -1;
        }
        debug_assert!(m1.enc == m2.enc);
        debug_assert!(m1.enc == ENC_UTF8 || m1.enc == ENC_UTF16LE || m1.enc == ENC_UTF16BE);

        if let Some(coll) = coll {
            return vdbe_compare_mem_string(m1, m2, coll, None);
        }
        // Sem colação cai no caso do blob e usa memcmp.
    }

    // Os dois são blobs.
    blob_compare(m1, m2)
}

// ---------------------------------------------------------------------------------------------
// Testes
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn text(s: &[u8]) -> Mem {
        let mut m = Mem::default();
        assert_eq!(
            mem_set_str(&mut m, Some(s), s.len() as i64, ENC_UTF8, StrDtor::Transient, 1_000_000_000),
            SQLITE_OK
        );
        m
    }

    #[test]
    fn stringify_int_and_real() {
        let mut m = Mem::default();
        mem_set_int64(&mut m, -12);
        assert_eq!(mem_stringify(&mut m, ENC_UTF8, true), SQLITE_OK);
        assert_eq!(m.bytes(), b"-12");
        assert_eq!(m.flags & (MEM_INT | MEM_STR), MEM_STR);
        let mut r = Mem::default();
        mem_set_double(&mut r, 1.5);
        assert_eq!(mem_stringify(&mut r, ENC_UTF8, false), SQLITE_OK);
        assert_eq!(r.bytes(), b"1.5");
        assert!(r.flags & MEM_REAL != 0 && r.flags & MEM_STR != 0);
    }

    #[test]
    fn numerify_text() {
        let mut m = text(b"12abc");
        mem_numerify(&mut m);
        assert_eq!(m.flags & (MEM_INT | MEM_REAL | MEM_STR), MEM_INT);
        assert_eq!(m.u_i, 12);
        let mut m = text(b"3.0");
        mem_numerify(&mut m);
        assert_eq!(m.flags & (MEM_INT | MEM_REAL), MEM_INT);
        assert_eq!(m.u_i, 3);
        let mut m = text(b"1.5");
        mem_numerify(&mut m);
        assert_eq!(m.flags & (MEM_INT | MEM_REAL), MEM_REAL);
        assert_eq!(m.u_r, 1.5);
    }

    #[test]
    fn compare_order() {
        let null = Mem::value_new();
        let mut i = Mem::default();
        mem_set_int64(&mut i, 5);
        let mut r = Mem::default();
        mem_set_double(&mut r, 5.5);
        let t = text(b"abc");
        let mut b = Mem::default();
        mem_set_str(&mut b, Some(&b"abc"[..]), 3, 0, StrDtor::Transient, 1_000_000_000);
        assert!(mem_compare(&null, &i, None) < 0);
        assert!(mem_compare(&i, &r, None) < 0);
        assert!(mem_compare(&r, &i, None) > 0);
        assert!(mem_compare(&r, &t, None) < 0);
        assert!(mem_compare(&t, &b, None) < 0);
        assert!(mem_compare(&b, &t, None) > 0);
        assert_eq!(mem_compare(&null, &Mem::value_new(), None), 0);
        let coll = CollSeq { name: b"NOCASE".to_vec(), enc: ENC_UTF8, x_cmp: CollFn::NoCase };
        assert_eq!(mem_compare(&text(b"ABC"), &text(b"abc"), Some(&coll)), 0);
        assert!(mem_compare(&text(b"ABC"), &text(b"abc"), None) < 0);
    }

    #[test]
    fn int_float() {
        assert_eq!(int_float_compare(3, 3.0), 0);
        assert_eq!(int_float_compare(3, 3.5), -1);
        assert_eq!(int_float_compare(-3, -3.5), 1);
        assert_eq!(int_float_compare(i64::MAX, 9223372036854775808.0), -1);
        assert_eq!(int_float_compare(1, f64::NAN), 1);
    }

    #[test]
    fn translate_roundtrip() {
        let mut m = text("héllo".as_bytes());
        assert_eq!(vdbe_change_encoding(&mut m, SQLITE_UTF16LE), SQLITE_OK);
        assert_eq!(m.enc, ENC_UTF16LE);
        assert_eq!(m.bytes(), &[b'h', 0, 0xe9, 0, b'l', 0, b'l', 0, b'o', 0]);
        assert_eq!(vdbe_change_encoding(&mut m, SQLITE_UTF16BE), SQLITE_OK);
        assert_eq!(m.bytes(), &[0, b'h', 0, 0xe9, 0, b'l', 0, b'l', 0, b'o']);
        assert_eq!(vdbe_change_encoding(&mut m, SQLITE_UTF8), SQLITE_OK);
        assert_eq!(m.bytes(), "héllo".as_bytes());
    }

    #[test]
    fn zero_blob_expands() {
        let mut m = Mem::default();
        mem_set_zero_blob(&mut m, 4);
        assert_eq!(mem_expand_blob(&mut m), SQLITE_OK);
        assert_eq!(m.bytes(), &[0, 0, 0, 0]);
        assert_eq!(m.flags & MEM_ZERO, 0);
    }

    #[test]
    fn set_str_too_big() {
        let mut m = Mem::default();
        assert_eq!(mem_set_str(&mut m, Some(&b"abcdef"[..]), 6, ENC_UTF8, StrDtor::Static, 5), SQLITE_TOOBIG);
        assert_eq!(m.flags, MEM_NULL);
    }

    #[test]
    fn value_type_table() {
        let mut m = Mem::default();
        mem_set_int64(&mut m, 1);
        assert_eq!(value_type(&m), SQLITE_INTEGER);
        assert_eq!(value_type(&Mem::value_new()), SQLITE_NULL);
        assert_eq!(value_type(&text(b"x")), SQLITE_TEXT);
    }

    #[test]
    fn cast_to_text_and_blob() {
        let mut m = Mem::default();
        mem_set_int64(&mut m, 42);
        mem_cast(&mut m, SQLITE_AFF_TEXT, ENC_UTF8);
        assert_eq!(m.flags & (MEM_STR | MEM_INT), MEM_STR);
        assert_eq!(m.bytes(), b"42");
        mem_cast(&mut m, SQLITE_AFF_BLOB, ENC_UTF8);
        assert!(m.flags & MEM_BLOB != 0 && m.flags & MEM_STR == 0);
    }
}
