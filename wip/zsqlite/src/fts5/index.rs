//! `fts5_index.c` (parte 1): o acesso de baixo nível ao índice FTS guardado nas tabelas `%_data` e
//! `%_idx`. Esta parte tem as constantes e os tipos, a leitura e a escrita de registros, o registro
//! de estrutura (segmentos e níveis), os iteradores de índice de doclist e os iteradores de
//! segmento. A parte 2 ([`super::index2`]) tem o multi-iterador, o escritor de segmentos, o merge, o
//! flush e o optimize; a parte 3 ([`super::index3`]) tem as consultas, o tokendata, os
//! tombstones do `contentless_delete`, a verificação de integridade e a interface pública
//! (`Fts5Index::open`, `query`, `begin_write` e as demais). As duas partes são reexportadas aqui.
//!
//! # Desvios do C, decorrentes do modelo v2
//!
//! * `Fts5Index` não guarda `pConfig` nem o `sqlite3 *db`: toda função recebe `db:
//!   &mut Connection` e a configuração (`&Fts5Config`, ou `&mut Fts5Config` onde a cadeia de
//!   chamadas chega em `fts5_structure_read`, que pode recarregar `%_config`). O erro pendente é
//!   o campo `rc`, como no C. `nPendingData` é `Fts5Hash::n_byte` (o hash é criado em
//!   `begin_write`; sem hash o valor é 0).
//! * O `sqlite3_blob` de leitura (`pReader`) vira um comando preparado `SELECT block FROM %_data
//!   WHERE id=?1`, reiniciado a cada leitura (o mesmo caminho do `rtree.rs`, porque este porte
//!   ainda não tem o `vdbeblob.c`). Os erros são os que o C converte em `FTS5_CORRUPT`: tabela
//!   ausente, linha ausente, coluna que não é BLOB nem TEXT. `close_reader` finaliza o comando.
//!   O `sqlite3_blob_write` de `set_cookie` vira uma leitura seguida de `REPLACE`.
//! * `Fts5Structure` é um valor, não um objeto com contagem de referências. `fts5_structure_read`
//!   devolve uma cópia do que está em `p.p_struct`; quem recebe a cópia a altera à vontade (o C
//!   chama `fts5StructureMakeWritable`, que só copia quando `nRef>1`, e todo chamador que muda a
//!   estrutura invalida o cache antes). A única mudança feita no objeto compartilhado
//!   (`contentless_delete`, que altera `nPgTombstone` e `nEntryTombstone` de `p->pStruct`) opera
//!   direto em `p.p_struct`. A identidade que `sqlite3Fts5StructureRef`/`Test` comparam é o campo
//!   `Fts5Structure::id`, renovado a cada leitura do disco.
//! * `Fts5SegIter.pSeg` é uma cópia do `Fts5StructureSegment`: o único escritor através dele,
//!   `fts5TrimSegments`, recebe o nível de entrada e atualiza lá (o iterador `i` de um merge lê o
//!   segmento `nInput-1-i` do nível).
//! * `Fts5Data` guarda o registro seguido de `FTS5_DATA_PADDING` zeros; leituras além disso
//!   (registro corrompido) enxergam zeros. Folhas do hash em memória são cópias.
//! * `Fts5Iter` não aponta para o índice (`pIndex`): todo método recebe `p`. `pColset` é uma cópia
//!   do `Fts5Colset`. `Fts5IndexIter.pData` é uma cópia (`p_data`) da poslist corrente.
//! * O vetor de tombstones de um segmento (`Fts5TombstoneArray`, compartilhado entre os iteradores
//!   do tokendata e carregado preguiçosamente) é `Rc<RefCell<..>>`, o único compartilhamento
//!   mutável que o C documenta.
//! * As funções de depuração (`fts5_decode`, `fts5_rowid`, `fts5_structure`, `fts5TestTerm`,
//!   `fts5TestDlidxReverse`, `fts5QueryCksum`) só existem sob `SQLITE_TEST`/`SQLITE_FTS5_DEBUG`/
//!   `SQLITE_DEBUG`, desligados no Debian: não entram, e `fts5_index_init` é só `SQLITE_OK`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::connection::{Connection, StmtId};
use crate::consts::{
    SQLITE_BLOB, SQLITE_DONE, SQLITE_ERROR, SQLITE_NOMEM, SQLITE_OK, SQLITE_PREPARE_NO_VTAB,
    SQLITE_PREPARE_PERSISTENT, SQLITE_ROW, SQLITE_TEXT,
};
use crate::mem::StrDtor;
use crate::prepare::prepare_v3;
use crate::printf::{mprintf, PrintfArg};
use crate::util::at;
use crate::vdbeapi::{
    bind_blob, bind_int, bind_int64, bind_null, column_blob, column_int, column_int64,
    column_type, finalize, reset, step,
};

use super::buffer::Fts5Buffer;
use super::hash::Fts5Hash;
use super::int::{
    Fts5Colset, Fts5Config, FTS5INDEX_QUERY_DESC, FTS5INDEX_QUERY_SCAN,
    FTS5INDEX_QUERY_SCANONETERM, FTS5_CORRUPT, FTS5_CURRENT_VERSION, FTS5_DETAIL_NONE,
    FTS5_MAX_SEGMENT,
};
use super::varint::{fts5_fast_get_varint32, fts5_get_varint, fts5_get_varint32};

pub use super::index2::*;
pub use super::index3::*;

// ---------------------------------------------------------------------------------------------
// Constantes
// ---------------------------------------------------------------------------------------------

/// `FTS5_OPT_WORK_UNIT`: páginas folha por passo do optimize.
pub const FTS5_OPT_WORK_UNIT: i32 = 1000;
/// `FTS5_WORK_UNIT`: páginas folha numa unidade de trabalho.
pub const FTS5_WORK_UNIT: i32 = 64;
/// `FTS5_MIN_DLIDX_SIZE`: acrescenta um dlidx se houver tantas páginas vazias.
pub const FTS5_MIN_DLIDX_SIZE: i32 = 4;
/// `FTS5_MAIN_PREFIX`: o byte que abre as chaves do índice principal.
pub const FTS5_MAIN_PREFIX: u8 = b'0';
/// `FTS5_MAX_LEVEL`.
pub const FTS5_MAX_LEVEL: i32 = 64;
/// `FTS5_STRUCTURE_V2`: os 4 bytes que abrem um registro de estrutura V2.
pub const FTS5_STRUCTURE_V2: [u8; 4] = [0xFF, 0x00, 0x00, 0x01];
/// `FTS5_AVERAGES_ROWID`: rowid do registro de médias.
pub const FTS5_AVERAGES_ROWID: i64 = 1;
/// `FTS5_STRUCTURE_ROWID`: rowid do registro de estrutura.
pub const FTS5_STRUCTURE_ROWID: i64 = 10;

/// `FTS5_DATA_ZERO_PADDING`.
pub const FTS5_DATA_ZERO_PADDING: usize = 8;
/// `FTS5_DATA_PADDING`: zeros que seguem cada registro lido.
pub const FTS5_DATA_PADDING: usize = 20;

/// `FTS5_SEGITER_ONETERM`.
pub const FTS5_SEGITER_ONETERM: i32 = 0x01;
/// `FTS5_SEGITER_REVERSE`.
pub const FTS5_SEGITER_REVERSE: i32 = 0x02;

/// `fts5_dri`: o rowid de `%_data` de um registro (segmento, flag de dlidx, altura, página).
#[inline]
pub fn fts5_dri(segid: i64, dlidx: i64, height: i64, pgno: i64) -> i64 {
    (segid << 37)
        .wrapping_add(dlidx << 36)
        .wrapping_add(height << 31)
        .wrapping_add(pgno)
}

/// `FTS5_SEGMENT_ROWID`.
#[inline]
pub fn fts5_segment_rowid(segid: i32, pgno: i32) -> i64 {
    fts5_dri(segid as i64, 0, 0, pgno as i64)
}

/// `FTS5_DLIDX_ROWID`.
#[inline]
pub fn fts5_dlidx_rowid(segid: i32, height: i32, pgno: i32) -> i64 {
    fts5_dri(segid as i64, 1, height as i64, pgno as i64)
}

/// `FTS5_TOMBSTONE_ROWID`.
#[inline]
pub fn fts5_tombstone_rowid(segid: i32, ipg: i32) -> i64 {
    fts5_dri(segid as i64 + (1 << 16), 0, 0, ipg as i64)
}

// ---------------------------------------------------------------------------------------------
// Auxiliares de bytes
// ---------------------------------------------------------------------------------------------

/// `fts5GetU16`.
#[inline]
pub(crate) fn get_u16(a: &[u8], off: usize) -> u16 {
    ((at(a, off) as u16) << 8) + at(a, off + 1) as u16
}

/// `fts5PutU16` (sem efeito se `a` é curto demais, o que só acontece com erro pendente).
#[inline]
pub(crate) fn put_u16(a: &mut [u8], off: usize, v: u16) {
    if off + 2 <= a.len() {
        a[off] = (v >> 8) as u8;
        a[off + 1] = (v & 0xFF) as u8;
    }
}

/// `fts5GetU32`.
#[inline]
pub(crate) fn get_u32(a: &[u8], off: usize) -> u32 {
    ((at(a, off) as u32) << 24)
        + ((at(a, off + 1) as u32) << 16)
        + ((at(a, off + 2) as u32) << 8)
        + at(a, off + 3) as u32
}

/// `fts5PutU32`.
#[inline]
pub(crate) fn put_u32(a: &mut [u8], off: usize, v: u32) {
    if off + 4 <= a.len() {
        a[off] = (v >> 24) as u8;
        a[off + 1] = (v >> 16) as u8;
        a[off + 2] = (v >> 8) as u8;
        a[off + 3] = v as u8;
    }
}

/// `fts5GetU64`.
#[inline]
pub(crate) fn get_u64(a: &[u8], off: usize) -> u64 {
    ((get_u32(a, off) as u64) << 32) + get_u32(a, off + 4) as u64
}

/// `fts5PutU64`.
#[inline]
pub(crate) fn put_u64(a: &mut [u8], off: usize, v: u64) {
    put_u32(a, off, (v >> 32) as u32);
    put_u32(a, off + 4, v as u32);
}

/// O resto de `a` a partir de `off` (vazio se `off` passa do fim).
#[inline]
pub(crate) fn tail(a: &[u8], off: usize) -> &[u8] {
    if off >= a.len() {
        &[]
    } else {
        &a[off..]
    }
}

/// `off` (um `int` do C) como índice: negativo vira 0.
#[inline]
pub(crate) fn ux(off: i32) -> usize {
    if off < 0 {
        0
    } else {
        off as usize
    }
}

/// `n` bytes de `a` a partir de `off`, limitados ao que existe.
#[inline]
pub(crate) fn sub(a: &[u8], off: i32, n: i32) -> &[u8] {
    let s = ux(off).min(a.len());
    let e = (s + ux(n)).min(a.len());
    &a[s..e]
}

/// `fts5GetVarint32(&a[off], v)`: devolve `(bytes lidos, valor de 31 bits)`.
#[inline]
pub(crate) fn gv32(a: &[u8], off: i32) -> (i32, i32) {
    let (n, v) = fts5_get_varint32(tail(a, ux(off)));
    (n, v as i32)
}

/// `fts5GetVarint(&a[off], v)`: devolve `(bytes lidos, valor de 64 bits)`.
#[inline]
pub(crate) fn gv64(a: &[u8], off: i32) -> (i32, u64) {
    let (n, v) = fts5_get_varint(tail(a, ux(off)));
    (n as i32, v)
}

/// `fts5FastGetVarint32(a, off, v)` sobre `i32`: lê, avança `*off` e devolve o valor.
#[inline]
pub(crate) fn fast32(a: &[u8], off: &mut i32) -> i32 {
    let mut o = ux(*off);
    let v = fts5_fast_get_varint32(a, &mut o);
    *off = o as i32;
    v as i32
}

/// `fts5BufferCompare`: negativo, zero ou positivo (só o sinal conta).
pub(crate) fn fts5_buffer_compare(left: &Fts5Buffer, right: &Fts5Buffer) -> i32 {
    let n_cmp = left.p.len().min(right.p.len());
    match left.p[..n_cmp].cmp(&right.p[..n_cmp]) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Equal => {
            if left.p.len() < right.p.len() {
                -1
            } else if left.p.len() > right.p.len() {
                1
            } else {
                0
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tipos
// ---------------------------------------------------------------------------------------------

/// `Fts5Data`: um registro lido de `%_data`. `p` tem `nn` bytes de dado seguidos de
/// `FTS5_DATA_PADDING` zeros.
#[derive(Debug, Clone, Default)]
pub struct Fts5Data {
    /// O registro mais o preenchimento de zeros.
    pub p: Vec<u8>,
    /// Tamanho do registro em bytes.
    pub nn: i32,
    /// Tamanho da folha sem o índice de página.
    pub sz_leaf: i32,
}

impl Fts5Data {
    /// Registro com os bytes de `v` (e o preenchimento); `sz_leaf` é 0 até alguém o definir.
    pub fn from_bytes(mut v: Vec<u8>) -> Fts5Data {
        let nn = v.len() as i32;
        v.resize(v.len() + FTS5_DATA_PADDING, 0);
        Fts5Data { p: v, nn, sz_leaf: 0 }
    }

    /// Registro de uma folha (`szLeaf` lido do cabeçalho de 2 bytes em 2).
    pub fn leaf(v: Vec<u8>) -> Fts5Data {
        let mut d = Fts5Data::from_bytes(v);
        d.sz_leaf = get_u16(&d.p, 2) as i32;
        d
    }

    /// Folha de doclist do hash ou do prefixo: `nn == sz_leaf == v.len()`.
    pub fn doclist(v: Vec<u8>) -> Fts5Data {
        let mut d = Fts5Data::from_bytes(v);
        d.sz_leaf = d.nn;
        d
    }
}

/// `fts5LeafIsTermless`: verdadeiro se a folha não tem termos.
#[inline]
pub(crate) fn fts5_leaf_is_termless(x: &Fts5Data) -> bool {
    x.sz_leaf >= x.nn
}

/// `fts5LeafFirstRowidOff`.
#[inline]
pub(crate) fn fts5_leaf_first_rowid_off(x: &Fts5Data) -> i32 {
    get_u16(&x.p, 0) as i32
}

/// `fts5LeafFirstTermOff`.
pub(crate) fn fts5_leaf_first_term_off(leaf: &Fts5Data) -> i32 {
    gv32(&leaf.p, leaf.sz_leaf).1
}

/// `Fts5StructureSegment`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Fts5StructureSegment {
    /// Id do segmento.
    pub i_segid: i32,
    /// Primeira folha do segmento.
    pub pgno_first: i32,
    /// Última folha do segmento.
    pub pgno_last: i32,
    /// Só `contentless_delete=1`: menor origem.
    pub i_origin1: u64,
    /// Só `contentless_delete=1`: maior origem.
    pub i_origin2: u64,
    /// Número de páginas da tabela hash de tombstones.
    pub n_pg_tombstone: i32,
    /// Número de tombstones que "contam".
    pub n_entry_tombstone: u64,
    /// Número de linhas do segmento.
    pub n_entry: u64,
}

/// `Fts5StructureLevel`: `a_seg` pode ter mais elementos que `n_seg` (folga, como no C).
#[derive(Debug, Clone, Default)]
pub struct Fts5StructureLevel {
    /// Segmentos num merge incremental.
    pub n_merge: i32,
    /// Total de segmentos do nível.
    pub n_seg: i32,
    /// Os segmentos, do mais antigo ao mais novo.
    pub a_seg: Vec<Fts5StructureSegment>,
}

/// `Fts5Structure`. `nLevel` é `a_level.len()`.
#[derive(Debug, Clone, Default)]
pub struct Fts5Structure {
    /// Identidade da leitura que originou o objeto (veja o cabeçalho do módulo).
    pub id: u64,
    /// Total de folhas escritas no nível 0.
    pub n_write_counter: u64,
    /// Origem para o próximo segmento (só `contentless_delete=1`).
    pub n_origin_cntr: u64,
    /// Total de segmentos.
    pub n_segment: i32,
    /// Os níveis.
    pub a_level: Vec<Fts5StructureLevel>,
}

impl Fts5Structure {
    /// `nLevel`.
    #[inline]
    pub fn n_level(&self) -> i32 {
        self.a_level.len() as i32
    }
}

/// `Fts5PageWriter`.
#[derive(Debug, Default)]
pub struct Fts5PageWriter {
    /// Número da página.
    pub pgno: i32,
    /// Valor anterior escrito no pgidx.
    pub i_prev_pgidx: i32,
    /// Dados da folha.
    pub buf: Fts5Buffer,
    /// Índice de página.
    pub pgidx: Fts5Buffer,
    /// Termo anterior da página.
    pub term: Fts5Buffer,
}

/// `Fts5DlidxWriter`.
#[derive(Debug, Default)]
pub struct Fts5DlidxWriter {
    /// Número da página.
    pub pgno: i32,
    /// Verdadeiro se `i_prev` vale.
    pub b_prev_valid: i32,
    /// Último rowid escrito na página.
    pub i_prev: i64,
    /// Dados da página.
    pub buf: Fts5Buffer,
}

/// `Fts5SegWriter`.
#[derive(Debug, Default)]
pub struct Fts5SegWriter {
    /// Id do segmento escrito.
    pub i_segid: i32,
    /// A página corrente.
    pub writer: Fts5PageWriter,
    /// Rowid anterior da folha corrente.
    pub i_prev_rowid: i64,
    /// O próximo rowid é o primeiro da doclist.
    pub b_first_rowid_in_doclist: u8,
    /// O próximo rowid é o primeiro da página.
    pub b_first_rowid_in_page: u8,
    /// O próximo termo é o primeiro da folha.
    pub b_first_term_in_page: u8,
    /// Folhas escritas.
    pub n_leaf_written: i32,
    /// Nós contíguos sem termo.
    pub n_empty: i32,
    /// Escritores de dlidx (`nDlidx` é `len()`).
    pub a_dlidx: Vec<Fts5DlidxWriter>,
    /// Próximo termo a inserir em `%_idx`.
    pub btterm: Fts5Buffer,
    /// Página correspondente a `btterm`.
    pub i_bt_page: i32,
}

/// `Fts5CResult`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Fts5CResult {
    /// Índice do iterador mais adiantado.
    pub i_first: u16,
    /// Verdadeiro se os termos são iguais.
    pub b_term_eq: u8,
}

/// O `xNext` de um `Fts5SegIter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SegNext {
    /// `fts5SegIterNext`.
    #[default]
    Normal,
    /// `fts5SegIterNext_Reverse`.
    Reverse,
    /// `fts5SegIterNext_None` (`detail=none`).
    NoDetail,
}

/// `Fts5TombstoneArray`: as páginas da tabela hash de tombstones de um segmento, carregadas
/// preguiçosamente. A contagem de referências do C é o `Rc`.
#[derive(Debug, Default)]
pub struct Fts5TombstoneArray {
    /// Número de páginas (`nTombstone`).
    pub n_tombstone: i32,
    /// As páginas já carregadas.
    pub ap_tombstone: Vec<Option<Fts5Data>>,
}

/// `Fts5SegIter`: um iterador sobre um segmento (ou sobre o hash em memória, ou sobre uma doclist).
#[derive(Debug, Default)]
pub struct Fts5SegIter {
    /// O segmento (`None` para o hash, para uma doclist e para preenchimento).
    pub p_seg: Option<Fts5StructureSegment>,
    /// Máscara de `FTS5_SEGITER_*`.
    pub flags: i32,
    /// Folha corrente.
    pub i_leaf_pgno: i32,
    /// Dados da folha corrente (`None` no fim).
    pub p_leaf: Option<Fts5Data>,
    /// Folha `i_leaf_pgno+1`.
    pub p_next_leaf: Option<Fts5Data>,
    /// Deslocamento na folha corrente.
    pub i_leaf_offset: i64,
    /// Páginas de tombstone.
    pub p_tomb_array: Option<Rc<RefCell<Fts5TombstoneArray>>>,
    /// O `xNext`.
    pub x_next: SegNext,
    /// Página da qual o termo corrente foi lido.
    pub i_term_leaf_pgno: i32,
    /// Deslocamento logo depois do termo.
    pub i_term_leaf_offset: i32,
    /// Próximo deslocamento no pgidx.
    pub i_pgidx_off: i32,
    /// Fim da doclist.
    pub i_endof_doclist: i32,
    /// Só reverso: entrada corrente em `a_rowid_offset`.
    pub i_rowid_offset: i32,
    /// Só reverso: tamanho alocado de `a_rowid_offset`.
    pub n_rowid_offset: i32,
    /// Só reverso: deslocamento dos campos de rowid.
    pub a_rowid_offset: Vec<i32>,
    /// Índice de doclist, se houver.
    pub p_dlidx: Option<Box<Fts5DlidxIter>>,
    /// Termo corrente.
    pub term: Fts5Buffer,
    /// Rowid corrente.
    pub i_rowid: i64,
    /// Bytes da poslist corrente.
    pub n_pos: i32,
    /// Verdadeiro se a flag de delete está ligada.
    pub b_del: u8,
}

/// `Fts5DlidxLvl`.
#[derive(Debug, Default)]
pub struct Fts5DlidxLvl {
    /// Dados da página corrente do nível.
    pub p_data: Option<Fts5Data>,
    /// Deslocamento corrente.
    pub i_off: i32,
    /// Já no fim.
    pub b_eof: i32,
    /// Usado pelos iteradores reversos.
    pub i_first_off: i32,
    /// Número da folha corrente.
    pub i_leaf_pgno: i32,
    /// Primeiro rowid da folha `i_leaf_pgno`.
    pub i_rowid: i64,
}

/// `Fts5DlidxIter` (`nLvl` é `a_lvl.len()`).
#[derive(Debug, Default)]
pub struct Fts5DlidxIter {
    /// Id do segmento.
    pub i_segid: i32,
    /// Os níveis.
    pub a_lvl: Vec<Fts5DlidxLvl>,
}

/// O `xSetOutputs` de um `Fts5Iter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SetOutputsKind {
    /// `fts5IterSetOutputs_Noop`.
    #[default]
    Noop,
    /// `fts5IterSetOutputs_None`.
    NoDetail,
    /// `fts5IterSetOutputs_Nocolset`.
    Nocolset,
    /// `fts5IterSetOutputs_ZeroColset`.
    ZeroColset,
    /// `fts5IterSetOutputs_Col`.
    Col,
    /// `fts5IterSetOutputs_Col100`.
    Col100,
    /// `fts5IterSetOutputs_Full`.
    Full,
}

/// `Fts5IndexIter` (a parte pública do `Fts5Iter`).
#[derive(Debug, Default, Clone)]
pub struct Fts5IterBase {
    /// Rowid corrente.
    pub i_rowid: i64,
    /// A poslist corrente (os `n_data` primeiros bytes valem).
    pub p_data: Vec<u8>,
    /// Bytes da poslist corrente.
    pub n_data: i32,
    /// Verdadeiro no fim.
    pub b_eof: u8,
}

/// `Fts5Iter`: o multi-iterador (mescla um ou mais iteradores de segmento). É o `Fts5IndexIter*`
/// da interface pública.
#[derive(Debug, Default)]
pub struct Fts5Iter {
    /// A parte pública.
    pub base: Fts5IterBase,
    /// Só tokendata=1.
    pub p_token_data_iter: Option<Box<Fts5TokenDataIter>>,
    /// Buffer da poslist corrente.
    pub poslist: Fts5Buffer,
    /// Restringe os casamentos a estas colunas.
    pub p_colset: Option<Fts5Colset>,
    /// O `xSetOutputs`.
    pub x_set_outputs: SetOutputsKind,
    /// Tamanho de `a_seg` (potência de dois).
    pub n_seg: i32,
    /// Verdadeiro para ordem decrescente.
    pub b_rev: i32,
    /// Verdadeiro para pular entradas apagadas.
    pub b_skip_empty: u8,
    /// O rowid "mais adiantado" que não é o de `a_first[1]`.
    pub i_switch_rowid: i64,
    /// O estado da mescla (`n_seg` elementos).
    pub a_first: Vec<Fts5CResult>,
    /// Os iteradores de segmento.
    pub a_seg: Vec<Fts5SegIter>,
}

/// O nome público do multi-iterador (`Fts5IndexIter` do `fts5Int.h`).
pub type Fts5IndexIter = Fts5Iter;

/// `Fts5TokenDataMap`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Fts5TokenDataMap {
    /// Linha em que o token está.
    pub i_rowid: i64,
    /// Posição do token (ou -1).
    pub i_pos: i64,
    /// Iterador de onde o token foi lido.
    pub i_iter: i32,
}

/// `Fts5TokenDataIter`.
#[derive(Debug, Default)]
pub struct Fts5TokenDataIter {
    /// O mapa token -> iterador (`nMap` é `len()`).
    pub a_map: Vec<Fts5TokenDataMap>,
    /// Os iteradores (`nIter` é `len()`).
    pub ap_iter: Vec<Fts5Iter>,
}

/// `Fts5Index`: um objeto por tabela `%_data`.
#[derive(Debug, Default)]
pub struct Fts5Index {
    /// Nome da tabela `%_data`.
    pub z_data_tbl: Vec<u8>,
    /// Folhas numa "unidade" de trabalho.
    pub n_work_unit: i32,
    /// Hash dos dados em memória (criado em `begin_write`).
    pub p_hash: Option<Fts5Hash>,
    /// Rowid do documento sendo escrito.
    pub i_write_rowid: i64,
    /// A escrita corrente é um delete.
    pub b_delete: i32,
    /// Deletes de contentless desde o último flush.
    pub n_contentless_delete: i32,
    /// INSERTs no hash.
    pub n_pending_row: i32,
    /// Erro corrente.
    pub rc: i32,
    /// Erro do último flush.
    pub flush_rc: i32,
    /// O comando de leitura de `%_data` (o `pReader` do C).
    pub p_reader: Option<StmtId>,
    /// `REPLACE INTO %_data VALUES(?,?)`.
    pub p_writer: Option<StmtId>,
    /// `DELETE FROM %_data WHERE id>=? AND id<=?`.
    pub p_deleter: Option<StmtId>,
    /// `INSERT INTO %_idx VALUES(?,?,?)`.
    pub p_idx_writer: Option<StmtId>,
    /// `DELETE FROM %_idx WHERE segid=?`.
    pub p_idx_deleter: Option<StmtId>,
    /// `SELECT pgno FROM %_idx WHERE segid=? AND term<=? ...`.
    pub p_idx_select: Option<StmtId>,
    /// `SELECT pgno FROM %_idx WHERE segid=? AND term>? ...`.
    pub p_idx_next_select: Option<StmtId>,
    /// Blocos lidos no total.
    pub n_read: i32,
    /// `DELETE FROM %_idx WHERE (segid, (pgno/2)) = (?1, ?2)`.
    pub p_delete_from_idx: Option<StmtId>,
    /// `PRAGMA data_version`.
    pub p_data_version: Option<StmtId>,
    /// `data_version` quando `p_struct` foi lida.
    pub i_struct_version: i64,
    /// A estrutura corrente do banco (cache).
    pub p_struct: Option<Fts5Structure>,
    /// Contador que gera `Fts5Structure::id`.
    pub n_struct_id: u64,
}

impl Fts5Index {
    /// `nPendingData`: o contador de bytes do hash.
    #[inline]
    pub fn n_pending_data(&self) -> i32 {
        self.p_hash.as_ref().map_or(0, |h| h.n_byte)
    }

    /// `p->nPendingData = 0`.
    #[inline]
    pub(crate) fn clear_pending_data(&mut self) {
        if let Some(h) = self.p_hash.as_mut() {
            h.n_byte = 0;
        }
    }

    /// `sqlite3Fts5IndexCloseReader`: fecha o comando de leitura.
    pub fn close_reader(&mut self, db: &mut Connection) {
        if let Some(id) = self.p_reader.take() {
            finalize(db, id);
        }
    }

    /// `sqlite3Fts5StructureRef`: a identidade da estrutura corrente (o `p->pStruct` do C; a
    /// estrutura precisa estar carregada).
    pub fn structure_ref(&self) -> u64 {
        self.p_struct.as_ref().map_or(0, |s| s.id)
    }

    /// `sqlite3Fts5StructureRelease`: sem efeito (a identidade não guarda nada).
    pub fn structure_release(_id: u64) {}

    /// `sqlite3Fts5StructureTest`: `SQLITE_ABORT` se a estrutura mudou desde `structure_ref`.
    pub fn structure_test(&self, id: u64) -> i32 {
        match self.p_struct.as_ref() {
            Some(s) if s.id == id => SQLITE_OK,
            _ => crate::consts::SQLITE_ABORT,
        }
    }
}

/// `fts5IndexReturn`: devolve `rc` e o zera.
pub(crate) fn fts5_index_return(p: &mut Fts5Index) -> i32 {
    let rc = p.rc;
    p.rc = SQLITE_OK;
    rc
}

// ---------------------------------------------------------------------------------------------
// Leitura e escrita de registros de %_data
// ---------------------------------------------------------------------------------------------

const PREPARE_FLAGS: u32 = SQLITE_PREPARE_PERSISTENT | SQLITE_PREPARE_NO_VTAB;

/// `fts5IndexPrepareStmt`: prepara `z_sql` (que pode ser `None`, falta de memória). Deixa o erro em
/// `p.rc`; devolve o comando.
pub(crate) fn fts5_index_prepare_stmt(
    p: &mut Fts5Index,
    db: &mut Connection,
    z_sql: Option<Vec<u8>>,
) -> Option<StmtId> {
    let mut out = None;
    if p.rc == SQLITE_OK {
        match z_sql {
            Some(z) => {
                let (rc, stmt, _tail) = prepare_v3(db, &z, -1, PREPARE_FLAGS);
                p.rc = rc;
                out = stmt;
            }
            None => p.rc = SQLITE_NOMEM,
        }
    }
    out
}

/// Os argumentos `(zDb, zName)` de um `mprintf` das tabelas sombra.
pub(crate) fn db_name_args(cfg: &Fts5Config) -> [PrintfArg; 2] {
    [
        PrintfArg::Text(Some(cfg.z_db.clone())),
        PrintfArg::Text(Some(cfg.z_name.clone())),
    ]
}

/// `fts5DataRead`: lê um registro de `%_data`. Em erro devolve `None` e deixa o erro em `p.rc`.
pub(crate) fn fts5_data_read(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    i_rowid: i64,
) -> Option<Fts5Data> {
    let mut p_ret: Option<Fts5Data> = None;
    if p.rc == SQLITE_OK {
        let mut rc = SQLITE_OK;

        if p.p_reader.is_none() {
            let z_sql = mprintf(
                b"SELECT block FROM '%q'.'%q' WHERE id=?1",
                &[
                    PrintfArg::Text(Some(cfg.z_db.clone())),
                    PrintfArg::Text(Some(p.z_data_tbl.clone())),
                ],
            );
            match z_sql {
                None => rc = SQLITE_NOMEM,
                Some(z) => {
                    let (rc2, stmt, _tail) = prepare_v3(db, &z, -1, PREPARE_FLAGS);
                    rc = rc2;
                    p.p_reader = stmt;
                }
            }
        }

        if rc == SQLITE_OK {
            if let Some(id) = p.p_reader {
                bind_int64(db, id, 1, i_rowid);
                let rc2 = step(db, id);
                if rc2 == SQLITE_ROW {
                    let t = column_type(db, id, 0);
                    if t == SQLITE_BLOB || t == SQLITE_TEXT {
                        let data = column_blob(db, id, 0).map(|s| s.to_vec()).unwrap_or_default();
                        p_ret = Some(Fts5Data::leaf(data));
                    } else {
                        rc = SQLITE_ERROR;
                    }
                } else if rc2 == SQLITE_DONE {
                    rc = SQLITE_ERROR;
                } else {
                    rc = rc2;
                }
                reset(db, id);
            }
        }

        /* Tabela, linha ou tipo de coluna errados indicam dano na tabela sombra. */
        if rc == SQLITE_ERROR {
            rc = FTS5_CORRUPT;
        }
        p.rc = rc;
        p.n_read += 1;
    }
    p_ret
}

/// `fts5LeafRead`: como `fts5_data_read`, mas confere o cabeçalho da folha.
pub(crate) fn fts5_leaf_read(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    i_rowid: i64,
) -> Option<Fts5Data> {
    let ret = fts5_data_read(p, db, cfg, i_rowid);
    if let Some(d) = ret {
        if d.nn < 4 || d.sz_leaf > d.nn {
            p.rc = FTS5_CORRUPT;
            return None;
        }
        return Some(d);
    }
    None
}

/// `fts5DataWrite`: `REPLACE` de um registro de `%_data`.
pub(crate) fn fts5_data_write(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    i_rowid: i64,
    data: &[u8],
) {
    if p.rc != SQLITE_OK {
        return;
    }
    if p.p_writer.is_none() {
        let z_sql = mprintf(
            b"REPLACE INTO '%q'.'%q_data'(id, block) VALUES(?,?)",
            &db_name_args(cfg),
        );
        p.p_writer = fts5_index_prepare_stmt(p, db, z_sql);
        if p.rc != SQLITE_OK {
            return;
        }
    }
    if let Some(id) = p.p_writer {
        bind_int64(db, id, 1, i_rowid);
        bind_blob(db, id, 2, Some(data), data.len() as i32, StrDtor::Transient);
        step(db, id);
        p.rc = reset(db, id);
        bind_null(db, id, 2);
    }
}

/// `fts5DataDelete`: `DELETE FROM %_data WHERE id>=?1 AND id<=?2`.
pub(crate) fn fts5_data_delete(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    i_first: i64,
    i_last: i64,
) {
    if p.rc != SQLITE_OK {
        return;
    }
    if p.p_deleter.is_none() {
        let z_sql = mprintf(
            b"DELETE FROM '%q'.'%q_data' WHERE id>=? AND id<=?",
            &db_name_args(cfg),
        );
        p.p_deleter = fts5_index_prepare_stmt(p, db, z_sql);
        if p.rc != SQLITE_OK {
            return;
        }
    }
    if let Some(id) = p.p_deleter {
        bind_int64(db, id, 1, i_first);
        bind_int64(db, id, 2, i_last);
        step(db, id);
        p.rc = reset(db, id);
    }
}

/// `fts5DataRemoveSegment`: remove todos os registros do segmento.
pub(crate) fn fts5_data_remove_segment(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    seg: &Fts5StructureSegment,
) {
    let i_segid = seg.i_segid;
    let i_first = fts5_segment_rowid(i_segid, 0);
    let i_last = fts5_segment_rowid(i_segid + 1, 0) - 1;
    fts5_data_delete(p, db, cfg, i_first, i_last);

    if seg.n_pg_tombstone != 0 {
        let i_tomb1 = fts5_tombstone_rowid(i_segid, 0);
        let i_tomb2 = fts5_tombstone_rowid(i_segid, seg.n_pg_tombstone - 1);
        fts5_data_delete(p, db, cfg, i_tomb1, i_tomb2);
    }
    if p.p_idx_deleter.is_none() {
        let z_sql = mprintf(b"DELETE FROM '%q'.'%q_idx' WHERE segid=?", &db_name_args(cfg));
        p.p_idx_deleter = fts5_index_prepare_stmt(p, db, z_sql);
    }
    if p.rc == SQLITE_OK {
        if let Some(id) = p.p_idx_deleter {
            bind_int(db, id, 1, i_segid);
            step(db, id);
            p.rc = reset(db, id);
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Estrutura
// ---------------------------------------------------------------------------------------------

/// `fts5StructureDecode`: decodifica o registro de estrutura `data` (que tem os `n_data` primeiros
/// bytes de dado, seguidos de zeros). Devolve a estrutura e o cookie de configuração.
pub(crate) fn fts5_structure_decode(
    data: &[u8],
    n_data: i32,
) -> Result<(Fts5Structure, i32), i32> {
    let mut rc = SQLITE_OK;
    let mut b_structure_v2 = false;
    let mut n_origin_cntr: u64 = 0;

    /* Lê o cookie de configuração */
    let i_cookie = get_u32(data, 0) as i32;
    let mut i: i32 = 4;

    /* Confere se é um registro de estrutura V2 e, se for, liga `b_structure_v2`. */
    if sub(data, i, 4) == FTS5_STRUCTURE_V2 {
        i += 4;
        b_structure_v2 = true;
    }

    /* Lê o total de níveis e de segmentos do começo do registro de estrutura. */
    let (n, n_level) = gv32(data, i);
    i += n;
    let (n, mut n_segment) = gv32(data, i);
    i += n;
    if n_level > FTS5_MAX_SEGMENT || n_level < 0 || n_segment > FTS5_MAX_SEGMENT || n_segment < 0 {
        return Err(FTS5_CORRUPT);
    }

    let mut ret = Fts5Structure::default();
    ret.n_segment = n_segment;
    ret.a_level = vec![Fts5StructureLevel::default(); n_level as usize];
    let (n, wc) = gv64(data, i);
    i += n;
    ret.n_write_counter = wc;

    let mut i_lvl = 0usize;
    while rc == SQLITE_OK && i_lvl < n_level as usize {
        let mut n_total = 0;

        if i >= n_data {
            rc = FTS5_CORRUPT;
        } else {
            let (n, n_merge) = gv32(data, i);
            i += n;
            ret.a_level[i_lvl].n_merge = n_merge;
            let (n, t) = gv32(data, i);
            i += n;
            n_total = t;
            if n_total < n_merge {
                rc = FTS5_CORRUPT;
            }
            if n_total > n_segment {
                /* O total de segmentos restantes nunca volta a zero: o `nSegment!=0` do fim
                ** acusaria o mesmo erro; falha já aqui para não alocar um vetor gigante. */
                rc = FTS5_CORRUPT;
                n_total = 0;
            }
            ret.a_level[i_lvl].a_seg = vec![Fts5StructureSegment::default(); n_total.max(0) as usize];
            n_segment -= n_total;
        }

        if rc == SQLITE_OK {
            ret.a_level[i_lvl].n_seg = n_total;
            for i_seg in 0..n_total as usize {
                if i >= n_data {
                    rc = FTS5_CORRUPT;
                    break;
                }
                let mut seg = Fts5StructureSegment::default();
                let (n, v) = gv32(data, i);
                i += n;
                seg.i_segid = v;
                let (n, v) = gv32(data, i);
                i += n;
                seg.pgno_first = v;
                let (n, v) = gv32(data, i);
                i += n;
                seg.pgno_last = v;
                if b_structure_v2 {
                    let (n, v) = gv64(data, i);
                    i += n;
                    seg.i_origin1 = v;
                    let (n, v) = gv64(data, i);
                    i += n;
                    seg.i_origin2 = v;
                    let (n, v) = gv32(data, i);
                    i += n;
                    seg.n_pg_tombstone = v;
                    let (n, v) = gv64(data, i);
                    i += n;
                    seg.n_entry_tombstone = v;
                    let (n, v) = gv64(data, i);
                    i += n;
                    seg.n_entry = v;
                    n_origin_cntr = n_origin_cntr.max(seg.i_origin2);
                }
                let bad = seg.pgno_last < seg.pgno_first;
                ret.a_level[i_lvl].a_seg[i_seg] = seg;
                if bad {
                    rc = FTS5_CORRUPT;
                    break;
                }
            }
            if i_lvl > 0 && ret.a_level[i_lvl - 1].n_merge != 0 && n_total == 0 {
                rc = FTS5_CORRUPT;
            }
            if i_lvl == n_level as usize - 1 && ret.a_level[i_lvl].n_merge != 0 {
                rc = FTS5_CORRUPT;
            }
        }
        i_lvl += 1;
    }
    if n_segment != 0 && rc == SQLITE_OK {
        rc = FTS5_CORRUPT;
    }
    if b_structure_v2 {
        ret.n_origin_cntr = n_origin_cntr.wrapping_add(1);
    }

    if rc != SQLITE_OK {
        return Err(rc);
    }
    Ok((ret, i_cookie))
}

/// `fts5StructureAddLevel`: acrescenta um nível.
pub(crate) fn fts5_structure_add_level(p_struct: &mut Fts5Structure) {
    p_struct.a_level.push(Fts5StructureLevel::default());
}

/// `fts5StructureExtendLevel`: garante espaço para mais `n_extra` segmentos no nível `i_lvl`
/// (`b_insert` abre o espaço no começo). `n_seg` não muda, como no C.
pub(crate) fn fts5_structure_extend_level(
    p_struct: &mut Fts5Structure,
    i_lvl: usize,
    n_extra: usize,
    b_insert: bool,
) {
    let lvl = &mut p_struct.a_level[i_lvl];
    let n_seg = lvl.n_seg.max(0) as usize;
    lvl.a_seg.resize(n_seg, Fts5StructureSegment::default());
    if !b_insert {
        lvl.a_seg.resize(n_seg + n_extra, Fts5StructureSegment::default());
    } else {
        let mut v = vec![Fts5StructureSegment::default(); n_extra];
        v.append(&mut lvl.a_seg);
        lvl.a_seg = v;
    }
}

/// `fts5StructureReadUncached`.
fn fts5_structure_read_uncached(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &mut Fts5Config,
) -> Option<Fts5Structure> {
    let mut p_ret: Option<Fts5Structure> = None;
    let p_data = fts5_data_read(p, db, cfg, FTS5_STRUCTURE_ROWID);
    if p.rc == SQLITE_OK {
        if let Some(d) = p_data {
            match fts5_structure_decode(&d.p, d.nn) {
                Err(rc) => p.rc = rc,
                Ok((mut s, i_cookie)) => {
                    if cfg.pgsz == 0 || cfg.i_cookie != i_cookie {
                        p.rc = cfg.load(db, i_cookie);
                    }
                    if p.rc == SQLITE_OK {
                        p.n_struct_id += 1;
                        s.id = p.n_struct_id;
                        p_ret = Some(s);
                    }
                }
            }
        }
    }
    p_ret
}

/// `fts5IndexDataVersion`.
pub(crate) fn fts5_index_data_version(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
) -> i64 {
    let mut i_version = 0;
    if p.rc == SQLITE_OK {
        if p.p_data_version.is_none() {
            let z_sql = mprintf(b"PRAGMA %Q.data_version", &[PrintfArg::Text(Some(cfg.z_db.clone()))]);
            p.p_data_version = fts5_index_prepare_stmt(p, db, z_sql);
            if p.rc != SQLITE_OK {
                return 0;
            }
        }
        if let Some(id) = p.p_data_version {
            if SQLITE_ROW == step(db, id) {
                i_version = column_int64(db, id, 0);
            }
            p.rc = reset(db, id);
        }
    }
    i_version
}

/// `fts5StructureRead`: devolve uma cópia da estrutura corrente (carregando-a se preciso). Em erro
/// devolve `None` e deixa o erro em `p.rc`.
pub(crate) fn fts5_structure_read(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &mut Fts5Config,
) -> Option<Fts5Structure> {
    if p.p_struct.is_none() {
        p.i_struct_version = fts5_index_data_version(p, db, cfg);
        if p.rc == SQLITE_OK {
            p.p_struct = fts5_structure_read_uncached(p, db, cfg);
        }
    }
    if p.rc != SQLITE_OK {
        return None;
    }
    p.p_struct.clone()
}

/// `fts5StructureInvalidate`.
pub(crate) fn fts5_structure_invalidate(p: &mut Fts5Index) {
    p.p_struct = None;
}

/// `fts5StructureWrite`: serializa e grava o registro de estrutura.
pub(crate) fn fts5_structure_write(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    p_struct: &Fts5Structure,
) {
    if p.rc == SQLITE_OK {
        let mut buf = Fts5Buffer::new();

        /* Acrescenta o cookie de configuração corrente */
        let mut i_cookie = cfg.i_cookie;
        if i_cookie < 0 {
            i_cookie = 0;
        }
        buf.p.extend_from_slice(&(i_cookie as u32).to_be_bytes());
        if p_struct.n_origin_cntr > 0 {
            buf.append_blob(&FTS5_STRUCTURE_V2);
        }
        buf.append_varint(p_struct.n_level() as i64);
        buf.append_varint(p_struct.n_segment as i64);
        buf.append_varint(p_struct.n_write_counter as i64);

        for lvl in p_struct.a_level.iter() {
            buf.append_varint(lvl.n_merge as i64);
            buf.append_varint(lvl.n_seg as i64);
            for i_seg in 0..lvl.n_seg.max(0) as usize {
                let seg = &lvl.a_seg[i_seg];
                buf.append_varint(seg.i_segid as i64);
                buf.append_varint(seg.pgno_first as i64);
                buf.append_varint(seg.pgno_last as i64);
                if p_struct.n_origin_cntr > 0 {
                    buf.append_varint(seg.i_origin1 as i64);
                    buf.append_varint(seg.i_origin2 as i64);
                    buf.append_varint(seg.n_pg_tombstone as i64);
                    buf.append_varint(seg.n_entry_tombstone as i64);
                    buf.append_varint(seg.n_entry as i64);
                }
            }
        }

        fts5_data_write(p, db, cfg, FTS5_STRUCTURE_ROWID, &buf.p);
    }
}

/// `fts5SegmentSize`.
fn fts5_segment_size(seg: &Fts5StructureSegment) -> i32 {
    1 + seg.pgno_last - seg.pgno_first
}

/// `fts5StructurePromoteTo`: promove ao nível `i_promote` todos os segmentos que couberem.
fn fts5_structure_promote_to(
    p: &mut Fts5Index,
    i_promote: usize,
    sz_promote: i32,
    p_struct: &mut Fts5Structure,
) {
    if p_struct.a_level[i_promote].n_merge == 0 {
        for il in i_promote + 1..p_struct.a_level.len() {
            if p_struct.a_level[il].n_merge != 0 {
                return;
            }
            let mut is = p_struct.a_level[il].n_seg - 1;
            while is >= 0 {
                let seg = p_struct.a_level[il].a_seg[is as usize].clone();
                let sz = fts5_segment_size(&seg);
                if sz > sz_promote {
                    return;
                }
                fts5_structure_extend_level(p_struct, i_promote, 1, true);
                if p.rc != SQLITE_OK {
                    return;
                }
                p_struct.a_level[i_promote].a_seg[0] = seg;
                p_struct.a_level[i_promote].n_seg += 1;
                p_struct.a_level[il].n_seg -= 1;
                is -= 1;
            }
        }
    }
}

/// `fts5StructurePromote`: um segmento acabou de ser escrito no nível `i_lvl`; promove segmentos
/// se for o caso.
pub(crate) fn fts5_structure_promote(p: &mut Fts5Index, i_lvl: usize, p_struct: &mut Fts5Structure) {
    if p.rc == SQLITE_OK {
        let mut i_promote: i32 = -1;
        let mut sz_promote = 0;
        let n_seg = p_struct.a_level[i_lvl].n_seg;

        if n_seg == 0 {
            return;
        }
        let p_seg = &p_struct.a_level[i_lvl].a_seg[(n_seg - 1) as usize];
        let sz_seg = 1 + p_seg.pgno_last - p_seg.pgno_first;

        /* Confere a condição (a) */
        let mut i_tst: i32 = i_lvl as i32 - 1;
        while i_tst >= 0 && p_struct.a_level[i_tst as usize].n_seg == 0 {
            i_tst -= 1;
        }
        if i_tst >= 0 {
            let mut sz_max = 0;
            let p_tst = &p_struct.a_level[i_tst as usize];
            for i in 0..p_tst.n_seg as usize {
                let sz = p_tst.a_seg[i].pgno_last - p_tst.a_seg[i].pgno_first + 1;
                if sz > sz_max {
                    sz_max = sz;
                }
            }
            if sz_max >= sz_seg {
                /* A condição (a) vale: promove o segmento mais novo do nível `i_lvl` ao nível
                ** `i_tst`. */
                i_promote = i_tst;
                sz_promote = sz_max;
            }
        }

        /* Se a condição (a) não vale, supõe-se que (b) vale; `fts5_structure_promote_to` não faz
        ** nada se não for o caso. */
        if i_promote < 0 {
            i_promote = i_lvl as i32;
            sz_promote = sz_seg;
        }
        fts5_structure_promote_to(p, i_promote as usize, sz_promote, p_struct);
    }
}

// ---------------------------------------------------------------------------------------------
// Índice de doclist (dlidx)
// ---------------------------------------------------------------------------------------------

/// `fts5DlidxLvlNext`: avança; devolve não zero no fim da página do índice.
pub(crate) fn fts5_dlidx_lvl_next(lvl: &mut Fts5DlidxLvl) -> i32 {
    let data = match lvl.p_data.as_ref() {
        Some(d) => d,
        None => {
            lvl.b_eof = 1;
            return 1;
        }
    };

    if lvl.i_off == 0 {
        lvl.i_off = 1;
        let (n, pg) = gv32(&data.p, 1);
        lvl.i_off += n;
        lvl.i_leaf_pgno = pg;
        let (n2, rowid) = gv64(&data.p, lvl.i_off);
        lvl.i_off += n2;
        lvl.i_rowid = rowid as i64;
        lvl.i_first_off = lvl.i_off;
    } else {
        let mut i_off = lvl.i_off;
        while i_off < data.nn {
            if at(&data.p, ux(i_off)) != 0 {
                break;
            }
            i_off += 1;
        }

        if i_off < data.nn {
            let (n, i_val) = gv64(&data.p, i_off);
            lvl.i_leaf_pgno += (i_off - lvl.i_off) + 1;
            i_off += n;
            lvl.i_rowid = lvl.i_rowid.wrapping_add(i_val as i64);
            lvl.i_off = i_off;
        } else {
            lvl.b_eof = 1;
        }
    }

    lvl.b_eof
}

/// `fts5DlidxIterNextR`.
fn fts5_dlidx_iter_next_r(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5DlidxIter,
    i_lvl: usize,
) -> i32 {
    if fts5_dlidx_lvl_next(&mut it.a_lvl[i_lvl]) != 0 && i_lvl + 1 < it.a_lvl.len() {
        fts5_dlidx_iter_next_r(p, db, cfg, it, i_lvl + 1);
        if it.a_lvl[i_lvl + 1].b_eof == 0 {
            let pgno = it.a_lvl[i_lvl + 1].i_leaf_pgno;
            let segid = it.i_segid;
            it.a_lvl[i_lvl] = Fts5DlidxLvl::default();
            it.a_lvl[i_lvl].p_data =
                fts5_data_read(p, db, cfg, fts5_dlidx_rowid(segid, i_lvl as i32, pgno));
            if it.a_lvl[i_lvl].p_data.is_some() {
                fts5_dlidx_lvl_next(&mut it.a_lvl[i_lvl]);
            }
        }
    }
    it.a_lvl[0].b_eof
}

/// `fts5DlidxIterNext`.
pub(crate) fn fts5_dlidx_iter_next(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5DlidxIter,
) -> i32 {
    fts5_dlidx_iter_next_r(p, db, cfg, it, 0)
}

/// `fts5DlidxIterFirst`.
fn fts5_dlidx_iter_first(it: &mut Fts5DlidxIter) -> i32 {
    for lvl in it.a_lvl.iter_mut() {
        fts5_dlidx_lvl_next(lvl);
    }
    it.a_lvl[0].b_eof
}

/// `fts5DlidxIterEof`.
pub(crate) fn fts5_dlidx_iter_eof(p: &Fts5Index, it: &Fts5DlidxIter) -> bool {
    p.rc != SQLITE_OK || it.a_lvl[0].b_eof != 0
}

/// `fts5DlidxIterLast`.
fn fts5_dlidx_iter_last(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5DlidxIter,
) {
    /* Leva cada nível até a última entrada da última página */
    let mut i = it.a_lvl.len() as i32 - 1;
    while p.rc == SQLITE_OK && i >= 0 {
        let iu = i as usize;
        while fts5_dlidx_lvl_next(&mut it.a_lvl[iu]) == 0 {}
        it.a_lvl[iu].b_eof = 0;

        if i > 0 {
            let pgno = it.a_lvl[iu].i_leaf_pgno;
            let segid = it.i_segid;
            it.a_lvl[iu - 1] = Fts5DlidxLvl::default();
            it.a_lvl[iu - 1].p_data =
                fts5_data_read(p, db, cfg, fts5_dlidx_rowid(segid, i - 1, pgno));
        }
        i -= 1;
    }
}

/// `fts5DlidxLvlPrev`: move para a entrada anterior.
fn fts5_dlidx_lvl_prev(lvl: &mut Fts5DlidxLvl) -> i32 {
    let i_off = lvl.i_off;

    if i_off <= lvl.i_first_off {
        lvl.b_eof = 1;
    } else {
        let a: Vec<u8> = lvl.p_data.as_ref().map(|d| d.p.clone()).unwrap_or_default();

        lvl.i_off = 0;
        fts5_dlidx_lvl_next(lvl);
        loop {
            let mut n_zero = 0;
            let mut ii = lvl.i_off;

            while ux(ii) < a.len() && a[ux(ii)] == 0 {
                n_zero += 1;
                ii += 1;
            }
            let (n, delta) = gv64(&a, ii);
            ii += n;

            if ii >= i_off {
                break;
            }
            lvl.i_leaf_pgno += n_zero + 1;
            lvl.i_rowid = lvl.i_rowid.wrapping_add(delta as i64);
            lvl.i_off = ii;
        }
    }

    lvl.b_eof
}

/// `fts5DlidxIterPrevR`.
fn fts5_dlidx_iter_prev_r(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5DlidxIter,
    i_lvl: usize,
) -> i32 {
    if fts5_dlidx_lvl_prev(&mut it.a_lvl[i_lvl]) != 0 && i_lvl + 1 < it.a_lvl.len() {
        fts5_dlidx_iter_prev_r(p, db, cfg, it, i_lvl + 1);
        if it.a_lvl[i_lvl + 1].b_eof == 0 {
            let pgno = it.a_lvl[i_lvl + 1].i_leaf_pgno;
            let segid = it.i_segid;
            it.a_lvl[i_lvl] = Fts5DlidxLvl::default();
            it.a_lvl[i_lvl].p_data =
                fts5_data_read(p, db, cfg, fts5_dlidx_rowid(segid, i_lvl as i32, pgno));
            if it.a_lvl[i_lvl].p_data.is_some() {
                while fts5_dlidx_lvl_next(&mut it.a_lvl[i_lvl]) == 0 {}
                it.a_lvl[i_lvl].b_eof = 0;
            }
        }
    }
    it.a_lvl[0].b_eof
}

/// `fts5DlidxIterPrev`.
pub(crate) fn fts5_dlidx_iter_prev(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5DlidxIter,
) -> i32 {
    fts5_dlidx_iter_prev_r(p, db, cfg, it, 0)
}

/// `fts5DlidxIterInit`: abre o iterador do índice de doclist da folha `i_leaf_pg`.
pub(crate) fn fts5_dlidx_iter_init(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    b_rev: bool,
    i_segid: i32,
    i_leaf_pg: i32,
) -> Option<Box<Fts5DlidxIter>> {
    let mut it = Fts5DlidxIter::default();
    let mut b_done = false;
    let mut i = 0;

    while p.rc == SQLITE_OK && !b_done {
        let i_rowid = fts5_dlidx_rowid(i_segid, i, i_leaf_pg);
        let mut lvl = Fts5DlidxLvl::default();
        lvl.p_data = fts5_data_read(p, db, cfg, i_rowid);
        if let Some(d) = lvl.p_data.as_ref() {
            if (at(&d.p, 0) & 0x0001) == 0 {
                b_done = true;
            }
        }
        it.a_lvl.push(lvl);
        i += 1;
    }

    if p.rc == SQLITE_OK && !it.a_lvl.is_empty() {
        it.i_segid = i_segid;
        if !b_rev {
            fts5_dlidx_iter_first(&mut it);
        } else {
            fts5_dlidx_iter_last(p, db, cfg, &mut it);
        }
    }

    if p.rc != SQLITE_OK {
        return None;
    }
    Some(Box::new(it))
}

/// `fts5DlidxIterRowid`.
#[inline]
pub(crate) fn fts5_dlidx_iter_rowid(it: &Fts5DlidxIter) -> i64 {
    it.a_lvl[0].i_rowid
}

/// `fts5DlidxIterPgno`.
#[inline]
pub(crate) fn fts5_dlidx_iter_pgno(it: &Fts5DlidxIter) -> i32 {
    it.a_lvl[0].i_leaf_pgno
}

// ---------------------------------------------------------------------------------------------
// Iteradores de segmento
// ---------------------------------------------------------------------------------------------

/// `fts5SegIterNextPage`: carrega a próxima folha no iterador.
pub(crate) fn fts5_seg_iter_next_page(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5SegIter,
) {
    let (segid, pgno_last) = match &it.p_seg {
        Some(s) => (s.i_segid, s.pgno_last),
        None => (0, 0),
    };
    it.p_leaf = None;
    it.i_leaf_pgno += 1;
    if let Some(next) = it.p_next_leaf.take() {
        it.p_leaf = Some(next);
    } else if it.i_leaf_pgno <= pgno_last {
        it.p_leaf = fts5_leaf_read(p, db, cfg, fts5_segment_rowid(segid, it.i_leaf_pgno));
    } else {
        it.p_leaf = None;
    }

    if let Some(leaf) = it.p_leaf.as_ref() {
        it.i_pgidx_off = leaf.sz_leaf;
        if fts5_leaf_is_termless(leaf) {
            it.i_endof_doclist = leaf.nn + 1;
        } else {
            let (n, v) = gv32(&leaf.p, it.i_pgidx_off);
            it.i_pgidx_off += n;
            it.i_endof_doclist = v;
        }
    }
}

/// `fts5GetPoslistSize`: devolve `(bytes lidos, nSz/2, bDel)`.
pub(crate) fn fts5_get_poslist_size(a: &[u8], off: i32) -> (i32, i32, i32) {
    let mut n = off;
    let n_sz = fast32(a, &mut n);
    (n - off, n_sz / 2, n_sz & 0x0001)
}

/// `fts5SegIterLoadNPos`: lê o campo de tamanho da poslist em `i_leaf_offset`.
pub(crate) fn fts5_seg_iter_load_npos(p: &mut Fts5Index, cfg: &Fts5Config, it: &mut Fts5SegIter) {
    if p.rc == SQLITE_OK {
        let leaf = match it.p_leaf.as_ref() {
            Some(l) => l,
            None => return,
        };
        let mut i_off = it.i_leaf_offset as i32;
        if cfg.e_detail == FTS5_DETAIL_NONE {
            let i_eod = it.i_endof_doclist.min(leaf.sz_leaf);
            it.b_del = 0;
            it.n_pos = 1;
            if i_off < i_eod && at(&leaf.p, ux(i_off)) == 0 {
                it.b_del = 1;
                i_off += 1;
                if i_off < i_eod && at(&leaf.p, ux(i_off)) == 0 {
                    it.n_pos = 1;
                    i_off += 1;
                } else {
                    it.n_pos = 0;
                }
            }
        } else {
            let n_sz = fast32(&leaf.p, &mut i_off);
            it.b_del = (n_sz & 0x0001) as u8;
            it.n_pos = n_sz >> 1;
        }
        it.i_leaf_offset = i_off as i64;
    }
}

/// `fts5SegIterLoadRowid`.
pub(crate) fn fts5_seg_iter_load_rowid(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5SegIter,
) {
    if it.p_leaf.is_none() {
        return;
    }
    let mut i_off = it.i_leaf_offset;
    while i_off >= it.p_leaf.as_ref().map_or(0, |l| l.sz_leaf) as i64 {
        fts5_seg_iter_next_page(p, db, cfg, it);
        if it.p_leaf.is_none() {
            if p.rc == SQLITE_OK {
                p.rc = FTS5_CORRUPT;
            }
            return;
        }
        i_off = 4;
    }
    if let Some(leaf) = it.p_leaf.as_ref() {
        let (n, v) = gv64(&leaf.p, i_off as i32);
        it.i_rowid = v as i64;
        it.i_leaf_offset = i_off + n as i64;
    }
}

/// `fts5SegIterLoadTerm`: `i_leaf_offset` aponta o campo `nSuffix` de um termo; `n_keep` é o
/// `nPrefix` (0 para o primeiro termo do segmento). Preenche `term` e `i_rowid`.
pub(crate) fn fts5_seg_iter_load_term(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5SegIter,
    n_keep: i32,
) {
    {
        let leaf = match it.p_leaf.as_ref() {
            Some(l) => l,
            None => return,
        };
        let a = &leaf.p;
        let mut i_off = it.i_leaf_offset;

        let (n, n_new) = gv32(a, i_off as i32);
        i_off += n as i64;
        if i_off + n_new as i64 > leaf.sz_leaf as i64 || n_keep > it.term.n() || n_new == 0 {
            p.rc = FTS5_CORRUPT;
            return;
        }
        it.term.p.truncate(n_keep as usize);
        it.term.append_blob(sub(a, i_off as i32, n_new));
        i_off += n_new as i64;
        it.i_term_leaf_offset = i_off as i32;
        it.i_term_leaf_pgno = it.i_leaf_pgno;
        it.i_leaf_offset = i_off;

        if it.i_pgidx_off >= leaf.nn {
            it.i_endof_doclist = leaf.nn + 1;
        } else {
            let (n, n_extra) = gv32(a, it.i_pgidx_off);
            it.i_pgidx_off += n;
            it.i_endof_doclist += n_extra;
        }
    }

    fts5_seg_iter_load_rowid(p, db, cfg, it);
}

/// `fts5SegIterSetNext`.
pub(crate) fn fts5_seg_iter_set_next(cfg: &Fts5Config, it: &mut Fts5SegIter) {
    if it.flags & FTS5_SEGITER_REVERSE != 0 {
        it.x_next = SegNext::Reverse;
    } else if cfg.e_detail == FTS5_DETAIL_NONE {
        it.x_next = SegNext::NoDetail;
    } else {
        it.x_next = SegNext::Normal;
    }
}

/// `fts5SegIterAllocTombstone`: aloca o vetor de páginas de tombstone do iterador.
pub(crate) fn fts5_seg_iter_alloc_tombstone(it: &mut Fts5SegIter) {
    let n_tomb = it.p_seg.as_ref().map_or(0, |s| s.n_pg_tombstone);
    if n_tomb > 0 {
        it.p_tomb_array = Some(Rc::new(RefCell::new(Fts5TombstoneArray {
            n_tombstone: n_tomb,
            ap_tombstone: (0..n_tomb).map(|_| None).collect(),
        })));
    }
}

/// `fts5SegIterInit`: inicia o iterador no primeiro termo do segmento.
pub(crate) fn fts5_seg_iter_init(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    seg: &Fts5StructureSegment,
    it: &mut Fts5SegIter,
) {
    if seg.pgno_first == 0 {
        /* Acontece se o segmento é entrada de um merge incremental e todos os dados já foram
        ** aparados ("trimmed"). Nesse caso o iterador fica vazio. */
        return;
    }

    if p.rc == SQLITE_OK {
        *it = Fts5SegIter::default();
        fts5_seg_iter_set_next(cfg, it);
        it.p_seg = Some(seg.clone());
        it.i_leaf_pgno = seg.pgno_first - 1;
        loop {
            fts5_seg_iter_next_page(p, db, cfg, it);
            let go = p.rc == SQLITE_OK && it.p_leaf.as_ref().map_or(false, |l| l.nn == 4);
            if !go {
                break;
            }
        }
    }

    if p.rc == SQLITE_OK && it.p_leaf.is_some() {
        it.i_leaf_offset = 4;
        it.i_pgidx_off = it.p_leaf.as_ref().map_or(0, |l| l.sz_leaf) + 1;
        fts5_seg_iter_load_term(p, db, cfg, it, 0);
        fts5_seg_iter_load_npos(p, cfg, it);
        fts5_seg_iter_alloc_tombstone(it);
    }
}

/// `fts5SegIterReverseInitPage`: só para iteradores de consulta `DESC`.
pub(crate) fn fts5_seg_iter_reverse_init_page(
    p: &mut Fts5Index,
    cfg: &Fts5Config,
    it: &mut Fts5SegIter,
) {
    let e_detail = cfg.e_detail;
    {
        let leaf = match it.p_leaf.as_ref() {
            Some(l) => l,
            None => return,
        };
        let mut n = leaf.sz_leaf;
        let mut i = it.i_leaf_offset as i32;
        let a = &leaf.p;
        let mut i_rowid_offset: i32 = 0;

        if n > it.i_endof_doclist {
            n = it.i_endof_doclist;
        }

        loop {
            if e_detail == FTS5_DETAIL_NONE {
                if i < n && at(a, ux(i)) == 0 {
                    i += 1;
                    if i < n && at(a, ux(i)) == 0 {
                        i += 1;
                    }
                }
            } else {
                let (nb, n_pos, _b_dummy) = fts5_get_poslist_size(a, i);
                i += nb;
                i += n_pos;
            }
            if i >= n {
                break;
            }
            let (nb, i_delta) = gv64(a, i);
            i += nb;
            it.i_rowid = it.i_rowid.wrapping_add(i_delta as i64);

            /* Se preciso, cresce o vetor `a_rowid_offset`. */
            if i_rowid_offset >= it.n_rowid_offset {
                let n_new = it.n_rowid_offset + 8;
                it.a_rowid_offset.resize(n_new as usize, 0);
                it.n_rowid_offset = n_new;
            }

            it.a_rowid_offset[i_rowid_offset as usize] = it.i_leaf_offset as i32;
            i_rowid_offset += 1;
            it.i_leaf_offset = i as i64;
        }
        it.i_rowid_offset = i_rowid_offset;
    }
    fts5_seg_iter_load_npos(p, cfg, it);
}

/// `fts5SegIterReverseNewPage`.
pub(crate) fn fts5_seg_iter_reverse_new_page(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5SegIter,
) {
    let segid = it.p_seg.as_ref().map_or(0, |s| s.i_segid);
    it.p_leaf = None;
    while p.rc == SQLITE_OK && it.i_leaf_pgno > it.i_term_leaf_pgno {
        it.i_leaf_pgno -= 1;
        let p_new = fts5_data_read(p, db, cfg, fts5_segment_rowid(segid, it.i_leaf_pgno));
        if let Some(new) = p_new {
            /* `i_term_leaf_offset` pode ser igual a `sz_leaf` se o termo é a última coisa da
            ** página, isto é, o primeiro rowid está na página seguinte. Nesse caso `p_leaf`
            ** fica `None` e este iterador está no fim. */
            let mut keep = false;
            if it.i_leaf_pgno == it.i_term_leaf_pgno {
                if it.i_term_leaf_offset < new.sz_leaf {
                    keep = true;
                    it.i_leaf_offset = it.i_term_leaf_offset as i64;
                }
            } else {
                let i_rowid_off = fts5_leaf_first_rowid_off(&new);
                if i_rowid_off != 0 {
                    if i_rowid_off >= new.sz_leaf {
                        p.rc = FTS5_CORRUPT;
                    } else {
                        keep = true;
                        it.i_leaf_offset = i_rowid_off as i64;
                    }
                }
            }

            if keep {
                let (n, v) = gv64(&new.p, it.i_leaf_offset as i32);
                it.i_rowid = v as i64;
                it.i_leaf_offset += n as i64;
                it.p_leaf = Some(new);
                break;
            }
        }
    }

    if let Some(leaf) = it.p_leaf.as_ref() {
        it.i_endof_doclist = leaf.nn + 1;
        fts5_seg_iter_reverse_init_page(p, cfg, it);
    }
}

/// `fts5SegIterNext_Reverse`: só para iteradores reversos.
pub(crate) fn fts5_seg_iter_next_reverse(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5SegIter,
) {
    if it.i_rowid_offset > 0 {
        it.i_rowid_offset -= 1;
        it.i_leaf_offset = it.a_rowid_offset[it.i_rowid_offset as usize] as i64;
        fts5_seg_iter_load_npos(p, cfg, it);
        let mut i_off = it.i_leaf_offset as i32;
        if cfg.e_detail != FTS5_DETAIL_NONE {
            i_off += it.n_pos;
        }
        let delta = it.p_leaf.as_ref().map_or(0, |l| gv64(&l.p, i_off).1);
        it.i_rowid = it.i_rowid.wrapping_sub(delta as i64);
    } else {
        fts5_seg_iter_reverse_new_page(p, db, cfg, it);
    }
}

/// Instala no iterador do hash em memória a doclist `list` (o `pLeaf->p = pList` do C).
fn fts5_seg_iter_hash_set_leaf(it: &mut Fts5SegIter, z_term: &[u8], list: Vec<u8>, end_plus_one: bool) {
    let n_list = list.len() as i32;
    let leaf = Fts5Data::doclist(list);
    it.i_leaf_offset = gv64(&leaf.p, 0).0 as i64;
    it.i_rowid = gv64(&leaf.p, 0).1 as i64;
    it.i_endof_doclist = if end_plus_one { n_list + 1 } else { n_list };
    it.term.set(z_term);
    it.p_leaf = Some(leaf);
}

/// `fts5SegIterNext_None`: avança; só para `detail=none` e sem ser reverso.
pub(crate) fn fts5_seg_iter_next_none(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5SegIter,
    mut pb_new_term: Option<&mut i32>,
) {
    let mut i_off = it.i_leaf_offset as i32;

    /* A próxima entrada está na página seguinte */
    while it.p_seg.is_some() && i_off >= it.p_leaf.as_ref().map_or(0, |l| l.sz_leaf) {
        fts5_seg_iter_next_page(p, db, cfg, it);
        if p.rc != SQLITE_OK || it.p_leaf.is_none() {
            return;
        }
        it.i_rowid = 0;
        i_off = 4;
    }

    if i_off < it.i_endof_doclist {
        /* A próxima entrada está na página corrente */
        if let Some(leaf) = it.p_leaf.as_ref() {
            let (n, i_delta) = gv64(&leaf.p, i_off);
            i_off += n;
            it.i_leaf_offset = i_off as i64;
            it.i_rowid = it.i_rowid.wrapping_add(i_delta as i64);
        }
    } else if it.flags & FTS5_SEGITER_ONETERM == 0 {
        if it.p_seg.is_some() {
            let mut n_keep = 0;
            if let Some(leaf) = it.p_leaf.as_ref() {
                if i_off != fts5_leaf_first_term_off(leaf) {
                    let (n, k) = gv32(&leaf.p, i_off);
                    i_off += n;
                    n_keep = k;
                }
            }
            it.i_leaf_offset = i_off as i64;
            fts5_seg_iter_load_term(p, db, cfg, it, n_keep);
        } else {
            let mut got: Option<(Vec<u8>, Vec<u8>)> = None;
            if let Some(h) = p.p_hash.as_mut() {
                h.scan_next();
                got = h.scan_entry().map(|(z, l)| (z.to_vec(), l.to_vec()));
            }
            match got {
                None => {
                    it.p_leaf = None;
                    return;
                }
                Some((z_term, list)) => {
                    fts5_seg_iter_hash_set_leaf(it, &z_term, list, false);
                }
            }
        }

        if let Some(b) = pb_new_term.as_deref_mut() {
            *b = 1;
        }
    } else {
        it.p_leaf = None;
        return;
    }

    fts5_seg_iter_load_npos(p, cfg, it);
}

/// `fts5SegIterNext`: avança o iterador para a próxima entrada. Não é erro chegar ao fim.
pub(crate) fn fts5_seg_iter_next(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5SegIter,
    mut pb_new_term: Option<&mut i32>,
) {
    let mut b_new_term = false;
    let mut n_keep = 0;

    let (sz_leaf, nn) = match it.p_leaf.as_ref() {
        Some(l) => (l.sz_leaf, l.nn),
        None => return,
    };

    /* Procura o fim da poslist dentro da página corrente. */
    let mut i_off: i32 = (it.i_leaf_offset as i32).wrapping_add(it.n_pos);

    if i_off < sz_leaf {
        /* A próxima entrada está na página corrente. */
        let leaf = it.p_leaf.as_ref().unwrap();
        if i_off >= it.i_endof_doclist {
            b_new_term = true;
            if i_off != fts5_leaf_first_term_off(leaf) {
                let (n, k) = gv32(&leaf.p, i_off);
                i_off += n;
                n_keep = k;
            }
        } else {
            let (n, i_delta) = gv64(&leaf.p, i_off);
            i_off += n;
            it.i_rowid = it.i_rowid.wrapping_add(i_delta as i64);
        }
        it.i_leaf_offset = i_off as i64;
    } else if it.p_seg.is_none() {
        let mut got: Option<(Vec<u8>, Vec<u8>)> = None;
        if it.flags & FTS5_SEGITER_ONETERM == 0 {
            if let Some(h) = p.p_hash.as_mut() {
                h.scan_next();
                got = h.scan_entry().map(|(z, l)| (z.to_vec(), l.to_vec()));
            }
        }
        match got {
            None => it.p_leaf = None,
            Some((z_term, list)) => {
                fts5_seg_iter_hash_set_leaf(it, &z_term, list, true);
                if let Some(b) = pb_new_term.as_deref_mut() {
                    *b = 1;
                }
            }
        }
    } else {
        i_off = 0;
        /* A próxima entrada não está na página corrente */
        while i_off == 0 {
            fts5_seg_iter_next_page(p, db, cfg, it);
            let leaf = match it.p_leaf.as_ref() {
                Some(l) => l,
                None => break,
            };
            let (sz, nnl) = (leaf.sz_leaf, leaf.nn);
            i_off = fts5_leaf_first_rowid_off(leaf);
            if i_off != 0 && i_off < sz {
                let (n, v) = gv64(&leaf.p, i_off);
                it.i_rowid = v as i64;
                i_off += n;
                it.i_leaf_offset = i_off as i64;

                if nnl > sz {
                    let (n2, e) = gv32(&leaf.p, sz);
                    it.i_pgidx_off = sz + n2;
                    it.i_endof_doclist = e;
                }
            } else if nnl > sz {
                let (n2, o) = gv32(&leaf.p, sz);
                it.i_pgidx_off = sz + n2;
                i_off = o;
                it.i_leaf_offset = i_off as i64;
                it.i_endof_doclist = i_off;
                b_new_term = true;
            }
            if i_off > sz {
                p.rc = FTS5_CORRUPT;
                return;
            }
        }
    }
    let _ = nn;

    /* Confere se o iterador chegou ao fim; se sim, termina aqui. */
    if it.p_leaf.is_some() {
        if b_new_term {
            if it.flags & FTS5_SEGITER_ONETERM != 0 {
                it.p_leaf = None;
            } else {
                fts5_seg_iter_load_term(p, db, cfg, it, n_keep);
                fts5_seg_iter_load_npos(p, cfg, it);
                if let Some(b) = pb_new_term.as_deref_mut() {
                    *b = 1;
                }
            }
        } else if let Some(leaf) = it.p_leaf.as_ref() {
            /* Equivale a fts5SegIterLoadNPos(): este bloco é crítico para o desempenho. */
            let mut off = it.i_leaf_offset as i32;
            let n_sz = fast32(&leaf.p, &mut off);
            it.i_leaf_offset = off as i64;
            it.b_del = (n_sz & 0x0001) as u8;
            it.n_pos = n_sz >> 1;
        }
    }
}

/// O `pIter->xNext(p, pIter, pbNewTerm)` do C.
pub(crate) fn fts5_seg_iter_x_next(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5SegIter,
    pb_new_term: Option<&mut i32>,
) {
    match it.x_next {
        SegNext::Normal => fts5_seg_iter_next(p, db, cfg, it, pb_new_term),
        SegNext::Reverse => fts5_seg_iter_next_reverse(p, db, cfg, it),
        SegNext::NoDetail => fts5_seg_iter_next_none(p, db, cfg, it, pb_new_term),
    }
}

/// `fts5SegIterReverse`: o iterador aponta o primeiro rowid de uma doclist; configura a iteração
/// em ordem decrescente.
pub(crate) fn fts5_seg_iter_reverse(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5SegIter,
) {
    let mut p_last: Option<Fts5Data> = None;
    let mut pgno_last = 0;
    let (segid, seg_pgno_last) = it
        .p_seg
        .as_ref()
        .map_or((0, 0), |s| (s.i_segid, s.pgno_last));

    if it.p_dlidx.is_some() && cfg.i_version == FTS5_CURRENT_VERSION {
        pgno_last = it.p_dlidx.as_ref().map_or(0, |d| fts5_dlidx_iter_pgno(d));
        p_last = fts5_leaf_read(p, db, cfg, fts5_segment_rowid(segid, pgno_last));
    } else if it.p_leaf.is_some() {
        let (sz_leaf, i_poslist0) = {
            let leaf = it.p_leaf.as_ref().unwrap();
            let ip = if it.i_term_leaf_pgno == it.i_leaf_pgno {
                it.i_term_leaf_offset
            } else {
                4
            };
            (leaf.sz_leaf, ip)
        };

        /* Agora `i_leaf_offset` aponta o primeiro byte do conteúdo da poslist do rowid corrente.
        ** Recua-o para o começo do campo de tamanho da poslist. */
        let mut i_poslist = i_poslist0;
        {
            let leaf = it.p_leaf.as_ref().unwrap();
            /* fts5IndexSkipVarint */
            let i_end = i_poslist + 9;
            loop {
                let b = at(&leaf.p, ux(i_poslist));
                i_poslist += 1;
                if !((b & 0x80) != 0 && i_poslist < i_end) {
                    break;
                }
            }
        }
        it.i_leaf_offset = i_poslist as i64;

        /* Se vale, o maior rowid do termo corrente pode não estar na página corrente: procura
        ** adiante onde ele de fato está. */
        if it.i_endof_doclist >= sz_leaf {
            let mut pgno = it.i_leaf_pgno + 1;
            while p.rc == SQLITE_OK && pgno <= seg_pgno_last {
                let i_abs = fts5_segment_rowid(segid, pgno);
                if let Some(p_new) = fts5_leaf_read(p, db, cfg, i_abs) {
                    let i_rowid = fts5_leaf_first_rowid_off(&p_new);
                    let b_termless = fts5_leaf_is_termless(&p_new);
                    if i_rowid != 0 {
                        p_last = Some(p_new);
                        pgno_last = pgno;
                    }
                    if !b_termless {
                        break;
                    }
                }
                pgno += 1;
            }
        }
    }

    /* Se `p_last` é `None` aqui, o último rowid da doclist está na página que o iterador já
    ** indica. Senão, `p_last` é a página que contém o último rowid: configura o iterador para
    ** apontar o primeiro rowid dela. */
    if let Some(last) = p_last {
        it.i_leaf_pgno = pgno_last;
        let mut i_off = fts5_leaf_first_rowid_off(&last);
        if i_off > last.sz_leaf {
            it.p_leaf = Some(last);
            p.rc = FTS5_CORRUPT;
            return;
        }
        let (n, v) = gv64(&last.p, i_off);
        it.i_rowid = v as i64;
        i_off += n;
        it.i_leaf_offset = i_off as i64;

        if fts5_leaf_is_termless(&last) {
            it.i_endof_doclist = last.nn + 1;
        } else {
            it.i_endof_doclist = fts5_leaf_first_term_off(&last);
        }
        it.p_leaf = Some(last);
    }

    fts5_seg_iter_reverse_init_page(p, cfg, it);
}

/// `fts5SegIterLoadDlidx`: se o termo corrente é o último da página, carrega o índice de doclist.
pub(crate) fn fts5_seg_iter_load_dlidx(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5SegIter,
) {
    let i_seg = it.p_seg.as_ref().map_or(0, |s| s.i_segid);
    let b_rev = it.flags & FTS5_SEGITER_REVERSE != 0;

    /* Se a doclist corrente termina nesta página, volta sem carregar o índice de doclist (ele
    ** pertence a outro termo). */
    if let Some(leaf) = it.p_leaf.as_ref() {
        if it.i_term_leaf_pgno == it.i_leaf_pgno && it.i_endof_doclist < leaf.sz_leaf {
            return;
        }
    }

    it.p_dlidx = fts5_dlidx_iter_init(p, db, cfg, b_rev, i_seg, it.i_term_leaf_pgno);
}

/// `fts5LeafSeek`: procura `term` na folha corrente do iterador.
fn fts5_leaf_seek(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    b_ge: bool,
    it: &mut Fts5SegIter,
    term: &[u8],
) {
    let n_term = term.len() as u32;
    let (mut a, mut n, sz_leaf0) = match it.p_leaf.as_ref() {
        Some(l) => (l.p.clone(), l.nn as u32, l.sz_leaf),
        None => return,
    };

    let mut n_match: u32 = 0;
    let mut n_keep: u32 = 0;
    let mut n_new: u32 = 0;
    let mut b_end_of_page = false;

    let mut i_pgidx: u32 = sz_leaf0 as u32;
    let (nb, t) = gv32(&a, i_pgidx as i32);
    i_pgidx += nb as u32;
    let mut i_term_off: u32 = t as u32;
    let mut i_off: u32 = i_term_off;
    if i_off > n {
        p.rc = FTS5_CORRUPT;
        return;
    }

    let mut found = false;
    loop {
        /* Descobre quantos bytes novos tem este termo */
        let mut o = i_off as i32;
        n_new = fast32(&a, &mut o) as u32;
        i_off = o as u32;
        if n_keep < n_match {
            break;
        }

        if n_keep == n_match {
            let n_cmp = n_new.min(n_term - n_match);
            let mut i = 0u32;
            while i < n_cmp {
                if at(&a, (i_off + i) as usize) != term[(n_match + i) as usize] {
                    break;
                }
                i += 1;
            }
            n_match += i;

            if n_term == n_match {
                if i == n_new {
                    found = true;
                }
                break;
            } else if i < n_new && at(&a, (i_off + i) as usize) > term[n_match as usize] {
                break;
            }
        }

        if i_pgidx >= n {
            b_end_of_page = true;
            break;
        }

        let (nb, delta) = gv32(&a, i_pgidx as i32);
        i_pgidx += nb as u32;
        i_term_off = i_term_off.wrapping_add(delta as u32);
        i_off = i_term_off;

        if i_off >= n {
            p.rc = FTS5_CORRUPT;
            return;
        }

        /* Lê o campo `n_keep` do termo seguinte. */
        let mut o = i_off as i32;
        n_keep = fast32(&a, &mut o) as u32;
        i_off = o as u32;
    }

    if !found {
        /* search_failed: */
        if !b_ge {
            it.p_leaf = None;
            return;
        } else if b_end_of_page {
            loop {
                fts5_seg_iter_next_page(p, db, cfg, it);
                let leaf = match it.p_leaf.as_ref() {
                    Some(l) => l,
                    None => return,
                };
                a = leaf.p.clone();
                if !fts5_leaf_is_termless(leaf) {
                    i_pgidx = leaf.sz_leaf as u32;
                    let (nb, o) = gv32(&a, i_pgidx as i32);
                    i_pgidx += nb as u32;
                    i_off = o as u32;
                    if i_off < 4 || i_off as i64 >= leaf.sz_leaf as i64 {
                        p.rc = FTS5_CORRUPT;
                        return;
                    } else {
                        n_keep = 0;
                        i_term_off = i_off;
                        n = leaf.nn as u32;
                        let (nb2, nn2) = gv32(&a, i_off as i32);
                        i_off += nb2 as u32;
                        n_new = nn2 as u32;
                        break;
                    }
                }
            }
        }
    }

    /* search_success: */
    if i_off as i64 + n_new as i64 > n as i64 || n_new < 1 {
        p.rc = FTS5_CORRUPT;
        return;
    }
    it.i_leaf_offset = (i_off + n_new) as i64;
    it.i_term_leaf_offset = it.i_leaf_offset as i32;
    it.i_term_leaf_pgno = it.i_leaf_pgno;

    it.term.set(&term[..(n_keep as usize).min(term.len())]);
    it.term.append_blob(sub(&a, i_off as i32, n_new as i32));

    let nn_now = it.p_leaf.as_ref().map_or(0, |l| l.nn);
    if i_pgidx >= n {
        it.i_endof_doclist = nn_now + 1;
    } else {
        let (nb, n_extra) = gv32(&a, i_pgidx as i32);
        i_pgidx += nb as u32;
        it.i_endof_doclist = (i_term_off as i32).wrapping_add(n_extra);
    }
    it.i_pgidx_off = i_pgidx as i32;

    fts5_seg_iter_load_rowid(p, db, cfg, it);
    fts5_seg_iter_load_npos(p, cfg, it);
}

/// `fts5IdxSelectStmt`.
fn fts5_idx_select_stmt(p: &mut Fts5Index, db: &mut Connection, cfg: &Fts5Config) -> Option<StmtId> {
    if p.p_idx_select.is_none() {
        let z_sql = mprintf(
            b"SELECT pgno FROM '%q'.'%q_idx' WHERE segid=? AND term<=? ORDER BY term DESC LIMIT 1",
            &db_name_args(cfg),
        );
        p.p_idx_select = fts5_index_prepare_stmt(p, db, z_sql);
    }
    p.p_idx_select
}

/// `fts5SegIterSeekInit`: posiciona o iterador em `term` dentro do segmento (ou no fim).
pub(crate) fn fts5_seg_iter_seek_init(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    term: &[u8],
    flags: i32,
    seg: &Fts5StructureSegment,
    it: &mut Fts5SegIter,
) {
    let mut i_pg: i32 = 1;
    let b_ge = flags & FTS5INDEX_QUERY_SCAN != 0;
    let mut b_dlidx = false;

    *it = Fts5SegIter::default();
    it.p_seg = Some(seg.clone());

    /* Este bloco põe em `i_pg` o número da folha que pode conter o termo, se ele existe no
    ** segmento. */
    let id = fts5_idx_select_stmt(p, db, cfg);
    if p.rc != SQLITE_OK {
        return;
    }
    if let Some(id) = id {
        bind_int(db, id, 1, seg.i_segid);
        bind_blob(db, id, 2, Some(term), term.len() as i32, StrDtor::Transient);
        if SQLITE_ROW == step(db, id) {
            let val = column_int(db, id, 0) as i64;
            i_pg = (val >> 1) as i32;
            b_dlidx = (val & 0x0001) != 0;
        }
        p.rc = reset(db, id);
        bind_null(db, id, 2);
    }

    if i_pg < seg.pgno_first {
        i_pg = seg.pgno_first;
        b_dlidx = false;
    }

    it.i_leaf_pgno = i_pg - 1;
    fts5_seg_iter_next_page(p, db, cfg, it);

    if it.p_leaf.is_some() {
        fts5_leaf_seek(p, db, cfg, b_ge, it, term);
    }

    if p.rc == SQLITE_OK && (!b_ge || flags & FTS5INDEX_QUERY_SCANONETERM != 0) {
        it.flags |= FTS5_SEGITER_ONETERM;
        if it.p_leaf.is_some() {
            if flags & FTS5INDEX_QUERY_DESC != 0 {
                it.flags |= FTS5_SEGITER_REVERSE;
            }
            if b_dlidx {
                fts5_seg_iter_load_dlidx(p, db, cfg, it);
            }
            if flags & FTS5INDEX_QUERY_DESC != 0 {
                fts5_seg_iter_reverse(p, db, cfg, it);
            }
        }
    }

    fts5_seg_iter_set_next(cfg, it);
    if flags & FTS5INDEX_QUERY_SCANONETERM == 0 {
        fts5_seg_iter_alloc_tombstone(it);
    }
}

/// `fts5IdxNextStmt`.
fn fts5_idx_next_stmt(p: &mut Fts5Index, db: &mut Connection, cfg: &Fts5Config) -> Option<StmtId> {
    if p.p_idx_next_select.is_none() {
        let z_sql = mprintf(
            b"SELECT pgno FROM '%q'.'%q_idx' WHERE segid=? AND term>? ORDER BY term ASC LIMIT 1",
            &db_name_args(cfg),
        );
        p.p_idx_next_select = fts5_index_prepare_stmt(p, db, z_sql);
    }
    p.p_idx_next_select
}

/// `fts5SegIterNextInit`: como `fts5_seg_iter_seek_init`, mas posiciona no primeiro termo depois
/// da página de `term`.
pub(crate) fn fts5_seg_iter_next_init(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    term: &[u8],
    seg: &Fts5StructureSegment,
    it: &mut Fts5SegIter,
) {
    let mut i_pg: i32 = -1;
    let mut b_dlidx = false;

    let sel = fts5_idx_next_stmt(p, db, cfg);
    if let Some(id) = sel {
        bind_int(db, id, 1, seg.i_segid);
        bind_blob(db, id, 2, Some(term), term.len() as i32, StrDtor::Transient);

        if step(db, id) == SQLITE_ROW {
            let val = column_int64(db, id, 0);
            i_pg = (val >> 1) as i32;
            b_dlidx = (val & 0x0001) != 0;
        }
        p.rc = reset(db, id);
        bind_null(db, id, 2);
        if p.rc != SQLITE_OK {
            return;
        }
    }

    *it = Fts5SegIter::default();
    it.p_seg = Some(seg.clone());
    it.flags |= FTS5_SEGITER_ONETERM;
    if i_pg >= 0 {
        it.i_leaf_pgno = i_pg - 1;
        fts5_seg_iter_next_page(p, db, cfg, it);
        fts5_seg_iter_set_next(cfg, it);
    }
    if it.p_leaf.is_some() {
        let (sz_leaf, i_term_off) = {
            let leaf = it.p_leaf.as_ref().unwrap();
            (leaf.sz_leaf, gv32(&leaf.p, leaf.sz_leaf))
        };
        it.i_pgidx_off = sz_leaf + i_term_off.0;
        it.i_leaf_offset = i_term_off.1 as i64;
        fts5_seg_iter_load_term(p, db, cfg, it, 0);
        fts5_seg_iter_load_npos(p, cfg, it);
        if b_dlidx {
            fts5_seg_iter_load_dlidx(p, db, cfg, it);
        }
    }
}

/// `fts5SegIterHashInit`: posiciona o iterador em `term` dentro do hash em memória (ou no fim). Com
/// `term` `None` ou com `FTS5INDEX_QUERY_SCAN`, varre os termos ordenados.
pub(crate) fn fts5_seg_iter_hash_init(
    p: &mut Fts5Index,
    cfg: &Fts5Config,
    term: Option<&[u8]>,
    flags: i32,
    it: &mut Fts5SegIter,
) {
    let mut leaf: Option<Fts5Data> = None;
    let mut z: Vec<u8> = Vec::new();

    if term.is_none() || flags & FTS5INDEX_QUERY_SCAN != 0 {
        let mut entry: Option<(Vec<u8>, Vec<u8>)> = None;
        if let Some(h) = p.p_hash.as_mut() {
            p.rc = h.scan_init(term.unwrap_or(&[]));
            entry = h.scan_entry().map(|(zz, l)| (zz.to_vec(), l.to_vec()));
        }
        if let Some((zz, list)) = entry {
            z = zz;
            leaf = Some(Fts5Data::doclist(list));
        }

        /* `scan_init` faz o hash preencher o campo de tamanho de todas as poslists existentes, o
        ** que impede acrescentar a elas. O único caso em que se acrescenta é quando a operação
        ** anterior na tabela foi um DELETE; zerando `b_delete` essa possibilidade some. */
        p.b_delete = 0;
    } else {
        let t = term.unwrap_or(&[]);
        if let Some(h) = p.p_hash.as_ref() {
            if let Some((buf, n_list)) = h.query(0, t) {
                let mut v = buf;
                v.truncate(n_list.max(0) as usize);
                leaf = Some(Fts5Data::doclist(v));
            }
        }
        z = t.to_vec();
        it.flags |= FTS5_SEGITER_ONETERM;
    }

    if let Some(l) = leaf {
        it.term.set(&z);
        let (n, v) = gv64(&l.p, 0);
        it.i_leaf_offset = n as i64;
        it.i_rowid = v as i64;
        it.i_endof_doclist = l.nn;
        it.p_leaf = Some(l);

        if flags & FTS5INDEX_QUERY_DESC != 0 {
            it.flags |= FTS5_SEGITER_REVERSE;
            fts5_seg_iter_reverse_init_page(p, cfg, it);
        } else {
            fts5_seg_iter_load_npos(p, cfg, it);
        }
    }

    fts5_seg_iter_set_next(cfg, it);
}

/// `fts5SegIterSetEOF`.
pub(crate) fn fts5_seg_iter_set_eof(it: &mut Fts5SegIter) {
    it.p_leaf = None;
}

/// `fts5SegIterGotoPage`: move o iterador para o primeiro rowid da página `i_leaf_pgno`.
fn fts5_seg_iter_goto_page(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5SegIter,
    i_leaf_pgno: i32,
) {
    let pgno_last = it.p_seg.as_ref().map_or(0, |s| s.pgno_last);
    if i_leaf_pgno > pgno_last {
        p.rc = FTS5_CORRUPT;
    } else {
        it.p_next_leaf = None;
        it.i_leaf_pgno = i_leaf_pgno - 1;

        while p.rc == SQLITE_OK {
            fts5_seg_iter_next_page(p, db, cfg, it);
            let leaf = match it.p_leaf.as_ref() {
                Some(l) => l,
                None => break,
            };
            let mut i_off = fts5_leaf_first_rowid_off(leaf);
            if i_off > 0 {
                let n = leaf.sz_leaf;
                if i_off < 4 || i_off >= n {
                    p.rc = FTS5_CORRUPT;
                } else {
                    let (nb, v) = gv64(&leaf.p, i_off);
                    it.i_rowid = v as i64;
                    i_off += nb;
                    it.i_leaf_offset = i_off as i64;
                    fts5_seg_iter_load_npos(p, cfg, it);
                }
                break;
            }
        }
    }
}

/// `fts5SegIterNextFrom`: avança o iterador até `i_match` ou além (sempre pelo menos uma vez).
pub(crate) fn fts5_seg_iter_next_from(
    p: &mut Fts5Index,
    db: &mut Connection,
    cfg: &Fts5Config,
    it: &mut Fts5SegIter,
    i_match: i64,
) {
    let b_rev = it.flags & FTS5_SEGITER_REVERSE != 0;
    let mut dl = match it.p_dlidx.take() {
        Some(d) => d,
        None => return,
    };
    let mut i_leaf_pgno = it.i_leaf_pgno;
    let mut b_move = true;

    if !b_rev {
        while !fts5_dlidx_iter_eof(p, &dl) && i_match > fts5_dlidx_iter_rowid(&dl) {
            i_leaf_pgno = fts5_dlidx_iter_pgno(&dl);
            fts5_dlidx_iter_next(p, db, cfg, &mut dl);
        }
        if i_leaf_pgno > it.i_leaf_pgno {
            fts5_seg_iter_goto_page(p, db, cfg, it, i_leaf_pgno);
            b_move = false;
        }
    } else {
        while !fts5_dlidx_iter_eof(p, &dl) && i_match < fts5_dlidx_iter_rowid(&dl) {
            fts5_dlidx_iter_prev(p, db, cfg, &mut dl);
        }
        i_leaf_pgno = fts5_dlidx_iter_pgno(&dl);

        if i_leaf_pgno < it.i_leaf_pgno {
            it.i_leaf_pgno = i_leaf_pgno + 1;
            fts5_seg_iter_reverse_new_page(p, db, cfg, it);
            b_move = false;
        }
    }
    it.p_dlidx = Some(dl);

    loop {
        if b_move && p.rc == SQLITE_OK {
            fts5_seg_iter_x_next(p, db, cfg, it, None);
        }
        if it.p_leaf.is_none() {
            break;
        }
        if !b_rev && it.i_rowid >= i_match {
            break;
        }
        if b_rev && it.i_rowid <= i_match {
            break;
        }
        b_move = true;
        if p.rc != SQLITE_OK {
            break;
        }
    }
}
